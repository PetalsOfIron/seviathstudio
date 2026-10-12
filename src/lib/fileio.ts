// Saving, opening, and exporting files (SeviathStudio project, PNG, PSD)
import { readPsd, writePsd, type Psd, type Layer as PsdLayer, type BlendMode as PsdBlend } from 'ag-psd';
import JSZip from 'jszip';
import type { VecLayer } from './vector';
import type { ColorMode } from './color';

export type SavedLayer = {
  name: string;
  visible: boolean;
  opacity: number;
  blend: string;
  clip: boolean;
  lockAlpha: boolean;
  canvas: HTMLCanvasElement;
  vector?: VecLayer; // vector layers keep their lines; `canvas` is the drawn picture of them
};

export type ProjectData = {
  width: number;
  height: number;
  paper: { exists: boolean; visible: boolean; color: string };
  layers: SavedLayer[]; // bottom first
  dpi?: number; // print resolution (pixels per inch), 72 if not set
  colorMode?: ColorMode; // 'rgb' (screen) or 'cmyk' (print)
};

// ---------- Downloads ----------

export function download(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}

export function canvasToBlob(canvas: HTMLCanvasElement, type = 'image/png', quality?: number): Promise<Blob> {
  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('Could not encode image'))), type, quality),
  );
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error('Could not load image'));
    img.src = src;
  });
}

// ---------- SeviathStudio project (.seviath) ----------
// A JSON file: document settings plus every layer stored as a PNG image.

export async function saveProject(data: ProjectData, filename: string) {
  const layers = data.layers.map((l) => ({
    name: l.name,
    visible: l.visible,
    opacity: l.opacity,
    blend: l.blend,
    clip: l.clip,
    lockAlpha: l.lockAlpha,
    png: l.canvas.toDataURL('image/png'),
    ...(l.vector ? { vector: l.vector } : {}),
  }));
  const json = JSON.stringify({
    app: 'SeviathStudio',
    version: 2,
    width: data.width,
    height: data.height,
    dpi: data.dpi ?? 72,
    colorMode: data.colorMode ?? 'rgb',
    paper: data.paper,
    layers,
  });
  download(new Blob([json], { type: 'application/json' }), filename);
}

export async function openProject(file: File): Promise<ProjectData> {
  const obj = JSON.parse(await file.text());
  if (obj.app !== 'SeviathStudio') throw new Error('This is not a SeviathStudio project file.');
  const layers: SavedLayer[] = [];
  for (const l of obj.layers) {
    const canvas = document.createElement('canvas');
    canvas.width = obj.width;
    canvas.height = obj.height;
    canvas.getContext('2d')!.drawImage(await loadImage(l.png), 0, 0);
    layers.push({ name: l.name, visible: l.visible, opacity: l.opacity, blend: l.blend, clip: l.clip, lockAlpha: l.lockAlpha, canvas, vector: l.vector });
  }
  return { width: obj.width, height: obj.height, paper: obj.paper, layers, dpi: obj.dpi ?? 72, colorMode: obj.colorMode ?? 'rgb' };
}

// ---------- PNG ----------

export async function exportPNG(canvas: HTMLCanvasElement, filename: string, dpi = 72) {
  download(await pngWithDpi(await canvasToBlob(canvas), dpi), filename);
}

// ---------- Print resolution inside PNG and JPG files ----------

const CRC_TABLE = (() => {
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
  for (let i = 0; i < bytes.length; i++) c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// Adds a pHYs chunk right after the header, so programs know the DPI
export async function pngWithDpi(png: Blob, dpi: number): Promise<Blob> {
  const src = new Uint8Array(await png.arrayBuffer());
  const ppm = Math.round(dpi / 0.0254);
  const chunk = new Uint8Array(21);
  const v = new DataView(chunk.buffer);
  v.setUint32(0, 9);
  chunk.set([0x70, 0x48, 0x59, 0x73], 4); // "pHYs"
  v.setUint32(8, ppm);
  v.setUint32(12, ppm);
  chunk[16] = 1; // meters
  v.setUint32(17, crc32(chunk.subarray(4, 17)));
  const at = 8 + 25; // signature + IHDR chunk
  const out = new Uint8Array(src.length + chunk.length);
  out.set(src.subarray(0, at));
  out.set(chunk, at);
  out.set(src.subarray(at), at + chunk.length);
  return new Blob([out], { type: 'image/png' });
}

// Writes the DPI into the JFIF header the browser made
export async function jpgWithDpi(jpg: Blob, dpi: number): Promise<Blob> {
  const b = new Uint8Array(await jpg.arrayBuffer());
  // FF D8, FF E0, length, "JFIF\0", version, units, x density, y density
  if (b[2] === 0xff && b[3] === 0xe0 && b[6] === 0x4a && b[7] === 0x46 && b[8] === 0x49 && b[9] === 0x46) {
    const d = Math.min(65535, Math.round(dpi));
    b[13] = 1; // dots per inch
    b[14] = d >> 8;
    b[15] = d & 255;
    b[16] = d >> 8;
    b[17] = d & 255;
  }
  return new Blob([b], { type: 'image/jpeg' });
}

// ---------- PSD ----------

const TO_PSD: Record<string, PsdBlend> = {
  Normal: 'normal',
  Multiply: 'multiply',
  Screen: 'screen',
  Overlay: 'overlay',
  Add: 'linear dodge',
  'Soft light': 'soft light',
  'Hard light': 'hard light',
  'Color dodge': 'color dodge',
  'Color burn': 'color burn',
  Darken: 'darken',
  Lighten: 'lighten',
  Difference: 'difference',
  Exclusion: 'exclusion',
  Hue: 'hue',
  Saturation: 'saturation',
  Color: 'color',
  Luminosity: 'luminosity',
};
const FROM_PSD: Record<string, string> = Object.fromEntries(Object.entries(TO_PSD).map(([k, v]) => [v, k]));

// Smallest rectangle that contains all non-transparent pixels (keeps PSD files small)
function contentBounds(canvas: HTMLCanvasElement) {
  const { width: w, height: h } = canvas;
  const data = canvas.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, w, h).data;
  let minX = w, minY = h, maxX = -1, maxY = -1;
  for (let y = 0; y < h; y++) {
    const row = y * w * 4;
    for (let x = 0; x < w; x++) {
      if (data[row + x * 4 + 3] !== 0) {
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
    }
  }
  return maxX < 0 ? null : { x: minX, y: minY, w: maxX - minX + 1, h: maxY - minY + 1 };
}

function crop(canvas: HTMLCanvasElement, r: { x: number; y: number; w: number; h: number }) {
  const c = document.createElement('canvas');
  c.width = r.w;
  c.height = r.h;
  c.getContext('2d')!.drawImage(canvas, -r.x, -r.y);
  return c;
}

// Every layer becomes a separate PSD layer with its name, visibility, opacity, blend mode,
// clipping mask and lock. Pixels are stored exactly as they are: no effects to break.
export function exportPSD(data: ProjectData, composite: HTMLCanvasElement, filename: string) {
  const children: PsdLayer[] = [];

  if (data.paper.exists) {
    const paper = document.createElement('canvas');
    paper.width = data.width;
    paper.height = data.height;
    const ctx = paper.getContext('2d')!;
    ctx.fillStyle = data.paper.color;
    ctx.fillRect(0, 0, data.width, data.height);
    children.push({ name: 'Paper', canvas: paper, left: 0, top: 0, hidden: !data.paper.visible });
  }

  for (const l of data.layers) {
    const b = contentBounds(l.canvas);
    const layer: PsdLayer = {
      name: l.name,
      hidden: !l.visible,
      opacity: l.opacity,
      blendMode: TO_PSD[l.blend] ?? 'normal',
      clipping: l.clip,
      transparencyProtected: l.lockAlpha,
      protected: { transparency: l.lockAlpha },
    };
    if (b) {
      layer.canvas = crop(l.canvas, b);
      layer.left = b.x;
      layer.top = b.y;
    }
    children.push(layer);
  }

  const dpi = data.dpi ?? 72;
  const psd: Psd = {
    width: data.width,
    height: data.height,
    canvas: composite,
    children,
    imageResources: {
      resolutionInfo: {
        horizontalResolution: dpi,
        horizontalResolutionUnit: 'PPI',
        widthUnit: 'Inches',
        verticalResolution: dpi,
        verticalResolutionUnit: 'PPI',
        heightUnit: 'Inches',
      },
    },
  };
  // noBackground: keep the bottom layer a normal layer (Photoshop would lock it as "Background")
  const buffer = writePsd(psd, { generateThumbnail: true, noBackground: true });
  download(new Blob([buffer], { type: 'image/vnd.adobe.photoshop' }), filename);
}

// Reads a PSD into layers. Groups are flattened into a plain list (group name is kept
// in front of the layer name); hidden groups make their layers hidden.
export async function importPSD(file: File): Promise<ProjectData> {
  const psd = readPsd(await file.arrayBuffer(), { skipThumbnail: true });
  const layers: SavedLayer[] = [];

  const walk = (items: PsdLayer[] | undefined, prefix: string, hiddenParent: boolean) => {
    for (const item of items ?? []) {
      const hidden = hiddenParent || !!item.hidden;
      if (item.children) {
        walk(item.children, prefix + (item.name ?? 'Group') + ' / ', hidden);
        continue;
      }
      const canvas = document.createElement('canvas');
      canvas.width = psd.width;
      canvas.height = psd.height;
      if (item.canvas) canvas.getContext('2d')!.drawImage(item.canvas, item.left ?? 0, item.top ?? 0);
      layers.push({
        name: prefix + (item.name ?? 'Layer'),
        visible: !hidden,
        opacity: item.opacity ?? 1,
        blend: FROM_PSD[item.blendMode ?? 'normal'] ?? 'Normal',
        clip: !!item.clipping,
        lockAlpha: !!(item.transparencyProtected || item.protected?.transparency),
        canvas,
      });
    }
  };
  walk(psd.children, '', false);

  // A bottom layer called "Paper" (as SeviathStudio exports it) becomes the paper again
  let paper = { exists: true, visible: false, color: '#ffffff' };
  if (layers.length && layers[0].name === 'Paper') {
    const bottom = layers.shift()!;
    const [r, g, b] = bottom.canvas.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, 1, 1).data;
    paper = { exists: true, visible: bottom.visible, color: '#' + [r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('') };
  }

  const res = psd.imageResources?.resolutionInfo;
  const dpi = res ? Math.round(res.horizontalResolutionUnit === 'PPCM' ? res.horizontalResolution * 2.54 : res.horizontalResolution) : 72;
  return { width: psd.width, height: psd.height, paper, layers, dpi };
}

// ---------- Images (PNG, JPG, WebP, GIF, BMP, SVG, AVIF, ICO) ----------

const IMAGE_EXT = ['png', 'jpg', 'jpeg', 'jfif', 'webp', 'gif', 'bmp', 'svg', 'avif', 'ico', 'apng'];

export const extOf = (name: string) => (name.match(/\.([^.]+)$/)?.[1] ?? '').toLowerCase();
export const baseName = (name: string) => name.replace(/\.[^.]+$/, '') || 'Untitled';

// Every file type the Open / Import window offers
export const ACCEPT = ['.seviath', '.psd', '.psb', '.ora', '.kra', ...IMAGE_EXT.map((e) => '.' + e), 'image/*'].join(',');

export function isImageFile(file: File) {
  return IMAGE_EXT.includes(extOf(file.name)) || (file.type.startsWith('image/') && !file.type.includes('photoshop'));
}

async function blobToCanvas(blob: Blob, fallbackW = 1000, fallbackH = 1000): Promise<HTMLCanvasElement> {
  const url = URL.createObjectURL(blob);
  try {
    const img = await loadImage(url);
    const canvas = document.createElement('canvas');
    // SVGs without a size report 0
    canvas.width = img.naturalWidth || fallbackW;
    canvas.height = img.naturalHeight || fallbackH;
    canvas.getContext('2d')!.drawImage(img, 0, 0, canvas.width, canvas.height);
    return canvas;
  } finally {
    URL.revokeObjectURL(url);
  }
}

function singleLayer(canvas: HTMLCanvasElement, name: string, paperVisible = true): ProjectData {
  return {
    width: canvas.width,
    height: canvas.height,
    paper: { exists: true, visible: paperVisible, color: '#ffffff' },
    layers: [{ name, visible: true, opacity: 1, blend: 'Normal', clip: false, lockAlpha: false, canvas }],
  };
}

export async function importImage(file: Blob, name: string): Promise<ProjectData> {
  let canvas: HTMLCanvasElement;
  try {
    canvas = await blobToCanvas(file);
  } catch {
    throw new Error(`"${name}" could not be read as an image. The file may be damaged or a type this browser can't show (TIFF and HEIC: export them as PNG or JPG first).`);
  }
  return singleLayer(canvas, name);
}

// ---------- OpenRaster (.ora): Krita, GIMP, MyPaint, Pinta… with all layers ----------

const FROM_ORA: Record<string, string> = {
  'svg:src-over': 'Normal',
  'svg:multiply': 'Multiply',
  'svg:screen': 'Screen',
  'svg:overlay': 'Overlay',
  'svg:plus': 'Add',
  'svg:soft-light': 'Soft light',
  'svg:hard-light': 'Hard light',
  'svg:color-dodge': 'Color dodge',
  'svg:color-burn': 'Color burn',
  'svg:darken': 'Darken',
  'svg:lighten': 'Lighten',
  'svg:difference': 'Difference',
  'svg:exclusion': 'Exclusion',
  'svg:hue': 'Hue',
  'svg:saturation': 'Saturation',
  'svg:color': 'Color',
  'svg:luminosity': 'Luminosity',
};
const TO_ORA: Record<string, string> = Object.fromEntries(Object.entries(FROM_ORA).map(([k, v]) => [v, k]));

export async function importORA(file: Blob): Promise<ProjectData> {
  const zip = await JSZip.loadAsync(file);
  const xmlText = await zip.file('stack.xml')?.async('string');
  if (!xmlText) throw new Error('This .ora file has no layer list (stack.xml).');
  const doc = new DOMParser().parseFromString(xmlText, 'application/xml');
  const image = doc.querySelector('image');
  if (!image) throw new Error('This .ora file is damaged.');
  const dpi = Math.round(parseFloat(image.getAttribute('xres') ?? '72')) || 72;
  const W = parseInt(image.getAttribute('w') ?? '0');
  const H = parseInt(image.getAttribute('h') ?? '0');
  if (!W || !H) throw new Error('This .ora file has no canvas size.');

  const topFirst: SavedLayer[] = [];
  const walk = async (stack: Element, prefix: string, hiddenParent: boolean, parentOpacity: number) => {
    for (const el of Array.from(stack.children)) {
      const hidden = hiddenParent || el.getAttribute('visibility') === 'hidden';
      const opacity = parentOpacity * parseFloat(el.getAttribute('opacity') ?? '1');
      const name = el.getAttribute('name') ?? (el.tagName === 'stack' ? 'Group' : 'Layer');
      if (el.tagName === 'stack') {
        await walk(el, prefix + name + ' / ', hidden, opacity);
      } else if (el.tagName === 'layer') {
        const src = el.getAttribute('src');
        const entry = src ? zip.file(src) : null;
        const canvas = document.createElement('canvas');
        canvas.width = W;
        canvas.height = H;
        if (entry) {
          const pic = await blobToCanvas(await entry.async('blob'));
          canvas.getContext('2d')!.drawImage(pic, parseFloat(el.getAttribute('x') ?? '0'), parseFloat(el.getAttribute('y') ?? '0'));
        }
        topFirst.push({
          name: prefix + name,
          visible: !hidden,
          opacity: Math.min(1, Math.max(0, opacity)),
          blend: FROM_ORA[el.getAttribute('composite-op') ?? 'svg:src-over'] ?? 'Normal',
          clip: false,
          lockAlpha: el.getAttribute('alpha-preserve') === 'true',
          canvas,
        });
      }
    }
  };
  const root = image.querySelector(':scope > stack');
  if (root) await walk(root, '', false, 1);
  const layers = topFirst.reverse();

  // Many programs save the white background as an ordinary bottom layer: keep it as a layer,
  // but turn our own paper off so it isn't doubled.
  return { width: W, height: H, paper: { exists: true, visible: false, color: '#ffffff' }, layers, dpi };
}

export async function exportORA(data: ProjectData, composite: HTMLCanvasElement, filename: string) {
  const zip = new JSZip();
  zip.file('mimetype', 'image/openraster', { compression: 'STORE' });
  const esc = (t: string) => t.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
  const items: string[] = [];
  const all = [...data.layers];
  // top first in stack.xml
  for (let i = all.length - 1; i >= 0; i--) {
    const l = all[i];
    const path = `data/layer${i}.png`;
    zip.file(path, await canvasToBlob(l.canvas));
    items.push(
      `<layer name="${esc(l.name)}" src="${path}" x="0" y="0" opacity="${l.opacity}" visibility="${l.visible ? 'visible' : 'hidden'}" composite-op="${TO_ORA[l.blend] ?? 'svg:src-over'}"${l.lockAlpha ? ' alpha-preserve="true"' : ''}/>`,
    );
  }
  if (data.paper.exists) {
    const paper = document.createElement('canvas');
    paper.width = data.width;
    paper.height = data.height;
    const ctx = paper.getContext('2d')!;
    ctx.fillStyle = data.paper.color;
    ctx.fillRect(0, 0, data.width, data.height);
    zip.file('data/paper.png', await canvasToBlob(paper));
    items.push(`<layer name="Paper" src="data/paper.png" x="0" y="0" opacity="1" visibility="${data.paper.visible ? 'visible' : 'hidden'}"/>`);
  }
  zip.file(
    'stack.xml',
    `<?xml version="1.0" encoding="UTF-8"?>\n<image version="0.0.5" w="${data.width}" h="${data.height}" xres="${data.dpi ?? 72}" yres="${data.dpi ?? 72}"><stack>${items.join('')}</stack></image>`,
  );
  zip.file('mergedimage.png', await canvasToBlob(composite));
  const s = Math.min(1, 256 / Math.max(data.width, data.height));
  const th = document.createElement('canvas');
  th.width = Math.max(1, Math.round(data.width * s));
  th.height = Math.max(1, Math.round(data.height * s));
  th.getContext('2d')!.drawImage(composite, 0, 0, th.width, th.height);
  zip.file('Thumbnails/thumbnail.png', await canvasToBlob(th));
  const blob = await zip.generateAsync({ type: 'blob', mimeType: 'image/openraster' });
  download(blob, filename);
}

// ---------- Krita (.kra) ----------
// Krita stores layers in its own tile format; the flattened picture inside is read as one layer.
// For all layers, save as .ora or .psd in Krita.

export async function importKRA(file: Blob, name: string): Promise<ProjectData> {
  const zip = await JSZip.loadAsync(file);
  const merged = zip.file('mergedimage.png') ?? zip.file('preview.png');
  if (!merged) throw new Error('This Krita file has no picture inside.');
  return singleLayer(await blobToCanvas(await merged.async('blob')), name, false);
}

// ---------- One entry point for every file type ----------

export type Opened = { data: ProjectData; name: string; note?: string };

const CLOSED: Record<string, string> = {
  clip: 'Clip Studio Paint',
  procreate: 'Procreate',
  sai: 'PaintTool SAI',
  sai2: 'PaintTool SAI 2',
  mdp: 'FireAlpaca / MediBang',
  xcf: 'GIMP',
  afphoto: 'Affinity Photo',
  tif: 'TIFF',
  tiff: 'TIFF',
  heic: 'HEIC',
};

export async function openAnyFile(file: File): Promise<Opened> {
  const ext = extOf(file.name);
  const name = baseName(file.name);
  if (ext === 'seviath' || (ext === 'json' && file.type.includes('json'))) return { data: await openProject(file), name };
  if (ext === 'psd' || ext === 'psb') return { data: await importPSD(file), name };
  if (ext === 'ora') return { data: await importORA(file), name };
  if (ext === 'kra' || ext === 'krz') {
    return { data: await importKRA(file, name), name, note: 'Krita file opened as one flat image. For separate layers, save it as .psd or .ora in Krita.' };
  }
  if (isImageFile(file)) return { data: await importImage(file, name), name };
  if (CLOSED[ext]) {
    const app = CLOSED[ext];
    throw new Error(
      ext === 'tif' || ext === 'tiff' || ext === 'heic'
        ? `${app} images can't be read by the browser. Save it as PNG or JPG first.`
        : `${app} files use a closed format. In ${app}, export as PSD (keeps layers) or PNG, then open that here.`,
    );
  }
  throw new Error(`SeviathStudio can't open .${ext || '?'} files.`);
}
