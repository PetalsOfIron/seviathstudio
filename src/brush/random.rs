// SPDX-License-Identifier: GPL-3.0-or-later
// The brush randomizer: roll the dice on a brush. Every setting belongs to a group; you can roll
// them all, roll one group, or lock the groups you love and roll the rest. Size, opacity and the
// brush's name are never touched. Painting brushes come first: what comes out is always something
// you can paint with (it never turns invisible).

use super::noise::Rng;
use super::{BrushSettings, Dip, Edge, Ends, Fray, Paint, ShapeBase, TexKind, Tip, Transition};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Group {
    Shape,
    Edges,
    Texture,
    Width,
    Color,
    Scatter,
    Dots,
    Lines,
    Fray,
    Dashes,
    Mixing,
    Wet,
    Hair,
    Skin,
}

impl Group {
    pub const ALL: [(Group, &'static str, &'static str); 14] = [
        (Group::Shape, "Shape", "The tip: its form, roundness, angle and spacing"),
        (Group::Edges, "Edges", "Edge type, wet rim and grain"),
        (Group::Texture, "Texture", "Paper texture and how strongly it shows"),
        (Group::Width, "Width", "Pressure response, tapering and pinch points"),
        (Group::Color, "Color", "Color change along the stroke and the paint running out"),
        (Group::Scatter, "Scatter", "Scatter, random size, angle, density and color, particles"),
        (Group::Dots, "Dots", "Stipple and color dots"),
        (Group::Lines, "Lines", "Threads and waves inside the stroke"),
        (Group::Fray, "Fray", "How the edge breaks up"),
        (Group::Dashes, "Dashes", "Dashed lines"),
        (Group::Mixing, "Mixing", "Color mixing and real paint"),
        (Group::Wet, "Wet", "Water, bleed, pooling and granulation"),
        (Group::Hair, "Hair", "Strands, clumps, curl, flyaways and combing"),
        (Group::Skin, "Skin", "Shade edge, warmth, dot patches and texture-keeping blends"),
    ];
}

fn pick<T: Copy>(r: &mut Rng, items: &[T]) -> T {
    items[((r.next() * items.len() as f32) as usize).min(items.len() - 1)]
}

fn between(r: &mut Rng, lo: f32, hi: f32) -> f32 {
    lo + r.next() * (hi - lo)
}

fn chance(r: &mut Rng, p: f32) -> bool {
    r.next() < p
}

fn random_color(r: &mut Rng) -> [f32; 3] {
    let h = r.next() * 360.0;
    let c = crate::oklab::lch_to_rgb_fit([between(r, 0.35, 0.8), between(r, 0.05, 0.16), h]);
    c
}

/// Roll the dice for the given groups. The same seed gives the same brush.
pub fn randomize(s: &mut BrushSettings, groups: &[Group], seed: u32) {
    let mut r = Rng::new(seed.wrapping_mul(2_654_435_761).wrapping_add(7));
    for _ in 0..3 {
        r.next();
    }
    for g in groups {
        match g {
            Group::Shape => {
                s.tip = pick(&mut r, &[Tip::Round, Tip::Shape, Tip::Shape, Tip::Bristle, Tip::Chalk, Tip::Wet, Tip::Strands, Tip::Grunge, Tip::Sponge, Tip::Feather]);
                if s.tip == Tip::Shape {
                    s.shape_base = pick(&mut r, &ShapeBase::ALL);
                }
                s.roundness = between(&mut r, 0.2, 1.0);
                s.angle = if chance(&mut r, 0.5) { 90.0 } else { between(&mut r, 0.0, 180.0) };
                s.follow_direction = chance(&mut r, 0.55);
                s.hardness = between(&mut r, 0.3, 1.0);
                s.spacing = between(&mut r, 0.04, 0.2);
                s.variants = 1 + (r.next() * 4.0) as u32;
                s.streaks = if chance(&mut r, 0.4) { between(&mut r, 0.2, 0.9) } else { 0.0 };
            }
            Group::Edges => {
                s.edge = pick(&mut r, &Edge::ALL);
                s.edge_amount = between(&mut r, 0.15, 0.8);
                s.edge_detail = between(&mut r, 0.2, 0.9);
                s.wet_rim = if chance(&mut r, 0.3) { between(&mut r, 0.2, 0.8) } else { 0.0 };
                s.grain = if chance(&mut r, 0.3) { between(&mut r, 0.1, 0.5) } else { 0.0 };
            }
            Group::Texture => {
                s.texture = if chance(&mut r, 0.7) { between(&mut r, 0.1, 0.6) } else { 0.0 };
                s.tex_kind = pick(&mut r, &TexKind::ALL);
                s.tex_scale = between(&mut r, 0.6, 2.0);
            }
            Group::Width => {
                s.size_pressure = chance(&mut r, 0.8);
                s.min_size = between(&mut r, 0.15, 0.8);
                s.pressure_curve = if chance(&mut r, 0.5) { between(&mut r, -0.6, 0.6) } else { 0.0 };
                s.ends = if chance(&mut r, 0.4) { Ends::Taper } else { Ends::Round };
                s.taper = between(&mut r, 0.15, 0.7);
                s.tip_start = between(&mut r, 0.02, 0.35);
                s.tip_end = between(&mut r, 0.02, 0.35);
                if chance(&mut r, 0.25) {
                    s.pinch = between(&mut r, 0.2, 0.85);
                    s.pinch_every = between(&mut r, 60.0, 300.0);
                } else {
                    s.pinch = 0.0;
                }
            }
            Group::Color => {
                s.transition = pick(&mut r, &[Transition::Off, Transition::Length, Transition::Speed, Transition::Pressure, Transition::Cycle]);
                s.trans_color = random_color(&mut r);
                s.trans_len = between(&mut r, 150.0, 900.0);
                s.trans_back = chance(&mut r, 0.3);
                s.drift = if chance(&mut r, 0.35) { between(&mut r, -0.9, 0.9) } else { 0.0 };
                s.drift_len = between(&mut r, 250.0, 1200.0);
            }
            Group::Scatter => {
                s.scatter = if chance(&mut r, 0.35) { between(&mut r, 0.1, 1.2) } else { 0.0 };
                s.size_jitter = if chance(&mut r, 0.4) { between(&mut r, 0.1, 0.6) } else { 0.0 };
                s.angle_jitter = if chance(&mut r, 0.35) { between(&mut r, 0.1, 1.0) } else { 0.0 };
                s.opacity_jitter = if chance(&mut r, 0.4) { between(&mut r, 0.1, 0.6) } else { 0.0 };
                s.color_jitter = if chance(&mut r, 0.35) { between(&mut r, 0.1, 0.6) } else { 0.0 };
                s.particles = if chance(&mut r, 0.2) { 2 + (r.next() * 12.0) as u32 } else { 0 };
                s.particle_size = between(&mut r, 0.08, 0.4);
            }
            Group::Dots => {
                if chance(&mut r, 0.35) {
                    s.dots = if chance(&mut r, 0.6) { 1.0 } else { between(&mut r, 0.3, 0.8) };
                    s.dot_min = between(&mut r, 0.02, 0.15);
                    s.dot_max = (s.dot_min + between(&mut r, 0.05, 0.35)).min(0.7);
                    s.dot_count = 1 + (r.next() * 6.0) as u32;
                    s.dot_spread = between(&mut r, 0.3, 1.4);
                    s.dot_colors = if chance(&mut r, 0.6) { between(&mut r, 0.3, 1.0) } else { 0.0 };
                } else {
                    s.dots = 0.0;
                }
            }
            Group::Lines => {
                if chance(&mut r, 0.35) {
                    s.lines = 2 + (r.next() * 7.0) as u32;
                    s.line_wave = between(&mut r, 0.05, 0.5);
                    s.line_len = between(&mut r, 40.0, 220.0);
                    s.line_width = between(&mut r, 0.04, 0.15);
                    s.line_strength = between(&mut r, 0.4, 0.9);
                    s.line_shade = between(&mut r, -0.8, 0.8);
                } else {
                    s.lines = 0;
                }
            }
            Group::Fray => {
                s.fray = pick(&mut r, &[Fray::Off, Fray::Off, Fray::Serrated, Fray::Split, Fray::Knotted]);
                s.fray_amount = between(&mut r, 0.3, 0.9);
                s.fray_len = between(&mut r, 3.0, 12.0);
            }
            Group::Dashes => {
                if chance(&mut r, 0.2) {
                    s.dash_len = between(&mut r, 20.0, 120.0);
                    s.gap_len = between(&mut r, 10.0, 60.0);
                } else {
                    s.gap_len = 0.0;
                }
            }
            Group::Mixing => {
                s.mix = if chance(&mut r, 0.5) { between(&mut r, 0.1, 0.7) } else { 0.0 };
                s.paint = pick(&mut r, &[Paint::Normal, Paint::Mix, Paint::Mix, Paint::Glaze]);
                s.dip = pick(&mut r, &[Dip::Gradual, Dip::Once, Dip::Every]);
                s.dip_every = between(&mut r, 100.0, 700.0);
            }
            Group::Hair => {
                if chance(&mut r, 0.3) {
                    s.strands = 4 + (r.next() * 28.0) as u32;
                    s.strand_width = between(&mut r, 0.03, 0.08);
                    s.strand_fill = if chance(&mut r, 0.3) { between(&mut r, 0.1, 0.5) } else { 0.0 };
                    s.clump = between(&mut r, 0.0, 0.9);
                    s.clump_count = 1 + (r.next() * 6.0) as u32;
                    s.curl = if chance(&mut r, 0.5) { between(&mut r, 0.1, 0.9) } else { 0.0 };
                    s.curl_len = between(&mut r, 50.0, 220.0);
                    s.strand_wobble = between(&mut r, 0.0, 0.6);
                    s.flyaways = if chance(&mut r, 0.5) { between(&mut r, 0.1, 1.0) } else { 0.0 };
                    s.sheen = if chance(&mut r, 0.5) { between(&mut r, 0.2, 1.0) } else { 0.0 };
                } else {
                    s.strands = 0;
                }
                s.comb = if s.mix > 0.3 && chance(&mut r, 0.4) { between(&mut r, 0.3, 1.0) } else { 0.0 };
                s.gloss = if chance(&mut r, 0.2) { between(&mut r, 0.2, 1.0) } else { 0.0 };
            }
            Group::Skin => {
                s.terminator = if chance(&mut r, 0.3) { between(&mut r, 0.2, 1.0) } else { 0.0 };
                s.tint = if chance(&mut r, 0.4) { between(&mut r, -0.8, 0.8) } else { 0.0 };
                s.dot_cluster = if chance(&mut r, 0.4) { between(&mut r, 0.3, 1.0) } else { 0.0 };
                let blends = s.mix > 0.3;
                s.keep_detail = if blends && chance(&mut r, 0.5) { between(&mut r, 0.3, 1.0) } else { 0.0 };
                s.sss = if chance(&mut r, 0.3) { between(&mut r, 0.2, 1.0) } else { 0.0 };
                s.temp_blend = blends && chance(&mut r, 0.2);
            }
            Group::Wet => {
                if chance(&mut r, 0.4) {
                    s.water = between(&mut r, 0.2, 0.9);
                    s.bleed = between(&mut r, 0.0, 0.8);
                    s.pool = between(&mut r, 0.0, 0.8);
                    s.granulation = between(&mut r, 0.0, 0.9);
                } else {
                    s.water = 0.0;
                    s.bleed = 0.0;
                    s.pool = 0.0;
                    s.granulation = 0.0;
                }
            }
        }
    }
    s.seed = 1 + (r.next() * 99_000.0) as u32;
    keep_visible(s);
    // the last word: really paint a short stroke, and if it is faint, simplify until it is not
    let steps: [&dyn Fn(&mut BrushSettings); 4] = [
        &|s| {
            s.terminator = 0.0;
            s.gap_len = 0.0;
        },
        &|s| {
            s.tip = Tip::Round;
            s.edge = Edge::Smooth;
            s.texture = 0.0;
            s.fray = Fray::Off;
        },
        &|s| {
            s.water = 0.0;
            s.bleed = 0.0;
            s.comb = 0.0;
            s.keep_detail = 0.0;
        },
        &|s| {
            s.mix = 0.0;
            s.dots = 0.0;
        },
    ];
    for step in steps {
        if test_stroke(s) >= 200 {
            break;
        }
        step(s);
    }
}

/// A close cousin of a brush: many numeric settings nudged by up to `amount` (0.25 = a quarter),
/// the same seed always giving the same cousin. Nothing switches on or off.
pub fn remix(s: &mut BrushSettings, seed: u32, amount: f32) {
    let mut r = Rng::new(seed.wrapping_mul(2_246_822_519).wrapping_add(11));
    let nudge = |r: &mut Rng, v: &mut f32, lo: f32, hi: f32| {
        // only settings that are in use change: 0 stays 0
        if v.abs() > 1e-4 {
            *v = (*v * (1.0 + r.signed() * amount)).clamp(lo, hi);
        }
    };
    nudge(&mut r, &mut s.hardness, 0.1, 1.0);
    nudge(&mut r, &mut s.spacing, 0.02, 1.0);
    nudge(&mut r, &mut s.roundness, 0.15, 1.0);
    nudge(&mut r, &mut s.flow, 0.3, 1.0);
    nudge(&mut r, &mut s.texture, 0.0, 0.6);
    nudge(&mut r, &mut s.scatter, 0.0, 3.0);
    nudge(&mut r, &mut s.size_jitter, 0.0, 0.7);
    nudge(&mut r, &mut s.angle_jitter, 0.0, 1.0);
    nudge(&mut r, &mut s.opacity_jitter, 0.0, 0.7);
    nudge(&mut r, &mut s.color_jitter, 0.0, 0.8);
    nudge(&mut r, &mut s.edge_amount, 0.0, 0.85);
    nudge(&mut r, &mut s.edge_detail, 0.0, 1.0);
    nudge(&mut r, &mut s.grain, 0.0, 0.8);
    nudge(&mut r, &mut s.streaks, 0.0, 1.0);
    nudge(&mut r, &mut s.wet_rim, 0.0, 1.0);
    nudge(&mut r, &mut s.water, 0.0, 0.9);
    nudge(&mut r, &mut s.bleed, 0.0, 1.0);
    nudge(&mut r, &mut s.pool, 0.0, 1.0);
    nudge(&mut r, &mut s.granulation, 0.0, 1.0);
    nudge(&mut r, &mut s.pinch, 0.0, 0.9);
    nudge(&mut r, &mut s.drift, -1.0, 1.0);
    nudge(&mut r, &mut s.curl, 0.0, 1.0);
    nudge(&mut r, &mut s.flyaways, 0.0, 1.0);
    nudge(&mut r, &mut s.sheen, 0.0, 1.0);
    nudge(&mut r, &mut s.fray_amount, 0.0, 0.9);
    nudge(&mut r, &mut s.line_wave, 0.0, 1.0);
    nudge(&mut r, &mut s.dot_spread, 0.0, 3.0);
    nudge(&mut r, &mut s.tint, -1.0, 1.0);
    if s.angle.abs() > 1e-4 && !s.follow_direction {
        s.angle += r.signed() * 40.0 * amount;
    }
    s.seed = 1 + (r.next() * 99_000.0) as u32;
    keep_visible(s);
}

/// How many pixels a short test stroke of this brush paints on an empty canvas.
fn test_stroke(s: &BrushSettings) -> usize {
    use super::engine::{Sample, Stroke};
    let mut d = crate::doc::Document::new(420, 200);
    let none = |_: i32, _: i32| None;
    let start = Sample { x: 50.0, y: 100.0, p: 0.4 };
    let mut st = Stroke::begin(&mut d, 0, s, eframe::egui::Color32::from_rgb(190, 60, 60), false, 1.0, true, start, &none);
    let pts: Vec<Sample> = (1..=50).map(|i| Sample { x: 50.0 + i as f32 * 6.0, y: 100.0 + (i as f32 * 0.15).sin() * 25.0, p: 0.4 + 0.6 * (i as f32 / 50.0) }).collect();
    for c in pts.chunks(5) {
        st.add(&mut d, c, &none);
    }
    st.finish(&mut d, &none);
    d.layers[0].tiles.values().flat_map(|t| t.iter()).filter(|c| c.a() > 12).count()
}

/// Share of the tip's square that is covered (the worst of its variants).
fn tip_cover(s: &BrushSettings) -> f32 {
    (0..s.variants.max(1))
        .map(|v| {
            let t = super::tips::tip_mask(s, v);
            let m = &t.mips[0];
            let remap = |a: f32| {
                if s.terminator > 0.0 {
                    let half = 0.5 * (1.0 - s.terminator.clamp(0.0, 1.0) * 0.92);
                    super::noise::smoothstep(0.5 - half, 0.5 + half, a)
                } else {
                    a
                }
            };
            m.a.iter().map(|a| remap(*a)).sum::<f32>() / m.a.len().max(1) as f32
        })
        .fold(f32::MAX, f32::min)
}

/// Whatever came out, it must paint: no invisible or empty brush.
fn keep_visible(s: &mut BrushSettings) {
    // some tip and edge pairs eat each other (thin strands with bristly edges leave nothing)
    if tip_cover(s) < 0.02 {
        s.terminator = 0.0;
    }
    if tip_cover(s) < 0.02 {
        s.edge = Edge::Smooth;
        s.streaks = 0.0;
        s.grain = 0.0;
    }
    if tip_cover(s) < 0.02 {
        s.tip = Tip::Round;
    }
    s.opacity = s.opacity.clamp(0.3, 1.0);
    s.flow = s.flow.clamp(0.3, 1.0);
    s.hardness = s.hardness.clamp(0.2, 1.0);
    s.roundness = s.roundness.clamp(0.15, 1.0);
    s.min_size = s.min_size.clamp(0.1, 1.0);
    s.edge_amount = s.edge_amount.min(0.85);
    s.fray_amount = s.fray_amount.min(0.9);
    s.texture = s.texture.min(0.6);
    s.water = s.water.min(0.9);
    s.spacing = s.spacing.clamp(0.03, 0.8);
    if s.dots >= 1.0 && s.dot_count == 0 {
        s.dot_count = 1;
    }
    if s.particles > 0 && s.particle_size < 0.05 {
        s.particle_size = 0.05;
    }
    if s.gap_len > 0.0 && s.dash_len < 8.0 {
        s.dash_len = 8.0;
    }
    if s.ends == Ends::Taper {
        s.tip_start = s.tip_start.max(0.04);
        s.tip_end = s.tip_end.max(0.04);
    }
    if s.strands > 0 && s.strand_fill < 0.1 {
        s.strand_fill = 0.12; // a few thin strands alone can be hard to see
    }
    if s.mix > 0.0 {
        // brushes that carry paint thin out quickly on bare canvas: keep them solid enough to see
        s.water = s.water.min(0.4);
        s.opacity_jitter = s.opacity_jitter.min(0.3);
    }
    s.pinch = s.pinch.min(0.9);
    s.size_jitter = s.size_jitter.min(0.7);
    s.opacity_jitter = s.opacity_jitter.min(0.7);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::engine::{Sample, Stroke};
    use crate::doc::Document;
    use eframe::egui::Color32;

    fn none(_: i32, _: i32) -> Option<Color32> {
        None
    }

    fn all_groups() -> Vec<Group> {
        Group::ALL.iter().map(|g| g.0).collect()
    }

    fn paint(s: &BrushSettings) -> usize {
        let mut d = Document::new(700, 300);
        let start = Sample { x: 80.0, y: 150.0, p: 0.3 };
        let mut st = Stroke::begin(&mut d, 0, s, Color32::from_rgb(190, 60, 60), false, 1.0, true, start, &none);
        let pts: Vec<Sample> = (1..=100).map(|i| Sample { x: 80.0 + i as f32 * 6.0, y: 150.0 + (i as f32 * 0.15).sin() * 40.0, p: 0.3 + 0.7 * (i as f32 / 100.0) }).collect();
        for c in pts.chunks(5) {
            st.add(&mut d, c, &none);
        }
        st.finish(&mut d, &none);
        d.layers[0].tiles.values().flat_map(|t| t.iter()).filter(|c| c.a() > 12).count()
    }

    #[test]
    fn every_random_brush_paints_something() {
        for seed in 0..120u32 {
            let mut s = BrushSettings { size: 40.0, ..BrushSettings::default() };
            randomize(&mut s, &all_groups(), seed);
            let n = paint(&s);
            assert!(n > 30, "seed {seed}: only {n} pixels painted with {s:?}");
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_brush_and_different_seeds_differ() {
        let make = |seed| {
            let mut s = BrushSettings::default();
            randomize(&mut s, &all_groups(), seed);
            format!("{s:?}")
        };
        assert_eq!(make(5), make(5));
        let distinct: std::collections::HashSet<String> = (0..20).map(make).collect();
        assert!(distinct.len() >= 19, "{}", distinct.len());
    }

    #[test]
    fn locked_groups_stay_exactly_as_they_were() {
        let base = BrushSettings { size: 33.0, opacity: 0.8, hardness: 0.7, spacing: 0.1, roundness: 0.5, angle: 45.0, edge: Edge::Torn, lines: 3, ..BrushSettings::default() };
        // roll everything except Shape and Lines
        let rolled: Vec<Group> = all_groups().into_iter().filter(|g| !matches!(g, Group::Shape | Group::Lines)).collect();
        for seed in 0..30 {
            let mut s = base.clone();
            randomize(&mut s, &rolled, seed);
            assert_eq!((s.tip, s.roundness, s.angle, s.follow_direction, s.spacing, s.variants, s.streaks), (base.tip, base.roundness, base.angle, base.follow_direction, base.spacing, base.variants, base.streaks), "shape is locked");
            assert_eq!((s.lines, s.line_wave, s.line_len), (base.lines, base.line_wave, base.line_len), "lines are locked");
            assert_eq!((s.size, s.opacity), (33.0, 0.8), "size and opacity are never rolled");
        }
        // rolling one group leaves the rest alone
        let mut s = base.clone();
        randomize(&mut s, &[Group::Dashes], 9);
        assert_eq!((s.edge, s.lines, s.dots, s.fray, s.water), (base.edge, base.lines, base.dots, base.fray, base.water));
    }

    #[test]
    fn rolled_values_stay_in_the_ranges_the_sliders_allow() {
        for seed in 0..200u32 {
            let mut s = BrushSettings::default();
            randomize(&mut s, &all_groups(), seed);
            let unit = |v: f32| (0.0..=1.0).contains(&v);
            assert!(unit(s.hardness) && unit(s.roundness) && unit(s.dots) && unit(s.fray_amount) && unit(s.water) && unit(s.bleed) && unit(s.pool) && unit(s.granulation) && unit(s.pinch), "seed {seed}");
            assert!((-1.0..=1.0).contains(&s.drift) && (-1.0..=1.0).contains(&s.pressure_curve) && (-1.0..=1.0).contains(&s.line_shade), "seed {seed}");
            assert!(s.lines <= 12 && s.dot_count <= 16 && s.dot_min <= s.dot_max, "seed {seed}");
            assert!(s.trans_color.iter().all(|c| (0.0..=1.0).contains(c)), "seed {seed}: {:?}", s.trans_color);
        }
    }

    #[test]
    fn a_remix_is_a_close_cousin_that_still_paints() {
        let base = crate::brush::presets().into_iter().find(|p| p.name == "Hair clumps").unwrap().s;
        let mut seen = std::collections::HashSet::new();
        for seed in 0..30u32 {
            let mut s = base.clone();
            remix(&mut s, seed, 0.25);
            assert_eq!((s.tip, s.strands > 0, s.paint, s.dots == 0.0, s.lines, s.fray), (base.tip, true, base.paint, true, base.lines, base.fray), "nothing switches on or off");
            assert!((s.hardness - base.hardness).abs() <= base.hardness * 0.26 + 1e-3, "close: {} vs {}", s.hardness, base.hardness);
            assert_eq!((s.size, s.strands, s.opacity), (base.size, base.strands, base.opacity), "size, strand count and opacity are left alone");
            seen.insert(format!("{:.4}{:.4}{}", s.spacing, s.sheen, s.seed));
            assert!(test_stroke(&s) > 100, "seed {seed} still paints");
        }
        assert!(seen.len() >= 25, "different seeds, different cousins: {}", seen.len());
        let (mut a, mut b) = (base.clone(), base.clone());
        remix(&mut a, 7, 0.25);
        remix(&mut b, 7, 0.25);
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "the same seed gives the same cousin");
    }
}
