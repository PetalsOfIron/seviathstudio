// SPDX-License-Identifier: GPL-3.0-or-later
// Getting a finished drawing ready for rigging in one step. A real drawing has a dozen layers for every part of the character (the
// eye is a white, lashes, a liner, a lid, shadows and highlights, in folders). Every layer is tagged from its name and from the
// folders around it, and the stacking of the artist is not touched: the order, the folders, the blend modes and the opacity stay as they
// are. A layer that is clipped to another (a shadow on a base) is tagged with the part of its base and stays clipped, as in the file;
// the rig picture cuts it to its base. The layers of a part can also be put together into one layer each (a smaller
// model for a game), only where nothing else lies between them in the stack, so the picture does not change. All of it is one step in
// the history.

use std::collections::HashMap;

use crate::doc::{Blend, Document, LayerId};
use crate::parts::{self, NameRule, Part};

/// What was done.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Report {
    /// Layers that got a tag.
    pub tagged: usize,
    /// Layers whose clipping was baked into their pixels.
    pub clips_baked: usize,
    /// Parts that now have a layer.
    pub parts: usize,
    /// How many layers were put together into those parts.
    pub layers_put_together: usize,
    /// Layers with paint that no part was found for (they stay as they are).
    pub left_alone: Vec<String>,
}

/// The lowest confidence a suggestion may have to be used (guesses from the shape are lower).
const MIN: f32 = 0.45;

/// The layers of a part in groups that have no other part between them in the stack (top first): putting them together keeps what is
/// above and what is below the same, so the drawing looks exactly as the artist stacked it. A part whose layers have another part in
/// between (the white of an eye below the iris, the lashes above it; the dress in front of the legs and the dress behind them) is
/// more than one layer, each at its own place.
fn runs(part: Part, ids: &[LayerId], doc: &Document, assigned: &HashMap<LayerId, Part>) -> Vec<Vec<LayerId>> {
    let pos = |id: LayerId| doc.index_of(id).unwrap_or(usize::MAX);
    let mut ids: Vec<LayerId> = ids.to_vec();
    ids.sort_by_key(|id| pos(*id));
    // a layer that works on what is below it (a shadow that darkens, a colour on a base) is not a drawing of its own: it goes with the
    // layers of its part below it, even when other parts lie between (the shade on the face over the eyes). Only drawings that look the
    // same wherever they are (normal blending, not clipped) start a new group when another part is between them and the group below.
    let own_look = |id: &LayerId| doc.layers.iter().find(|l| l.id == *id).is_some_and(|l| l.blend == Blend::Normal && !l.clip);
    let mut out: Vec<Vec<LayerId>> = Vec::new();
    for id in ids.iter().rev() {
        let start_new = match out.last().and_then(|r| r.last()) {
            None => true,
            Some(prev) => {
                let (upper, lower) = (pos(*id), pos(*prev));
                // another part's layer between the two: they are not next to each other
                own_look(id) && doc.layers.get(upper + 1..lower).is_some_and(|between| between.iter().any(|l| parts::is_part_layer(l) && l.visible && assigned.get(&l.id).is_some_and(|p| *p != part)))
            }
        };
        if start_new {
            // a layer that works on what is below it, with nothing of its part below it, has nothing to work on
            if own_look(id) || !out.is_empty() {
                out.push(vec![*id]);
            }
        } else if let Some(r) = out.last_mut() {
            r.push(*id);
        }
    }
    // the groups were made from the bottom; each is top first again, and the top group first
    for r in out.iter_mut() {
        r.reverse();
    }
    out.reverse();
    out
}

/// Make a clipped layer stand on its own: what lies outside the layer it is clipped to is cleared, and the clipping is switched off. The
/// picture stays the same. Returns how many layers were done.
pub fn bake_clips(doc: &mut Document) -> usize {
    let mut done = 0;
    for i in 0..doc.layers.len() {
        let l = &doc.layers[i];
        if !(parts::is_part_layer(l) && l.clip && !l.tiles.is_empty()) {
            continue;
        }
        // the base: the next layer below in the same folder that is not itself clipped
        let Some(base) = doc.layers[i + 1..].iter().find(|b| b.parent == l.parent && !b.clip) else { continue };
        let base_tiles = base.tiles.clone();
        let layer = &mut doc.layers[i];
        let keys: Vec<_> = layer.tiles.keys().copied().collect();
        for key in keys {
            let mask = base_tiles.get(&key);
            let tile = layer.tile_mut(key);
            for (k, px) in tile.iter_mut().enumerate() {
                let a = mask.map(|m| m[k].a() as f32 / 255.0).unwrap_or(0.0);
                if a < 1.0 {
                    // pixels are premultiplied: all four numbers are scaled together
                    let f = |v: u8| (v as f32 * a).round() as u8;
                    *px = eframe::egui::Color32::from_rgba_premultiplied(f(px.r()), f(px.g()), f(px.b()), f(px.a()));
                }
            }
        }
        layer.clip = false;
        layer.rev += 1;
        done += 1;
    }
    done
}

/// Tag the layers, bake the clipping, and (with `merge`) put the layers of each part together where that changes nothing.
pub fn tidy(doc: &mut Document, rules: &[NameRule], merge: bool) -> Report {
    let mut report = Report::default();
    let found = parts::suggest_with(&doc.layers, doc.w, doc.h, false, rules);
    let mut groups: HashMap<Part, Vec<LayerId>> = HashMap::new();
    for s in found.iter().filter(|s| s.confidence >= MIN) {
        groups.entry(s.part).or_default().push(s.layer);
    }
    // layers that were tagged by hand join their part too
    for l in doc.layers.iter().filter(|l| parts::is_part_layer(l) && l.visible && !l.tiles.is_empty()) {
        if let Some(p) = l.part {
            groups.entry(p).or_default().push(l.id);
        }
    }
    // a layer that is clipped to the layer below it (a shadow, a highlight, a colour on a base) belongs to the part of that layer, whatever
    // its name says; when the base has no part, it has none either
    let mut part_of: HashMap<LayerId, Part> = groups.iter().flat_map(|(p, ids)| ids.iter().map(move |id| (*id, *p))).collect();
    for i in 0..doc.layers.len() {
        let l = &doc.layers[i];
        if !(parts::is_part_layer(l) && l.visible && l.clip) {
            continue;
        }
        // the base: the next layer below in the same folder that is not itself clipped
        let base = doc.layers[i + 1..].iter().find(|b| b.parent == l.parent && !b.clip);
        match base.and_then(|b| part_of.get(&b.id).copied()) {
            Some(p) => {
                part_of.insert(l.id, p);
            }
            None => {
                part_of.remove(&l.id);
            }
        }
    }
    // the picture stays as it is: the layers keep their places, their blend modes and their opacity
    if !merge {
        for l in doc.layers.iter_mut() {
            if let Some(p) = part_of.get(&l.id) {
                if l.part != Some(*p) {
                    l.part = Some(*p);
                }
                report.tagged += 1;
            }
        }
        let mut kinds: Vec<Part> = part_of.values().copied().collect();
        kinds.sort_by_key(|p| Part::ALL.iter().position(|q| q == p));
        kinds.dedup();
        report.parts = kinds.len();
        // the inside of the mouth (inner mouth, teeth) and a skin patch that lies in a feature ("Facepatch" in the mouth folder) are cut to the
        // black outline of the face, so they never go outside it. The stacking stays as drawn: face, inner mouth, teeth, patch.
        let cut: Vec<LayerId> = doc
            .layers
            .iter()
            .filter(|l| {
                let n = l.name.to_lowercase();
                !l.clip && (l.part == Some(Part::MouthInside) || (l.part.is_some_and(|p| p != Part::Face && p != Part::Neck) && ((n.contains("face") && n.contains("pa")) || (l.part == Some(Part::Mouth) && n.contains("skin"))) && !n.contains("line")))
            })
            .map(|l| l.id)
            .collect();
        for id in cut {
            if !doc.rig.clip_to.iter().any(|c| c.0 == id) {
                std::sync::Arc::make_mut(&mut doc.rig).clip_to.push((id, Part::Face));
            }
        }
        report.left_alone = doc.layers.iter().filter(|l| parts::is_part_layer(l) && l.visible && !l.tiles.is_empty() && l.part.is_none()).map(|l| l.name.clone()).collect();
        doc.mark_all_dirty();
        return report;
    }
    let mut groups: HashMap<Part, Vec<LayerId>> = HashMap::new();
    for (id, p) in &part_of {
        groups.entry(*p).or_default().push(*id);
    }
    let mut order: Vec<(Part, Vec<LayerId>)> = groups.into_iter().collect();
    order.sort_by_key(|(p, _)| Part::ALL.iter().position(|q| q == p).unwrap_or(usize::MAX));
    // empty layers have nothing to put together
    for (_, ids) in order.iter_mut() {
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| doc.layers.iter().find(|l| l.id == *id).is_some_and(|l| !l.tiles.is_empty() && l.visible));
    }
    let assigned: HashMap<LayerId, Part> = order.iter().flat_map(|(p, ids)| ids.iter().map(move |id| (*id, *p))).collect();
    // where the runs are is decided before anything is put together (the places of the layers change)
    let plan: Vec<(Part, Vec<Vec<LayerId>>)> = order.iter().filter(|(_, ids)| !ids.is_empty()).map(|(p, ids)| (*p, runs(*p, ids, doc, &assigned))).collect();
    for (part, group_runs) in plan {
        for ids in group_runs {
            if ids.is_empty() {
                continue;
            }
            let target = if ids.len() == 1 {
                ids[0]
            } else {
                match doc.merge_layers(&ids, part.label()) {
                    Ok(id) => {
                        report.layers_put_together += ids.len();
                        id
                    }
                    Err(_) => continue,
                }
            };
            if let Some(l) = doc.layers.iter_mut().find(|l| l.id == target) {
                l.part = Some(part);
                report.parts += 1;
            }
        }
    }
    report.clips_baked = bake_clips(doc);
    report.left_alone = doc.layers.iter().filter(|l| parts::is_part_layer(l) && l.visible && !l.tiles.is_empty() && l.part.is_none()).map(|l| l.name.clone()).collect();
    doc.mark_all_dirty();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::LayerKind;

    fn square(doc: &mut Document, name: &str, parent: Option<LayerId>, x: i32, y: i32, col: [u8; 3]) -> LayerId {
        let img = image::RgbaImage::from_pixel(40, 40, image::Rgba([col[0], col[1], col[2], 255]));
        let mut l = doc.new_layer(name, LayerKind::Raster);
        l.tiles = crate::files::tiles_from_rgba(&img, x, y, doc.w, doc.h);
        l.rev += 1;
        l.parent = parent;
        let id = l.id;
        doc.layers.insert(0, l);
        id
    }

    #[test]
    fn by_default_every_layer_is_only_tagged_and_the_stacking_is_not_touched() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let eyes = {
            let mut f = d.new_layer("EyesL", LayerKind::Group);
            f.expanded = true;
            let id = f.id;
            d.layers.insert(0, f);
            id
        };
        let white = square(&mut d, "Eyewhitel", Some(eyes), 600, 400, [255, 255, 255]);
        let iris = square(&mut d, "Irisl", Some(eyes), 610, 405, [40, 90, 160]);
        let shade = square(&mut d, "Eyeshadowl", Some(eyes), 590, 395, [90, 60, 80]);
        {
            let l = d.layers.iter_mut().find(|l| l.id == shade).unwrap();
            l.blend = Blend::Multiply;
            l.opacity = 60.0;
        }
        let before: Vec<(String, Option<LayerId>, Blend, f32)> = d.layers.iter().map(|l| (l.name.clone(), l.parent, l.blend, l.opacity)).collect();
        let r = tidy(&mut d, &[], false);
        let after: Vec<(String, Option<LayerId>, Blend, f32)> = d.layers.iter().map(|l| (l.name.clone(), l.parent, l.blend, l.opacity)).collect();
        assert_eq!(before, after, "the order, the folders, the blend modes and the opacity are as they were");
        assert_eq!(r.layers_put_together, 0);
        let part = |id| d.layers.iter().find(|l| l.id == id).unwrap().part;
        assert_eq!((part(white), part(iris), part(shade)), (Some(Part::EyeL), Some(Part::EyeballL), Some(Part::EyeL)));
        assert_eq!(r.tagged, 3);
    }

    #[test]
    fn clipping_stays_as_in_the_file_and_the_rig_picture_is_cut_to_the_base() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let leg = square(&mut d, "Rightleg", None, 300, 300, [230, 190, 160]);
        // a shade that is bigger than the leg and clipped to it
        let shade = square(&mut d, "Shade", None, 280, 300, [100, 60, 40]);
        d.layers.iter_mut().find(|l| l.id == shade).unwrap().clip = true;
        let r = tidy(&mut d, &[], false);
        assert_eq!(r.clips_baked, 0);
        let l = d.layers.iter().find(|l| l.id == shade).unwrap();
        assert!(l.clip, "still clipped, as in the file");
        let t = crate::rig::raster::LayerTex::of_in(&d, l).unwrap();
        let a = |x: i32, y: i32| t.px[(y - t.y0) as usize * t.w + (x - t.x0) as usize][3];
        assert_eq!(a(290, 320), 0.0, "outside the leg the picture is cut");
        assert_eq!(a(310, 320), 1.0, "inside the leg it stays");
        assert_eq!(d.layers.iter().find(|l| l.id == leg).unwrap().part, Some(Part::LegR));
        assert_eq!(l.part, Some(Part::LegR));
    }

    #[test]
    fn many_layers_of_a_part_become_one_layer_tagged_with_the_part() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        // the folders of a real drawing: a left eye folder with a white, an iris, lashes and a brow, and a right eye folder
        let eyes_l = {
            let mut f = d.new_layer("EyesL", LayerKind::Group);
            f.expanded = true;
            let id = f.id;
            d.layers.insert(0, f);
            id
        };
        let white = square(&mut d, "Eyewhitel", Some(eyes_l), 600, 400, [255, 255, 255]);
        let lash = square(&mut d, "EyelashL", Some(eyes_l), 600, 395, [10, 10, 10]);
        let iris = square(&mut d, "Irisl", Some(eyes_l), 610, 405, [40, 90, 160]);
        let brow = square(&mut d, "Eyebrowl", Some(eyes_l), 600, 340, [60, 40, 30]);
        let eye_r = square(&mut d, "EyeR", None, 300, 400, [255, 255, 255]);
        let mystery = square(&mut d, "Layer 77", None, 100, 100, [1, 2, 3]);
        let r = tidy(&mut d, &[], true);
        let tagged = |p: Part| d.layers.iter().filter(|l| l.part == Some(p)).count();
        // the white, the lash and the liner of the left eye are one layer, the iris and the brow are their own
        assert_eq!((tagged(Part::EyeL), tagged(Part::EyeballL), tagged(Part::BrowL), tagged(Part::EyeR)), (1, 1, 1, 1), "{:?}", d.layers.iter().map(|l| (&l.name, l.part)).collect::<Vec<_>>());
        for gone in [white, lash] {
            assert!(d.index_of(gone).is_none(), "layer {gone} was baked into the eye");
        }
        assert!(d.index_of(iris).is_some() && d.index_of(brow).is_some(), "a part of one layer is only tagged");
        assert!(d.index_of(eye_r).is_some() && d.index_of(mystery).is_some());
        assert_eq!(r.layers_put_together, 2);
        assert_eq!(r.left_alone, ["Layer 77"]);
        // the baked eye holds the paint of both layers
        let eye = d.layers.iter().find(|l| l.part == Some(Part::EyeL)).unwrap();
        let px = |x: i32, y: i32| eye.tiles.get(&(x.div_euclid(256), y.div_euclid(256))).map(|t| t[((y.rem_euclid(256)) * 256 + x.rem_euclid(256)) as usize]).unwrap_or_default();
        assert_eq!(px(620, 438).to_array()[..3], [255, 255, 255], "the white");
        assert_eq!(px(620, 420).to_array()[..3], [10, 10, 10], "the lashes are above the white");
        assert_eq!(eye.name, "Eye L");
    }

    #[test]
    fn a_shade_that_darkens_goes_with_the_part_below_it_even_over_other_parts() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let face = square(&mut d, "Face", None, 300, 200, [240, 205, 180]);
        let eye = square(&mut d, "Eye L", None, 320, 250, [40, 70, 140]);
        let shade = square(&mut d, "Faceshadow", None, 300, 200, [150, 120, 120]);
        d.layers.iter_mut().find(|l| l.id == shade).unwrap().blend = Blend::Multiply;
        // a shade with nothing of its part below it
        let lonely = square(&mut d, "Nose shade", None, 600, 600, [10, 10, 10]);
        d.layers.iter_mut().find(|l| l.id == lonely).unwrap().blend = Blend::Multiply;
        let r = tidy(&mut d, &[], true);
        assert!(d.index_of(shade).is_none(), "the shade is baked into the face, over the eye in the stack");
        assert!(d.index_of(eye).is_some());
        assert_eq!(d.layers.iter().filter(|l| l.part == Some(Part::Face)).count(), 1);
        assert!(r.left_alone.contains(&"Nose shade".to_string()), "{:?}", r.left_alone);
        let _ = (face, lonely);
    }

    #[test]
    fn the_stacking_of_the_artist_stays_as_it_was() {
        // the white of an eye below the iris and the lashes above it: two layers of the eye, the iris stays between them
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let white = square(&mut d, "Eyewhite R", None, 300, 400, [255, 255, 255]);
        let iris = square(&mut d, "Iris R", None, 310, 405, [40, 90, 160]);
        let lash = square(&mut d, "Eyelash R", None, 300, 395, [10, 10, 10]);
        tidy(&mut d, &[], true);
        let order: Vec<(String, Option<Part>)> = d.layers.iter().map(|l| (l.name.clone(), l.part)).collect();
        assert_eq!(d.layers.len(), 3, "nothing could be put together without moving the iris: {order:?}");
        assert_eq!(d.layers.iter().filter(|l| l.part == Some(Part::EyeR)).count(), 2);
        assert!(d.index_of(lash).unwrap() < d.index_of(iris).unwrap() && d.index_of(iris).unwrap() < d.index_of(white).unwrap(), "{order:?}");
    }

    #[test]
    fn a_part_on_both_sides_of_another_part_is_not_put_over_it() {
        // a dress with a front in front of the legs and a back behind them: the front must stay in front
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let back = square(&mut d, "Backdress", None, 300, 300, [100, 0, 0]);
        let leg = square(&mut d, "Rightleg", None, 320, 300, [230, 190, 160]);
        let front = square(&mut d, "Skirt", None, 310, 300, [200, 0, 0]);
        let skirt_lace = square(&mut d, "Lace", None, 312, 300, [250, 250, 250]);
        tidy(&mut d, &[], true);
        let at = |p: Part| d.layers.iter().position(|l| l.part == Some(p));
        // top to bottom: the front dress (skirt and lace together), the leg, the back dress
        let clothes: Vec<usize> = d.layers.iter().enumerate().filter(|(_, l)| l.part == Some(Part::Clothing)).map(|(i, _)| i).collect();
        assert_eq!(clothes.len(), 2, "the front and the back are two layers: {:?}", d.layers.iter().map(|l| (&l.name, l.part)).collect::<Vec<_>>());
        let leg_at = at(Part::LegR).unwrap();
        assert!(clothes[0] < leg_at && leg_at < clothes[1], "front, leg, back: {clothes:?} {leg_at}");
        assert!(d.index_of(back).is_some() && d.index_of(leg).is_some(), "the back dress and the leg are as they were");
        assert!(d.index_of(front).is_none() && d.index_of(skirt_lace).is_none(), "the front is the skirt and the lace together");
    }

    #[test]
    fn a_clipped_layer_goes_with_the_layer_it_is_clipped_to() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let leg = square(&mut d, "Rightleg", None, 300, 300, [230, 190, 160]);
        // a shadow clipped to the leg that says "shirt" in its name, and one clipped to something that has no part
        let shadow = square(&mut d, "shirtshadow", None, 300, 300, [120, 80, 60]);
        d.layers.iter_mut().find(|l| l.id == shadow).unwrap().clip = true;
        let prop = square(&mut d, "Layer 5", None, 600, 600, [9, 9, 9]);
        let lost = square(&mut d, "Hairhighlight", None, 600, 600, [200, 200, 0]);
        d.layers.iter_mut().find(|l| l.id == lost).unwrap().clip = true;
        let r = tidy(&mut d, &[], true);
        assert!(d.index_of(shadow).is_none() && d.index_of(leg).is_none(), "the shadow was put together with the leg");
        assert_eq!(d.layers.iter().filter(|l| l.part == Some(Part::LegR)).count(), 1);
        assert_eq!(d.layers.iter().filter(|l| l.part == Some(Part::Clothing)).count(), 0, "not clothing, whatever its name says");
        // clipped to a layer with no part: left alone, not made a part on its own
        assert!(d.index_of(lost).is_some() && d.layers.iter().find(|l| l.id == lost).unwrap().part.is_none());
        assert!(r.left_alone.contains(&"Layer 5".to_string()) && r.left_alone.contains(&"Hairhighlight".to_string()), "{:?}", r.left_alone);
        let _ = prop;
    }

    #[test]
    fn a_layer_with_no_good_name_takes_its_part_from_the_folder() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let hair = {
            let mut f = d.new_layer("Front hair", LayerKind::Group);
            f.expanded = true;
            let id = f.id;
            d.layers.insert(0, f);
            id
        };
        square(&mut d, "Highlight 1", Some(hair), 400, 100, [200, 150, 100]);
        square(&mut d, "strand copy", Some(hair), 440, 100, [150, 100, 60]);
        let r = tidy(&mut d, &[], true);
        assert_eq!(d.layers.iter().filter(|l| l.part == Some(Part::HairFront)).count(), 1);
        assert_eq!(r.layers_put_together, 2);
        assert!(r.left_alone.is_empty());
    }

    #[test]
    fn the_skin_inside_the_mouth_is_not_the_face() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let inner = {
            let mut f = d.new_layer("Innermouth", LayerKind::Group);
            f.expanded = true;
            let id = f.id;
            d.layers.insert(0, f);
            id
        };
        let skin = square(&mut d, "Faceskintop", Some(inner), 480, 600, [200, 80, 90]);
        square(&mut d, "Face", None, 300, 200, [240, 205, 180]);
        tidy(&mut d, &[], true);
        assert_eq!(d.layers.iter().find(|l| l.id == skin).unwrap().part, Some(Part::MouthInside));
    }

    #[test]
    fn teeth_and_a_face_patch_are_cut_to_the_face_and_the_stacking_stays() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        let mouth = {
            let mut f = d.new_layer("Mouth", LayerKind::Group);
            f.expanded = true;
            let id = f.id;
            d.layers.insert(0, f);
            id
        };
        let patch = square(&mut d, "Facepatch", Some(mouth), 450, 600, [240, 205, 180]);
        let teeth = square(&mut d, "Upperteeth", Some(mouth), 460, 610, [255, 255, 255]);
        let inner = square(&mut d, "Innermouth", Some(mouth), 455, 605, [120, 40, 50]);
        let face = square(&mut d, "Face", None, 300, 200, [240, 205, 180]);
        let order: Vec<LayerId> = d.layers.iter().map(|l| l.id).collect();
        tidy(&mut d, &[], false);
        assert_eq!(order, d.layers.iter().map(|l| l.id).collect::<Vec<_>>(), "the stacking is as drawn");
        let cut: Vec<LayerId> = d.rig.clip_to.iter().filter(|c| c.1 == Part::Face).map(|c| c.0).collect();
        assert!(cut.contains(&patch) && cut.contains(&teeth) && cut.contains(&inner));
        assert!(!cut.contains(&face));
    }

    #[test]
    fn a_layer_with_no_side_goes_with_the_eye_it_lies_on_in_a_drawing_named_the_other_way_round() {
        let mut d = Document::new(1000, 1000);
        d.layers.clear();
        // this artist's L is what the viewer sees on the left
        let l_white = square(&mut d, "EyewhiteL", None, 400, 300, [255, 255, 255]);
        let r_white = square(&mut d, "EyewhiteR", None, 560, 300, [255, 255, 255]);
        let l_line = square(&mut d, "eyelinetopL", None, 400, 290, [20, 20, 40]);
        let r_line = square(&mut d, "eyelinetopR", None, 560, 290, [20, 20, 40]);
        // no side in the name, on the left eye as the viewer sees it
        let loose = square(&mut d, "Eyelinebot", None, 405, 340, [20, 20, 40]);
        tidy(&mut d, &[], false);
        let part = |id| d.layers.iter().find(|l| l.id == id).unwrap().part;
        assert_eq!(part(loose), part(l_white), "with the eye it lies on");
        assert_eq!(part(l_line), part(l_white));
        assert_eq!(part(r_line), part(r_white));
        assert_ne!(part(l_white), part(r_white));
    }
}
