// SPDX-License-Identifier: GPL-3.0-or-later
// Reads the binary `.moc3` of a Live2D model (the file VTube Studio and games load) and opens it like a `.cmo3`: the file is read
// into flat arrays, written out as the same kind of XML tree a `.cmo3` has, and the `.cmo3` importer does the rest (meshes cut out of
// the textures, deformers as changers, sliders with their shapes).
//
// The layout (little endian, versions 1 to 5, checked on real models): `MOC3`, the version, a flag for big endian; at 0x40 a table of
// 160 section offsets. Section 0 is a table of counts, section 1 the canvas, and then one flat array per field of every kind of
// object (parts, deformers, warp and rotation deformers, art meshes, sliders and their shapes), each starting on a 64 byte border.
// Positions of a shape are in the space of its parent: canvas units around the origin at the top (a unit is `pixels per unit` pixels),
// 0 to 1 inside a warp, pixels inside a rotation. Like the `.cmo3` tree they run downward.
//
// Not read: the colour blending of versions 4 and 5 (multiply and screen colours, blend modes), the glue, the draw order groups
// and the masks (clipping is not supported by the importer either).

use super::cmo3::{Imported, import_dom};
use super::xml::Dom;

const MAX_COUNT: u32 = 8_000_000;

/// The numbers of one model, as they are in the file.
#[derive(Default, Debug)]
pub struct Moc3 {
    pub version: u8,
    /// Pixels per unit, origin and size of the canvas.
    pub ppu: f32,
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub part_ids: Vec<String>,
    pub part_visible: Vec<i32>,
    pub part_parent: Vec<i32>,
    pub def_ids: Vec<String>,
    pub def_kfbind: Vec<i32>,
    pub def_parent: Vec<i32>,
    /// 0 warp, 1 rotation.
    pub def_type: Vec<i32>,
    /// The number in the list of warps or of rotations.
    pub def_spec: Vec<i32>,
    pub warp_kfbind: Vec<i32>,
    pub warp_kfbegin: Vec<i32>,
    pub warp_kfcount: Vec<i32>,
    pub warp_rows: Vec<i32>,
    pub warp_cols: Vec<i32>,
    pub warp_vcount: Vec<i32>,
    pub rot_kfbind: Vec<i32>,
    pub rot_kfbegin: Vec<i32>,
    pub rot_kfcount: Vec<i32>,
    pub rot_base: Vec<f32>,
    pub mesh_ids: Vec<String>,
    pub mesh_kfbind: Vec<i32>,
    pub mesh_kfbegin: Vec<i32>,
    pub mesh_kfcount: Vec<i32>,
    pub mesh_visible: Vec<i32>,
    pub mesh_part: Vec<i32>,
    pub mesh_parent: Vec<i32>,
    pub mesh_texture: Vec<i32>,
    pub mesh_vcount: Vec<i32>,
    pub mesh_uvbegin: Vec<i32>,
    pub mesh_idxbegin: Vec<i32>,
    pub mesh_idxcount: Vec<i32>,
    pub mesh_maskcount: Vec<i32>,
    pub param_ids: Vec<String>,
    pub param_max: Vec<f32>,
    pub param_min: Vec<f32>,
    pub param_default: Vec<f32>,
    pub param_bind_begin: Vec<i32>,
    pub param_bind_count: Vec<i32>,
    pub warp_kf_pos: Vec<i32>,
    pub rot_kf_angle: Vec<f32>,
    pub rot_kf_ox: Vec<f32>,
    pub rot_kf_oy: Vec<f32>,
    pub rot_kf_scale: Vec<f32>,
    pub mesh_kf_opacity: Vec<f32>,
    pub mesh_kf_order: Vec<f32>,
    pub mesh_kf_pos: Vec<i32>,
    pub positions: Vec<f32>,
    /// Keyform binding: where its list of slider bindings starts and how long it is.
    pub kb_begin: Vec<i32>,
    pub kb_count: Vec<i32>,
    /// The slider bindings a keyform binding lists, and for each slider binding where its keys start and how many there are.
    pub binding_list: Vec<i32>,
    pub pb_key_begin: Vec<i32>,
    pub pb_key_count: Vec<i32>,
    pub keys: Vec<f32>,
    pub uvs: Vec<f32>,
    pub indices: Vec<u16>,
}

struct Reader<'a> {
    d: &'a [u8],
    offs: Vec<u32>,
}

impl Reader<'_> {
    fn span(&self, section: usize, n: usize, size: usize) -> Result<&[u8], String> {
        let o = *self.offs.get(section).ok_or("The file has no such section.")? as usize;
        let len = n.checked_mul(size).ok_or("A list in the file is too long.")?;
        let end = o.checked_add(len).ok_or("A list in the file is too long.")?;
        if o == 0 || end > self.d.len() {
            return Err(format!("The model file is cut short (section {section})."));
        }
        Ok(&self.d[o..end])
    }

    fn ints(&self, section: usize, n: usize) -> Result<Vec<i32>, String> {
        Ok(self.span(section, n, 4)?.chunks_exact(4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
    }

    fn floats(&self, section: usize, n: usize) -> Result<Vec<f32>, String> {
        Ok(self.span(section, n, 4)?.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
    }

    fn shorts(&self, section: usize, n: usize) -> Result<Vec<u16>, String> {
        Ok(self.span(section, n, 2)?.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect())
    }

    fn bytes(&self, section: usize, n: usize) -> Result<Vec<u8>, String> {
        Ok(self.span(section, n, 1)?.to_vec())
    }

    fn ids(&self, section: usize, n: usize) -> Result<Vec<String>, String> {
        Ok(self.span(section, n, 64)?.chunks_exact(64).map(|b| String::from_utf8_lossy(b.split(|c| *c == 0).next().unwrap_or(&[])).chars().filter(|c| !c.is_control()).collect()).collect())
    }
}

pub fn parse(d: &[u8]) -> Result<Moc3, String> {
    if d.len() < 0x40 + 160 * 4 || &d[..4] != b"MOC3" {
        return Err("This is not a Live2D .moc3 file (it does not start with MOC3).".into());
    }
    if d[5] != 0 {
        return Err("This .moc3 file is big endian, which is not supported.".into());
    }
    let version = d[4];
    if version == 0 || version > 5 {
        return Err(format!("This .moc3 file has version {version}; versions 1 to 5 are supported."));
    }
    let u = |p: usize| u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]);
    let offs: Vec<u32> = (0..160).map(|i| u(0x40 + i * 4)).collect();
    let r = Reader { d, offs };
    let c: Vec<u32> = (0..23).map(|i| r.offs[0] as usize + i * 4).map(|p| if p + 4 <= d.len() { u(p) } else { 0 }).collect();
    if c.iter().any(|n| *n > MAX_COUNT) {
        return Err("The model file has absurd counts.".into());
    }
    let n = |i: usize| c[i] as usize;
    let (parts, defs, warps, rots, meshes, params) = (n(0), n(1), n(2), n(3), n(4), n(5));
    let (pkf, wkf, rkf, mkf, npos, npbi, nkb, npb, nkeys, nuv, nidx) = (n(6), n(7), n(8), n(9), n(10), n(11), n(12), n(13), n(14), n(15), n(16));
    let _ = pkf;
    let cv = r.floats(1, 6)?;
    let mut m = Moc3 { version, ppu: cv[0], origin: [cv[1], cv[2]], size: [cv[3], cv[4]], ..Default::default() };
    if !(m.ppu.is_finite() && m.ppu > 0.0 && m.size[0] >= 1.0 && m.size[1] >= 1.0) {
        return Err("The canvas of the model has a strange size.".into());
    }
    m.part_ids = r.ids(3, parts)?;
    m.part_visible = r.ints(7, parts)?;
    m.part_parent = r.ints(9, parts)?;
    m.def_ids = r.ids(11, defs)?;
    m.def_kfbind = r.ints(12, defs)?;
    m.def_parent = r.ints(16, defs)?;
    m.def_type = r.ints(17, defs)?;
    m.def_spec = r.ints(18, defs)?;
    m.warp_kfbind = r.ints(19, warps)?;
    m.warp_kfbegin = r.ints(20, warps)?;
    m.warp_kfcount = r.ints(21, warps)?;
    m.warp_vcount = r.ints(22, warps)?;
    m.warp_rows = r.ints(23, warps)?;
    m.warp_cols = r.ints(24, warps)?;
    m.rot_kfbind = r.ints(25, rots)?;
    m.rot_kfbegin = r.ints(26, rots)?;
    m.rot_kfcount = r.ints(27, rots)?;
    m.rot_base = r.floats(28, rots)?;
    m.mesh_ids = r.ids(33, meshes)?;
    m.mesh_kfbind = r.ints(34, meshes)?;
    m.mesh_kfbegin = r.ints(35, meshes)?;
    m.mesh_kfcount = r.ints(36, meshes)?;
    m.mesh_visible = r.ints(37, meshes)?;
    m.mesh_part = r.ints(39, meshes)?;
    m.mesh_parent = r.ints(40, meshes)?;
    m.mesh_texture = r.ints(41, meshes)?;
    let _flags = r.bytes(42, meshes)?;
    m.mesh_vcount = r.ints(43, meshes)?;
    m.mesh_uvbegin = r.ints(44, meshes)?;
    m.mesh_idxbegin = r.ints(45, meshes)?;
    m.mesh_idxcount = r.ints(46, meshes)?;
    m.mesh_maskcount = r.ints(48, meshes)?;
    m.param_ids = r.ids(50, params)?;
    m.param_max = r.floats(51, params)?;
    m.param_min = r.floats(52, params)?;
    m.param_default = r.floats(53, params)?;
    m.param_bind_begin = r.ints(56, params)?;
    m.param_bind_count = r.ints(57, params)?;
    m.warp_kf_pos = r.ints(60, wkf)?;
    m.rot_kf_angle = r.floats(62, rkf)?;
    m.rot_kf_ox = r.floats(63, rkf)?;
    m.rot_kf_oy = r.floats(64, rkf)?;
    m.rot_kf_scale = r.floats(65, rkf)?;
    m.mesh_kf_opacity = r.floats(68, mkf)?;
    m.mesh_kf_order = r.floats(69, mkf)?;
    m.mesh_kf_pos = r.ints(70, mkf)?;
    m.positions = r.floats(71, npos)?;
    m.binding_list = r.ints(72, npbi)?;
    m.kb_begin = r.ints(73, nkb)?;
    m.kb_count = r.ints(74, nkb)?;
    m.pb_key_begin = r.ints(75, npb)?;
    m.pb_key_count = r.ints(76, npb)?;
    m.keys = r.floats(77, nkeys)?;
    m.uvs = r.floats(78, nuv)?;
    m.indices = r.shorts(79, nidx)?;
    Ok(m)
}

/// Where the shapes of an object are measured from.
#[derive(Clone, Copy, PartialEq)]
enum Parent {
    Root,
    Warp,
    Rot,
}

/// One slider of a keyform grid and its key points.
struct Axis {
    param: usize,
    keys: Vec<f32>,
}

struct Writer {
    s: String,
    ids: usize,
}

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

impl Writer {
    fn open(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.s.push('<');
        self.s.push_str(tag);
        for (k, v) in attrs {
            self.s.push_str(&format!(" {k}=\"{}\"", esc(v)));
        }
        self.s.push('>');
    }

    fn close(&mut self, tag: &str) {
        self.s.push_str(&format!("</{tag}>"));
    }

    fn empty(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.open(tag, attrs);
        self.s.pop();
        self.s.push_str("/>");
    }

    fn leaf(&mut self, tag: &str, name: &str, text: &str) {
        self.open(tag, &[("xs.n", name)]);
        self.s.push_str(&esc(text));
        self.close(tag);
    }

    fn numbers(&mut self, name: &str, v: &[f32]) {
        self.open("float-array", &[("xs.n", name)]);
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                self.s.push(' ');
            }
            self.s.push_str(&format!("{x}"));
        }
        self.close("float-array");
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.ids += 1;
        format!("#{prefix}{}", self.ids)
    }
}

impl Moc3 {
    fn get(v: &[i32], i: usize) -> i32 {
        v.get(i).copied().unwrap_or(0)
    }

    /// The sliders and key points a keyform binding is made of.
    fn axes_of(&self, kb: i32) -> Option<Vec<Axis>> {
        let kb = usize::try_from(kb).ok()?;
        let (begin, count) = (Self::get(&self.kb_begin, kb), Self::get(&self.kb_count, kb));
        let mut out = Vec::new();
        for j in begin..begin + count {
            let pbx = *self.binding_list.get(usize::try_from(j).ok()?)? as usize;
            let param = (0..self.param_ids.len()).find(|p| {
                let (b, c) = (self.param_bind_begin[*p], self.param_bind_count[*p]);
                (b..b + c).contains(&(pbx as i32))
            })?;
            let (kb0, kc) = (usize::try_from(*self.pb_key_begin.get(pbx)?).ok()?, usize::try_from(*self.pb_key_count.get(pbx)?).ok()?);
            let keys = self.keys.get(kb0..kb0.checked_add(kc)?)?.to_vec();
            out.push(Axis { param, keys });
        }
        Some(out)
    }

    /// Is the space a deformer's shapes are measured in made of canvas units (the chain of its parents has no rotation, which would
    /// bring in pixels)? Then a rotation inside it has a size that turns its pixels into those units.
    fn in_units(&self, mut def: i32) -> bool {
        for _ in 0..self.def_ids.len() + 1 {
            let Some(d) = usize::try_from(def).ok().filter(|d| *d < self.def_ids.len()) else { return true };
            if Self::get(&self.def_type, d) == 1 {
                return false;
            }
            def = Self::get(&self.def_parent, d);
        }
        false
    }

    fn parent_of(&self, def: i32) -> Parent {
        match usize::try_from(def).ok().and_then(|d| self.def_type.get(d)) {
            Some(0) => Parent::Warp,
            Some(1) => Parent::Rot,
            _ => Parent::Root,
        }
    }

    /// A pair of the file's numbers as the `.cmo3` tree has it (mirrored upward to downward).
    fn pt(&self, parent: Parent, x: f32, y: f32) -> [f32; 2] {
        match parent {
            Parent::Root => [self.origin[0] + x * self.ppu, self.origin[1] + y * self.ppu],
            Parent::Warp | Parent::Rot => [x, y],
        }
    }

    fn flat(&self, parent: Parent, begin: usize, vertices: usize) -> Option<Vec<f32>> {
        let src = self.positions.get(begin..begin.checked_add(vertices * 2)?)?;
        Some(src.chunks_exact(2).flat_map(|p| self.pt(parent, p[0], p[1])).collect())
    }

    /// The keyform grid of an object, written the way a `.cmo3` does. Returns how many keyforms there are, or None when the numbers do
    /// not agree. `form` writes the keyform number `k` (with its guid already placed by this function through `guid_slot`).
    fn grid(&self, w: &mut Writer, axes: &[Axis], count: usize, param_guid: &[String], form_ids: &[String]) -> Option<()> {
        let total: usize = axes.iter().map(|a| a.keys.len().max(1)).product();
        if total != count || form_ids.len() != count {
            return None;
        }
        let gid = w.fresh("kg");
        w.open("CKeyformGridSource", &[("xs.n", "keyformGridSource"), ("xs.id", &gid)]);
        w.open("list", &[("xs.n", "keyformBindings")]);
        let mut bids = Vec::new();
        for a in axes {
            let bid = w.fresh("kb");
            w.open("KeyformBinding", &[("xs.id", &bid)]);
            w.empty("guid", &[("xs.n", "parameterGuid"), ("xs.ref", &param_guid[a.param])]);
            w.open("list", &[("xs.n", "keys")]);
            for k in &a.keys {
                w.open("f", &[]);
                w.s.push_str(&format!("{k}"));
                w.close("f");
            }
            w.close("list");
            w.close("KeyformBinding");
            bids.push(bid);
        }
        w.close("list");
        w.open("list", &[("xs.n", "keyformsOnGrid")]);
        for (l, fid) in form_ids.iter().enumerate() {
            w.open("KeyformOnGrid", &[]);
            w.open("AccessKey", &[("xs.n", "accessKey")]);
            w.open("list", &[("xs.n", "_keyOnParameterList")]);
            let mut rest = l;
            for (ai, a) in axes.iter().enumerate() {
                let n = a.keys.len().max(1);
                w.open("KeyOnParameter", &[]);
                w.empty("b", &[("xs.n", "binding"), ("xs.ref", &bids[ai])]);
                w.leaf("i", "keyIndex", &(rest % n).to_string());
                w.close("KeyOnParameter");
                rest /= n;
            }
            w.close("list");
            w.close("AccessKey");
            w.empty("guid", &[("xs.n", "keyformGuid"), ("xs.ref", fid)]);
            w.close("KeyformOnGrid");
        }
        w.close("list");
        w.close("CKeyformGridSource");
        Some(())
    }

    /// The model as the XML tree of a `.cmo3`. Returns it with a note for everything that was left out.
    pub fn to_xml(&self, name: &str, textures: usize) -> (String, Vec<String>) {
        let mut notes = Vec::new();
        let mut w = Writer { s: String::new(), ids: 0 };
        w.open("CModelSource", &[("xs.id", "#model")]);
        w.leaf("s", "name", name);
        w.open("CanvasInfo", &[("xs.n", "canvas")]);
        w.leaf("i", "pixelWidth", &(self.size[0].round() as i64).to_string());
        w.leaf("i", "pixelHeight", &(self.size[1].round() as i64).to_string());
        w.close("CanvasInfo");
        // textures: the numbers of the UV map are in the picture upward, the tree wants them downward
        for t in 0..textures {
            w.open("CTextureInput", &[("xs.id", &format!("#tex{t}"))]);
            w.open("CImageResource", &[("xs.n", "srcImageResource")]);
            w.empty("CImageFileBuf", &[("xs.n", "imageFileBuf"), ("path", &format!("tex{t}"))]);
            w.close("CImageResource");
            w.empty("CAffine", &[("xs.n", "transformImageResource01toLogical01"), ("m00", "1"), ("m01", "0"), ("m02", "0"), ("m10", "0"), ("m11", "1"), ("m12", "0")]);
            w.close("CTextureInput");
        }
        // sliders
        let mut param_guid = Vec::new();
        for (i, id) in self.param_ids.iter().enumerate() {
            let g = format!("#gparam{i}");
            let (lo, hi) = (self.param_min[i], self.param_max[i]);
            w.open("CParameterSource", &[("xs.id", &format!("#param{i}"))]);
            w.empty("CParameterGuid", &[("xs.n", "guid"), ("xs.id", &g)]);
            w.leaf("f", "minValue", &lo.to_string());
            w.leaf("f", "maxValue", &hi.to_string());
            w.leaf("f", "defaultValue", &self.param_default[i].to_string());
            w.leaf("s", "name", id);
            w.empty("CParameterId", &[("xs.n", "id"), ("idstr", id)]);
            w.close("CParameterSource");
            param_guid.push(g);
        }
        // deformers
        let guid_of_def = |i: usize| format!("#gdef{i}");
        for (di, dname) in self.def_ids.iter().enumerate() {
            let (ty, spec) = (Self::get(&self.def_type, di), Self::get(&self.def_spec, di).max(0) as usize);
            let parent_def = Self::get(&self.def_parent, di);
            let parent = self.parent_of(parent_def);
            let (kfbind, kfbegin, kfcount) = if ty == 0 {
                (Self::get(&self.warp_kfbind, spec), Self::get(&self.warp_kfbegin, spec), Self::get(&self.warp_kfcount, spec))
            } else {
                (Self::get(&self.rot_kfbind, spec), Self::get(&self.rot_kfbegin, spec), Self::get(&self.rot_kfcount, spec))
            };
            let Some(axes) = self.axes_of(kfbind) else {
                notes.push(format!("{dname} has a damaged list of sliders and was left out."));
                continue;
            };
            let count = kfcount.max(0) as usize;
            let tag = if ty == 0 { "CWarpDeformerSource" } else { "CRotationDeformerSource" };
            let mut form_ids: Vec<String> = Vec::new();
            let mut forms = Writer { s: String::new(), ids: 0 };
            let (rows, cols) = (Self::get(&self.warp_rows, spec).max(1) as usize, Self::get(&self.warp_cols, spec).max(1) as usize);
            let mut ok = true;
            forms.open("list", &[("xs.n", "keyforms")]);
            for k in 0..count {
                let kf = kfbegin.max(0) as usize + k;
                let fid = forms.fresh(&format!("fd{di}x"));
                let gid = format!("{fid}g");
                if ty == 0 {
                    let verts = (rows + 1) * (cols + 1);
                    let pb = Self::get(&self.warp_kf_pos, kf).max(0) as usize;
                    let Some(pts) = self.flat(parent, pb, verts) else {
                        ok = false;
                        break;
                    };
                    forms.open("CWarpDeformerForm", &[("xs.id", &fid)]);
                    forms.open("ACForm", &[("xs.n", "super")]);
                    forms.open("ACForm2", &[("xs.n", "super")]);
                    forms.empty("guid", &[("xs.n", "guid"), ("xs.id", &gid)]);
                    forms.close("ACForm2");
                    forms.close("ACForm");
                    forms.numbers("positions", &pts);
                    forms.close("CWarpDeformerForm");
                } else {
                    let g = |v: &[f32]| v.get(kf).copied().unwrap_or(0.0);
                    let o = self.pt(parent, g(&self.rot_kf_ox), g(&self.rot_kf_oy));
                    let scale = g(&self.rot_kf_scale) * if self.in_units(parent_def) { self.ppu } else { 1.0 };
                    forms.open("CRotationDeformerForm", &[("xs.id", &fid), ("angle", &g(&self.rot_kf_angle).to_string()), ("originX", &o[0].to_string()), ("originY", &o[1].to_string()), ("scale", &scale.to_string())]);
                    forms.open("ACForm", &[("xs.n", "super")]);
                    forms.open("ACForm2", &[("xs.n", "super")]);
                    forms.empty("guid", &[("xs.n", "guid"), ("xs.id", &gid)]);
                    forms.close("ACForm2");
                    forms.close("ACForm");
                    forms.close("CRotationDeformerForm");
                }
                form_ids.push(gid);
            }
            forms.close("list");
            if !ok {
                notes.push(format!("{dname} has shapes outside the file and was left out."));
                continue;
            }
            w.open(tag, &[("xs.id", &format!("#def{di}"))]);
            w.open("CDeformerSource", &[("xs.n", "super")]);
            w.empty("guid", &[("xs.n", "guid"), ("xs.id", &guid_of_def(di))]);
            if let Some(p) = usize::try_from(parent_def).ok().filter(|p| *p < self.def_ids.len()) {
                w.empty("guid", &[("xs.n", "targetDeformerGuid"), ("xs.ref", &guid_of_def(p))]);
            }
            w.open("ACDeformerSource", &[("xs.n", "super")]);
            w.leaf("s", "localName", dname);
            if self.grid(&mut w, &axes, count, &param_guid, &form_ids).is_none() {
                notes.push(format!("{dname} has shapes that do not match its sliders."));
            }
            w.close("ACDeformerSource");
            w.close("CDeformerSource");
            if ty == 0 {
                w.leaf("i", "col", &cols.to_string());
                w.leaf("i", "row", &rows.to_string());
            } else {
                w.leaf("f", "baseAngle", &(self.rot_base.get(spec).copied().unwrap_or(0.0)).to_string());
            }
            w.s.push_str(&forms.s);
            w.close(tag);
        }
        // parts
        for (pi, pname) in self.part_ids.iter().enumerate() {
            w.open("CPartSource", &[("xs.id", &format!("#part{pi}"))]);
            w.empty("guid", &[("xs.n", "guid"), ("xs.id", &format!("#gpart{pi}"))]);
            w.open("CPartControl", &[("xs.n", "super")]);
            if let Some(p) = usize::try_from(Self::get(&self.part_parent, pi)).ok().filter(|p| *p < self.part_ids.len()) {
                w.empty("guid", &[("xs.n", "parentGuid"), ("xs.ref", &format!("#gpart{p}"))]);
            }
            w.leaf("s", "isVisible", if Self::get(&self.part_visible, pi) != 0 { "true" } else { "false" });
            w.leaf("s", "localName", pname);
            w.close("CPartControl");
            w.leaf("s", "isSketch", "false");
            w.open("list", &[("xs.n", "_childGuids")]);
            for q in 0..self.part_ids.len() {
                if Self::get(&self.part_parent, q) == pi as i32 {
                    w.empty("guid", &[("xs.ref", &format!("#gpart{q}"))]);
                }
            }
            for mi in 0..self.mesh_ids.len() {
                if Self::get(&self.mesh_part, mi) == pi as i32 {
                    w.empty("guid", &[("xs.ref", &format!("#gmesh{mi}"))]);
                }
            }
            w.close("list");
            w.close("CPartSource");
        }
        // art meshes
        let mut masked = 0usize;
        for (mi, mname) in self.mesh_ids.iter().enumerate() {
            let parent_def = Self::get(&self.mesh_parent, mi);
            let parent = self.parent_of(parent_def);
            let nv = Self::get(&self.mesh_vcount, mi).max(0) as usize;
            let (ub, ib, ic) = (Self::get(&self.mesh_uvbegin, mi).max(0) as usize, Self::get(&self.mesh_idxbegin, mi).max(0) as usize, Self::get(&self.mesh_idxcount, mi).max(0) as usize);
            let (Some(uv), Some(idx)) = (self.uvs.get(ub..ub + nv * 2), self.indices.get(ib..ib + ic)) else {
                notes.push(format!("{mname} has a damaged mesh and was left out."));
                continue;
            };
            let Some(axes) = self.axes_of(Self::get(&self.mesh_kfbind, mi)) else {
                notes.push(format!("{mname} has a damaged list of sliders and was left out."));
                continue;
            };
            let count = Self::get(&self.mesh_kfcount, mi).max(0) as usize;
            let kfbegin = Self::get(&self.mesh_kfbegin, mi).max(0) as usize;
            let mut forms = Writer { s: String::new(), ids: 0 };
            let mut form_ids = Vec::new();
            let mut ok = true;
            forms.open("list", &[("xs.n", "keyforms")]);
            for k in 0..count {
                let kf = kfbegin + k;
                let fid = forms.fresh(&format!("fm{mi}x"));
                let gid = format!("{fid}g");
                let Some(pts) = self.flat(parent, Self::get(&self.mesh_kf_pos, kf).max(0) as usize, nv) else {
                    ok = false;
                    break;
                };
                forms.open("CArtMeshForm", &[("xs.id", &fid)]);
                forms.open("ACDrawableForm", &[("xs.n", "super")]);
                forms.leaf("i", "drawOrder", &(self.mesh_kf_order.get(kf).copied().unwrap_or(500.0).round() as i64).to_string());
                forms.leaf("f", "opacity", &self.mesh_kf_opacity.get(kf).copied().unwrap_or(1.0).to_string());
                forms.open("ACForm2", &[("xs.n", "super")]);
                forms.empty("guid", &[("xs.n", "guid"), ("xs.id", &gid)]);
                forms.close("ACForm2");
                forms.close("ACDrawableForm");
                forms.numbers("positions", &pts);
                forms.close("CArtMeshForm");
                form_ids.push(gid);
            }
            forms.close("list");
            if !ok {
                notes.push(format!("{mname} has shapes outside the file and was left out."));
                continue;
            }
            w.open("CArtMeshSource", &[("xs.id", &format!("#mesh{mi}"))]);
            w.open("CDrawableSource", &[("xs.n", "super")]);
            w.empty("guid", &[("xs.n", "guid"), ("xs.id", &format!("#gmesh{mi}"))]);
            if let Some(p) = usize::try_from(parent_def).ok().filter(|p| *p < self.def_ids.len()) {
                w.empty("guid", &[("xs.n", "targetDeformerGuid"), ("xs.ref", &guid_of_def(p))]);
            }
            if Self::get(&self.mesh_maskcount, mi) > 0 {
                masked += 1;
                w.open("list", &[("xs.n", "clipGuidList")]);
                w.empty("guid", &[("xs.ref", &format!("#gmesh{mi}"))]);
                w.close("list");
            }
            w.open("ACDrawableSource", &[("xs.n", "super")]);
            w.leaf("s", "localName", mname);
            w.leaf("s", "isVisible", if Self::get(&self.mesh_visible, mi) != 0 { "true" } else { "false" });
            if self.grid(&mut w, &axes, count, &param_guid, &form_ids).is_none() {
                notes.push(format!("{mname} has shapes that do not match its sliders."));
            }
            w.close("ACDrawableSource");
            w.close("CDrawableSource");
            w.open("int-array", &[("xs.n", "indices")]);
            w.s.push_str(&idx.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" "));
            w.close("int-array");
            w.numbers("uvs", uv);
            let t = Self::get(&self.mesh_texture, mi);
            if (0..textures as i32).contains(&t) {
                w.empty("CTextureInput", &[("xs.n", "texture"), ("xs.ref", &format!("#tex{t}"))]);
            }
            w.s.push_str(&forms.s);
            w.close("CArtMeshSource");
        }
        if masked > 0 {
            notes.push(format!("{masked} meshes are clipped by another mesh (the mouth inside, for example); the clipping is not applied yet."));
        }
        w.close("CModelSource");
        (w.s, notes)
    }
}

/// Open a `.moc3` with its texture pictures (the bytes of the files, in the order of the model's list).
pub fn import_bytes(data: &[u8], textures: Vec<Vec<u8>>, name: &str) -> Result<Imported, String> {
    let moc = parse(data)?;
    let (xml, notes) = moc.to_xml(name, textures.len());
    let dom = Dom::parse(&xml)?;
    drop(xml);
    let mut imported = import_dom(dom, &|path| path.strip_prefix("tex").and_then(|n| n.parse::<usize>().ok()).and_then(|n| textures.get(n).cloned()))?;
    imported.report.notes.splice(0..0, notes);
    Ok(imported)
}

/// Open a `.moc3` from a folder as VTube Studio and games keep them: the `.model3.json` next to it (or the one given instead) names the
/// textures; its physics, expressions, motions and VTube Studio settings go onto the rig as well.
pub fn import_file(path: &std::path::Path) -> Result<Imported, String> {
    use super::runtime;
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let is_json = path.to_string_lossy().to_lowercase().ends_with(".json");
    let model3 = if is_json {
        Some(path.to_path_buf())
    } else {
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().to_lowercase().ends_with(".model3.json")).collect();
        found.sort_by_key(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&stem)));
        found.into_iter().next()
    };
    let files = match &model3 {
        Some(m) => Some(runtime::load_folder(m)?),
        None => None,
    };
    let moc_path = match &files {
        Some(f) if is_json => dir.join(f.model.file_references.moc.replace('\\', "/")),
        _ => path.to_path_buf(),
    };
    let len = std::fs::metadata(&moc_path).map_err(|e| format!("Cannot read the model file: {e}"))?.len();
    if len > 500_000_000 {
        return Err("This model file is too big to open.".into());
    }
    let moc = std::fs::read(&moc_path).map_err(|e| format!("Cannot read the model file: {e}"))?;
    let mut textures: Vec<Vec<u8>> = Vec::new();
    if let Some(f) = &files {
        for t in &f.textures {
            textures.push(std::fs::read(t).map_err(|e| format!("Cannot read the texture {}: {e}", t.display()))?);
        }
    }
    if textures.is_empty() {
        return Err("The textures of the model were not found. Open the .model3.json that comes with it, or keep the folder as it was.".into());
    }
    let name = moc_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Live2D model".into());
    let mut imp = import_bytes(&moc, textures, &name)?;
    if let Some(f) = files {
        let report = runtime::apply(std::sync::Arc::make_mut(&mut imp.doc.rig), &f);
        imp.report.notes.push(format!("From the model files: {} swinging settings, {} expressions, {} motions.", report.physics_settings, report.expressions, report.motions));
        imp.report.notes.extend(f.notes.iter().cloned());
        imp.report.notes.extend(report.notes.iter().take(3).cloned());
    }
    imp.name = name;
    Ok(imp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::Values;

    /// A `.moc3` written by hand: one slider (Angle, -30 to 30), a rotation at the middle of the canvas that follows it, and a square
    /// mesh inside the rotation.
    pub(crate) fn tiny_moc3() -> Vec<u8> {
        // (section, count, item size) of everything the reader takes
        let (parts, defs, warps, rots, meshes, params, pkf, wkf, rkf, mkf, npos, npbi, nkb, npb, nkeys, nuv, nidx) = (1usize, 1, 0, 1, 1, 1, 1, 0, 3, 1, 8, 1, 2, 1, 3, 8, 6);
        let mut spec: Vec<(usize, usize, usize)> = vec![(2, parts, 8), (3, parts, 64)];
        spec.extend((4..=9).map(|s| (s, parts, 4)));
        spec.extend([(10, defs, 8), (11, defs, 64)]);
        spec.extend((12..=18).map(|s| (s, defs, 4)));
        spec.extend((19..=24).map(|s| (s, warps, 4)));
        spec.extend((25..=28).map(|s| (s, rots, 4)));
        spec.extend((29..=32).map(|s| (s, meshes, 8)));
        spec.push((33, meshes, 64));
        spec.extend((34..=48).map(|s| (s, meshes, if s == 42 { 1 } else { 4 })));
        spec.extend([(49, params, 8), (50, params, 64)]);
        spec.extend((51..=57).map(|s| (s, params, 4)));
        spec.push((58, pkf, 4));
        spec.extend([(59, wkf, 4), (60, wkf, 4)]);
        spec.extend((61..=67).map(|s| (s, rkf, 4)));
        spec.extend((68..=70).map(|s| (s, mkf, 4)));
        spec.extend([(71, npos, 4), (72, npbi, 4), (73, nkb, 4), (74, nkb, 4), (75, npb, 4), (76, npb, 4), (77, nkeys, 4), (78, nuv, 4), (79, nidx, 2), (80, 1, 4)]);
        let mut sections: std::collections::BTreeMap<usize, Vec<u8>> = std::collections::BTreeMap::new();
        for (s, n, size) in &spec {
            sections.insert(*s, vec![0u8; (n * size).max(1)]);
        }
        let counts = [parts, defs, warps, rots, meshes, params, pkf, wkf, rkf, mkf, npos, npbi, nkb, npb, nkeys, nuv, nidx, 1, 1, 1, 0, 0, 0, 0];
        sections.insert(0, counts.iter().flat_map(|c| (*c as u32).to_le_bytes()).collect());
        let f = |v: &[f32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let i = |v: &[i32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let id = |s: &str| -> Vec<u8> {
            let mut b = s.as_bytes().to_vec();
            b.resize(64, 0);
            b
        };
        sections.insert(1, f(&[100.0, 200.0, 200.0, 400.0, 400.0, 0.0]));
        sections.insert(3, id("Part"));
        sections.insert(7, i(&[1]));
        sections.insert(9, i(&[-1]));
        sections.insert(11, id("Rotation"));
        sections.insert(16, i(&[-1]));
        sections.insert(17, i(&[1]));
        sections.insert(18, i(&[0]));
        // the rotation: the first keyform binding, three keyforms from 0
        sections.insert(27, i(&[3]));
        sections.insert(33, id("Square"));
        sections.insert(34, i(&[1])); // the mesh's keyform binding: the second, which has no slider
        sections.insert(36, i(&[1]));
        sections.insert(37, i(&[1]));
        sections.insert(40, i(&[0]));
        sections.insert(41, i(&[0]));
        let mut flags = vec![0u8; 1];
        flags[0] = 0;
        sections.insert(42, flags);
        sections.insert(43, i(&[4]));
        sections.insert(46, i(&[6]));
        sections.insert(50, id("ParamAngle"));
        sections.insert(51, f(&[30.0]));
        sections.insert(52, f(&[-30.0]));
        sections.insert(53, f(&[0.0]));
        sections.insert(57, i(&[1]));
        sections.insert(62, f(&[-30.0, 0.0, 30.0]));
        sections.insert(65, f(&[0.01, 0.01, 0.01]));
        sections.insert(68, f(&[1.0]));
        sections.insert(69, f(&[500.0]));
        sections.insert(71, f(&[-50.0, -50.0, 50.0, -50.0, 50.0, 50.0, -50.0, 50.0]));
        sections.insert(73, i(&[0, 1]));
        sections.insert(74, i(&[1, 0]));
        sections.insert(76, i(&[3]));
        sections.insert(77, f(&[-30.0, 0.0, 30.0]));
        sections.insert(78, f(&[0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0]));
        sections.insert(79, [0u16, 1, 2, 0, 2, 3].iter().flat_map(|x| x.to_le_bytes()).collect());
        // lay it out: header, the table of offsets, then every section on a 64 byte border
        let mut out = vec![0u8; 0x40 + 160 * 4];
        out[..4].copy_from_slice(b"MOC3");
        out[4] = 3;
        let mut offs = [0u32; 160];
        for (s, bytes) in &sections {
            while out.len() % 64 != 0 {
                out.push(0);
            }
            offs[*s] = out.len() as u32;
            out.extend(bytes);
        }
        while out.len() % 64 != 0 {
            out.push(0);
        }
        for (k, o) in offs.iter().enumerate() {
            out[0x40 + k * 4..0x40 + k * 4 + 4].copy_from_slice(&o.to_le_bytes());
        }
        out
    }

    #[test]
    fn a_small_moc3_is_read_and_becomes_a_rig_that_turns() {
        let data = tiny_moc3();
        let m = parse(&data).unwrap();
        assert_eq!((m.version, m.ppu, m.origin, m.size), (3, 100.0, [200.0, 200.0], [400.0, 400.0]));
        assert_eq!((m.param_ids.clone(), m.def_ids.clone(), m.mesh_ids.clone()), (vec!["ParamAngle".to_string()], vec!["Rotation".to_string()], vec!["Square".to_string()]));
        assert_eq!(m.rot_kf_angle, [-30.0, 0.0, 30.0]);
        let imp = import_bytes(&data, vec![crate::import::tests::texture_png()], "Tiny").unwrap();
        assert_eq!((imp.doc.w, imp.doc.h), (400, 400));
        assert_eq!((imp.report.params, imp.report.rotations, imp.report.meshes), (1, 1, 1), "{:?}", imp.report.notes);
        let rig = &*imp.doc.rig;
        assert_eq!(rig.params[0].cid, "ParamAngle");
        // the rotation sits at the origin of the canvas and turns with the slider
        let rot = rig.changers.iter().find(|c| c.name == "Rotation").unwrap();
        assert!(matches!(rot.kind, crate::rig::Kind::Rotation { pivot } if pivot == [200.0, 200.0]));
        let mut v = Values::default();
        v.set(rig.params[0].id, 30.0);
        assert_eq!(rig.pose(&v).rot[&rot.id], 30.0);
        // the mesh is the square around the pivot, as the units of the rotation say (0.01 of a unit of 100 pixels is one pixel)
        let mesh = &rig.meshes[0];
        assert_eq!(mesh.verts[0], [150.0, 150.0]);
        assert_eq!(mesh.verts[2], [250.0, 250.0]);
        assert!(!imp.doc.layers[0].tiles.is_empty(), "the picture was cut out of the texture");
    }

    #[test]
    fn a_model_folder_opens_from_the_moc3_or_from_the_model3_json() {
        let dir = std::env::temp_dir().join(format!("seviath_moc3_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a.2048")).unwrap();
        std::fs::write(dir.join("a.moc3"), tiny_moc3()).unwrap();
        std::fs::write(dir.join("a.2048").join("texture_00.png"), crate::import::tests::texture_png()).unwrap();
        // no .model3.json: the textures cannot be found
        assert!(import_file(&dir.join("a.moc3")).err().unwrap().contains("textures"));
        std::fs::write(dir.join("a.model3.json"), "{\"Version\":3,\"FileReferences\":{\"Moc\":\"a.moc3\",\"Textures\":[\"a.2048/texture_00.png\"]}}").unwrap();
        for p in ["a.moc3", "a.model3.json"] {
            let imp = import_file(&dir.join(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
            assert_eq!((imp.report.meshes, imp.name.as_str()), (1, "a"), "{p}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_that_are_not_moc3_are_refused_clearly() {
        assert!(parse(b"hello").is_err());
        let mut data = tiny_moc3();
        data[5] = 1;
        assert!(parse(&data).unwrap_err().contains("big endian"));
        let mut data = tiny_moc3();
        data[4] = 9;
        assert!(parse(&data).unwrap_err().contains("version 9"));
        // cut short: a section past the end
        let data = tiny_moc3();
        assert!(parse(&data[..data.len() / 2]).is_err());
    }
}
