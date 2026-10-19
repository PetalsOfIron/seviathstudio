// SPDX-License-Identifier: GPL-3.0-or-later
// Reading Clip Studio Paint files (.clip). The file is a row of chunks (`CSFCHUNK` header, then `CHNK` + name + size): one chunk holds an
// SQLite database (the layers, their folders, clipping, blend modes and where their pixels are), the others hold the pixels: for every
// layer a row of 256 x 256 blocks, each a zlib stream with the alpha plane first (256 * 256 bytes) and then the colour (B, G, R, X for every
// pixel, not multiplied by the alpha). Written from looking at real files, not from a specification: what is not understood is left out
// and said in `Document.meta.notes`.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::Color32;
use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags};

use crate::doc::{Blend, Document, Layer, LayerKind, TILE};

const MAGIC: &[u8; 8] = b"CSFCHUNK";
const BLOCK_BEGIN: &str = "BlockDataBeginChunk";
const BLOCK_END: &str = "BlockDataEndChunk";

fn be64(b: &[u8], at: usize) -> Result<u64, String> {
    b.get(at..at + 8).map(|s| u64::from_be_bytes(s.try_into().unwrap())).ok_or_else(|| "The Clip Studio file is cut short.".to_string())
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes(s.try_into().unwrap()))
}

/// The blend mode of Clip Studio (numbers found by comparing a drawing with its PSD) as one of ours. Modes we do not have are the nearest one.
fn blend_of(n: i64) -> Blend {
    match n {
        0 => Blend::Normal,
        1 | 6 => Blend::Darken,
        2 => Blend::Multiply,
        3 | 4 => Blend::ColorBurn,
        7 | 13 => Blend::Lighten,
        8 => Blend::Screen,
        9 | 10 => Blend::ColorDodge,
        11 | 12 => Blend::Add,
        14 => Blend::Overlay,
        15 => Blend::SoftLight,
        16 | 17 | 18 | 19 | 20 => Blend::HardLight,
        21 => Blend::Difference,
        22 => Blend::Exclusion,
        23 => Blend::Hue,
        24 => Blend::Saturation,
        25 => Blend::Color,
        26 => Blend::Luminosity,
        _ => Blend::Normal,
    }
}

/// UTF-16 big endian text of a name, as the block markers write it.
fn utf16_be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect()
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// One block of pixels: where it is (its number) and its bytes when it is not empty.
struct Block {
    index: usize,
    zlib: Option<(usize, usize)>,
}

/// The blocks of an external chunk (`body` is the chunk's data after its name and size): the compressed ranges inside `body`.
fn blocks_of(body: &[u8]) -> Vec<Block> {
    let (begin, end) = (utf16_be(BLOCK_BEGIN), utf16_be(BLOCK_END));
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = find(body, &begin, pos) {
        let mut p = i + begin.len();
        let (Some(index), Some(not_empty)) = (be32(body, p), be32(body, p + 16)) else { break };
        p += 20;
        let mut zlib = None;
        if not_empty != 0 {
            // the length of the data, then the length of the zlib stream (little endian), then the stream
            let Some(len_bytes) = body.get(p + 4..p + 8) else { break };
            let zl = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
            let start = p + 8;
            if start + zl <= body.len() {
                zlib = Some((start, zl));
            }
            p = start + zl;
        }
        out.push(Block { index: index as usize, zlib });
        pos = find(body, &end, p).unwrap_or(body.len());
    }
    out
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(TILE * TILE * 5);
    flate2::read::ZlibDecoder::new(data).read_to_end(&mut out).ok()?;
    Some(out)
}

/// The pixels of one layer: its blocks as tiles of the document.
fn tiles_of(body: &[u8], w: u32, h: u32) -> HashMap<(i32, i32), Arc<Vec<Color32>>> {
    let cols = (w as usize).div_ceil(TILE);
    let rows = (h as usize).div_ceil(TILE);
    let blocks = blocks_of(body);
    let made: Vec<((i32, i32), Vec<Color32>)> = blocks
        .par_iter()
        .filter_map(|b| {
            let (start, len) = b.zlib?;
            if cols == 0 || b.index / cols >= rows {
                return None;
            }
            let (ty, tx) = (b.index / cols, b.index % cols);
            let raw = inflate(&body[start..start + len])?;
            let n = TILE * TILE;
            let mut px = vec![Color32::TRANSPARENT; n];
            let mut any = false;
            // the alpha plane, then B, G, R and a spare byte for every pixel; a block with only the alpha plane is a shape (black)
            let colour = raw.len() >= n * 5;
            for k in 0..n {
                let (x, y) = (tx * TILE + k % TILE, ty * TILE + k / TILE);
                if x >= w as usize || y >= h as usize {
                    continue;
                }
                let a = *raw.get(k)?;
                if a == 0 {
                    continue;
                }
                let (r, g, bl) = if colour {
                    let o = n + k * 4;
                    (raw[o + 2], raw[o + 1], raw[o])
                } else {
                    (0, 0, 0)
                };
                px[k] = Color32::from_rgba_unmultiplied(r, g, bl, a);
                any = true;
            }
            any.then_some(((tx as i32, ty as i32), px))
        })
        .collect();
    made.into_iter().map(|(k, v)| (k, Arc::new(v))).collect()
}

struct Row {
    name: String,
    kind: i64,
    clip: bool,
    opacity: f32,
    blend: Blend,
    folder: bool,
    visible: bool,
    next: i64,
    first_child: i64,
    mipmap: i64,
}

fn temp_db() -> PathBuf {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!("seviath_clip_{}_{t}.db", std::process::id()))
}

/// Open a `.clip` file's bytes as a document: the layers in their folders with their names, opacity, blend modes, hiding and clipping.
pub fn read(bytes: &[u8]) -> Result<Document, String> {
    if bytes.len() < 24 || &bytes[..8] != MAGIC {
        return Err("This is not a Clip Studio file.".into());
    }
    // the chunks
    let mut pos = 24usize;
    let mut sqlite: Option<(usize, usize)> = None;
    while pos + 16 <= bytes.len() {
        let name = &bytes[pos..pos + 8];
        let size = be64(bytes, pos + 8)? as usize;
        if name == b"CHNKSQLi" {
            sqlite = Some((pos + 16, size));
            break;
        }
        pos = pos.saturating_add(16).saturating_add(size);
    }
    let (start, size) = sqlite.ok_or("The Clip Studio file has no layer table.")?;
    let db_bytes = bytes.get(start..start + size).ok_or("The Clip Studio file is cut short.")?;
    // the database is opened from a file of its own (removed again)
    let path = temp_db();
    std::fs::write(&path, db_bytes).map_err(|e| e.to_string())?;
    let result = (|| -> Result<Document, String> {
        let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| format!("The layer table could not be read ({e})."))?;
        let sql = |e: rusqlite::Error| format!("The layer table of the Clip Studio file could not be read ({e}).");
        let (cw, ch, res, root): (f64, f64, f64, i64) = db
            .query_row("select CanvasWidth, CanvasHeight, CanvasResolution, CanvasRootFolder from Canvas", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(sql)?;
        let (w, h) = (cw.round().max(1.0) as u32, ch.round().max(1.0) as u32);
        if w > super::MAX_SIDE || h > super::MAX_SIDE {
            return Err(format!("This drawing is {w} x {h} pixels: too big to open."));
        }
        // the layers
        let mut rows: HashMap<i64, Row> = HashMap::new();
        {
            let mut st = db
                .prepare("select MainId, LayerName, LayerType, LayerClip, LayerOpacity, LayerComposite, LayerFolder, LayerVisibility, LayerNextIndex, LayerFirstChildIndex, LayerRenderMipmap from Layer")
                .map_err(sql)?;
            let it = st
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        Row {
                            name: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                            kind: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                            clip: r.get::<_, Option<i64>>(3)?.unwrap_or(0) != 0,
                            opacity: (r.get::<_, Option<f64>>(4)?.unwrap_or(256.0) / 256.0 * 100.0) as f32,
                            blend: blend_of(r.get::<_, Option<i64>>(5)?.unwrap_or(0)),
                            folder: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
                            visible: r.get::<_, Option<i64>>(7)?.unwrap_or(1) & 1 != 0,
                            next: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
                            first_child: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
                            mipmap: r.get::<_, Option<i64>>(10)?.unwrap_or(0),
                        },
                    ))
                })
                .map_err(sql)?;
            for r in it {
                let (id, row) = r.map_err(sql)?;
                rows.insert(id, row);
            }
        }
        // where the pixels of a layer are: its render mipmap, the first (full size) mipmap info, its offscreen, the external chunk
        let external = |mipmap: i64| -> Option<usize> {
            let base: i64 = db.query_row("select BaseMipmapInfo from Mipmap where MainId = ?1", [mipmap], |r| r.get(0)).ok()?;
            let off: i64 = db.query_row("select Offscreen from MipmapInfo where MainId = ?1", [base], |r| r.get(0)).ok()?;
            let ext: Vec<u8> = db.query_row("select BlockData from Offscreen where MainId = ?1", [off], |r| r.get(0)).ok()?;
            let id = String::from_utf8_lossy(&ext).to_string();
            let at: i64 = db.query_row("select Offset from ExternalChunk where ExternalID = ?1", [id], |r| r.get(0)).ok()?;
            Some(at as usize)
        };
        // the layer tree: a folder's first child is its lowest layer and every layer points to the one above it
        let mut layers: Vec<Layer> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        let mut next_id = 1u64;
        let mut work: Vec<(usize, i64)> = Vec::new();
        fn children(rows: &HashMap<i64, Row>, folder: i64) -> Vec<i64> {
            let mut list = Vec::new();
            let mut c = rows.get(&folder).map(|r| r.first_child).unwrap_or(0);
            let mut guard = 0;
            while c != 0 && guard < 100_000 {
                guard += 1;
                let Some(r) = rows.get(&c) else { break };
                list.push(c);
                c = r.next;
            }
            list.reverse();
            list
        }
        fn walk(rows: &HashMap<i64, Row>, folder: i64, parent: Option<u64>, next_id: &mut u64, layers: &mut Vec<Layer>, work: &mut Vec<(usize, i64)>, skipped: &mut Vec<String>, depth: usize) {
            if depth > 64 {
                return;
            }
            for id in children(rows, folder) {
                let r = &rows[&id];
                if !r.folder && r.mipmap == 0 {
                    // a paper layer and the like: nothing to draw
                    if r.kind != 1584 {
                        skipped.push(r.name.clone());
                    }
                    continue;
                }
                let lid = *next_id;
                *next_id += 1;
                let mut l = new_layer(lid, parent, &r.name);
                l.kind = if r.folder { LayerKind::Group } else { LayerKind::Raster };
                l.visible = r.visible;
                l.opacity = r.opacity.clamp(0.0, 100.0).round();
                l.blend = if r.folder && r.blend == Blend::Normal { Blend::PassThrough } else { r.blend };
                l.clip = r.clip && !r.folder;
                l.expanded = true;
                let at = layers.len();
                layers.push(l);
                if r.folder {
                    walk(rows, id, Some(lid), next_id, layers, work, skipped, depth + 1);
                } else {
                    work.push((at, id));
                }
            }
        }
        walk(&rows, root, None, &mut next_id, &mut layers, &mut work, &mut skipped, 0);
        // the pixels, a layer at a time (the blocks of one layer are opened in parallel)
        for (at, id) in work {
            let Some(chunk) = external(rows[&id].mipmap) else { continue };
            if be32(bytes, chunk + 8).is_none() {
                continue;
            }
            let size = be64(bytes, chunk + 8)? as usize;
            let Some(body) = bytes.get(chunk + 16..chunk + 16 + size) else { continue };
            // the chunk's own name and the size of its data come first
            let name_len = be64(body, 0)? as usize;
            let data_start = 8 + name_len + 8;
            let Some(data) = body.get(data_start..) else { continue };
            layers[at].tiles = tiles_of(data, w, h);
            layers[at].rev += 1;
        }
        let mut paper_layer = new_layer(next_id, None, "Paper");
        paper_layer.kind = LayerKind::Paper;
        paper_layer.visible = false;
        layers.push(paper_layer);
        let mut doc = Document::from_parts(w, h, Color32::WHITE, layers);
        doc.meta.dpi = res.round().clamp(1.0, 2400.0) as u32;
        if !skipped.is_empty() {
            doc.meta.notes = format!("Opened from a Clip Studio file. These layers were left out (a kind of layer that is not read yet): {}.", skipped.join(", "));
        } else {
            doc.meta.notes = "Opened from a Clip Studio file.".into();
        }
        super::project::ensure_usable(&mut doc);
        Ok(doc)
    })();
    let _ = std::fs::remove_file(&path);
    result
}

fn new_layer(id: u64, parent: Option<u64>, name: &str) -> Layer {
    Layer {
        id,
        parent,
        name: name.to_owned(),
        kind: LayerKind::Raster,
        visible: true,
        locked: false,
        lock_alpha: false,
        clip: false,
        expanded: true,
        tag: 0,
        opacity: 100.0,
        blend: Blend::Normal,
        tiles: HashMap::new(),
        fav: false,
        vector: None,
        fx: Vec::new(),
        adjust: None,
        light: None,
        depth: false,
        material: None,
        lightfx: None,
        solid: false,
        text: None,
        part: None,
        rev: 1,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_blend_modes_of_clip_studio_are_ours() {
        assert_eq!(blend_of(0), Blend::Normal);
        assert_eq!(blend_of(2), Blend::Multiply);
        assert_eq!(blend_of(8), Blend::Screen);
        assert_eq!(blend_of(9), Blend::ColorDodge);
        assert_eq!(blend_of(12), Blend::Add);
        assert_eq!(blend_of(999), Blend::Normal);
    }

    #[test]
    fn a_block_of_pixels_is_found_and_opened() {
        // one block: the alpha plane (all 255) and the colour (blue, green, red, spare)
        let n = TILE * TILE;
        let mut raw = vec![255u8; n];
        for _ in 0..n {
            raw.extend_from_slice(&[10, 20, 30, 0]);
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut z, &raw).unwrap();
        let z = z.finish().unwrap();
        let mut body = Vec::new();
        body.extend_from_slice(&(BLOCK_BEGIN.len() as u32).to_be_bytes());
        body.extend_from_slice(&utf16_be(BLOCK_BEGIN));
        for v in [0u32, 0x50000, 256, 256, 1] {
            body.extend_from_slice(&v.to_be_bytes());
        }
        body.extend_from_slice(&((z.len() + 4) as u32).to_be_bytes());
        body.extend_from_slice(&(z.len() as u32).to_le_bytes());
        body.extend_from_slice(&z);
        body.extend_from_slice(&(BLOCK_END.len() as u32).to_be_bytes());
        body.extend_from_slice(&utf16_be(BLOCK_END));
        let tiles = tiles_of(&body, 300, 100);
        assert_eq!(tiles.len(), 1);
        let t = &tiles[&(0, 0)];
        assert_eq!(t[0], Color32::from_rgba_unmultiplied(30, 20, 10, 255));
        // the part of the block outside the page stays empty
        assert_eq!(t[TILE * 150], Color32::TRANSPARENT);
    }

    #[test]
    fn something_that_is_not_a_clip_file_is_refused_in_words() {
        assert!(read(b"not a clip file at all, really").is_err());
        assert!(read(&[]).is_err());
    }

    /// A small `.clip` file made here: a folder with two layers (the upper one multiplied, half see-through and clipped, hidden is the third).
    pub(crate) fn tiny_clip() -> Vec<u8> {
        let path = temp_db();
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(
                "create table Canvas (CanvasWidth, CanvasHeight, CanvasResolution, CanvasRootFolder);
                 create table Layer (MainId integer, LayerName, LayerType, LayerClip, LayerOpacity, LayerComposite, LayerFolder, LayerVisibility, LayerNextIndex, LayerFirstChildIndex, LayerRenderMipmap);
                 create table Mipmap (MainId integer, LayerId, BaseMipmapInfo);
                 create table MipmapInfo (MainId integer, Offscreen);
                 create table Offscreen (MainId integer, BlockData);
                 create table ExternalChunk (ExternalID, Offset);
                 insert into Canvas values (300.0, 100.0, 72.0, 1);
                 -- the root folder 1 has the folder 2 (its lowest layer is 2) and a layer 5 above it
                 insert into Layer values (1, '', 256, 0, 256, 0, 1, 1, 0, 2, 0);
                 insert into Layer values (2, 'Group', 0, 0, 256, 0, 1, 1, 5, 3, 0);
                 insert into Layer values (3, 'Base', 1, 0, 256, 0, 0, 1, 4, 0, 10);
                 insert into Layer values (4, 'Shade', 1, 1, 128, 2, 0, 1, 0, 0, 10);
                 insert into Layer values (5, 'Top', 1, 0, 256, 0, 0, 0, 0, 0, 10);
                 insert into Mipmap values (10, 3, 20);
                 insert into MipmapInfo values (20, 30);
                 insert into Offscreen values (30, cast('extrnlidAAAA' as blob));",
            )
            .unwrap();
        }
        let db_bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        // the chunk with the pixels: one block, all red
        let n = TILE * TILE;
        let mut raw = vec![255u8; n];
        for _ in 0..n {
            raw.extend_from_slice(&[0, 0, 200, 0]);
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut z, &raw).unwrap();
        let z = z.finish().unwrap();
        let mut blocks = Vec::new();
        blocks.extend_from_slice(&(BLOCK_BEGIN.len() as u32).to_be_bytes());
        blocks.extend_from_slice(&utf16_be(BLOCK_BEGIN));
        for v in [0u32, 0x50000, 256, 256, 1] {
            blocks.extend_from_slice(&v.to_be_bytes());
        }
        blocks.extend_from_slice(&((z.len() + 4) as u32).to_be_bytes());
        blocks.extend_from_slice(&(z.len() as u32).to_le_bytes());
        blocks.extend_from_slice(&z);
        blocks.extend_from_slice(&(BLOCK_END.len() as u32).to_be_bytes());
        blocks.extend_from_slice(&utf16_be(BLOCK_END));
        let name = b"extrnlidAAAA";
        let mut ext_body = Vec::new();
        ext_body.extend_from_slice(&(name.len() as u64).to_be_bytes());
        ext_body.extend_from_slice(name);
        ext_body.extend_from_slice(&(blocks.len() as u64).to_be_bytes());
        ext_body.extend_from_slice(&blocks);
        let mut file = Vec::new();
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&0u64.to_be_bytes());
        file.extend_from_slice(&24u64.to_be_bytes());
        let ext_at = file.len();
        file.extend_from_slice(b"CHNKExta");
        file.extend_from_slice(&(ext_body.len() as u64).to_be_bytes());
        file.extend_from_slice(&ext_body);
        // the offset of the chunk goes into the table, so the database is made again with it
        let path = temp_db();
        std::fs::write(&path, &db_bytes).unwrap();
        {
            let db = Connection::open(&path).unwrap();
            db.execute("insert into ExternalChunk values ('extrnlidAAAA', ?1)", [ext_at as i64]).unwrap();
        }
        let db_bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        file.extend_from_slice(b"CHNKSQLi");
        file.extend_from_slice(&(db_bytes.len() as u64).to_be_bytes());
        file.extend_from_slice(&db_bytes);
        file.extend_from_slice(b"CHNKFoot");
        file.extend_from_slice(&0u64.to_be_bytes());
        file
    }

    #[test]
    fn a_small_clip_file_opens_with_its_folders_order_clipping_blend_and_pixels() {
        let d = read(&tiny_clip()).unwrap();
        assert_eq!((d.w, d.h, d.meta.dpi), (300, 100, 72));
        let names: Vec<(&str, bool)> = d.layers.iter().map(|l| (l.name.as_str(), l.parent.is_some())).collect();
        // the top layer first; the folder, then what is inside it from the top down; the paper last
        assert_eq!(names, vec![("Top", false), ("Group", false), ("Shade", true), ("Base", true), ("Paper", false)]);
        let by = |n: &str| d.layers.iter().find(|l| l.name == n).unwrap();
        assert!(!by("Top").visible);
        assert!(by("Shade").clip && by("Shade").blend == Blend::Multiply && (by("Shade").opacity - 50.0).abs() < 1.0);
        assert!(!by("Base").clip && by("Base").blend == Blend::Normal);
        assert_eq!(by("Group").kind, LayerKind::Group);
        // the pixels of the layers that share the block
        let t = &by("Base").tiles[&(0, 0)];
        assert_eq!(t[0], Color32::from_rgba_unmultiplied(200, 0, 0, 255));
        assert_eq!(t[TILE - 1 + 0], Color32::from_rgba_unmultiplied(200, 0, 0, 255));
    }
}
