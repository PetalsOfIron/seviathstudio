// SPDX-License-Identifier: GPL-3.0-or-later
// A small glTF 2.0 binary (.glb) writer and reader. VRM is glTF with extra information, so this is the base of the
// VRM engine. The writer collects buffers, accessors, images, materials, meshes, nodes and skins and packs them; the
// reader checks every size and index before use, because model files come from outside.

use serde_json::{Value, json};

const MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

/// glTF component types.
pub const F32: u32 = 5126;
pub const U16: u32 = 5123;
pub const U32: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_BUFFER: u32 = 34963;

/// Collects everything that goes into the file.
#[derive(Default)]
pub struct Builder {
    pub bin: Vec<u8>,
    pub views: Vec<Value>,
    pub accessors: Vec<Value>,
    pub images: Vec<Value>,
    pub textures: Vec<Value>,
    pub materials: Vec<Value>,
    pub meshes: Vec<Value>,
    pub nodes: Vec<Value>,
    pub skins: Vec<Value>,
    pub animations: Vec<Value>,
    pub scenes: Vec<Value>,
    pub extensions_used: Vec<String>,
    pub extensions: serde_json::Map<String, Value>,
    pub asset_generator: String,
}

impl Builder {
    pub fn new(generator: &str) -> Builder {
        Builder { asset_generator: generator.to_owned(), ..Default::default() }
    }

    fn align(&mut self) {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
    }

    /// Put bytes in the binary buffer; returns the buffer view.
    pub fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.align();
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let mut v = json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() });
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.views.push(v);
        self.views.len() - 1
    }

    fn accessor(&mut self, view: usize, component: u32, count: usize, kind: &str, minmax: Option<([f32; 3], [f32; 3])>, normalized: bool) -> usize {
        let mut a = json!({ "bufferView": view, "componentType": component, "count": count, "type": kind });
        if let Some((lo, hi)) = minmax {
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        if normalized {
            a["normalized"] = json!(true);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    pub fn vec3s(&mut self, data: &[[f32; 3]], with_bounds: bool) -> usize {
        let bytes: Vec<u8> = data.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        let bounds = if with_bounds && !data.is_empty() {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in data {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            Some((lo, hi))
        } else {
            None
        };
        self.accessor(view, F32, data.len(), "VEC3", bounds, false)
    }

    pub fn vec2s(&mut self, data: &[[f32; 2]]) -> usize {
        let bytes: Vec<u8> = data.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        self.accessor(view, F32, data.len(), "VEC2", None, false)
    }

    pub fn vec4s(&mut self, data: &[[f32; 4]]) -> usize {
        let bytes: Vec<u8> = data.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        self.accessor(view, F32, data.len(), "VEC4", None, false)
    }

    pub fn joints(&mut self, data: &[[u16; 4]]) -> usize {
        let bytes: Vec<u8> = data.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        self.accessor(view, U16, data.len(), "VEC4", None, false)
    }

    pub fn indices(&mut self, tris: &[[u32; 3]]) -> usize {
        let bytes: Vec<u8> = tris.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ELEMENT_BUFFER));
        self.accessor(view, U32, tris.len() * 3, "SCALAR", None, false)
    }

    pub fn mat4s(&mut self, data: &[[f32; 16]]) -> usize {
        let bytes: Vec<u8> = data.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, None);
        self.accessor(view, F32, data.len(), "MAT4", None, false)
    }

    /// Numbers of an animation (times as SCALAR, turns as VEC4): no buffer target; returns the accessor.
    pub fn animation_data(&mut self, data: &[f32], kind: &str) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
        let per = match kind {
            "VEC4" => 4,
            "VEC3" => 3,
            _ => 1,
        };
        let view = self.view(&bytes, None);
        let minmax = if per == 1 && !data.is_empty() { Some(([data.iter().cloned().fold(f32::MAX, f32::min), 0.0, 0.0], [data.iter().cloned().fold(f32::MIN, f32::max), 0.0, 0.0])) } else { None };
        let id = self.accessor(view, F32, data.len() / per, kind, None, false);
        if let Some((lo, hi)) = minmax {
            self.accessors[id]["min"] = json!([lo[0]]);
            self.accessors[id]["max"] = json!([hi[0]]);
        }
        id
    }

    /// A PNG picture; returns the texture index.
    pub fn png_texture(&mut self, png: &[u8], name: &str) -> usize {
        let view = self.view(png, None);
        self.images.push(json!({ "bufferView": view, "mimeType": "image/png", "name": name }));
        self.textures.push(json!({ "source": self.images.len() - 1, "sampler": 0 }));
        self.textures.len() - 1
    }

    /// The whole file.
    pub fn glb(mut self) -> Vec<u8> {
        self.align();
        let mut root = json!({
            "asset": { "version": "2.0", "generator": self.asset_generator },
            "buffers": [{ "byteLength": self.bin.len() }],
            "bufferViews": self.views,
            "accessors": self.accessors,
            "nodes": self.nodes,
            "scenes": self.scenes,
            "scene": 0,
        });
        let map = root.as_object_mut().unwrap();
        for (k, v) in [("images", self.images), ("textures", self.textures), ("materials", self.materials), ("meshes", self.meshes), ("skins", self.skins), ("animations", self.animations)] {
            if !v.is_empty() {
                map.insert(k.into(), Value::Array(v));
            }
        }
        if !map.get("textures").is_none_or(|t| t.as_array().is_some_and(|a| a.is_empty())) {
            map.insert("samplers".into(), json!([{ "magFilter": 9729, "minFilter": 9729, "wrapS": 33071, "wrapT": 33071 }]));
        }
        if !self.extensions_used.is_empty() {
            map.insert("extensionsUsed".into(), json!(self.extensions_used));
        }
        if !self.extensions.is_empty() {
            map.insert("extensions".into(), Value::Object(self.extensions));
        }
        pack(&root, &self.bin)
    }
}

/// A JSON description and a binary buffer as one .glb file.
pub fn pack(root: &Value, bin: &[u8]) -> Vec<u8> {
    let mut js = serde_json::to_vec(root).unwrap();
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let mut bin = bin.to_vec();
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend(MAGIC.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((js.len() as u32).to_le_bytes());
    out.extend(CHUNK_JSON.to_le_bytes());
    out.extend(js);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(CHUNK_BIN.to_le_bytes());
    out.extend(bin);
    out
}

/// A file read back: its JSON and its binary chunk.
pub struct Glb {
    pub json: Value,
    pub bin: Vec<u8>,
}

pub fn read_glb(data: &[u8]) -> Result<Glb, String> {
    let u32at = |p: usize| data.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if u32at(0) != Some(MAGIC) || u32at(4) != Some(2) {
        return Err("This is not a glTF 2.0 binary file.".into());
    }
    let total = u32at(8).ok_or("The file is cut short.")? as usize;
    if total > data.len() {
        return Err("The file is cut short.".into());
    }
    let (mut json, mut bin) = (None, Vec::new());
    let mut p = 12;
    while p + 8 <= total {
        let (len, kind) = (u32at(p).unwrap() as usize, u32at(p + 4).unwrap());
        let body = data.get(p + 8..p + 8 + len).ok_or("A part of the file is cut short.")?;
        match kind {
            CHUNK_JSON => json = Some(serde_json::from_slice::<Value>(body).map_err(|e| format!("The model description is damaged: {e}"))?),
            CHUNK_BIN => bin = body.to_vec(),
            _ => {}
        }
        p += 8 + len;
    }
    Ok(Glb { json: json.ok_or("The file has no model description.")?, bin })
}

impl Glb {
    fn view_bytes(&self, view: usize) -> Result<&[u8], String> {
        let v = self.json["bufferViews"].get(view).ok_or("A buffer view is missing.")?;
        let (off, len) = (v["byteOffset"].as_u64().unwrap_or(0) as usize, v["byteLength"].as_u64().ok_or("A buffer view has no length.")? as usize);
        self.bin.get(off..off.checked_add(len).ok_or("A buffer view is too big.")?).ok_or_else(|| "A buffer view points outside the file.".to_string())
    }

    /// The bytes of an accessor, with its component count and component type.
    fn accessor_bytes(&self, acc: usize) -> Result<(&[u8], usize, u32, usize), String> {
        let a = self.json["accessors"].get(acc).ok_or("An accessor is missing.")?;
        let count = a["count"].as_u64().ok_or("An accessor has no count.")? as usize;
        let n = match a["type"].as_str().unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            _ => return Err("An accessor has a type that is not supported.".into()),
        };
        let comp = a["componentType"].as_u64().ok_or("An accessor has no component type.")? as u32;
        let size = match comp {
            F32 | U32 => 4,
            U16 => 2,
            _ => return Err("An accessor has a component type that is not supported.".into()),
        };
        let bytes = self.view_bytes(a["bufferView"].as_u64().ok_or("An accessor has no buffer view.")? as usize)?;
        let extra = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let need = count.checked_mul(n * size).and_then(|v| v.checked_add(extra)).ok_or("An accessor is too big.")?;
        let body = bytes.get(extra..need).ok_or("An accessor points outside its buffer.")?;
        Ok((body, n, comp, count))
    }

    pub fn floats(&self, acc: usize) -> Result<Vec<f32>, String> {
        let (b, n, comp, count) = self.accessor_bytes(acc)?;
        if comp != F32 {
            return Err("Expected floating point numbers.".into());
        }
        Ok(b.chunks_exact(4).take(count * n).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }

    pub fn vec3s(&self, acc: usize) -> Result<Vec<[f32; 3]>, String> {
        Ok(self.floats(acc)?.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())
    }

    pub fn vec2s(&self, acc: usize) -> Result<Vec<[f32; 2]>, String> {
        Ok(self.floats(acc)?.chunks_exact(2).map(|c| [c[0], c[1]]).collect())
    }

    pub fn index_list(&self, acc: usize) -> Result<Vec<u32>, String> {
        let (b, _, comp, _) = self.accessor_bytes(acc)?;
        Ok(match comp {
            U32 => b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
            U16 => b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect(),
            _ => return Err("Expected whole numbers.".into()),
        })
    }

    /// An image's bytes (a PNG inside the file).
    pub fn image(&self, n: usize) -> Result<&[u8], String> {
        let i = self.json["images"].get(n).ok_or("An image is missing.")?;
        self.view_bytes(i["bufferView"].as_u64().ok_or("An image has no data in the file.")? as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_written_can_be_read_back_exactly() {
        let mut b = Builder::new("test");
        let pos = b.vec3s(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.5]], true);
        let uv = b.vec2s(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        let idx = b.indices(&[[0, 1, 2]]);
        let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
        let tex = b.png_texture(&png, "skin");
        b.nodes.push(json!({ "name": "root" }));
        b.scenes.push(json!({ "nodes": [0] }));
        let bytes = b.glb();
        assert_eq!(bytes.len() % 4, 0);
        let g = read_glb(&bytes).unwrap();
        assert_eq!(g.json["asset"]["version"], "2.0");
        assert_eq!(g.vec3s(pos).unwrap(), [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.5]]);
        assert_eq!(g.vec2s(uv).unwrap()[2], [0.0, 1.0]);
        assert_eq!(g.index_list(idx).unwrap(), [0, 1, 2]);
        assert_eq!(g.image(tex).unwrap(), &png[..]);
        // the bounds of the positions are written
        assert_eq!(g.json["accessors"][0]["max"], json!([1.0, 2.0, 0.5]));
        assert_eq!(g.json["accessors"][0]["min"], json!([0.0, 0.0, 0.0]));
    }

    #[test]
    fn damaged_files_are_refused_instead_of_crashing() {
        let mut b = Builder::new("test");
        b.vec3s(&[[0.0; 3]; 4], false);
        b.nodes.push(json!({}));
        b.scenes.push(json!({ "nodes": [0] }));
        let good = b.glb();
        assert!(read_glb(b"not a model").is_err());
        assert!(read_glb(&good[..good.len() - 30]).is_err(), "cut short");
        let mut lying = good.clone();
        lying[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_glb(&lying).is_err());
        // an accessor that points far outside the buffer
        let mut g = read_glb(&good).unwrap();
        g.json["accessors"][0]["count"] = json!(1_000_000_000u64);
        assert!(g.vec3s(0).is_err());
        g.json["accessors"][0]["count"] = json!(usize::MAX);
        assert!(g.vec3s(0).is_err());
        assert!(g.vec3s(7).is_err());
    }
}
