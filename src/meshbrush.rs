// SPDX-License-Identifier: GPL-3.0-or-later
// Brushes that draw geometry on a mesh: a stroke over the surface becomes a hair strand (a thin tube), a hair card (a flat tapered ribbon), a rope, a strip of
// cloth, or a spike. The stroke is a line of places on the surface with the way the surface faces there; the brush gives it a width, a taper, a lift off the
// surface and a droop (hair falls). The new points get texture coordinates in a cell of their own (u across, v from the root to the tip), so a colour that
// runs from the root to the tip can be painted in that cell. This file knows nothing about the screen: it turns a line of places into points and triangles.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A thin round strand of hair.
    Strand,
    /// A flat card of hair that gets thinner towards its end.
    Card,
    /// A round rope that is as thick all along.
    Rope,
    /// A flat strip that is as wide all along (cloth, a band).
    Strip,
    /// A cone from the start of the stroke to its end, standing out of the surface.
    Spike,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Strand, Kind::Card, Kind::Rope, Kind::Strip, Kind::Spike];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Strand => "Hair strand",
            Kind::Card => "Hair card",
            Kind::Rope => "Rope",
            Kind::Strip => "Strip",
            Kind::Spike => "Spike",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Kind::Strand => "A thin round strand: drag from the root over the head, the way the hair lies",
            Kind::Card => "A flat card of hair, thinner at the end: cheap for a game, and good for locks",
            Kind::Rope => "A round rope of the same thickness all along: a braid, a tail, a tentacle",
            Kind::Strip => "A flat strip of the same width: cloth, a band, a ribbon",
            Kind::Spike => "A cone from where you start to where you end, standing out of the surface: a horn, a thorn",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub kind: Kind,
    /// Width (diameter) in metres.
    pub width: f32,
    /// How much thinner the end is than the start (0 is the same, 1 is a point).
    pub taper: f32,
    /// How far the stroke is lifted off the surface, in metres.
    pub lift: f32,
    /// How far the end of the stroke falls (metres, down).
    pub droop: f32,
    pub root: [u8; 3],
    pub tip: [u8; 3],
}

impl Default for Brush {
    fn default() -> Self {
        Brush { kind: Kind::Card, width: 0.03, taper: 0.6, lift: 0.004, droop: 0.0, root: [60, 40, 35], tip: [150, 110, 80] }
    }
}

impl Brush {
    /// The usual numbers of a kind of brush (the colours stay).
    pub fn with_kind(mut self, k: Kind) -> Brush {
        self.kind = k;
        let (width, taper, lift, droop) = match k {
            Kind::Strand => (0.006, 0.7, 0.003, 0.0),
            Kind::Card => (0.03, 0.6, 0.004, 0.0),
            Kind::Rope => (0.02, 0.0, 0.01, 0.0),
            Kind::Strip => (0.04, 0.0, 0.004, 0.0),
            Kind::Spike => (0.03, 1.0, 0.0, 0.0),
        };
        (self.width, self.taper, self.lift, self.droop) = (width, taper, lift, droop);
        self
    }
}

/// A place of the stroke: the place on the surface and the way the surface faces there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub p: [f32; 3],
    pub n: [f32; 3],
}

/// The new geometry. The texture coordinates are inside one cell: u from 0 to 1 across the stroke, v from 0 at the root to 1 at the tip.
#[derive(Clone, Debug, Default)]
pub struct Built {
    pub verts: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = len(a);
    if l < 1e-9 { [0.0, 1.0, 0.0] } else { mul(a, 1.0 / l) }
}

/// The stroke as evenly spaced spots: smoothed a little, and cut up by length.
fn resample(spots: &[Spot], spacing: f32, max: usize) -> Vec<Spot> {
    if spots.len() < 2 {
        return spots.to_vec();
    }
    // smooth the places (the hand shakes)
    let mut s: Vec<Spot> = spots.to_vec();
    for _ in 0..2 {
        let old = s.clone();
        for i in 1..s.len() - 1 {
            s[i].p = mul(add(add(old[i - 1].p, old[i + 1].p), mul(old[i].p, 2.0)), 0.25);
        }
    }
    let mut along = vec![0.0f32];
    for i in 1..s.len() {
        along.push(along[i - 1] + len(sub(s[i].p, s[i - 1].p)));
    }
    let total = *along.last().unwrap();
    if total < 1e-6 {
        return vec![s[0]];
    }
    let n = ((total / spacing.max(1e-4)).ceil() as usize + 1).clamp(2, max);
    (0..n)
        .map(|k| {
            let d = total * k as f32 / (n - 1) as f32;
            let j = along.partition_point(|a| *a < d).clamp(1, s.len() - 1);
            let t = ((d - along[j - 1]) / (along[j] - along[j - 1]).max(1e-9)).clamp(0.0, 1.0);
            Spot { p: add(mul(s[j - 1].p, 1.0 - t), mul(s[j].p, t)), n: unit(add(mul(s[j - 1].n, 1.0 - t), mul(s[j].n, t))) }
        })
        .collect()
}

/// The line the stroke is built along: evenly spaced, lifted off the surface, with the droop at the end (a spike is a straight line standing out).
fn prepared(b: &Brush, stroke: &[Spot]) -> Option<Vec<Spot>> {
    if stroke.len() < 2 {
        return None;
    }
    let mut path = if b.kind == Kind::Spike {
        // a straight cone from the start to the end, standing out of the surface
        let (a, z) = (stroke[0], stroke[stroke.len() - 1]);
        let reach = len(sub(z.p, a.p));
        if reach < 1e-4 {
            return None;
        }
        let tip = add(z.p, mul(a.n, reach * 0.6));
        (0..5).map(|k| { let t = k as f32 / 4.0; Spot { p: add(mul(a.p, 1.0 - t), mul(tip, t)), n: a.n } }).collect::<Vec<_>>()
    } else {
        resample(stroke, (b.width * 0.8).max(0.004), 60)
    };
    if path.len() < 2 {
        return None;
    }
    let count = path.len();
    // lift off the surface, and the droop towards the end
    for (i, s) in path.iter_mut().enumerate() {
        let t = i as f32 / (count - 1) as f32;
        s.p = add(add(s.p, mul(s.n, b.lift)), [0.0, -b.droop * t * t, 0.0]);
    }
    Some(path)
}

/// The joints of a chain of bones along the stroke: `pieces + 1` places, evenly spaced along the line the stroke is built along (the root first).
pub fn joints(b: &Brush, stroke: &[Spot], pieces: usize) -> Option<Vec<[f32; 3]>> {
    let path = prepared(b, stroke)?;
    let mut along = vec![0.0f32];
    for i in 1..path.len() {
        along.push(along[i - 1] + len(sub(path[i].p, path[i - 1].p)));
    }
    let total = *along.last()?;
    if total < 1e-5 || pieces == 0 {
        return None;
    }
    Some(
        (0..=pieces)
            .map(|k| {
                let d = total * k as f32 / pieces as f32;
                let j = along.partition_point(|a| *a < d).clamp(1, path.len() - 1);
                let t = ((d - along[j - 1]) / (along[j] - along[j - 1]).max(1e-9)).clamp(0.0, 1.0);
                add(mul(path[j - 1].p, 1.0 - t), mul(path[j].p, t))
            })
            .collect(),
    )
}

/// How long the line is, in metres.
pub fn length(stroke: &[Spot]) -> f32 {
    stroke.windows(2).map(|w| len(sub(w[1].p, w[0].p))).sum()
}

/// The geometry of a stroke, or None when the stroke is too short to make anything.
pub fn build(b: &Brush, stroke: &[Spot]) -> Option<Built> {
    let path = prepared(b, stroke)?;
    let count = path.len();
    let round = matches!(b.kind, Kind::Strand | Kind::Rope | Kind::Spike);
    let taper = if b.kind == Kind::Spike { 1.0 } else { b.taper };
    let ring = if round { 5 } else { 2 };
    let mut out = Built::default();
    for i in 0..count {
        let t = i as f32 / (count - 1) as f32;
        let tangent = unit(sub(path[(i + 1).min(count - 1)].p, path[i.saturating_sub(1)].p));
        let mut side = cross(tangent, path[i].n);
        if len(side) < 1e-6 {
            side = cross(tangent, [0.0, 1.0, 0.0]);
        }
        let side = unit(side);
        let up = unit(cross(side, tangent));
        let half = b.width * 0.5 * (1.0 - taper * t).max(0.0);
        for k in 0..ring {
            let (offset, u) = if round {
                let a = k as f32 / ring as f32 * std::f32::consts::TAU;
                (add(mul(side, a.cos() * half), mul(up, a.sin() * half)), k as f32 / ring as f32)
            } else {
                (mul(side, if k == 0 { -half } else { half }), k as f32)
            };
            out.verts.push(add(path[i].p, offset));
            out.uvs.push([u, t]);
        }
    }
    for i in 0..count - 1 {
        for k in 0..ring {
            let k2 = if round { (k + 1) % ring } else { k + 1 };
            if !round && k + 1 >= ring {
                continue;
            }
            let (a, c) = ((i * ring + k) as u32, ((i + 1) * ring + k) as u32);
            let (b2, d) = ((i * ring + k2) as u32, ((i + 1) * ring + k2) as u32);
            out.tris.push([a, b2, c]);
            out.tris.push([b2, d, c]);
            if !round {
                // a flat ribbon is seen from both sides
                out.tris.push([a, c, b2]);
                out.tris.push([b2, c, d]);
            }
        }
    }
    Some(out)
}

/// A cell of the texture for a stroke: the colour runs from the root to the tip (down the cell), with a few fine streaks across, like the hair it draws.
pub fn cell_image(size: u32, root: [u8; 3], tip: [u8; 3], seed: u32) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(size, size);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let t = y as f32 / (size - 1).max(1) as f32;
        let streak = {
            let h = (x.wrapping_mul(2654435761) ^ seed.wrapping_mul(40503)) >> 7;
            ((h & 0xff) as f32 / 255.0 - 0.5) * 0.25
        };
        let mix = |a: u8, b: u8| ((a as f32 + (b as f32 - a as f32) * t) * (1.0 + streak)).clamp(0.0, 255.0) as u8;
        *p = image::Rgba([mix(root[0], tip[0]), mix(root[1], tip[1]), mix(root[2], tip[2]), 255]);
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize, len: f32) -> Vec<Spot> {
        (0..n).map(|i| Spot { p: [i as f32 / (n - 1) as f32 * len, 1.0, 0.0], n: [0.0, 0.0, 1.0] }).collect()
    }

    #[test]
    fn a_card_is_a_flat_ribbon_that_gets_thinner_and_is_seen_from_both_sides() {
        let b = Brush::default().with_kind(Kind::Card);
        let m = build(&b, &line(10, 0.3)).unwrap();
        assert_eq!(m.verts.len() % 2, 0);
        let rows = m.verts.len() / 2;
        let width = |i: usize| len(sub(m.verts[i * 2], m.verts[i * 2 + 1]));
        assert!(width(0) > width(rows - 1) * 2.0, "{} {}", width(0), width(rows - 1));
        assert_eq!(m.tris.len(), (rows - 1) * 4, "two triangles for each side");
        assert!(m.tris.iter().flatten().all(|i| (*i as usize) < m.verts.len()));
        assert!(m.uvs.iter().all(|t| (0.0..=1.0).contains(&t[0]) && (0.0..=1.0).contains(&t[1])));
        assert_eq!(m.uvs[0][1], 0.0);
        assert!((m.uvs[m.uvs.len() - 1][1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_strand_is_round_and_a_lift_moves_it_off_the_surface() {
        let b = Brush::default().with_kind(Kind::Strand);
        let m = build(&b, &line(8, 0.2)).unwrap();
        assert_eq!(m.verts.len() % 5, 0, "five points round");
        let lifted = Brush { lift: 0.05, ..b };
        let m2 = build(&lifted, &line(8, 0.2)).unwrap();
        let z = |m: &Built| m.verts.iter().map(|v| v[2]).sum::<f32>() / m.verts.len() as f32;
        assert!(z(&m2) > z(&m) + 0.04, "pushed along the normal");
    }

    #[test]
    fn a_spike_ends_in_a_point_that_stands_out_of_the_surface() {
        let b = Brush::default().with_kind(Kind::Spike);
        let m = build(&b, &line(6, 0.1)).unwrap();
        let top = m.verts.iter().map(|v| v[2]).fold(f32::MIN, f32::max);
        assert!(top > 0.04, "{top}");
        // the last ring has no width
        let last = &m.verts[m.verts.len() - 5..];
        assert!(last.iter().all(|v| len(sub(*v, last[0])) < 1e-5));
    }

    #[test]
    fn a_droop_lets_the_end_fall_and_a_dot_makes_nothing() {
        let b = Brush { droop: 0.2, ..Brush::default() };
        let m = build(&b, &line(8, 0.2)).unwrap();
        let low = m.verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
        assert!(low < 0.85, "{low}");
        assert!(build(&b, &line(1, 0.2)).is_none());
        let dot = vec![Spot { p: [0.0; 3], n: [0.0, 0.0, 1.0] }; 4];
        assert!(build(&b, &dot).is_none(), "a stroke that does not go anywhere makes nothing");
    }

    #[test]
    fn the_cell_runs_from_the_root_colour_to_the_tip_colour() {
        let img = cell_image(32, [0, 0, 0], [200, 200, 200], 1);
        assert!(img.get_pixel(16, 0).0[0] < 40 && img.get_pixel(16, 31).0[0] > 150);
    }
}
