// SPDX-License-Identifier: GPL-3.0-or-later
// Putting a signature on the picture: stamped (a vector layer you can still move and scale)
// or written live, with your own timing and pressure.

use std::sync::Arc;

use eframe::egui;

use super::DrawState;
use crate::doc::{LayerId, LayerKind};
use crate::signature::{AutoSign, Signature};
use crate::vector::{self, VecData};

/// A signature being written onto a new layer.
pub struct SignReplay {
    layer: LayerId,
    sig: Signature,
    at: (f32, f32, f32),
    start: Option<f64>,
}

impl DrawState {
    /// Stamp: a new vector layer on top with the whole signature.
    pub fn stamp_signature(&mut self, sig: &Signature, rule: &AutoSign) {
        if sig.is_empty() {
            return;
        }
        self.apply_external("Stamp signature", |d| {
            sig.add_layer(&mut d.doc, rule, "Signature");
            d.active = d.doc.layers[0].id;
        });
    }

    /// Live: a new empty layer on top that the signature is written onto over its own duration.
    pub fn start_sign_replay(&mut self, sig: Signature, rule: &AutoSign) {
        if sig.is_empty() || self.sign_replay.is_some() {
            return;
        }
        let at = sig.place(self.doc.w, self.doc.h, rule);
        let mut l = self.doc.new_layer("Signature", LayerKind::Vector);
        l.vector = Some(Arc::new(VecData::default()));
        l.opacity = rule.opacity.clamp(0.0, 100.0);
        let id = l.id;
        self.doc.layers.insert(0, l);
        self.active = id;
        self.doc.mark_all_dirty();
        self.sign_replay = Some(SignReplay { layer: id, sig, at, start: None });
    }

    #[cfg(test)]
    pub fn signing_live(&self) -> bool {
        self.sign_replay.is_some()
    }

    /// Every frame while a signature is being written.
    pub(super) fn sign_replay_step(&mut self, ctx: &egui::Context) {
        let Some(r) = self.sign_replay.as_mut() else { return };
        let now = ctx.input(|i| i.time);
        let skip = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let t = (now - *r.start.get_or_insert(now)) as f32;
        let done = skip || t >= r.sig.duration() + 0.02;
        let (x, y, k) = r.at;
        let strokes = r.sig.vec_strokes(x, y, k, (!done).then_some(t));
        let (sw, sh) = r.sig.size();
        let id = r.layer;
        let Some(i) = self.doc.index_of(id) else {
            self.sign_replay = None;
            return;
        };
        let (w, h) = (self.doc.w, self.doc.h);
        let l = &mut self.doc.layers[i];
        l.vector = Some(Arc::new(VecData { strokes, width: 1.0, fills: Vec::new() }));
        vector::render_all(l, w, h);
        l.rev += 1;
        let pad = (r.sig.style.size * k).ceil() as i32 + 8;
        self.doc.mark_dirty_rect(x as i32 - pad, y as i32 - pad, (x + sw * k) as i32 + pad, (y + sh * k) as i32 + pad);
        if done {
            self.sign_replay = None;
        } else {
            ctx.request_repaint();
        }
    }
}
