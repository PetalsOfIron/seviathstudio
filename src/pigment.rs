// SPDX-License-Identifier: GPL-3.0-or-later
// Pigment mixing: colors mix like paint, not like light. Blue and yellow make green with life in it,
// never the muddy gray that averaging RGB numbers gives.
//
// How: a color is turned into a reflectance spectrum (how much light each part of the rainbow it
// bounces back), found by our own smooth, non-negative fit to the color. Spectra of two paints are
// mixed the Kubelka-Munk way (the classic model of paint: absorption K and scattering S, mixed
// in proportion), and the mixed spectrum is turned back into screen color. Everything here is
// derived from public physics (CIE 1931 observer as the multi-lobe fit of Wyman, Sloan and
// Shirley, a 6500 K white, the sRGB matrix); it is not Mixbox, which is not free software.
//
// Glazing is the other real behavior: a thin transparent layer over dry paint filters the light
// that passes twice through it, so the colors multiply and the paint underneath gets deeper.

use std::sync::OnceLock;

use crate::blend::Px;
use crate::oklab::{to_linear, to_srgb};

/// Number of wavelength bands (405 … 675 nm).
pub const N: usize = 10;
pub type Spectrum = [f32; N];

const LAMBDA0: f64 = 405.0;
const STEP: f64 = 30.0;

fn lambda(i: usize) -> f64 {
    LAMBDA0 + STEP * i as f64
}

fn lobe(l: f64, mu: f64, s1: f64, s2: f64) -> f64 {
    let t = (l - mu) / if l < mu { s1 } else { s2 };
    (-0.5 * t * t).exp()
}

/// CIE 1931 color matching functions (multi-lobe fit).
fn cmf(l: f64) -> [f64; 3] {
    [
        1.056 * lobe(l, 599.8, 37.9, 31.0) + 0.362 * lobe(l, 442.0, 16.0, 26.7) - 0.065 * lobe(l, 501.1, 20.4, 26.2),
        0.821 * lobe(l, 568.8, 46.9, 40.5) + 0.286 * lobe(l, 530.9, 16.3, 31.1),
        1.217 * lobe(l, 437.0, 11.8, 36.0) + 0.681 * lobe(l, 459.0, 26.0, 13.8),
    ]
}

/// A 6504 K white light (Planck), close to the D65 white screens use.
fn white_light(l: f64) -> f64 {
    let (h, c, k, t) = (6.626_070_15e-34, 2.997_924_58e8, 1.380_649e-23, 6504.0);
    let m = l * 1e-9;
    1.0 / (m.powi(5) * ((h * c / (m * k * t)).exp() - 1.0))
}

type M3 = [[f64; 3]; 3];

const XYZ_TO_SRGB: M3 = [[3.2404542, -1.5371385, -0.4985314], [-0.9692660, 1.8760108, 0.0415560], [0.0556434, -0.2040259, 1.0572252]];

struct Model {
    /// Spectrum → linear sRGB (white balanced so a perfect white reflector gives 1, 1, 1).
    m: [[f64; N]; 3],
    /// Smoothest spectrum change for a given color change (N rows, 3 columns).
    p: [[f64; 3]; N],
    /// Broad bumps (blue, green, red) that add up to 1 everywhere: the starting guess.
    start: [[f64; 3]; N],
}

fn model() -> &'static Model {
    static M: OnceLock<Model> = OnceLock::new();
    M.get_or_init(build)
}

fn solve(mut a: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    // inverse by Gauss-Jordan, `a` is square
    let n = a.len();
    for (i, row) in a.iter_mut().enumerate() {
        row.extend((0..n).map(|j| if i == j { 1.0 } else { 0.0 }));
    }
    for c in 0..n {
        let piv = (c..n).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs())).unwrap_or(c);
        a.swap(c, piv);
        let d = a[c][c];
        for v in a[c].iter_mut() {
            *v /= d;
        }
        for r in 0..n {
            if r != c {
                let f = a[r][c];
                if f != 0.0 {
                    let src = a[c].clone();
                    for (v, s) in a[r].iter_mut().zip(src) {
                        *v -= f * s;
                    }
                }
            }
        }
    }
    a.into_iter().map(|r| r[n..].to_vec()).collect()
}

fn build() -> Model {
    // weights of each band for X, Y, Z under the white light
    let mut w = [[0f64; N]; 3];
    for i in 0..N {
        let (s, c) = (white_light(lambda(i)), cmf(lambda(i)));
        for k in 0..3 {
            w[k][i] = c[k] * s * STEP;
        }
    }
    let ysum: f64 = w[1].iter().sum();
    for row in w.iter_mut() {
        for v in row.iter_mut() {
            *v /= ysum;
        }
    }
    // XYZ → linear sRGB, then balance so white (all ones) is exactly white
    let mut m = [[0f64; N]; 3];
    for r in 0..3 {
        for i in 0..N {
            m[r][i] = (0..3).map(|k| XYZ_TO_SRGB[r][k] * w[k][i]).sum();
        }
    }
    for row in m.iter_mut() {
        let white: f64 = row.iter().sum();
        for v in row.iter_mut() {
            *v /= white;
        }
    }
    // smoothness: minimise the second differences; A = (DᵀD + εI)⁻¹
    let mut dtd = vec![vec![0f64; N]; N];
    for r in 1..N - 1 {
        let d = [(r - 1, 1.0), (r, -2.0), (r + 1, 1.0)];
        for &(i, a) in &d {
            for &(j, b) in &d {
                dtd[i][j] += a * b;
            }
        }
    }
    for (i, row) in dtd.iter_mut().enumerate() {
        row[i] += 1e-3;
    }
    let a = solve(dtd);
    // P = A Mᵀ (M A Mᵀ)⁻¹
    let am: Vec<[f64; 3]> = (0..N).map(|i| [0, 1, 2].map(|k| (0..N).map(|j| a[i][j] * m[k][j]).sum())).collect();
    let mut mam = vec![vec![0f64; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            mam[r][c] = (0..N).map(|j| m[r][j] * am[j][c]).sum();
        }
    }
    let inv = solve(mam);
    let mut p = [[0f64; 3]; N];
    for i in 0..N {
        for c in 0..3 {
            p[i][c] = (0..3).map(|k| am[i][k] * inv[k][c]).sum();
        }
    }
    // starting guess: three hat functions over the rainbow
    let mut start = [[0f64; 3]; N];
    for (i, row) in start.iter_mut().enumerate() {
        let l = lambda(i);
        let hat = |c: f64, w: f64| (1.0 - (l - c).abs() / w).max(0.0);
        let (b, g, r) = (hat(440.0, 130.0), hat(540.0, 110.0), hat(640.0, 130.0));
        let s = (b + g + r).max(1e-9);
        *row = [r / s, g / s, b / s];
    }
    Model { m, p, start }
}

fn rgb_lin(m: &Model, s: &Spectrum) -> [f64; 3] {
    [0, 1, 2].map(|k| (0..N).map(|i| m.m[k][i] * s[i] as f64).sum())
}

const MIN_R: f32 = 0.004;

/// The reflectance spectrum of a screen color (sRGB 0..1).
pub fn spectrum(rgb: [f32; 3]) -> Spectrum {
    let m = model();
    let target = [to_linear(rgb[0].clamp(0.0, 1.0)) as f64, to_linear(rgb[1].clamp(0.0, 1.0)) as f64, to_linear(rgb[2].clamp(0.0, 1.0)) as f64];
    let mut r = [0f64; N];
    for i in 0..N {
        r[i] = (m.start[i][0] * target[0] + m.start[i][1] * target[1] + m.start[i][2] * target[2]).clamp(MIN_R as f64, 1.0);
    }
    for _ in 0..8 {
        let now = [0, 1, 2].map(|k| (0..N).map(|i| m.m[k][i] * r[i]).sum::<f64>());
        let err = [target[0] - now[0], target[1] - now[1], target[2] - now[2]];
        if err.iter().all(|e| e.abs() < 0.0008) {
            break;
        }
        for i in 0..N {
            r[i] = (r[i] + m.p[i][0] * err[0] + m.p[i][1] * err[1] + m.p[i][2] * err[2]).clamp(MIN_R as f64, 1.0);
        }
    }
    r.map(|v| v as f32)
}

/// Screen color (sRGB 0..1) of a reflectance spectrum.
pub fn to_rgb(s: &Spectrum) -> [f32; 3] {
    let lin = rgb_lin(model(), s);
    [0, 1, 2].map(|k| to_srgb(lin[k].clamp(0.0, 1.0) as f32))
}

/// Kubelka-Munk: absorption over scattering for a reflectance of a thick layer.
fn ks_of(r: f32) -> f32 {
    let r = r.clamp(MIN_R, 1.0);
    (1.0 - r) * (1.0 - r) / (2.0 * r)
}

fn r_of(ks: f32) -> f32 {
    1.0 + ks - (ks * ks + 2.0 * ks).sqrt()
}

/// A paint: its spectrum kept as K/S so mixing is just a weighted sum.
#[derive(Clone, Copy, Debug)]
pub struct Pigment {
    ks: [f32; N],
    rgb: [f32; 3],
}

impl Pigment {
    pub fn new(rgb: [f32; 3]) -> Pigment {
        let spec = spectrum(rgb);
        Pigment { ks: spec.map(ks_of), rgb }
    }

    pub fn rgb(&self) -> [f32; 3] {
        self.rgb
    }

    /// Absorption over scattering per band (mixing is a weighted sum of these).
    pub fn ks(&self) -> [f32; N] {
        self.ks
    }
}

/// Screen color of a K/S spectrum (the result of mixing or spreading pigments).
pub fn rgb_from_ks(ks: &[f32; N]) -> [f32; 3] {
    to_rgb(&ks.map(r_of))
}

/// Mix paints in proportion to the weights (they don't have to add up to one).
pub fn mix_pigments(items: &[(&Pigment, f32)]) -> [f32; 3] {
    let total: f32 = items.iter().map(|i| i.1.max(0.0)).sum();
    if total <= 1e-6 {
        return items.first().map(|i| i.0.rgb).unwrap_or([0.0; 3]);
    }
    if items.len() == 1 {
        return items[0].0.rgb;
    }
    let mut s = [0f32; N];
    for (p, w) in items {
        let w = w.max(0.0) / total;
        for i in 0..N {
            s[i] += p.ks[i] * w;
        }
    }
    to_rgb(&s.map(r_of))
}

/// `a` mixed with `b`: t = how much of b (0..1).
pub fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    if t <= 0.0 || a == b {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    mix_pigments(&[(&Pigment::new(a), 1.0 - t), (&Pigment::new(b), t)])
}

/// A thin transparent layer of `paint` over `base`: the light passes through it twice, so the
/// colors multiply. `amount` 0..1 is how much paint there is.
pub fn glaze(base: [f32; 3], paint: [f32; 3], amount: f32) -> [f32; 3] {
    if amount <= 0.0 {
        return base;
    }
    let (b, p) = (spectrum(base), spectrum(paint));
    let mut out = [0f32; N];
    for i in 0..N {
        out[i] = (b[i] * p[i].powf(amount.min(1.0))).max(MIN_R);
    }
    to_rgb(&out)
}

fn unpremul(p: Px) -> [f32; 3] {
    if p[3] <= 0.0 { [0.0; 3] } else { [(p[0] / p[3]).clamp(0.0, 1.0), (p[1] / p[3]).clamp(0.0, 1.0), (p[2] / p[3]).clamp(0.0, 1.0)] }
}

/// Remembers the last colors, because neighbouring pixels are very often the same color.
#[derive(Default)]
pub struct Mixer {
    last: Option<([f32; 3], Pigment)>,
}

impl Mixer {
    pub fn pigment(&mut self, rgb: [f32; 3]) -> Pigment {
        if let Some((c, p)) = &self.last {
            if *c == rgb {
                return *p;
            }
        }
        let p = Pigment::new(rgb);
        self.last = Some((rgb, p));
        p
    }

    /// Premultiplied `a` and `b` mixed: `t` is the share of b. Colors mix as paint, coverage as coverage.
    pub fn mix_premult(&mut self, a: Px, b: Px, paint_b: &Pigment, t: f32) -> Px {
        let alpha = a[3] + (b[3] - a[3]) * t;
        if alpha <= 0.0 {
            return [0.0; 4];
        }
        let (wa, wb) = (a[3] * (1.0 - t), b[3] * t);
        let rgb = if wa <= 1e-6 {
            paint_b.rgb()
        } else if wb <= 1e-6 {
            unpremul(a)
        } else {
            let (ca, cb) = (unpremul(a), paint_b.rgb());
            if ca == cb {
                ca
            } else {
                let pa = self.pigment(ca);
                mix_pigments(&[(&pa, wa), (paint_b, wb)])
            }
        };
        [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
    }

    /// `paint` laid over `base` as a glaze with coverage `a` (0..1).
    pub fn glaze_over(&mut self, base: Px, paint: [f32; 3], a: f32) -> Px {
        let out_a = a + base[3] * (1.0 - a);
        if out_a <= 0.0 {
            return [0.0; 4];
        }
        let rgb = if base[3] <= 1e-6 {
            paint
        } else {
            let g = glaze(unpremul(base), paint, a);
            // where the base is only partly there, the paint also lands on bare paper
            let share = base[3].min(1.0);
            [paint[0] + (g[0] - paint[0]) * share, paint[1] + (g[1] - paint[1]) * share, paint[2] + (g[2] - paint[2]) * share]
        };
        [rgb[0] * out_a, rgb[1] * out_a, rgb[2] * out_a, out_a]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
    }

    fn chroma(c: [f32; 3]) -> f32 {
        let l = crate::oklab::rgb_to_oklab(c);
        l[1].hypot(l[2])
    }

    #[test]
    fn colors_survive_the_trip_through_a_spectrum() {
        for c in [[0.5, 0.5, 0.5], [0.9, 0.2, 0.2], [0.2, 0.6, 0.3], [0.15, 0.25, 0.8], [0.95, 0.85, 0.2], [0.85, 0.7, 0.6], [0.05, 0.05, 0.05], [1.0, 1.0, 1.0]] {
            let back = to_rgb(&spectrum(c));
            assert!(close(c, back, 0.03), "{c:?} came back as {back:?}");
        }
        // strongly saturated colors are at the edge of what paint can do, so allow more
        for c in [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]] {
            let back = to_rgb(&spectrum(c));
            assert!(close(c, back, 0.17), "{c:?} came back as {back:?}");
        }
        let s = spectrum([0.1, 0.9, 0.3]);
        assert!(s.iter().all(|v| *v > 0.0 && *v <= 1.0), "a real, non-negative spectrum: {s:?}");
        // and it is smooth: no wild jumps between neighbouring bands
        assert!(s.windows(2).all(|w| (w[0] - w[1]).abs() < 0.8), "{s:?}");
    }

    #[test]
    fn blue_and_yellow_make_a_lively_green_not_gray() {
        let (blue, yellow) = ([0.1, 0.2, 0.85], [0.98, 0.88, 0.1]);
        let paint = mix(blue, yellow, 0.5);
        assert!(paint[1] > paint[0] && paint[1] > paint[2], "it is green: {paint:?}");
        let rgb_avg = [(blue[0] + yellow[0]) / 2.0, (blue[1] + yellow[1]) / 2.0, (blue[2] + yellow[2]) / 2.0];
        assert!(chroma(paint) > chroma(rgb_avg) * 1.5, "paint {paint:?} (chroma {}) vs the RGB average {rgb_avg:?} (chroma {})", chroma(paint), chroma(rgb_avg));
    }

    #[test]
    fn red_and_yellow_make_orange_and_mixing_is_fair() {
        let (red, yellow) = ([0.9, 0.1, 0.1], [0.98, 0.88, 0.1]);
        let o = mix(red, yellow, 0.5);
        assert!(o[0] > 0.85 && o[1] > 0.15 && o[1] < 0.8 && o[2] < 0.3, "orange: {o:?}");
        assert!(close(mix(red, yellow, 0.0), red, 1e-6) && close(mix(red, yellow, 1.0), yellow, 1e-6));
        // more yellow, more yellow
        assert!(mix(red, yellow, 0.7)[1] > mix(red, yellow, 0.3)[1]);
        // the same color mixed with itself does not change
        let c = [0.3, 0.5, 0.4];
        assert!(close(mix(c, c, 0.5), c, 1e-6));
        // white lightens, black darkens, and neither goes muddy
        let l = |c: [f32; 3]| crate::oklab::rgb_to_oklab(c)[0];
        assert!(l(mix([0.8, 0.1, 0.1], [1.0, 1.0, 1.0], 0.5)) > l([0.8, 0.1, 0.1]));
        assert!(l(mix([0.8, 0.1, 0.1], [0.0, 0.0, 0.0], 0.3)) < l([0.8, 0.1, 0.1]));
    }

    #[test]
    fn a_glaze_deepens_what_is_below_it() {
        let (yellow, blue) = ([0.98, 0.88, 0.1], [0.1, 0.2, 0.85]);
        let g = glaze(yellow, blue, 1.0);
        assert!(g[1] > g[0] && g[1] > g[2] || (g[0] + g[1] + g[2]) < 1.2, "a darker, greenish result: {g:?}");
        let l = |c: [f32; 3]| crate::oklab::rgb_to_oklab(c)[0];
        assert!(l(g) < l(yellow), "darker than the base");
        assert!(close(glaze(yellow, blue, 0.0), yellow, 1e-6), "no paint, no change");
        assert!(l(glaze(yellow, blue, 0.3)) > l(g), "a thinner glaze changes less");
        // a glaze of white-ish paint barely changes the base
        let w = glaze(yellow, [0.99, 0.99, 0.99], 1.0);
        assert!(close(w, yellow, 0.06), "{w:?}");
    }

    #[test]
    fn premultiplied_mixing_keeps_coverage_and_clear_pixels() {
        let mut m = Mixer::default();
        let red = Pigment::new([0.9, 0.1, 0.1]);
        // over nothing: the paint as it is
        let out = m.mix_premult([0.0; 4], [0.9, 0.1, 0.1, 1.0], &red, 0.5);
        assert!((out[3] - 0.5).abs() < 1e-5 && (out[0] / out[3] - 0.9).abs() < 0.01, "{out:?}");
        // t = 1: exactly the new paint
        let yellow = Pigment::new([0.98, 0.88, 0.1]);
        let out = m.mix_premult([0.9, 0.1, 0.1, 1.0], [0.98, 0.88, 0.1, 1.0], &yellow, 1.0);
        assert!(close([out[0], out[1], out[2]], [0.98, 0.88, 0.1], 0.01) && out[3] == 1.0);
        // half and half on solid paint: orange, fully covered
        let out = m.mix_premult([0.9, 0.1, 0.1, 1.0], [0.98, 0.88, 0.1, 1.0], &yellow, 0.5);
        assert!(out[3] == 1.0 && out[0] > 0.85 && out[1] > 0.15 && out[1] < 0.8, "{out:?}");
        // a glaze over nothing is just the paint, over paint it is deeper
        let g = m.glaze_over([0.0; 4], [0.1, 0.2, 0.85], 1.0);
        assert!(close([g[0], g[1], g[2]], [0.1, 0.2, 0.85], 0.01));
        let g = m.glaze_over([0.98, 0.88, 0.1, 1.0], [0.1, 0.2, 0.85], 0.6);
        assert!(g[3] == 1.0 && g[0] < 0.98);
        assert_eq!(m.mix_premult([0.0; 4], [0.0; 4], &red, 0.5), [0.0; 4]);
    }

    #[test]
    fn mixing_is_quick_enough_for_a_brush_dab() {
        let t = std::time::Instant::now();
        let mut m = Mixer::default();
        let paint = Pigment::new([0.2, 0.4, 0.8]);
        let mut acc = 0.0;
        for i in 0..2000u32 {
            let c = [(i % 255) as f32 / 255.0, ((i * 7) % 255) as f32 / 255.0, ((i * 13) % 255) as f32 / 255.0];
            let out = m.mix_premult([c[0], c[1], c[2], 1.0], [0.2, 0.4, 0.8, 1.0], &paint, 0.4);
            acc += out[0];
        }
        assert!(acc > 0.0);
        let per = t.elapsed().as_secs_f64() / 2000.0;
        // generous: tests run in parallel on a loaded machine, and the real build is much faster
        assert!(per < 0.0003, "{per} s per pixel mix");
    }
}
