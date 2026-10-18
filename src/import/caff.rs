// SPDX-License-Identifier: GPL-3.0-or-later
// The container of a Live2D Cubism model file (.cmo3): a small archive ("CAFF") with a list of files. The names and
// numbers are scrambled with a key stored in the header; the model itself is main.xml, packed inside as a zip entry.
// Files come from outside: every size and position is checked before it is used.

use std::io::Read;

const MAX_FILES: usize = 100_000;
/// The biggest single file we unpack (the model XML of a big model is about 10 MB).
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub start: u64,
    pub size: u64,
    pub scrambled: bool,
    /// 16: stored as it is, 33: packed with zip.
    pub packing: u8,
}

pub struct Caff {
    data: Vec<u8>,
    key: u32,
    pub entries: Vec<Entry>,
}

impl Caff {
    pub fn parse(data: Vec<u8>) -> Result<Caff, String> {
        if data.len() < 0x40 || &data[..4] != b"CAFF" {
            return Err("This is not a Live2D model file (it does not start with CAFF).".into());
        }
        let key = u32::from_be_bytes([data[14], data[15], data[16], data[17]]);
        let kb = key as u8;
        let mut p = 0x36usize;
        let int = |p: usize| -> Result<u32, String> {
            let b = data.get(p..p + 4).ok_or("The file list is cut short.")?;
            Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) ^ key)
        };
        let string = |p: usize| -> Result<(String, usize), String> {
            let len = (*data.get(p).ok_or("The file list is cut short.")? ^ kb) as usize;
            let b = data.get(p + 1..p + 1 + len).ok_or("The file list is cut short.")?;
            Ok((b.iter().map(|c| (c ^ kb) as char).filter(|c| !c.is_control()).collect(), p + 1 + len))
        };
        let n = int(p)? as usize;
        p += 4;
        if n > MAX_FILES {
            return Err("The file list is too long.".into());
        }
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let (name, q) = string(p)?;
            let (_tag, q) = string(q)?;
            let start = int(q + 4)? as u64;
            let size = int(q + 8)? as u64;
            let q = q + 12;
            let scrambled = data.get(q).ok_or("The file list is cut short.")? ^ kb != 0;
            let packing = data.get(q + 1).ok_or("The file list is cut short.")? ^ kb;
            p = q + 10;
            if start.checked_add(size).is_none_or(|e| e > data.len() as u64) {
                return Err(format!("The file {name} points outside the model file."));
            }
            entries.push(Entry { name, start, size, scrambled, packing });
        }
        Ok(Caff { data, key, entries })
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// The bytes of one file, unscrambled and unpacked.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let e = self.find(name).ok_or_else(|| format!("The model has no file {name}."))?;
        let kb = self.key as u8;
        let raw = &self.data[e.start as usize..(e.start + e.size) as usize];
        let bytes: Vec<u8> = if e.scrambled { raw.iter().map(|b| b ^ kb).collect() } else { raw.to_vec() };
        if e.packing == 16 {
            return Ok(bytes);
        }
        unzip_first(&bytes)
    }
}

/// The first file of a zip, read from its local header (the list at the end of these zips is not always there).
fn unzip_first(z: &[u8]) -> Result<Vec<u8>, String> {
    let u16le = |p: usize| z.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32le = |p: usize| z.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64);
    if z.get(..4) != Some(b"PK\x03\x04") {
        return Err("A packed file does not start like a zip.".into());
    }
    let (method, name_len, extra_len) = (u16le(8).ok_or("Cut short")?, u16le(26).ok_or("Cut short")?, u16le(28).ok_or("Cut short")?);
    let body = z.get(30 + name_len + extra_len..).ok_or("A packed file is cut short.")?;
    match method {
        0 => {
            let n = u32le(22).unwrap_or(0) as usize;
            Ok(body.get(..n.min(body.len())).unwrap_or(body).to_vec())
        }
        8 => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(body).take(MAX_UNPACKED).read_to_end(&mut out).map_err(|e| format!("A packed file is damaged: {e}"))?;
            Ok(out)
        }
        m => Err(format!("A packed file uses a packing method ({m}) that is not supported.")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Build a small archive the way the format is made, to test reading it.
    pub fn make(key: u32, files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let kb = key as u8;
        let mut head = vec![0u8; 0x36];
        head[..4].copy_from_slice(b"CAFF");
        head[7..11].copy_from_slice(b"----");
        head[14..18].copy_from_slice(&key.to_be_bytes());
        let mut table = Vec::new();
        table.extend((files.len() as u32 ^ key).to_be_bytes());
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        let mut table_len = 4;
        for (n, _, _) in files {
            table_len += 1 + n.len() + 1 + 12 + 2 + 8;
        }
        let mut pos = (0x36 + table_len) as u32;
        for (n, content, zipped) in files {
            let packed: Vec<u8> = if *zipped {
                let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(content).unwrap();
                let body = enc.finish().unwrap();
                let mut z = b"PK\x03\x04\x14\x00\x00\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x08\x00\x00\x00contents".to_vec();
                z[26] = 8;
                z.extend(body);
                z
            } else {
                content.to_vec()
            };
            let scr: Vec<u8> = packed.iter().map(|b| b ^ kb).collect();
            table.push(n.len() as u8 ^ kb);
            table.extend(n.bytes().map(|c| c ^ kb));
            table.push(kb);
            table.extend((0u32 ^ key).to_be_bytes());
            table.extend((pos ^ key).to_be_bytes());
            table.extend((scr.len() as u32 ^ key).to_be_bytes());
            table.push(1 ^ kb);
            table.push(if *zipped { 33 } else { 16 } ^ kb);
            table.extend([kb; 8]);
            pos += scr.len() as u32;
            bodies.push(scr);
        }
        let mut out = head;
        out.extend(table);
        for b in bodies {
            out.extend(b);
        }
        out
    }

    #[test]
    fn reads_stored_and_zipped_files_and_refuses_damage() {
        let png = b"\x89PNG\r\n\x1a\nhello";
        let xml = b"<?xml version=\"1.0\"?><CModelSource/>".repeat(50);
        let data = make(0x038656e3, &[("image_0.png", png, false), ("main.xml", &xml, true)]);
        let c = Caff::parse(data.clone()).unwrap();
        assert_eq!(c.entries.len(), 2);
        assert_eq!(c.read("image_0.png").unwrap(), png);
        assert_eq!(c.read("main.xml").unwrap(), xml);
        assert!(c.read("nothing").is_err());
        // not a model file, a cut file, and an entry that points outside
        assert!(Caff::parse(b"PK\x03\x04".repeat(40)).is_err());
        assert!(Caff::parse(data[..0x40].to_vec()).is_err());
        assert!(Caff::parse(data[..data.len() - 20].to_vec()).is_err());
        let mut bad = data.clone();
        bad.truncate(0x36 + 4);
        bad.extend([0; 40]);
        assert!(Caff::parse(bad).is_err());
    }
}
