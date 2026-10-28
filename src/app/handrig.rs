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
use crate::take::Take;
use crate::attach;
use crate::meshbrush::{self, Brush, Kind as BrushKind, Spot};
use crate::meshedit::{self, Cut};
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
    /// Paint which bone a part of the mesh belongs to (the whole part moves with that bone: rigging without weights).
    Parts,
    /// Draw a loop round a part of the mesh: all of it belongs to the picked bone.
    Lasso,
    /// Draw geometry on the surface with a brush: hair, ropes, strips, spikes.
    Mesh,
    /// Put parts on the body with numbers: horns, wings, a tail, ears.
    Attach,
    /// Push, pull, smooth and grab the mesh like the sculpt mode of Blender.
    Sculpt,
}

/// The brushes of sculpting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sculpt {
    /// Push out (or cut in), along the way the surface faces on average under the brush.
    Draw,
    /// Push out along the way each point faces: the surface swells.
    Inflate,
    Smooth,
    /// Take hold and move.
    Grab,
    /// Press the surface flat.
    Flatten,
    /// Draw the points together.
    Pinch,
}

impl Sculpt {
    const ALL: [Sculpt; 6] = [Sculpt::Draw, Sculpt::Inflate, Sculpt::Smooth, Sculpt::Grab, Sculpt::Flatten, Sculpt::Pinch];

    fn label(self) -> &'static str {
        match self {
            Sculpt::Draw => "Draw",
            Sculpt::Inflate => "Inflate",
            Sculpt::Smooth => "Smooth",
            Sculpt::Grab => "Grab",
            Sculpt::Flatten => "Flatten",
            Sculpt::Pinch => "Pinch",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Sculpt::Draw => "Push the surface out. Ctrl: cut it in.",
            Sculpt::Inflate => "Swell the surface along the way each point faces. Ctrl: shrink it.",
            Sculpt::Smooth => "Make the surface smoother.",
            Sculpt::Grab => "Take hold of a part of the surface and drag it.",
            Sculpt::Flatten => "Press the surface flat.",
            Sculpt::Pinch => "Draw the points together (sharper edges, creases). Ctrl: push them apart.",
        }
    }
}

/// Which tools a page has.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolSet {
    Rig,
    Parts,
    /// The Bases page: turn the view and paint.
    Look,
}

impl ToolSet {
    fn allows(self, t: Tool) -> bool {
        match self {
            ToolSet::Rig => matches!(t, Tool::Select | Tool::Draw | Tool::Weights | Tool::Texture | Tool::Pose | Tool::Turn),
            ToolSet::Parts => matches!(t, Tool::Select | Tool::Draw | Tool::Parts | Tool::Lasso | Tool::Turn),
            ToolSet::Look => matches!(t, Tool::Texture | Tool::Mesh | Tool::Attach | Tool::Sculpt | Tool::Turn),
        }
    }
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
    Parts,
    Lasso,
    Mesh,
    Sculpt,
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
    /// The loop being drawn round a part (on the picture).
    lasso: Vec<Pos2>,
    /// How many times the joints are softened.
    pub soften: usize,
    /// The animations made by hand, the one that is shown, the names of the bones they were made for, the frame, whether it plays, whether moving a bone
    /// keys it, and whether the page is the one of the animations.
    pub takes: Vec<Take>,
    pub take: usize,
    take_names: Vec<String>,
    pub frame: f32,
    pub playing: bool,
    pub auto_key: bool,
    pub animating: bool,
    anim_shown: bool,
    /// The brush that draws geometry, the line being drawn, where the strip of the texture for the colours of the strokes starts (v), how many cells of it are
    /// used, and what the strokes added (points, triangles) to take them back.
    pub brush: Brush,
    stroke: Vec<Pos2>,
    pub strip_v0: Option<f32>,
    strip_next: usize,
    geo_undo: Vec<(usize, usize)>,
    /// A cut of the mesh by a plane, and the meshes from before the cuts (with their weights).
    pub cut: Cut,
    cut_undo: Vec<(AssetMesh, Vec<Vec<(u16, f32)>>)>,
    /// The part that is being shaped (horns, wings...): which, its numbers, and the sizes of the mesh from before it was put on.
    pub attach: Option<(attach::Part, attach::Params, usize, usize)>,
    /// Mirror mode: what is painted or drawn on one side is done on the other side too (across the middle of the body, x = `mirror_x`).
    pub mirror: bool,
    pub mirror_x: f32,
    /// Sculpting: the brush, how strong it is, the shape of the mesh before each stroke (to take it back), the way every point faces, and what a grab holds.
    pub sculpt: Sculpt,
    pub sculpt_strength: f32,
    sculpt_undo: Vec<Vec<[f32; 3]>>,
    sculpt_normals: Vec<[f32; 3]>,
    held: Vec<(Vec<(usize, f32)>, Vec<[f32; 3]>, f32, bool)>,
    held_from: Pos2,
    /// The animations of the library (and when they were looked for).
    lib: Vec<crate::animlib::Entry>,
    lib_seen: bool,
    pub lib_message: Option<String>,
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
            lasso: Vec::new(),
            soften: 2,
            takes: Vec::new(),
            take: 0,
            take_names: Vec::new(),
            frame: 0.0,
            playing: false,
            auto_key: true,
            animating: false,
            anim_shown: false,
            brush: Brush::default(),
            stroke: Vec::new(),
            strip_v0: None,
            strip_next: 0,
            geo_undo: Vec::new(),
            cut: Cut { axis: 2, limit: 0.0, above: false },
            cut_undo: Vec::new(),
            attach: None,
            mirror: false,
            mirror_x: 0.0,
            sculpt: Sculpt::Draw,
            sculpt_strength: 0.5,
            sculpt_undo: Vec::new(),
            sculpt_normals: Vec::new(),
            held: Vec::new(),
            held_from: Pos2::ZERO,
            lib: Vec::new(),
            lib_seen: false,
            lib_message: None,
        }
    }
}

/// What a click on the page asks the app to do (it needs the whole app).
#[derive(Clone, Copy, PartialEq)]
enum Ask {
    None,
    Open,
    Save,
    SaveFbx,
    SaveMesh,
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

/// The colour of the part of a bone (the same bone, the same colour); the part of the picked bone is lighter, and a point no bone moves is grey.
fn part_color(bone: Option<usize>, picked: Option<usize>) -> [u8; 3] {
    let Some(b) = bone else { return [90, 90, 96] };
    let h = (b as f32 * 0.618_034).fract();
    let (s, v) = if picked == Some(b) { (0.55, 1.0) } else { (0.6, 0.62) };
    let k = (h * 6.0).floor();
    let f = h * 6.0 - k;
    let (p, q, r) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, bl) = match k as i32 % 6 {
        0 => (v, r, p),
        1 => (q, v, p),
        2 => (p, v, r),
        3 => (p, q, v),
        4 => (r, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (bl * 255.0) as u8]
}

impl HandRig {
    /// A mesh arrived: no bones yet, the view fits it.
    pub fn start_for(&mut self, mesh: &AssetMesh) {
        let bounds = mesh.bounds();
        let mirror_x = attach::Anchors::of(mesh).cx;
        *self = HandRig { rig: Rig3::new(mesh.verts.len()), bounds, mirror_x, cut: Cut { axis: 2, limit: (bounds.0[2] + bounds.1[2]) / 2.0, above: false }, ..HandRig::default() };
    }

    /// The mesh and its bones are moved by a function of the place (turned, scaled): the picture fits it again.
    pub fn transform(&mut self, mesh: &mut AssetMesh, f: impl Fn([f32; 3]) -> [f32; 3]) {
        for v in &mut mesh.verts {
            *v = f(*v);
        }
        for b in &mut self.rig.bones {
            b.head = f(b.head);
            b.tail = f(b.tail);
        }
        self.bounds = mesh.bounds();
        self.mirror_x = attach::Anchors::of(mesh).cx;
        self.painted = true;
        self.near = None;
        self.version += 1;
    }

    /// Every texel of the picture (not the strip for strokes) is this colour.
    pub fn fill_color(&mut self, mesh: &mut AssetMesh, color: [u8; 3]) -> bool {
        if mesh.texture.is_none() {
            return false;
        }
        self.texture_checkpoint(mesh);
        let strip = self.strip_v0;
        if let Some(tex) = mesh.texture.as_mut() {
            let limit = strip.map(|v| (v * tex.height() as f32) as u32).unwrap_or(tex.height());
            for (_, y, p) in tex.enumerate_pixels_mut() {
                if y < limit {
                    *p = image::Rgba([color[0], color[1], color[2], 255]);
                }
            }
        }
        self.painted = true;
        self.version += 1;
        true
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

    /// The texture was changed from outside (the looks were baked again): the picture is made again.
    pub fn texture_changed(&mut self) {
        self.version += 1;
    }

    #[cfg(test)]
    pub fn start_rerender(&mut self) {
        self.version += 1;
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

    /// The place of the picture that shows the mirror image of what is under a pixel: the point of the surface is turned across the middle of the body and
    /// looked for on the picture (so it works in any view where both sides can be seen).
    fn mirror_pixel(&self, mesh: &AssetMesh, cam: &Camera, at: Pos2) -> Option<Pos2> {
        let (px, py) = (at.x as i32, at.y as i32);
        if px < 0 || py < 0 || px >= cam.w as i32 || py >= cam.h as i32 {
            return None;
        }
        let id = *self.ids.get(py as usize * cam.w as usize + px as usize)?;
        if id == u32::MAX {
            return None;
        }
        let (p, _) = mesh.surface_at(&mesh.verts, cam, id as usize, px as f32 + 0.5, py as f32 + 0.5)?;
        let q = cam.project([2.0 * self.mirror_x - p[0], p[1], p[2]]);
        Some(Pos2::new(q[0], q[1]))
    }

    /// The bone on the other side (Left and Right, .L and .R...), or the same bone when it is in the middle. None when the other side has no bone.
    fn mirror_bone(&self, bone: usize) -> Option<usize> {
        let name = &self.rig.bones.get(bone)?.name;
        let other = crate::rig3d::mirrored_name(name);
        if other == *name {
            return Some(bone);
        }
        self.rig.bones.iter().position(|b| b.name == other)
    }

    /// Mirror mode for the weights: the bone on the other side gets the mirror image of the stroke.
    fn mirror_weights(&mut self, mesh: &AssetMesh, cam: &Camera, at: Pos2, mode: Paint) {
        if !self.mirror {
            return;
        }
        let (Some(b), Some(m)) = (self.sel.and_then(|s| self.mirror_bone(s)), self.mirror_pixel(mesh, cam, at)) else { return };
        let keep = self.sel;
        self.sel = Some(b);
        self.paint_at(mesh, cam, m, mode);
        self.sel = keep;
    }

    #[cfg(test)]
    fn mirror_part_test(&mut self, mesh: &AssetMesh, cam: &Camera, at: Pos2, bone: usize) {
        let points: Vec<usize> = self.verts_under(mesh, cam, at, self.radius).into_iter().map(|p| p.0).collect();
        self.rig.assign_part(&points, bone);
        self.mirror_part(mesh, cam, at, bone);
    }

    /// Mirror mode for the parts: the mirror image of the stroke belongs to the bone on the other side.
    fn mirror_part(&mut self, mesh: &AssetMesh, cam: &Camera, at: Pos2, bone: usize) {
        if !self.mirror {
            return;
        }
        let (Some(b), Some(m)) = (self.mirror_bone(bone), self.mirror_pixel(mesh, cam, at)) else { return };
        let points: Vec<usize> = self.verts_under(mesh, cam, m, self.radius).into_iter().map(|p| p.0).collect();
        self.rig.assign_part(&points, b);
    }

    /// A free strip of the texture for the colours of the strokes and of parts is made when the mesh has none (a mesh that was made from a text, a picture or a
    /// drawing has its texture all used).
    pub fn ensure_strip(&mut self, mesh: &mut AssetMesh) -> bool {
        if self.strip_v0.is_some() && mesh.texture.is_some() {
            return true;
        }
        match meshedit::add_strip(mesh, 0.125) {
            Some(v0) => {
                self.strip_v0 = Some(v0);
                self.version += 1;
                true
            }
            None => false,
        }
    }

    /// The strength of a touch of sculpting in metres: as much as the brush is wide, times the strength.
    fn sculpt_reach(&self, cam: &Camera) -> f32 {
        self.radius / cam.scale() * self.sculpt_strength * 0.25
    }

    /// One touch of a sculpting brush at a place of the picture (the mirror image of it is another touch).
    fn sculpt_dab(&mut self, mesh: &mut AssetMesh, cam: &Camera, at: Pos2, opposite: bool) {
        let under = self.verts_under(mesh, cam, at, self.radius);
        if under.is_empty() || self.sculpt_normals.len() != mesh.verts.len() {
            return;
        }
        let touched: Vec<(usize, f32)> = under.into_iter().map(|(i, u)| (i, u * u * (3.0 - 2.0 * u))).collect();
        let sign = if opposite { -1.0 } else { 1.0 };
        let reach = self.sculpt_reach(cam);
        match self.sculpt {
            Sculpt::Draw | Sculpt::Inflate => {
                let mut avg = [0.0f32; 3];
                for (i, f) in &touched {
                    for k in 0..3 {
                        avg[k] += self.sculpt_normals[*i][k] * f;
                    }
                }
                let l = (avg[0] * avg[0] + avg[1] * avg[1] + avg[2] * avg[2]).sqrt().max(1e-9);
                let avg = [avg[0] / l, avg[1] / l, avg[2] / l];
                for (i, f) in touched {
                    let n = if self.sculpt == Sculpt::Draw { avg } else { self.sculpt_normals[i] };
                    for k in 0..3 {
                        mesh.verts[i][k] += n[k] * reach * f * sign;
                    }
                }
            }
            Sculpt::Smooth => {
                if self.near.is_none() {
                    self.near = Some(neighbours(&mesh.verts, &mesh.tris));
                }
                let near = self.near.as_ref().unwrap();
                let old: Vec<[f32; 3]> = touched.iter().map(|(i, _)| mesh.verts[*i]).collect();
                for ((i, f), o) in touched.iter().zip(&old) {
                    let nb = &near[*i];
                    if nb.is_empty() {
                        continue;
                    }
                    let mut avg = [0.0f32; 3];
                    for j in nb {
                        for k in 0..3 {
                            avg[k] += mesh.verts[*j as usize][k] / nb.len() as f32;
                        }
                    }
                    let a = (self.sculpt_strength * f * 0.7).clamp(0.0, 1.0);
                    for k in 0..3 {
                        mesh.verts[*i][k] = o[k] + (avg[k] - o[k]) * a;
                    }
                }
            }
            Sculpt::Flatten => {
                let (mut c, mut n, mut w) = ([0.0f32; 3], [0.0f32; 3], 0.0f32);
                for (i, f) in &touched {
                    for k in 0..3 {
                        c[k] += mesh.verts[*i][k] * f;
                        n[k] += self.sculpt_normals[*i][k] * f;
                    }
                    w += f;
                }
                if w < 1e-6 {
                    return;
                }
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                let (c, n) = ([c[0] / w, c[1] / w, c[2] / w], [n[0] / l, n[1] / l, n[2] / l]);
                for (i, f) in touched {
                    let d = (0..3).map(|k| (mesh.verts[i][k] - c[k]) * n[k]).sum::<f32>();
                    let a = (self.sculpt_strength * f * 0.6).clamp(0.0, 1.0) * sign.max(0.0);
                    for k in 0..3 {
                        mesh.verts[i][k] -= n[k] * d * a;
                    }
                }
            }
            Sculpt::Pinch => {
                let (mut c, mut w) = ([0.0f32; 3], 0.0f32);
                for (i, f) in &touched {
                    for k in 0..3 {
                        c[k] += mesh.verts[*i][k] * f;
                    }
                    w += f;
                }
                if w < 1e-6 {
                    return;
                }
                let c = [c[0] / w, c[1] / w, c[2] / w];
                for (i, f) in touched {
                    let a = (self.sculpt_strength * f * 0.25).clamp(0.0, 1.0) * sign;
                    for k in 0..3 {
                        let d = c[k] - mesh.verts[i][k];
                        mesh.verts[i][k] += d * a;
                    }
                }
            }
            Sculpt::Grab => {}
        }
        self.painted = true;
        self.version += 1;
    }

    /// Take hold of the points under a circle of the picture (and, in mirror mode, of the ones on the other side) to drag them.
    fn sculpt_hold(&mut self, mesh: &AssetMesh, cam: &Camera, at: Pos2) {
        self.held.clear();
        self.held_from = at;
        let mut spots = vec![(at, false)];
        if self.mirror {
            if let Some(m) = self.mirror_pixel(mesh, cam, at) {
                spots.push((m, true));
            }
        }
        for (p, mirrored) in spots {
            let under: Vec<(usize, f32)> = self.verts_under(mesh, cam, p, self.radius).into_iter().map(|(i, u)| (i, u * u * (3.0 - 2.0 * u))).collect();
            if under.is_empty() {
                continue;
            }
            let start: Vec<[f32; 3]> = under.iter().map(|(i, _)| mesh.verts[*i]).collect();
            let depth = under.iter().map(|(i, _)| cam.project(mesh.verts[*i])[2]).sum::<f32>() / under.len() as f32;
            self.held.push((under, start, depth, mirrored));
        }
    }

    /// Drag what is held to the place of the pointer.
    fn sculpt_drag(&mut self, mesh: &mut AssetMesh, cam: &Camera, now: Pos2) {
        let held = std::mem::take(&mut self.held);
        for (under, start, depth, mirrored) in &held {
            // where the pointer started and is now, in space, at the depth of what is held
            let (a, b) = (cam.unproject([self.held_from.x, self.held_from.y, *depth]), cam.unproject([now.x, now.y, *depth]));
            let mut d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            if *mirrored {
                // the other side is held where the mirror image of the start was, and moves as its mirror image
                d[0] = -d[0];
                let m = cam.unproject([self.held_from.x, self.held_from.y, *depth]);
                let _ = m;
            }
            for ((i, f), s) in under.iter().zip(start) {
                for k in 0..3 {
                    mesh.verts[*i][k] = s[k] + d[k] * f * self.sculpt_strength.max(0.2);
                }
            }
        }
        self.held = held;
        self.painted = true;
        self.version += 1;
    }

    /// Take the last stroke of sculpting back.
    fn undo_sculpt(&mut self, mesh: &mut AssetMesh) {
        if let Some(v) = self.sculpt_undo.pop() {
            if v.len() == mesh.verts.len() {
                mesh.verts = v;
                self.sculpt_normals.clear();
                self.version += 1;
            }
        }
    }

    /// The points that are seen under a circle of the picture, each with how near the middle it is (1 in the middle, 0 at the edge).
    fn verts_under(&self, mesh: &AssetMesh, cam: &Camera, at: Pos2, r: f32) -> Vec<(usize, f32)> {
        let w = cam.w as i32;
        let mut out = Vec::new();
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
            out.push((i, 1.0 - d2.sqrt() / r));
        }
        out
    }

    /// The loop is closed: every point that is seen inside it belongs to the picked bone.
    fn finish_lasso(&mut self, mesh: &AssetMesh, cam: &Camera) {
        let poly = std::mem::take(&mut self.lasso);
        let Some(bone) = self.sel else { return };
        if self.mirror {
            // the loop on the other side belongs to the bone on the other side
            let other: Vec<Pos2> = poly.iter().filter_map(|p| self.mirror_pixel(mesh, cam, *p)).collect();
            if let (Some(b), true) = (self.mirror_bone(bone), other.len() == poly.len()) {
                self.lasso_into(mesh, cam, &other, b);
            }
        }
        self.lasso_into(mesh, cam, &poly, bone);
    }

    /// Every point that is seen inside a loop belongs to a bone.
    fn lasso_into(&mut self, mesh: &AssetMesh, cam: &Camera, poly: &[Pos2], bone: usize) {
        if poly.len() < 3 || self.rig.weights.len() != mesh.verts.len() {
            return;
        }
        let inside = |x: f32, y: f32| {
            let mut c = false;
            let mut j = poly.len() - 1;
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[j]);
                if (a.y > y) != (b.y > y) && x < (b.x - a.x) * (y - a.y) / (b.y - a.y) + a.x {
                    c = !c;
                }
                j = i;
            }
            c
        };
        let w = cam.w as i32;
        let mut points = Vec::new();
        for (i, v) in mesh.verts.iter().enumerate() {
            let p = cam.project(*v);
            let (px, py) = (p[0] as i32, p[1] as i32);
            if px < 0 || py < 0 || px >= w || py >= cam.h as i32 || !inside(p[0], p[1]) {
                continue;
            }
            if self.depth.get(py as usize * w as usize + px as usize).is_some_and(|d| p[2] > *d + 8.0) {
                continue;
            }
            points.push(i);
        }
        self.rig.assign_part(&points, bone);
        self.version += 1;
    }

    /// The stroke is let go: the places under it become a stroke of geometry with the brush, and its colours go in a cell of the strip of the texture.
    fn finish_stroke(&mut self, mesh: &mut AssetMesh, cam: &Camera) {
        let pts = std::mem::take(&mut self.stroke);
        let Some(v0) = self.strip_v0 else { return };
        let Some((size, high)) = mesh.texture.as_ref().map(|t| (t.width(), t.height())) else { return };
        let (cell, w) = (32u32, cam.w as usize);
        let cols = (size / cell) as usize;
        let y_start = (v0 * high as f32).ceil() as u32;
        let rows = (high.saturating_sub(y_start) / cell) as usize;
        if self.strip_next >= cols * rows {
            self.message = Some("The strip of the texture for the colours of the strokes is full: take a stroke back, or use a bigger texture.".into());
            return;
        }
        let mut spots = Vec::new();
        let mut first_tri = None;
        for p in &pts {
            let (px, py) = (p.x as i32, p.y as i32);
            if px < 0 || py < 0 || px >= cam.w as i32 || py >= cam.h as i32 {
                continue;
            }
            let Some(&id) = self.ids.get(py as usize * w + px as usize) else { continue };
            if id == u32::MAX {
                continue;
            }
            if let Some((pos, n)) = mesh.surface_at(&mesh.verts, cam, id as usize, px as f32 + 0.5, py as f32 + 0.5) {
                first_tri.get_or_insert(id as usize);
                spots.push(Spot { p: pos, n });
            }
        }
        let from = first_tri.and_then(|t| mesh.tris.get(t)).map(|t| t[0] as usize);
        let start = (mesh.verts.len(), mesh.tris.len());
        if !self.place_stroke(mesh, &spots, from, false) {
            return;
        }
        if self.mirror {
            // the mirror image: the same places turned across the middle of the body, with the triangles turned the other way round so they face out
            let image: Vec<Spot> = spots.iter().map(|s| Spot { p: [2.0 * self.mirror_x - s.p[0], s.p[1], s.p[2]], n: [-s.n[0], s.n[1], s.n[2]] }).collect();
            if self.place_stroke(mesh, &image, from, true) {
                // one step takes both strokes back
                self.geo_undo.pop();
            }
        }
        // what the first stroke started from is what is taken back
        if let Some(last) = self.geo_undo.last_mut() {
            *last = start;
        }
    }

    /// One stroke of geometry from places of the surface.
    fn place_stroke(&mut self, mesh: &mut AssetMesh, spots: &[Spot], from: Option<usize>, mirrored: bool) -> bool {
        let Some(mut built) = meshbrush::build(&self.brush, spots) else {
            self.message = Some("Draw the stroke over the mesh: it has to touch the surface and go somewhere.".into());
            return false;
        };
        if mirrored {
            for t in &mut built.tris {
                t.swap(1, 2);
            }
        }
        let old = (mesh.verts.len(), mesh.tris.len());
        if !self.append_built(mesh, &built, self.strip_next, from) {
            return false;
        }
        self.geo_undo.push(old);
        self.strip_next += 1;
        true
    }

    /// The geometry is put on the mesh: its colours (root to end) are painted in a cell of the strip of the texture, its texture coordinates look into that cell,
    /// and its points move with what the point `from` of the mesh moves with.
    fn append_built(&mut self, mesh: &mut AssetMesh, built: &meshbrush::Built, k: usize, from: Option<usize>) -> bool {
        let Some(v0) = self.strip_v0 else { return false };
        let Some((size, high)) = mesh.texture.as_ref().map(|t| (t.width(), t.height())) else { return false };
        let cell = 32u32;
        let cols = (size / cell) as usize;
        let y_start = (v0 * high as f32).ceil() as u32;
        let rows = (high.saturating_sub(y_start) / cell) as usize;
        if k >= cols * rows {
            self.message = Some("The strip of the texture for the colours of the strokes is full: take a stroke back, or use a bigger texture.".into());
            return false;
        }
        let (x0, y0) = ((k % cols) as u32 * cell, y_start + (k / cols) as u32 * cell);
        if let Some(tex) = mesh.texture.as_mut() {
            let img = meshbrush::cell_image(cell, self.brush.root, self.brush.tip, k as u32 + 1);
            image::imageops::replace(tex, &img, x0 as i64, y0 as i64);
        }
        let base = mesh.verts.len() as u32;
        let old_len = mesh.verts.len();
        for (v, uv) in built.verts.iter().zip(&built.uvs) {
            mesh.verts.push(*v);
            mesh.uvs.push([(x0 as f32 + 2.0 + uv[0] * (cell - 4) as f32) / size as f32, (y0 as f32 + 1.0 + uv[1] * (cell - 2) as f32) / high as f32]);
        }
        mesh.tris.extend(built.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
        if self.rig.weights.len() == old_len {
            let w = from.and_then(|f| self.rig.weights.get(f)).cloned().unwrap_or_default();
            self.rig.weights.extend(std::iter::repeat_n(w, built.verts.len()));
        }
        self.painted = true;
        self.near = None;
        self.version += 1;
        true
    }

    /// The part that is being shaped is built (again) on the mesh: what it was before is taken away first.
    pub fn attach_apply(&mut self, mesh: &mut AssetMesh) {
        let Some((part, params, v0, t0)) = self.attach else { return };
        mesh.verts.truncate(v0);
        mesh.uvs.truncate(v0);
        mesh.tris.truncate(t0);
        self.rig.weights.truncate(v0);
        let anchors = attach::Anchors::of(mesh);
        let Some(built) = attach::build(part, &params, &anchors) else { return };
        // it moves with the point of the mesh nearest to its first point
        let first = built.verts[0];
        let from = mesh.verts.iter().enumerate().min_by(|a, b| {
            let d = |p: &[f32; 3]| (p[0] - first[0]).powi(2) + (p[1] - first[1]).powi(2) + (p[2] - first[2]).powi(2);
            d(a.1).total_cmp(&d(b.1))
        }).map(|x| x.0);
        self.append_built(mesh, &built, self.strip_next, from);
    }

    /// Start a part (or change to another one): it is put on the mesh with its first numbers.
    pub fn attach_start(&mut self, mesh: &mut AssetMesh, part: attach::Part) {
        if !self.ensure_strip(mesh) {
            self.message = Some("This mesh has no texture, so the colours of a part have no place. Unwrap it first (the Edit page).".into());
            return;
        }
        let (v0, t0) = match self.attach {
            Some((_, _, v, t)) => (v, t),
            None => (mesh.verts.len(), mesh.tris.len()),
        };
        self.attach = Some((part, part.defaults(), v0, t0));
        self.attach_apply(mesh);
    }

    /// Keep the part: it is a part of the mesh now (it can be taken back like a stroke).
    pub fn attach_keep(&mut self, mesh: &AssetMesh) {
        if let Some((_, _, v0, t0)) = self.attach.take() {
            if mesh.verts.len() > v0 {
                self.geo_undo.push((v0, t0));
                self.strip_next += 1;
            }
        }
    }

    /// Take the part that is being shaped away.
    pub fn attach_drop(&mut self, mesh: &mut AssetMesh) {
        if let Some((_, _, v0, t0)) = self.attach.take() {
            mesh.verts.truncate(v0);
            mesh.uvs.truncate(v0);
            mesh.tris.truncate(t0);
            self.rig.weights.truncate(v0);
            self.version += 1;
        }
    }

    /// Take the last stroke of the mesh brush back.
    fn undo_stroke(&mut self, mesh: &mut AssetMesh) {
        if let Some((v, t)) = self.geo_undo.pop() {
            mesh.verts.truncate(v);
            mesh.uvs.truncate(v);
            mesh.tris.truncate(t);
            self.rig.weights.truncate(v);
            self.strip_next = self.strip_next.saturating_sub(1);
            self.near = None;
            self.version += 1;
        }
    }

    /// Cut a side of the mesh away (what is left floating in pieces stays; `keep_biggest` takes it).
    fn cut_mesh(&mut self, mesh: &mut AssetMesh, keep_biggest: bool) {
        self.cut_undo.push((mesh.clone(), self.rig.weights.clone()));
        if self.cut_undo.len() > 3 {
            self.cut_undo.remove(0);
        }
        let (new, map) = if keep_biggest { meshedit::keep_biggest(mesh) } else { meshedit::cut_plane(mesh, self.cut) };
        let mut weights = vec![Vec::new(); new.verts.len()];
        if self.rig.weights.len() == map.len() {
            for (old, to) in map.iter().enumerate() {
                if let Some(j) = to {
                    weights[*j as usize] = self.rig.weights[old].clone();
                }
            }
        }
        *mesh = new;
        self.rig.weights = weights;
        self.geo_undo.clear();
        self.painted = true;
        self.near = None;
        self.version += 1;
    }

    fn undo_cut(&mut self, mesh: &mut AssetMesh) {
        if let Some((m, w)) = self.cut_undo.pop() {
            *mesh = m;
            self.rig.weights = w;
            self.geo_undo.clear();
            self.near = None;
            self.version += 1;
        }
    }

    /// A bone was turned or the figure moved: when moving a bone keys it, the pose is kept as a key at this frame.
    fn pose_edited(&mut self, bone: usize) {
        if !(self.animating && self.auto_key) {
            return;
        }
        let frame = self.frame.round() as usize;
        let rot = self.pose.rot.get(bone).copied().unwrap_or([0.0; 3]);
        let bones = self.rig.bones.len();
        let t = self.current_take();
        t.fit(bones);
        t.set_key(bone, frame, rot);
    }

    /// The take that is shown (there is always one).
    pub fn current_take(&mut self) -> &mut Take {
        if self.takes.is_empty() {
            self.takes.push(Take::new("Take 1", self.rig.bones.len()));
        }
        self.take = self.take.min(self.takes.len() - 1);
        &mut self.takes[self.take]
    }

    /// The pose of the take at the frame is the pose that is shown.
    fn show_frame(&mut self) {
        let f = self.frame;
        let bones = self.rig.bones.len();
        let pose = {
            let t = self.current_take();
            t.fit(bones);
            t.pose_at(f)
        };
        self.pose = pose;
        self.version += 1;
    }

    /// The bones changed (added, taken away, renamed): the keys of the takes follow the bones by their names.
    fn sync_takes(&mut self) {
        let names: Vec<String> = self.rig.bones.iter().map(|b| b.name.clone()).collect();
        if names != self.take_names {
            if !self.take_names.is_empty() {
                let old = std::mem::take(&mut self.take_names);
                let rig = self.rig.clone();
                for t in &mut self.takes {
                    t.remap(&old, &rig);
                }
            }
            self.take_names = names;
            self.pose.rot.resize(self.rig.bones.len(), [0.0; 3]);
        }
    }

    /// Every take that has a key, as poses for a file.
    pub fn take_anims(&self) -> Vec<crate::take::Anim> {
        self.takes.iter().filter(|t| t.key_count() > 0).map(|t| t.bake()).collect()
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
                        self.mirror_weights(mesh, &cam, now, mode);
                    }
                }
                Grab::Texture => {
                    if let Some(now) = now {
                        self.paint_texture(mesh, &cam, now);
                        if let (true, Some(m)) = (self.mirror, self.mirror_pixel(mesh, &cam, now)) {
                            self.paint_texture(mesh, &cam, m);
                        }
                    }
                }
                Grab::Parts => {
                    if let (Some(now), Some(bone)) = (now, self.sel) {
                        let points: Vec<usize> = self.verts_under(mesh, &cam, now, self.radius).into_iter().map(|p| p.0).collect();
                        self.rig.assign_part(&points, bone);
                        self.mirror_part(mesh, &cam, now, bone);
                        self.version += 1;
                    }
                }
                Grab::Sculpt => {
                    if let Some(now) = now {
                        if self.sculpt == Sculpt::Grab {
                            self.sculpt_drag(mesh, &cam, now);
                        } else {
                            let opposite = mods.command;
                            self.sculpt_dab(mesh, &cam, now, opposite);
                            if let (true, Some(m)) = (self.mirror, self.mirror_pixel(mesh, &cam, now)) {
                                self.sculpt_dab(mesh, &cam, m, opposite);
                            }
                        }
                    }
                }
                Grab::Mesh => {
                    if let Some(now) = now {
                        if self.stroke.last().is_none_or(|l| (*l - now).length() > 2.0) {
                            self.stroke.push(now);
                        }
                    }
                }
                Grab::Lasso => {
                    if let Some(now) = now {
                        if self.lasso.last().is_none_or(|l| (*l - now).length() > 3.0) {
                            self.lasso.push(now);
                        }
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
                            self.pose_edited(i);
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
            if self.grab == Grab::Lasso {
                self.finish_lasso(mesh, &cam);
            }
            if self.grab == Grab::Sculpt {
                self.held.clear();
                self.sculpt_normals.clear();
            }
            if self.grab == Grab::Mesh {
                if let Some(now) = ui.input(|i| i.pointer.interact_pos()).map(local) {
                    self.stroke.push(now);
                }
                self.finish_stroke(mesh, &cam);
            }
            self.grab = Grab::None;
            self.draw_to = None;
            self.lasso.clear();
        }
        if resp.clicked_by(egui::PointerButton::Primary) && matches!(self.tool, Tool::Select | Tool::Pose | Tool::Weights | Tool::Parts | Tool::Lasso) {
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
                } else if self.tool == Tool::Sculpt {
                    self.undo_sculpt(mesh);
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
        let parts_tint = matches!(self.tool, Tool::Parts | Tool::Lasso);
        let key = [
            w as i64,
            h as i64,
            (self.yaw * 10.0) as i64,
            (self.pitch * 10.0) as i64,
            (self.zoom * 1000.0) as i64,
            (self.pan[0] * 4.0) as i64 * 100_000 + (self.pan[1] * 4.0) as i64,
            self.version as i64,
            self.sel.map(|s| s as i64).unwrap_or(-1),
            (tinting as i64) | ((parts_tint as i64) << 1),
            self.wire as i64,
            self.posed() as i64,
            mesh.verts.len() as i64,
        ];
        if self.view.as_ref().map(|v| v.0) != Some(key) {
            let verts = if self.posed() { self.rig.skin(&mesh.verts, &self.pose) } else { mesh.verts.clone() };
            let tint: Option<Vec<[u8; 3]>> = if parts_tint && self.rig.weights.len() == mesh.verts.len() {
                Some((0..mesh.verts.len()).map(|i| part_color(self.rig.main_bone(i), self.sel)).collect())
            } else {
                None
            }
            .or(match (tinting, self.sel) {
                (true, Some(b)) if self.rig.weights.len() == mesh.verts.len() => Some((0..mesh.verts.len()).map(|i| heat(self.rig.weight_of(i, b))).collect()),
                _ => None,
            });
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
        if self.stroke.len() > 1 {
            let pts: Vec<Pos2> = self.stroke.iter().map(|p| to_screen(p.x, p.y)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(3.0, EMERALD)));
        }
        if self.lasso.len() > 1 {
            let pts: Vec<Pos2> = self.lasso.iter().map(|p| to_screen(p.x, p.y)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, GOLD)));
        }
        if let (Grab::Draw { from, .. }, Some(to)) = (self.grab, self.draw_to) {
            painter.line_segment([screen(from), to], Stroke::new(2.0, EMERALD));
            painter.circle_filled(screen(from), 4.0, EMERALD);
        }
        if matches!(self.tool, Tool::Weights | Tool::Texture | Tool::Parts | Tool::Sculpt) {
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
            Tool::Parts => "Paint the part of the mesh that moves with the picked bone. Right button: turn the view. Wheel: zoom.",
            Tool::Lasso => "Draw a loop round a part: all of it moves with the picked bone. Right button: turn the view. Wheel: zoom.",
            Tool::Mesh => "Draw over the mesh: the stroke becomes hair, a rope, a strip or a spike. Right button: turn the view. Wheel: zoom.",
            Tool::Attach => "Horns, wings, a tail or ears with numbers. Right button: turn the view. Wheel: zoom.",
            Tool::Sculpt => "Push, pull, smooth or grab the mesh. Ctrl: the other way. Right button: turn the view. Wheel: zoom.",
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
                let mode = if mods.command { Paint::Subtract } else if mods.shift { Paint::Smooth } else { self.mode };
                self.paint_at(mesh, cam, at, mode);
                self.mirror_weights(mesh, cam, at, mode);
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
                if let (true, Some(m)) = (self.mirror, self.mirror_pixel(mesh, cam, at)) {
                    self.paint_texture(mesh, cam, m);
                }
                Grab::Texture
            }
            Tool::Attach => Grab::None,
            Tool::Sculpt => {
                self.sculpt_undo.push(mesh.verts.clone());
                if self.sculpt_undo.len() > 6 {
                    self.sculpt_undo.remove(0);
                }
                self.sculpt_normals = crate::meshfile::vertex_normals(&mesh.verts, &mesh.tris);
                if self.sculpt == Sculpt::Grab {
                    self.sculpt_hold(mesh, cam, at);
                } else {
                    let opposite = mods.command;
                    self.sculpt_dab(mesh, cam, at, opposite);
                    if let (true, Some(m)) = (self.mirror, self.mirror_pixel(mesh, cam, at)) {
                        self.sculpt_dab(mesh, cam, m, opposite);
                    }
                }
                Grab::Sculpt
            }
            Tool::Mesh => {
                if !self.ensure_strip(mesh) {
                    self.message = Some("This mesh has no texture, so the colours of a stroke have no place. Unwrap it first (the Edit page).".into());
                    return Grab::None;
                }
                self.stroke = vec![at];
                Grab::Mesh
            }
            Tool::Parts | Tool::Lasso => {
                let Some(bone) = self.sel else {
                    self.message = Some("Pick a bone first (click it, or pick it in the list), then paint its part.".into());
                    return Grab::None;
                };
                self.checkpoint();
                if self.tool == Tool::Lasso {
                    self.lasso = vec![at];
                    return Grab::Lasso;
                }
                let points: Vec<usize> = self.verts_under(mesh, cam, at, self.radius).into_iter().map(|p| p.0).collect();
                self.rig.assign_part(&points, bone);
                self.mirror_part(mesh, cam, at, bone);
                self.version += 1;
                Grab::Parts
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

    fn tools_ui(&mut self, ui: &mut Ui, mesh: &mut AssetMesh, set: ToolSet) {
        section_title(ui, "Tool");
        ui.horizontal_wrapped(|ui| {
            for (t, label, tip) in [
                (Tool::Select, "Pick and move", "Pick a bone, drag its ends or its middle to move it"),
                (Tool::Draw, "Draw bones", "Drag in the picture to draw a bone. Start at the end of a bone to continue the chain"),
                (Tool::Weights, "Paint weights", "Paint which bone moves which part of the mesh"),
                (Tool::Texture, "Paint texture", "Redraw the colours of the mesh, with colours picked from its own texture"),
                (Tool::Sculpt, "Sculpt", "Push, pull, smooth and grab the mesh, like the sculpt mode of Blender"),
                (Tool::Attach, "Add parts", "Horns, wings, a tail, ears: with numbers you change"),
                (Tool::Mesh, "Draw mesh", "Draw hair, ropes, strips and spikes on the mesh with brushes"),
                (Tool::Parts, "Paint parts", "Paint which part of the mesh moves with the picked bone: the whole part, no weights"),
                (Tool::Lasso, "Loop a part", "Draw a loop round a part: all of it moves with the picked bone"),
                (Tool::Pose, "Pose", "Turn bones to see how the mesh bends"),
                (Tool::Turn, "Turn view", "The left button turns the view (for a pen or tablet)"),
            ]
            .into_iter()
            .filter(|(t, _, _)| set.allows(*t))
            {
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
        if matches!(self.tool, Tool::Texture | Tool::Mesh | Tool::Weights | Tool::Parts | Tool::Lasso | Tool::Sculpt) {
            ui.checkbox(&mut self.mirror, "Mirror left and right").help("What you paint or draw on one side is done on the other side too, across the middle of the body. For the weights and the parts, the bone on the other side (Left and Right in the names) gets the mirror image. Both sides have to be seen in the picture.");
        }
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
            Tool::Sculpt => {
                section_title(ui, "Sculpt");
                ui.horizontal_wrapped(|ui| {
                    for s in Sculpt::ALL {
                        if widgets::chip(ui, s.label(), self.sculpt == s).help(s.about()).clicked() {
                            self.sculpt = s;
                        }
                    }
                });
                ui.add(egui::Slider::new(&mut self.radius, 5.0..=250.0).text("Size")).help("The size of the brush in pixels");
                ui.add(egui::Slider::new(&mut self.sculpt_strength, 0.05..=1.0).text("Strength"));
                if ui.add_enabled(!self.sculpt_undo.is_empty(), egui::Button::new("Take the last stroke back")).help("Ctrl+Z").clicked() {
                    self.undo_sculpt(mesh);
                }
                ui.label(RichText::new("It works on what is seen: turn the mesh to reach the other side. Ctrl while you draw does the other way. The bones and the weights stay as they are.").size(11.0).color(TEXT_DIM));
            }
            Tool::Attach => {
                section_title(ui, "Parts with numbers");
                ui.label(RichText::new("Pick a part: it is put on the body. Change the numbers until it is right, then keep it. Colours: the root and end colours of the mesh brushes.").size(11.0).color(TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    for part in attach::Part::ALL {
                        let on = self.attach.is_some_and(|a| a.0 == part);
                        if widgets::chip(ui, part.label(), on).help("Put this part on the body").clicked() {
                            self.attach_start(mesh, part);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Root").size(12.0).color(TEXT_DIM));
                    let a = ui.color_edit_button_srgb(&mut self.brush.root).changed();
                    ui.label(RichText::new("End").size(12.0).color(TEXT_DIM));
                    let b = ui.color_edit_button_srgb(&mut self.brush.tip).changed();
                    if (a || b) && self.attach.is_some() {
                        self.attach_apply(mesh);
                    }
                });
                if let Some((part, mut p, v0, t0)) = self.attach {
                    let mut changed = false;
                    if matches!(part, attach::Part::Horns | attach::Part::Wings) {
                        let (lo, hi) = if part == attach::Part::Horns { (1usize, 4usize) } else { (3, 24) };
                        changed |= ui.add(egui::Slider::new(&mut p.count, lo..=hi).text(if part == attach::Part::Horns { "How many on a side" } else { "Feathers on a wing" })).changed();
                    }
                    changed |= ui.add(egui::Slider::new(&mut p.size, 0.3..=3.0).text("Size")).changed();
                    changed |= ui.add(egui::Slider::new(&mut p.thick, 0.3..=3.0).text(if matches!(part, attach::Part::Wings | attach::Part::Ears) { "Wide" } else { "Thick" })).changed();
                    changed |= ui.add(egui::Slider::new(&mut p.curl, -1.0..=1.0).text("Curl")).changed();
                    if part != attach::Part::Tail {
                        changed |= ui.add(egui::Slider::new(&mut p.spread, 0.0..=90.0).text(if part == attach::Part::Wings { "Raised" } else { "Stands out" }).suffix(" deg")).changed();
                        changed |= ui.add(egui::Slider::new(&mut p.apart, 0.3..=2.5).text("Apart")).changed();
                    }
                    changed |= ui.add(egui::Slider::new(&mut p.up, -0.1..=0.1).text("Up")).changed();
                    changed |= ui.add(egui::Slider::new(&mut p.back, -0.1..=0.1).text("Back")).changed();
                    if changed {
                        self.attach = Some((part, p, v0, t0));
                        self.attach_apply(mesh);
                    }
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("Keep it").strong()).clicked() {
                            self.attach_keep(mesh);
                        }
                        if ui.button("Take it away").clicked() {
                            self.attach_drop(mesh);
                        }
                    });
                }
                if ui.add_enabled(!self.geo_undo.is_empty() && self.attach.is_none(), egui::Button::new("Take the last part or stroke back")).clicked() {
                    self.undo_stroke(mesh);
                }
            }
            Tool::Mesh => {
                section_title(ui, "Mesh brushes");
                ui.horizontal_wrapped(|ui| {
                    for k in BrushKind::ALL {
                        if widgets::chip(ui, k.label(), self.brush.kind == k).help(k.about()).clicked() {
                            self.brush = self.brush.with_kind(k);
                        }
                    }
                });
                ui.add(egui::Slider::new(&mut self.brush.width, 0.002..=0.2).logarithmic(true).text("Width").suffix(" m")).help("How wide or thick the stroke is");
                ui.add(egui::Slider::new(&mut self.brush.taper, 0.0..=1.0).text("Thinner to the end")).help("0: the same all along, 1: a point at the end");
                ui.add(egui::Slider::new(&mut self.brush.lift, 0.0..=0.05).text("Off the surface").suffix(" m")).help("How far the stroke floats over the surface");
                ui.add(egui::Slider::new(&mut self.brush.droop, 0.0..=0.5).text("Falls at the end").suffix(" m")).help("The end of the stroke hangs down, like hair");
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Root").size(12.0).color(TEXT_DIM));
                    ui.color_edit_button_srgb(&mut self.brush.root);
                    ui.label(RichText::new("End").size(12.0).color(TEXT_DIM));
                    ui.color_edit_button_srgb(&mut self.brush.tip);
                });
                if ui.add_enabled(!self.geo_undo.is_empty(), egui::Button::new("Take the last stroke back")).clicked() {
                    self.undo_stroke(mesh);
                }
                ui.label(RichText::new(format!("{} strokes drawn. Drag over the mesh, from the root outwards. The colours are painted in a strip of the texture, and the new parts follow the bone under their start.", self.geo_undo.len())).size(11.0).color(TEXT_DIM));
            }
            Tool::Parts | Tool::Lasso => {
                section_title(ui, "Parts");
                ui.label(RichText::new("Every colour is the part of one bone. Pick a bone (click it, or in the list), then paint its part or loop it. The whole part moves with that bone.").size(11.0).color(TEXT_DIM));
                ui.add(egui::Slider::new(&mut self.radius, 5.0..=200.0).text("Size")).help("The size of the brush in pixels");
                if ui.button("Every point to the nearest bone").help("A start: every point of the mesh moves with the bone that is nearest to it. Then paint over what is wrong.").clicked() {
                    self.checkpoint();
                    self.rig.assign_nearest_parts(&mesh.verts);
                    self.version += 1;
                }
                if ui.add_enabled(self.sel.is_some(), egui::Button::new("Take the picked part away")).help("The picked bone moves nothing").clicked() {
                    if let Some(b) = self.sel {
                        self.checkpoint();
                        self.rig.clear_bone(b);
                        self.version += 1;
                    }
                }
                ui.add_space(4.0);
                ui.add(egui::Slider::new(&mut self.soften, 1..=8).text("Passes")).help("How many times the points at a joint take some of the bone next to them");
                if ui.button("Soften the joints").help("So the surface bends at a joint and does not tear: the points at the border of two parts share their two bones a little. The parts stay parts.").clicked() {
                    self.checkpoint();
                    if self.near.is_none() {
                        self.near = Some(neighbours(&mesh.verts, &mesh.tris));
                    }
                    let near = self.near.take().unwrap_or_default();
                    self.rig.soften(&near, self.soften, 0.5);
                    self.near = Some(near);
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
                            self.pose_edited(i);
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
        if set == ToolSet::Look {
            ui.add_space(6.0);
            section_title(ui, "Cut away");
            ui.label(RichText::new("Cut a part off the mesh (the wings of a base, a tail): everything on one side of a plane goes.").size(11.0).color(TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                for (axis, label) in [(0usize, "Sideways"), (1, "Up"), (2, "Front")] {
                    if widgets::chip(ui, label, self.cut.axis == axis).clicked() {
                        self.cut.axis = axis;
                        let (lo, hi) = (self.bounds.0[axis], self.bounds.1[axis]);
                        self.cut.limit = (lo + hi) / 2.0;
                    }
                }
            });
            let (lo, hi) = (self.bounds.0[self.cut.axis], self.bounds.1[self.cut.axis]);
            ui.add(egui::Slider::new(&mut self.cut.limit, lo..=hi).text("Where").suffix(" m"));
            ui.horizontal(|ui| {
                if widgets::chip(ui, "Cut away the lower side", !self.cut.above).clicked() {
                    self.cut.above = false;
                }
                if widgets::chip(ui, "the higher side", self.cut.above).clicked() {
                    self.cut.above = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Cut").help("Take that side away").clicked() {
                    self.cut_mesh(mesh, false);
                }
                if ui.button("Keep the biggest piece").help("What the cut left floating goes").clicked() {
                    self.cut_mesh(mesh, true);
                }
                if ui.add_enabled(!self.cut_undo.is_empty(), egui::Button::new("Take the cut back")).clicked() {
                    self.undo_cut(mesh);
                }
            });
            ui.label(RichText::new("The axes are those of the mesh: it faces the front (+z). Look at it from the side to see where to cut.").size(11.0).color(TEXT_DIM));
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

    /// The left panel of the Animate page: the takes, playing, keys, the pose of the picked bone and the move of the figure.
    fn animate_ui(&mut self, ui: &mut Ui) {
        self.pose.rot.resize(self.rig.bones.len(), [0.0; 3]);
        self.current_take();
        section_title(ui, "Animation");
        ui.horizontal_wrapped(|ui| {
            for i in 0..self.takes.len() {
                if widgets::chip(ui, &self.takes[i].name.clone(), self.take == i).help("Show this animation").clicked() {
                    self.take = i;
                    self.frame = 0.0;
                    self.show_frame();
                }
            }
            if ui.small_button("New").help("Another animation").clicked() {
                let n = self.takes.len() + 1;
                self.takes.push(Take::new(&format!("Take {n}"), self.rig.bones.len()));
                self.take = self.takes.len() - 1;
                self.frame = 0.0;
                self.show_frame();
            }
        });
        let bones = self.rig.bones.len();
        let mut changed = false;
        {
            let t = self.current_take();
            t.fit(bones);
            ui.add(egui::TextEdit::singleline(&mut t.name).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Frames").size(12.0).color(TEXT_DIM));
                changed |= ui.add(egui::DragValue::new(&mut t.frames).range(2..=2400).speed(1.0)).changed();
                for f in [24.0f32, 30.0, 60.0] {
                    if widgets::chip(ui, &format!("{f} fps"), t.fps == f).help("How many frames a second").clicked() {
                        t.fps = f;
                    }
                }
            });
            ui.checkbox(&mut t.looping, "It loops").help("It starts again when it ends (walking, breathing)");
            ui.label(RichText::new(format!("{:.2} seconds, {} keys", t.seconds(), t.key_count())).size(11.5).color(TEXT_DIM));
        }
        if self.takes.len() > 1 && ui.small_button("Delete this animation").clicked() {
            self.takes.remove(self.take);
            self.take = self.take.saturating_sub(1);
            changed = true;
        }
        if changed {
            self.frame = self.frame.min(self.current_take().frames as f32);
            self.show_frame();
        }
        ui.add_space(6.0);
        section_title(ui, "Play");
        ui.horizontal(|ui| {
            if ui.button(if self.playing { "Pause" } else { "Play" }).help("Play the animation on the mesh").clicked() {
                self.playing = !self.playing;
                if self.playing && self.frame >= self.current_take().frames as f32 {
                    self.frame = 0.0;
                }
            }
            if ui.button("Start").help("Go back to the first frame").clicked() {
                self.playing = false;
                self.frame = 0.0;
                self.show_frame();
            }
        });
        ui.add_space(6.0);
        section_title(ui, "Keys");
        ui.checkbox(&mut self.auto_key, "Key the bone when I move it").help("Turning a bone sets a key for it at this frame");
        let frame = self.frame.round() as usize;
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.sel.is_some(), egui::Button::new("Key this bone")).help("Keep the pose of the picked bone at this frame").clicked() {
                if let Some(b) = self.sel {
                    let rot = self.pose.rot.get(b).copied().unwrap_or([0.0; 3]);
                    self.current_take().set_key(b, frame, rot);
                }
            }
            if ui.button("Key every bone").help("Keep the whole pose at this frame").clicked() {
                let (rots, off) = (self.pose.rot.clone(), self.pose.offset);
                let t = self.current_take();
                for (b, r) in rots.iter().enumerate() {
                    t.set_key(b, frame, *r);
                }
                t.set_move(frame, off);
            }
            if ui.button("Take the keys of this frame away").clicked() {
                self.current_take().remove_keys_at(frame);
                self.show_frame();
            }
            if ui.button("Stand straight").help("Every bone back to its place (then key it to start an animation from the rest)").clicked() {
                self.pose = Pose { rot: vec![[0.0; 3]; self.rig.bones.len()], offset: [0.0; 3] };
                self.version += 1;
            }
        });
        ui.add_space(6.0);
        section_title(ui, "The picked bone");
        match self.sel {
            Some(i) if i < self.pose.rot.len() => {
                ui.label(RichText::new(&self.rig.bones[i].name).size(12.5).strong());
                let mut moved = false;
                for (k, axis) in ["Bend (x)", "Turn (y)", "Tilt (z)"].into_iter().enumerate() {
                    moved |= ui.add(egui::Slider::new(&mut self.pose.rot[i][k], -180.0..=180.0).text(axis).suffix(" deg")).changed();
                }
                if moved {
                    self.version += 1;
                    self.pose_edited(i);
                }
            }
            _ => {
                ui.label(RichText::new("Pick a bone in the picture or in the list, then drag it in the picture or use the sliders.").size(11.5).color(TEXT_DIM));
            }
        }
        ui.add_space(4.0);
        let mut moved = false;
        for (k, axis) in ["Move right", "Move up", "Move forward"].into_iter().enumerate() {
            moved |= ui.add(egui::Slider::new(&mut self.pose.offset[k], -1.5..=1.5).text(axis).suffix(" m")).changed();
        }
        if moved {
            self.version += 1;
            if self.auto_key {
                let off = self.pose.offset;
                self.current_take().set_move(frame, off);
            }
        }
        self.library_ui(ui);
        section_title(ui, "View");
        ui.horizontal_wrapped(|ui| {
            for (label, to) in [("Front", (0.0, 0.0)), ("Side", (90.0, 0.0)), ("Back", (180.0, 0.0))] {
                if ui.small_button(label).clicked() {
                    self.yaw = to.0;
                    self.pitch = to.1;
                }
            }
        });
        ui.label(RichText::new("Every animation that has keys is saved in the files (.glb and .fbx) with the ready animations.").size(11.0).color(TEXT_DIM));
    }

    /// The library of animations: the ones that were saved, and the ones that came with the app. They fit any character with bones of the same names.
    fn library_ui(&mut self, ui: &mut Ui) {
        if !self.lib_seen {
            self.lib = crate::animlib::all();
            self.lib_seen = true;
        }
        section_title(ui, "Library");
        ui.label(RichText::new("Animations are kept by the names of the bones, so one that was made for a character moves any other character that has bones with the same names.").size(11.0).color(TEXT_DIM));
        let have_keys = self.takes.get(self.take).is_some_and(|t| t.key_count() > 0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(have_keys, egui::Button::new("Save this in my library")).help("Keeps this animation in the folder of the app, for every project and every character").clicked() {
                let file = crate::animlib::TakeFile::from_take(&self.takes[self.take], &self.rig);
                self.lib_message = Some(match crate::animlib::save(&crate::animlib::user_folder(), &file) {
                    Ok(p) => format!("Saved in {}.", p.display()),
                    Err(e) => e,
                });
                self.lib_seen = false;
            }
            if let Some(src) = crate::animlib::source_folder() {
                if ui.add_enabled(have_keys, egui::Button::new("Put it in the installer")).help("Saves it in the animations folder of the source of the app: the installer takes that folder, so everyone who installs the app has this animation").clicked() {
                    let file = crate::animlib::TakeFile::from_take(&self.takes[self.take], &self.rig);
                    self.lib_message = Some(match crate::animlib::save(&src, &file) {
                        Ok(p) => format!("Saved in {}: the installer will have it.", p.display()),
                        Err(e) => e,
                    });
                    self.lib_seen = false;
                }
            }
            if ui.small_button("Look again").help("Look in the folders for new animations").clicked() {
                self.lib_seen = false;
            }
        });
        let mut use_it: Option<std::path::PathBuf> = None;
        let mut delete: Option<std::path::PathBuf> = None;
        for e in &self.lib {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}  ({} frames{})", e.name, e.frames, if e.source == crate::animlib::Source::BuiltIn { ", built in" } else { "" })).size(12.0));
                if ui.small_button("Use").help("Add it to this mesh as an animation you can play, change and save").clicked() {
                    use_it = Some(e.path.clone());
                }
                if e.source == crate::animlib::Source::Mine && ui.small_button("Delete").help("Take it out of your library").clicked() {
                    delete = Some(e.path.clone());
                }
            });
        }
        if self.lib.is_empty() {
            ui.label(RichText::new("Nothing in the library yet.").size(11.5).color(TEXT_DIM));
        }
        if let Some(p) = use_it {
            self.lib_message = Some(match crate::animlib::open(&p) {
                Ok(f) => {
                    let (take, found) = f.to_take(&self.rig);
                    let total = f.tracks.len();
                    if found == 0 && total > 0 {
                        format!("None of the {total} bones of this animation is in this mesh (the names must be the same).")
                    } else {
                        self.takes.push(take);
                        self.take = self.takes.len() - 1;
                        self.frame = 0.0;
                        self.show_frame();
                        format!("{}: {found} of {total} bones found.", f.name)
                    }
                }
                Err(e) => e,
            });
        }
        if let Some(p) = delete {
            let _ = std::fs::remove_file(p);
            self.lib_seen = false;
        }
        if let Some(m) = &self.lib_message {
            ui.label(RichText::new(m).size(11.0).color(TEXT_DIM));
        }
        ui.add_space(6.0);
    }

    /// The strip under the picture: the frames, the keys of the picked bone (gold) and of the others (grey), and the playhead. Click or drag to go to a frame.
    fn timeline_ui(&mut self, ui: &mut Ui) {
        self.current_take();
        let (frames, keys_of): (usize, Vec<bool>) = {
            let sel = self.sel;
            let t = &self.takes[self.take];
            (t.frames, (0..=t.frames).map(|f| sel.is_some_and(|b| t.has_key(b, f))).collect())
        };
        let any: Vec<bool> = (0..=frames).map(|f| self.takes[self.take].any_key_at(f)).collect();
        let h = 46.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click_and_drag());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 4.0, Color32::from_rgb(24, 32, 30));
        let x_of = |f: f32| rect.left() + 8.0 + (rect.width() - 16.0) * f / frames.max(1) as f32;
        for f in 0..=frames {
            let tall = f % 10 == 0;
            p.line_segment([Pos2::new(x_of(f as f32), rect.bottom() - if tall { 12.0 } else { 6.0 }), Pos2::new(x_of(f as f32), rect.bottom())], Stroke::new(1.0, TEXT_DIM));
            if tall {
                p.text(Pos2::new(x_of(f as f32) + 2.0, rect.bottom() - 14.0), egui::Align2::LEFT_BOTTOM, format!("{f}"), egui::FontId::proportional(10.0), TEXT_DIM);
            }
            if any[f] {
                p.circle_filled(Pos2::new(x_of(f as f32), rect.top() + 12.0), 3.5, Color32::from_gray(130));
            }
            if keys_of[f] {
                p.circle_filled(Pos2::new(x_of(f as f32), rect.top() + 12.0), 5.0, GOLD);
            }
        }
        let head = x_of(self.frame);
        p.line_segment([Pos2::new(head, rect.top()), Pos2::new(head, rect.bottom())], Stroke::new(2.0, EMERALD));
        if let (true, Some(pos)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
            let f = ((pos.x - rect.left() - 8.0) / (rect.width() - 16.0) * frames as f32).round().clamp(0.0, frames as f32);
            if (f - self.frame).abs() > 0.4 {
                self.playing = false;
                self.frame = f;
                self.show_frame();
            }
        }
        ui.label(RichText::new(format!("Frame {} of {}", self.frame.round() as usize, frames)).size(11.5).color(TEXT_DIM));
    }

    /// The time goes on while it plays.
    fn advance(&mut self, dt: f32) {
        if !self.playing {
            return;
        }
        let (frames, fps, looping) = {
            let t = self.current_take();
            (t.frames as f32, t.fps, t.looping)
        };
        self.frame += dt * fps;
        if self.frame > frames {
            if looping {
                self.frame = self.frame.rem_euclid(frames.max(1.0));
            } else {
                self.frame = frames;
                self.playing = false;
            }
        }
        self.show_frame();
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
        if ui.add_enabled(!self.rig.bones.is_empty(), egui::Button::new(RichText::new("Save for Unreal (.fbx)").strong())).help("The same in an FBX file for Unreal Engine, Unity, Maya and Blender: mesh, skeleton, skin, texture and the animations. The texture is also saved next to it.").clicked() {
            *ask = Ask::SaveFbx;
        }
        if let Some(m) = &self.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
    }
}

impl SeviathApp {
    pub(super) fn mesh_rig_ui(&mut self, ui: &mut Ui) {
        if matches!(self.m3.page, Page::Bases | Page::Edit) {
            egui::Panel::left("mesh_bases_left").default_size(320.0).size_range(250.0..=480.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("mesh_bases_scroll").show(ui, |ui| if self.m3.page == Page::Edit { self.edit_panel(ui) } else { self.bases_panel(ui) });
            });
            if self.m3.mesh.is_none() {
                egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18)).inner_margin(egui::Margin::same(16))).show(ui, |ui| {
                    ui.label(RichText::new(if self.m3.page == Page::Edit { "There is no mesh yet. Make one on the From a text or From a picture pages, or pick a base." } else { "Pick a base on the left." }).size(14.0).strong());
                    ui.label(RichText::new("It is unwrapped for painting, and its looks (skin, eyes, brows, mouth) are drawn on it. Everything else you paint by hand.").size(12.0).color(TEXT_DIM));
                });
                return;
            }
        } else if self.m3.mesh.is_none() {
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
        let page = self.m3.page;
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let MeshUi { hand, mesh, .. } = &mut self.m3;
        let mesh = mesh.as_mut().expect("there is a mesh");
        hand.animating = page == Page::Animate;
        if hand.animating {
            hand.tool = Tool::Pose;
            hand.sync_takes();
            hand.pose.rot.resize(hand.rig.bones.len(), [0.0; 3]);
            if !hand.anim_shown {
                hand.anim_shown = true;
                hand.show_frame();
            }
            if hand.playing {
                hand.advance(dt);
                ui.ctx().request_repaint();
            }
        } else {
            hand.anim_shown = false;
            hand.playing = false;
        }
        let set = match page {
            Page::Parts => ToolSet::Parts,
            Page::Bases | Page::Edit => ToolSet::Look,
            _ => ToolSet::Rig,
        };
        if matches!(page, Page::Bases | Page::Edit) && !set.allows(hand.tool) {
            hand.tool = Tool::Turn;
        }
        if page == Page::Parts && !set.allows(hand.tool) {
            hand.tool = Tool::Parts;
        }
        if page == Page::Rig && !set.allows(hand.tool) {
            hand.tool = Tool::Select;
        }
        if !matches!(page, Page::Bases | Page::Edit) {
            egui::Panel::left("mesh_rig_left").default_size(300.0).size_range(240.0..=460.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("mesh_rig_left_scroll").show(ui, |ui| {
                    if page == Page::Animate {
                        hand.animate_ui(ui)
                    } else {
                        hand.tools_ui(ui, &mut *mesh, set)
                    }
                });
            });
        }
        egui::Panel::right("mesh_rig_right").default_size(290.0).size_range(240.0..=460.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("mesh_rig_right_scroll").show(ui, |ui| {
                if matches!(page, Page::Bases | Page::Edit) {
                    hand.tools_ui(ui, &mut *mesh, set);
                    ui.add_space(8.0);
                    section_title(ui, "Save");
                    if ui.button(RichText::new("Save the mesh (.glb)").strong()).help("The mesh with its texture, for Unity, Godot and Blender").clicked() {
                        ask = Ask::SaveMesh;
                    }
                    if ui.button("Save for Unreal (.fbx)").help("The mesh with its texture in an FBX file (the texture is saved next to it)").clicked() {
                        ask = Ask::SaveFbx;
                    }
                    ui.label(RichText::new("Next: the Parts page (no weights) or the Bones and weights page, to rig it.").size(11.0).color(TEXT_DIM));
                } else {
                    hand.bones_ui(ui, &mut ask)
                }
            });
        });
        if page == Page::Animate {
            egui::Panel::bottom("mesh_timeline").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(8))).show(ui, |ui| hand.timeline_ui(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18)).inner_margin(egui::Margin::same(6))).show(ui, |ui| hand.viewport(ui, &mut *mesh));
        match ask {
            Ask::Open => self.mesh_open_glb(),
            Ask::Save => self.mesh_save_rigged(),
            Ask::SaveFbx => self.mesh_save_fbx(),
            Ask::SaveMesh => self.mesh_save_plain(),
            Ask::None => {}
        }
    }

    /// The mesh with its texture, without bones.
    fn mesh_save_plain(&mut self) {
        let Some(mesh) = &self.m3.mesh else { return };
        let name = self.m3.file_name();
        let Some(p) = rfd::FileDialog::new().set_title("Save the mesh").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.glb")).add_filter("glTF binary", &["glb"]).save_file() else { return };
        let bytes = crate::meshfile::write_glb(mesh, None, &[], 30.0);
        self.m3.hand.message = Some(match std::fs::write(&p, bytes) {
            Ok(()) => format!("Saved {}.", p.display()),
            Err(e) => format!("Could not save the file: {e}"),
        });
    }

    /// The animations that go in a saved file: the ready ones that were added, and the ones made by hand that have keys.
    pub(super) fn mesh_all_anims(&self, with_takes: bool) -> Vec<crate::take::Anim> {
        let mut all = crate::take::anims_from_clips(&self.m3.hand.rig, &self.m3.anim.added, self.m3.anim.fps as f32);
        if with_takes {
            all.extend(self.m3.hand.take_anims());
        }
        all
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

    /// The rigged mesh is saved as an .fbx for Unreal, with the animations when asked, and its texture next to it.
    fn mesh_save_fbx(&mut self) {
        let Some(mesh) = &self.m3.mesh else { return };
        let name = self.m3.file_name();
        let Some(p) = rfd::FileDialog::new().set_title("Save for Unreal").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.fbx")).add_filter("FBX", &["fbx"]).save_file() else { return };
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(name);
        let anims = if self.m3.hand.with_animations { self.mesh_all_anims(true) } else { Vec::new() };
        let Some(mesh) = &self.m3.mesh else { return };
        let ex = crate::fbx::write_fbx_anims(mesh, Some(&self.m3.hand.rig), &anims, self.m3.anim.fps as f32, &stem);
        let mut res = std::fs::write(&p, &ex.fbx).map_err(|e| e.to_string());
        if let (Ok(()), Some(tex), Some(file)) = (&res, &mesh.texture, &ex.texture_name) {
            res = tex.save(p.with_file_name(file)).map_err(|e| e.to_string());
        }
        self.m3.hand.message = Some(match res {
            Ok(()) => format!("Saved {} for Unreal with {} bones{}.", p.display(), self.m3.hand.rig.bones.len(), if anims.is_empty() { String::new() } else { format!(" and {} animations", anims.len()) }),
            Err(e) => format!("Could not save the file: {e}"),
        });
    }

    /// The rigged mesh is saved as a .glb, with the animations when asked.
    fn mesh_save_rigged(&mut self) {
        let Some(mesh) = &self.m3.mesh else { return };
        let name = self.m3.file_name();
        let Some(p) = rfd::FileDialog::new().set_title("Save the rigged mesh").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}_rigged.glb")).add_filter("glTF binary", &["glb"]).save_file() else { return };
        let anims = if self.m3.hand.with_animations { self.mesh_all_anims(true) } else { Vec::new() };
        let Some(mesh) = &self.m3.mesh else { return };
        let bytes = crate::meshfile::write_glb_anims(mesh, Some(&self.m3.hand.rig), &anims);
        self.m3.hand.message = Some(match std::fs::write(&p, bytes) {
            Ok(()) => format!("Saved {} with {} bones{}.", p.display(), self.m3.hand.rig.bones.len(), if anims.is_empty() { String::new() } else { format!(" and {} animations", anims.len()) }),
            Err(e) => format!("Could not save the file: {e}"),
        });
    }
}

use super::meshtab::{MeshUi, Page};

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

    #[test]
    fn a_stroke_over_the_surface_becomes_geometry_with_its_colours_in_the_strip_and_can_be_taken_back() {
        let (mut mesh, _) = crate::meshfile::tests_figure();
        // a texture whose bottom eighth is the strip, and uvs that stay above it
        mesh.texture = Some(RgbaImage::from_pixel(256, 256, image::Rgba([200, 160, 140, 255])));
        for uv in &mut mesh.uvs {
            uv[1] *= 0.8;
        }
        let mut hand = HandRig::default();
        hand.start_for(&mesh);
        hand.strip_v0 = Some(0.875);
        let cam = hand.camera(200, 300);
        let (_, depth, ids) = mesh.render_full(&mesh.verts, &cam, false, crate::assetview::View::Textured, None);
        hand.depth = depth;
        hand.ids = ids;
        let (v0, t0) = (mesh.verts.len(), mesh.tris.len());
        hand.brush = hand.brush.with_kind(BrushKind::Card);
        hand.brush.root = [10, 10, 10];
        hand.brush.tip = [250, 250, 250];
        hand.stroke = (0..20).map(|i| Pos2::new(90.0 + i as f32 * 1.5, 80.0 + i as f32 * 6.0)).collect();
        hand.finish_stroke(&mut mesh, &cam);
        assert!(mesh.verts.len() > v0 && mesh.tris.len() > t0, "geometry was added");
        assert_eq!(mesh.uvs.len(), mesh.verts.len());
        assert_eq!(hand.rig.weights.len(), mesh.verts.len(), "the new points have weights");
        // the new points look into the strip, and the strip has the colours of the brush from the root to the end
        let new_v: Vec<f32> = mesh.uvs[v0..].iter().map(|q| q[1]).collect();
        assert!(new_v.iter().all(|v| *v >= 0.875 && *v <= 1.0), "{new_v:?}");
        let tex = mesh.texture.as_ref().unwrap();
        let (top, bottom) = (tex.get_pixel(16, 224 + 2).0, tex.get_pixel(16, 224 + 29).0);
        assert!(top[0] < 80 && bottom[0] > 150, "{top:?} {bottom:?}");
        hand.undo_stroke(&mut mesh);
        assert_eq!((mesh.verts.len(), mesh.tris.len(), hand.rig.weights.len()), (v0, t0, v0));
    }

    #[test]
    fn a_cut_takes_a_side_away_and_can_be_taken_back_with_the_weights() {
        let (mut mesh, _) = crate::meshfile::tests_figure();
        let mut hand = HandRig::default();
        hand.start_for(&mesh);
        hand.rig.add_bone("a", None, [0.0; 3], [0.0, 1.0, 0.0]);
        hand.rig.assign_nearest_parts(&mesh.verts);
        let before = mesh.verts.len();
        hand.cut = Cut { axis: 1, limit: 1.0, above: true };
        hand.cut_mesh(&mut mesh, false);
        assert!(mesh.verts.len() < before);
        assert_eq!(hand.rig.weights.len(), mesh.verts.len());
        assert!(mesh.verts.iter().all(|v| v[1] <= 1.0 + 1e-4));
        hand.undo_cut(&mut mesh);
        assert_eq!((mesh.verts.len(), hand.rig.weights.len()), (before, before));
    }

    #[test]
    fn a_part_is_built_changed_by_its_numbers_kept_and_taken_back() {
        let (mut mesh, _) = crate::meshfile::tests_figure();
        mesh.texture = Some(RgbaImage::from_pixel(256, 256, image::Rgba([200, 160, 140, 255])));
        for uv in &mut mesh.uvs {
            uv[1] *= 0.8;
        }
        let mut hand = HandRig::default();
        hand.start_for(&mesh);
        hand.strip_v0 = Some(0.875);
        let (v0, t0) = (mesh.verts.len(), mesh.tris.len());
        hand.attach_start(&mut mesh, attach::Part::Horns);
        let one = mesh.verts.len();
        assert!(one > v0 && mesh.tris.len() > t0);
        assert_eq!(hand.rig.weights.len(), mesh.verts.len());
        // other numbers: the part is built again, not added again
        let (part, mut p, a, b) = hand.attach.unwrap();
        p.count = 3;
        hand.attach = Some((part, p, a, b));
        hand.attach_apply(&mut mesh);
        assert!(mesh.verts.len() > one * 2 && mesh.verts.len() < one * 4, "{} {}", mesh.verts.len(), one);
        hand.attach_keep(&mesh);
        assert!(hand.attach.is_none() && hand.geo_undo.len() == 1);
        // wings on top of the horns, then thrown away
        let kept = mesh.verts.len();
        hand.attach_start(&mut mesh, attach::Part::Wings);
        assert!(mesh.verts.len() > kept);
        hand.attach_drop(&mut mesh);
        assert_eq!((mesh.verts.len(), hand.rig.weights.len()), (kept, kept));
        hand.undo_stroke(&mut mesh);
        assert_eq!((mesh.verts.len(), mesh.tris.len()), (v0, t0));
    }

    /// A figure with a texture and a strip, seen from the front, with the picture of ids made.
    fn mirror_setup() -> (AssetMesh, HandRig, Camera) {
        let (mut mesh, _) = crate::meshfile::tests_figure();
        mesh.texture = Some(RgbaImage::from_pixel(256, 256, image::Rgba([200, 160, 140, 255])));
        for uv in &mut mesh.uvs {
            uv[1] *= 0.8;
        }
        let mut hand = HandRig::default();
        hand.start_for(&mesh);
        hand.strip_v0 = Some(0.875);
        hand.mirror = true;
        let cam = hand.camera(300, 300);
        let (_, depth, ids) = mesh.render_full(&mesh.verts, &cam, false, crate::assetview::View::Textured, None);
        hand.depth = depth;
        hand.ids = ids;
        (mesh, hand, cam)
    }

    #[test]
    fn a_stroke_of_paint_is_done_on_the_other_side_too() {
        let (mut mesh, mut hand, cam) = mirror_setup();
        // the middle of the body is at x = 0: a point to the left of the middle
        let left = {
            let p = cam.project([-0.07, 1.0, 0.1]);
            Pos2::new(p[0], p[1])
        };
        let m = hand.mirror_pixel(&mesh, &cam, left).expect("the other side is seen");
        let back = cam.unproject([m.x, m.y, cam.project([0.07, 1.0, 0.1])[2]]);
        assert!((back[0] - 0.07).abs() < 0.01, "{back:?}: the mirror image is across x = 0");
        hand.color = [10, 200, 30];
        hand.tex_opacity = 1.0;
        hand.tex_radius = 6.0;
        let before = mesh.texture.clone().unwrap();
        hand.paint_texture(&mut mesh, &cam, left);
        let one_side = mesh.texture.as_ref().unwrap().pixels().zip(before.pixels()).filter(|(a, b)| a != b).count();
        let mut other = mesh.clone();
        other.texture = Some(before.clone());
        hand.paint_texture(&mut other, &cam, left);
        hand.paint_texture(&mut other, &cam, m);
        let both = other.texture.as_ref().unwrap().pixels().zip(before.pixels()).filter(|(a, b)| a != b).count();
        assert!(one_side > 0 && both > one_side, "{one_side} {both}");
    }

    #[test]
    fn mirrored_parts_and_strokes_go_to_the_bone_and_the_place_on_the_other_side() {
        let (mut mesh, mut hand, cam) = mirror_setup();
        let (lo, hi) = mesh.bounds();
        hand.rig = Rig3::humanoid(mesh.verts.len(), lo, hi);
        hand.rig.assign_nearest_parts(&mesh.verts);
        let left_arm = hand.rig.bones.iter().position(|b| b.name == "leftUpperArm").unwrap();
        let right_arm = hand.rig.bones.iter().position(|b| b.name == "rightUpperArm").unwrap();
        assert_eq!(hand.mirror_bone(left_arm), Some(right_arm));
        let hips = hand.rig.bones.iter().position(|b| b.name == "hips").unwrap();
        assert_eq!(hand.mirror_bone(hips), Some(hips), "a bone in the middle is its own mirror image");
        // painting the left arm part on the left of the body gives the right arm part on the right
        let p = cam.project([-0.09, 1.2, 0.1]);
        hand.radius = 14.0;
        hand.mirror_part_test(&mesh, &cam, Pos2::new(p[0], p[1]), left_arm);
        let right_points = (0..mesh.verts.len()).filter(|i| hand.rig.main_bone(*i) == Some(right_arm) && mesh.verts[*i][0] > 0.0).count();
        assert!(right_points > 0, "the other side got its part");
        // a stroke: the mirror image is one step, and takes both back
        let v0 = mesh.verts.len();
        hand.brush = hand.brush.with_kind(BrushKind::Card);
        hand.stroke = (0..16).map(|i| Pos2::new(cam.project([-0.09, 0.0, 0.1])[0] + i as f32, 90.0 + i as f32 * 6.0)).collect();
        hand.finish_stroke(&mut mesh, &cam);
        let added = mesh.verts.len() - v0;
        assert!(added > 0);
        let xs: Vec<f32> = mesh.verts[v0..].iter().map(|v| v[0]).collect();
        assert!(xs.iter().any(|x| *x < -0.03) && xs.iter().any(|x| *x > 0.03), "strokes on both sides");
        hand.undo_stroke(&mut mesh);
        assert_eq!(mesh.verts.len(), v0, "one step takes both back");
    }

    /// A flat grid facing +z, seen from the front.
    fn sculpt_setup() -> (AssetMesh, HandRig, Camera) {
        let mut verts = Vec::new();
        for y in 0..=20 {
            for x in 0..=20 {
                verts.push([x as f32 * 0.02 - 0.2, y as f32 * 0.02, 0.0]);
            }
        }
        let mut tris = Vec::new();
        for y in 0..20u32 {
            for x in 0..20u32 {
                let a = y * 21 + x;
                tris.push([a, a + 1, a + 21]);
                tris.push([a + 1, a + 22, a + 21]);
            }
        }
        let mesh = AssetMesh { uvs: vec![[0.0; 2]; verts.len()], verts, tris, texture: None, normal: None };
        let mut hand = HandRig::default();
        hand.start_for(&mesh);
        let cam = hand.camera(300, 300);
        let (_, depth, ids) = mesh.render_full(&mesh.verts, &cam, false, crate::assetview::View::Textured, None);
        hand.depth = depth;
        hand.ids = ids;
        hand.radius = 60.0;
        hand.sculpt_strength = 1.0;
        (mesh, hand, cam)
    }

    fn centre_px(cam: &Camera) -> Pos2 {
        let p = cam.project([0.0, 0.2, 0.0]);
        Pos2::new(p[0], p[1])
    }

    #[test]
    fn the_sculpt_brushes_push_smooth_and_grab_the_surface_and_a_stroke_can_be_taken_back() {
        let (mut mesh, mut hand, cam) = sculpt_setup();
        let mid = mesh.verts.iter().position(|v| v[0].abs() < 1e-4 && (v[1] - 0.2).abs() < 1e-4).unwrap();
        let z0: Vec<f32> = mesh.verts.iter().map(|v| v[2]).collect();
        hand.sculpt_undo.push(mesh.verts.clone());
        hand.sculpt_normals = crate::meshfile::vertex_normals(&mesh.verts, &mesh.tris);
        // draw: the middle comes out of the surface (towards the viewer, +z or -z by the winding), the far corner does not move
        hand.sculpt = Sculpt::Draw;
        hand.sculpt_dab(&mut mesh, &cam, centre_px(&cam), false);
        assert!(mesh.verts[mid][2].abs() > 0.005, "{}", mesh.verts[mid][2]);
        assert_eq!(mesh.verts[0][2], z0[0]);
        // the other way
        let up = mesh.verts[mid][2];
        hand.sculpt_dab(&mut mesh, &cam, centre_px(&cam), true);
        assert!(mesh.verts[mid][2].abs() < up.abs(), "ctrl cuts in");
        // smooth: a spike gets lower
        mesh.verts[mid][2] = 0.1;
        hand.sculpt = Sculpt::Smooth;
        hand.sculpt_dab(&mut mesh, &cam, centre_px(&cam), false);
        assert!(mesh.verts[mid][2] < 0.07, "{}", mesh.verts[mid][2]);
        // grab: hold, drag to the right, the middle follows
        hand.sculpt = Sculpt::Grab;
        let x0 = mesh.verts[mid][0];
        hand.sculpt_hold(&mesh, &cam, centre_px(&cam));
        hand.sculpt_drag(&mut mesh, &cam, Pos2::new(centre_px(&cam).x + 30.0, centre_px(&cam).y));
        assert!(mesh.verts[mid][0] > x0 + 0.02, "{} {}", mesh.verts[mid][0], x0);
        // back
        hand.undo_sculpt(&mut mesh);
        assert_eq!(mesh.verts.iter().map(|v| v[2]).collect::<Vec<_>>(), z0);
    }

    #[test]
    fn a_mirrored_sculpt_stroke_works_on_both_sides() {
        let (mut mesh, mut hand, cam) = sculpt_setup();
        hand.mirror = true;
        hand.mirror_x = 0.0;
        hand.sculpt_normals = crate::meshfile::vertex_normals(&mesh.verts, &mesh.tris);
        hand.sculpt = Sculpt::Inflate;
        hand.radius = 25.0;
        let left = {
            let p = cam.project([-0.1, 0.2, 0.0]);
            Pos2::new(p[0], p[1])
        };
        hand.sculpt_dab(&mut mesh, &cam, left, false);
        let m = hand.mirror_pixel(&mesh, &cam, left).unwrap();
        hand.sculpt_dab(&mut mesh, &cam, m, false);
        let moved = |side: f32| mesh.verts.iter().filter(|v| v[0] * side > 0.0 && v[2].abs() > 1e-4).count();
        assert!(moved(-1.0) > 0 && moved(1.0) > 0, "{} {}", moved(-1.0), moved(1.0));
    }

    #[test]
    fn a_generated_mesh_gets_a_free_strip_when_a_brush_needs_one() {
        let (mut mesh, _) = crate::meshfile::tests_figure();
        mesh.texture = Some(RgbaImage::from_pixel(256, 256, image::Rgba([200, 160, 140, 255])));
        let mut hand = HandRig::default();
        hand.start_for(&mesh);
        assert!(hand.strip_v0.is_none());
        assert!(hand.ensure_strip(&mut mesh));
        let v0 = hand.strip_v0.unwrap();
        assert!(v0 < 0.9 && v0 > 0.85, "{v0}");
        assert!(mesh.uvs.iter().all(|q| q[1] <= v0 + 1e-4));
        // a part can be put on it now
        hand.attach_start(&mut mesh, attach::Part::Horns);
        assert!(hand.attach.is_some() && mesh.verts.len() > 44);
        let high = mesh.texture.as_ref().unwrap().height() as f32;
        assert!(mesh.uvs[44..].iter().all(|q| q[1] >= v0 && q[1] <= 1.0), "the part looks into the strip of a texture that is taller than wide ({high})");
    }
}
