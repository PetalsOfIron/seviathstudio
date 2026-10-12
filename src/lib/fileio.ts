// Saving, opening, and exporting files (SeviathStudio project, PNG, PSD)
import { readPsd, writePsd, type Psd, type Layer as PsdLayer, type BlendMode as PsdBlend } from 'ag-psd';

export type SavedLayer = {
  name: string;
  visible: boolean;
  opacity: number;
  blend: string;
  clip: boolean;
  lockAlpha: boolean;
  canvas: HTMLCanvasElement;
};

export type ProjectData = {
  width: number;
  height: number;
  paper: { exists: boolean; visible: boolean; color: string };
  layers: SavedLayer[]; // bottom first
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

function canvasToBlob(canvas: HTMLCanvasElement, type = 'image/png'): Promise<Blob> {
  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('Could not encode image'))), type),
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
  }));
  const json = JSON.stringify({ app: 'SeviathStudio', version: 1, width: data.width, height: data.height, paper: data.paper, layers });
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
    layers.push({ name: l.name, visible: l.visible, opacity: l.opacity, blend: l.blend, clip: l.clip, lockAlpha: l.lockAlpha, canvas });
  }
  return { width: obj.width, height: obj.height, paper: obj.paper, layers };
}

// ---------- PNG ----------

export async function exportPNG(canvas: HTMLCanvasElement, filename: string) {
  download(await canvasToBlob(canvas), filename);
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

  const psd: Psd = { width: data.width, height: data.height, canvas: composite, children };
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

  return { width: psd.width, height: psd.height, paper, layers };
}
