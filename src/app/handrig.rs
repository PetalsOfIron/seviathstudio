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
use crate::brush::BrushSettings;
use crate::brush::engine::{Sample as BrushSample, Stroke as BrushStroke};
use crate::doc::{Document, LayerKind, TILE};
use crate::meshbrush::{self, Brush, Curl, Kind as BrushKind, Spot};
use crate::meshedit::{self, Cut};
use crate::meshops;
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
    /// Pick faces of the mesh with a loop or a brush, then delete, extrude, duplicate or change them.
    Faces,
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
            ToolSet::Look => matches!(t, Tool::Texture | Tool::Faces | Tool::Mesh | Tool::Attach | Tool::Sculpt | Tool::Turn),
        }
    }
}

/// The brush of the Draw tab, as the 3D tab paints with it: every brush, the size, the shape, the dynamics, the texture of the brush and the colour are the ones
/// picked there.
#[derive(Clone)]
pub struct PaintKit {
    pub settings: BrushSettings,
    pub color: egui::Color32,
    pub erase: bool,
}

/// A stroke of the Draw brush on the texture: the texture is a canvas of its own (one layer), the engine paints on it as it does in the Draw tab, and the tiles
/// that changed are put back on the texture of the mesh as the stroke goes.
struct TexSession {
    doc: Document,
    settings: BrushSettings,
    color: egui::Color32,
    erase: bool,
    /// How many texels the size of the brush is per pixel of the picture.
    texels_per_pixel: f32,
    last: Pos2,
    /// One stroke for the pointer, one for its mirror image: the stroke, and where it was last (in texels).
    strokes: Vec<(Option<BrushStroke>, Option<[f32; 2]>)>,
    touched: std::collections::HashSet<(i32, i32)>,
}

/// A stroke that was drawn on the mesh (a strand of hair, a rope, a strip, a spike) or a part that was put on it: where its points are, the line it was drawn
/// along (to change its thickness and its length), and the bones that move it when it is hair.
#[derive(Clone, Debug)]
pub struct Strand {
    pub name: String,
    verts: (usize, usize),
    /// The first bone of its chain and how many bones there are, when it is hair.
    bones: Option<(usize, usize)>,
    /// The line it follows, from the root to the end (empty for a part).
    line: Vec<[f32; 3]>,
    pub width_mul: f32,
    /// The picture of hair it looks into (hair only).
    pub tex: Option<usize>,
}

/// The bones that are made with a drawn thing or a part (hair, a tail, a wing) and go where it goes.
fn is_part_bone(name: &str) -> bool {
    name.starts_with("hair") || name.starts_with("tail") || name.starts_with("wing")
}

/// The nearest place on a line of points (and how far along the line it is, as a share of its length is not needed: the segment and the place).
fn near_on_line(line: &[[f32; 3]], p: [f32; 3]) -> [f32; 3] {
    let mut best = (f32::MAX, line.first().copied().unwrap_or(p));
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l2 = (ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2]).max(1e-12);
        let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / l2).clamp(0.0, 1.0);
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
        let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2);
        if d < best.0 {
            best = (d, q);
        }
    }
    best.1
}

/// A picture of hair: a rectangle of the strip of the texture (the rectangle that VRoid has for its hair: across the strand left to right, the root at the top and
/// the end at the bottom). A line of hair covers the whole of it, and all the hair that is drawn with it looks the same.
#[derive(Clone, Debug)]
pub struct HairTex {
    pub name: String,
    slot: usize,
    ready: bool,
}

/// The width of a picture of hair, in texels.
const HAIR_W: u32 = 64;

/// The window in which the picture of the hair is drawn.
struct HairEditor {
    doc: Document,
    stroke: Option<BrushStroke>,
    handle: Option<TextureHandle>,
    /// Where the picture is on the texture of the mesh (x, y, width, height), and which picture of hair it is.
    region: (u32, u32, u32, u32),
    tex: usize,
}

/// How the faces are picked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickMode {
    /// Draw a loop round them.
    Loop,
    /// Paint over them.
    Brush,
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
    Faces,
    /// Dragging a strand about, or the end of one to make it longer or shorter.
    StrandMove(usize),
    StrandTip(usize),
    /// A shape is put where the pointer is (and follows it).
    ShapePlace,
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
    attach_label: Option<String>,
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
    /// The brush of the Draw tab (set every frame by the page), the session of a stroke, and how big the brush is on the mesh as a multiplier.
    pub kit: Option<PaintKit>,
    tex_session: Option<TexSession>,
    pub tex_size_mul: f32,
    /// The colour was changed here: the Draw tab takes it.
    pub color_dirty: bool,
    /// The picked points of the mesh, how they are picked, whether the back is picked too, and the numbers of what is done with them.
    pub selected: Vec<bool>,
    pub pick_mode: PickMode,
    pub through: bool,
    pub ext_dist: f32,
    pub push_dist: f32,
    pub scale_by: f32,
    pub smooth_passes: usize,
    /// Hair: the strokes of the mesh brush are rigged as hair (a chain of bones along the line), how many strands there are, where the bones of the last strokes
    /// began (to take them back), how the hair behaves, and the state of its motion.
    pub hair: bool,
    hair_count: usize,
    hair_marks: Vec<(usize, usize)>,
    pub hair_physics: bool,
    pub hair_params: crate::hairsim::HairParams,
    hair_sim: Option<crate::hairsim::HairSim>,
    hair_sig: usize,
    /// The strokes and parts that were drawn, which one is picked, and whether dragging moves it (and not draw a new one).
    pub strands: Vec<Strand>,
    pub strand_sel: Option<usize>,
    pub strand_move: bool,
    /// The picture of the hair has been put on the texture, and the window for drawing it.
    /// The pictures of hair that were made, and the one the hair brush draws with.
    pub hair_textures: Vec<HairTex>,
    pub hair_cur: usize,
    hair_editor: Option<HairEditor>,
    /// A shape (ball, disc...) that is being made: which, its numbers, where it is, and the sizes of the mesh from before it was put on.
    pub shape_pending: Option<(meshops::Shape, meshops::ShapeParams, [f32; 3], usize, usize)>,
    /// A shape goes where the mesh is clicked (and dragged) instead of the middle of the body.
    pub shape_place: bool,
    pub brush_window: bool,
    /// What fixing and fitting does: the biggest hole that is closed (in edges), how near to a joint the faces are cut smaller, how many times, and whether the
    /// files get quads.
    pub hole_edges: usize,
    pub joint_radius: f32,
    pub joint_levels: usize,
    pub quads: bool,
    pub quad_angle: f32,
    pub geometry_message: Option<String>,
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
            attach_label: None,
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
            kit: None,
            tex_session: None,
            tex_size_mul: 1.0,
            color_dirty: false,
            selected: Vec::new(),
            pick_mode: PickMode::Loop,
            through: false,
            ext_dist: 0.02,
            push_dist: 0.01,
            scale_by: 1.1,
            smooth_passes: 2,
            hair: false,
            hair_count: 0,
            hair_marks: Vec::new(),
            hair_physics: true,
            hair_params: crate::hairsim::HairParams::default(),
            hair_sim: None,
            hair_sig: 0,
            strands: Vec::new(),
            strand_sel: None,
            strand_move: false,
            hair_textures: Vec::new(),
            hair_cur: 0,
            hair_editor: None,
            shape_pending: None,
            shape_place: true,
            brush_window: false,
            hole_edges: 40,
            joint_radius: 0.12,
            joint_levels: 1,
            quads: false,
            quad_angle: 12.0,
            geometry_message: None,
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
        let w = cam.w as usize;
        let _ = v0;
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
                let keep = self.geo_undo.len();
                self.hair_marks.retain(|m| m.0 < keep);
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
        let bones_before = self.rig.bones.len();
        let Some(used_cell) = self.append_built(mesh, &built, self.strip_next, from) else { return false };
        self.geo_undo.push(old);
        if used_cell {
            self.strip_next += 1;
        }
        if self.hair {
            self.rig_hair(mesh, spots, &built, old.0, from, mirrored, bones_before);
        }
        // it is a stroke that can be picked, made thicker or thinner, longer or shorter, and moved
        let n = self.strands.len() + 1;
        let (kind, side) = (if self.hair { "Hair" } else { self.brush.kind.label() }, if mirrored { " (other side)" } else { "" });
        let bones = if self.hair && self.rig.bones.len() > bones_before { Some((bones_before, self.rig.bones.len() - bones_before)) } else { None };
        let tex = if self.hair { Some(self.hair_cur) } else { None };
        self.strands.push(Strand { name: format!("{kind} {n}{side}"), verts: (old.0, mesh.verts.len()), bones, line: meshbrush::joints(&self.brush, spots, 10).unwrap_or_default(), width_mul: 1.0, tex });
        self.strand_sel = Some(self.strands.len() - 1);
        true
    }

    /// A line of hair is rigged: a chain of bones along it (hanging from the bone that moves what is under its start), and the strand is skinned to the chain, so
    /// it bends along its length. The bones are called `hair_...`, which is how the hair is found when it is made to move.
    #[allow(clippy::too_many_arguments)]
    fn rig_hair(&mut self, mesh: &AssetMesh, spots: &[Spot], built: &meshbrush::Built, v0: usize, from: Option<usize>, mirrored: bool, bones_before: usize) {
        let pieces = ((meshbrush::length(spots) / 0.05).round() as usize).clamp(3, 8);
        let Some(joints) = meshbrush::joints(&self.brush, spots, pieces) else { return };
        let mut parent = from.and_then(|f| self.rig.main_bone(f));
        if mirrored {
            parent = parent.and_then(|p| self.mirror_bone(p));
        }
        if !mirrored {
            self.hair_count += 1;
        }
        let k = self.hair_count;
        let first = self.rig.bones.len();
        for i in 0..pieces {
            let name = format!("hair_{k}{}_{i}", if mirrored { "m" } else { "" });
            let up = if i == 0 { parent } else { Some(first + i - 1) };
            self.rig.add_bone(&name, up, joints[i], joints[i + 1]);
        }
        // the strand follows the bones of its chain: along it, each bone moves the stretch it hangs over, and two share a joint
        if self.rig.weights.len() == mesh.verts.len() {
            for (j, uv) in built.uvs.iter().enumerate() {
                let u = uv[1].clamp(0.0, 1.0) * pieces as f32;
                let i = (u.floor() as usize).min(pieces - 1);
                let f = u - i as f32;
                let next = if i + 1 < pieces { ((f - 0.55) / 0.45).clamp(0.0, 1.0).powi(2) * (3.0 - 2.0 * ((f - 0.55) / 0.45).clamp(0.0, 1.0)) } else { 0.0 };
                self.rig.weights[v0 + j] = if next > 0.0 { vec![((first + i) as u16, 1.0 - next), ((first + i + 1) as u16, next)] } else { vec![((first + i) as u16, 1.0)] };
            }
        }
        self.hair_marks.push((self.geo_undo.len() - 1, bones_before));
        let _ = mirrored;
        self.pose.rot.resize(self.rig.bones.len(), [0.0; 3]);
        self.hair_sim = None;
    }

    /// The geometry is put on the mesh: its colours (root to end) are painted in a cell of the strip of the texture (hair looks into a picture of hair instead), its
    /// texture coordinates look into that, and its points move with what the point `from` of the mesh moves with.
    fn append_built(&mut self, mesh: &mut AssetMesh, built: &meshbrush::Built, k: usize, from: Option<usize>) -> Option<bool> {
        let v0 = self.strip_v0?;
        let (size, high) = mesh.texture.as_ref().map(|t| (t.width(), t.height()))?;
        let y_start = (v0 * high as f32).ceil() as u32;
        let slots = Self::hair_slots(size, high, y_start);
        // hair: all the lines of hair look into the picture of hair that is picked; the other strokes have a cell of their own
        let hair_uv = self.hair && !slots.is_empty();
        let (cx, cy, cw, ch, used_cell) = if hair_uv {
            let idx = self.ensure_hair_texture(mesh)?;
            let (rx, ry, rw, rh) = slots[self.hair_textures[idx].slot];
            self.paint_hair_default(mesh, idx);
            (rx, ry, rw, rh, false)
        } else {
            let cells = Self::strip_cells(size, high, y_start, Self::hair_block(&slots));
            let Some(&(x0, y0)) = cells.get(k) else {
                self.message = Some("The strip of the texture for the colours of the strokes is full: take a stroke back, or use a bigger texture.".into());
                return None;
            };
            if let Some(tex) = mesh.texture.as_mut() {
                let img = meshbrush::cell_image(32, self.brush.root, self.brush.tip, k as u32 + 1);
                image::imageops::replace(tex, &img, x0 as i64, y0 as i64);
            }
            (x0, y0, 32, 32, true)
        };
        let base = mesh.verts.len() as u32;
        let old_len = mesh.verts.len();
        for (v, uv) in built.verts.iter().zip(&built.uvs) {
            mesh.verts.push(*v);
            mesh.uvs.push([(cx as f32 + 2.0 + uv[0] * (cw - 4) as f32) / size as f32, (cy as f32 + 1.0 + uv[1] * (ch - 2) as f32) / high as f32]);
        }
        mesh.tris.extend(built.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
        if self.rig.weights.len() == old_len {
            let w = from.and_then(|f| self.rig.weights.get(f)).cloned().unwrap_or_default();
            self.rig.weights.extend(std::iter::repeat_n(w, built.verts.len()));
        }
        self.painted = true;
        self.near = None;
        self.version += 1;
        Some(used_cell)
    }

    /// The places for pictures of hair: rectangles (`HAIR_W` texels wide, as tall as the strip lets them be, up to 256) at the right end of the strip, in the half of it
    /// that the cells of the strokes leave free. Empty when the strip is too small.
    fn hair_slots(size: u32, high: u32, y_start: u32) -> Vec<(u32, u32, u32, u32)> {
        let rows = high.saturating_sub(y_start);
        let h = ((rows / 32) * 32).min(256);
        if h < 64 || size < HAIR_W * 4 {
            return Vec::new();
        }
        let cols = (size / 2) / HAIR_W;
        let x0 = size - cols * HAIR_W;
        let rows_n = (rows / h).max(1);
        (0..rows_n).flat_map(|r| (0..cols).map(move |c| (x0 + c * HAIR_W, y_start + r * h, HAIR_W, h))).collect()
    }

    /// The block of the strip that all the places for hair take (the cells of the strokes keep out of it).
    fn hair_block(slots: &[(u32, u32, u32, u32)]) -> Option<(u32, u32, u32, u32)> {
        let (first, last) = (slots.first()?, slots.last()?);
        Some((first.0, first.1, last.0 + last.2 - first.0, last.1 + last.3 - first.1))
    }

    /// The origins of the cells of the strip (32 texels square), not the ones that are under the block of the hair.
    fn strip_cells(size: u32, high: u32, y_start: u32, block: Option<(u32, u32, u32, u32)>) -> Vec<(u32, u32)> {
        let (cols, rows) = ((size / 32) as usize, (high.saturating_sub(y_start) / 32) as usize);
        let mut out = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                let (x, y) = (c as u32 * 32, y_start + r as u32 * 32);
                let under = block.is_some_and(|(bx, by, bw, bh)| x + 32 > bx && x < bx + bw && y + 32 > by && y < by + bh);
                if !under {
                    out.push((x, y));
                }
            }
        }
        out
    }

    /// The rectangle of a picture of hair on the texture of the mesh.
    fn hair_rect(&self, mesh: &AssetMesh, idx: usize) -> Option<(u32, u32, u32, u32)> {
        let tex = mesh.texture.as_ref()?;
        let y_start = (self.strip_v0? * tex.height() as f32).ceil() as u32;
        let slot = self.hair_textures.get(idx)?.slot;
        Self::hair_slots(tex.width(), tex.height(), y_start).get(slot).copied()
    }

    /// There is always a picture of hair to draw with (the first one is made when it is needed).
    fn ensure_hair_texture(&mut self, mesh: &AssetMesh) -> Option<usize> {
        let tex = mesh.texture.as_ref()?;
        let y_start = (self.strip_v0? * tex.height() as f32).ceil() as u32;
        if Self::hair_slots(tex.width(), tex.height(), y_start).is_empty() {
            return None;
        }
        if self.hair_textures.is_empty() {
            self.hair_textures.push(HairTex { name: "Hair 1".into(), slot: 0, ready: false });
        }
        self.hair_cur = self.hair_cur.min(self.hair_textures.len() - 1);
        Some(self.hair_cur)
    }

    /// A picture of hair that has not got its colours yet gets a smooth colour from the root to the end (the colours of the brush).
    fn paint_hair_default(&mut self, mesh: &mut AssetMesh, idx: usize) {
        if self.hair_textures.get(idx).is_none_or(|h| h.ready) {
            return;
        }
        if let Some((rx, ry, rw, rh)) = self.hair_rect(mesh, idx) {
            if let Some(tex) = mesh.texture.as_mut() {
                image::imageops::replace(tex, &meshbrush::gradient_image(rw, rh, self.brush.root, self.brush.tip, idx as u32 + 1), rx as i64, ry as i64);
            }
            self.hair_textures[idx].ready = true;
        }
    }

    /// Another picture of hair, made of the colours of the brush: it is the one the hair brush draws with from now on.
    pub fn new_hair_texture(&mut self, mesh: &mut AssetMesh) {
        if !self.ensure_strip(mesh) || self.ensure_hair_texture(mesh).is_none() {
            self.message = Some("This mesh has no room for a picture of hair: it needs a texture (unwrap it first) that is big enough.".into());
            return;
        }
        let Some(tex) = mesh.texture.as_ref() else { return };
        let y_start = (self.strip_v0.unwrap_or(0.9) * tex.height() as f32).ceil() as u32;
        if self.hair_textures.len() >= Self::hair_slots(tex.width(), tex.height(), y_start).len() {
            self.message = Some("There is no room for more pictures of hair: use a bigger texture.".into());
            return;
        }
        self.hair_textures.push(HairTex { name: format!("Hair {}", self.hair_textures.len() + 1), slot: self.hair_textures.len(), ready: false });
        self.hair_cur = self.hair_textures.len() - 1;
        self.paint_hair_default(mesh, self.hair_cur);
        self.hair_editor = None;
    }

    /// A picture from a file becomes a picture of hair (it is made as big as the place for it) and the one the hair brush draws with.
    pub fn import_hair_texture(&mut self, mesh: &mut AssetMesh, path: &std::path::Path) {
        let img = match image::open(path) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                self.message = Some(format!("The picture could not be opened: {e}"));
                return;
            }
        };
        let before = self.hair_textures.len();
        self.new_hair_texture(mesh);
        if self.hair_textures.len() == before {
            return;
        }
        if let Some((rx, ry, rw, rh)) = self.hair_rect(mesh, self.hair_cur) {
            let fit = image::imageops::resize(&img, rw, rh, image::imageops::FilterType::Lanczos3);
            if let Some(tex) = mesh.texture.as_mut() {
                image::imageops::replace(tex, &fit, rx as i64, ry as i64);
            }
            if let Some(name) = path.file_stem() {
                self.hair_textures[self.hair_cur].name = name.to_string_lossy().chars().take(20).collect();
            }
        }
        self.painted = true;
        self.version += 1;
    }

    /// A line of hair looks into another picture of hair: its texture coordinates move from the rectangle it had to the new one.
    pub fn strand_set_hair_texture(&mut self, mesh: &mut AssetMesh, i: usize, tex: usize) {
        let Some(s) = self.strands.get(i) else { return };
        let Some(old) = s.tex else { return };
        let (Some(from), Some(to)) = (self.hair_rect(mesh, old), self.hair_rect(mesh, tex)) else { return };
        let Some(t) = mesh.texture.as_ref() else { return };
        let (size, high) = (t.width() as f32, t.height() as f32);
        for uv in mesh.uvs.iter_mut().take(s.verts.1).skip(s.verts.0) {
            let u = (uv[0] * size - from.0 as f32 - 2.0) / (from.2 - 4) as f32;
            let v = (uv[1] * high - from.1 as f32 - 1.0) / (from.3 - 2) as f32;
            *uv = [(to.0 as f32 + 2.0 + u * (to.2 - 4) as f32) / size, (to.1 as f32 + 1.0 + v * (to.3 - 2) as f32) / high];
        }
        self.strands[i].tex = Some(tex);
        self.paint_hair_default(mesh, tex);
        self.version += 1;
    }

    /// Open a picture of hair to draw it: the whole rectangle every line of hair that looks into it covers, with the brush of the Draw tab.
    pub fn open_hair_editor(&mut self, mesh: &mut AssetMesh) {
        if !self.ensure_strip(mesh) {
            self.message = Some("This mesh has no texture, so the hair has no picture. Unwrap it first (the Redraw and sculpt page).".into());
            return;
        }
        let Some(idx) = self.ensure_hair_texture(mesh) else {
            self.message = Some("The strip of the texture is too small for a hair picture: use a bigger texture.".into());
            return;
        };
        self.paint_hair_default(mesh, idx);
        let Some((rx, ry, rw, rh)) = self.hair_rect(mesh, idx) else { return };
        let Some(tex) = mesh.texture.as_ref() else { return };
        let crop = image::imageops::crop_imm(tex, rx, ry, rw, rh).to_image();
        let mut doc = Document::new(rw, rh);
        doc.layers[0].tiles = crate::files::tiles_from_rgba(&crop, 0, 0, rw, rh);
        self.hair_editor = Some(HairEditor { doc, stroke: None, handle: None, region: (rx, ry, rw, rh), tex: idx });
    }

    /// The window for drawing the picture of hair.
    pub fn hair_editor_ui(&mut self, ctx: &egui::Context, mesh: &mut AssetMesh) {
        let Some(mut ed) = self.hair_editor.take() else { return };
        let mut open = true;
        let kit = self.kit.clone();
        let (rw, rh) = (ed.region.2, ed.region.3);
        let zoom = (440 / rh).max(1) as f32;
        let mut redo_default = false;
        let title = self.hair_textures.get(ed.tex).map(|h| h.name.clone()).unwrap_or_default();
        egui::Window::new(RichText::new(format!("Hair picture: {title}")).strong()).id(egui::Id::new("hair_picture_window")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new("Every line of hair that uses this picture is covered by the whole of it: across the line left to right, the root at the top and the end at the bottom. Draw it with the brush of the Draw tab (the same brush and colour).").size(11.5).color(TEXT_DIM));
            let (rect, resp) = ui.allocate_exact_size(vec2(rw as f32 * zoom, rh as f32 * zoom), Sense::click_and_drag());
            let to_doc = |p: Pos2| BrushSample { x: ((p.x - rect.min.x) / zoom).clamp(0.0, rw as f32 - 1.0), y: ((p.y - rect.min.y) / zoom).clamp(0.0, rh as f32 - 1.0), p: 1.0 };
            let none = |_: i32, _: i32| -> Option<egui::Color32> { None };
            if let Some(kit) = &kit {
                let mut settings = kit.settings.clone();
                settings.size = (settings.size / zoom * 2.0).max(1.0);
                settings.stabilize = 0.0;
                if resp.drag_started() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        ed.stroke = Some(BrushStroke::begin(&mut ed.doc, 0, &settings, kit.color, kit.erase, 1.0, false, to_doc(p), &none));
                    }
                } else if resp.dragged() {
                    if let (Some(p), Some(st)) = (resp.interact_pointer_pos(), ed.stroke.as_mut()) {
                        st.add(&mut ed.doc, &[to_doc(p)], &none);
                    }
                }
                if resp.drag_stopped() {
                    if let Some(st) = ed.stroke.take() {
                        st.finish(&mut ed.doc, &none);
                    }
                }
            }
            // the picture, as it is now
            let mut img = RgbaImage::new(rw, rh);
            for ty in 0..=((rh as usize - 1) / TILE) {
                for tx in 0..=((rw as usize - 1) / TILE) {
                    Self::put_tile(&mut img, &ed.doc, (tx as i32, ty as i32));
                }
            }
            let color = egui::ColorImage::from_rgba_unmultiplied([rw as usize, rh as usize], img.as_raw());
            let tex = ui.ctx().load_texture("hair_picture", color, TextureOptions::NEAREST);
            ui.painter().image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            ed.handle = Some(tex);
            // it goes on the texture of the mesh as it is drawn
            if let Some(t) = mesh.texture.as_mut() {
                image::imageops::replace(t, &img, ed.region.0 as i64, ed.region.1 as i64);
            }
            ui.horizontal(|ui| {
                if ui.button("Root to end colours").help("The picture goes back to a smooth colour from the root colour to the end colour (the colours of the brush)").clicked() {
                    redo_default = true;
                }
            });
        });
        if redo_default {
            let img = meshbrush::gradient_image(rw, rh, self.brush.root, self.brush.tip, ed.tex as u32 + 1);
            ed.doc.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, rw, rh);
            ed.doc.layers[0].rev += 1;
        }
        self.painted = true;
        self.version += 1;
        if open {
            self.hair_editor = Some(ed);
        }
        ctx.request_repaint();
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
        let _ = self.append_built(mesh, &built, self.strip_next, from);
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
        self.attach_label = Some(part.label().to_string());
        self.attach_apply(mesh);
    }

    /// Keep the part: it is a part of the mesh now (it can be taken back like a stroke).
    pub fn attach_keep(&mut self, mesh: &AssetMesh) {
        if let Some((part, params, v0, t0)) = self.attach.take() {
            if mesh.verts.len() > v0 {
                self.geo_undo.push((v0, t0));
                self.strip_next += 1;
                let bones_before = self.rig.bones.len();
                let chains = attach::rigging(part, &params, &attach::Anchors::of(&AssetMesh { verts: mesh.verts[..v0].to_vec(), uvs: Vec::new(), tris: Vec::new(), texture: None, normal: None }));
                let bones = self.rig_part(mesh, v0, &chains);
                if bones.is_some() {
                    self.hair_marks.push((self.geo_undo.len() - 1, bones_before));
                }
                let name = self.attach_label.take().unwrap_or_else(|| "Part".into());
                self.strands.push(Strand { name, verts: (v0, mesh.verts.len()), bones, line: Vec::new(), width_mul: 1.0, tex: None });
            }
        }
    }

    /// The bones of a part that moves (a tail, wings): chains along it, hanging from the bone that moves what is under their root, and the points of the part follow
    /// the bones of the chain they are nearest to, along its length.
    fn rig_part(&mut self, mesh: &AssetMesh, v0: usize, chains: &[attach::RigChain]) -> Option<(usize, usize)> {
        if chains.is_empty() || self.rig.weights.len() != mesh.verts.len() {
            return None;
        }
        let first = self.rig.bones.len();
        let k = self.hair_count + 1;
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for c in chains {
            let root = c.joints[0];
            let near = mesh.verts[..v0].iter().enumerate().min_by(|a, b| {
                let d = |p: &[f32; 3]| (p[0] - root[0]).powi(2) + (p[1] - root[1]).powi(2) + (p[2] - root[2]).powi(2);
                d(a.1).total_cmp(&d(b.1))
            }).map(|x| x.0);
            let parent = near.and_then(|f| self.rig.main_bone(f));
            let start = self.rig.bones.len();
            for i in 0..c.joints.len() - 1 {
                let name = if c.name == "tail" { format!("tail_{k}_{i}") } else { format!("{}_{i}", c.name) };
                let up = if i == 0 { parent } else { Some(start + i - 1) };
                self.rig.add_bone(&name, up, c.joints[i], c.joints[i + 1]);
            }
            ranges.push((start, c.joints.len() - 1));
        }
        // every point of the part follows the chain that is nearest to it, along the chain
        for j in v0..mesh.verts.len() {
            let p = mesh.verts[j];
            let mut best = (f32::MAX, 0usize, 0.0f32);
            for (ci, c) in chains.iter().enumerate() {
                let n = c.joints.len() - 1;
                for s in 0..n {
                    let (a, b) = (c.joints[s], c.joints[s + 1]);
                    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let l2 = (ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2]).max(1e-12);
                    let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / l2).clamp(0.0, 1.0);
                    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
                    let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2);
                    if d < best.0 {
                        best = (d, ci, s as f32 + t);
                    }
                }
            }
            let (_, ci, u) = best;
            let (start, n) = ranges[ci];
            let i = (u.floor() as usize).min(n - 1);
            let f = u - i as f32;
            let next = if i + 1 < n { ((f - 0.55) / 0.45).clamp(0.0, 1.0).powi(2) * (3.0 - 2.0 * ((f - 0.55) / 0.45).clamp(0.0, 1.0)) } else { 0.0 };
            self.rig.weights[j] = if next > 0.0 { vec![((start + i) as u16, 1.0 - next), ((start + i + 1) as u16, next)] } else { vec![((start + i) as u16, 1.0)] };
        }
        self.pose.rot.resize(self.rig.bones.len(), [0.0; 3]);
        self.hair_sim = None;
        Some((first, self.rig.bones.len() - first))
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

    /// A strand is moved: its points, its line and its bones.
    pub fn strand_translate(&mut self, mesh: &mut AssetMesh, i: usize, d: [f32; 3]) {
        let Some(s) = self.strands.get_mut(i) else { return };
        for v in mesh.verts.iter_mut().take(s.verts.1).skip(s.verts.0) {
            *v = [v[0] + d[0], v[1] + d[1], v[2] + d[2]];
        }
        for p in &mut s.line {
            *p = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
        }
        if let Some((first, n)) = s.bones {
            for b in self.rig.bones.iter_mut().skip(first).take(n) {
                if is_part_bone(&b.name) {
                    for p in [&mut b.head, &mut b.tail] {
                        *p = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                    }
                }
            }
        }
        self.hair_sim = None;
        self.painted = true;
        self.version += 1;
    }

    /// A strand is made thicker or thinner (as a multiplier of how thick it was drawn): every point moves nearer to or further from the line it follows.
    pub fn strand_thickness(&mut self, mesh: &mut AssetMesh, i: usize, mul: f32) {
        let Some(s) = self.strands.get_mut(i) else { return };
        if s.line.len() < 2 {
            return;
        }
        let k = mul / s.width_mul.max(1e-3);
        for v in mesh.verts.iter_mut().take(s.verts.1).skip(s.verts.0) {
            let q = near_on_line(&s.line, *v);
            *v = [q[0] + (v[0] - q[0]) * k, q[1] + (v[1] - q[1]) * k, q[2] + (v[2] - q[2]) * k];
        }
        s.width_mul = mul;
        self.painted = true;
        self.version += 1;
    }

    /// The end of a line is dragged to a place: the strand gets longer or shorter along the way it goes (its root stays, its thickness stays), and so do its bones.
    pub fn strand_set_tip(&mut self, mesh: &mut AssetMesh, i: usize, tip: [f32; 3]) {
        let Some(s) = self.strands.get_mut(i) else { return };
        let (Some(root), Some(old)) = (s.line.first().copied(), s.line.last().copied()) else { return };
        let dist = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let (was, now) = (dist(root, old), dist(root, tip));
        if was < 1e-5 {
            return;
        }
        let k = (now / was).clamp(0.15, 6.0);
        let grow = |p: [f32; 3]| [root[0] + (p[0] - root[0]) * k, root[1] + (p[1] - root[1]) * k, root[2] + (p[2] - root[2]) * k];
        for v in mesh.verts.iter_mut().take(s.verts.1).skip(s.verts.0) {
            let q = near_on_line(&s.line, *v);
            let q2 = grow(q);
            *v = [q2[0] + (v[0] - q[0]), q2[1] + (v[1] - q[1]), q2[2] + (v[2] - q[2])];
        }
        for p in &mut s.line {
            *p = grow(*p);
        }
        if let Some((first, n)) = s.bones {
            for b in self.rig.bones.iter_mut().skip(first).take(n) {
                if is_part_bone(&b.name) {
                    b.head = grow(b.head);
                    b.tail = grow(b.tail);
                }
            }
        }
        self.hair_sim = None;
        self.painted = true;
        self.version += 1;
    }

    /// A drawn object is turned about its root (the start of its line, or the middle of a part): its points, its line and its bones.
    pub fn strand_rotate(&mut self, mesh: &mut AssetMesh, i: usize, axis: usize, degrees: f32) {
        let Some(s) = self.strands.get_mut(i) else { return };
        let pivot = s.line.first().copied().unwrap_or_else(|| {
            let n = (s.verts.1 - s.verts.0).max(1) as f32;
            let mut c = [0.0f32; 3];
            for v in mesh.verts.iter().take(s.verts.1).skip(s.verts.0) {
                for k in 0..3 {
                    c[k] += v[k] / n;
                }
            }
            c
        });
        let (sn, cs) = degrees.to_radians().sin_cos();
        let turn = |p: [f32; 3]| {
            let d = [p[0] - pivot[0], p[1] - pivot[1], p[2] - pivot[2]];
            let r = match axis {
                0 => [d[0], d[1] * cs - d[2] * sn, d[1] * sn + d[2] * cs],
                1 => [d[0] * cs + d[2] * sn, d[1], -d[0] * sn + d[2] * cs],
                _ => [d[0] * cs - d[1] * sn, d[0] * sn + d[1] * cs, d[2]],
            };
            [pivot[0] + r[0], pivot[1] + r[1], pivot[2] + r[2]]
        };
        for v in mesh.verts.iter_mut().take(s.verts.1).skip(s.verts.0) {
            *v = turn(*v);
        }
        for p in &mut s.line {
            *p = turn(*p);
        }
        if let Some((first, n)) = s.bones {
            for b in self.rig.bones.iter_mut().skip(first).take(n) {
                if is_part_bone(&b.name) {
                    b.head = turn(b.head);
                    b.tail = turn(b.tail);
                }
            }
        }
        self.hair_sim = None;
        self.painted = true;
        self.version += 1;
    }

    /// A drawn object is made bigger or smaller about its root (all of it, the thickness too).
    pub fn strand_scale(&mut self, mesh: &mut AssetMesh, i: usize, k: f32) {
        let Some(s) = self.strands.get_mut(i) else { return };
        let pivot = s.line.first().copied().unwrap_or_else(|| {
            let n = (s.verts.1 - s.verts.0).max(1) as f32;
            let mut c = [0.0f32; 3];
            for v in mesh.verts.iter().take(s.verts.1).skip(s.verts.0) {
                for a in 0..3 {
                    c[a] += v[a] / n;
                }
            }
            c
        });
        let grow = |p: [f32; 3]| [pivot[0] + (p[0] - pivot[0]) * k, pivot[1] + (p[1] - pivot[1]) * k, pivot[2] + (p[2] - pivot[2]) * k];
        for v in mesh.verts.iter_mut().take(s.verts.1).skip(s.verts.0) {
            *v = grow(*v);
        }
        for p in &mut s.line {
            *p = grow(*p);
        }
        if let Some((first, n)) = s.bones {
            for b in self.rig.bones.iter_mut().skip(first).take(n) {
                if is_part_bone(&b.name) {
                    b.head = grow(b.head);
                    b.tail = grow(b.tail);
                }
            }
        }
        self.hair_sim = None;
        self.painted = true;
        self.version += 1;
    }

    /// An object becomes a part of the mesh (it is not in the list any more: it cannot be moved on its own any more). It stays where it is and keeps its bones.
    pub fn strand_merge(&mut self, i: usize) {
        if i < self.strands.len() {
            self.strands.remove(i);
            self.strand_sel = None;
        }
    }

    /// The strand under a pixel of the picture (the triangle that is seen there belongs to it).
    fn strand_at(&self, mesh: &AssetMesh, cam: &Camera, at: Pos2) -> Option<usize> {
        let (px, py) = (at.x as i32, at.y as i32);
        if px < 0 || py < 0 || px >= cam.w as i32 || py >= cam.h as i32 {
            return None;
        }
        let id = *self.ids.get(py as usize * cam.w as usize + px as usize)?;
        let v = *mesh.tris.get(id as usize)?.first()? as usize;
        self.strands.iter().position(|s| (s.verts.0..s.verts.1).contains(&v))
    }

    /// The handle at the end of a line that is near a pixel of the picture.
    fn tip_at(&self, cam: &Camera, at: Pos2) -> Option<usize> {
        self.strands.iter().position(|s| {
            s.line.last().is_some_and(|t| {
                let q = cam.project(*t);
                (Pos2::new(q[0], q[1]) - at).length() < 10.0
            })
        })
    }

    /// A shape is made (or made again with other numbers): it takes the place of the shape that was being made.
    pub fn shape_apply(&mut self, mesh: &mut AssetMesh) {
        let Some((shape, params, at, v0, t0)) = self.shape_pending else { return };
        mesh.verts.truncate(v0);
        mesh.uvs.truncate(v0);
        mesh.tris.truncate(t0);
        self.rig.weights.truncate(v0);
        let m = meshops::make(shape, &params);
        let built = meshbrush::Built { verts: m.verts.iter().map(|v| [v[0] + at[0], v[1] + at[1], v[2] + at[2]]).collect(), uvs: m.uvs.clone(), tris: m.tris.clone() };
        let from = mesh.verts.iter().enumerate().min_by(|a, b| {
            let d = |p: &[f32; 3]| (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2);
            d(a.1).total_cmp(&d(b.1))
        }).map(|x| x.0);
        // a shape is not hair: its colours are in a cell of their own
        let hair = std::mem::replace(&mut self.hair, false);
        let _ = self.append_built(mesh, &built, self.strip_next, from);
        self.hair = hair;
    }

    /// The shape that is being made goes where the pointer is on the surface (a little out of it, so it sits on it).
    fn place_shape_at(&mut self, mesh: &mut AssetMesh, cam: &Camera, at: Pos2) {
        let Some((shape, params, _, v0, t0)) = self.shape_pending else { return };
        let (px, py) = (at.x as i32, at.y as i32);
        if px < 0 || py < 0 || px >= cam.w as i32 || py >= cam.h as i32 {
            return;
        }
        let Some(&id) = self.ids.get(py as usize * cam.w as usize + px as usize) else { return };
        if id == u32::MAX {
            return;
        }
        // the ids are of the mesh as it was drawn: a triangle of the shape itself is not a place to put it on
        if id as usize >= t0 {
            return;
        }
        let Some((p, n)) = mesh.surface_at(&mesh.verts, cam, id as usize, px as f32 + 0.5, py as f32 + 0.5) else { return };
        let r = params.size * 0.8;
        self.shape_pending = Some((shape, params, [p[0] + n[0] * r, p[1] + n[1] * r, p[2] + n[2] * r], v0, t0));
        self.shape_apply(mesh);
    }

    /// Start a shape (or change to another): it is put in the middle of the body, to be moved where it is wanted.
    pub fn shape_start(&mut self, mesh: &mut AssetMesh, shape: meshops::Shape) {
        if !self.ensure_strip(mesh) {
            self.message = Some("This mesh has no texture, so the colours of a shape have no place. Unwrap it first (the Redraw and sculpt page).".into());
            return;
        }
        let (v0, t0) = match self.shape_pending {
            Some((_, _, _, v, t)) => (v, t),
            None => (mesh.verts.len(), mesh.tris.len()),
        };
        let (lo, hi) = self.bounds;
        let at = match self.shape_pending {
            Some((_, _, at, _, _)) => at,
            None => [(lo[0] + hi[0]) / 2.0, lo[1] + (hi[1] - lo[1]) * 0.6, (lo[2] + hi[2]) / 2.0 + 0.1 * (hi[1] - lo[1])],
        };
        let params = meshops::ShapeParams { size: 0.05 * (hi[1] - lo[1]).max(0.2), height: 0.1 * (hi[1] - lo[1]).max(0.2), ..Default::default() };
        self.shape_pending = Some((shape, params, at, v0, t0));
        self.shape_apply(mesh);
    }

    /// Keep the shape: it is an object of the mesh now (it can be moved, turned and made bigger on its own, and taken back).
    pub fn shape_keep(&mut self, mesh: &AssetMesh) {
        if let Some((shape, _, _, v0, t0)) = self.shape_pending.take() {
            if mesh.verts.len() > v0 {
                self.geo_undo.push((v0, t0));
                self.strip_next += 1;
                self.strands.push(Strand { name: shape.label().to_string(), verts: (v0, mesh.verts.len()), bones: None, line: Vec::new(), width_mul: 1.0, tex: None });
                self.strand_sel = Some(self.strands.len() - 1);
            }
        }
    }

    pub fn shape_drop(&mut self, mesh: &mut AssetMesh) {
        if let Some((_, _, _, v0, t0)) = self.shape_pending.take() {
            mesh.verts.truncate(v0);
            mesh.uvs.truncate(v0);
            mesh.tris.truncate(t0);
            self.rig.weights.truncate(v0);
            self.version += 1;
        }
    }

    /// The mesh is made another mesh with more points: the old one is kept to take it back, and a new point (the middle of two points) moves with the mean of what
    /// moves the two.
    fn apply_subdivided(&mut self, mesh: &mut AssetMesh, new: AssetMesh, parents: &[(u32, u32)]) {
        let before = (mesh.clone(), self.rig.weights.clone());
        let old = mesh.verts.len();
        let mut weights = self.rig.weights.clone();
        if weights.len() == old {
            for (a, b) in parents {
                let mut mix: std::collections::HashMap<u16, f32> = std::collections::HashMap::new();
                for (bone, x) in weights[*a as usize].iter().chain(weights[*b as usize].iter()) {
                    *mix.entry(*bone).or_default() += x * 0.5;
                }
                let mut v: Vec<(u16, f32)> = mix.into_iter().collect();
                v.sort_by(|x, y| y.1.total_cmp(&x.1));
                v.truncate(4);
                v.sort_by(|x, y| x.0.cmp(&y.0));
                weights.push(v);
            }
        }
        self.cut_undo.push(before);
        if self.cut_undo.len() > 3 {
            self.cut_undo.remove(0);
        }
        *mesh = new;
        self.rig.weights = weights;
        self.rig.normalize(4);
        self.selected = vec![false; mesh.verts.len()];
        self.strands.clear();
        self.strand_sel = None;
        self.geo_undo.clear();
        self.painted = true;
        self.near = None;
        self.version += 1;
    }

    /// The faces are cut smaller: the picked ones, or all of them when nothing is picked.
    pub fn subdivide_picked(&mut self, mesh: &mut AssetMesh) {
        let faces = if self.selected.len() == mesh.verts.len() && self.selected.iter().any(|s| *s) { meshedit::selected_tris(mesh, &self.selected) } else { vec![true; mesh.tris.len()] };
        if let Some((new, parents)) = meshops::subdivide(mesh, &faces) {
            let (a, b) = (mesh.verts.len(), new.verts.len());
            self.apply_subdivided(mesh, new, &parents);
            self.geometry_message = Some(format!("{} points more ({} now).", b - a, b));
        }
    }

    /// More points where the body bends: the faces near the joints of the bones are cut smaller (a few times), so the mesh bends well there.
    pub fn fit_joints(&mut self, mesh: &mut AssetMesh) {
        if self.rig.bones.len() < 2 {
            self.geometry_message = Some("Put the bones in first (the Bones and weights page): the joints are where the bones meet.".into());
            return;
        }
        let start = mesh.verts.len();
        for _ in 0..self.joint_levels.clamp(1, 3) {
            let faces = meshops::faces_near_joints(mesh, &self.rig, self.joint_radius);
            let Some((new, parents)) = meshops::subdivide(mesh, &faces) else { break };
            self.apply_subdivided(mesh, new, &parents);
        }
        self.geometry_message = Some(format!("{} points added near the joints.", mesh.verts.len() - start));
    }

    /// The holes of the mesh are closed (up to the size asked for).
    pub fn fix_holes(&mut self, mesh: &mut AssetMesh) {
        let max = self.hole_edges;
        let mut found = 0;
        self.edit_mesh(mesh, |_, m| {
            let (new, copy_of, n) = meshops::fill_holes(m, max)?;
            found = n;
            let map: Vec<Option<u32>> = (0..m.verts.len() as u32).map(Some).collect();
            let mut selection = vec![false; new.verts.len()];
            for s in selection.iter_mut().skip(m.verts.len()) {
                *s = true;
            }
            *m = new;
            Some((map, copy_of, selection))
        });
        self.geometry_message = Some(if found == 0 { "No holes to close (or they are bigger than the size set).".into() } else { format!("{found} holes closed.") });
    }

    /// The faces for a file with quads (when the option is on): the triangles that pair up, as quads.
    pub fn quads_for_export(&self, mesh: &AssetMesh) -> Option<(Vec<[u32; 4]>, Vec<[u32; 3]>)> {
        self.quads.then(|| meshops::pair_quads(mesh, self.quad_angle))
    }

    /// Take the last stroke of the mesh brush back.
    fn undo_stroke(&mut self, mesh: &mut AssetMesh) {
        if let Some((v, t)) = self.geo_undo.pop() {
            // the bones of a strand of hair that was drawn go with it
            let at = self.geo_undo.len();
            if let Some(b) = self.hair_marks.iter().filter(|m| m.0 == at).map(|m| m.1).min() {
                self.rig.bones.truncate(b);
                self.pose.rot.truncate(b);
                self.hair_sim = None;
            }
            self.hair_marks.retain(|m| m.0 != at);
            self.strands.retain(|s| s.verts.0 < v);
            self.strand_sel = None;
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
        self.strands.clear();
        self.strand_sel = None;
        self.geo_undo.clear();
        self.painted = true;
        self.near = None;
        self.version += 1;
    }

    fn undo_cut(&mut self, mesh: &mut AssetMesh) {
        if let Some((m, w)) = self.cut_undo.pop() {
            *mesh = m;
            self.rig.weights = w;
            self.strands.clear();
            self.strand_sel = None;
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

    /// The hair swings: the chains of bones that hang from the head are run by the simulation, and their turns are put in the pose that is shown.
    pub fn apply_hair(&mut self, mesh: &AssetMesh, dt: f32) {
        if !self.hair_physics {
            return;
        }
        if self.hair_sim.is_none() || self.hair_sig != self.rig.bones.len() {
            let colliders = self.hair_colliders(mesh);
            self.hair_sim = Some(crate::hairsim::HairSim::with_colliders(&self.rig, &colliders));
            self.hair_sig = self.rig.bones.len();
        }
        let params = self.hair_params;
        if let Some(sim) = self.hair_sim.as_mut() {
            if sim.is_empty() {
                return;
            }
            let mut p = self.pose.clone();
            sim.step(&self.rig, &mut p, &params, dt);
            self.pose = p;
            self.version += 1;
        }
    }

    /// How wide the body is round the bones that the hair must not go through (the head, the neck, the chest, the shoulders and the upper arms): the mean
    /// distance of the points that each moves from the bone, a little less.
    pub fn hair_colliders(&self, mesh: &AssetMesh) -> Vec<crate::hairsim::Collider> {
        let wanted = ["head", "neck", "chest", "upperchest", "spine", "leftshoulder", "rightshoulder", "leftupperarm", "rightupperarm"];
        let mut sum = vec![(0.0f32, 0u32); self.rig.bones.len()];
        if self.rig.weights.len() != mesh.verts.len() {
            return Vec::new();
        }
        for (i, v) in mesh.verts.iter().enumerate() {
            if let Some(b) = self.rig.main_bone(i) {
                if wanted.contains(&self.rig.bones[b].name.to_lowercase().as_str()) {
                    let bone = &self.rig.bones[b];
                    sum[b].0 += crate::rig3d::distance_to_bone(*v, bone.head, bone.tail).0;
                    sum[b].1 += 1;
                }
            }
        }
        sum.iter().enumerate().filter(|(_, s)| s.1 > 20).map(|(b, s)| (b, 0.9 * s.0 / s.1 as f32)).collect()
    }

    /// Is there hair that moves (bones of strands that were drawn)?
    pub fn has_hair(&self) -> bool {
        self.rig.bones.iter().any(|b| {
            let n = b.name.to_lowercase();
            n.starts_with("hair") || n.starts_with("tail")
        })
    }

    /// The animations get the motion of the hair, when it is on: the files carry it as keys.
    pub fn with_hair(&self, mut anims: Vec<crate::take::Anim>, mesh: &AssetMesh) -> Vec<crate::take::Anim> {
        if self.hair_physics && self.has_hair() {
            crate::hairsim::bake(&self.rig, &mut anims, &self.hair_params, &self.hair_colliders(mesh));
        }
        anims
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

    /// How many texels there are in a metre of the surface (the textures of the pieces have the same density, so this is for all of the mesh).
    fn texels_per_metre(mesh: &AssetMesh) -> f32 {
        let Some(tex) = mesh.texture.as_ref() else { return 500.0 };
        let (w, h) = (tex.width() as f32, tex.height() as f32);
        let (mut uv_area, mut world) = (0.0f64, 0.0f64);
        for t in &mesh.tris {
            let (a, b, c) = (mesh.uvs[t[0] as usize], mesh.uvs[t[1] as usize], mesh.uvs[t[2] as usize]);
            uv_area += (0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * w * h) as f64;
            let (p, q, r) = (mesh.verts[t[0] as usize], mesh.verts[t[1] as usize], mesh.verts[t[2] as usize]);
            let (u, v) = ([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [r[0] - p[0], r[1] - p[1], r[2] - p[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            world += 0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() as f64;
        }
        if world < 1e-12 { 500.0 } else { (uv_area / world).sqrt() as f32 }
    }

    /// The texel of a pixel of the picture, as a place on the canvas of the texture.
    fn uv_pixel(&self, mesh: &AssetMesh, cam: &Camera, at: Pos2, w: f32, h: f32) -> Option<[f32; 2]> {
        let (px, py) = (at.x as i32, at.y as i32);
        if px < 0 || py < 0 || px >= cam.w as i32 || py >= cam.h as i32 {
            return None;
        }
        let id = *self.ids.get(py as usize * cam.w as usize + px as usize)?;
        if id == u32::MAX {
            return None;
        }
        let uv = mesh.uv_at(&mesh.verts, cam, id as usize, px as f32 + 0.5, py as f32 + 0.5)?;
        Some([uv[0].rem_euclid(1.0) * w, uv[1].rem_euclid(1.0) * h])
    }

    /// The points that are picked or let go by a loop or a brush are worked out when the pointer is let go (the loop), or as the brush goes.
    fn pick_points(&mut self, mesh: &AssetMesh, cam: &Camera, points: &[usize], how: u8) {
        if self.selected.len() != mesh.verts.len() {
            self.selected = vec![false; mesh.verts.len()];
        }
        for p in points {
            if let Some(s) = self.selected.get_mut(*p) {
                *s = how != 2;
            }
        }
        let _ = cam;
        self.version += 1;
    }

    /// The loop is closed: the points inside it are picked (shift adds to what is picked, ctrl takes away, and without them it starts again).
    fn finish_faces(&mut self, mesh: &AssetMesh, cam: &Camera, mods: egui::Modifiers) {
        if self.pick_mode != PickMode::Loop {
            self.lasso.clear();
            return;
        }
        let poly = std::mem::take(&mut self.lasso);
        if poly.len() < 3 {
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
        if !mods.shift && !mods.command {
            self.selected = vec![false; mesh.verts.len()];
        }
        let w = cam.w as i32;
        let mut points = Vec::new();
        for (i, v) in mesh.verts.iter().enumerate() {
            let p = cam.project(*v);
            let (px, py) = (p[0] as i32, p[1] as i32);
            if px < 0 || py < 0 || px >= w || py >= cam.h as i32 || !inside(p[0], p[1]) {
                continue;
            }
            // the points on the back are not picked unless that is asked for
            if !self.through && self.depth.get(py as usize * w as usize + px as usize).is_some_and(|d| p[2] > *d + 8.0) {
                continue;
            }
            points.push(i);
        }
        self.pick_points(mesh, cam, &points, if mods.command { 2 } else { 1 });
    }

    /// Something is done to the mesh that makes it another mesh: the old one is kept to take it back, and what belongs to the points follows.
    fn edit_mesh(&mut self, mesh: &mut AssetMesh, f: impl FnOnce(&mut Self, &mut AssetMesh) -> Option<(Vec<Option<u32>>, Vec<u32>, Vec<bool>)>) {
        let before = (mesh.clone(), self.rig.weights.clone());
        let old = mesh.verts.len();
        let Some((map, copy_of, selection)) = f(self, mesh) else { return };
        // the weights: the points that stay keep theirs, the new ones take the weights of the points they are copies of
        let mut weights = vec![Vec::new(); mesh.verts.len()];
        if before.1.len() == old {
            for (o, to) in map.iter().enumerate() {
                if let Some(j) = to {
                    weights[*j as usize] = before.1[o].clone();
                }
            }
            for (k, src) in copy_of.iter().enumerate() {
                let at = old.saturating_sub(0) + k;
                let _ = at;
                let new_index = map.iter().filter(|m| m.is_some()).count() + k;
                if new_index < weights.len() {
                    weights[new_index] = before.1.get(*src as usize).cloned().unwrap_or_default();
                }
            }
        }
        self.strands.clear();
        self.strand_sel = None;
        self.cut_undo.push(before);
        if self.cut_undo.len() > 3 {
            self.cut_undo.remove(0);
        }
        self.rig.weights = weights;
        self.selected = if selection.len() == mesh.verts.len() { selection } else { vec![false; mesh.verts.len()] };
        self.geo_undo.clear();
        self.painted = true;
        self.near = None;
        self.version += 1;
    }

    /// A stroke of the Draw brush starts on the surface.
    fn tex_begin(&mut self, mesh: &mut AssetMesh, cam: &Camera, at: Pos2) -> bool {
        let (Some(kit), Some(tex)) = (self.kit.clone(), mesh.texture.as_ref()) else { return false };
        let (w, h) = tex.dimensions();
        let mut doc = Document::new(w, h);
        let mut layer = doc.new_layer("Texture", LayerKind::Raster);
        layer.tiles = crate::files::tiles_from_rgba(tex, 0, 0, w, h);
        doc.layers.clear();
        let li = doc.insert_layer(layer, 0);
        let _ = li;
        let k = (Self::texels_per_metre(mesh) / cam.scale()).max(0.01);
        let mut settings = kit.settings.clone();
        settings.size = (settings.size * k * self.tex_size_mul).max(1.0);
        settings.stabilize = 0.0;
        self.texture_checkpoint(mesh);
        self.tex_session = Some(TexSession { doc, settings, color: kit.color, erase: kit.erase, texels_per_pixel: k, last: at, strokes: vec![(None, None), (None, None)], touched: Default::default() });
        self.tex_move(mesh, cam, at);
        true
    }

    /// The pointer moved: the brush goes along the surface (a stroke is cut where the surface is cut: at the seams of the texture).
    fn tex_move(&mut self, mesh: &mut AssetMesh, cam: &Camera, now: Pos2) {
        let Some(mut s) = self.tex_session.take() else { return };
        let Some(tex_dims) = mesh.texture.as_ref().map(|t| t.dimensions()) else {
            self.tex_session = Some(s);
            return;
        };
        let (w, h) = (tex_dims.0 as f32, tex_dims.1 as f32);
        // the points between where the pointer was and where it is
        let d = now - s.last;
        let steps = (d.length() / 2.0).ceil().max(1.0) as usize;
        let none = |_: i32, _: i32| -> Option<egui::Color32> { None };
        let mut batches: Vec<Vec<BrushSample>> = vec![Vec::new(), Vec::new()];
        let mut starts: Vec<Option<BrushSample>> = vec![None, None];
        let jump = (s.settings.size * 4.0).max(48.0);
        for i in 1..=steps {
            let p = s.last + d * (i as f32 / steps as f32);
            let mut spots = vec![self.uv_pixel(mesh, cam, p, w, h)];
            spots.push(if self.mirror { self.mirror_pixel(mesh, cam, p).and_then(|m| self.uv_pixel(mesh, cam, m, w, h)) } else { None });
            for (k, spot) in spots.into_iter().enumerate() {
                let Some(uv) = spot else { continue };
                let smp = BrushSample { x: uv[0], y: uv[1], p: 1.0 };
                // a jump across the texture (a seam, or another piece of the surface): the stroke is ended and another one starts
                let far = s.strokes[k].1.is_some_and(|l| ((l[0] - uv[0]).powi(2) + (l[1] - uv[1]).powi(2)).sqrt() > jump);
                if far {
                    if !batches[k].is_empty() {
                        if let Some(st) = s.strokes[k].0.as_mut() {
                            st.add(&mut s.doc, &batches[k], &none);
                        }
                        batches[k].clear();
                    }
                    if let Some(st) = s.strokes[k].0.take() {
                        st.finish(&mut s.doc, &none);
                    }
                }
                if s.strokes[k].0.is_none() {
                    starts[k] = Some(smp);
                    s.strokes[k].0 = Some(BrushStroke::begin(&mut s.doc, 0, &s.settings, s.color, s.erase, 1.0, false, smp, &none));
                } else {
                    batches[k].push(smp);
                }
                s.strokes[k].1 = Some(uv);
            }
        }
        for k in 0..2 {
            if let (false, Some(st)) = (batches[k].is_empty(), s.strokes[k].0.as_mut()) {
                st.add(&mut s.doc, &batches[k], &none);
            }
        }
        s.last = now;
        // the tiles that were touched go back on the texture of the mesh
        let reach = s.settings.size * 1.5 + 8.0;
        let mut keys = std::collections::HashSet::new();
        for (k, st) in s.strokes.iter().enumerate() {
            if let (Some(_), Some(l)) = (&st.0, st.1) {
                let _ = k;
                let (x0, x1, y0, y1) = (((l[0] - reach).max(0.0) as usize) / TILE, ((l[0] + reach).min(w - 1.0).max(0.0) as usize) / TILE, ((l[1] - reach).max(0.0) as usize) / TILE, ((l[1] + reach).min(h - 1.0).max(0.0) as usize) / TILE);
                for ty in y0..=y1 {
                    for tx in x0..=x1 {
                        keys.insert((tx as i32, ty as i32));
                    }
                }
            }
        }
        // the whole stretch of this move: the tiles round the points that were just painted
        for b in batches.iter().chain(starts.iter().map(|s| s.iter().cloned().collect::<Vec<_>>()).collect::<Vec<_>>().iter()) {
            for p in b {
                let (x0, x1, y0, y1) = (((p.x - reach).max(0.0) as usize) / TILE, ((p.x + reach).min(w - 1.0).max(0.0) as usize) / TILE, ((p.y - reach).max(0.0) as usize) / TILE, ((p.y + reach).min(h - 1.0).max(0.0) as usize) / TILE);
                for ty in y0..=y1 {
                    for tx in x0..=x1 {
                        keys.insert((tx as i32, ty as i32));
                    }
                }
            }
        }
        if let Some(tex) = mesh.texture.as_mut() {
            for key in &keys {
                Self::put_tile(tex, &s.doc, *key);
            }
        }
        s.touched.extend(keys);
        self.painted = true;
        self.version += 1;
        self.tex_session = Some(s);
    }

    /// The pixels of a tile of the canvas go on the picture.
    fn put_tile(tex: &mut RgbaImage, doc: &Document, key: (i32, i32)) {
        let px = doc.flatten_tile(key);
        for j in 0..TILE {
            let y = key.1 as usize * TILE + j;
            if y >= tex.height() as usize {
                break;
            }
            for i in 0..TILE {
                let x = key.0 as usize * TILE + i;
                if x >= tex.width() as usize {
                    break;
                }
                let c = crate::blend::from_px(px[j * TILE + i]);
                tex.put_pixel(x as u32, y as u32, image::Rgba(c.to_srgba_unmultiplied()));
            }
        }
    }

    /// The pointer is let go: the strokes are finished (the engine puts on its last touches) and the tiles go on the texture once more.
    fn tex_end(&mut self, mesh: &mut AssetMesh) {
        let Some(mut s) = self.tex_session.take() else { return };
        let none = |_: i32, _: i32| -> Option<egui::Color32> { None };
        for k in 0..s.strokes.len() {
            if let Some(st) = s.strokes[k].0.take() {
                st.finish(&mut s.doc, &none);
            }
        }
        if let Some(tex) = mesh.texture.as_mut() {
            for key in &s.touched {
                Self::put_tile(tex, &s.doc, *key);
            }
        }
        self.painted = true;
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
            self.color_dirty = true;
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
                Grab::Texture if self.tex_session.is_some() => {
                    if let Some(now) = now {
                        self.tex_move(mesh, &cam, now);
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
                Grab::ShapePlace => {
                    if let Some(now) = now {
                        self.place_shape_at(mesh, &cam, now);
                    }
                }
                Grab::StrandMove(i) => {
                    if let Some(now) = now {
                        let depth = self.strands.get(i).and_then(|s| s.line.first().copied()).map(|p| cam.project(p)[2]).unwrap_or(0.0);
                        let (a, b) = (cam.unproject([now.x - delta.x, now.y - delta.y, depth]), cam.unproject([now.x, now.y, depth]));
                        self.strand_translate(mesh, i, [b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
                    }
                }
                Grab::StrandTip(i) => {
                    if let Some(now) = now {
                        if let Some(tip) = self.strands.get(i).and_then(|s| s.line.last().copied()) {
                            let depth = cam.project(tip)[2];
                            self.strand_set_tip(mesh, i, cam.unproject([now.x, now.y, depth]));
                        }
                    }
                }
                Grab::Faces => {
                    if let Some(now) = now {
                        if self.pick_mode == PickMode::Loop {
                            if self.lasso.last().is_none_or(|l| (*l - now).length() > 3.0) {
                                self.lasso.push(now);
                            }
                        } else {
                            let under: Vec<usize> = self.verts_under(mesh, &cam, now, self.radius).into_iter().map(|p| p.0).collect();
                            self.pick_points(mesh, &cam, &under, if mods.command { 2 } else { 1 });
                        }
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
            if self.grab == Grab::Texture {
                self.tex_end(mesh);
            }
            if self.grab == Grab::Faces {
                self.finish_faces(mesh, &cam, mods);
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
        // the picked points as orange dots (the ones that are seen; a few of them when there are very many)
        if self.tool == Tool::Faces && self.selected.len() == mesh.verts.len() {
            let picked: Vec<usize> = (0..mesh.verts.len()).filter(|i| self.selected[*i]).collect();
            let step = (picked.len() / 6000).max(1);
            for i in picked.into_iter().step_by(step) {
                let p = cam.project(mesh.verts[i]);
                let (px, py) = (p[0] as i32, p[1] as i32);
                if px < 0 || py < 0 || px >= cam.w as i32 || py >= cam.h as i32 {
                    continue;
                }
                if !self.through && self.depth.get(py as usize * cam.w as usize + px as usize).is_some_and(|d| p[2] > *d + 8.0) {
                    continue;
                }
                painter.circle_filled(to_screen(p[0], p[1]), 2.0, Color32::from_rgb(255, 150, 30));
            }
        }
        // the handle at the end of every line: drag it to make the line longer or shorter
        if self.tool == Tool::Mesh {
            for (i, s) in self.strands.iter().enumerate() {
                if let Some(t) = s.line.last() {
                    let q = cam.project(*t);
                    let picked = self.strand_sel == Some(i);
                    painter.circle_filled(to_screen(q[0], q[1]), if picked { 6.0 } else { 4.5 }, if picked { GOLD } else { Color32::from_rgb(230, 180, 90) });
                    painter.circle_stroke(to_screen(q[0], q[1]), if picked { 6.0 } else { 4.5 }, Stroke::new(1.5, Color32::BLACK));
                }
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
        if matches!(self.tool, Tool::Weights | Tool::Texture | Tool::Parts | Tool::Sculpt) || (self.tool == Tool::Faces && self.pick_mode == PickMode::Brush) {
            if let Some(at) = hover {
                painter.circle_stroke(to_screen(at.x, at.y), if self.tool == Tool::Texture { self.kit.as_ref().map(|k| k.settings.size * self.tex_size_mul * 0.5).unwrap_or(self.tex_radius) } else { self.radius }, Stroke::new(1.5, Color32::WHITE));
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
            Tool::Faces => "Draw a loop (or paint) over what you want to pick. Shift adds, Ctrl takes away. Right button: turn the view. Wheel: zoom.",
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
                if self.kit.is_some() && self.tex_begin(mesh, cam, at) {
                    return Grab::Texture;
                }
                self.texture_checkpoint(mesh);
                self.paint_texture(mesh, cam, at);
                if let (true, Some(m)) = (self.mirror, self.mirror_pixel(mesh, cam, at)) {
                    self.paint_texture(mesh, cam, m);
                }
                Grab::Texture
            }
            Tool::Attach => Grab::None,
            Tool::Faces => {
                if self.selected.len() != mesh.verts.len() {
                    self.selected = vec![false; mesh.verts.len()];
                }
                if self.pick_mode == PickMode::Loop {
                    self.lasso = vec![at];
                } else {
                    let under: Vec<usize> = self.verts_under(mesh, cam, at, self.radius).into_iter().map(|p| p.0).collect();
                    self.pick_points(mesh, cam, &under, if mods.command { 2 } else { 1 });
                }
                Grab::Faces
            }
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
            Tool::Mesh if self.shape_pending.is_some() && self.shape_place => {
                self.place_shape_at(mesh, cam, at);
                Grab::ShapePlace
            }
            Tool::Mesh if self.tip_at(cam, at).is_some() => {
                let i = self.tip_at(cam, at).unwrap_or(0);
                self.strand_sel = Some(i);
                Grab::StrandTip(i)
            }
            Tool::Mesh if self.strand_move => match self.strand_at(mesh, cam, at).or(self.strand_sel) {
                Some(i) => {
                    self.strand_sel = Some(i);
                    Grab::StrandMove(i)
                }
                None => Grab::None,
            },
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
                (Tool::Faces, "Select mesh", "Pick faces of the mesh with a loop or a brush, then delete, extrude, duplicate or change them, like in Blender"),
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
        if matches!(self.tool, Tool::Texture | Tool::Mesh | Tool::Weights | Tool::Parts | Tool::Lasso | Tool::Sculpt | Tool::Faces) {
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
            Tool::Faces => {
                section_title(ui, "Select mesh");
                ui.horizontal_wrapped(|ui| {
                    for (m, label, tip) in [(PickMode::Loop, "Loop", "Draw a loop round what you want. Shift adds, Ctrl takes away"), (PickMode::Brush, "Brush", "Paint over what you want. Ctrl takes away")] {
                        if widgets::chip(ui, label, self.pick_mode == m).help(tip).clicked() {
                            self.pick_mode = m;
                        }
                    }
                });
                ui.checkbox(&mut self.through, "Also the back").help("Pick the points on the other side of the mesh too, not only the ones you see");
                if self.pick_mode == PickMode::Brush {
                    ui.add(egui::Slider::new(&mut self.radius, 5.0..=200.0).text("Size"));
                }
                if self.selected.len() != mesh.verts.len() {
                    self.selected = vec![false; mesh.verts.len()];
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button(RichText::new("Select all mesh").strong()).clicked() {
                        self.selected = vec![true; mesh.verts.len()];
                        self.version += 1;
                    }
                    if ui.button("None").clicked() {
                        self.selected = vec![false; mesh.verts.len()];
                        self.version += 1;
                    }
                    if ui.button("Invert").clicked() {
                        self.selected.iter_mut().for_each(|s| *s = !*s);
                        self.version += 1;
                    }
                });
                if self.near.is_none() {
                    self.near = Some(neighbours(&mesh.verts, &mesh.tris));
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Wider").help("The points next to the picked ones are picked too").clicked() {
                        let near = self.near.take().unwrap_or_default();
                        self.selected = meshedit::grow(&self.selected, &near);
                        self.near = Some(near);
                        self.version += 1;
                    }
                    if ui.button("Narrower").help("The points at the border of what is picked are let go").clicked() {
                        let near = self.near.take().unwrap_or_default();
                        self.selected = meshedit::shrink(&self.selected, &near);
                        self.near = Some(near);
                        self.version += 1;
                    }
                });
                let (points, faces) = (self.selected.iter().filter(|s| **s).count(), meshedit::selected_tris(mesh, &self.selected).iter().filter(|f| **f).count());
                ui.label(RichText::new(format!("{points} points and {faces} faces are picked")).size(11.5).color(if points > 0 { GOLD } else { TEXT_DIM }));
                ui.add_space(4.0);
                section_title(ui, "Do something with them");
                let any = faces > 0;
                if ui.add_enabled(any, egui::Button::new("Delete picked")).help("The picked faces are taken away").clicked() {
                    let sel = self.selected.clone();
                    self.edit_mesh(mesh, |_, m| {
                        let (new, map) = meshedit::delete_selected(m, &sel);
                        *m = new;
                        Some((map, Vec::new(), Vec::new()))
                    });
                }
                ui.add(egui::Slider::new(&mut self.ext_dist, -0.2..=0.2).text("How far").suffix(" m")).help("For extrude: how far the faces are pushed out (in, when it is below 0)");
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(any, egui::Button::new("Extrude")).help("The picked faces are pushed out along the way they face, with walls round them").clicked() {
                        let (sel, d) = (self.selected.clone(), self.ext_dist);
                        self.edit_mesh(mesh, |_, m| {
                            let made = meshedit::extrude(m, &sel, d)?;
                            let map: Vec<Option<u32>> = (0..m.verts.len() as u32).map(Some).collect();
                            *m = made.mesh;
                            Some((map, made.copy_of, made.selection))
                        });
                    }
                    if ui.add_enabled(any, egui::Button::new("Duplicate")).help("A copy of the picked faces, as new geometry next to them").clicked() {
                        let (sel, d) = (self.selected.clone(), self.ext_dist);
                        self.edit_mesh(mesh, |_, m| {
                            let made = meshedit::duplicate(m, &sel, [d.max(0.02), 0.0, 0.0])?;
                            let map: Vec<Option<u32>> = (0..m.verts.len() as u32).map(Some).collect();
                            *m = made.mesh;
                            Some((map, made.copy_of, made.selection))
                        });
                    }
                });
                ui.add(egui::Slider::new(&mut self.push_dist, -0.1..=0.1).text("Push").suffix(" m"));
                ui.add(egui::Slider::new(&mut self.scale_by, 0.2..=3.0).text("Scale"));
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(points > 0, egui::Button::new("Push out")).help("The picked points move along the way the surface faces").clicked() {
                        let (sel, d) = (self.selected.clone(), self.push_dist);
                        self.edit_mesh(mesh, |_, m| {
                            meshedit::push_along_normals(m, &sel, d);
                            Some(((0..m.verts.len() as u32).map(Some).collect(), Vec::new(), sel))
                        });
                    }
                    if ui.add_enabled(points > 0, egui::Button::new("Scale")).help("The picked points are scaled about their middle").clicked() {
                        let (sel, k) = (self.selected.clone(), self.scale_by);
                        self.edit_mesh(mesh, |_, m| {
                            meshedit::scale_selected(m, &sel, k);
                            Some(((0..m.verts.len() as u32).map(Some).collect(), Vec::new(), sel))
                        });
                    }
                    if ui.add_enabled(points > 0, egui::Button::new("Smooth")).help("The picked points are made smoother").clicked() {
                        let (sel, passes) = (self.selected.clone(), self.smooth_passes);
                        let near = self.near.clone().unwrap_or_default();
                        self.edit_mesh(mesh, |_, m| {
                            meshedit::smooth_selected(m, &sel, &near, passes);
                            Some(((0..m.verts.len() as u32).map(Some).collect(), Vec::new(), sel))
                        });
                    }
                });
                ui.add_space(4.0);
                section_title(ui, "Points and faces");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("More points").help("The picked faces (all of them when nothing is picked) are cut into smaller ones: more points to bend and sculpt").clicked() {
                        self.subdivide_picked(mesh);
                    }
                    if ui.button("Fix holes").help("Closes the holes of the mesh (a hole of no more edges than the size below)").clicked() {
                        self.fix_holes(mesh);
                    }
                });
                ui.add(egui::Slider::new(&mut self.hole_edges, 4..=400).text("Biggest hole (edges)"));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Count the quads").help("How many pairs of triangles make a quad").clicked() {
                        let (q, l) = meshops::pair_quads(mesh, self.quad_angle);
                        self.geometry_message = Some(format!("{} quads and {} triangles left.", q.len(), l.len()));
                    }
                    ui.checkbox(&mut self.quads, "Save .fbx and .obj with quads").help("Triangles that are flat together are saved as quads (fewer, bigger faces). The .glb always has triangles.");
                });
                ui.add(egui::Slider::new(&mut self.quad_angle, 1.0..=40.0).text("Flat within").suffix(" deg"));
                if let Some(m) = &self.geometry_message {
                    ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
                }
                if ui.add_enabled(!self.cut_undo.is_empty(), egui::Button::new("Take the last change back")).clicked() {
                    self.undo_cut(mesh);
                    self.selected = vec![false; mesh.verts.len()];
                }
                ui.label(RichText::new("The orange dots are the picked points. The bones and the weights follow the points.").size(11.0).color(TEXT_DIM));
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
                if let Some((part, _, v0, t0)) = self.attach {
                    ui.label(RichText::new("Presets").size(12.0).color(TEXT_DIM));
                    ui.horizontal_wrapped(|ui| {
                        for (name, params) in part.presets() {
                            if widgets::chip(ui, name, false).help("The numbers of this kind: change them afterwards like any others").clicked() {
                                self.attach = Some((part, params, v0, t0));
                                self.attach_apply(mesh);
                            }
                        }
                    });
                }
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
                    if widgets::chip(ui, "Hair", self.hair).help("Draw one line for a strand of hair: the strand is made along it and its bones are made and rigged, so it swings like hair when the head moves").clicked() {
                        self.hair = true;
                        self.brush = self.brush.with_kind(BrushKind::Card);
                        self.brush.width = 0.02;
                        self.brush.taper = 0.75;
                    }
                    for k in BrushKind::ALL {
                        if widgets::chip(ui, k.label(), self.brush.kind == k && !self.hair).help(k.about()).clicked() {
                            self.hair = false;
                            self.brush = self.brush.with_kind(k);
                        }
                    }
                });
                if self.hair {
                    ui.label(RichText::new("Kind of hair").size(12.0).color(TEXT_DIM));
                    ui.horizontal_wrapped(|ui| {
                        // every kind is one line with its bones inside: the bones are along the line you draw, the curl is on the strand
                        for (name, tip, kind, width, taper, curl, size, turn) in [
                            ("Straight", "A flat strand of straight hair", BrushKind::Card, 0.02, 0.75, Curl::None, 0.0, 0.05),
                            ("Wavy", "A strand that waves to the side as it goes", BrushKind::Card, 0.02, 0.7, Curl::Wave, 0.012, 0.09),
                            ("Curly", "A thin round strand that curls round the line you draw: ringlets", BrushKind::Strand, 0.009, 0.5, Curl::Spiral, 0.012, 0.05),
                            ("Coil", "A thick strand with big tight turns: coils, springs", BrushKind::Strand, 0.012, 0.4, Curl::Spiral, 0.022, 0.045),
                            ("Locks", "A wide flat clump of hair with a soft wave", BrushKind::Card, 0.05, 0.55, Curl::Wave, 0.008, 0.14),
                            ("Spiky", "A short stiff point that stands out", BrushKind::Card, 0.03, 0.95, Curl::None, 0.0, 0.05),
                        ] {
                            let on = self.brush.kind == kind && self.brush.curl == curl && (self.brush.width - width).abs() < 1e-4;
                            if widgets::chip(ui, name, on).help(tip).clicked() {
                                self.hair = true;
                                self.brush = self.brush.with_kind(kind);
                                (self.brush.width, self.brush.taper, self.brush.curl, self.brush.curl_size, self.brush.curl_len) = (width, taper, curl, size, turn);
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (c, label) in [(Curl::None, "No curl"), (Curl::Wave, "Wave"), (Curl::Spiral, "Spiral")] {
                            if widgets::chip(ui, label, self.brush.curl == c).clicked() {
                                self.brush.curl = c;
                            }
                        }
                    });
                    if self.brush.curl != Curl::None {
                        ui.add(egui::Slider::new(&mut self.brush.curl_size, 0.002..=0.06).text("Curl size").suffix(" m")).help("How far the strand goes from the line you draw");
                        ui.add(egui::Slider::new(&mut self.brush.curl_len, 0.01..=0.3).logarithmic(true).text("One turn").suffix(" m")).help("How long one turn or wave is");
                    }
                    section_title(ui, "Hair texture");
                    ui.label(RichText::new("A hair texture is a rectangle (like the hair texture of VRoid): across the line, the root at the top, the end at the bottom. It covers the whole of every line of hair that uses it. The hair brush draws with the picked one.").size(11.0).color(TEXT_DIM));
                    ui.horizontal_wrapped(|ui| {
                        for i in 0..self.hair_textures.len() {
                            if widgets::chip(ui, &self.hair_textures[i].name.clone(), self.hair_cur == i).help("Draw hair with this texture").clicked() {
                                self.hair_cur = i;
                                self.hair_editor = None;
                            }
                        }
                        if self.hair_textures.is_empty() {
                            ui.label(RichText::new("The first one is made with the first line of hair.").size(11.0).color(TEXT_DIM));
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("New").help("Another hair texture, from the root and end colours").clicked() {
                            self.new_hair_texture(mesh);
                        }
                        if ui.button("Import a picture...").help("A picture file becomes a hair texture").clicked() {
                            if let Some(p) = rfd::FileDialog::new().set_title("A hair texture").add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                                self.import_hair_texture(mesh, &p);
                            }
                        }
                        if ui.button("Draw it...").help("Open the picked texture and draw it by hand, with the brush of the Draw tab").clicked() {
                            self.open_hair_editor(mesh);
                        }
                    });
                }
                if self.hair {
                    ui.label(RichText::new("One line is one strand: from the root outwards, the way the hair lies. The strand and the bones that make it swing come from the line. Put the bones of a person in first (the Bones and weights page), so the hair hangs from the head. Then play an animation on the Animate by hand page.").size(11.0).color(TEXT_DIM));
                }
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
                section_title(ui, "Shapes");
                ui.horizontal_wrapped(|ui| {
                    for shape in meshops::Shape::ALL {
                        let on = self.shape_pending.is_some_and(|s| s.0 == shape);
                        if widgets::chip(ui, shape.label(), on).help("Put this shape on the mesh. Change its numbers, then keep it, and move, turn or enlarge it like any drawn thing").clicked() {
                            self.shape_start(mesh, shape);
                        }
                    }
                });
                if let Some((shape, mut p, mut at, v0, t0)) = self.shape_pending {
                    let mut changed = false;
                    changed |= ui.add(egui::Slider::new(&mut p.size, 0.005..=0.6).logarithmic(true).text("Size").suffix(" m")).changed();
                    if matches!(shape, meshops::Shape::Cylinder | meshops::Shape::Cone) {
                        changed |= ui.add(egui::Slider::new(&mut p.height, 0.01..=1.0).logarithmic(true).text("Height").suffix(" m")).changed();
                    }
                    changed |= ui.add(egui::Slider::new(&mut p.segments, 3..=96).text(if matches!(shape, meshops::Shape::Cube | meshops::Shape::Plane) { "Points along a side" } else { "Points round" })).help("How many points the shape has: more is rounder and can be bent and sculpted finer").changed();
                    if matches!(shape, meshops::Shape::Ball | meshops::Shape::Cylinder | meshops::Shape::Cone) {
                        changed |= ui.add(egui::Slider::new(&mut p.rings, 2..=96).text("Rings")).changed();
                    }
                    ui.checkbox(&mut self.shape_place, "Click the mesh to put it there").help("Click or drag on the mesh and the shape goes there (it is turned off to draw strokes while a shape is being made)");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Place").size(12.0).color(TEXT_DIM));
                        for (k, axis) in ["x", "y", "z"].into_iter().enumerate() {
                            changed |= ui.add(egui::DragValue::new(&mut at[k]).speed(0.005).prefix(format!("{axis} ")).suffix(" m")).changed();
                        }
                    });
                    if changed {
                        self.shape_pending = Some((shape, p, at, v0, t0));
                        self.shape_apply(mesh);
                    }
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("Keep it").strong()).clicked() {
                            self.shape_keep(mesh);
                        }
                        if ui.button("Take it away").clicked() {
                            self.shape_drop(mesh);
                        }
                    });
                }
                self.strands_ui(ui, mesh);
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
                ui.add_space(4.0);
                section_title(ui, "Fit the mesh to the bones");
                ui.label(RichText::new("A body bends well when it has points where the bones bend: elbows, knees, hips, shoulders. This cuts the faces near the joints smaller.").size(11.0).color(TEXT_DIM));
                ui.add(egui::Slider::new(&mut self.joint_radius, 0.02..=0.4).text("Near a joint").suffix(" m"));
                ui.add(egui::Slider::new(&mut self.joint_levels, 1..=3).text("How many times"));
                if ui.button("Add points at the joints").clicked() {
                    self.fit_joints(mesh);
                }
                if let Some(m) = &self.geometry_message {
                    ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
                }
            }
            Tool::Texture => {
                section_title(ui, "Paint the texture");
                ui.horizontal(|ui| {
                    if ui.color_edit_button_srgb(&mut self.color).changed() {
                        self.color_dirty = true;
                    }
                    if widgets::chip(ui, "Pick from the mesh", self.picking).help("Click the mesh to take its colour. Alt does the same while painting.").clicked() {
                        self.picking = !self.picking;
                    }
                });
                if ui.button(if self.brush_window { "Close the brushes" } else { "Pick a brush..." }).help("The brushes of the Draw tab: every brush, its settings and its textures (cloth, leopard dots, scales, fur...)").clicked() {
                    self.brush_window = !self.brush_window;
                }
                if self.kit.is_some() {
                    ui.label(RichText::new("It paints with the brush you picked in the Draw tab: every brush, its size, shape, dynamics and texture, and the colour. The eraser and the smudge of the Draw tab work here too.").size(11.0).color(TEXT_DIM));
                    ui.add(egui::Slider::new(&mut self.tex_size_mul, 0.2..=4.0).text("Brush size here")).help("How big the brush of the Draw tab is on the mesh (as a multiplier)");
                } else {
                    ui.add(egui::Slider::new(&mut self.tex_radius, 3.0..=120.0).text("Size")).help("The size of the brush in pixels");
                    ui.add(egui::Slider::new(&mut self.tex_opacity, 0.05..=1.0).text("Strength")).help("How much colour one touch puts on");
                }
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
        if self.has_hair() {
            section_title(ui, "Hair");
            if ui.checkbox(&mut self.hair_physics, "The hair moves").help("The strands of hair swing when the head moves, here and in the files (the motion goes in the keys of the animations)").changed() {
                self.hair_sim = None;
            }
            ui.add(egui::Slider::new(&mut self.hair_params.stiffness, 0.0..=1.0).text("Stiff")).help("0: the hair hangs free, 1: it keeps its shape");
            ui.add(egui::Slider::new(&mut self.hair_params.damping, 0.6..=0.99).text("Keeps swinging")).help("Higher: it swings for longer");
            ui.add(egui::Slider::new(&mut self.hair_params.gravity, 0.0..=20.0).text("Falls"));
            if ui.small_button("Reset the hair").clicked() {
                self.hair_sim = None;
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

    /// The strokes that were drawn: pick one, make it thicker or thinner, move it, and drag the handle at the end of its line to make it longer or shorter.
    fn strands_ui(&mut self, ui: &mut Ui, mesh: &mut AssetMesh) {
        if self.strands.is_empty() {
            return;
        }
        section_title(ui, "What was drawn");
        egui::ScrollArea::vertical().id_salt("mesh_strands").max_height(120.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for i in 0..self.strands.len() {
                    if widgets::chip(ui, &self.strands[i].name.clone(), self.strand_sel == Some(i)).clicked() {
                        self.strand_sel = Some(i);
                    }
                }
            });
        });
        let Some(i) = self.strand_sel.filter(|i| *i < self.strands.len()) else {
            ui.label(RichText::new("Pick one of them (or click it in the picture) to change it.").size(11.0).color(TEXT_DIM));
            return;
        };
        if let Some(cur) = self.strands[i].tex {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Texture").size(12.0).color(TEXT_DIM));
                for k in 0..self.hair_textures.len() {
                    if widgets::chip(ui, &self.hair_textures[k].name.clone(), cur == k).help("This line of hair looks into this texture").clicked() && cur != k {
                        self.strand_set_hair_texture(mesh, i, k);
                    }
                }
            });
            if ui.small_button("All the hair uses the picked texture").help("Every line of hair gets the texture that the hair brush draws with").clicked() {
                for j in 0..self.strands.len() {
                    if self.strands[j].tex.is_some() && self.strands[j].tex != Some(self.hair_cur) {
                        self.strand_set_hair_texture(mesh, j, self.hair_cur);
                    }
                }
            }
        }
        if !self.strands[i].line.is_empty() {
            let mut mul = self.strands[i].width_mul;
            if ui.add(egui::Slider::new(&mut mul, 0.3..=4.0).text("Thickness")).help("How thick it is, as a multiplier of how it was drawn").changed() {
                self.strand_thickness(mesh, i, mul);
            }
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Move").size(12.0).color(TEXT_DIM));
            for (k, axis) in ["x", "y", "z"].into_iter().enumerate() {
                let mut d = 0.0f32;
                if ui.add(egui::DragValue::new(&mut d).speed(0.002).prefix(format!("{axis} ")).suffix(" m")).help("Drag, or type how far (metres): right, up, towards the front").changed() {
                    let mut v = [0.0; 3];
                    v[k] = d;
                    self.strand_translate(mesh, i, v);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Turn").size(12.0).color(TEXT_DIM));
            for (k, axis) in ["x", "y", "z"].into_iter().enumerate() {
                let mut d = 0.0f32;
                if ui.add(egui::DragValue::new(&mut d).speed(0.5).prefix(format!("{axis} ")).suffix(" deg")).help("Drag, or type how many degrees: turned about its root").changed() {
                    self.strand_rotate(mesh, i, k, d);
                }
            }
        });
        let mut grow = 1.0f32;
        if ui.add(egui::DragValue::new(&mut grow).speed(0.005).prefix("Bigger x ").range(0.2..=5.0)).help("Drag to make it bigger (above 1) or smaller (below 1), about its root").changed() {
            self.strand_scale(mesh, i, grow);
        }
        ui.horizontal(|ui| {
            if ui.button("Merge it into the mesh").help("It becomes a part of the mesh: it keeps its place and its bones, but it is not in this list any more and cannot be moved on its own").clicked() {
                self.strand_merge(i);
            }
            if ui.button("Merge all").clicked() {
                self.strands.clear();
                self.strand_sel = None;
            }
        });
        ui.checkbox(&mut self.strand_move, "Drag it with the mouse").help("Dragging in the picture moves the picked line (instead of drawing a new one). The round handle at the end of a line always makes it longer or shorter.");
        ui.label(RichText::new("Drag the round handle at the end of a line to make it longer or shorter, if it was drawn too short.").size(11.0).color(TEXT_DIM));
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
        // the brush of the Draw tab is the brush of the texture here, and the colour goes both ways
        if self.m3.hand.color_dirty {
            let c = self.m3.hand.color;
            self.draw.primary = Color32::from_rgb(c[0], c[1], c[2]);
            self.m3.hand.color_dirty = false;
        }
        let (settings, color, erase) = self.draw.brush_snapshot();
        self.m3.hand.color = [color.r(), color.g(), color.b()];
        self.m3.hand.kit = Some(PaintKit { settings, color, erase });
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
            if hand.has_hair() {
                hand.apply_hair(&*mesh, dt);
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
        hand.hair_editor_ui(&ui.ctx().clone(), &mut *mesh);
        if hand.brush_window {
            let mut open = true;
            egui::Window::new(RichText::new("Brushes").strong()).id(egui::Id::new("brushes_3d")).open(&mut open).default_size([380.0, 620.0]).show(&ui.ctx().clone(), |ui| {
                egui::ScrollArea::vertical().id_salt("brushes_3d_scroll").show(ui, |ui| self.draw.brush_window_ui(ui));
            });
            if !open {
                hand.brush_window = false;
            }
        }
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
        match self.m3.mesh.as_ref() {
            Some(m) => self.m3.hand.with_hair(all, m),
            None => all,
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

    /// The rigged mesh is saved as an .fbx for Unreal, with the animations when asked, and its texture next to it.
    fn mesh_save_fbx(&mut self) {
        let Some(mesh) = &self.m3.mesh else { return };
        let name = self.m3.file_name();
        let Some(p) = rfd::FileDialog::new().set_title("Save for Unreal").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.fbx")).add_filter("FBX", &["fbx"]).save_file() else { return };
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(name);
        let anims = if self.m3.hand.with_animations { self.mesh_all_anims(true) } else { Vec::new() };
        let Some(mesh) = &self.m3.mesh else { return };
        let quads = self.m3.hand.quads_for_export(mesh);
        let ex = crate::fbx::write_fbx_polys(mesh, Some(&self.m3.hand.rig), &anims, self.m3.anim.fps as f32, &stem, quads);
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

    #[test]
    fn the_brush_of_the_draw_tab_paints_the_texture_of_the_mesh_through_the_engine() {
        let (mut mesh, mut hand, cam) = mirror_setup();
        hand.mirror = false;
        let mut settings = BrushSettings::default();
        settings.size = 18.0;
        settings.opacity = 1.0;
        settings.stabilize = 0.0;
        hand.kit = Some(PaintKit { settings, color: egui::Color32::from_rgb(20, 220, 40), erase: false });
        let before = mesh.texture.clone().unwrap();
        let a = cam.project([-0.05, 0.9, 0.1]);
        let b = cam.project([0.05, 1.3, 0.1]);
        assert!(hand.tex_begin(&mut mesh, &cam, Pos2::new(a[0], a[1])));
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            hand.tex_move(&mut mesh, &cam, Pos2::new(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t));
        }
        hand.tex_end(&mut mesh);
        assert!(hand.tex_session.is_none());
        let after = mesh.texture.as_ref().unwrap();
        let green = after.pixels().filter(|p| p.0[1] > 180 && p.0[0] < 80).count();
        assert!(green > 30, "the engine painted {green} green texels");
        assert_ne!(&before, after);
        // a stroke can be taken back like the others
        hand.texture_undo(&mut mesh);
        assert_eq!(mesh.texture.as_ref().unwrap(), &before);
    }

    #[test]
    fn a_loop_picks_points_and_the_faces_can_be_extruded_with_their_weights_and_deleted() {
        let (mut mesh, mut hand, cam) = sculpt_setup();
        hand.rig.add_bone("a", None, [0.0; 3], [0.0, 1.0, 0.0]);
        hand.rig.assign_nearest_parts(&mesh.verts);
        // a loop round the middle of the grid
        let c = centre_px(&cam);
        hand.lasso = vec![Pos2::new(c.x - 30.0, c.y - 30.0), Pos2::new(c.x + 30.0, c.y - 30.0), Pos2::new(c.x + 30.0, c.y + 30.0), Pos2::new(c.x - 30.0, c.y + 30.0)];
        hand.finish_faces(&mesh, &cam, egui::Modifiers::NONE);
        let picked = hand.selected.iter().filter(|s| **s).count();
        assert!(picked > 5 && picked < mesh.verts.len(), "{picked}");
        // shift adds a second loop, ctrl takes it away
        hand.lasso = vec![Pos2::new(c.x - 30.0, c.y - 30.0), Pos2::new(c.x + 30.0, c.y - 30.0), Pos2::new(c.x + 30.0, c.y + 30.0), Pos2::new(c.x - 30.0, c.y + 30.0)];
        hand.finish_faces(&mesh, &cam, egui::Modifiers { command: true, ..Default::default() });
        assert_eq!(hand.selected.iter().filter(|s| **s).count(), 0, "the same loop with ctrl takes it all away");
        hand.selected = vec![true; mesh.verts.len()];
        // extrude everything: the weights of the new points are those of the old ones
        let (v0, sel) = (mesh.verts.len(), hand.selected.clone());
        hand.edit_mesh(&mut mesh, |_, m| {
            let made = meshedit::extrude(m, &sel, 0.05)?;
            let map: Vec<Option<u32>> = (0..m.verts.len() as u32).map(Some).collect();
            *m = made.mesh;
            Some((map, made.copy_of, made.selection))
        });
        assert!(mesh.verts.len() > v0);
        assert_eq!(hand.rig.weights.len(), mesh.verts.len());
        assert!(hand.rig.weights.iter().all(|w| !w.is_empty()), "every point, new or old, has its bone");
        assert_eq!(hand.selected.len(), mesh.verts.len());
        // delete what is picked: only what is not picked is left
        let sel = hand.selected.clone();
        let before = mesh.verts.len();
        hand.edit_mesh(&mut mesh, |_, m| {
            let (new, map) = meshedit::delete_selected(m, &sel);
            *m = new;
            Some((map, Vec::new(), Vec::new()))
        });
        assert!(mesh.verts.len() < before);
        assert_eq!(hand.rig.weights.len(), mesh.verts.len());
        hand.undo_cut(&mut mesh);
        assert_eq!(mesh.verts.len(), before);
    }

    /// A hair stroke drawn on the figure with the bones of a person in it.
    fn hair_figure() -> (AssetMesh, HandRig, Camera) {
        let (mut mesh, mut hand, cam) = mirror_setup();
        // a texture that is big enough for the pictures of hair (rectangles in the strip)
        mesh.texture = Some(RgbaImage::from_pixel(1024, 1024, image::Rgba([200, 160, 140, 255])));
        hand.mirror = false;
        let (lo, hi) = mesh.bounds();
        hand.rig = Rig3::humanoid(mesh.verts.len(), lo, hi);
        hand.rig.assign_nearest_parts(&mesh.verts);
        hand.hair = true;
        hand.brush = hand.brush.with_kind(BrushKind::Card);
        hand.brush.width = 0.03;
        let x = cam.project([0.0, 1.9, 0.1])[0];
        hand.stroke = (0..20).map(|i| Pos2::new(x - 20.0 + i as f32 * 0.5, 40.0 + i as f32 * 8.0)).collect();
        hand.finish_stroke(&mut mesh, &cam);
        (mesh, hand, cam)
    }

    #[test]
    fn one_line_of_hair_is_a_strand_with_a_chain_of_bones_that_it_is_skinned_to_and_it_goes_back_with_undo() {
        let (mut mesh, mut hand, _cam) = hair_figure();
        let hair: Vec<usize> = (0..hand.rig.bones.len()).filter(|i| hand.rig.bones[*i].name.starts_with("hair")).collect();
        assert!(hair.len() >= 3, "a chain of bones: {}", hair.len());
        assert!(hand.rig.bones[hair[0]].parent.is_some(), "it hangs from a bone of the body");
        assert_eq!(hand.rig.bones[hair[1]].parent, Some(hair[0]));
        assert_eq!(hand.strands.len(), 1);
        // the points of the strand are moved by the chain and by nothing else
        let (v0, v1) = (hand.strands[0].verts.0, hand.strands[0].verts.1);
        assert!(v1 > v0);
        for i in v0..v1 {
            assert!(hand.rig.weights[i].iter().all(|w| hair.contains(&(w.0 as usize))), "point {i}: {:?}", hand.rig.weights[i]);
            let sum: f32 = hand.rig.weights[i].iter().map(|w| w.1).sum();
            assert!((sum - 1.0).abs() < 1e-4);
        }
        // the root follows the first bone and the end follows the last
        assert!(hand.rig.weights[v0].iter().any(|w| w.0 as usize == hair[0]));
        assert!(hand.rig.weights[v1 - 1].iter().any(|w| w.0 as usize == *hair.last().unwrap()));
        // a stroke drawn again is another strand, and one step takes the last back with its bones
        let bones = hand.rig.bones.len();
        hand.undo_stroke(&mut mesh);
        assert!(hand.rig.bones.len() < bones && hand.strands.is_empty());
        assert!(!hand.has_hair());
    }

    #[test]
    fn a_strand_is_moved_made_thicker_and_made_longer_by_its_end_with_its_bones() {
        let (mut mesh, mut hand, _cam) = hair_figure();
        let (v0, v1) = (hand.strands[0].verts.0, hand.strands[0].verts.1);
        let first = hand.strands[0].bones.unwrap().0;
        let (p0, b0) = (mesh.verts[v0], hand.rig.bones[first].head);
        hand.strand_translate(&mut mesh, 0, [0.0, 0.05, 0.1]);
        assert!((mesh.verts[v0][2] - p0[2] - 0.1).abs() < 1e-5 && (hand.rig.bones[first].head[2] - b0[2] - 0.1).abs() < 1e-5, "the points and the bones move together");
        // thicker: the points are further from the line
        let spread = |m: &AssetMesh, h: &HandRig| {
            let line = &h.strands[0].line;
            (v0..v1).map(|i| { let q = near_on_line(line, m.verts[i]); ((m.verts[i][0] - q[0]).powi(2) + (m.verts[i][1] - q[1]).powi(2) + (m.verts[i][2] - q[2]).powi(2)).sqrt() }).fold(0.0f32, f32::max)
        };
        let thin = spread(&mesh, &hand);
        hand.strand_thickness(&mut mesh, 0, 2.0);
        assert!((spread(&mesh, &hand) / thin - 2.0).abs() < 0.05, "{} {}", spread(&mesh, &hand), thin);
        // longer: the end of the line goes further from the root, the thickness stays
        let (root, tip) = (hand.strands[0].line[0], *hand.strands[0].line.last().unwrap());
        let len = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let before = len(root, tip);
        let thick = spread(&mesh, &hand);
        hand.strand_set_tip(&mut mesh, 0, [root[0] + (tip[0] - root[0]) * 1.5, root[1] + (tip[1] - root[1]) * 1.5, root[2] + (tip[2] - root[2]) * 1.5]);
        let (r2, t2) = (hand.strands[0].line[0], *hand.strands[0].line.last().unwrap());
        assert!((len(r2, t2) / before - 1.5).abs() < 0.02, "{}", len(r2, t2) / before);
        assert!((spread(&mesh, &hand) - thick).abs() < thick * 0.1, "the thickness stays");
        let last = hand.strands[0].bones.unwrap();
        let tail = hand.rig.bones[last.0 + last.1 - 1].tail;
        assert!(len(tail, t2) < 0.02, "the bones follow the line");
    }

    #[test]
    fn hair_has_rectangular_textures_that_cover_every_line_and_each_line_can_use_another() {
        let (mut mesh, mut hand, cam) = hair_figure();
        let tex = mesh.texture.as_ref().unwrap();
        let (size, high) = tex.dimensions();
        let y_start = (hand.strip_v0.unwrap() * high as f32).ceil() as u32;
        let slots = HandRig::hair_slots(size, high, y_start);
        assert!(slots.len() >= 2, "{} places for pictures of hair", slots.len());
        let (rx, ry, rw, rh) = slots[0];
        assert!(rh > rw, "a tall rectangle, like the hair texture of VRoid: {rw} x {rh}");
        // the points of the line look into the rectangle, and the rectangle is covered from the top to the bottom
        let (v0, v1) = (hand.strands[0].verts.0, hand.strands[0].verts.1);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for i in v0..v1 {
            let (x, y) = (mesh.uvs[i][0] * size as f32, mesh.uvs[i][1] * high as f32);
            assert!(x >= rx as f32 && x <= (rx + rw) as f32 && y >= ry as f32 && y <= (ry + rh) as f32, "point {i} looks at {x} {y}");
            lo = lo.min(y);
            hi = hi.max(y);
        }
        assert!(lo < ry as f32 + rh as f32 * 0.1 && hi > ry as f32 + rh as f32 * 0.9, "the whole length of the line covers the whole rectangle: {lo} {hi}");
        assert_eq!(hand.strip_next, 0, "no cell was used");
        assert_eq!(hand.strands[0].tex, Some(0));
        // another texture: the next line of hair looks into it, and the first line can be changed to it
        hand.new_hair_texture(&mut mesh);
        assert_eq!((hand.hair_textures.len(), hand.hair_cur), (2, 1));
        let x = cam.project([0.0, 1.9, 0.1])[0];
        hand.stroke = (0..20).map(|i| Pos2::new(x + 10.0 + i as f32 * 0.5, 40.0 + i as f32 * 8.0)).collect();
        hand.finish_stroke(&mut mesh, &cam);
        assert_eq!(hand.strands.len(), 2);
        assert_eq!(hand.strands[1].tex, Some(1));
        let (r2x, ..) = slots[1];
        let second = hand.strands[1].verts.0;
        assert!(mesh.uvs[second][0] * size as f32 >= r2x as f32, "the second line looks into the second rectangle");
        hand.strand_set_hair_texture(&mut mesh, 0, 1);
        assert_eq!(hand.strands[0].tex, Some(1));
        for i in v0..v1 {
            let x = mesh.uvs[i][0] * size as f32;
            assert!(x >= r2x as f32 && x <= (r2x + rw) as f32, "the line moved to the second rectangle: {x}");
        }
        // a picture from a file is a texture of its own, as big as the place for it
        let dir = std::env::temp_dir().join("sevi_hair_tex");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("curls.png");
        RgbaImage::from_pixel(30, 90, image::Rgba([10, 240, 20, 255])).save(&file).unwrap();
        hand.import_hair_texture(&mut mesh, &file);
        assert_eq!((hand.hair_textures.len(), hand.hair_textures[2].name.as_str()), (3, "curls"));
        let (ix, iy, ..) = HandRig::hair_slots(size, high, y_start)[2];
        let px = mesh.texture.as_ref().unwrap().get_pixel(ix + 30, iy + 100).0;
        assert!(px[1] > 200 && px[0] < 60, "{px:?}");
        // the cells of the strokes keep out of the block of the pictures
        let block = HandRig::hair_block(&slots);
        let cells = HandRig::strip_cells(size, high, y_start, block);
        let (bx, by, bw, bh) = block.unwrap();
        assert!(cells.iter().all(|(cx, cy)| !(cx + 32 > bx && *cx < bx + bw && cy + 32 > by && *cy < by + bh)));
        assert!(!cells.is_empty());
        // opened, it has the pixels of the texture
        hand.hair_cur = 0;
        hand.open_hair_editor(&mut mesh);
        let ed = hand.hair_editor.as_ref().expect("the editor is open");
        assert_eq!(ed.region, slots[0]);
        let c = crate::blend::from_px(ed.doc.flatten_tile((0, 0))[0]);
        let p = mesh.texture.as_ref().unwrap().get_pixel(rx, ry).0;
        assert!((c.r() as i32 - p[0] as i32).abs() <= 2, "{c:?} {p:?}");
    }

    #[test]
    fn a_shape_goes_where_the_mesh_is_clicked_and_not_onto_itself() {
        let (mut mesh, mut hand, cam) = mirror_setup();
        hand.shape_start(&mut mesh, meshops::Shape::Ball);
        let before = hand.shape_pending.unwrap().2;
        let p = cam.project([0.1, 1.5, 0.1]);
        // the ids are of the mesh as it was last drawn: made again here
        let (_, depth, ids) = mesh.render_full(&mesh.verts, &cam, false, crate::assetview::View::Textured, None);
        hand.depth = depth;
        hand.ids = ids;
        hand.place_shape_at(&mut mesh, &cam, Pos2::new(p[0], p[1]));
        let after = hand.shape_pending.unwrap().2;
        assert_ne!(before, after, "the shape moved to where the mesh was clicked");
        assert!(after[1] > 1.2 && after[1] < 1.8, "{after:?}: at the height that was clicked");
    }

    #[test]
    fn a_tail_is_rigged_with_a_chain_that_swings_and_wings_with_bones_on_each_side_and_both_go_back_with_undo() {
        let (mut mesh, mut hand, _cam) = hair_figure();
        hand.hair = false;
        let strands = hand.strands.len();
        let bones0 = hand.rig.bones.len();
        // a tail: a preset, kept
        hand.attach_start(&mut mesh, attach::Part::Tail);
        let (part, params) = (attach::Part::Tail, attach::Part::Tail.presets().into_iter().find(|p| p.0 == "Fox").unwrap().1);
        let (_, _, v0, t0) = hand.attach.unwrap();
        hand.attach = Some((part, params, v0, t0));
        hand.attach_apply(&mut mesh);
        let v_end = mesh.verts.len();
        hand.attach_keep(&mesh);
        let tail: Vec<usize> = (bones0..hand.rig.bones.len()).collect();
        assert_eq!(tail.len(), 6, "six bones in a chain");
        assert!(tail.iter().all(|i| hand.rig.bones[*i].name.starts_with("tail_")));
        assert!(hand.rig.bones[tail[0]].parent.is_some(), "it hangs from a bone of the body");
        assert_eq!(hand.rig.bones[tail[1]].parent, Some(tail[0]));
        assert!(hand.has_hair(), "a tail swings like hair");
        for j in v0..v_end {
            assert!(hand.rig.weights[j].iter().all(|w| tail.contains(&(w.0 as usize))), "point {j}: {:?}", hand.rig.weights[j]);
        }
        // it goes with the strand record: moved, its bones move too
        let b0 = hand.rig.bones[tail[0]].head;
        let idx = hand.strands.len() - 1;
        hand.strand_translate(&mut mesh, idx, [0.0, 0.0, -0.1]);
        assert!((hand.rig.bones[tail[0]].head[2] - b0[2] + 0.1).abs() < 1e-5);
        // wings: a chain on each side
        hand.attach_start(&mut mesh, attach::Part::Wings);
        hand.attach_keep(&mesh);
        let names: Vec<&str> = hand.rig.bones.iter().map(|b| b.name.as_str()).filter(|n| n.starts_with("wing_")).collect();
        assert!(names.iter().any(|n| n.starts_with("wing_L")) && names.iter().any(|n| n.starts_with("wing_R")), "{names:?}");
        // taking them back takes the bones
        hand.undo_stroke(&mut mesh);
        assert!(!hand.rig.bones.iter().any(|b| b.name.starts_with("wing_")));
        hand.undo_stroke(&mut mesh);
        assert_eq!(hand.rig.bones.len(), bones0);
        assert_eq!(hand.strands.len(), strands);
    }
}
