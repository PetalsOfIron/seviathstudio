// SPDX-License-Identifier: GPL-3.0-or-later
// The quality checker: moves every slider to its ends and looks for things that go wrong: a gap that opens between
// two parts that touch, a part that folds over itself, a tagged part that has no mesh, a slider that does nothing.
// Most problems come with a Fix.

use super::{Id, Rig, Values};
use crate::doc::{Layer, LayerId};

#[derive(Clone, PartialEq, Debug)]
pub enum IssueKind {
    /// The slider `param`, at `value`, pulls the two meshes apart.
    Gap { a: Id, b: Id, param: Id, value: f32, px: f32 },
    /// A mesh turns inside out (triangles flip) at this slider value.
    Fold { mesh: Id, param: Id, value: f32 },
    /// A tagged layer that cannot move because it has no mesh.
    Unmeshed(LayerId),
    /// A slider that no changer uses.
    Idle(Id),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Issue {
    pub kind: IssueKind,
    pub text: String,
    /// What the Fix button does (None: no automatic fix).
    pub fix: Option<&'static str>,
}

fn layer_name(layers: &[Layer], id: LayerId) -> String {
    layers.iter().find(|l| l.id == id).map(|l| l.name.clone()).unwrap_or_else(|| "?".into())
}

fn area(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2.0
}

/// How close two meshes have to be at rest to count as touching.
fn reach(doc_w: u32) -> f32 {
    (doc_w as f32 * 0.01).max(6.0)
}

pub fn check(rig: &Rig, layers: &[Layer], doc_w: u32) -> Vec<Issue> {
    let mut out = Vec::new();
    let thr = (doc_w as f32 * 0.012).max(10.0);
    let r = reach(doc_w);
    // pairs of meshes that touch at rest, with the vertex pairs along the seam
    let mut seams: Vec<(usize, usize, Vec<(usize, usize, f32)>)> = Vec::new();
    for i in 0..rig.meshes.len() {
        for j in i + 1..rig.meshes.len() {
            let (a, b) = (&rig.meshes[i], &rig.meshes[j]);
            if a.layer == b.layer || rig.glues.iter().any(|g| ((g.a == a.id && g.b == b.id) || (g.a == b.id && g.b == a.id)) && g.weight >= 0.9) {
                continue;
            }
            let mut pairs = Vec::new();
            for (vi, va) in a.verts.iter().enumerate() {
                for (vj, vb) in b.verts.iter().enumerate() {
                    let d = (va[0] - vb[0]).hypot(va[1] - vb[1]);
                    if d <= r {
                        pairs.push((vi, vj, d));
                    }
                }
            }
            if !pairs.is_empty() {
                seams.push((i, j, pairs));
            }
        }
    }
    let mut gap_worst: Vec<Option<(f32, Id, f32)>> = vec![None; seams.len()];
    let mut fold_worst: Vec<Option<(f32, Id, f32)>> = vec![None; rig.meshes.len()];
    for p in &rig.params {
        let mut values = vec![p.min, (p.min + p.default) / 2.0, p.max, (p.max + p.default) / 2.0];
        values.retain(|v| (v - p.default).abs() > 1e-4);
        values.dedup();
        for v in values {
            let mut vals = Values::default();
            vals.set(p.id, v);
            let pose = rig.pose(&vals);
            let all = rig.deformed_all(&pose);
            for (k, (i, j, pairs)) in seams.iter().enumerate() {
                let gap = pairs.iter().map(|(vi, vj, d0)| {
                    let (a, b) = (all[*i][*vi], all[*j][*vj]);
                    (a[0] - b[0]).hypot(a[1] - b[1]) - d0
                }).fold(0.0, f32::max);
                if gap > thr && gap_worst[k].is_none_or(|w| gap > w.0) {
                    gap_worst[k] = Some((gap, p.id, v));
                }
            }
            for (mi, m) in rig.meshes.iter().enumerate() {
                let flipped = m.tris.iter().filter(|t| {
                    let (a0, a1) = (area(m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize]), area(all[mi][t[0] as usize], all[mi][t[1] as usize], all[mi][t[2] as usize]));
                    a0 * a1 < 0.0 && a1.abs() > 1.0
                }).count();
                let share = flipped as f32 / m.tris.len().max(1) as f32;
                if share > 0.03 && fold_worst[mi].is_none_or(|w| share > w.0) {
                    fold_worst[mi] = Some((share, p.id, v));
                }
            }
        }
    }
    let pname = |id: Id| rig.param(id).map(|p| p.name.clone()).unwrap_or_default();
    for (k, w) in gap_worst.iter().enumerate() {
        if let Some((gap, param, value)) = w {
            let (a, b) = (&rig.meshes[seams[k].0], &rig.meshes[seams[k].1]);
            out.push(Issue {
                kind: IssueKind::Gap { a: a.id, b: b.id, param: *param, value: *value, px: *gap },
                text: format!("At {} = {:.0} a gap of {:.0} px opens between “{}” and “{}”.", pname(*param), value, gap, layer_name(layers, a.layer), layer_name(layers, b.layer)),
                fix: Some("Glue them"),
            });
        }
    }
    for (mi, w) in fold_worst.iter().enumerate() {
        if let Some((share, param, value)) = w {
            let m = &rig.meshes[mi];
            out.push(Issue {
                kind: IssueKind::Fold { mesh: m.id, param: *param, value: *value },
                text: format!("At {} = {:.0} “{}” folds over itself ({:.0}% of its triangles flip). Make the slider's shape smaller.", pname(*param), value, layer_name(layers, m.layer), share * 100.0),
                fix: None,
            });
        }
    }
    for l in layers.iter().filter(|l| l.part.is_some() && l.visible && !l.tiles.is_empty()) {
        let in_skin = rig.skin.as_ref().is_some_and(|s| s.layers.contains(&l.id));
        if rig.mesh_of(l.id).is_none() && !in_skin {
            out.push(Issue { kind: IssueKind::Unmeshed(l.id), text: format!("“{}” is tagged but has no mesh, so it will not move.", l.name), fix: Some("Make a mesh") });
        }
    }
    for p in &rig.params {
        let used = rig.bindings.iter().any(|b| b.params.contains(&p.id)) || rig.iks.iter().any(|k| k.px == p.id || k.py == p.id);
        if !used {
            out.push(Issue { kind: IssueKind::Idle(p.id), text: format!("The slider “{}” does nothing: no changer uses it.", p.name), fix: Some("Remove the slider") });
        }
    }
    out
}

/// Do what an issue's Fix button says. Returns true when something changed.
pub fn fix(rig: &mut Rig, issue: &Issue, layers: &[Layer], doc_w: u32, doc_h: u32) -> bool {
    match &issue.kind {
        IssueKind::Gap { a, b, .. } => rig.make_glue(*a, *b, reach(doc_w) * 1.5, 1.0) > 0,
        IssueKind::Idle(p) => {
            let n = rig.params.len();
            rig.params.retain(|x| x.id != *p);
            rig.params.len() != n
        }
        IssueKind::Unmeshed(id) => {
            let Some(l) = layers.iter().find(|l| l.id == *id) else { return false };
            let Some((verts, tris)) = super::mesh::generate(l, doc_w, doc_h, 0.5) else { return false };
            let mid = rig.fresh_id();
            rig.meshes.retain(|m| m.layer != *id);
            rig.meshes.push(super::ArtMesh { id: mid, layer: *id, verts, tris, parent: None });
            true
        }
        IssueKind::Fold { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, LayerKind, TILE};
    use crate::rig::{ArtMesh, Form, Kind};
    use eframe::egui::Color32;
    use std::sync::Arc;

    fn quad_mesh(rig: &mut Rig, layer: LayerId, x0: f32, y0: f32, x1: f32, y1: f32) -> Id {
        let id = rig.fresh_id();
        // a grid of 3 x 3 vertices so the seam has several
        let mut verts = Vec::new();
        for r in 0..3 {
            for c in 0..3 {
                verts.push([x0 + (x1 - x0) * c as f32 / 2.0, y0 + (y1 - y0) * r as f32 / 2.0]);
            }
        }
        let mut tris = Vec::new();
        for r in 0..2u32 {
            for c in 0..2u32 {
                let i = r * 3 + c;
                tris.push([i, i + 1, i + 4]);
                tris.push([i, i + 4, i + 3]);
            }
        }
        rig.meshes.push(ArtMesh { id, layer, verts, tris, parent: None });
        id
    }

    fn layers_with_paint(d: &mut Document, names: &[&str]) -> Vec<LayerId> {
        names
            .iter()
            .map(|n| {
                let mut l = d.new_layer(n, LayerKind::Raster);
                let mut px = vec![Color32::TRANSPARENT; TILE * TILE];
                px[0] = Color32::RED;
                l.tiles.insert((0, 0), Arc::new(px));
                let id = l.id;
                d.layers.push(l);
                id
            })
            .collect()
    }

    #[test]
    fn a_gap_that_opens_at_the_end_of_a_slider_is_found_and_glued() {
        let mut d = Document::new(1000, 1000);
        let ids = layers_with_paint(&mut d, &["Neck", "Head"]);
        let mut rig = Rig::default();
        let neck = quad_mesh(&mut rig, ids[0], 400.0, 500.0, 600.0, 700.0);
        let head = quad_mesh(&mut rig, ids[1], 400.0, 300.0, 600.0, 500.0);
        let c = rig.add_changer("Head turn", Kind::Rotation { pivot: [500.0, 600.0] }, None);
        rig.meshes.iter_mut().find(|m| m.id == head).unwrap().parent = Some(c);
        let p = rig.add_param("Angle Z", -30.0, 30.0, 0.0);
        rig.bind_rotation(c, p, 40.0);
        let issues = check(&rig, &d.layers, 1000);
        let gap = issues.iter().find(|i| matches!(i.kind, IssueKind::Gap { .. })).expect("the seam opens when the head turns");
        assert!(gap.text.contains("Angle Z") && gap.text.contains("Neck") && gap.text.contains("Head"), "{}", gap.text);
        assert_eq!(gap.fix, Some("Glue them"));
        let _ = neck;
        // the fix: glue them, and the checker is quiet about it
        assert!(fix(&mut rig, gap, &d.layers, 1000, 1000));
        assert_eq!(rig.glues.len(), 1);
        let after = check(&rig, &d.layers, 1000);
        assert!(!after.iter().any(|i| matches!(i.kind, IssueKind::Gap { .. })), "{after:?}");
    }

    #[test]
    fn a_rig_that_holds_together_has_nothing_to_report() {
        let mut d = Document::new(1000, 1000);
        let ids = layers_with_paint(&mut d, &["A", "B"]);
        let mut rig = Rig::default();
        quad_mesh(&mut rig, ids[0], 100.0, 100.0, 300.0, 300.0);
        let b = quad_mesh(&mut rig, ids[1], 600.0, 600.0, 800.0, 800.0);
        let c = rig.add_changer("Small turn", Kind::Rotation { pivot: [700.0, 700.0] }, None);
        rig.meshes.iter_mut().find(|m| m.id == b).unwrap().parent = Some(c);
        let p = rig.add_param("Turn", -30.0, 30.0, 0.0);
        rig.bind_rotation(c, p, 10.0);
        assert!(check(&rig, &d.layers, 1000).is_empty());
    }

    #[test]
    fn a_part_that_folds_over_is_reported() {
        let mut d = Document::new(1000, 1000);
        let ids = layers_with_paint(&mut d, &["Flap"]);
        let mut rig = Rig::default();
        let m = quad_mesh(&mut rig, ids[0], 100.0, 100.0, 400.0, 400.0);
        let w = rig.add_changer("Squash", Kind::Warp { rect: [90.0, 90.0, 410.0, 410.0], cols: 1, rows: 1 }, None);
        rig.meshes.iter_mut().find(|x| x.id == m).unwrap().parent = Some(w);
        let p = rig.add_param("Squash", 0.0, 1.0, 0.0);
        // the top edge is pulled down past the bottom: the picture turns inside out
        rig.set_keyform(w, p, 1.0, Form::Warp { offsets: vec![[0.0, 600.0], [0.0, 600.0], [0.0, 0.0], [0.0, 0.0]] });
        let issues = check(&rig, &d.layers, 1000);
        let fold = issues.iter().find(|i| matches!(i.kind, IssueKind::Fold { .. })).expect("a fold");
        assert!(fold.text.contains("Flap") && fold.fix.is_none());
    }

    #[test]
    fn unmeshed_parts_and_idle_sliders_are_reported_and_fixed() {
        let mut d = Document::new(1000, 1000);
        let ids = layers_with_paint(&mut d, &["Tagged"]);
        d.layers.iter_mut().find(|l| l.id == ids[0]).unwrap().part = Some(crate::parts::Part::Nose);
        let mut rig = Rig::default();
        let p = rig.add_param("Lonely", 0.0, 1.0, 0.0);
        // the layer has one pixel: a mesh can't be made from it, but the report is still right
        let issues = check(&rig, &d.layers, 1000);
        assert!(issues.iter().any(|i| i.kind == IssueKind::Unmeshed(ids[0])));
        let idle = issues.iter().find(|i| i.kind == IssueKind::Idle(p)).expect("an idle slider");
        assert!(fix(&mut rig, idle, &d.layers, 1000, 1000));
        assert!(rig.params.is_empty());
        // a layer with real paint gets a mesh from the fix
        let mut big = d.new_layer("Big", LayerKind::Raster);
        for ty in 0..2 {
            for tx in 0..2 {
                big.tiles.insert((tx, ty), Arc::new(vec![Color32::from_rgb(10, 20, 30); TILE * TILE]));
            }
        }
        big.part = Some(crate::parts::Part::Face);
        let id = big.id;
        d.layers.push(big);
        let issues = check(&rig, &d.layers, 1000);
        let un = issues.iter().find(|i| i.kind == IssueKind::Unmeshed(id)).unwrap();
        assert!(fix(&mut rig, un, &d.layers, 1000, 1000));
        assert!(rig.mesh_of(id).is_some());
        assert!(!check(&rig, &d.layers, 1000).iter().any(|i| i.kind == IssueKind::Unmeshed(id)));
    }
}
