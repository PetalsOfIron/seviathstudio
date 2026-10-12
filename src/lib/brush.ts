// Brush settings, presets and the brush tip ("dab") used for painting

export type BrushSettings = {
  size: number; // diameter in pixels
  opacity: number; // 0..1, the most a single stroke can cover
  flow: number; // 0..1, how much paint each dab adds (low = airbrush build-up)
  hardness: number; // 0..1, 1 = sharp edge, 0 = very soft edge
  spacing: number; // distance between dabs, as a fraction of the size
  sizePressure: boolean; // pen pressure changes the size
  minSize: number; // 0..1, size at the lightest pressure (1 = no change)
  texture: number; // 0..1, paper grain strength (fixed to the canvas, like CSP)
  opacityPressure: boolean; // pen pressure changes how much paint is laid down
  stabilize: number; // 0..1, smooths shaky lines
  ends: 'round' | 'taper' | 'flat'; // shape of both ends of a line
  taper: number; // 0..1, how long the tapered tips are
  mix: number; // 0..1, color mixing: 0 = clean paint, 1 = only blends the colors already there
  mixRefer: 'all' | 'layer'; // mix with everything you see, or only with the active layer
};

export type BrushPreset = BrushSettings & { name: string };

export const PRESETS: BrushPreset[] = [
  // Ballpoint-like: almost even width, a little grain
  { name: 'Pen', size: 8, opacity: 0.95, flow: 0.9, hardness: 0.85, spacing: 0.06, sizePressure: true, minSize: 0.65, opacityPressure: true, texture: 0.18, stabilize: 0.3, ends: 'taper', taper: 0.25, mix: 0, mixRefer: 'all' },
  // Brush ink: razor-sharp edge, thin-to-thick with pressure, no grain
  { name: 'Ink', size: 4, opacity: 1, flow: 1, hardness: 1, spacing: 0.04, sizePressure: true, minSize: 0.25, opacityPressure: false, texture: 0, stabilize: 0.55, ends: 'taper', taper: 0.5, mix: 0, mixRefer: 'all' },
  { name: 'Pencil', size: 6, opacity: 1, flow: 1, hardness: 0.7, spacing: 0.06, sizePressure: true, minSize: 0.5, opacityPressure: true, texture: 0.45, stabilize: 0.1, ends: 'round', taper: 0.3, mix: 0, mixRefer: 'all' },
  { name: 'Marker', size: 30, opacity: 0.5, flow: 1, hardness: 0.85, spacing: 0.06, sizePressure: false, minSize: 1, opacityPressure: false, texture: 0, stabilize: 0.2, ends: 'flat', taper: 0.3, mix: 0, mixRefer: 'all' },
  { name: 'Soft round', size: 60, opacity: 1, flow: 0.35, hardness: 0.2, spacing: 0.08, sizePressure: true, minSize: 0.2, opacityPressure: true, texture: 0, stabilize: 0, ends: 'round', taper: 0.3, mix: 0, mixRefer: 'all' },
  // Picks up the colors under it and smears them together for soft transitions
  { name: 'Blender', size: 50, opacity: 1, flow: 0.6, hardness: 0.3, spacing: 0.08, sizePressure: true, minSize: 0.4, opacityPressure: true, texture: 0, stabilize: 0.1, ends: 'round', taper: 0.3, mix: 1, mixRefer: 'all' },
  // Your color mixes with the paint already on the canvas, like wet paint
  { name: 'Paint mix', size: 40, opacity: 1, flow: 0.8, hardness: 0.5, spacing: 0.08, sizePressure: true, minSize: 0.4, opacityPressure: true, texture: 0, stabilize: 0.1, ends: 'round', taper: 0.3, mix: 0.5, mixRefer: 'all' },
  { name: 'Airbrush', size: 150, opacity: 1, flow: 0.06, hardness: 0, spacing: 0.06, sizePressure: false, minSize: 1, opacityPressure: true, texture: 0, stabilize: 0, ends: 'round', taper: 0.3, mix: 0, mixRefer: 'all' },
];

// The eraser keeps its own settings, separate from the brush
export const ERASER_PRESETS: BrushPreset[] = [
  // Removes everything it touches, completely
  { name: 'Hard eraser', size: 20, opacity: 1, flow: 1, hardness: 0.9, spacing: 0.05, sizePressure: true, minSize: 0.5, opacityPressure: false, texture: 0, stabilize: 0, ends: 'round', taper: 0.3, mix: 0, mixRefer: 'all' },
  // Fades paint out gently; press harder or go over again to erase more
  { name: 'Soft eraser', size: 80, opacity: 1, flow: 0.3, hardness: 0.1, spacing: 0.06, sizePressure: true, minSize: 0.4, opacityPressure: true, texture: 0, stabilize: 0, ends: 'round', taper: 0.3, mix: 0, mixRefer: 'all' },
  // Grainy, like a real rubber on pencil
  { name: 'Textured eraser', size: 30, opacity: 1, flow: 1, hardness: 0.7, spacing: 0.06, sizePressure: true, minSize: 0.5, opacityPressure: true, texture: 0.45, stabilize: 0, ends: 'round', taper: 0.3, mix: 0, mixRefer: 'all' },
];

export function settingsOf(p: BrushPreset): BrushSettings {
  const { name, ...settings } = p;
  return { ...settings };
}

// ---------- Brush tip ----------
// A round tip is drawn once into a small canvas and then stamped (scaled) along the stroke.

const DAB_SIZE = 256;
const cache = new Map<string, HTMLCanvasElement>();

export function getDab(color: string, hardness: number): HTMLCanvasElement {
  const key = color + '|' + hardness.toFixed(2);
  const cached = cache.get(key);
  if (cached) return cached;
  if (cache.size > 40) cache.clear();

  const dab = document.createElement('canvas');
  dab.width = DAB_SIZE;
  dab.height = DAB_SIZE;
  const ctx = dab.getContext('2d')!;
  const c = DAB_SIZE / 2;
  const n = parseInt(color.replace('#', ''), 16) || 0;
  const rgb = `${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}`;
  const h = Math.min(Math.max(hardness, 0), 0.97); // keep a tiny soft edge so lines are anti-aliased

  const g = ctx.createRadialGradient(c, c, 0, c, c, c);
  g.addColorStop(0, `rgba(${rgb}, 1)`);
  g.addColorStop(h, `rgba(${rgb}, 1)`);
  g.addColorStop(h + (1 - h) * 0.5, `rgba(${rgb}, 0.35)`);
  g.addColorStop(1, `rgba(${rgb}, 0)`);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, DAB_SIZE, DAB_SIZE);

  cache.set(key, dab);
  return dab;
}

// ---------- Paper texture ----------
// A tileable grain pattern. Strokes are "worn away" by it, and because the pattern is
// fixed to the canvas, the grain stays in place no matter where or how you draw.

let paper: HTMLCanvasElement | null = null;

export function getPaper(): HTMLCanvasElement {
  if (paper) return paper;
  const SIZE = 256;

  // 1. One tile of noise: coarse blocks (paper bumps) + fine per-pixel grain
  const tile = document.createElement('canvas');
  tile.width = SIZE;
  tile.height = SIZE;
  const tctx = tile.getContext('2d')!;
  tctx.imageSmoothingEnabled = false;
  const noise = (cells: number, alpha: number) => {
    const small = document.createElement('canvas');
    small.width = cells;
    small.height = cells;
    const sctx = small.getContext('2d')!;
    const img = sctx.createImageData(cells, cells);
    for (let i = 0; i < img.data.length; i += 4) {
      img.data[i + 3] = Math.pow(Math.random(), 3) * 255; // black with random transparency
    }
    sctx.putImageData(img, 0, 0);
    tctx.globalAlpha = alpha;
    tctx.drawImage(small, 0, 0, SIZE, SIZE);
  };
  noise(64, 0.65);
  noise(256, 0.8);

  // 2. Blur a 3x3 copy of the tile and keep the middle one, so the edges wrap seamlessly
  const big = document.createElement('canvas');
  big.width = SIZE * 3;
  big.height = SIZE * 3;
  const bctx = big.getContext('2d')!;
  for (let y = 0; y < 3; y++) for (let x = 0; x < 3; x++) bctx.drawImage(tile, x * SIZE, y * SIZE);

  const out = document.createElement('canvas');
  out.width = SIZE;
  out.height = SIZE;
  const octx = out.getContext('2d')!;
  octx.filter = 'blur(1.2px)';
  octx.drawImage(big, -SIZE, -SIZE);
  octx.filter = 'none';

  paper = out;
  return out;
}
