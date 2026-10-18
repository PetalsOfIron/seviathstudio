// SPDX-License-Identifier: GPL-3.0-or-later
// Views of a character: the same character drawn from the front, three quarters, the side and the back. Every drawn layer is put in a
// view, and one slider ("View") says which view shows: at 0 the first, at 1 the second and so on. Between two views they fade into
// each other (or cut, when crossfade is off), so the slider can be moved by the head turn of the face tracker, a hotkey or the timeline.
// It is made of ordinary changers (a Fade changer for every layer), so it saves, animates and streams like the rest of the rig.

use serde::{Deserialize, Serialize};

use super::{Binding, Ease, Form, Id, Kind, Rig};

/// What building the views changed for one drawing, to take it back.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Made {
    pub mesh: Id,
    pub original_parent: Option<Id>,
    pub changer: Id,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Views {
    pub names: Vec<String>,
    /// (layer, view number): which view a drawing belongs to.
    pub assign: Vec<(u64, usize)>,
    /// Fade into the next view (true) or cut to it.
    pub crossfade: bool,
    /// The slider, once the views are built.
    pub param: Option<Id>,
    pub made: Vec<Made>,
}

impl Default for Views {
    fn default() -> Self {
        Views { names: Vec::new(), assign: Vec::new(), crossfade: true, param: None, made: Vec::new() }
    }
}

impl Views {
    pub const SUGGESTED: [&'static str; 4] = ["Front", "Three quarters", "Side", "Back"];

    pub fn is_built(&self) -> bool {
        !self.made.is_empty()
    }

    /// Put a layer in a view (it can be in one only); a view number past the end takes it out.
    pub fn assign_layer(&mut self, layer: u64, view: usize) {
        self.assign.retain(|(l, _)| *l != layer);
        if view < self.names.len() {
            self.assign.push((layer, view));
        }
    }

    pub fn view_of(&self, layer: u64) -> Option<usize> {
        self.assign.iter().find(|(l, _)| *l == layer).map(|(_, v)| *v)
    }
}

impl Rig {
    /// Take the views away: the drawings go back to the changers they hung in, and the slider stays (it may be animated).
    pub fn clear_views(&mut self) {
        let made = std::mem::take(&mut self.views.made);
        for m in &made {
            if let Some(mesh) = self.meshes.iter_mut().find(|x| x.id == m.mesh) {
                mesh.parent = m.original_parent;
            }
            self.changers.retain(|c| c.id != m.changer);
            self.bindings.retain(|b| b.changer != m.changer);
        }
    }

    /// Make (or remake) the views from the layers that are in a view. Returns how many drawings got a fade. Drawings without a mesh
    /// are left out.
    pub fn build_views(&mut self, layer_has_mesh: &dyn Fn(u64) -> bool) -> Result<usize, String> {
        let n = self.views.names.len();
        if n < 2 {
            return Err("A character needs at least two views: add them first.".into());
        }
        self.clear_views();
        let max = (n - 1) as f32;
        let param = match self.views.param.filter(|id| self.param(*id).is_some()) {
            Some(id) => {
                if let Some(p) = self.params.iter_mut().find(|p| p.id == id) {
                    p.max = max;
                    p.default = p.default.clamp(p.min, p.max);
                }
                id
            }
            None => self.add_param("View", 0.0, max, 0.0),
        };
        self.views.param = Some(param);
        let assign = self.views.assign.clone();
        let crossfade = self.views.crossfade;
        let mut count = 0;
        for (layer, view) in assign {
            if view >= n || !layer_has_mesh(layer) {
                continue;
            }
            let Some(mi) = self.meshes.iter().position(|m| m.layer == layer) else { continue };
            let (mesh_id, parent) = (self.meshes[mi].id, self.meshes[mi].parent);
            let changer = self.add_changer(&format!("View fade {mesh_id}"), Kind::Fade, parent);
            let (mut keys, mut forms) = (Vec::new(), Vec::new());
            for i in 0..n {
                let shown = if i == view { 1.0 } else { 0.0 };
                keys.push(i as f32);
                forms.push(Form::Vals { v: vec![shown] });
                // cutting: stay as it is until just before the next view
                if !crossfade && i + 1 < n {
                    keys.push((i + 1) as f32 - 0.001);
                    forms.push(Form::Vals { v: vec![shown] });
                }
            }
            self.bindings.push(Binding { changer, params: vec![param], keys: vec![keys], forms, ease: Ease::Linear });
            self.meshes[mi].parent = Some(changer);
            self.views.made.push(Made { mesh: mesh_id, original_parent: parent, changer });
            count += 1;
        }
        if count == 0 {
            return Err("No drawing is in a view yet (or the ones that are have no mesh). Put layers in views first.".into());
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::{ArtMesh, Values};

    fn rig_with_layers(layers: &[u64]) -> Rig {
        let mut r = Rig::default();
        for l in layers {
            let id = r.fresh_id();
            r.meshes.push(ArtMesh { id, layer: *l, verts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], tris: vec![[0, 1, 2]], parent: None });
        }
        r
    }

    #[test]
    fn views_fade_the_drawings_in_and_out_by_one_slider() {
        let mut r = rig_with_layers(&[11, 12, 13]);
        r.views.names = vec!["Front".into(), "Side".into(), "Back".into()];
        r.views.assign_layer(11, 0);
        r.views.assign_layer(12, 1);
        r.views.assign_layer(13, 2);
        assert_eq!(r.views.view_of(12), Some(1));
        let n = r.build_views(&|_| true).unwrap();
        assert_eq!(n, 3);
        let p = r.views.param.unwrap();
        assert_eq!((r.param(p).unwrap().min, r.param(p).unwrap().max), (0.0, 2.0));
        let fade = |r: &Rig, layer: u64, view: f32| {
            let mut v = Values::default();
            v.set(p, view);
            let m = r.mesh_of(layer).unwrap().clone();
            r.mesh_fade(&m, &r.pose(&v))
        };
        // at a view only its drawings show
        assert_eq!((fade(&r, 11, 0.0), fade(&r, 12, 0.0), fade(&r, 13, 0.0)), (1.0, 0.0, 0.0));
        assert_eq!((fade(&r, 11, 1.0), fade(&r, 12, 1.0), fade(&r, 13, 1.0)), (0.0, 1.0, 0.0));
        assert_eq!((fade(&r, 11, 2.0), fade(&r, 13, 2.0)), (0.0, 1.0));
        // half way they cross fade
        assert!((fade(&r, 11, 0.5) - 0.5).abs() < 1e-4 && (fade(&r, 12, 0.5) - 0.5).abs() < 1e-4);
        // cutting instead: stays on its view until just before the next
        r.views.crossfade = false;
        r.build_views(&|_| true).unwrap();
        assert_eq!(r.views.param, Some(p), "the same slider is kept");
        assert_eq!((fade(&r, 11, 0.5), fade(&r, 12, 0.5)), (1.0, 0.0));
        assert_eq!((fade(&r, 11, 1.0), fade(&r, 12, 1.0)), (0.0, 1.0));
        assert_eq!(r.changers.iter().filter(|c| c.kind == Kind::Fade).count(), 3, "remaking does not pile changers up");
    }

    #[test]
    fn clearing_gives_the_drawings_back_their_parents() {
        let mut r = rig_with_layers(&[11, 12]);
        let head = r.add_changer("Head", Kind::Rotation { pivot: [0.0, 0.0] }, None);
        r.meshes[0].parent = Some(head);
        r.views.names = vec!["A".into(), "B".into()];
        r.views.assign_layer(11, 0);
        r.views.assign_layer(12, 1);
        r.build_views(&|_| true).unwrap();
        let fade_parent = r.mesh_of(11).unwrap().parent.unwrap();
        assert_eq!(r.changer(fade_parent).unwrap().parent, Some(head), "the fade hangs in the head");
        assert!(r.views.is_built());
        r.clear_views();
        assert_eq!(r.mesh_of(11).unwrap().parent, Some(head));
        assert_eq!(r.mesh_of(12).unwrap().parent, None);
        assert!(!r.views.is_built() && r.changers.len() == 1 && r.bindings.is_empty());
    }

    #[test]
    fn views_need_two_names_and_a_drawing_with_a_mesh() {
        let mut r = rig_with_layers(&[11]);
        assert!(r.build_views(&|_| true).unwrap_err().contains("two views"));
        r.views.names = vec!["A".into(), "B".into()];
        assert!(r.build_views(&|_| true).unwrap_err().contains("No drawing"));
        r.views.assign_layer(11, 0);
        r.views.assign_layer(99, 1);
        assert_eq!(r.build_views(&|l| l == 11).unwrap(), 1, "a drawing without a mesh is left out");
        // taking a layer out of its view
        r.views.assign_layer(11, 99);
        assert_eq!(r.views.view_of(11), None);
    }
}
