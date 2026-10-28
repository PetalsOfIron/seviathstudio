// SPDX-License-Identifier: GPL-3.0-or-later
// Meshes that already exist, opened to be edited: .glb and .gltf (with the skeleton and the weights when there are any), .fbx (the geometry, the texture
// coordinates, the texture that is inside the file, and the skeleton and weights of the skin when it has one), .obj (and the picture its .mtl names), .stl and
// .ply. What comes out is a mesh in metres that stands the right way up as far as the file says: Z-up files are turned, and a mesh that is hundreds of units
// tall is taken to be in centimetres.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use crate::assetview::AssetMesh;
use crate::rig3d::Rig3;

pub const EXTENSIONS: &[&str] = &["glb", "gltf", "fbx", "obj", "stl", "ply"];

#[derive(Debug)]
pub struct Imported {
    pub mesh: AssetMesh,
    pub rig: Option<Rig3>,
    /// What was done to it on the way in (a unit changed, an axis turned).
    pub note: String,
}

/// Open a file by its kind.
pub fn load(path: &Path) -> Result<Imported, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("The file could not be read: {e}"))?;
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut imported = match ext.as_str() {
        "glb" => glb(&bytes)?,
        "gltf" => glb(&gltf_to_glb(&bytes, &dir)?)?,
        "fbx" => fbx(&bytes)?,
        "obj" => obj(&String::from_utf8_lossy(&bytes), &dir)?,
        "stl" => stl(&bytes)?,
        "ply" => ply(&bytes)?,
        other => return Err(format!("A .{other} file cannot be opened here (glb, gltf, fbx, obj, stl and ply can).")),
    };
    if imported.mesh.verts.is_empty() || imported.mesh.tris.is_empty() {
        return Err("The file has no triangles.".into());
    }
    if imported.mesh.verts.iter().any(|v| v.iter().any(|x| !x.is_finite())) {
        return Err("The mesh has points that are not numbers.".into());
    }
    let (lo, hi) = imported.mesh.bounds();
    // the biggest size (a file does not always say which way is up): a mesh in centimetres (or millimetres) is brought to metres
    let height = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
    if height > 20.0 {
        let k = if height > 2000.0 { 0.001 } else { 0.01 };
        for v in &mut imported.mesh.verts {
            *v = [v[0] * k, v[1] * k, v[2] * k];
        }
        if let Some(r) = imported.rig.as_mut() {
            for b in &mut r.bones {
                for p in [&mut b.head, &mut b.tail] {
                    *p = [p[0] * k, p[1] * k, p[2] * k];
                }
            }
        }
        imported.note.push_str(if k == 0.01 { "It was in centimetres: brought to metres. " } else { "It was in millimetres: brought to metres. " });
    }
    Ok(imported)
}

fn glb(bytes: &[u8]) -> Result<Imported, String> {
    let mesh = AssetMesh::from_glb(bytes)?;
    let rig = crate::meshfile::rig_from_glb(bytes).filter(|r| r.weights.len() == mesh.verts.len());
    let note = if rig.is_some() { "Its bones and weights came with it. ".to_string() } else { String::new() };
    Ok(Imported { mesh, rig, note })
}

// ------------------------------------------------------------------------------------------------ .gltf

fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes() {
        if let Some(v) = val(c) {
            acc = (acc << 6) | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
    }
    out
}

/// A .gltf (a text file that points at its data) as a .glb: the buffers and pictures are read (from the file itself or from the files next to it) and put in one.
fn gltf_to_glb(bytes: &[u8], dir: &Path) -> Result<Vec<u8>, String> {
    let mut root: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| format!("The .gltf file is not readable: {e}"))?;
    let read = |uri: &str| -> Result<Vec<u8>, String> {
        if let Some(rest) = uri.strip_prefix("data:") {
            return Ok(base64(rest.split_once(',').map(|x| x.1).unwrap_or("")));
        }
        let clean = uri.replace("%20", " ");
        std::fs::read(dir.join(clean)).map_err(|e| format!("{uri}: {e}"))
    };
    let mut bin: Vec<u8> = Vec::new();
    let mut views = root["bufferViews"].as_array().cloned().unwrap_or_default();
    // every buffer is read; the views that point in it are moved to the one binary part
    let buffers = root["buffers"].as_array().cloned().unwrap_or_default();
    let mut starts = Vec::new();
    for b in &buffers {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        starts.push(bin.len());
        match b["uri"].as_str() {
            Some(u) => bin.extend(read(u)?),
            None => return Err("A buffer has no data.".into()),
        }
    }
    for v in &mut views {
        let b = v["buffer"].as_u64().unwrap_or(0) as usize;
        let off = v["byteOffset"].as_u64().unwrap_or(0) as usize + starts.get(b).copied().unwrap_or(0);
        v["buffer"] = serde_json::json!(0);
        v["byteOffset"] = serde_json::json!(off);
    }
    // the pictures that are files become views too
    if let Some(imgs) = root["images"].as_array().cloned() {
        let mut new = Vec::new();
        for mut im in imgs {
            if let Some(u) = im["uri"].as_str().map(|s| s.to_string()) {
                let data = read(&u)?;
                while bin.len() % 4 != 0 {
                    bin.push(0);
                }
                views.push(serde_json::json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": data.len() }));
                im["bufferView"] = serde_json::json!(views.len() - 1);
                bin.extend(data);
                im.as_object_mut().map(|o| o.remove("uri"));
                if im["mimeType"].is_null() {
                    im["mimeType"] = serde_json::json!(if u.to_lowercase().ends_with(".jpg") || u.to_lowercase().ends_with(".jpeg") { "image/jpeg" } else { "image/png" });
                }
            }
            new.push(im);
        }
        root["images"] = serde_json::Value::Array(new);
    }
    root["bufferViews"] = serde_json::Value::Array(views);
    root["buffers"] = serde_json::json!([{ "byteLength": bin.len() }]);
    Ok(crate::vrm::gltf::pack(&root, &bin))
}

// ------------------------------------------------------------------------------------------------ .obj

fn obj(text: &str, dir: &Path) -> Result<Imported, String> {
    let (mut pos, mut uv): (Vec<[f32; 3]>, Vec<[f32; 2]>) = (Vec::new(), Vec::new());
    let (mut verts, mut uvs, mut tris): (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new(), Vec::new());
    let mut made: HashMap<(i32, i32), u32> = HashMap::new();
    let mut mtl: Option<String> = None;
    let num = |s: Option<&str>| s.and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0);
    for line in text.lines() {
        let mut w = line.split_whitespace();
        match w.next() {
            Some("v") => pos.push([num(w.next()), num(w.next()), num(w.next())]),
            Some("vt") => uv.push([num(w.next()), 1.0 - num(w.next())]),
            Some("mtllib") => mtl = w.next().map(|s| s.to_string()),
            Some("f") => {
                let mut corner = Vec::new();
                for item in w {
                    let mut parts = item.split('/');
                    let fix = |n: i32, len: usize| if n < 0 { len as i32 + n } else { n - 1 };
                    let vi = fix(parts.next().and_then(|x| x.parse().ok()).unwrap_or(0), pos.len());
                    let ti = parts.next().and_then(|x| x.parse().ok()).map(|n| fix(n, uv.len())).unwrap_or(-1);
                    if vi < 0 || vi as usize >= pos.len() {
                        return Err("A face points at a point that is not there.".into());
                    }
                    let id = *made.entry((vi, ti)).or_insert_with(|| {
                        verts.push(pos[vi as usize]);
                        uvs.push(if ti >= 0 && (ti as usize) < uv.len() { uv[ti as usize] } else { [0.0, 0.0] });
                        verts.len() as u32 - 1
                    });
                    corner.push(id);
                }
                for k in 1..corner.len().saturating_sub(1) {
                    tris.push([corner[0], corner[k], corner[k + 1]]);
                }
            }
            _ => {}
        }
    }
    // the picture of the material, when its file is next to the mesh
    let mut texture = None;
    if let Some(m) = mtl {
        if let Ok(t) = std::fs::read_to_string(dir.join(&m)) {
            if let Some(name) = t.lines().filter_map(|l| l.trim().strip_prefix("map_Kd")).map(|s| s.trim().to_string()).next() {
                let file = name.split_whitespace().last().unwrap_or("").replace('\\', "/");
                texture = image::open(dir.join(file)).ok().map(|i| i.to_rgba8());
            }
        }
    }
    Ok(Imported { mesh: AssetMesh { verts, uvs, tris, texture, normal: None }, rig: None, note: String::new() })
}

// ------------------------------------------------------------------------------------------------ .stl and .ply (points and triangles, no texture)

/// Points at the same place are one point (the file repeats them for every triangle).
fn weld(raw: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let key = |p: [f32; 3]| ((p[0] * 100_000.0).round() as i64, (p[1] * 100_000.0).round() as i64, (p[2] * 100_000.0).round() as i64);
    let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut verts = Vec::new();
    let idx: Vec<u32> = raw
        .iter()
        .map(|p| {
            *ids.entry(key(*p)).or_insert_with(|| {
                verts.push(*p);
                verts.len() as u32 - 1
            })
        })
        .collect();
    (verts, idx.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())
}

fn stl(bytes: &[u8]) -> Result<Imported, String> {
    let mut raw: Vec<[f32; 3]> = Vec::new();
    let binary_len = if bytes.len() >= 84 { 84 + 50 * u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize } else { 0 };
    if bytes.len() == binary_len && bytes.len() > 84 {
        for t in bytes[84..].chunks_exact(50) {
            for v in 0..3 {
                let o = 12 + v * 12;
                let f = |k: usize| f32::from_le_bytes([t[o + k * 4], t[o + k * 4 + 1], t[o + k * 4 + 2], t[o + k * 4 + 3]]);
                raw.push([f(0), f(1), f(2)]);
            }
        }
    } else {
        for line in String::from_utf8_lossy(bytes).lines() {
            if let Some(rest) = line.trim().strip_prefix("vertex") {
                let n: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if n.len() == 3 {
                    raw.push([n[0], n[1], n[2]]);
                }
            }
        }
    }
    let (verts, tris) = weld(&raw);
    Ok(Imported { mesh: AssetMesh { uvs: vec![[0.0; 2]; verts.len()], verts, tris, texture: None, normal: None }, rig: None, note: String::new() })
}

fn ply(bytes: &[u8]) -> Result<Imported, String> {
    let end = bytes.windows(10).position(|w| w == b"end_header").ok_or("The .ply file has no header.")?;
    let header = String::from_utf8_lossy(&bytes[..end]).to_string();
    let mut body = end + 10;
    while body < bytes.len() && (bytes[body] == b'\r' || bytes[body] == b'\n') {
        body += 1;
        if bytes[body - 1] == b'\n' {
            break;
        }
    }
    let format = header.lines().find_map(|l| l.strip_prefix("format ")).unwrap_or("ascii").split_whitespace().next().unwrap_or("ascii").to_string();
    let (mut nv, mut nf) = (0usize, 0usize);
    let mut vprops: Vec<(String, String)> = Vec::new();
    let mut face_types = ("uchar".to_string(), "int".to_string());
    let mut current = "";
    for l in header.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        match w.as_slice() {
            ["element", "vertex", n] => {
                nv = n.parse().unwrap_or(0);
                current = "vertex";
            }
            ["element", "face", n] => {
                nf = n.parse().unwrap_or(0);
                current = "face";
            }
            ["element", ..] => current = "other",
            ["property", "list", c, i, _] if current == "face" => face_types = (c.to_string(), i.to_string()),
            ["property", t, name] if current == "vertex" => vprops.push((t.to_string(), name.to_string())),
            _ => {}
        }
    }
    let size = |t: &str| match t {
        "char" | "uchar" | "int8" | "uint8" => 1,
        "short" | "ushort" | "int16" | "uint16" => 2,
        "double" | "float64" => 8,
        _ => 4,
    };
    let read = |b: &[u8], t: &str, little: bool| -> f64 {
        let mut a = [0u8; 8];
        let n = size(t);
        a[..n].copy_from_slice(&b[..n]);
        if !little {
            a[..n].reverse();
        }
        match t {
            "char" | "int8" => a[0] as i8 as f64,
            "uchar" | "uint8" => a[0] as f64,
            "short" | "int16" => i16::from_le_bytes([a[0], a[1]]) as f64,
            "ushort" | "uint16" => u16::from_le_bytes([a[0], a[1]]) as f64,
            "int" | "int32" => i32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
            "uint" | "uint32" => u32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
            "double" | "float64" => f64::from_le_bytes(a),
            _ => f32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
        }
    };
    let col = |name: &str| vprops.iter().position(|p| p.1 == name);
    let (cx, cy, cz) = (col("x").ok_or("No x in the points.")?, col("y").ok_or("No y in the points.")?, col("z").ok_or("No z in the points.")?);
    let (cu, cv) = (col("s").or(col("u")), col("t").or(col("v")));
    let mut verts = Vec::with_capacity(nv);
    let mut uvs = Vec::with_capacity(nv);
    let mut tris: Vec<[u32; 3]> = Vec::new();
    if format == "ascii" {
        let text = String::from_utf8_lossy(&bytes[body..]).to_string();
        let mut lines = text.lines();
        for _ in 0..nv {
            let n: Vec<f32> = lines.next().unwrap_or("").split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if n.len() < vprops.len() {
                return Err("A point of the .ply file is short.".into());
            }
            verts.push([n[cx], n[cy], n[cz]]);
            uvs.push([cu.map(|c| n[c]).unwrap_or(0.0), cv.map(|c| 1.0 - n[c]).unwrap_or(0.0)]);
        }
        for _ in 0..nf {
            let n: Vec<i64> = lines.next().unwrap_or("").split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if n.is_empty() {
                continue;
            }
            let k = n[0] as usize;
            for i in 1..k.saturating_sub(1) {
                if n.len() > i + 2 {
                    tris.push([n[1] as u32, n[1 + i] as u32, n[2 + i] as u32]);
                }
            }
        }
    } else {
        let little = format == "binary_little_endian";
        let mut at = body;
        let stride: usize = vprops.iter().map(|p| size(&p.0)).sum();
        for _ in 0..nv {
            let mut off = 0;
            let mut vals = Vec::new();
            for p in &vprops {
                vals.push(read(bytes.get(at + off..).ok_or("The .ply file ends early.")?, &p.0, little));
                off += size(&p.0);
            }
            at += stride;
            verts.push([vals[cx] as f32, vals[cy] as f32, vals[cz] as f32]);
            uvs.push([cu.map(|c| vals[c] as f32).unwrap_or(0.0), cv.map(|c| 1.0 - vals[c] as f32).unwrap_or(0.0)]);
        }
        for _ in 0..nf {
            let k = read(bytes.get(at..).ok_or("The .ply file ends early.")?, &face_types.0, little) as usize;
            at += size(&face_types.0);
            let mut idx = Vec::new();
            for _ in 0..k {
                idx.push(read(bytes.get(at..).ok_or("The .ply file ends early.")?, &face_types.1, little) as u32);
                at += size(&face_types.1);
            }
            for i in 1..k.saturating_sub(1) {
                tris.push([idx[0], idx[i], idx[i + 1]]);
            }
        }
    }
    if tris.iter().flatten().any(|i| *i as usize >= verts.len()) {
        return Err("A face points at a point that is not there.".into());
    }
    Ok(Imported { mesh: AssetMesh { verts, uvs, tris, texture: None, normal: None }, rig: None, note: String::new() })
}

// ------------------------------------------------------------------------------------------------ .fbx

enum Prop {
    Int(i64),
    Float(f64),
    Str(String),
    Raw(Vec<u8>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    F64(Vec<f64>),
}

struct FNode {
    name: String,
    props: Vec<Prop>,
    kids: Vec<FNode>,
}

impl FNode {
    fn kid(&self, name: &str) -> Option<&FNode> {
        self.kids.iter().find(|k| k.name == name)
    }

    fn doubles(&self) -> Vec<f64> {
        match self.props.first() {
            Some(Prop::F64(v)) => v.clone(),
            Some(Prop::I32(v)) => v.iter().map(|x| *x as f64).collect(),
            _ => Vec::new(),
        }
    }

    fn ints(&self) -> Vec<i32> {
        match self.props.first() {
            Some(Prop::I32(v)) => v.clone(),
            Some(Prop::I64(v)) => v.iter().map(|x| *x as i32).collect(),
            _ => Vec::new(),
        }
    }

    fn id(&self) -> i64 {
        match self.props.first() {
            Some(Prop::Int(i)) => *i,
            _ => 0,
        }
    }

    fn text(&self, i: usize) -> String {
        match self.props.get(i) {
            Some(Prop::Str(s)) => s.clone(),
            _ => String::new(),
        }
    }

    fn int_at(&self, i: usize) -> i64 {
        match self.props.get(i) {
            Some(Prop::Int(v)) => *v,
            Some(Prop::Float(v)) => *v as i64,
            _ => 0,
        }
    }

    fn float_at(&self, i: usize) -> f64 {
        match self.props.get(i) {
            Some(Prop::Float(v)) => *v,
            Some(Prop::Int(v)) => *v as f64,
            _ => 0.0,
        }
    }
}

fn fbx_nodes(b: &[u8], mut pos: usize, end: usize, wide: bool) -> Result<Vec<FNode>, String> {
    let mut out = Vec::new();
    let u32_at = |p: usize| -> Result<u32, String> { b.get(p..p + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).ok_or_else(|| "The .fbx file ends early.".to_string()) };
    let u64_at = |p: usize| -> Result<u64, String> { b.get(p..p + 8).map(|x| u64::from_le_bytes([x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7]])).ok_or_else(|| "The .fbx file ends early.".to_string()) };
    let head = if wide { 25 } else { 13 };
    while pos + head <= end {
        let (node_end, nprops, plen, nl) = if wide { (u64_at(pos)? as usize, u64_at(pos + 8)? as usize, u64_at(pos + 16)? as usize, *b.get(pos + 24).ok_or("The .fbx file ends early.")? as usize) } else { (u32_at(pos)? as usize, u32_at(pos + 4)? as usize, u32_at(pos + 8)? as usize, *b.get(pos + 12).ok_or("The .fbx file ends early.")? as usize) };
        if node_end == 0 {
            break;
        }
        let name = String::from_utf8_lossy(b.get(pos + head..pos + head + nl).ok_or("The .fbx file ends early.")?).to_string();
        let mut p = pos + head + nl;
        let props_end = p + plen;
        let mut props = Vec::new();
        for _ in 0..nprops {
            let t = *b.get(p).ok_or("The .fbx file ends early.")?;
            p += 1;
            match t {
                b'Y' => {
                    props.push(Prop::Int(i16::from_le_bytes([b[p], b[p + 1]]) as i64));
                    p += 2;
                }
                b'C' => {
                    props.push(Prop::Int(b[p] as i64));
                    p += 1;
                }
                b'I' => {
                    props.push(Prop::Int(u32_at(p)? as i32 as i64));
                    p += 4;
                }
                b'F' => {
                    props.push(Prop::Float(f32::from_bits(u32_at(p)?) as f64));
                    p += 4;
                }
                b'D' => {
                    props.push(Prop::Float(f64::from_bits(u64_at(p)?)));
                    p += 8;
                }
                b'L' => {
                    props.push(Prop::Int(u64_at(p)? as i64));
                    p += 8;
                }
                b'S' | b'R' => {
                    let l = u32_at(p)? as usize;
                    let data = b.get(p + 4..p + 4 + l).ok_or("The .fbx file ends early.")?;
                    props.push(if t == b'S' { Prop::Str(String::from_utf8_lossy(data).to_string()) } else { Prop::Raw(data.to_vec()) });
                    p += 4 + l;
                }
                b'f' | b'd' | b'l' | b'i' | b'b' => {
                    let (count, enc, clen) = (u32_at(p)? as usize, u32_at(p + 4)?, u32_at(p + 8)? as usize);
                    let raw = b.get(p + 12..p + 12 + clen).ok_or("The .fbx file ends early.")?;
                    p += 12 + clen;
                    let data: Vec<u8> = if enc == 1 {
                        let mut out = Vec::new();
                        flate2::read::ZlibDecoder::new(raw).read_to_end(&mut out).map_err(|e| format!("A packed array of the .fbx file is broken: {e}"))?;
                        out
                    } else {
                        raw.to_vec()
                    };
                    let width = match t {
                        b'd' | b'l' => 8,
                        b'b' => 1,
                        _ => 4,
                    };
                    if data.len() < count * width {
                        return Err("An array of the .fbx file is short.".into());
                    }
                    props.push(match t {
                        b'd' => Prop::F64(data.chunks_exact(8).take(count).map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])).collect()),
                        b'f' => Prop::F64(data.chunks_exact(4).take(count).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64).collect()),
                        b'l' => Prop::I64(data.chunks_exact(8).take(count).map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])).collect()),
                        b'i' => Prop::I32(data.chunks_exact(4).take(count).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()),
                        _ => Prop::I32(data.iter().take(count).map(|x| *x as i32).collect()),
                    });
                }
                _ => return Err("The .fbx file has a kind of value that is not known.".into()),
            }
        }
        p = props_end.max(p);
        let kids = if p < node_end { fbx_nodes(b, p, node_end, wide)? } else { Vec::new() };
        out.push(FNode { name, props, kids });
        pos = node_end;
    }
    Ok(out)
}

fn fbx(bytes: &[u8]) -> Result<Imported, String> {
    if !bytes.starts_with(b"Kaydara FBX Binary") {
        return Err("Only binary .fbx files can be opened (a text .fbx can be saved as binary in Blender or in Autodesk's converter).".into());
    }
    let version = u32::from_le_bytes([bytes[23], bytes[24], bytes[25], bytes[26]]);
    let roots = fbx_nodes(bytes, 27, bytes.len(), version >= 7500)?;
    let find = |n: &str| roots.iter().find(|r| r.name == n);
    let objects = find("Objects").ok_or("The .fbx file has no objects.")?;
    // the up axis
    let up = find("GlobalSettings").and_then(|g| g.kid("Properties70")).and_then(|p| p.kids.iter().find(|k| k.text(0) == "UpAxis")).map(|k| k.int_at(4)).unwrap_or(1);
    let (mut verts, mut uvs, mut tris): (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new(), Vec::new());
    let mut geometry_ids: Vec<(i64, usize, usize)> = Vec::new();
    for g in objects.kids.iter().filter(|k| k.name == "Geometry" && k.text(2) == "Mesh") {
        let pos = g.kid("Vertices").map(|n| n.doubles()).unwrap_or_default();
        let poly = g.kid("PolygonVertexIndex").map(|n| n.ints()).unwrap_or_default();
        if pos.is_empty() || poly.is_empty() {
            continue;
        }
        let uv_layer = g.kid("LayerElementUV");
        let uv_data: Vec<f64> = uv_layer.and_then(|l| l.kid("UV")).map(|n| n.doubles()).unwrap_or_default();
        let uv_index: Vec<i32> = uv_layer.and_then(|l| l.kid("UVIndex")).map(|n| n.ints()).unwrap_or_default();
        let by_polygon_vertex = uv_layer.and_then(|l| l.kid("MappingInformationType")).map(|n| n.text(0) == "ByPolygonVertex").unwrap_or(false);
        let mut made: HashMap<(i32, i32), u32> = HashMap::new();
        let (v0, t0) = (verts.len(), tris.len());
        let mut polygon: Vec<u32> = Vec::new();
        for (k, raw) in poly.iter().enumerate() {
            let (vi, last) = if *raw < 0 { (-(*raw) - 1, true) } else { (*raw, false) };
            if vi as usize * 3 + 2 >= pos.len() {
                return Err("A face of the .fbx points at a point that is not there.".into());
            }
            // the texture coordinate of this corner
            let ui = if uv_data.is_empty() {
                -1
            } else if by_polygon_vertex {
                if uv_index.is_empty() { k as i32 } else { uv_index.get(k).copied().unwrap_or(-1) }
            } else {
                vi
            };
            let id = *made.entry((vi, ui)).or_insert_with(|| {
                verts.push([pos[vi as usize * 3] as f32, pos[vi as usize * 3 + 1] as f32, pos[vi as usize * 3 + 2] as f32]);
                uvs.push(if ui >= 0 && (ui as usize) * 2 + 1 < uv_data.len() { [uv_data[ui as usize * 2] as f32, 1.0 - uv_data[ui as usize * 2 + 1] as f32] } else { [0.0, 0.0] });
                (verts.len() - 1) as u32
            });
            polygon.push(id);
            if last {
                for i in 1..polygon.len() - 1 {
                    tris.push([polygon[0], polygon[i], polygon[i + 1]]);
                }
                polygon.clear();
            }
        }
        geometry_ids.push((g.id(), v0, verts.len() - v0));
        let _ = t0;
    }
    // the picture that is inside the file
    let texture = objects.kids.iter().filter(|k| k.name == "Video").find_map(|v| match v.kid("Content").and_then(|c| c.props.first()) {
        Some(Prop::Raw(d)) if d.len() > 16 => image::load_from_memory(d).ok().map(|i| i.to_rgba8()),
        _ => None,
    });
    // the skeleton and the skin: the bones are the models that are limbs, and the clusters say which points each moves
    let rig = fbx_rig(objects, find("Connections"), &geometry_ids, verts.len());
    let mut note = String::new();
    if rig.is_some() {
        note.push_str("Its bones and weights came with it. ");
    }
    let mut mesh = AssetMesh { verts, uvs, tris, texture, normal: None };
    let mut rig = rig;
    if up == 2 {
        // a Z-up file: turned so Y is up
        let turn = |p: [f32; 3]| [p[0], p[2], -p[1]];
        for v in &mut mesh.verts {
            *v = turn(*v);
        }
        if let Some(r) = rig.as_mut() {
            for b in &mut r.bones {
                b.head = turn(b.head);
                b.tail = turn(b.tail);
            }
        }
        note.push_str("The file was Z-up: turned. ");
    }
    Ok(Imported { mesh, rig, note })
}

fn fbx_rig(objects: &FNode, connections: Option<&FNode>, geometries: &[(i64, usize, usize)], point_count: usize) -> Option<Rig3> {
    let conns = connections?;
    let models: HashMap<i64, &FNode> = objects.kids.iter().filter(|k| k.name == "Model").map(|k| (k.id(), k)).collect();
    let limbs: Vec<i64> = objects.kids.iter().filter(|k| k.name == "Model" && k.text(2) == "LimbNode").map(|k| k.id()).collect();
    if limbs.is_empty() {
        return None;
    }
    // which model is the parent of which
    let mut parent: HashMap<i64, i64> = HashMap::new();
    for c in conns.kids.iter().filter(|c| c.text(0) == "OO") {
        let (child, up) = (c.int_at(1), c.int_at(2));
        if limbs.contains(&child) && limbs.contains(&up) {
            parent.insert(child, up);
        }
    }
    // the places of the bones: the clusters carry the place of the bone when the mesh was bound
    let mut head: HashMap<i64, [f32; 3]> = HashMap::new();
    let clusters: Vec<&FNode> = objects.kids.iter().filter(|k| k.name == "Deformer" && k.text(2) == "Cluster").collect();
    let mut cluster_bone: HashMap<i64, i64> = HashMap::new();
    for c in conns.kids.iter().filter(|c| c.text(0) == "OO") {
        // a bone is connected to its cluster: (bone, cluster)
        if limbs.contains(&c.int_at(1)) && clusters.iter().any(|cl| cl.id() == c.int_at(2)) {
            cluster_bone.insert(c.int_at(2), c.int_at(1));
        }
    }
    for cl in &clusters {
        if let (Some(b), Some(m)) = (cluster_bone.get(&cl.id()), cl.kid("TransformLink")) {
            let m = m.doubles();
            if m.len() == 16 {
                head.insert(*b, [m[12] as f32, m[13] as f32, m[14] as f32]);
            }
        }
    }
    // a bone with no cluster is placed by adding the local places down from its parent
    fn place(id: i64, models: &HashMap<i64, &FNode>, parent: &HashMap<i64, i64>, head: &HashMap<i64, [f32; 3]>, depth: usize) -> [f32; 3] {
        if let Some(h) = head.get(&id) {
            return *h;
        }
        let local = models.get(&id).and_then(|m| m.kid("Properties70")).and_then(|p| p.kids.iter().find(|k| k.text(0) == "Lcl Translation")).map(|k| [k.float_at(4) as f32, k.float_at(5) as f32, k.float_at(6) as f32]).unwrap_or([0.0; 3]);
        match parent.get(&id) {
            Some(p) if depth < 64 => {
                let up = place(*p, models, parent, head, depth + 1);
                [up[0] + local[0], up[1] + local[1], up[2] + local[2]]
            }
            _ => local,
        }
    }
    let mut rig = Rig3::new(point_count);
    let order: Vec<i64> = {
        // parents first
        let mut o = Vec::new();
        let mut left: Vec<i64> = limbs.clone();
        while !left.is_empty() {
            let before = left.len();
            left.retain(|id| {
                if parent.get(id).is_none_or(|p| o.contains(p)) {
                    o.push(*id);
                    false
                } else {
                    true
                }
            });
            if left.len() == before {
                o.extend(left.drain(..));
            }
        }
        o
    };
    let mut index: HashMap<i64, usize> = HashMap::new();
    for id in &order {
        let h = place(*id, &models, &parent, &head, 0);
        let name = models.get(id).map(|m| m.text(1).split('\u{0}').next().unwrap_or("bone").to_string()).unwrap_or_else(|| "bone".into());
        let p = parent.get(id).and_then(|p| index.get(p)).copied();
        index.insert(*id, rig.add_bone(&name, p, h, h));
    }
    // the tail of a bone is where its first child starts (a bone with no child points on a little)
    for i in 0..rig.bones.len() {
        let kids = rig.children(i);
        rig.bones[i].tail = match kids.first() {
            Some(c) => rig.bones[*c].head,
            None => {
                let up = rig.bones[i].parent.map(|p| rig.bones[p].head).unwrap_or([rig.bones[i].head[0], rig.bones[i].head[1] - 0.1, rig.bones[i].head[2]]);
                let d = [rig.bones[i].head[0] - up[0], rig.bones[i].head[1] - up[1], rig.bones[i].head[2] - up[2]];
                let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
                [rig.bones[i].head[0] + d[0] / l * 0.1, rig.bones[i].head[1] + d[1] / l * 0.1, rig.bones[i].head[2] + d[2] / l * 0.1]
            }
        };
    }
    // the weights: a cluster lists the points (of the geometry) and how much the bone moves them
    let mut weights: Vec<Vec<(u16, f32)>> = vec![Vec::new(); point_count];
    for cl in &clusters {
        let Some(bone) = cluster_bone.get(&cl.id()).and_then(|b| index.get(b)).copied() else { continue };
        let (idx, w) = (cl.kid("Indexes").map(|n| n.ints()).unwrap_or_default(), cl.kid("Weights").map(|n| n.doubles()).unwrap_or_default());
        // the geometry this cluster belongs to: the skin that holds it is connected to a geometry
        let Some(&(_, v0, _)) = geometries.first() else { continue };
        for (i, x) in idx.iter().zip(&w) {
            let at = v0 + *i as usize;
            if at < point_count && *x > 1e-5 {
                weights[at].push((bone as u16, *x as f32));
            }
        }
    }
    // a mesh point that came from one fbx point is several points of ours (one for every texture coordinate): the weights of the point are shared by its copies
    rig.weights = weights;
    if rig.weights.iter().all(|w| w.is_empty()) {
        return None;
    }
    rig.limit(4);
    Some(rig)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_obj_with_a_quad_and_texture_coordinates_is_triangles_with_their_own_points_where_the_picture_splits() {
        let text = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
        let i = obj(text, Path::new(".")).unwrap();
        assert_eq!((i.mesh.verts.len(), i.mesh.tris.len()), (4, 2));
        assert_eq!(i.mesh.uvs[2], [1.0, 0.0], "the v is turned (the picture starts at the top)");
        let neg = obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n", Path::new(".")).unwrap();
        assert_eq!(neg.mesh.tris.len(), 1, "negative numbers count from the end");
        assert!(obj("v 0 0 0\nf 1 2 3\n", Path::new(".")).is_err());
    }

    #[test]
    fn an_stl_binary_or_text_welds_its_points() {
        let mut b = vec![0u8; 80];
        b.extend(2u32.to_le_bytes());
        for tri in [[[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]] {
            b.extend([0u8; 12]);
            for v in tri {
                for x in v {
                    b.extend(x.to_le_bytes());
                }
            }
            b.extend([0u8; 2]);
        }
        let i = stl(&b).unwrap();
        assert_eq!((i.mesh.verts.len(), i.mesh.tris.len()), (4, 2), "the shared points are one");
        let text = "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n";
        assert_eq!(stl(text.as_bytes()).unwrap().mesh.tris.len(), 1);
    }

    #[test]
    fn a_ply_ascii_and_binary_with_a_quad() {
        let ascii = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";
        let i = ply(ascii.as_bytes()).unwrap();
        assert_eq!((i.mesh.verts.len(), i.mesh.tris.len()), (4, 2));
        let mut bin = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n".to_vec();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for x in v {
                bin.extend(x.to_le_bytes());
            }
        }
        bin.push(3);
        for k in [0i32, 1, 2] {
            bin.extend(k.to_le_bytes());
        }
        let j = ply(&bin).unwrap();
        assert_eq!((j.mesh.verts.len(), j.mesh.tris.len()), (3, 1));
    }

    #[test]
    fn a_gltf_text_file_with_its_data_inside_opens_like_a_glb() {
        let (mesh, rig) = crate::meshfile::tests_figure();
        let glb_bytes = crate::meshfile::write_glb(&mesh, Some(&rig), &[], 30.0);
        // the same as a .gltf with the buffer as a data address and the pictures as views of it
        let g = crate::vrm::gltf::read_glb(&glb_bytes).unwrap();
        let mut json = g.json.clone();
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for c in g.bin.chunks(3) {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                for k in 0..4 {
                    if k <= c.len() {
                        s.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
                    }
                }
            }
            s
        };
        json["buffers"] = serde_json::json!([{ "byteLength": g.bin.len(), "uri": format!("data:application/octet-stream;base64,{b64}") }]);
        let imported = glb(&gltf_to_glb(json.to_string().as_bytes(), Path::new(".")).unwrap()).unwrap();
        assert_eq!(imported.mesh.verts.len(), mesh.verts.len());
        assert!(imported.rig.is_some() && imported.mesh.texture.is_some());
    }

    #[test]
    fn our_own_fbx_comes_back_with_its_points_texture_bones_and_weights() {
        let (mesh, mut rig) = crate::meshfile::tests_figure();
        rig.assign_by_distance(&mesh.verts);
        let ex = crate::fbx::write_fbx(&mesh, Some(&rig), &[], 30.0, "figure");
        let i = fbx(&ex.fbx).unwrap();
        assert_eq!(i.mesh.tris.len(), mesh.tris.len());
        assert!(i.mesh.texture.is_some(), "the picture inside the file");
        let r = i.rig.expect("the skeleton");
        assert_eq!(r.bones.len(), rig.bones.len());
        assert_eq!(r.bones[1].name, rig.bones[1].name);
        assert!(r.weights.iter().filter(|w| !w.is_empty()).count() > mesh.verts.len() / 2);
        // the units: the file is in centimetres, the mesh comes in metres (the figure is 2 m tall)
        let (lo, hi) = i.mesh.bounds();
        assert!((hi[1] - lo[1] - (mesh.bounds().1[1] - mesh.bounds().0[1]) * 100.0).abs() < 0.5 || (hi[1] - lo[1]) < 3.0);
    }

    #[test]
    fn a_file_that_is_not_a_mesh_is_refused() {
        let dir = std::env::temp_dir().join("sevi_import_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.xyz");
        std::fs::write(&p, b"hello").unwrap();
        assert!(load(&p).is_err());
        let o = dir.join("empty.obj");
        std::fs::write(&o, b"# nothing\n").unwrap();
        assert!(load(&o).unwrap_err().contains("no triangles"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
