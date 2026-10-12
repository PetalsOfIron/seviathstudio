// Importing brushes from other programs, and saving/sharing your own.
//
//  Photoshop  .abr        brush tip pictures (sampled brushes) + names, spacing and size
//  Krita      .kpp        one brush preset (size, spacing, roundness, angle, softness…)
//             .bundle     a pack of presets with their tip pictures
//  GIMP/Krita .gbr .gih   brush tip pictures
//  Clip Studio .sut       name, size, opacity, spacing and the tip picture when it can be found
//  Any image  .png .jpg   becomes a brush tip (dark = paint, or the picture's transparency)
//  SeviathStudio .seviathbrush, and brush cards: a PNG picture of the brush with the brush inside
//
// Every program has its own brush engine, so imported brushes keep their tip and basic
// settings; special effects of the other program (dual brush, color dynamics…) are not copied.

import JSZip from 'jszip';
import { renderBrushPreview, ensureTip, getTipMask, type BrushSettings } from './brush';

export type ImportedBrush = { name: string; settings: Partial<BrushSettings>; source: string; category?: string; about?: string };
export type ImportResult = { brushes: ImportedBrush[]; notes: string[] };

const ext = (n: string) => (n.match(/\.([^.]+)$/)?.[1] ?? '').toLowerCase();
const base = (n: string) => n.replace(/^.*[\\/]/, '').replace(/\.[^.]+$/, '');
const newTipId = () => Math.random().toString(36).slice(2, 10);

// ---------- Tip pictures ----------

// Grayscale or alpha values (0 = nothing, 255 = full paint) → white PNG tip with transparency
function maskToTip(values: ArrayLike<number>, w: number, h: number): string {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  const ctx = c.getContext('2d')!;
  const img = ctx.createImageData(w, h);
  for (let i = 0; i < w * h; i++) {
    img.data[i * 4] = 255;
    img.data[i * 4 + 1] = 255;
    img.data[i * 4 + 2] = 255;
    img.data[i * 4 + 3] = values[i];
  }
  ctx.putImageData(img, 0, 0);
  return shrink(c);
}

// Tips are drawn at 256 px at most, so bigger pictures are made smaller (smaller files too)
function shrink(c: HTMLCanvasElement): string {
  const max = 256;
  if (c.width <= max && c.height <= max) return c.toDataURL('image/png');
  const k = max / Math.max(c.width, c.height);
  const s = document.createElement('canvas');
  s.width = Math.max(1, Math.round(c.width * k));
  s.height = Math.max(1, Math.round(c.height * k));
  const ctx = s.getContext('2d')!;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(c, 0, 0, s.width, s.height);
  return s.toDataURL('image/png');
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error('Could not read the picture'));
    img.src = src;
  });
}

// Any picture → tip. Pictures with transparency use it; others use darkness (black = paint).
export async function imageToTip(blob: Blob): Promise<{ tip: string; w: number; h: number }> {
  const url = URL.createObjectURL(blob);
  try {
    const img = await loadImage(url);
    const c = document.createElement('canvas');
    c.width = img.naturalWidth || 256;
    c.height = img.naturalHeight || 256;
    const ctx = c.getContext('2d', { willReadFrequently: true })!;
    ctx.drawImage(img, 0, 0, c.width, c.height);
    const d = ctx.getImageData(0, 0, c.width, c.height).data;
    let clear = 0;
    for (let i = 3; i < d.length; i += 4) if (d[i] < 250) clear++;
    const useAlpha = clear > (d.length / 4) * 0.02;
    const vals = new Uint8ClampedArray(c.width * c.height);
    for (let p = 0, i = 0; p < vals.length; p++, i += 4) {
      const lum = 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
      vals[p] = useAlpha ? d[i + 3] : 255 - lum;
    }
    return { tip: maskToTip(vals, c.width, c.height), w: c.width, h: c.height };
  } finally {
    URL.revokeObjectURL(url);
  }
}

const imageTip = (tip: string, size: number, spacing: number): Partial<BrushSettings> => ({
  tip: 'image',
  tipImage: tip,
  tipId: newTipId(),
  size: Math.max(1, Math.min(500, Math.round(size))),
  spacing: Math.max(0.02, Math.min(2, spacing)),
  hardness: 1,
  roundness: 1,
  sizePressure: true,
  minSize: 0.5,
});

// ---------- Photoshop .abr ----------

class Reader {
  v: DataView;
  pos = 0;
  constructor(public buf: ArrayBuffer) {
    this.v = new DataView(buf);
  }
  get left() {
    return this.buf.byteLength - this.pos;
  }
  u8() {
    return this.v.getUint8(this.pos++);
  }
  u16() {
    const x = this.v.getUint16(this.pos);
    this.pos += 2;
    return x;
  }
  i32() {
    const x = this.v.getInt32(this.pos);
    this.pos += 4;
    return x;
  }
  u32() {
    const x = this.v.getUint32(this.pos);
    this.pos += 4;
    return x;
  }
  str(n: number) {
    let s = '';
    for (let i = 0; i < n; i++) s += String.fromCharCode(this.v.getUint8(this.pos + i));
    this.pos += n;
    return s;
  }
}

// Photoshop's simple run-length packing
function unpackBits(r: Reader, out: Uint8Array, at: number, count: number, rowBytes: number) {
  const end = r.pos + count;
  let o = at;
  while (r.pos < end && o < at + rowBytes) {
    let n = r.u8();
    if (n > 127) n -= 256;
    if (n >= 0) {
      for (let i = 0; i <= n && o < at + rowBytes; i++) out[o++] = r.u8();
    } else if (n !== -128) {
      const v = r.u8();
      for (let i = 0; i < 1 - n && o < at + rowBytes; i++) out[o++] = v;
    }
  }
  r.pos = end;
}

// The picture of one sampled brush: rectangle, bit depth, compression, then pixels
function readSampled(r: Reader, limit: number): { vals: Uint8Array; w: number; h: number } | null {
  const top = r.i32(), left = r.i32(), bottom = r.i32(), right = r.i32();
  const depth = r.u16();
  const comp = r.u8();
  const w = right - left, h = bottom - top;
  if (w <= 0 || h <= 0 || w > 8000 || h > 8000 || (depth !== 8 && depth !== 16) || comp > 1) return null;
  const bpp = depth / 8;
  const raw = new Uint8Array(w * h * bpp);
  if (comp === 0) {
    if (r.pos + raw.length > limit) return null;
    raw.set(new Uint8Array(r.buf, r.pos, raw.length));
    r.pos += raw.length;
  } else {
    const counts: number[] = [];
    for (let y = 0; y < h; y++) counts.push(r.u16());
    for (let y = 0; y < h; y++) {
      if (r.pos + counts[y] > limit) return null;
      unpackBits(r, raw, y * w * bpp, counts[y], w * bpp);
    }
  }
  const vals = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i++) vals[i] = raw[i * bpp]; // 16-bit: the high byte is enough
  return { vals, w, h };
}

// Looks for the brush rectangle header inside a block (its position differs between versions)
function findSampled(buf: ArrayBuffer, from: number, to: number) {
  const tryAt = (at: number) => {
    if (at + 19 > to) return null;
    const r = new Reader(buf);
    r.pos = at;
    try {
      const s = readSampled(r, to);
      return s && s.w * s.h > 1 ? s : null;
    } catch {
      return null;
    }
  };
  for (const skip of [301, 47, 37, 264]) {
    const s = tryAt(from + skip);
    if (s) return s;
  }
  for (let at = from; at < Math.min(to - 19, from + 1200); at++) {
    const s = tryAt(at);
    if (s) return s;
  }
  return null;
}

// Values from the brush descriptions ('desc' block): names, spacing, diameter
function descValues(bytes: Uint8Array) {
  const names: string[] = [];
  const spacing: number[] = [];
  const diam: number[] = [];
  const find = (sig: string, from: number) => {
    outer: for (let i = from; i <= bytes.length - sig.length; i++) {
      for (let k = 0; k < sig.length; k++) if (bytes[i + k] !== sig.charCodeAt(k)) continue outer;
      return i;
    }
    return -1;
  };
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  for (let i = find('Nm  TEXT', 0); i >= 0; i = find('Nm  TEXT', i + 8)) {
    const n = dv.getUint32(i + 8);
    let s = '';
    for (let k = 0; k < n && i + 12 + k * 2 + 1 < bytes.length; k++) {
      const c = dv.getUint16(i + 12 + k * 2);
      if (c) s += String.fromCharCode(c);
    }
    names.push(s.trim());
  }
  for (let i = find('SpcnUntF#Prc', 0); i >= 0; i = find('SpcnUntF#Prc', i + 12)) spacing.push(dv.getFloat64(i + 12));
  for (let i = find('DmtrUntF#Pxl', 0); i >= 0; i = find('DmtrUntF#Pxl', i + 12)) diam.push(dv.getFloat64(i + 12));
  return { names, spacing, diam };
}

export function parseABR(buf: ArrayBuffer, file: string): ImportResult {
  const r = new Reader(buf);
  const version = r.u16();
  const out: ImportedBrush[] = [];
  const notes: string[] = [];
  if (version === 1 || version === 2) {
    const count = r.u16();
    for (let i = 0; i < count && r.left > 6; i++) {
      const type = r.u16();
      const size = r.u32();
      const next = r.pos + size;
      if (type === 1) {
        // computed (round) brush
        r.u32();
        const spacing = r.u16(), diameter = r.u16(), roundness = r.u16();
        const angle = r.u16() << 16 >> 16; // signed
        const hardness = r.u16();
        out.push({
          name: `${base(file)} ${i + 1}`,
          source: file,
          settings: { tip: 'round', size: diameter, spacing: Math.max(0.02, spacing / 100), roundness: Math.max(0.05, roundness / 100), angle, hardness: hardness / 100 },
        });
      } else if (type === 2) {
        r.u32();
        const spacing = r.u16();
        let name = `${base(file)} ${i + 1}`;
        if (version === 2) {
          const n = r.u32();
          let s = '';
          for (let k = 0; k < n; k++) {
            const c = r.u16();
            if (c) s += String.fromCharCode(c);
          }
          if (s.trim()) name = s.trim();
        }
        r.u8(); // antialias
        r.pos += 8; // short bounds
        const s = readSampled(r, next);
        if (s) out.push({ name, source: file, settings: imageTip(maskToTip(s.vals, s.w, s.h), Math.max(s.w, s.h), spacing / 100 || 0.25) });
      }
      r.pos = next;
    }
  } else if (version >= 6) {
    const sub = r.u16();
    void sub;
    let descBytes: Uint8Array | null = null;
    const samples: { vals: Uint8Array; w: number; h: number }[] = [];
    while (r.left >= 12) {
      const sig = r.str(4);
      if (sig !== '8BIM') {
        // sections are padded: step back and search for the next one
        r.pos -= 3;
        continue;
      }
      const key = r.str(4);
      const size = r.u32();
      const start = r.pos, end = Math.min(buf.byteLength, start + size);
      if (key === 'samp') {
        let p = start;
        while (p + 4 < end) {
          const len = new DataView(buf).getUint32(p);
          const blockStart = p + 4;
          let next = blockStart + len;
          if (len % 4) next += 4 - (len % 4);
          const s = findSampled(buf, blockStart, Math.min(end, blockStart + len));
          if (s) samples.push(s);
          if (len === 0) break;
          p = next;
        }
      } else if (key === 'desc') {
        descBytes = new Uint8Array(buf, start, end - start);
      }
      r.pos = end;
    }
    const d = descBytes ? descValues(descBytes) : { names: [], spacing: [], diam: [] };
    // descriptions also list the computed brushes, so names only line up when the counts match
    const named = d.names.length >= samples.length ? d.names.slice(d.names.length - samples.length) : [];
    samples.forEach((s, i) => {
      const spacing = (d.spacing.length === samples.length ? d.spacing[i] : 25) / 100;
      const size = d.diam.length === samples.length ? d.diam[i] : Math.max(s.w, s.h);
      out.push({ name: named[i] || `${base(file)} ${i + 1}`, source: file, settings: imageTip(maskToTip(s.vals, s.w, s.h), size, spacing) });
    });
    if (!samples.length) notes.push(`${file}: no brush tip pictures found (it may only contain round brushes or patterns).`);
  } else {
    notes.push(`${file}: this .abr version (${version}) is not supported.`);
  }
  return { brushes: out, notes };
}

// ---------- GIMP / Krita .gbr and .gih ----------

export function parseGBR(buf: ArrayBuffer, file: string, offset = 0): { brush: ImportedBrush; end: number } | null {
  const v = new DataView(buf, offset);
  const headerSize = v.getUint32(0);
  const version = v.getUint32(4);
  const w = v.getUint32(8), h = v.getUint32(12), bytes = v.getUint32(16);
  if (!w || !h || w > 8000 || h > 8000 || (bytes !== 1 && bytes !== 4)) return null;
  let spacing = 25;
  let name = base(file);
  if (version >= 2) {
    spacing = v.getUint32(24);
    const nameBytes = new Uint8Array(buf, offset + 28, Math.max(0, headerSize - 28));
    const n = new TextDecoder().decode(nameBytes).replace(/\0.*$/, '').trim();
    if (n) name = n;
  }
  const data = new Uint8Array(buf, offset + headerSize, w * h * bytes);
  const vals = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i++) vals[i] = bytes === 1 ? data[i] : data[i * 4 + 3];
  return {
    brush: { name, source: file, settings: imageTip(maskToTip(vals, w, h), Math.max(w, h), spacing / 100) },
    end: offset + headerSize + w * h * bytes,
  };
}

function parseGIH(buf: ArrayBuffer, file: string): ImportedBrush | null {
  // two text lines (name, then parameters), then GIMP brushes; the first one is used
  const bytes = new Uint8Array(buf);
  let lines = 0, i = 0;
  while (i < bytes.length && lines < 2) if (bytes[i++] === 10) lines++;
  return parseGBR(buf, file, i)?.brush ?? null;
}

// ---------- Krita .kpp and .bundle ----------

async function inflate(data: Uint8Array): Promise<Uint8Array> {
  const ds = new DecompressionStream('deflate');
  const stream = new Blob([data.slice()]).stream().pipeThrough(ds);
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

// The text chunks inside a PNG (Krita keeps the preset there)
async function pngTexts(buf: ArrayBuffer): Promise<Record<string, string>> {
  const v = new DataView(buf);
  const out: Record<string, string> = {};
  let p = 8;
  const latin = new TextDecoder('latin1');
  const utf8 = new TextDecoder();
  while (p + 12 <= buf.byteLength) {
    const len = v.getUint32(p);
    const type = latin.decode(new Uint8Array(buf, p + 4, 4));
    const data = new Uint8Array(buf, p + 8, Math.min(len, buf.byteLength - p - 8));
    const z = data.indexOf(0);
    if (z > 0) {
      const key = latin.decode(data.subarray(0, z));
      if (type === 'tEXt') out[key] = latin.decode(data.subarray(z + 1));
      else if (type === 'zTXt') out[key] = utf8.decode(await inflate(data.subarray(z + 2)));
      else if (type === 'iTXt') {
        const compressed = data[z + 1] === 1;
        let q = z + 3;
        q = data.indexOf(0, q) + 1; // language
        q = data.indexOf(0, q) + 1; // translated keyword
        const rest = data.subarray(q);
        out[key] = utf8.decode(compressed ? await inflate(rest) : rest);
      }
    }
    if (type === 'IEND') break;
    p += 12 + len;
  }
  return out;
}

const b64bytes = (s: string) => Uint8Array.from(atob(s.replace(/\s+/g, '')), (c) => c.charCodeAt(0));

async function kppToBrush(buf: ArrayBuffer, file: string, files: Map<string, Uint8Array>): Promise<ImportedBrush | null> {
  const texts = await pngTexts(buf);
  const xml = texts['preset'];
  if (!xml) return null;
  const doc = new DOMParser().parseFromString(xml, 'application/xml');
  const root = doc.querySelector('Preset');
  if (!root) return null;
  const name = root.getAttribute('name') || base(file);
  const paintop = root.getAttribute('paintopid') ?? 'paintbrush';
  const param = (n: string) => doc.querySelector(`param[name="${n}"]`)?.textContent ?? null;
  const s: Partial<BrushSettings> = { tip: 'round' };

  const op = parseFloat(param('OpacityValue') ?? '');
  if (isFinite(op)) s.opacity = Math.max(0.01, Math.min(1, op));
  const flow = parseFloat(param('FlowValue') ?? '');
  if (isFinite(flow)) s.flow = Math.max(0.01, Math.min(1, flow));
  s.sizePressure = (param('PressureSize') ?? 'true') === 'true';
  s.opacityPressure = param('PressureOpacity') === 'true';
  if (paintop === 'colorsmudge') s.mix = 0.5;
  if (paintop === 'hairy') s.tip = 'bristle';
  if (paintop === 'spray') {
    s.tip = 'splatter';
    s.scatter = 0.5;
  }

  const def = param('brush_definition');
  if (def) {
    const bd = new DOMParser().parseFromString(def, 'application/xml').querySelector('Brush');
    if (bd) {
      const type = bd.getAttribute('type');
      const spacing = parseFloat(bd.getAttribute('spacing') ?? '0.1');
      if (isFinite(spacing)) s.spacing = Math.max(0.02, Math.min(2, spacing));
      let angle = parseFloat(bd.getAttribute('angle') ?? '0');
      if (Math.abs(angle) <= 6.3) angle = (angle * 180) / Math.PI;
      s.angle = Math.round(angle);
      if (type === 'auto_brush') {
        const m = bd.querySelector('MaskGenerator');
        const d = parseFloat(m?.getAttribute('diameter') ?? '20');
        s.size = Math.max(1, Math.min(500, d));
        s.roundness = Math.max(0.05, Math.min(1, parseFloat(m?.getAttribute('ratio') ?? '1')));
        const fade = parseFloat(m?.getAttribute('hfade') ?? '1');
        s.hardness = Math.max(0, Math.min(1, isFinite(fade) ? fade : 1));
      } else {
        // picture tip: in the bundle, or embedded in the preset
        const fname = (bd.getAttribute('filename') ?? '').replace(/^.*[\\/]/, '');
        const scale = parseFloat(bd.getAttribute('scale') ?? '1') || 1;
        let tip: { tip: string; w: number; h: number } | null = null;
        const bytes = files.get(fname) ?? embedded(doc, fname);
        if (bytes) {
          if (fname.toLowerCase().endsWith('.gbr') || fname.toLowerCase().endsWith('.gih')) {
            const g = fname.toLowerCase().endsWith('.gih') ? parseGIH(bytes.slice().buffer, fname) : parseGBR(bytes.slice().buffer, fname)?.brush;
            if (g?.settings.tipImage) tip = { tip: g.settings.tipImage, w: g.settings.size ?? 64, h: g.settings.size ?? 64 };
          } else {
            try {
              tip = await imageToTip(new Blob([bytes.slice()]));
            } catch {
              tip = null;
            }
          }
        }
        if (tip) Object.assign(s, imageTip(tip.tip, Math.max(tip.w, tip.h) * scale, s.spacing ?? 0.1), { angle: s.angle, opacity: s.opacity, flow: s.flow, sizePressure: s.sizePressure, opacityPressure: s.opacityPressure });
        else s.size = Math.max(1, Math.min(500, 30 * scale));
      }
    }
  }
  return { name, source: file, settings: s };
}

// Krita 5 can embed the tip inside the preset as base64
function embedded(doc: Document, fname: string): Uint8Array | null {
  for (const el of Array.from(doc.querySelectorAll('resource, resources > *'))) {
    const f = (el.getAttribute('filename') ?? el.getAttribute('name') ?? '').replace(/^.*[\\/]/, '');
    const text = el.textContent?.trim() ?? '';
    if (text.length > 64 && (!fname || f === fname)) {
      try {
        return b64bytes(text);
      } catch {
        // not base64
      }
    }
  }
  return null;
}

async function parseKrita(buf: ArrayBuffer, file: string): Promise<ImportResult> {
  if (ext(file) === 'kpp') {
    const b = await kppToBrush(buf, file, new Map());
    return b ? { brushes: [b], notes: [] } : { brushes: [], notes: [`${file}: no Krita brush settings found inside.`] };
  }
  // .bundle (zip)
  const zip = await JSZip.loadAsync(buf);
  const files = new Map<string, Uint8Array>();
  for (const f of Object.values(zip.files)) {
    if (f.dir) continue;
    if (/^brushes\//i.test(f.name) || /\.(gbr|gih|png)$/i.test(f.name)) files.set(f.name.replace(/^.*\//, ''), await f.async('uint8array'));
  }
  const brushes: ImportedBrush[] = [];
  for (const f of Object.values(zip.files)) {
    if (f.dir || !/\.kpp$/i.test(f.name)) continue;
    const b = await kppToBrush((await f.async('uint8array')).slice().buffer, f.name, files);
    if (b) brushes.push({ ...b, source: file });
  }
  // a bundle with only tips (no presets): each tip becomes a brush
  if (!brushes.length) {
    for (const [n, bytes] of files) {
      if (/\.gbr$/i.test(n)) {
        const g = parseGBR(bytes.slice().buffer, n);
        if (g) brushes.push({ ...g.brush, source: file });
      } else if (/\.png$/i.test(n)) {
        const t = await imageToTip(new Blob([bytes.slice()]));
        brushes.push({ name: base(n), source: file, settings: imageTip(t.tip, Math.max(t.w, t.h), 0.15) });
      }
    }
  }
  return { brushes, notes: brushes.length ? [] : [`${file}: no brushes found in this bundle.`] };
}

// ---------- Clip Studio Paint .sut ----------
// A .sut file is a small database. The tip picture is stored inside it in CSP's own way;
// when a normal PNG is found there it is used, otherwise the brush gets a round tip.

async function parseSUT(buf: ArrayBuffer, file: string): Promise<ImportResult> {
  // @ts-ignore sql.js has no type definitions
  const mod = await import('sql.js');
  // @ts-ignore the wasm file is served by Vite
  const wasm = (await import('sql.js/dist/sql-wasm-browser.wasm?url')).default as string;
  const initSqlJs = (mod.default ?? mod) as (o: unknown) => Promise<{ Database: new (d: Uint8Array) => SqlDb }>;
  const SQL = await initSqlJs({ locateFile: () => wasm });
  const db = new SQL.Database(new Uint8Array(buf));
  const notes: string[] = [];
  try {
    const rows = (sql: string) => {
      try {
        const r = db.exec(sql)[0];
        if (!r) return [] as Record<string, unknown>[];
        return r.values.map((v) => Object.fromEntries(r.columns.map((c, i) => [c, v[i]])));
      } catch {
        return [] as Record<string, unknown>[];
      }
    };
    const nodes = rows('SELECT * FROM Node');
    const variants = rows('SELECT * FROM Variant');
    // the tip picture: any PNG stored inside the material data
    let tip: { tip: string; w: number; h: number } | null = null;
    for (const m of rows('SELECT FileData FROM MaterialFile')) {
      const data = m.FileData as Uint8Array | null;
      if (!data) continue;
      const png = findPng(data);
      if (png) {
        try {
          tip = await imageToTip(new Blob([png.slice()], { type: 'image/png' }));
          break;
        } catch {
          // not readable
        }
      }
    }
    const num = (o: Record<string, unknown> | undefined, re: RegExp) => {
      if (!o) return NaN;
      const k = Object.keys(o).find((c) => re.test(c));
      const v = k ? Number(o[k]) : NaN;
      return isFinite(v) ? v : NaN;
    };
    const brushes: ImportedBrush[] = [];
    const named = nodes.filter((n) => typeof n.NodeName === 'string' && n.NodeName);
    for (const n of named.length ? named : [{ NodeName: base(file) }]) {
      const vid = n.NodeVariantID ?? n.NodeInitVariantID;
      const v = variants.find((x) => x.VariantID === vid) ?? variants[0];
      const s: Partial<BrushSettings> = { tip: 'round' };
      const size = num(v, /^BrushSize$/i);
      if (size > 0) s.size = Math.max(1, Math.min(500, size > 1000 ? size / 10 : size));
      const op = num(v, /^Opacity$/i);
      if (op > 0) s.opacity = Math.max(0.01, Math.min(1, op > 100 ? op / 256 : op > 1 ? op / 100 : op));
      const interval = num(v, /Interval$/i);
      if (interval > 0) s.spacing = Math.max(0.02, Math.min(2, interval > 5 ? interval / 100 : interval));
      const hard = num(v, /Hardness/i);
      if (hard >= 0) s.hardness = Math.max(0, Math.min(1, hard > 1 ? hard / 100 : hard));
      const thick = num(v, /Thickness$/i);
      if (thick > 0) s.roundness = Math.max(0.05, Math.min(1, thick > 1 ? thick / 100 : thick));
      const rot = num(v, /Rotation$/i);
      if (isFinite(rot)) s.angle = Math.round(rot);
      if (tip) Object.assign(s, imageTip(tip.tip, s.size ?? Math.max(tip.w, tip.h), s.spacing ?? 0.1), { opacity: s.opacity, angle: s.angle, roundness: s.roundness });
      brushes.push({ name: String(n.NodeName), source: file, settings: s });
    }
    if (!tip) notes.push(`${file}: the tip picture is stored in Clip Studio's own format and could not be read, so a round tip is used. Size, opacity and spacing were imported.`);
    return { brushes, notes };
  } finally {
    db.close();
  }
}

type SqlDb = { exec: (s: string) => { columns: string[]; values: unknown[][] }[]; close: () => void };

function findPng(data: Uint8Array): Uint8Array | null {
  const sig = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  for (let i = 0; i < data.length - 8; i++) {
    if (data[i] !== 0x89) continue;
    if (!sig.every((b, k) => data[i + k] === b)) continue;
    // find IEND
    for (let j = i + 8; j < data.length - 8; j++) {
      if (data[j] === 0x49 && data[j + 1] === 0x45 && data[j + 2] === 0x4e && data[j + 3] === 0x44) return data.subarray(i, j + 8);
    }
  }
  return null;
}

// ---------- SeviathStudio brush files ----------

export type BrushFile = { app: 'SeviathStudio'; type: 'brushes'; version: 1; brushes: { name: string; category?: string; about?: string; settings: Partial<BrushSettings> }[] };

export function brushFile(brushes: { name: string; about?: string; settings: Partial<BrushSettings> }[]): Blob {
  const f: BrushFile = { app: 'SeviathStudio', type: 'brushes', version: 1, brushes };
  return new Blob([JSON.stringify(f)], { type: 'application/json' });
}

function parseSeviath(text: string, file: string): ImportResult {
  const o = JSON.parse(text) as BrushFile;
  if (o.app !== 'SeviathStudio' || !Array.isArray(o.brushes)) throw new Error('not a SeviathStudio brush file');
  return { brushes: o.brushes.map((b) => ({ name: b.name, about: b.about, settings: b.settings, source: file })), notes: [] };
}

// ---------- Brush cards ----------
// A picture of the brush (its stroke, name and tip) that also carries the whole brush.
// Post it anywhere; whoever drops the picture into Import gets the brush.

const CRC = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(bytes: Uint8Array) {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) c = CRC[(c ^ bytes[i]) & 255] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function pngChunk(type: string, data: Uint8Array) {
  const out = new Uint8Array(12 + data.length);
  const v = new DataView(out.buffer);
  v.setUint32(0, data.length);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(data, 8);
  v.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
  return out;
}

// Adds a text chunk (UTF-8) to a PNG, just before its end
function withPngText(png: Uint8Array, key: string, text: string) {
  const k = new TextEncoder().encode(key);
  const body = new TextEncoder().encode(text);
  const data = new Uint8Array(k.length + 5 + body.length);
  data.set(k, 0); // keyword, then: \0, not compressed, method, no language, no translation
  data.set(body, k.length + 5);
  const chunk = pngChunk('iTXt', data);
  const iend = png.length - 12;
  const out = new Uint8Array(png.length + chunk.length);
  out.set(png.subarray(0, iend), 0);
  out.set(chunk, iend);
  out.set(png.subarray(iend), iend + chunk.length);
  return out;
}

export async function brushCard(name: string, about: string | undefined, settings: BrushSettings): Promise<Blob> {
  // image tips must be loaded before they can be painted
  await new Promise<void>((res) => {
    if (ensureTip(settings, () => res())) res();
    setTimeout(res, 1500);
  });
  const W = 640, H = 330;
  const c = document.createElement('canvas');
  c.width = W;
  c.height = H;
  const ctx = c.getContext('2d')!;
  const bg = ctx.createLinearGradient(0, 0, W, H);
  bg.addColorStop(0, '#202029');
  bg.addColorStop(1, '#14141a');
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, W, H);
  // the stroke on paper
  ctx.fillStyle = '#f3f1ec';
  ctx.beginPath();
  ctx.roundRect(20, 68, W - 40, 190, 14);
  ctx.fill();
  const pv = document.createElement('canvas');
  pv.width = W - 60;
  pv.height = 170;
  renderBrushPreview(pv, settings, '#1d1d24');
  ctx.drawImage(pv, 30, 78);
  // the tip, top right
  ctx.fillStyle = '#0d0d12';
  ctx.beginPath();
  ctx.roundRect(W - 76, 10, 52, 52, 8);
  ctx.fill();
  ctx.drawImage(getTipMask(settings), W - 72, 14, 44, 44);
  ctx.fillStyle = '#fff';
  ctx.font = '600 26px system-ui, sans-serif';
  ctx.fillText(name.slice(0, 32), 24, 46, W - 120);
  ctx.fillStyle = '#9a9aa8';
  ctx.font = '13px system-ui, sans-serif';
  if (about) ctx.fillText(about.slice(0, 90), 24, 284, W - 48);
  ctx.fillStyle = '#00b5c8';
  ctx.fillText('SeviathStudio brush · drop this picture into Brushes → Import to get it', 24, 310, W - 48);
  const blob = await new Promise<Blob>((res, rej) => c.toBlob((b) => (b ? res(b) : rej(new Error('Could not make the picture'))), 'image/png'));
  const json = JSON.stringify({ app: 'SeviathStudio', type: 'brushes', version: 1, brushes: [{ name, about, settings }] } satisfies BrushFile);
  const png = withPngText(new Uint8Array(await blob.arrayBuffer()), 'seviathbrush', json);
  return new Blob([png.slice()], { type: 'image/png' });
}

// ---------- One entry point ----------

export const BRUSH_ACCEPT = '.abr,.kpp,.bundle,.gbr,.gih,.sut,.seviathbrush,.json,.png,.jpg,.jpeg,.webp';

export async function importBrushFiles(files: File[]): Promise<ImportResult> {
  const all: ImportResult = { brushes: [], notes: [] };
  for (const f of files) {
    try {
      const e = ext(f.name);
      let r: ImportResult;
      if (e === 'abr') r = parseABR(await f.arrayBuffer(), f.name);
      else if (e === 'kpp' || e === 'bundle') r = await parseKrita(await f.arrayBuffer(), f.name);
      else if (e === 'gbr') {
        const g = parseGBR(await f.arrayBuffer(), f.name);
        r = { brushes: g ? [g.brush] : [], notes: g ? [] : [`${f.name}: not a GIMP brush.`] };
      } else if (e === 'gih') {
        const g = parseGIH(await f.arrayBuffer(), f.name);
        r = { brushes: g ? [g] : [], notes: g ? [] : [`${f.name}: not a GIMP brush pipe.`] };
      } else if (e === 'sut') r = await parseSUT(await f.arrayBuffer(), f.name);
      else if (e === 'seviathbrush' || e === 'json') r = parseSeviath(await f.text(), f.name);
      else if (['png', 'jpg', 'jpeg', 'webp'].includes(e)) {
        // a brush card carries the whole brush
        const card = e === 'png' ? (await pngTexts(await f.arrayBuffer()))['seviathbrush'] : undefined;
        if (card) {
          r = parseSeviath(card, f.name);
          all.brushes.push(...r.brushes);
          continue;
        }
        const t = await imageToTip(f);
        r = { brushes: [{ name: base(f.name), source: f.name, settings: imageTip(t.tip, Math.min(120, Math.max(t.w, t.h)), 0.15) }], notes: [] };
      } else r = { brushes: [], notes: [`${f.name}: unknown brush file type.`] };
      all.brushes.push(...r.brushes);
      all.notes.push(...r.notes);
    } catch (err) {
      all.notes.push(`${f.name}: could not be read (${err instanceof Error ? err.message : err}).`);
    }
  }
  return all;
}
