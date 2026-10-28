// SPDX-License-Identifier: GPL-3.0-or-later
// The Rig by hand page of the 3D tab: no AI. Bones are drawn inside the mesh (or the bones of a person are put there to move into place), and the
// weights, which bone moves which part of the mesh and how much, are made with an automatic start (the nearest bones), an envelope per bone, and a brush
// that adds, takes away and smooths, the way a 3D program does it. A pose mode turns the bones to see how the mesh bends. The mesh is seen by a
// small software renderer (`assetview.rs`) with a camera you turn, move and zoom; the bones are drawn over it.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Ui, vec2};
use image::RgbaImage;

use super::SeviathApp;
use crate::assetview::{AssetMesh, Camera};
use crate::rig3d::{Paint, Pose, Rig3, neighbours};
use crate::theme::*;
use crate::widgets::{self, HelpExt, section_title};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tool {
    /// Pick bones and move them.
    #[default]
    Select,
    /// Draw new bones.
    Draw,
    /// Paint the weights.
    Weights,
    /// Turn bones to see the mesh bend.
    Pose,
    /// Turn, move and zoom the view with the left button (for a pen or a tablet).
    Turn,
    /// Paint the texture of the mesh, with colours picked from the texture itself.
    Texture,
}

/// What the left button holds at the moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Grab {
    #[default]
    None,
    Head(usize),
    Tail(usize),
    Whole(usize),
    /// A new bone is being drawn from this place (and under this bone).
    Draw {
        from: [f32; 3],
        parent: Option<usize>,
    },
    Brush,
    Turn,
    Pan,
    Rotate,
    Texture,
}

/// Which part of a bone is picked.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Head,
    Tail,
    Body,
}

pub struct HandRig {
    pub rig: Rig3,
    undo: Vec<Rig3>,
    redo: Vec<Rig3>,
    pub sel: Option<usize>,
    pub tool: Tool,
    pub mode: Paint,
    /// The size of the brush in pixels and how strong it is.
    pub radius: f32,
    pub strength: f32,
    /// Show the weight of the picked bone in colours on the mesh.
    pub heat: bool,
    pub wire: bool,
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan: [f32; 2],
    pub pose: Pose,
    /// How far from a bone (inner: all of it, outer: none of it) an envelope reaches, in metres.
    pub envelope: (f32, f32),
    pub with_animations: bool,
    pub message: Option<String>,
    grab: Grab,
    draw_to: Option<Pos2>,
    near: Option<Vec<Vec<u32>>>,
    bounds: ([f32; 3], [f32; 3]),
    /// Counts what changed the picture (bones, weights, pose).
    version: u64,
    view: Option<([i64; 12], TextureHandle)>,
    depth: Vec<f32>,
    /// The triangle that is seen in every pixel (to paint on the surface).
    ids: Vec<u32>,
    /// The colour painted with, the colour that was picked from the texture last, the size (pixels), the strength, how far a colour may differ to be
    /// replaced, and the texture before each stroke.
    pub color: [u8; 3],
    sample: [u8; 3],
    pub tex_radius: f32,
    pub tex_opacity: f32,
    pub tolerance: f32,
    pub picking: bool,
    tex_undo: Vec<RgbaImage>,
    /// The texture was painted (a saved .glb must be made again from the mesh).
    pub painted: bool,
}

impl Default for HandRig {
    fn default() -> Self {
        HandRig {
            rig: Rig3::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            sel: None,
            tool: Tool::Select,
            mode: Paint::Add,
            radius: 40.0,
            strength: 0.5,
            heat: true,
            wire: false,
            yaw: 0.0,
            pitch: 8.0,
            zoom: 1.0,
            pan: [0.0; 2],
            pose: Pose::default(),
            envelope: (0.04, 0.18),
            with_animations: true,
            message: None,
            grab: Grab::None,
            draw_to: None,
            near: None,
            bounds: ([0.0; 3], [1.0; 3]),
            version: 0,
            view: None,
            depth: Vec::new(),
            ids: Vec::new(),
            color: [200, 80, 80],
            sample: [200, 80, 80],
            tex_radius: 18.0,
            tex_opacity: 0.6,
            tolerance: 30.0,
            picking: false,
            tex_undo: Vec::new(),
            painted: false,
        }
    }
}

/// What a click on the page asks the app to do (it needs the whole app).
#[derive(Clone, Copy, PartialEq)]
enum Ask {
    None,
    Open,
    Save,
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let (ab, ap) = (b - a, p - a);
    let l2 = ab.length_sq();
    let t = if l2 > 1e-6 { (ap.dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t - p).length()
}

/// Blue where a bone does not move a point, to red where it moves it all.
fn heat(w: f32) -> [u8; 3] {
    let w = w.clamp(0.0, 1.0);
    let (r, g, b) = if w < 0.5 {
        let u = w / 0.5;
        (0.0, u, 1.0 - u * 0.6)
    } else {
        let u = (w - 0.5) / 0.5;
        (u, 1.0 - u * 0.8, 0.4 - u * 0.4)
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

impl HandRig {
    /// A mesh arrived: no bones yet, the view fits it.
    pub fn start_for(&mut self, mesh: &AssetMesh) {
        *self = HandRig { rig: Rig3::new(mesh.verts.len()), bounds: mesh.bounds(), ..HandRig::default() };
    }

    /// A rig that came with a file (it must be of this mesh).
    pub fn use_rig(&mut self, rig: Rig3) {
        self.rig = rig;
        self.sel = None;
        self.pose = Pose::default();
        self.version += 1;
        self.undo.clear();
        self.redo.clear();
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.rig.clone());
        if self.undo.len() > 16 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(r) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.rig, r));
            self.sel = self.sel.filter(|s| *s < self.rig.bones.len());
            self.version += 1;
        }
    }

    pub fn redo(&mut self) {
        if let Some(r) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.rig, r));
            self.sel = self.sel.filter(|s| *s < self.rig.bones.len());
            self.version += 1;
        }
    }

    fn posed(&self) -> bool {
        self.tool == Tool::Pose && self.pose.rot.iter().any(|r| *r != [0.0; 3])
    }

    fn camera(&self, w: u32, h: u32) -> Camera {
        let mut c = Camera::fit(self.bounds, self.yaw, self.pitch, w, h);
        c.zoom = self.zoom;
        c.pan = self.pan;
        c
    }

    /// The mesh in a pose, as a picture from a side (what the Animations page shows).
    pub fn render_pose(&self, mesh: &AssetMesh, pose: &Pose, yaw: f32, w: u32, h: u32) -> RgbaImage {
        let verts = self.rig.skin(&mesh.verts, pose);
        let cam = Camera::fit(self.bounds, yaw, 6.0, w, h);
        mesh.render_cam(&verts, &cam, false, crate::assetview::View::Textured, None).0
    }

    /// Where the bones are drawn: at rest, or in the pose.
    fn shown(&self) -> Vec<([f32; 3], [f32; 3])> {
        if self.posed() { self.rig.posed_bones(&self.pose) } else { self.rig.bones.iter().map(|b| (b.head, b.tail)).collect() }
    }

    fn pick(&self, cam: &Camera, at: Pos2) -> Option<(usize, Part)> {
        let shown = self.shown();
        let s = |p: [f32; 3]| {
            let q = cam.project(p);
            Pos2::new(q[0], q[1])
        };
        // the handles first (the picked bone's before the others), then the bodies
        let order: Vec<usize> = self.sel.into_iter().chain((0..shown.len()).filter(|i| Some(*i) != self.sel)).collect();
        for &i in &order {
            if (s(shown[i].0) - at).length() < 9.0 {
                return Some((i, Part::Head));
            }
            if (s(shown[i].1) - at).length() < 9.0 {
                return Some((i, Part::Tail));
            }
        }
        order.into_iter().filter_map(|i| {
            let d = dist_to_segment(at, s(shown[i].0), s(shown[i].1));
            (d < 8.0).then_some((i, d))
        }).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| (i, Part::Body))
    }

    /// The brush at a place of the picture: the points under it that are seen change.
    fn paint_at(&mut self, mesh: &AssetMesh, cam: &Camera, at: Pos2, mode: Paint) {
        let Some(bone) = self.sel else { return };
        if self.rig.weights.len() != mesh.verts.len() {
            return;
        }
        let r = self.radius;
        let w = cam.w as i32;
        let mut touched: Vec<(usize, f32)> = Vec::new();
        for (i, v) in mesh.verts.iter().enumerate() {
            let p = cam.project(*v);
            let (dx, dy) = (p[0] - at.x, p[1] - at.y);
            let d2 = dx * dx + dy * dy;
            if d2 >= r * r {
                continue;
            }
            let (px, py) = (p[0] as i32, p[1] as i32);
            if px < 0 || py < 0 || px >= w || py >= cam.h as i32 {
                continue;
            }
            if self.depth.get(py as usize * w as usize + px as usize).is_some_and(|d| p[2] > *d + 8.0) {
                continue;
            }
            let u = 1.0 - d2.sqrt() / r;
            touched.push((i, u * u * (3.0 - 2.0 * u)));
        }
        if mode == Paint::Smooth && self.near.is_none() {
            self.near = Some(neighbours(&mesh.verts, &mesh.tris));
        }
        let empty = Vec::new();
        let near = self.near.as_ref().unwrap_or(&empty);
        self.rig.paint_points(near, bone, &touched, self.strength * 0.3, mode);
        self.version += 1;
    }

    /// The texel under a pixel of the picture.
    fn texel_at(&self, mesh: &AssetMesh, cam: &Camera, at: Pos2) -> Option<(u32, u32)> {
        let tex = mesh.texture.as_ref()?;
        let (px, py) = (at.x as i32, at.y as i32);
        if px < 0 || py < 0 || px >= cam.w as i32 || py >= cam.h as i32 {
            return None;
        }
        let id = *self.ids.get(py as usize * cam.w as usize + px as usize)?;
        if id == u32::MAX {
            return None;
        }
        let uv = mesh.uv_at(&mesh.verts, cam, id as usize, px as f32 + 0.5, py as f32 + 0.5)?;
        Some((((uv[0].rem_euclid(1.0)) * (tex.width() - 1) as f32) as u32, ((uv[1].rem_euclid(1.0)) * (tex.height() - 1) as f32) as u32))
    }

    /// Take the colour of the texture under the pixel.
    fn pick_color(&mut self, mesh: &AssetMesh, cam: &Camera, at: Pos2) {
        if let (Some((x, y)), Some(tex)) = (self.texel_at(mesh, cam, at), mesh.texture.as_ref()) {
            let c = tex.get_pixel(x, y).0;
            self.color = [c[0], c[1], c[2]];
            self.sample = self.color;
        }
    }

    /// One touch of the brush on the surface: the texels under the circle move towards the colour.
    fn paint_texture(&mut self, mesh: &mut AssetMesh, cam: &Camera, at: Pos2) {
        let Some(mut tex) = mesh.texture.take() else { return };
        let r = self.tex_radius;
        let (x0, x1) = ((at.x - r).floor().max(0.0) as i32, (at.x + r).ceil().min(cam.w as f32 - 1.0) as i32);
        let (y0, y1) = ((at.y - r).floor().max(0.0) as i32, (at.y + r).ceil().min(cam.h as f32 - 1.0) as i32);
        for py in y0..=y1 {
            for px in x0..=x1 {
                let d = ((px as f32 + 0.5 - at.x).powi(2) + (py as f32 + 0.5 - at.y).powi(2)).sqrt();
                if d >= r {
                    continue;
                }
                let id = self.ids.get(py as usize * cam.w as usize + px as usize).copied().unwrap_or(u32::MAX);
                if id == u32::MAX {
                    continue;
                }
                let Some(uv) = mesh.uv_at(&mesh.verts, cam, id as usize, px as f32 + 0.5, py as f32 + 0.5) else { continue };
                let (tx, ty) = (((uv[0].rem_euclid(1.0)) * (tex.width() - 1) as f32) as u32, ((uv[1].rem_euclid(1.0)) * (tex.height() - 1) as f32) as u32);
                let u = 1.0 - d / r;
                let a = (self.tex_opacity * u * u * (3.0 - 2.0 * u)).clamp(0.0, 1.0);
                let old = tex.get_pixel(tx, ty).0;
                let mix = |k: usize| (old[k] as f32 + (self.color[k] as f32 - old[k] as f32) * a).round() as u8;
                tex.put_pixel(tx, ty, image::Rgba([mix(0), mix(1), mix(2), old[3]]));
            }
        }
        mesh.texture = Some(tex);
        self.painted = true;
        self.version += 1;
    }

    fn texture_checkpoint(&mut self, mesh: &AssetMesh) {
        if let Some(t) = &mesh.texture {
            self.tex_undo.push(t.clone());
            if self.tex_undo.len() > 8 {
                self.tex_undo.remove(0);
            }
        }
    }

    fn texture_undo(&mut self, mesh: &mut AssetMesh) {
        if let Some(t) = self.tex_undo.pop() {
            mesh.texture = Some(t);
            self.version += 1;
        }
    }

    /// The picture of the mesh and the bones, and what the mouse does in it.
    fn viewport(&mut self, ui: &mut Ui, mesh: &mut AssetMesh) {
        let avail = ui.available_size();
        let (w, h) = ((avail.x.max(240.0) as u32) / 2 * 2, (avail.y.max(240.0) as u32) / 2 * 2);
        let (rect, resp) = ui.allocate_exact_size(vec2(w as f32, h as f32), Sense::click_and_drag());
        let local = |p: Pos2| Pos2::new(p.x - rect.min.x, p.y - rect.min.y);
        let to_screen = |x: f32, y: f32| Pos2::new(rect.min.x + x, rect.min.y + y);
        let hover = resp.hover_pos().map(local);
        let mods = ui.input(|i| i.modifiers);

        // zoom with the wheel, round the place under the pointer
        if let (true, Some(at)) = (resp.hovered(), hover) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let cam = self.camera(w, h);
                let depth = cam.project(cam.centre)[2];
                let world = cam.unproject([at.x, at.y, depth]);
                self.zoom = (self.zoom * (scroll * 0.003).exp()).clamp(0.3, 12.0);
                let cam2 = self.camera(w, h);
                let back = cam2.project(world);
                self.pan[0] += at.x - back[0];
                self.pan[1] += at.y - back[1];
            }
        }
        let cam = self.camera(w, h);

        // what the buttons do
        let primary_start = resp.drag_started_by(egui::PointerButton::Primary);
        if resp.drag_started_by(egui::PointerButton::Secondary) {
            self.grab = Grab::Turn;
        } else if resp.drag_started_by(egui::PointerButton::Middle) {
            self.grab = if mods.shift { Grab::Pan } else { Grab::Turn };
        } else if primary_start {
            if let Some(at) = hover.or_else(|| ui.input(|i| i.pointer.press_origin()).map(local)) {
                self.grab = self.start_grab(mesh, &cam, at, mods);
            }
        }
        let delta = resp.drag_delta();
        if resp.dragged() {
            let now = ui.input(|i| i.pointer.interact_pos()).map(local);
            match self.grab {
                Grab::Turn => {
                    self.yaw -= delta.x * 0.5;
                    self.pitch = (self.pitch + delta.y * 0.4).clamp(-89.0, 89.0);
                }
                Grab::Pan => {
                    self.pan[0] += delta.x;
                    self.pan[1] += delta.y;
                }
                Grab::Head(i) | Grab::Tail(i) => {
                    if let Some(now) = now {
                        let part_tail = matches!(self.grab, Grab::Tail(_));
                        let old = if part_tail { self.rig.bones[i].tail } else { self.rig.bones[i].head };
                        let d = cam.project(old)[2];
                        let to = cam.unproject([now.x, now.y, d]);
                        if part_tail { self.rig.move_tail(i, to) } else { self.rig.move_head(i, to) }
                        self.version += 1;
                    }
                }
                Grab::Whole(i) => {
                    if let Some(now) = now {
                        let d = cam.project(self.rig.bones[i].head)[2];
                        let a = cam.unproject([now.x - delta.x, now.y - delta.y, d]);
                        let b = cam.unproject([now.x, now.y, d]);
                        self.rig.move_with_children(i, [b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
                        self.version += 1;
                    }
                }
                Grab::Draw { .. } => self.draw_to = now.map(|p| to_screen(p.x, p.y)),
                Grab::Brush => {
                    if let Some(now) = now {
                        let mode = if mods.command { Paint::Subtract } else if mods.shift { Paint::Smooth } else { self.mode };
                        self.paint_at(mesh, &cam, now, mode);
                    }
                }
                Grab::Texture => {
                    if let Some(now) = now {
                        self.paint_texture(mesh, &cam, now);
                    }
                }
                Grab::Rotate => {
                    if let Some(i) = self.sel {
                        if i < self.pose.rot.len() {
                            if mods.shift {
                                self.pose.rot[i][2] += delta.x * 0.5;
                            } else {
                                self.pose.rot[i][1] += delta.x * 0.5;
                                self.pose.rot[i][0] += delta.y * 0.5;
                            }
                            self.version += 1;
                        }
                    }
                }
                Grab::None => {}
            }
        }
        if resp.drag_stopped() {
            if let Grab::Draw { from, parent } = self.grab {
                if let Some(now) = ui.input(|i| i.pointer.interact_pos()).map(local) {
                    let d = cam.project(from)[2];
                    let to = cam.unproject([now.x, now.y, d]);
                    let a = cam.project(from);
                    if (Pos2::new(a[0], a[1]) - now).length() > 6.0 {
                        let name = format!("bone.{:03}", self.rig.bones.len() + 1);
                        let i = self.rig.add_bone(&name, parent, from, to);
                        self.sel = Some(i);
                        self.pose.rot.resize(self.rig.bones.len(), [0.0; 3]);
                        self.version += 1;
                    }
                }
            }
            self.grab = Grab::None;
            self.draw_to = None;
        }
        if resp.clicked_by(egui::PointerButton::Primary) && matches!(self.tool, Tool::Select | Tool::Pose | Tool::Weights) {
            if let Some(at) = hover {
                self.sel = self.pick(&cam, at).map(|p| p.0);
                self.version += 1;
            }
        }
        if resp.hovered() {
            if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::X)) && self.tool != Tool::Weights {
                if let Some(i) = self.sel {
                    self.checkpoint();
                    self.rig.delete_bone(i);
                    self.sel = None;
                    self.version += 1;
                }
            }
            let (undo, redo) = ui.input(|i| (i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z), i.modifiers.command && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z)))));
            if undo {
                if self.tool == Tool::Texture {
                    self.texture_undo(mesh);
                } else {
                    self.undo();
                }
            }
            if redo {
                self.redo();
            }
        }

        // the picture of the mesh
        let tinting = self.tool == Tool::Weights && self.heat && self.sel.is_some();
        let key = [
            w as i64,
            h as i64,
            (self.yaw * 10.0) as i64,
            (self.pitch * 10.0) as i64,
            (self.zoom * 1000.0) as i64,
            (self.pan[0] * 4.0) as i64 * 100_000 + (self.pan[1] * 4.0) as i64,
            self.version as i64,
            self.sel.map(|s| s as i64).unwrap_or(-1),
            tinting as i64,
            self.wire as i64,
            self.posed() as i64,
            mesh.verts.len() as i64,
        ];
        if self.view.as_ref().map(|v| v.0) != Some(key) {
            let verts = if self.posed() { self.rig.skin(&mesh.verts, &self.pose) } else { mesh.verts.clone() };
            let tint: Option<Vec<[u8; 3]>> = match (tinting, self.sel) {
                (true, Some(b)) if self.rig.weights.len() == mesh.verts.len() => Some((0..mesh.verts.len()).map(|i| heat(self.rig.weight_of(i, b))).collect()),
                _ => None,
            };
            let (img, depth, ids) = mesh.render_full(&verts, &cam, self.wire, crate::assetview::View::Textured, tint.as_deref());
            self.depth = depth;
            self.ids = ids;
            let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            self.view = Some((key, ui.ctx().load_texture("mesh_rig_view", color, TextureOptions::LINEAR)));
        }
        let painter = ui.painter_at(rect);
        if let Some((_, tex)) = &self.view {
            painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }

        // the bones, over the mesh
        let shown = self.shown();
        let screen = |p: [f32; 3]| {
            let q = cam.project(p);
            to_screen(q[0], q[1])
        };
        for (i, (a, b)) in shown.iter().enumerate() {
            let on = self.sel == Some(i);
            let col = if on { GOLD } else { Color32::from_rgb(80, 220, 235) };
            let (pa, pb) = (screen(*a), screen(*b));
            painter.line_segment([pa, pb], Stroke::new(if on { 4.5 } else { 3.0 }, Color32::from_black_alpha(160)));
            painter.line_segment([pa, pb], Stroke::new(if on { 2.5 } else { 1.6 }, col));
            painter.circle_filled(pa, if on { 5.0 } else { 3.5 }, col);
            painter.circle_stroke(pb, if on { 4.5 } else { 3.0 }, Stroke::new(1.5, col));
            if on {
                painter.text(pb + vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, &self.rig.bones[i].name, egui::FontId::proportional(12.0), Color32::WHITE);
            }
        }
        if let (Grab::Draw { from, .. }, Some(to)) = (self.grab, self.draw_to) {
            painter.line_segment([screen(from), to], Stroke::new(2.0, EMERALD));
            painter.circle_filled(screen(from), 4.0, EMERALD);
        }
        if matches!(self.tool, Tool::Weights | Tool::Texture) {
            if let Some(at) = hover {
                painter.circle_stroke(to_screen(at.x, at.y), if self.tool == Tool::Texture { self.tex_radius } else { self.radius }, Stroke::new(1.5, Color32::WHITE));
            }
        }
        resp.on_hover_text(match self.tool {
            Tool::Select => "Click a bone to pick it. Drag its round end to move it, or its middle to move it with the bones under it. Right button: turn the view. Wheel: zoom.",
            Tool::Draw => "Drag to draw a bone. Start at the end of a bone to continue it. Right button: turn the view. Wheel: zoom.",
            Tool::Weights => "Paint on the mesh. Ctrl: take away. Shift: smooth. Right button: turn the view. Wheel: zoom.",
            Tool::Pose => "Pick a bone and drag to turn it. Shift: twist.",
            Tool::Turn => "Drag to turn. Shift: move. Wheel: zoom.",
            Tool::Texture => "Paint on the mesh. Alt: pick the colour under the pointer. Right button: turn the view. Wheel: zoom.",
        });
    }

    /// What the left button starts to do at a place of the picture.
    fn start_grab(&mut self, mesh: &mut AssetMesh, cam: &Camera, at: Pos2, mods: egui::Modifiers) -> Grab {
        match self.tool {
            Tool::Turn => {
                if mods.shift { Grab::Pan } else { Grab::Turn }
            }
            Tool::Select => match self.pick(cam, at) {
                Some((i, part)) => {
                    self.sel = Some(i);
                    self.checkpoint();
                    match part {
                        Part::Head => Grab::Head(i),
                        Part::Tail => Grab::Tail(i),
                        Part::Body => Grab::Whole(i),
                    }
                }
                None => Grab::None,
            },
            Tool::Draw => {
                let shown = self.shown();
                let near = |p: [f32; 3]| {
                    let q = cam.project(p);
                    (Pos2::new(q[0], q[1]) - at).length() < 12.0
                };
                self.checkpoint();
                if let Some(i) = (0..shown.len()).find(|i| near(shown[*i].1)) {
                    return Grab::Draw { from: self.rig.bones[i].tail, parent: Some(i) };
                }
                if let Some(i) = (0..shown.len()).find(|i| near(shown[*i].0)) {
                    return Grab::Draw { from: self.rig.bones[i].head, parent: self.rig.bones[i].parent };
                }
                // in the middle of the mesh, in the plane of the picture
                let (lo, hi) = (self.bounds.0, self.bounds.1);
                let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
                let d = cam.project(centre)[2];
                Grab::Draw { from: cam.unproject([at.x, at.y, d]), parent: None }
            }
            Tool::Weights => {
                if self.sel.is_none() {
                    self.message = Some("Pick a bone first (click it), then paint.".into());
                    return Grab::None;
                }
                self.checkpoint();
                self.paint_at(mesh, cam, at, if mods.command { Paint::Subtract } else if mods.shift { Paint::Smooth } else { self.mode });
                Grab::Brush
            }
            Tool::Texture => {
                if mesh.texture.is_none() {
                    self.message = Some("This mesh has no texture to paint on.".into());
                    return Grab::None;
                }
                if self.picking || mods.alt {
                    self.pick_color(mesh, cam, at);
                    self.picking = false;
                    return Grab::None;
                }
                self.texture_checkpoint(mesh);
                self.paint_texture(mesh, cam, at);
                Grab::Texture
            }
            Tool::Pose => match self.pick(cam, at) {
                Some((i, _)) => {
                    self.sel = Some(i);
                    self.pose.rot.resize(self.rig.bones.len(), [0.0; 3]);
                    Grab::Rotate
                }
                None => Grab::None,
            },
        }
    }

    // ------------------------------------------------------------------ the panels

    fn tools_ui(&mut self, ui: &mut Ui, mesh: &mut AssetMesh) {
        section_title(ui, "Tool");
        ui.horizontal_wrapped(|ui| {
            for (t, label, tip) in [
                (Tool::Select, "Pick and move", "Pick a bone, drag its ends or its middle to move it"),
                (Tool::Draw, "Draw bones", "Drag in the picture to draw a bone. Start at the end of a bone to continue the chain"),
                (Tool::Weights, "Paint weights", "Paint which bone moves which part of the mesh"),
                (Tool::Texture, "Paint texture", "Redraw the colours of the mesh, with colours picked from its own texture"),
                (Tool::Pose, "Pose", "Turn bones to see how the mesh bends"),
                (Tool::Turn, "Turn view", "The left button turns the view (for a pen or tablet)"),
            ] {
                if widgets::chip(ui, label, self.tool == t).help(tip).clicked() {
                    self.tool = t;
                    self.version += 1;
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo")).help("Take back the last change (Ctrl+Z)").clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo")).help("Do it again (Ctrl+Y)").clicked() {
                self.redo();
            }
        });
        ui.add_space(6.0);
        match self.tool {
            Tool::Select | Tool::Draw | Tool::Turn => {
                section_title(ui, "Bones");
                if ui.button("Add the bones of a person").help("The bones of a person with the names the animations use, sized to the mesh. Move every joint into place by hand after that.").clicked() {
                    self.checkpoint();
                    let (lo, hi) = self.bounds;
                    self.rig = Rig3::humanoid(mesh.verts.len(), lo, hi);
                    self.sel = Some(0);
                    self.pose = Pose { rot: vec![[0.0; 3]; self.rig.bones.len()], offset: [0.0; 3] };
                    self.version += 1;
                }
                if ui.add_enabled(self.sel.is_some(), egui::Button::new("Add a bone under the picked one")).help("A new bone that starts at the end of the picked bone, pointing the same way").clicked() {
                    if let Some(i) = self.sel {
                        self.checkpoint();
                        let b = &self.rig.bones[i];
                        let d = [b.tail[0] - b.head[0], b.tail[1] - b.head[1], b.tail[2] - b.head[2]];
                        let to = [b.tail[0] + d[0] * 0.8, b.tail[1] + d[1] * 0.8, b.tail[2] + d[2] * 0.8];
                        let n = self.rig.add_bone(&format!("bone.{:03}", self.rig.bones.len() + 1), Some(i), b.tail, to);
                        self.sel = Some(n);
                        self.version += 1;
                    }
                }
                ui.label(RichText::new("Right button: turn the view. Wheel: zoom. Delete: remove the picked bone.").size(11.0).color(TEXT_DIM));
            }
            Tool::Weights => {
                section_title(ui, "Brush");
                ui.horizontal_wrapped(|ui| {
                    for (m, label, tip) in [(Paint::Add, "Add", "More of the picked bone"), (Paint::Subtract, "Take away", "Less of the picked bone (Ctrl)"), (Paint::Smooth, "Smooth", "Make the weights more like their neighbours (Shift)")] {
                        if widgets::chip(ui, label, self.mode == m).help(tip).clicked() {
                            self.mode = m;
                        }
                    }
                });
                ui.add(egui::Slider::new(&mut self.radius, 5.0..=200.0).text("Size")).help("The size of the brush in pixels");
                ui.add(egui::Slider::new(&mut self.strength, 0.05..=1.0).text("Strength")).help("How much one touch changes");
                if ui.checkbox(&mut self.heat, "Show the weight in colours").help("Blue: the picked bone does not move it. Red: it moves it all.").changed() {
                    self.version += 1;
                }
                ui.add_space(6.0);
                section_title(ui, "Weights");
                if ui.button("Automatic weights").help("Every point follows the nearest bones. A good start to paint over.").clicked() {
                    self.checkpoint();
                    self.rig.assign_by_distance(&mesh.verts);
                    self.version += 1;
                }
                ui.add(egui::Slider::new(&mut self.envelope.0, 0.0..=0.5).text("Full")).help("Points this near to the bone (metres) follow it completely");
                ui.add(egui::Slider::new(&mut self.envelope.1, 0.01..=1.0).text("Reach")).help("Points farther than this (metres) are not moved by the bone");
                if ui.add_enabled(self.sel.is_some(), egui::Button::new("Envelope for the picked bone")).help("Gives the picked bone the points near it, fading out with the distance, without taking the others away").clicked() {
                    if let Some(b) = self.sel {
                        self.checkpoint();
                        self.rig.assign_envelope(&mesh.verts, b, self.envelope.0, self.envelope.1.max(self.envelope.0 + 0.001));
                        self.version += 1;
                    }
                }
                if ui.add_enabled(self.sel.is_some(), egui::Button::new("Clear the picked bone")).help("The picked bone moves nothing").clicked() {
                    if let Some(b) = self.sel {
                        self.checkpoint();
                        self.rig.clear_bone(b);
                        self.version += 1;
                    }
                }
                if ui.button("Make every point add up to one").help("Normalize: the bones of a point share it, no more than four").clicked() {
                    self.checkpoint();
                    self.rig.normalize(4);
                    self.version += 1;
                }
                let loose = self.rig.unweighted();
                ui.label(RichText::new(format!("{loose} points are moved by no bone")).size(11.5).color(if loose > 0 { GOLD } else { TEXT_DIM }));
            }
            Tool::Texture => {
                section_title(ui, "Paint the texture");
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut self.color);
                    if widgets::chip(ui, "Pick from the mesh", self.picking).help("Click the mesh to take its colour. Alt does the same while painting.").clicked() {
                        self.picking = !self.picking;
                    }
                });
                ui.add(egui::Slider::new(&mut self.tex_radius, 3.0..=120.0).text("Size")).help("The size of the brush in pixels");
                ui.add(egui::Slider::new(&mut self.tex_opacity, 0.05..=1.0).text("Strength")).help("How much colour one touch puts on");
                ui.add_space(4.0);
                ui.add(egui::Slider::new(&mut self.tolerance, 0.0..=120.0).text("Likeness")).help("How different a colour may be to count as the picked one");
                if ui.add_enabled(mesh.texture.is_some(), egui::Button::new("Recolour the picked colour")).help("Every part of the texture that has about the colour you last picked from the mesh gets the new colour: the whole hair, the whole coat.").clicked() {
                    self.texture_checkpoint(mesh);
                    if let Some(tex) = mesh.texture.as_mut() {
                        let (from, to, tol) = (self.sample, self.color, self.tolerance);
                        for px in tex.pixels_mut() {
                            let d = ((px[0] as f32 - from[0] as f32).powi(2) + (px[1] as f32 - from[1] as f32).powi(2) + (px[2] as f32 - from[2] as f32).powi(2)).sqrt();
                            if d <= tol {
                                let k = 1.0 - d / (tol + 1.0) * 0.5;
                                for c in 0..3 {
                                    px[c] = (to[c] as f32 * k + px[c] as f32 * (1.0 - k)).round().clamp(0.0, 255.0) as u8;
                                }
                            }
                        }
                    }
                    self.painted = true;
                    self.version += 1;
                }
                if ui.add_enabled(!self.tex_undo.is_empty(), egui::Button::new("Undo the paint")).help("Take back the last stroke (Ctrl+Z)").clicked() {
                    self.texture_undo(mesh);
                }
                ui.label(RichText::new("Pick a colour from the mesh, then paint with it or a colour near it to redraw a part. Save the mesh to keep it.").size(11.0).color(TEXT_DIM));
            }
            Tool::Pose => {
                section_title(ui, "Pose");
                self.pose.rot.resize(self.rig.bones.len(), [0.0; 3]);
                match self.sel {
                    Some(i) if i < self.pose.rot.len() => {
                        ui.label(RichText::new(&self.rig.bones[i].name).size(12.5).strong());
                        let mut changed = false;
                        for (k, axis) in ["Bend (x)", "Turn (y)", "Tilt (z)"].into_iter().enumerate() {
                            changed |= ui.add(egui::Slider::new(&mut self.pose.rot[i][k], -180.0..=180.0).text(axis).suffix(" deg")).changed();
                        }
                        if changed {
                            self.version += 1;
                        }
                        if ui.button("Reset this bone").clicked() {
                            self.pose.rot[i] = [0.0; 3];
                            self.version += 1;
                        }
                    }
                    _ => {
                        ui.label(RichText::new("Pick a bone, then drag in the picture or use the sliders.").size(11.5).color(TEXT_DIM));
                    }
                }
                if ui.button("Reset the whole pose").clicked() {
                    self.pose = Pose { rot: vec![[0.0; 3]; self.rig.bones.len()], offset: [0.0; 3] };
                    self.version += 1;
                }
            }
        }
        ui.add_space(6.0);
        section_title(ui, "View");
        ui.horizontal_wrapped(|ui| {
            for (label, to) in [("Front", (0.0, 0.0)), ("Side", (90.0, 0.0)), ("Back", (180.0, 0.0)), ("Top", (0.0, 89.0))] {
                if ui.small_button(label).help("Look from this side.").clicked() {
                    self.yaw = to.0;
                    self.pitch = to.1;
                }
            }
            if ui.small_button("Fit").help("The whole mesh in view").clicked() {
                self.zoom = 1.0;
                self.pan = [0.0; 2];
            }
        });
        if ui.checkbox(&mut self.wire, "Show the triangles").changed() {
            self.version += 1;
        }
    }

    fn bones_ui(&mut self, ui: &mut Ui, ask: &mut Ask) {
        section_title(ui, "Bones");
        if self.rig.bones.is_empty() {
            ui.label(RichText::new("No bones yet. Use Draw bones, or Add the bones of a person.").size(11.5).color(TEXT_DIM));
        }
        // the tree, parents before children
        let mut order: Vec<(usize, usize)> = Vec::new();
        let mut stack: Vec<(usize, usize)> = (0..self.rig.bones.len()).rev().filter(|i| self.rig.bones[*i].parent.is_none()).map(|i| (i, 0)).collect();
        while let Some((i, depth)) = stack.pop() {
            order.push((i, depth));
            stack.extend(self.rig.children(i).into_iter().rev().map(|c| (c, depth + 1)));
        }
        egui::ScrollArea::vertical().id_salt("mesh_bone_list").max_height(220.0).show(ui, |ui| {
            for (i, depth) in order {
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 10.0);
                    if ui.selectable_label(self.sel == Some(i), &self.rig.bones[i].name).clicked() {
                        self.sel = Some(i);
                        self.version += 1;
                    }
                });
            }
        });
        if let Some(i) = self.sel.filter(|i| *i < self.rig.bones.len()) {
            ui.add_space(6.0);
            section_title(ui, "The picked bone");
            let mut name = self.rig.bones[i].name.clone();
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
            if r.gained_focus() {
                self.checkpoint();
            }
            if r.changed() {
                self.rig.bones[i].name = name.chars().filter(|c| !c.is_control()).take(40).collect();
            }
            let parent_name = |p: Option<usize>| p.map(|p| self.rig.bones[p].name.clone()).unwrap_or_else(|| "none".into());
            let mut new_parent = None;
            egui::ComboBox::from_id_salt("mesh_bone_parent").selected_text(format!("Under: {}", parent_name(self.rig.bones[i].parent))).width(190.0).show_ui(ui, |ui| {
                if ui.selectable_label(self.rig.bones[i].parent.is_none(), "none").clicked() {
                    new_parent = Some(None);
                }
                for p in 0..self.rig.bones.len() {
                    if p != i && ui.selectable_label(self.rig.bones[i].parent == Some(p), &self.rig.bones[p].name).clicked() {
                        new_parent = Some(Some(p));
                    }
                }
            });
            if let Some(p) = new_parent {
                self.checkpoint();
                if !self.rig.set_parent(i, p) {
                    self.undo.pop();
                    self.message = Some("A bone cannot be under one of the bones below it.".into());
                }
                self.version += 1;
            }
            for (label, tail) in [("Head", false), ("Tail", true)] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
                    let mut p = if tail { self.rig.bones[i].tail } else { self.rig.bones[i].head };
                    let mut changed = false;
                    for v in &mut p {
                        let r = ui.add(egui::DragValue::new(v).speed(0.002).fixed_decimals(3));
                        if r.drag_started() || r.gained_focus() {
                            self.undo.push(self.rig.clone());
                            self.redo.clear();
                        }
                        changed |= r.changed();
                    }
                    if changed {
                        if tail { self.rig.move_tail(i, p) } else { self.rig.move_head(i, p) }
                        self.version += 1;
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("Mirror").help("A copy on the other side (x), with the name turned round: Left to Right").clicked() {
                    self.checkpoint();
                    if let Some(n) = self.rig.mirror_bone(i) {
                        self.sel = Some(n);
                    }
                    self.version += 1;
                }
                if ui.button("Delete").help("Take the bone away. The bones under it and the points it moved go to its parent. (Delete key)").clicked() {
                    self.checkpoint();
                    self.rig.delete_bone(i);
                    self.sel = None;
                    self.version += 1;
                }
            });
        }
        ui.add_space(8.0);
        section_title(ui, "Files");
        if ui.button("Open a mesh or a rigged file (.glb)...").help("A .glb with or without a skeleton. A rig in it is read, so you can go on.").clicked() {
            *ask = Ask::Open;
        }
        ui.checkbox(&mut self.with_animations, "Put the animations in the file").help("The animations of the Animations page go in the saved file, for the bones that have the names of a person (hips, spine, leftUpperArm...).");
        if ui.add_enabled(!self.rig.bones.is_empty(), egui::Button::new(RichText::new("Save the rigged mesh (.glb)").strong())).help("The mesh, its pictures, the bones and the weights in one .glb file. Unity, Unreal, Godot and Blender open it.").clicked() {
            *ask = Ask::Save;
        }
        if let Some(m) = &self.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
    }
}

impl SeviathApp {
    pub(super) fn mesh_rig_ui(&mut self, ui: &mut Ui) {
        if self.m3.mesh.is_none() {
            let mut open = false;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18)).inner_margin(egui::Margin::same(16))).show(ui, |ui| {
                ui.label(RichText::new("There is no mesh yet.").size(14.0).strong());
                ui.label(RichText::new("Make one on the From a text or From a picture or drawing page, or open a .glb file you have.").size(12.0).color(TEXT_DIM));
                if ui.button("Open a .glb file...").clicked() {
                    open = true;
                }
            });
            if open {
                self.mesh_open_glb();
            }
            return;
        }
        let mut ask = Ask::None;
        let MeshUi { hand, mesh, .. } = &mut self.m3;
        let mesh = mesh.as_mut().expect("there is a mesh");
        egui::Panel::left("mesh_rig_left").default_size(300.0).size_range(240.0..=460.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("mesh_rig_left_scroll").show(ui, |ui| hand.tools_ui(ui, &mut *mesh));
        });
        egui::Panel::right("mesh_rig_right").default_size(290.0).size_range(240.0..=460.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("mesh_rig_right_scroll").show(ui, |ui| hand.bones_ui(ui, &mut ask));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18)).inner_margin(egui::Margin::same(6))).show(ui, |ui| hand.viewport(ui, &mut *mesh));
        match ask {
            Ask::Open => self.mesh_open_glb(),
            Ask::Save => self.mesh_save_rigged(),
            Ask::None => {}
        }
    }

    /// A .glb is opened as the mesh (with its rig, when it has one).
    fn mesh_open_glb(&mut self) {
        let Some(p) = rfd::FileDialog::new().set_title("A mesh").set_directory(self.settings.projects_folder()).add_filter("glTF binary", &["glb"]).pick_file() else { return };
        match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| AssetMesh::from_glb(&b).map(|m| (m, b))) {
            Ok((mesh, bytes)) => {
                let rig = crate::meshfile::rig_from_glb(&bytes).filter(|r| r.weights.len() == mesh.verts.len());
                let has = rig.is_some();
                self.m3.set_mesh(mesh, bytes);
                if let Some(r) = rig {
                    self.m3.hand.use_rig(r);
                }
                self.m3.message = Some(format!("Opened {}{}.", p.display(), if has { " with its bones" } else { "" }));
            }
            Err(e) => self.m3.hand.message = Some(format!("The file could not be opened: {e}")),
        }
    }

    /// The rigged mesh is saved as a .glb, with the animations when asked.
    fn mesh_save_rigged(&mut self) {
        let Some(mesh) = &self.m3.mesh else { return };
        let name = self.m3.file_name();
        let Some(p) = rfd::FileDialog::new().set_title("Save the rigged mesh").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}_rigged.glb")).add_filter("glTF binary", &["glb"]).save_file() else { return };
        let clips = if self.m3.hand.with_animations { self.m3.anim.added.clone() } else { Vec::new() };
        let bytes = crate::meshfile::write_glb(mesh, Some(&self.m3.hand.rig), &clips, self.m3.anim.fps as f32);
        self.m3.hand.message = Some(match std::fs::write(&p, bytes) {
            Ok(()) => format!("Saved {} with {} bones{}.", p.display(), self.m3.hand.rig.bones.len(), if clips.is_empty() { String::new() } else { format!(" and {} animations", clips.len()) }),
            Err(e) => format!("Could not save the file: {e}"),
        });
    }
}

use super::meshtab::MeshUi;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brush_paints_the_texture_under_it_and_the_pointer_picks_it_up_again() {
        let (mut mesh, _) = crate::meshfile::tests_figure();
        let mut hand = HandRig::default();
        hand.start_for(&mesh);
        let cam = hand.camera(200, 300);
        let (_, depth, ids) = mesh.render_full(&mesh.verts, &cam, false, crate::assetview::View::Textured, None);
        hand.depth = depth;
        hand.ids = ids;
        let at = Pos2::new(100.0, 150.0);
        let before = mesh.texture.as_ref().unwrap().clone();
        hand.color = [10, 200, 30];
        hand.tex_opacity = 1.0;
        hand.tex_radius = 12.0;
        hand.paint_texture(&mut mesh, &cam, at);
        assert!(hand.painted);
        assert_ne!(&before, mesh.texture.as_ref().unwrap(), "the texture changed");
        // the middle of the brush is the new colour; the same place picked gives it back
        hand.color = [0; 3];
        hand.pick_color(&mesh, &cam, at);
        let c = hand.color;
        assert!(c[1] > 150 && c[0] < 80, "{c:?}");
        // nothing is painted where there is no mesh
        let mut other = mesh.clone();
        hand.paint_texture(&mut other, &cam, Pos2::new(2.0, 2.0));
        assert_eq!(other.texture, mesh.texture);
    }
}
