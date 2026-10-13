// SPDX-License-Identifier: GPL-3.0-or-later
// A tiny anti-aliased painter for one-channel masks (brush tips): filled paths, round lines,
// circles, radial gradients and blur. Coordinates are in mask pixels.

use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// White paint on top (source-over).
    Over,
    /// Removes paint (destination-out).
    Out,
}

#[derive(Clone)]
pub struct Mask {
    pub n: usize,
    pub a: Vec<f32>,
}

impl Mask {
    pub fn new(n: usize) -> Self {
        Self { n, a: vec![0.0; n * n] }
    }

    #[inline]
    fn put(&mut self, i: usize, cov: f32, mode: Mode) {
        let c = cov.clamp(0.0, 1.0);
        let v = &mut self.a[i];
        match mode {
            Mode::Over => *v = 1.0 - (1.0 - *v) * (1.0 - c),
            Mode::Out => *v *= 1.0 - c,
        }
    }

    /// Fill closed polygons (even-odd), anti-aliased with 4 sub-rows per pixel.
    pub fn fill(&mut self, path: &Path, alpha: f32, mode: Mode) {
        let n = self.n;
        const SS: usize = 4;
        let mut row = vec![0.0f32; n + 1];
        let mut xs: Vec<f32> = Vec::new();
        let (y_min, y_max) = path.y_range();
        let y0 = (y_min.floor().max(0.0)) as usize;
        let y1 = (y_max.ceil().min(n as f32)) as usize;
        for y in y0..y1 {
            row.iter_mut().for_each(|v| *v = 0.0);
            let mut any = false;
            for k in 0..SS {
                let sy = y as f32 + (k as f32 + 0.5) / SS as f32;
                xs.clear();
                for poly in &path.polys {
                    let m = poly.len();
                    for i in 0..m {
                        let (ax, ay) = poly[i];
                        let (bx, by) = poly[(i + 1) % m];
                        if (ay <= sy && by > sy) || (by <= sy && ay > sy) {
                            xs.push(ax + (sy - ay) / (by - ay) * (bx - ax));
                        }
                    }
                }
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                for pair in xs.chunks(2) {
                    if pair.len() < 2 {
                        break;
                    }
                    let (a, b) = (pair[0].max(0.0), pair[1].min(n as f32));
                    if b <= a {
                        continue;
                    }
                    any = true;
                    let (ia, ib) = (a.floor() as usize, (b.ceil() as usize).min(n));
                    for x in ia..ib {
                        let left = (x as f32).max(a);
                        let right = ((x + 1) as f32).min(b);
                        if right > left {
                            row[x] += (right - left) / SS as f32;
                        }
                    }
                }
            }
            if any {
                for x in 0..n {
                    if row[x] > 0.0 {
                        self.put(y * n + x, row[x] * alpha, mode);
                    }
                }
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, alpha: f32, mode: Mode) {
        self.each_near(cx, cy, r + 1.0, mode, |x, y| {
            let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            (r - d + 0.5).clamp(0.0, 1.0) * alpha
        });
    }

    /// A round-capped line from `a` to `b`, width going from `wa` to `wb`.
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), wa: f32, wb: f32, alpha: f32, mode: Mode) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let r = wa.max(wb) / 2.0 + 1.0;
        let (minx, maxx) = (a.0.min(b.0) - r, a.0.max(b.0) + r);
        let (miny, maxy) = (a.1.min(b.1) - r, a.1.max(b.1) + r);
        let n = self.n as i32;
        for y in (miny.floor() as i32).max(0)..(maxy.ceil() as i32).min(n) {
            for x in (minx.floor() as i32).max(0)..(maxx.ceil() as i32).min(n) {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
                let (qx, qy) = (a.0 + dx * t, a.1 + dy * t);
                let d = ((px - qx).powi(2) + (py - qy).powi(2)).sqrt();
                let half = (wa + (wb - wa) * t) / 2.0;
                let c = (half - d + 0.5).clamp(0.0, 1.0);
                if c > 0.0 {
                    self.put(y as usize * self.n + x as usize, c * alpha, mode);
                }
            }
        }
    }

    /// Radial gradient around (cx, cy) with radius r; `stops` = (t, alpha), t in 0..1.
    pub fn radial(&mut self, cx: f32, cy: f32, r: f32, stops: &[(f32, f32)], mode: Mode) {
        self.each_near(cx, cy, r, mode, |x, y| {
            let t = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / r;
            gradient(stops, t)
        });
    }

    fn each_near(&mut self, cx: f32, cy: f32, r: f32, mode: Mode, f: impl Fn(f32, f32) -> f32) {
        let n = self.n as i32;
        for y in ((cy - r).floor() as i32).max(0)..((cy + r).ceil() as i32 + 1).min(n) {
            for x in ((cx - r).floor() as i32).max(0)..((cx + r).ceil() as i32 + 1).min(n) {
                let c = f(x as f32 + 0.5, y as f32 + 0.5);
                if c > 0.0 {
                    self.put(y as usize * self.n + x as usize, c, mode);
                }
            }
        }
    }

    /// Blur like CSS `blur(sigma)` in the web version (sigma = standard deviation in pixels).
    pub fn blur_css(&mut self, sigma: f32) {
        blur(&mut self.a, self.n, self.n, sigma * 2.0);
    }

    /// Scaled copy (box-filtered when shrinking).
    pub fn resized(&self, m: usize) -> Mask {
        let mut out = Mask::new(m);
        let k = self.n as f32 / m as f32;
        let s = k.ceil().max(1.0) as usize;
        for y in 0..m {
            for x in 0..m {
                let (sx, sy) = ((x as f32 * k) as usize, (y as f32 * k) as usize);
                let mut sum = 0.0;
                let mut cnt = 0.0;
                for j in 0..s {
                    for i in 0..s {
                        let (u, v) = (sx + i, sy + j);
                        if u < self.n && v < self.n {
                            sum += self.a[v * self.n + u];
                            cnt += 1.0;
                        }
                    }
                }
                out.a[y * m + x] = if cnt > 0.0 { sum / cnt } else { 0.0 };
            }
        }
        out
    }
}

/// Piecewise-linear gradient lookup.
pub fn gradient(stops: &[(f32, f32)], t: f32) -> f32 {
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for w in stops.windows(2) {
        let ((t0, a0), (t1, a1)) = (w[0], w[1]);
        if t <= t1 {
            let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            return a0 + (a1 - a0) * k;
        }
    }
    0.0 // past the last stop: nothing
}

/// Three box blurs (≈ Gaussian with the given radius), zero outside the w×h area.
pub fn blur(a: &mut [f32], w: usize, h: usize, radius: f32) {
    if radius < 0.3 {
        return;
    }
    // box size for 3 passes approximating a Gaussian with sigma ≈ radius/2
    let sigma = radius / 2.0;
    let box_r = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    let mut tmp = vec![0.0f32; a.len()];
    for _ in 0..3 {
        box_h(a, &mut tmp, w, h, box_r);
        box_v(&tmp, a, w, h, box_r);
    }
}

fn box_h(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut acc: f32 = row.iter().take(r + 1).sum();
        for x in 0..w {
            dst[y * w + x] = acc * norm;
            if x + r + 1 < w {
                acc += row[x + r + 1];
            }
            if x >= r {
                acc -= row[x - r];
            }
        }
    }
}

fn box_v(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for x in 0..w {
        let mut acc: f32 = (0..=r.min(h - 1)).map(|y| src[y * w + x]).sum();
        for y in 0..h {
            dst[y * w + x] = acc * norm;
            if y + r + 1 < h {
                acc += src[(y + r + 1) * w + x];
            }
            if y >= r {
                acc -= src[(y - r) * w + x];
            }
        }
    }
}

/// Path made of closed polygons (curves are flattened as they are added).
#[derive(Default, Clone)]
pub struct Path {
    pub polys: Vec<Vec<(f32, f32)>>,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.polys.push(vec![(x, y)]);
        self
    }
    fn cur(&mut self) -> &mut Vec<(f32, f32)> {
        if self.polys.is_empty() {
            self.polys.push(Vec::new());
        }
        self.polys.last_mut().unwrap()
    }
    fn last(&self) -> (f32, f32) {
        self.polys.last().and_then(|p| p.last().copied()).unwrap_or((0.0, 0.0))
    }
    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.cur().push((x, y));
        self
    }
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut Self {
        let (ax, ay) = self.last();
        for i in 1..=16 {
            let t = i as f32 / 16.0;
            let u = 1.0 - t;
            self.cur().push((u * u * ax + 2.0 * u * t * cx + t * t * x, u * u * ay + 2.0 * u * t * cy + t * t * y));
        }
        self
    }
    pub fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) -> &mut Self {
        let (ax, ay) = self.last();
        for i in 1..=24 {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.cur().push((a * ax + b * c1x + c * c2x + d * x, a * ay + b * c1y + c * c2y + d * y));
        }
        self
    }
    /// Arc around (cx, cy), angles in radians (clockwise on screen), appended to the current polygon.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> &mut Self {
        let steps = (((a1 - a0).abs() / TAU) * 64.0).ceil().max(4.0) as usize;
        for i in 0..=steps {
            let a = a0 + (a1 - a0) * i as f32 / steps as f32;
            self.cur().push((cx + a.cos() * r, cy + a.sin() * r));
        }
        self
    }
    pub fn circle(cx: f32, cy: f32, r: f32) -> Path {
        let mut p = Path::new();
        p.polys.push(Vec::new());
        p.arc(cx, cy, r, 0.0, TAU);
        p
    }
    pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Path {
        Path { polys: vec![vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]] }
    }
    pub fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
        use std::f32::consts::{FRAC_PI_2, PI};
        let mut p = Path::new();
        p.polys.push(Vec::new());
        p.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
        p.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
        p.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
        p.arc(x + r, y + r, r, PI, PI + FRAC_PI_2);
        p
    }
    fn y_range(&self) -> (f32, f32) {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for p in &self.polys {
            for &(_, y) in p {
                lo = lo.min(y);
                hi = hi.max(y);
            }
        }
        (lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_circle_area() {
        let mut m = Mask::new(64);
        m.fill(&Path::circle(32.0, 32.0, 20.0), 1.0, Mode::Over);
        let area: f32 = m.a.iter().sum();
        let expect = std::f32::consts::PI * 400.0;
        assert!((area - expect).abs() / expect < 0.02, "{area} vs {expect}");
        assert!(m.a[32 * 64 + 32] > 0.99 && m.a[0] == 0.0);
    }

    #[test]
    fn blur_keeps_total_roughly() {
        let mut m = Mask::new(64);
        m.circle(32.0, 32.0, 8.0, 1.0, Mode::Over);
        let before: f32 = m.a.iter().sum();
        m.blur_css(2.0);
        let after: f32 = m.a.iter().sum();
        assert!((before - after).abs() / before < 0.05);
        assert!(m.a[32 * 64 + 32] < 1.0);
    }
}
