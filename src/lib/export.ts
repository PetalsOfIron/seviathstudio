// Export to every file type: flat pictures (PNG, JPG, WebP, BMP, TIFF, GIF, ICO, SVG)
// and layered files (PSD, OpenRaster, SeviathStudio project).
// @ts-ignore gifenc ships without type definitions
import { GIFEncoder, quantize, applyPalette } from 'gifenc';
import { download, canvasToBlob, exportPSD, exportORA, saveProject, pngWithDpi, jpgWithDpi, type ProjectData } from './fileio';
import { applyPrintPreview, toCmykBytes } from './color';

export type ExportFormat = 'png' | 'jpg' | 'webp' | 'bmp' | 'tiff' | 'gif' | 'ico' | 'svg' | 'psd' | 'ora' | 'seviath';

export type ExportOptions = {
  format: ExportFormat;
  scale: number; // 0.1..2, flat pictures only
  quality: number; // 0..1, JPG and WebP
  transparent: boolean; // leave the paper out (formats with transparency)
  cmyk: boolean; // TIFF: write real CMYK ink values (print projects)
  printColors: boolean; // CMYK projects: save colors as they will print (what the canvas shows)
};

export const FORMATS: { id: ExportFormat; label: string; note: string; layers?: boolean; alpha?: boolean; lossy?: boolean }[] = [
  { id: 'psd', label: 'PSD', note: 'Photoshop, CSP, Krita, Procreate. Keeps layers, blend modes, clipping.', layers: true },
  { id: 'ora', label: 'ORA', note: 'OpenRaster: Krita, GIMP, MyPaint open it with all layers.', layers: true },
  { id: 'seviath', label: 'SEVIATH', note: 'SeviathStudio project file (backup or move to another computer).', layers: true },
  { id: 'png', label: 'PNG', note: 'Sharp, keeps transparency. Best for most uses.', alpha: true },
  { id: 'jpg', label: 'JPG', note: 'Small file for photos and posting. No transparency.', lossy: true },
  { id: 'webp', label: 'WebP', note: 'Small file for the web, keeps transparency.', alpha: true, lossy: true },
  { id: 'gif', label: 'GIF', note: '256 colors max. Simple transparency.', alpha: true },
  { id: 'bmp', label: 'BMP', note: 'Old Windows format, no compression. No transparency.' },
  { id: 'tiff', label: 'TIFF', note: 'For print shops. No compression. Can be CMYK in print projects.', alpha: true },
  { id: 'ico', label: 'ICO', note: 'Windows / website icon (16, 32, 48 and 256 px, square).', alpha: true },
  { id: 'svg', label: 'SVG', note: 'The picture inside an SVG file (not vector lines).', alpha: true },
];

// ---------- Helpers ----------

function scaled(src: HTMLCanvasElement, scale: number) {
  if (Math.abs(scale - 1) < 1e-6) return src;
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.round(src.width * scale));
  c.height = Math.max(1, Math.round(src.height * scale));
  const ctx = c.getContext('2d')!;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(src, 0, 0, c.width, c.height);
  return c;
}

// Puts a solid background behind the picture (for formats without transparency)
function flatten(src: HTMLCanvasElement, bg: string) {
  const c = document.createElement('canvas');
  c.width = src.width;
  c.height = src.height;
  const ctx = c.getContext('2d')!;
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, c.width, c.height);
  ctx.drawImage(src, 0, 0);
  return c;
}

const pixels = (c: HTMLCanvasElement) => c.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, c.width, c.height).data;

// ---------- BMP (24-bit) ----------

export function encodeBMP(c: HTMLCanvasElement, dpi = 72): Blob {
  const w = c.width, h = c.height;
  const row = Math.ceil((w * 3) / 4) * 4;
  const size = 54 + row * h;
  const buf = new ArrayBuffer(size);
  const v = new DataView(buf);
  v.setUint8(0, 0x42); v.setUint8(1, 0x4d); // "BM"
  v.setUint32(2, size, true);
  v.setUint32(10, 54, true);
  v.setUint32(14, 40, true);
  v.setInt32(18, w, true);
  v.setInt32(22, h, true); // bottom-up
  v.setUint16(26, 1, true);
  v.setUint16(28, 24, true);
  v.setUint32(34, row * h, true);
  v.setInt32(38, Math.round(dpi / 0.0254), true); // pixels per meter
  v.setInt32(42, Math.round(dpi / 0.0254), true);
  const d = pixels(c);
  const out = new Uint8Array(buf);
  for (let y = 0; y < h; y++) {
    let o = 54 + (h - 1 - y) * row;
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      out[o++] = d[i + 2];
      out[o++] = d[i + 1];
      out[o++] = d[i];
    }
  }
  return new Blob([buf], { type: 'image/bmp' });
}

// ---------- TIFF (uncompressed; RGB or CMYK, with or without transparency) ----------

export function encodeTIFF(c: HTMLCanvasElement, dpi = 72, cmyk = false, alpha = true): Blob {
  const w = c.width, h = c.height;
  const rgba = pixels(c);
  let body: Uint8Array;
  let samples: number;
  if (cmyk) {
    body = toCmykBytes(rgba, alpha);
    samples = alpha ? 5 : 4;
  } else if (alpha) {
    body = new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.length);
    samples = 4;
  } else {
    body = new Uint8Array(w * h * 3);
    for (let i = 0, o = 0; i < rgba.length; i += 4) {
      body[o++] = rgba[i];
      body[o++] = rgba[i + 1];
      body[o++] = rgba[i + 2];
    }
    samples = 3;
  }
  const SHORT = 3, LONG = 4, RATIONAL = 5;
  const tags: [number, number, number, number][] = []; // tag, type, count, value or offset
  const extra = samples - (cmyk ? 4 : 3); // 1 if there is an alpha sample
  const nTags = 12 + (extra ? 1 : 0) + (cmyk ? 1 : 0);
  const ifd = 8;
  const bps = ifd + 2 + nTags * 12 + 4; // bits-per-sample list
  const res = bps + samples * 2; // two resolution fractions
  const data = res + 16;
  tags.push([256, LONG, 1, w]);
  tags.push([257, LONG, 1, h]);
  tags.push([258, SHORT, samples, samples <= 2 ? 8 : bps]);
  tags.push([259, SHORT, 1, 1]); // no compression
  tags.push([262, SHORT, 1, cmyk ? 5 : 2]); // 5 = separated (CMYK), 2 = RGB
  tags.push([273, LONG, 1, data]);
  tags.push([277, SHORT, 1, samples]);
  tags.push([278, LONG, 1, h]);
  tags.push([279, LONG, 1, body.length]);
  tags.push([282, RATIONAL, 1, res]);
  tags.push([283, RATIONAL, 1, res + 8]);
  tags.push([296, SHORT, 1, 2]); // inches
  if (cmyk) tags.push([332, SHORT, 1, 1]); // ink set = CMYK
  if (extra) tags.push([338, SHORT, 1, 2]); // extra sample = transparency
  tags.sort((a, b) => a[0] - b[0]);
  const buf = new ArrayBuffer(data + body.length);
  const v = new DataView(buf);
  v.setUint16(0, 0x4949); // "II": little endian
  v.setUint16(2, 42, true);
  v.setUint32(4, ifd, true);
  v.setUint16(ifd, nTags, true);
  tags.forEach(([tag, type, count, value], i) => {
    const o = ifd + 2 + i * 12;
    v.setUint16(o, tag, true);
    v.setUint16(o + 2, type, true);
    v.setUint32(o + 4, count, true);
    if (type === SHORT && count === 1) v.setUint16(o + 8, value, true);
    else v.setUint32(o + 8, value, true);
  });
  v.setUint32(ifd + 2 + nTags * 12, 0, true); // no next image
  for (let i = 0; i < samples; i++) v.setUint16(bps + i * 2, 8, true);
  v.setUint32(res, Math.round(dpi), true);
  v.setUint32(res + 4, 1, true);
  v.setUint32(res + 8, Math.round(dpi), true);
  v.setUint32(res + 12, 1, true);
  new Uint8Array(buf, data).set(body);
  return new Blob([buf], { type: 'image/tiff' });
}

// ---------- GIF (256 colors) ----------

export function encodeGIF(c: HTMLCanvasElement, transparent: boolean): Blob {
  const d = pixels(c);
  const format = transparent ? 'rgba4444' : 'rgb565';
  const palette = quantize(d, 256, { format, oneBitAlpha: transparent });
  const index = applyPalette(d, palette, format);
  const gif = GIFEncoder();
  let transparentIndex = 0;
  if (transparent) {
    transparentIndex = palette.findIndex((p: number[]) => p[3] === 0);
  }
  gif.writeFrame(index, c.width, c.height, {
    palette,
    transparent: transparent && transparentIndex >= 0,
    transparentIndex: Math.max(0, transparentIndex),
  });
  gif.finish();
  return new Blob([gif.bytes()], { type: 'image/gif' });
}

// ---------- ICO (PNG images inside) ----------

export async function encodeICO(c: HTMLCanvasElement): Promise<Blob> {
  const sizes = [16, 32, 48, 256];
  const pngs: Uint8Array[] = [];
  for (const s of sizes) {
    const t = document.createElement('canvas');
    t.width = s;
    t.height = s;
    const ctx = t.getContext('2d')!;
    ctx.imageSmoothingQuality = 'high';
    // fit the picture into a square, centered
    const k = Math.min(s / c.width, s / c.height);
    const w = c.width * k, h = c.height * k;
    ctx.drawImage(c, (s - w) / 2, (s - h) / 2, w, h);
    pngs.push(new Uint8Array(await (await canvasToBlob(t)).arrayBuffer()));
  }
  const head = 6 + 16 * sizes.length;
  const total = head + pngs.reduce((a, p) => a + p.length, 0);
  const buf = new Uint8Array(total);
  const v = new DataView(buf.buffer);
  v.setUint16(2, 1, true); // icon
  v.setUint16(4, sizes.length, true);
  let offset = head;
  sizes.forEach((s, i) => {
    const o = 6 + i * 16;
    buf[o] = s >= 256 ? 0 : s;
    buf[o + 1] = s >= 256 ? 0 : s;
    v.setUint16(o + 4, 1, true); // planes
    v.setUint16(o + 6, 32, true); // bits
    v.setUint32(o + 8, pngs[i].length, true);
    v.setUint32(o + 12, offset, true);
    buf.set(pngs[i], offset);
    offset += pngs[i].length;
  });
  return new Blob([buf], { type: 'image/x-icon' });
}

// ---------- SVG (picture inside) ----------

export async function encodeSVG(c: HTMLCanvasElement): Promise<Blob> {
  const url = c.toDataURL('image/png');
  const svg = `<?xml version="1.0" encoding="UTF-8"?>\n<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="${c.width}" height="${c.height}" viewBox="0 0 ${c.width} ${c.height}"><image width="${c.width}" height="${c.height}" href="${url}" xlink:href="${url}"/></svg>`;
  return new Blob([svg], { type: 'image/svg+xml' });
}

// ---------- One entry point ----------

// `picture(withPaper)` gives the whole drawing as one image
export async function exportAs(
  opts: ExportOptions,
  data: ProjectData,
  picture: (withPaper: boolean) => HTMLCanvasElement,
  name: string,
) {
  const f = opts.format;
  if (f === 'psd') return exportPSD(data, picture(true), `${name}.psd`);
  if (f === 'ora') return exportORA(data, picture(true), `${name}.ora`);
  if (f === 'seviath') return saveProject(data, `${name}.seviath`);

  const info = FORMATS.find((x) => x.id === f)!;
  const keepAlpha = !!info.alpha && opts.transparent;
  const dpi = (data.dpi ?? 72) * opts.scale;
  const isCmyk = data.colorMode === 'cmyk';
  let pic = scaled(picture(!keepAlpha), opts.scale);
  if (!info.alpha) pic = flatten(pic, data.paper.exists ? data.paper.color : '#ffffff');
  // print projects: save the colors as they will look printed (CMYK TIFF converts itself)
  if (isCmyk && opts.printColors && !(f === 'tiff' && opts.cmyk)) {
    const ctx = pic.getContext('2d', { willReadFrequently: true })!;
    const img = ctx.getImageData(0, 0, pic.width, pic.height);
    applyPrintPreview(img.data);
    ctx.putImageData(img, 0, 0);
  }

  let blob: Blob;
  switch (f) {
    case 'png':
      blob = await pngWithDpi(await canvasToBlob(pic, 'image/png'), dpi);
      break;
    case 'jpg':
      blob = await jpgWithDpi(await canvasToBlob(pic, 'image/jpeg', opts.quality), dpi);
      break;
    case 'webp':
      blob = await canvasToBlob(pic, 'image/webp', opts.quality);
      if (blob.type !== 'image/webp') throw new Error('This browser cannot make WebP files. Use PNG or JPG.');
      break;
    case 'bmp':
      blob = encodeBMP(pic, dpi);
      break;
    case 'tiff':
      blob = encodeTIFF(pic, dpi, isCmyk && opts.cmyk, keepAlpha);
      break;
    case 'gif':
      blob = encodeGIF(pic, keepAlpha);
      break;
    case 'ico':
      blob = await encodeICO(pic);
      break;
    case 'svg':
      blob = await encodeSVG(pic);
      break;
    default:
      throw new Error('Unknown format');
  }
  download(blob, `${name}.${f}`);
}
