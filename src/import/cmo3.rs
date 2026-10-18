// SPDX-License-Identifier: GPL-3.0-or-later
// Opens a Live2D Cubism model file (.cmo3) as a SeviathStudio project: every art mesh becomes a layer with its own
// mesh, parameters become sliders, and warp and rotation deformers become Warp and Rotation changers with their
// keyforms. What does not fit this app's rig is listed in the report instead of being dropped silently.
//
// How the pieces map:
//  - Live2D positions are in the space of the deformer they sit in: 0..1 inside a warp, pixels around the origin of a
//    rotation, canvas pixels at the top. Here a changer works on the picture as it is at rest, so every position is
//    first carried to that rest space (`Space`), and a warp's grid is laid out evenly over the box of its default shape.
//  - The art of a mesh is cut out of its texture with the mesh's UV map and painted into a layer at the rest positions.
//  - A mesh whose own shape changes with a slider (eyelids, mouth) gets a small Warp fitted to those shapes.

use std::collections::HashMap;
use std::sync::Arc;

use super::caff::Caff;
use super::xml::{Dom, NodeId};
use crate::doc::{Document, LayerKind};
use crate::rig::{ArtMesh, Binding, Ease, Form, Id, Kind, MAX_WARP, Rig};

/// What was converted and what was not.
#[derive(Default, Debug, Clone)]
pub struct Report {
    pub params: usize,
    pub meshes: usize,
    pub warps: usize,
    pub rotations: usize,
    /// Meshes that change their own shape with a slider (turned into small warps).
    pub morphs: usize,
    pub notes: Vec<String>,
}

pub struct Imported {
    pub doc: Document,
    pub report: Report,
    pub name: String,
}

const MAX_CANVAS: u32 = 16_000;
const MAX_MESH_PIXELS: u64 = 64_000_000;

/// Where a deformer's own coordinates live, carried to the rest picture.
#[derive(Clone, Copy, Debug)]
enum Space {
    Root,
    Warp { rect: [f32; 4] },
    /// A rotation: where it is, and how its frame is turned (base angle plus the angle at the default) and sized.
    Rot { origin: [f32; 2], deg: f32, scale: f32 },
}

impl Space {
    fn map(&self, p: [f32; 2]) -> [f32; 2] {
        match self {
            Space::Root => p,
            Space::Warp { rect } => [rect[0] + p[0] * (rect[2] - rect[0]), rect[1] + p[1] * (rect[3] - rect[1])],
            Space::Rot { origin, deg, scale } => {
                let (s, c) = deg.to_radians().sin_cos();
                [origin[0] + (p[0] * c - p[1] * s) * scale, origin[1] + (p[0] * s + p[1] * c) * scale]
            }
        }
    }
}

struct Axis {
    param: String,
    keys: Vec<f32>,
}

/// The shapes of a deformer or a mesh: one per combination of key points of its sliders.
struct Grid {
    axes: Vec<Axis>,
    /// (key number on each axis, the form)
    forms: Vec<(Vec<usize>, NodeId)>,
}

struct ParamInfo {
    id: Id,
    default: f32,
}

struct Deformer {
    node: NodeId,
    warp: bool,
    name: String,
    guid: String,
    target: String,
}

struct Converted {
    changer: Id,
    space: Space,
}

struct Importer<'a> {
    dom: &'a Dom,
    rig: Rig,
    params: HashMap<String, ParamInfo>,
    notes: Vec<String>,
    /// Counts of things that did not fit, by sentence.
    skipped: HashMap<&'static str, usize>,
    fixed_param: Option<Id>,
    done: HashMap<String, Converted>,
    deformers: Vec<Deformer>,
    by_guid: HashMap<String, usize>,
    report: Report,
}

fn guid_key(dom: &Dom, n: Option<NodeId>) -> String {
    n.and_then(|n| dom.key(n)).unwrap_or("").to_owned()
}

/// The element with the keyform grid and the names: one `super` down from a deformer or mesh, then another.
fn controllable(dom: &Dom, n: NodeId) -> Option<NodeId> {
    let a = dom.child(n, "super")?;
    dom.child(a, "super")
}

fn form_guid(dom: &Dom, form: NodeId) -> String {
    let acform = dom.child(form, "super").and_then(|a| dom.child(a, "super"));
    guid_key(dom, acform.and_then(|f| dom.child(f, "guid")))
}

impl<'a> Importer<'a> {
    fn skip(&mut self, what: &'static str) {
        *self.skipped.entry(what).or_default() += 1;
    }

    fn forms_of(&self, owner: NodeId) -> HashMap<String, NodeId> {
        let mut m = HashMap::new();
        if let Some(list) = self.dom.child(owner, "keyforms") {
            for f in self.dom.kids(list) {
                let f = self.dom.resolve(*f);
                m.insert(form_guid(self.dom, f), f);
            }
        }
        m
    }

    fn read_grid(&self, owner: NodeId) -> Grid {
        let dom = self.dom;
        let forms = self.forms_of(owner);
        let mut grid = Grid { axes: Vec::new(), forms: Vec::new() };
        let Some(g) = controllable(dom, owner).and_then(|c| dom.child(c, "keyformGridSource")).map(|g| dom.resolve(g)) else {
            // no grid: the only shape there is
            if let Some(f) = forms.values().next() {
                grid.forms.push((Vec::new(), *f));
            }
            return grid;
        };
        let mut axis_of: HashMap<NodeId, usize> = HashMap::new();
        if let Some(list) = dom.child(g, "keyformBindings") {
            for b in dom.kids(list) {
                let b = dom.resolve(*b);
                let param = guid_key(dom, dom.child(b, "parameterGuid"));
                let keys: Vec<f32> = dom.child(b, "keys").map(|k| dom.kids(k).iter().filter_map(|f| dom.text(*f).parse().ok()).collect()).unwrap_or_default();
                axis_of.insert(b, grid.axes.len());
                grid.axes.push(Axis { param, keys });
            }
        }
        if let Some(list) = dom.child(g, "keyformsOnGrid") {
            for kg in dom.kids(list) {
                let mut idx = vec![0usize; grid.axes.len()];
                if let Some(keys) = dom.child(*kg, "accessKey").and_then(|a| dom.child(a, "_keyOnParameterList")) {
                    for k in dom.kids(keys) {
                        let b = dom.child(*k, "binding").map(|b| dom.resolve(b));
                        let ki = dom.child_i(*k, "keyIndex").unwrap_or(0).max(0) as usize;
                        if let Some(ax) = b.and_then(|b| axis_of.get(&b)) {
                            idx[*ax] = ki;
                        }
                    }
                }
                let fg = guid_key(dom, dom.child(*kg, "keyformGuid"));
                if let Some(f) = forms.get(&fg) {
                    grid.forms.push((idx, *f));
                }
            }
        }
        grid
    }

    fn default_of(&self, param: &str) -> f32 {
        self.params.get(param).map(|p| p.default).unwrap_or(0.0)
    }

    /// The numbers of the shape at the sliders' default values (the shapes blended linearly).
    fn blend(&self, grid: &Grid, data: &dyn Fn(NodeId) -> Vec<f32>) -> Vec<f32> {
        let n = grid.axes.len();
        if n == 0 {
            return grid.forms.first().map(|(_, f)| data(*f)).unwrap_or_default();
        }
        // per axis: the two key numbers around the default and the share of the upper one
        let mut around: Vec<(usize, usize, f32)> = Vec::new();
        for ax in &grid.axes {
            let d = self.default_of(&ax.param);
            let mut ord: Vec<usize> = (0..ax.keys.len()).collect();
            ord.sort_by(|a, b| ax.keys[*a].total_cmp(&ax.keys[*b]));
            if ord.is_empty() {
                around.push((0, 0, 0.0));
                continue;
            }
            let k = |i: usize| ax.keys[ord[i]];
            if d <= k(0) {
                around.push((ord[0], ord[0], 0.0));
            } else if d >= k(ord.len() - 1) {
                around.push((ord[ord.len() - 1], ord[ord.len() - 1], 0.0));
            } else {
                let hi = (1..ord.len()).find(|i| k(*i) >= d).unwrap_or(ord.len() - 1);
                let (a, b) = (k(hi - 1), k(hi));
                around.push((ord[hi - 1], ord[hi], if b > a { (d - a) / (b - a) } else { 0.0 }));
            }
        }
        let lookup: HashMap<&Vec<usize>, NodeId> = grid.forms.iter().map(|(i, f)| (i, *f)).collect();
        let mut out: Vec<f32> = Vec::new();
        let mut total = 0.0;
        for corner in 0..(1u32 << n.min(10)) {
            let (mut w, mut idx) = (1.0f32, Vec::with_capacity(n));
            for (a, (lo, hi, t)) in around.iter().enumerate() {
                if corner >> a & 1 == 1 {
                    w *= t;
                    idx.push(*hi);
                } else {
                    w *= 1.0 - t;
                    idx.push(*lo);
                }
            }
            if w <= 0.0 {
                continue;
            }
            let Some(f) = lookup.get(&idx) else { continue };
            let d = data(*f);
            if out.is_empty() {
                out = vec![0.0; d.len()];
            }
            if d.len() != out.len() {
                continue;
            }
            for (o, v) in out.iter_mut().zip(&d) {
                *o += v * w;
            }
            total += w;
        }
        if total > 0.0 && total < 0.999 {
            for o in &mut out {
                *o /= total;
            }
        }
        out
    }

    /// The sliders and shapes of a grid as bindings of one changer.
    fn bind(&mut self, changer: Id, grid: &Grid, form_of: &dyn Fn(NodeId) -> Option<Form>, what: &str) {
        // sliders with a single key point say nothing; use the first two that do
        let mut used: Vec<usize> = (0..grid.axes.len()).filter(|a| grid.axes[*a].keys.len() >= 2 && self.params.contains_key(&grid.axes[*a].param)).collect();
        if used.len() > 2 {
            self.notes.push(format!("{what} follows {} sliders at once; only the first two are kept.", used.len()));
            used.truncate(2);
        }
        if used.is_empty() {
            return;
        }
        // the key number to use on the axes that are not kept: the one closest to the default
        let fixed: Vec<usize> = grid
            .axes
            .iter()
            .map(|ax| {
                let d = self.default_of(&ax.param);
                (0..ax.keys.len()).min_by(|a, b| (ax.keys[*a] - d).abs().total_cmp(&(ax.keys[*b] - d).abs())).unwrap_or(0)
            })
            .collect();
        let sorted = |a: usize| -> Vec<usize> {
            let mut o: Vec<usize> = (0..grid.axes[a].keys.len()).collect();
            o.sort_by(|x, y| grid.axes[a].keys[*x].total_cmp(&grid.axes[a].keys[*y]));
            o
        };
        let orders: Vec<Vec<usize>> = used.iter().map(|a| sorted(*a)).collect();
        let lookup: HashMap<&Vec<usize>, NodeId> = grid.forms.iter().map(|(i, f)| (i, *f)).collect();
        let nx = orders[0].len();
        let ny = orders.get(1).map(Vec::len).unwrap_or(1);
        let mut forms = Vec::with_capacity(nx * ny);
        for iy in 0..ny {
            for ix in 0..nx {
                let mut idx = fixed.clone();
                idx[used[0]] = orders[0][ix];
                if used.len() > 1 {
                    idx[used[1]] = orders[1][iy];
                }
                let Some(f) = lookup.get(&idx).and_then(|f| form_of(*f)) else {
                    self.notes.push(format!("{what} is missing a shape for one of its key points, so its slider was left out."));
                    return;
                };
                forms.push(f);
            }
        }
        let params: Vec<Id> = used.iter().map(|a| self.params[&grid.axes[*a].param].id).collect();
        if params.len() == 2 && params[0] == params[1] {
            return;
        }
        let keys: Vec<Vec<f32>> = used.iter().zip(&orders).map(|(a, o)| o.iter().map(|k| grid.axes[*a].keys[*k]).collect()).collect();
        self.rig.bindings.push(Binding { changer, params, keys, forms, ease: Ease::Linear });
    }

    /// A slider that never moves, to hold the shape of a changer that has no slider.
    fn fixed_param(&mut self) -> Id {
        if let Some(p) = self.fixed_param {
            return p;
        }
        let p = self.rig.add_param("Fixed shapes", 0.0, 1.0, 0.0);
        self.fixed_param = Some(p);
        p
    }

    fn bind_fixed(&mut self, changer: Id, form: Form) {
        let p = self.fixed_param();
        self.rig.bindings.push(Binding { changer, params: vec![p], keys: vec![vec![0.0]], forms: vec![form], ease: Ease::Linear });
    }

    fn space_of(&mut self, target: &str) -> (Option<Id>, Space) {
        if let Some(&i) = self.by_guid.get(target) {
            self.convert_deformer(i);
            if let Some(c) = self.done.get(target) {
                return (Some(c.changer), c.space);
            }
        }
        (None, Space::Root)
    }

    fn convert_deformer(&mut self, i: usize) {
        let (node, warp, name, guid, target) = {
            let d = &self.deformers[i];
            (d.node, d.warp, d.name.clone(), d.guid.clone(), d.target.clone())
        };
        if self.done.contains_key(&guid) {
            return;
        }
        // guard against a loop in the file
        self.done.insert(guid.clone(), Converted { changer: 0, space: Space::Root });
        let (parent, pspace) = self.space_of(&target);
        let grid = self.read_grid(node);
        let conv = if warp { self.convert_warp(node, &name, parent, pspace, &grid) } else { self.convert_rotation(node, &name, parent, pspace, &grid) };
        self.done.insert(guid, conv);
    }

    fn convert_warp(&mut self, node: NodeId, name: &str, parent: Option<Id>, pspace: Space, grid: &Grid) -> Converted {
        let dom = self.dom;
        let (cols, rows) = (dom.child_i(node, "col").unwrap_or(1).clamp(1, 200) as u32, dom.child_i(node, "row").unwrap_or(1).clamp(1, 200) as u32);
        let per_row = (cols + 1) as usize;
        let (nc, nr) = (cols.min(MAX_WARP), rows.min(MAX_WARP));
        if (nc, nr) != (cols, rows) {
            self.skip("warp deformers with a finer grid than 24 were made coarser");
        }
        let n_old = ((cols + 1) * (rows + 1)) as usize;
        // the grid of a shape in the rest space, made coarser if needed
        let points_of = |f: NodeId| -> Vec<f32> {
            let Some(p) = dom.child(f, "positions") else { return Vec::new() };
            let v = dom.numbers(p);
            if v.len() != n_old * 2 {
                return Vec::new();
            }
            if (nc, nr) == (cols, rows) {
                return v;
            }
            let at = |c: usize, r: usize, k: usize| v[(r * per_row + c) * 2 + k];
            let mut out = Vec::with_capacity(((nc + 1) * (nr + 1) * 2) as usize);
            for j in 0..=nr {
                for i in 0..=nc {
                    let (u, w) = (i as f32 / nc as f32 * cols as f32, j as f32 / nr as f32 * rows as f32);
                    let (c0, r0) = ((u.floor() as usize).min(cols as usize - 1), (w.floor() as usize).min(rows as usize - 1));
                    let (fu, fw) = (u - c0 as f32, w - r0 as f32);
                    for k in 0..2 {
                        let top = at(c0, r0, k) * (1.0 - fu) + at(c0 + 1, r0, k) * fu;
                        let bot = at(c0, r0 + 1, k) * (1.0 - fu) + at(c0 + 1, r0 + 1, k) * fu;
                        out.push(top * (1.0 - fw) + bot * fw);
                    }
                }
            }
            out
        };
        let to_rest = |v: &[f32]| -> Vec<[f32; 2]> { v.chunks_exact(2).map(|p| pspace.map([p[0], p[1]])).collect() };
        let default = self.blend(grid, &|f| points_of(f));
        let default_rest = to_rest(&default);
        let (cols, rows) = (nc, nr);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &default_rest {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if default_rest.is_empty() || !lo.iter().chain(&hi).all(|v| v.is_finite()) {
            self.skip("deformers with no usable shape were left out");
            return Converted { changer: 0, space: Space::Root };
        }
        let rect = [lo[0], lo[1], hi[0].max(lo[0] + 1.0), hi[1].max(lo[1] + 1.0)];
        let id = self.rig.add_changer(name, Kind::Warp { rect, cols, rows }, parent);
        if std::env::var("CMO3_DEBUG").is_ok() {
            eprintln!("warp {name} rect {rect:?} parent space {pspace:?} {cols}x{rows}");
        }
        self.report.warps += 1;
        let rest = Rig::warp_rest(rect, cols, rows);
        let offsets_of = |f: NodeId| -> Option<Form> {
            let v = points_of(f);
            if v.len() != rest.len() * 2 {
                return None;
            }
            let pts = to_rest(&v);
            Some(Form::Warp { offsets: pts.iter().zip(&rest).map(|(p, g)| [p[0] - g[0], p[1] - g[1]]).collect() })
        };
        let before = self.rig.bindings.len();
        self.bind(id, grid, &offsets_of, name);
        if self.rig.bindings.len() == before {
            // no slider moves it: keep the shape it has if it is not an even grid
            let off: Vec<[f32; 2]> = default_rest.iter().zip(&rest).map(|(p, g)| [p[0] - g[0], p[1] - g[1]]).collect();
            if off.iter().any(|o| o[0].abs() > 0.5 || o[1].abs() > 0.5) {
                self.bind_fixed(id, Form::Warp { offsets: off });
            }
        }
        Converted { changer: id, space: Space::Warp { rect } }
    }

    fn convert_rotation(&mut self, node: NodeId, name: &str, parent: Option<Id>, pspace: Space, grid: &Grid) -> Converted {
        let dom = self.dom;
        let nums = |f: NodeId| -> Vec<f32> { ["angle", "originX", "originY", "scale"].iter().map(|k| dom.attr_f(f, k).unwrap_or(if *k == "scale" { 1.0 } else { 0.0 })).collect() };
        let d = self.blend(grid, &nums);
        if d.len() < 4 {
            self.skip("deformers with no usable shape were left out");
            return Converted { changer: 0, space: Space::Root };
        }
        let origin = pspace.map([d[1], d[2]]);
        // the frame of a rotation is turned by its base angle even when the slider is at its default
        let base = dom.child_f(node, "baseAngle").unwrap_or(0.0);
        // inside another rotation the frame turns with it
        let (pdeg, pscale) = match pspace {
            Space::Rot { deg, scale, .. } => (deg, scale),
            _ => (0.0, 1.0),
        };
        let id = self.rig.add_changer(name, Kind::Rotation { pivot: origin }, parent);
        if std::env::var("CMO3_DEBUG").is_ok() {
            eprintln!("rotation {name} origin {origin:?} base {base} default {} parent space {pspace:?}", d[0]);
        }
        self.report.rotations += 1;
        // the pivot is one point here; a rotation whose origin or size changes with a slider is turned about the default one
        let mut moves = false;
        for (_, f) in &grid.forms {
            let v = nums(*f);
            let o = pspace.map([v[1], v[2]]);
            if (o[0] - origin[0]).hypot(o[1] - origin[1]) > 1.5 || (v[3] - d[3]).abs() > 0.02 {
                moves = true;
            }
        }
        if moves {
            self.skip("rotations whose centre or size changes with a slider keep one centre and one size");
        }
        // turns here are counted from the picture at the default, which already has the default angle in it
        let a0 = d[0];
        let form_of = |f: NodeId| -> Option<Form> { Some(Form::Rot { deg: nums(f)[0] - a0 }) };
        self.bind(id, grid, &form_of, name);
        Converted { changer: id, space: Space::Rot { origin, deg: pdeg + base + a0, scale: pscale * d[3] } }
    }
}

/// A texture picture and how the mesh's UV numbers map into it.
struct Texture {
    img: Arc<image::RgbaImage>,
    /// UV to pixel: x = a*u + b*v + c, y = d*u + e*v + f.
    m: [f32; 6],
}

fn texture_of(dom: &Dom, mesh: NodeId, caff: &Caff, cache: &mut HashMap<String, Option<Arc<image::RgbaImage>>>) -> Option<Texture> {
    let tex = dom.resolve(dom.child(mesh, "texture")?);
    let res = dom.resolve(dom.child(tex, "srcImageResource")?);
    let path = dom.attr(dom.child(res, "imageFileBuf")?, "path")?.to_owned();
    let img = cache
        .entry(path.clone())
        .or_insert_with(|| {
            let bytes = caff.read(&path).ok()?;
            image::load_from_memory(&bytes).ok().map(|i| Arc::new(i.to_rgba8()))
        })
        .clone()?;
    // the file says how the picture (0..1) sits in the logical texture (0..1) the UV numbers point into
    let a = dom.child(tex, "transformImageResource01toLogical01");
    let g = |k: &str, d: f32| a.and_then(|a| dom.attr_f(a, k)).unwrap_or(d);
    let (m00, m01, m02, m10, m11, m12) = (g("m00", 1.0), g("m01", 0.0), g("m02", 0.0), g("m10", 0.0), g("m11", 1.0), g("m12", 0.0));
    let det = m00 * m11 - m01 * m10;
    let (w, h) = (img.width() as f32, img.height() as f32);
    let inv = if det.abs() < 1e-9 {
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    } else {
        // resource = inverse(M) * logical
        [m11 / det, -m01 / det, (m01 * m12 - m11 * m02) / det, -m10 / det, m00 / det, (m10 * m02 - m00 * m12) / det]
    };
    Some(Texture { img, m: [inv[0] * w, inv[1] * w, inv[2] * w, inv[3] * h, inv[4] * h, inv[5] * h] })
}

fn sample(t: &Texture, u: f32, v: f32) -> [f32; 4] {
    let (x, y) = (t.m[0] * u + t.m[1] * v + t.m[2] - 0.5, t.m[3] * u + t.m[4] * v + t.m[5] - 0.5);
    let (w, h) = (t.img.width() as i32, t.img.height() as i32);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |xx: i32, yy: i32| -> [f32; 4] {
        let p = t.img.get_pixel(xx.clamp(0, w - 1) as u32, yy.clamp(0, h - 1) as u32).0;
        let a = p[3] as f32;
        // blend with the colour weighted by alpha so transparent edges do not bleed their colour
        [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, a]
    };
    let (a, b, c, d) = (px(x0, y0), px(x0 + 1, y0), px(x0, y0 + 1), px(x0 + 1, y0 + 1));
    let mut o = [0.0; 4];
    for k in 0..4 {
        o[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
    }
    if o[3] > 0.0 {
        [o[0] / o[3], o[1] / o[3], o[2] / o[3], o[3]]
    } else {
        [0.0; 4]
    }
}

/// Paint the mesh's triangles from the texture: a picture of its bounding box, and where its corner is.
fn paint_mesh(tex: &Texture, verts: &[[f32; 2]], uvs: &[[f32; 2]], tris: &[[u32; 3]], doc_w: u32, doc_h: u32) -> Option<(image::RgbaImage, i32, i32)> {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for v in verts {
        for k in 0..2 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let (x0, y0) = ((lo[0].floor() as i32).max(0), (lo[1].floor() as i32).max(0));
    let (x1, y1) = ((hi[0].ceil() as i32 + 1).min(doc_w as i32), (hi[1].ceil() as i32 + 1).min(doc_h as i32));
    if x1 <= x0 || y1 <= y0 || (x1 - x0) as u64 * (y1 - y0) as u64 > MAX_MESH_PIXELS {
        return None;
    }
    let mut img = image::RgbaImage::new((x1 - x0) as u32, (y1 - y0) as u32);
    for t in tris {
        let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
        let (ua, ub, uc) = (uvs[t[0] as usize], uvs[t[1] as usize], uvs[t[2] as usize]);
        let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if det.abs() < 1e-9 {
            continue;
        }
        let (tx0, tx1) = (((a[0].min(b[0]).min(c[0])).floor() as i32).max(x0), ((a[0].max(b[0]).max(c[0])).ceil() as i32 + 1).min(x1));
        let (ty0, ty1) = (((a[1].min(b[1]).min(c[1])).floor() as i32).max(y0), ((a[1].max(b[1]).max(c[1])).ceil() as i32 + 1).min(y1));
        for y in ty0..ty1 {
            for x in tx0..tx1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let l1 = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / det;
                let l2 = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / det;
                let l3 = 1.0 - l1 - l2;
                let e = -0.02;
                if l1 < e || l2 < e || l3 < e {
                    continue;
                }
                let (u, v) = (ua[0] * l1 + ub[0] * l2 + uc[0] * l3, ua[1] * l1 + ub[1] * l2 + uc[1] * l3);
                let s = sample(tex, u, v);
                if s[3] <= 0.0 {
                    continue;
                }
                let dst = img.get_pixel_mut((x - x0) as u32, (y - y0) as u32);
                if dst.0[3] == 0 || (s[3] as u8) >= dst.0[3] {
                    *dst = image::Rgba([s[0].round() as u8, s[1].round() as u8, s[2].round() as u8, s[3].round() as u8]);
                }
            }
        }
    }
    Some((img, x0, y0))
}

/// Fit a Warp (a grid of `cols` by `rows` over `rect`) to how the points of a mesh moved: control point shifts so that
/// the picture's displacement at each point is as close as it can be.
fn fit_warp(rest: &[[f32; 2]], moved: &[[f32; 2]], rect: [f32; 4], cols: u32, rows: u32) -> Vec<[f32; 2]> {
    let n = ((cols + 1) * (rows + 1)) as usize;
    let (w, h) = ((rect[2] - rect[0]).max(1e-3), (rect[3] - rect[1]).max(1e-3));
    let mut ata = vec![0.0f64; n * n];
    let mut atb = [vec![0.0f64; n], vec![0.0f64; n]];
    for (p, m) in rest.iter().zip(moved) {
        let u = ((p[0] - rect[0]) / w * cols as f32).clamp(0.0, cols as f32);
        let v = ((p[1] - rect[1]) / h * rows as f32).clamp(0.0, rows as f32);
        let (i, j) = ((u.floor() as u32).min(cols - 1), (v.floor() as u32).min(rows - 1));
        let (fu, fv) = ((u - i as f32) as f64, (v - j as f32) as f64);
        let idx = |c: u32, r: u32| (r * (cols + 1) + c) as usize;
        let cell = [(idx(i, j), (1.0 - fu) * (1.0 - fv)), (idx(i + 1, j), fu * (1.0 - fv)), (idx(i, j + 1), (1.0 - fu) * fv), (idx(i + 1, j + 1), fu * fv)];
        let d = [(m[0] - p[0]) as f64, (m[1] - p[1]) as f64];
        for (a, wa) in cell {
            for (b, wb) in cell {
                ata[a * n + b] += wa * wb;
            }
            atb[0][a] += wa * d[0];
            atb[1][a] += wa * d[1];
        }
    }
    for i in 0..n {
        ata[i * n + i] += 1e-2;
    }
    let sol: Vec<Vec<f64>> = atb.iter().map(|b| solve(ata.clone(), b.clone(), n)).collect();
    (0..n).map(|i| [sol[0][i] as f32, sol[1][i] as f32]).collect()
}

/// Gaussian elimination with pivoting for a small symmetric system.
fn solve(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Vec<f64> {
    for c in 0..n {
        let piv = (c..n).max_by(|x, y| a[x * n + c].abs().total_cmp(&a[y * n + c].abs())).unwrap_or(c);
        if a[piv * n + c].abs() < 1e-12 {
            continue;
        }
        if piv != c {
            for k in 0..n {
                a.swap(c * n + k, piv * n + k);
            }
            b.swap(c, piv);
        }
        let d = a[c * n + c];
        for r in c + 1..n {
            let f = a[r * n + c] / d;
            if f == 0.0 {
                continue;
            }
            for k in c..n {
                a[r * n + k] -= f * a[c * n + k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for k in r + 1..n {
            s -= a[r * n + k] * x[k];
        }
        let d = a[r * n + r];
        x[r] = if d.abs() < 1e-12 { 0.0 } else { s / d };
    }
    x
}

pub fn import_bytes(data: Vec<u8>) -> Result<Imported, String> {
    let caff = Caff::parse(data)?;
    let xml_bytes = caff.read("main.xml")?;
    let xml = String::from_utf8_lossy(&xml_bytes).into_owned();
    drop(xml_bytes);
    let dom = Dom::parse(&xml)?;
    drop(xml);
    let root = dom.nodes.iter().position(|n| n.tag == "CModelSource").ok_or("The file has no model in it.")?;
    let name = dom.child_text(root, "name").unwrap_or("Live2D model").chars().filter(|c| !c.is_control()).take(60).collect::<String>();
    let canvas = dom.child(root, "canvas");
    let (cw, ch) = (canvas.and_then(|c| dom.child_i(c, "pixelWidth")).unwrap_or(2000), canvas.and_then(|c| dom.child_i(c, "pixelHeight")).unwrap_or(2000));
    let (cw, ch) = (cw.clamp(16, MAX_CANVAS as i64) as u32, ch.clamp(16, MAX_CANVAS as i64) as u32);

    let mut imp = Importer { dom: &dom, rig: Rig::default(), params: HashMap::new(), notes: Vec::new(), skipped: HashMap::new(), fixed_param: None, done: HashMap::new(), deformers: Vec::new(), by_guid: HashMap::new(), report: Report::default() };

    // sliders
    let mut srcs = Vec::new();
    dom.find_all(root, "CParameterSource", &mut srcs);
    for p in srcs {
        let guid = guid_key(&dom, dom.child(p, "guid"));
        let (lo, hi) = (dom.child_f(p, "minValue").unwrap_or(0.0), dom.child_f(p, "maxValue").unwrap_or(1.0));
        if guid.is_empty() || imp.params.contains_key(&guid) || !(lo.is_finite() && hi.is_finite()) || lo >= hi {
            continue;
        }
        let def = dom.child_f(p, "defaultValue").unwrap_or(0.0);
        let label = dom.child_text(p, "name").filter(|s| !s.is_empty()).map(str::to_owned).or_else(|| dom.child(p, "id").and_then(|i| dom.attr(dom.resolve(i), "idstr")).map(str::to_owned)).unwrap_or_else(|| "Slider".into());
        let id = imp.rig.add_param(&label, lo, hi, def);
        // the Live2D id of the slider, for the physics, expressions and motions of the model
        if let Some(idstr) = dom.child(p, "id").and_then(|i| dom.attr(dom.resolve(i), "idstr")) {
            if let Some(pm) = imp.rig.params.last_mut() {
                pm.cid = idstr.to_string();
            }
        }
        imp.params.insert(guid, ParamInfo { id, default: def.clamp(lo, hi) });
        imp.report.params += 1;
    }

    // deformers
    for (tag, warp) in [("CWarpDeformerSource", true), ("CRotationDeformerSource", false)] {
        let mut all = Vec::new();
        dom.find_all(0, tag, &mut all);
        for n in all {
            if dom.attr(n, "xs.id").is_none() {
                continue;
            }
            let sup = dom.child(n, "super");
            let guid = guid_key(&dom, sup.and_then(|s| dom.child(s, "guid")));
            let target = guid_key(&dom, sup.and_then(|s| dom.child(s, "targetDeformerGuid")));
            let nm = controllable(&dom, n).and_then(|c| dom.child_text(c, "localName")).unwrap_or(if warp { "Warp" } else { "Rotation" }).to_owned();
            if guid.is_empty() || imp.by_guid.contains_key(&guid) {
                continue;
            }
            imp.by_guid.insert(guid.clone(), imp.deformers.len());
            imp.deformers.push(Deformer { node: n, warp, name: nm, guid, target });
        }
    }
    for i in 0..imp.deformers.len() {
        imp.convert_deformer(i);
    }

    // the parts tree: the order things are listed in (first = in front) and which parts are hidden or only a sketch
    struct Part {
        parent: String,
        visible: bool,
        sketch: bool,
        children: Vec<String>,
    }
    let mut parts: HashMap<String, Part> = HashMap::new();
    let mut part_nodes = Vec::new();
    dom.find_all(0, "CPartSource", &mut part_nodes);
    for n in part_nodes {
        if dom.attr(n, "xs.id").is_none() {
            continue;
        }
        let guid = guid_key(&dom, dom.child(n, "guid"));
        let ctl = dom.child(n, "super");
        parts.insert(
            guid,
            Part {
                parent: guid_key(&dom, ctl.and_then(|c| dom.child(c, "parentGuid"))),
                visible: ctl.and_then(|c| dom.child_text(c, "isVisible")).map(|v| v != "false").unwrap_or(true),
                sketch: dom.child_text(n, "isSketch") == Some("true"),
                children: dom.child(n, "_childGuids").map(|c| dom.kids(c).iter().map(|k| guid_key(&dom, Some(*k))).collect()).unwrap_or_default(),
            },
        );
    }
    // drawable guid: (position in the list, hidden by a part, only a sketch)
    let mut listed: HashMap<String, (usize, bool, bool)> = HashMap::new();
    {
        fn walk(parts: &HashMap<String, Part>, guid: &str, hidden: bool, sketch: bool, counter: &mut usize, out: &mut HashMap<String, (usize, bool, bool)>, depth: usize) {
            let Some(p) = parts.get(guid) else { return };
            if depth > 64 {
                return;
            }
            let (hidden, sketch) = (hidden || !p.visible, sketch || p.sketch);
            for c in &p.children {
                if parts.contains_key(c) {
                    walk(parts, c, hidden, sketch, counter, out, depth + 1);
                } else {
                    out.insert(c.clone(), (*counter, hidden, sketch));
                    *counter += 1;
                }
            }
        }
        let mut counter = 0;
        let roots: Vec<String> = parts.iter().filter(|(_, p)| p.parent.is_empty() || !parts.contains_key(&p.parent)).map(|(g, _)| g.clone()).collect();
        for r in roots {
            walk(&parts, &r, false, false, &mut counter, &mut listed, 0);
        }
    }
    // art meshes
    let mut meshes = Vec::new();
    dom.find_all(0, "CArtMeshSource", &mut meshes);
    meshes.retain(|n| dom.attr(*n, "xs.id").is_some() && dom.child(*n, "indices").is_some());
    struct Made {
        draw_order: i64,
        name: String,
        visible: bool,
        opacity: f32,
        tiles: std::collections::HashMap<crate::doc::TileKey, crate::doc::TileData>,
        mesh: ArtMesh,
    }
    let mut made: Vec<Made> = Vec::new();
    let mut cache: HashMap<String, Option<Arc<image::RgbaImage>>> = HashMap::new();
    let mut doc = Document::new(cw, ch);
    let mut clipped = 0usize;
    let mut sketches = 0usize;
    let mut no_texture = 0usize;
    for m in meshes {
        let sup = dom.child(m, "super");
        let ctl = sup.and_then(|s| dom.child(s, "super"));
        let nm = ctl.and_then(|c| dom.child_text(c, "localName")).unwrap_or("Mesh").to_owned();
        let visible = ctl.and_then(|c| dom.child_text(c, "isVisible")).map(|v| v == "true").unwrap_or(true);
        let dguid = guid_key(&dom, sup.and_then(|s| dom.child(s, "guid")));
        let (list_at, part_hidden, sketch) = listed.get(&dguid).copied().unwrap_or((usize::MAX / 2, false, false));
        if sketch {
            sketches += 1;
            continue;
        }
        let visible = visible && !part_hidden;
        let target = guid_key(&dom, sup.and_then(|s| dom.child(s, "targetDeformerGuid")));
        if sup.and_then(|s| dom.child(s, "clipGuidList")).is_some_and(|l| !dom.kids(l).is_empty()) {
            clipped += 1;
        }
        let tris_flat = dom.numbers(dom.child(m, "indices").unwrap());
        let uvs_flat = dom.child(m, "uvs").map(|u| dom.numbers(u)).unwrap_or_default();
        let nverts = uvs_flat.len() / 2;
        if nverts == 0 || nverts > 100_000 || tris_flat.len() < 3 || tris_flat.iter().any(|i| *i < 0.0 || *i as usize >= nverts) {
            continue;
        }
        let tris: Vec<[u32; 3]> = tris_flat.chunks_exact(3).map(|t| [t[0] as u32, t[1] as u32, t[2] as u32]).collect();
        let uvs: Vec<[f32; 2]> = uvs_flat.chunks_exact(2).map(|p| [p[0], p[1]]).collect();
        let (parent, mut space) = imp.space_of(&target);
        let grid = imp.read_grid(m);
        let pos_of = |f: NodeId| -> Vec<f32> { dom.child(f, "positions").map(|p| dom.numbers(p)).filter(|v| v.len() == nverts * 2).unwrap_or_default() };
        let mut local = imp.blend(&grid, &pos_of);
        if std::env::var("CMO3_DEBUG").is_ok() && local.len() != nverts * 2 {
            eprintln!("mesh {nm}: axes {} forms {} blend len {} want {} params {:?}", grid.axes.len(), grid.forms.len(), local.len(), nverts * 2, grid.axes.iter().map(|a| (a.param.clone(), imp.params.contains_key(&a.param), a.keys.len())).collect::<Vec<_>>());
        }
        if local.len() != nverts * 2 {
            // fall back to the canvas positions the mesh was made with
            local = dom.child(m, "positions").map(|p| dom.numbers(p)).filter(|v| v.len() == nverts * 2).unwrap_or_default();
            if local.len() != nverts * 2 {
                continue;
            }
            // canvas positions: already in the rest picture
            space = Space::Root;
            imp.skip("meshes had no shape for their sliders and were placed where they were drawn");
        }
        let verts: Vec<[f32; 2]> = local.chunks_exact(2).map(|p| space.map([p[0], p[1]])).collect();
        if verts.iter().any(|v| !v[0].is_finite() || !v[1].is_finite()) {
            continue;
        }
        // the art
        let Some(tex) = texture_of(&dom, m, &caff, &mut cache) else {
            no_texture += 1;
            continue;
        };
        let Some((img, ox, oy)) = paint_mesh(&tex, &verts, &uvs, &tris, cw, ch) else {
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for v in &verts {
                for k in 0..2 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            let tn = imp.by_guid.get(&target).map(|i| imp.deformers[*i].name.clone());
            imp.notes.push(format!("{nm} lies outside the canvas or is too big ({lo:?} to {hi:?}, inside {tn:?}, {space:?}, first local {:?}) and was left out.", &local[..4.min(local.len())]));
            continue;
        };
        let tiles = crate::files::tiles_from_rgba(&img, ox, oy, cw, ch);
        // draw order and opacity at the default slider values
        let order_opacity = imp.blend(&grid, &|f| {
            let head = dom.child(f, "super");
            vec![head.and_then(|d| dom.child_i(d, "drawOrder")).unwrap_or(500) as f32, head.and_then(|d| dom.child_f(d, "opacity")).unwrap_or(1.0)]
        });
        // the draw order number first, and for equal ones the order of the parts list (first listed is in front)
        let draw_order = order_opacity.first().copied().unwrap_or(500.0).round() as i64 * 1_000_000 - list_at as i64;
        let opacity = order_opacity.get(1).copied().unwrap_or(1.0).clamp(0.0, 1.0);
        let mut mesh_parent = parent;
        // a mesh that changes its own shape: a small warp fitted to those shapes
        let moves = grid.axes.iter().any(|a| a.keys.len() >= 2) && grid.forms.len() > 1;
        if moves {
            let shapes: Vec<(NodeId, Vec<[f32; 2]>)> = grid.forms.iter().filter_map(|(_, f)| Some((*f, pos_of(*f)))).filter(|(_, v)| v.len() == nverts * 2).map(|(f, v)| (f, v.chunks_exact(2).map(|p| space.map([p[0], p[1]])).collect())).collect();
            let big = shapes.iter().flat_map(|(_, s)| s.iter().zip(&verts).map(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()))).fold(0.0f32, f32::max);
            if big > 0.25 {
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for v in &verts {
                    for k in 0..2 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
                let pad = 0.02 * (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
                let rect = [lo[0] - pad, lo[1] - pad, (hi[0] + pad).max(lo[0] + 4.0), (hi[1] + pad).max(lo[1] + 4.0)];
                let side = ((nverts as f32).sqrt() / 2.0).round().clamp(2.0, 8.0) as u32;
                let w = imp.rig.add_changer(&format!("{nm} shape"), Kind::Warp { rect, cols: side, rows: side }, parent);
                let fits: HashMap<NodeId, Vec<[f32; 2]>> = shapes.iter().map(|(f, s)| (*f, fit_warp(&verts, s, rect, side, side))).collect();
                let before = imp.rig.bindings.len();
                imp.bind(w, &grid, &|f| fits.get(&f).map(|o| Form::Warp { offsets: o.clone() }), &nm);
                if imp.rig.bindings.len() > before {
                    mesh_parent = Some(w);
                    imp.report.morphs += 1;
                } else {
                    imp.rig.changers.retain(|c| c.id != w);
                }
            }
        }
        // opacity and draw order that change with a slider: Fade and Order changers
        let mut opacity = opacity;
        if grid.axes.iter().any(|a| a.keys.len() >= 2) && grid.forms.len() > 1 {
            let head = |f: NodeId| dom.child(f, "super");
            let ops: Vec<f32> = grid.forms.iter().map(|(_, f)| head(*f).and_then(|d| dom.child_f(d, "opacity")).unwrap_or(1.0)).collect();
            let ords: Vec<f32> = grid.forms.iter().map(|(_, f)| head(*f).and_then(|d| dom.child_i(d, "drawOrder")).unwrap_or(500) as f32).collect();
            let spread = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min);
            if spread(&ops) > 0.01 {
                let w = imp.rig.add_changer(&format!("{nm} opacity"), Kind::Fade, mesh_parent);
                let before = imp.rig.bindings.len();
                imp.bind(w, &grid, &|f| Some(Form::Vals { v: vec![head(f).and_then(|d| dom.child_f(d, "opacity")).unwrap_or(1.0).clamp(0.0, 1.0)] }), &nm);
                if imp.rig.bindings.len() > before {
                    mesh_parent = Some(w);
                    // the opacity now comes from the changer
                    opacity = 1.0;
                } else {
                    imp.rig.changers.retain(|c| c.id != w);
                }
            }
            if spread(&ords) > 0.5 {
                let def = order_opacity.first().copied().unwrap_or(500.0);
                let w = imp.rig.add_changer(&format!("{nm} order"), Kind::Order, mesh_parent);
                let before = imp.rig.bindings.len();
                imp.bind(w, &grid, &|f| Some(Form::Vals { v: vec![(head(f).and_then(|d| dom.child_i(d, "drawOrder")).unwrap_or(500) as f32 - def) * 0.1] }), &nm);
                if imp.rig.bindings.len() > before {
                    mesh_parent = Some(w);
                } else {
                    imp.rig.changers.retain(|c| c.id != w);
                }
            }
        }
        let id = imp.rig.fresh_id();
        made.push(Made { draw_order, name: nm, visible, opacity, tiles, mesh: ArtMesh { id, layer: 0, verts, tris, parent: mesh_parent } });
    }
    // the layers, the lowest draw order at the bottom
    made.sort_by_key(|m| m.draw_order);
    doc.layers.clear();
    for mut m in made {
        let mut l = doc.new_layer(&m.name, LayerKind::Raster);
        l.tiles = m.tiles;
        l.visible = m.visible;
        l.opacity = m.opacity * 100.0;
        l.rev += 1;
        m.mesh.layer = l.id;
        imp.rig.meshes.push(m.mesh);
        doc.layers.insert(0, l);
        imp.report.meshes += 1;
    }
    let paper = doc.new_layer("Paper", LayerKind::Paper);
    doc.layers.push(paper);
    if sketches > 0 {
        imp.notes.push(format!("{sketches} sketch meshes (reference drawings) were left out."));
    }
    if clipped > 0 {
        imp.skip("meshes use a clipping mask (not supported yet)");
        let _ = clipped;
    }
    if no_texture > 0 {
        imp.notes.push(format!("{no_texture} meshes have no picture and were left out."));
    }
    let mut notes = std::mem::take(&mut imp.notes);
    let mut sk: Vec<_> = imp.skipped.iter().collect();
    sk.sort();
    notes.extend(sk.into_iter().map(|(s, n)| format!("{n} {s}.")));
    let mut report = imp.report.clone();
    report.notes = notes;
    doc.rig = Arc::new(imp.rig.sanitized());
    doc.mark_all_dirty();
    Ok(Imported { doc, report, name })
}

pub fn import_file(path: &std::path::Path) -> Result<Imported, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("Cannot read the file: {e}"))?.len();
    if len > 2_000_000_000 {
        return Err("This model file is too big to open.".into());
    }
    import_bytes(std::fs::read(path).map_err(|e| format!("Cannot read the file: {e}"))?)
}
