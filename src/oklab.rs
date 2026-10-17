// SPDX-License-Identifier: GPL-3.0-or-later
// OKLab / OKLCh color math (Björn Ottosson's OKLab, public domain). Mixing colors here keeps
// the middle of a blend clean: blue to yellow goes through green, not gray.

/// sRGB 0..1 → linear light.
#[inline]
pub fn to_linear(c: f32) -> f32 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// linear light → sRGB 0..1.
#[inline]
pub fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB 0..1 → OKLab [L, a, b].
pub fn rgb_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (to_linear(rgb[0]), to_linear(rgb[1]), to_linear(rgb[2]));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab → linear sRGB (may be outside 0..1 for colors outside the gamut).
pub fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
    let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
    let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// OKLab → sRGB 0..1, clamped.
pub fn oklab_to_rgb(lab: [f32; 3]) -> [f32; 3] {
    let lin = oklab_to_linear(lab);
    [to_srgb(lin[0].clamp(0.0, 1.0)), to_srgb(lin[1].clamp(0.0, 1.0)), to_srgb(lin[2].clamp(0.0, 1.0))]
}

/// OKLab → polar [L, chroma, hue in degrees 0..360).
pub fn lab_to_lch(lab: [f32; 3]) -> [f32; 3] {
    let h = lab[2].atan2(lab[1]).to_degrees();
    [lab[0], (lab[1] * lab[1] + lab[2] * lab[2]).sqrt(), if h < 0.0 { h + 360.0 } else { h }]
}

pub fn lch_to_lab(lch: [f32; 3]) -> [f32; 3] {
    let h = lch[2].to_radians();
    [lch[0], lch[1] * h.cos(), lch[1] * h.sin()]
}

fn in_gamut(lin: [f32; 3]) -> bool {
    lin.iter().all(|v| (-0.0005..=1.0005).contains(v))
}

/// OKLCh → sRGB 0..1. A color outside what screens can show keeps its lightness and hue and
/// loses chroma until it fits.
pub fn lch_to_rgb_fit(lch: [f32; 3]) -> [f32; 3] {
    let [l, c, h] = lch;
    let l = l.clamp(0.0, 1.0);
    if in_gamut(oklab_to_linear(lch_to_lab([l, c, h]))) {
        return oklab_to_rgb(lch_to_lab([l, c, h]));
    }
    let (mut lo, mut hi) = (0.0f32, c);
    for _ in 0..18 {
        let mid = (lo + hi) / 2.0;
        if in_gamut(oklab_to_linear(lch_to_lab([l, mid, h]))) { lo = mid } else { hi = mid }
    }
    oklab_to_rgb(lch_to_lab([l, lo, h]))
}

pub fn rgb8_to_oklab(c: [u8; 3]) -> [f32; 3] {
    rgb_to_oklab([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0])
}

pub fn rgb_to_u8(c: [f32; 3]) -> [u8; 3] {
    [(c[0] * 255.0).round().clamp(0.0, 255.0) as u8, (c[1] * 255.0).round().clamp(0.0, 255.0) as u8, (c[2] * 255.0).round().clamp(0.0, 255.0) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for c in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.8, 0.2, 0.4], [0.1, 0.9, 0.3], [0.5, 0.5, 0.5]] {
            let back = oklab_to_rgb(rgb_to_oklab(c));
            for k in 0..3 {
                assert!((back[k] - c[k]).abs() < 0.002, "{c:?} -> {back:?}");
            }
        }
        // white has L = 1, black L = 0
        assert!((rgb_to_oklab([1.0, 1.0, 1.0])[0] - 1.0).abs() < 1e-3);
        assert!(rgb_to_oklab([0.0; 3])[0].abs() < 1e-3);
    }

    #[test]
    fn blue_to_yellow_middle_is_not_gray() {
        let a = rgb_to_oklab([0.0, 0.0, 1.0]);
        let b = rgb_to_oklab([1.0, 1.0, 0.0]);
        let mid = oklab_to_rgb([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]);
        let rgb_mid = [0.5f32, 0.5, 0.5];
        // plain RGB averaging gives exactly gray; OKLab stays colorful
        let spread = |c: [f32; 3]| c.iter().cloned().fold(0.0f32, f32::max) - c.iter().cloned().fold(1.0f32, f32::min);
        assert!(spread(mid) > 0.2 && spread(rgb_mid) < 0.01, "{mid:?}");
    }

    #[test]
    fn out_of_gamut_loses_chroma_only() {
        let c = lch_to_rgb_fit([0.7, 0.4, 150.0]); // far too colorful
        assert!(c.iter().all(|v| (0.0..=1.0).contains(v)));
        let back = lab_to_lch(rgb_to_oklab(c));
        assert!((back[0] - 0.7).abs() < 0.03, "lightness kept: {back:?}");
        assert!((back[2] - 150.0).abs() < 6.0, "hue kept: {back:?}");
    }
}
