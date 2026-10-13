// SPDX-License-Identifier: GPL-3.0-or-later
// Repeatable noise and random numbers, so procedural tips and textures look the same every time.
// Same functions as the web version (brush.ts), so brushes behave alike in both.

/// Small fast random number generator (LCG), seeded.
#[derive(Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(2654435761).wrapping_add(0x9E37_79B9) | 1)
    }
    /// 0..1
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / 16_777_216.0
    }
    /// −1..1
    pub fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

pub fn hash(x: i32, y: i32, seed: i32) -> f32 {
    let mut h = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(seed.wrapping_mul(2147483647));
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1274126177);
    ((h ^ ((h as u32) >> 16) as i32) as u32) as f32 / 4_294_967_295.0
}

pub fn noise(x: f32, y: f32, seed: i32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1, yi, seed);
    let c = hash(xi, yi + 1, seed);
    let d = hash(xi + 1, yi + 1, seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

pub fn fbm(x: f32, y: f32, seed: i32) -> f32 {
    noise(x, y, seed) * 0.55 + noise(x * 2.1, y * 2.1, seed + 11) * 0.3 + noise(x * 4.3, y * 4.3, seed + 23) * 0.15
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Value noise that repeats every `p` cells, so textures tile without seams.
pub fn pnoise(x: f32, y: f32, p: i32, seed: i32) -> f32 {
    let (xf, yf) = (x.floor(), y.floor());
    let (fx, fy) = (x - xf, y - yf);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let w = |v: i32| v.rem_euclid(p.max(1));
    let (xi, yi) = (xf as i32, yf as i32);
    let a = hash(w(xi), w(yi), seed);
    let b = hash(w(xi + 1), w(yi), seed);
    let c = hash(w(xi), w(yi + 1), seed);
    let d = hash(w(xi + 1), w(yi + 1), seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

/// Fractal tiling noise; `cells` = noise cells across the tile.
pub fn pfbm(u: f32, v: f32, cells: i32, seed: i32, oct: i32) -> f32 {
    let (mut sum, mut amp, mut tot, mut f) = (0.0, 0.55, 0.0, 1);
    for o in 0..oct {
        let c = cells * f;
        sum += pnoise(u * c as f32, v * c as f32, c, seed + o * 13) * amp;
        tot += amp;
        amp *= 0.5;
        f *= 2;
    }
    sum / tot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_in_range_and_repeatable() {
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 1.13);
            let n = noise(x, y, 5);
            assert!((0.0..=1.0).contains(&n));
            assert_eq!(n, noise(x, y, 5));
        }
        let mut r = Rng::new(3);
        assert!((0..1000).map(|_| r.next()).all(|v| (0.0..1.0).contains(&v)));
    }

    #[test]
    fn periodic_noise_tiles() {
        let p = 8;
        for i in 0..20 {
            let y = i as f32 * 0.31;
            assert!((pnoise(0.0, y, p, 1) - pnoise(p as f32, y, p, 1)).abs() < 1e-5);
        }
    }
}
