// Blend modes and compositing on premultiplied float pixels, following the W3C Compositing spec
// (the same math Photoshop-compatible apps use, so PSD export later looks the same).

use eframe::egui::Color32;

use crate::doc::Blend;

/// Premultiplied RGBA, 0..1.
pub type Px = [f32; 4];

pub const CLEAR: Px = [0.0; 4];

#[inline]
pub fn to_px(c: Color32) -> Px {
    const K: f32 = 1.0 / 255.0;
    [c.r() as f32 * K, c.g() as f32 * K, c.b() as f32 * K, c.a() as f32 * K]
}

#[inline]
pub fn from_px(p: Px) -> Color32 {
    let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    let a = q(p[3]);
    // Keep premultiplied invariants (color ≤ alpha) after rounding.
    Color32::from_rgba_premultiplied(q(p[0]).min(a), q(p[1]).min(a), q(p[2]).min(a), a)
}

/// Blend `src` over `dst` with `mode` and an extra opacity on the source.
#[inline]
pub fn over(dst: Px, src: Px, opacity: f32, mode: Blend) -> Px {
    let sa = src[3] * opacity;
    if sa <= 0.0 {
        return dst;
    }
    let da = dst[3];
    if mode == Blend::Normal || da <= 0.0 {
        let k = 1.0 - sa;
        return [src[0] * opacity + dst[0] * k, src[1] * opacity + dst[1] * k, src[2] * opacity + dst[2] * k, sa + da * k];
    }
    let cs = unpremul(src);
    let cb = unpremul(dst);
    let b = mix(mode, cb, cs);
    let mut out = [0.0; 4];
    for i in 0..3 {
        // co = cs·(1−ab) + cb·(1−as) + as·ab·B(Cb, Cs)   (premultiplied)
        out[i] = sa * cs[i] * (1.0 - da) + da * cb[i] * (1.0 - sa) + sa * da * b[i];
    }
    out[3] = sa + da * (1.0 - sa);
    out
}

/// Like `over`, but the result keeps `dst`'s alpha: used for clipping masks
/// (a clipped layer only shows where the layer it is clipped to has paint).
#[inline]
pub fn atop(dst: Px, src: Px, opacity: f32, mode: Blend) -> Px {
    let sa = src[3] * opacity;
    let da = dst[3];
    if sa <= 0.0 || da <= 0.0 {
        return dst;
    }
    let cs = unpremul(src);
    let cb = unpremul(dst);
    let b = if mode == Blend::Normal { cs } else { mix(mode, cb, cs) };
    let mut out = [0.0; 4];
    for i in 0..3 {
        out[i] = sa * da * b[i] + da * cb[i] * (1.0 - sa);
    }
    out[3] = da;
    out
}

/// Linear mix between two premultiplied pixels (pass-through folder opacity).
#[inline]
pub fn lerp(a: Px, b: Px, t: f32) -> Px {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

#[inline]
fn unpremul(p: Px) -> [f32; 3] {
    if p[3] <= 0.0 {
        [0.0; 3]
    } else {
        [(p[0] / p[3]).min(1.0), (p[1] / p[3]).min(1.0), (p[2] / p[3]).min(1.0)]
    }
}

/// B(Cb, Cs): the blend function on straight (unpremultiplied) colors.
fn mix(mode: Blend, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    let sep = |f: fn(f32, f32) -> f32| [f(b[0], s[0]), f(b[1], s[1]), f(b[2], s[2])];
    match mode {
        Blend::Normal | Blend::PassThrough => s,
        Blend::Multiply => sep(|b, s| b * s),
        Blend::Screen => sep(screen),
        Blend::Overlay => sep(|b, s| hard_light(s, b)),
        Blend::Darken => sep(f32::min),
        Blend::Lighten => sep(f32::max),
        Blend::ColorDodge => sep(|b, s| {
            if b <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }),
        Blend::ColorBurn => sep(|b, s| {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }),
        Blend::HardLight => sep(hard_light),
        Blend::SoftLight => sep(|b, s| {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }),
        Blend::Difference => sep(|b, s| (b - s).abs()),
        Blend::Exclusion => sep(|b, s| b + s - 2.0 * b * s),
        Blend::Add => sep(|b, s| (b + s).min(1.0)),
        Blend::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        Blend::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        Blend::Color => set_lum(s, lum(b)),
        Blend::Luminosity => set_lum(b, lum(s)),
    }
}

fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 { b * 2.0 * s } else { screen(b, 2.0 * s - 1.0) }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        for v in &mut c {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 {
        for v in &mut c {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].partial_cmp(&c[b]).unwrap_or(std::cmp::Ordering::Equal));
    let (lo, mid, hi) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Px, b: Px) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    const GRAY: Px = [0.5, 0.5, 0.5, 1.0];
    const WHITE: Px = [1.0, 1.0, 1.0, 1.0];
    const BLACK: Px = [0.0, 0.0, 0.0, 1.0];

    #[test]
    fn normal_half_opacity() {
        assert!(close(over(WHITE, BLACK, 0.5, Blend::Normal), [0.5, 0.5, 0.5, 1.0]));
    }

    #[test]
    fn separable_modes_on_opaque() {
        assert!(close(over(GRAY, GRAY, 1.0, Blend::Multiply), [0.25, 0.25, 0.25, 1.0]));
        assert!(close(over(GRAY, GRAY, 1.0, Blend::Screen), [0.75, 0.75, 0.75, 1.0]));
        assert!(close(over(GRAY, WHITE, 1.0, Blend::Difference), [0.5, 0.5, 0.5, 1.0]));
        assert!(close(over(GRAY, GRAY, 1.0, Blend::Add), WHITE));
        // Overlay with white backdrop stays white; with mid-gray source it's neutral.
        assert!(close(over(WHITE, GRAY, 1.0, Blend::Overlay), WHITE));
        assert!(close(over(GRAY, GRAY, 1.0, Blend::SoftLight), GRAY));
    }

    #[test]
    fn blend_onto_transparent_is_plain_source() {
        let red = [1.0, 0.0, 0.0, 1.0];
        for m in Blend::ALL {
            assert!(close(over(CLEAR, red, 1.0, m), red), "{m:?}");
        }
    }

    #[test]
    fn color_mode_keeps_backdrop_luminosity() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let out = over(GRAY, red, 1.0, Blend::Color);
        let l = lum([out[0], out[1], out[2]]);
        assert!((l - 0.5).abs() < 1e-3 && out[0] > out[1], "{out:?}");
    }

    #[test]
    fn atop_keeps_base_alpha() {
        let base = [0.5, 0.0, 0.0, 0.5]; // half-transparent red
        let out = atop(base, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Normal);
        assert!(close(out, [0.0, 0.0, 0.5, 0.5]));
        assert!(close(atop(CLEAR, WHITE, 1.0, Blend::Normal), CLEAR));
    }
}
