// Gradients: the gradient tool (drag across the canvas) and painting with a gradient.
// Colors blend in OKLab by default, so a mix of two colors never turns muddy gray in the
// middle (blue → yellow goes through clean colors instead of gray).

import { rgbToOklab, oklabToRgb } from './color';

export type GradStop = { pos: number; color: string; alpha: number }; // pos 0..1, alpha 0..1
export type GradShape = 'linear' | 'radial' | 'angle' | 'diamond' | 'reflected';
export type GradRepeat = 'none' | 'repeat' | 'mirror';

export type GradientSettings = {
  stops: GradStop[];
  shape: GradShape;
  repeat: GradRepeat;
  smooth: boolean; // true = OKLab (clean colors), false = plain RGB like most programs
  dither: boolean; // tiny noise so smooth gradients don't show bands
  opacity: number;
};

export const SHAPES: { id: GradShape; label: string; icon: string }[] = [
  { id: 'linear', label: 'Linear', icon: '▭' },
  { id: 'radial', label: 'Radial', icon: '◉' },
  { id: 'angle', label: 'Angle', icon: '◔' },
  { id: 'diamond', label: 'Diamond', icon: '◇' },
  { id: 'reflected', label: 'Mirrored', icon: '⧓' },
];

export const PRESETS: { name: string; stops: GradStop[] }[] = [
  { name: 'Color → transparent', stops: [] }, // filled in from the current color
  { name: 'Color → second color', stops: [] }, // filled in from the two colors
  {
    name: 'Rainbow',
    stops: ['#ff3b30', '#ff9500', '#ffcc00', '#34c759', '#00c7be', '#007aff', '#af52de'].map((c, i, a) => ({ pos: i / (a.length - 1), color: c, alpha: 1 })),
  },
  { name: 'Sunset', stops: [{ pos: 0, color: '#2b1055', alpha: 1 }, { pos: 0.45, color: '#d53a9d', alpha: 1 }, { pos: 0.75, color: '#ff8a5c', alpha: 1 }, { pos: 1, color: '#ffd56b', alpha: 1 }] },
  { name: 'Sky', stops: [{ pos: 0, color: '#1e3c72', alpha: 1 }, { pos: 0.6, color: '#5fa8e8', alpha: 1 }, { pos: 1, color: '#e6f3ff', alpha: 1 }] },
  { name: 'Pastel', stops: [{ pos: 0, color: '#ffc8dd', alpha: 1 }, { pos: 0.5, color: '#cdb4db', alpha: 1 }, { pos: 1, color: '#a2d2ff', alpha: 1 }] },
  { name: 'Skin shadow', stops: [{ pos: 0, color: '#f2c6a8', alpha: 1 }, { pos: 1, color: '#b06a5b', alpha: 1 }] },
  { name: 'Hair shine', stops: [{ pos: 0, color: '#ffffff', alpha: 0 }, { pos: 0.5, color: '#ffffff', alpha: 0.85 }, { pos: 1, color: '#ffffff', alpha: 0 }] },
  { name: 'Fire', stops: [{ pos: 0, color: '#3d0000', alpha: 1 }, { pos: 0.4, color: '#e2241b', alpha: 1 }, { pos: 0.75, color: '#ff9e1b', alpha: 1 }, { pos: 1, color: '#fff3b0', alpha: 1 }] },
];

const hexRGB = (hex: string): [number, number, number] => {
  const n = parseInt(hex.replace('#', ''), 16) || 0;
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
export const rgbHex = (r: number, g: number, b: number) =>
  '#' + [r, g, b].map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, '0')).join('');

export function sortStops(stops: GradStop[]) {
  return [...stops].sort((a, b) => a.pos - b.pos);
}

// The color at t (0..1) as [r, g, b, a] with a 0..255
export function colorAt(stops: GradStop[], t: number, smooth: boolean): [number, number, number, number] {
  const s = sortStops(stops);
  if (!s.length) return [0, 0, 0, 0];
  if (t <= s[0].pos) return [...hexRGB(s[0].color), s[0].alpha * 255];
  const last = s[s.length - 1];
  if (t >= last.pos) return [...hexRGB(last.color), last.alpha * 255];
  let i = 0;
  while (i < s.length - 1 && s[i + 1].pos < t) i++;
  const a = s[i], b = s[i + 1];
  const u = b.pos > a.pos ? (t - a.pos) / (b.pos - a.pos) : 0;
  const ca = hexRGB(a.color), cb = hexRGB(b.color);
  const alpha = (a.alpha + (b.alpha - a.alpha) * u) * 255;
  if (!smooth) return [ca[0] + (cb[0] - ca[0]) * u, ca[1] + (cb[1] - ca[1]) * u, ca[2] + (cb[2] - ca[2]) * u, alpha];
  // a fully see-through stop takes the other stop's color, so fading out never turns gray/black
  const la = rgbToOklab(...(a.alpha === 0 ? cb : ca));
  const lb = rgbToOklab(...(b.alpha === 0 ? ca : cb));
  const rgb = oklabToRgb(la[0] + (lb[0] - la[0]) * u, la[1] + (lb[1] - la[1]) * u, la[2] + (lb[2] - la[2]) * u);
  return [rgb[0], rgb[1], rgb[2], alpha];
}

// 1024 precomputed colors: fast enough to color every pixel of a big canvas
export function makeLUT(g: Pick<GradientSettings, 'stops' | 'smooth'>): Uint8ClampedArray {
  const n = 1024;
  const lut = new Uint8ClampedArray(n * 4);
  for (let i = 0; i < n; i++) {
    const c = colorAt(g.stops, i / (n - 1), g.smooth);
    lut.set([c[0], c[1], c[2], c[3]], i * 4);
  }
  return lut;
}

// Maps a position to 0..1 using the repeat mode
export function wrapT(t: number, repeat: GradRepeat) {
  if (repeat === 'none') return Math.min(1, Math.max(0, t));
  if (repeat === 'repeat') return t - Math.floor(t);
  const m = t - 2 * Math.floor(t / 2); // 0..2
  return m <= 1 ? m : 2 - m;
}

// Draws the gradient from p0 to p1 into a canvas of size w × h. `scale` < 1 makes a quick
// small preview (the result is then drawn bigger).
export function renderGradient(
  w: number,
  h: number,
  p0: { x: number; y: number },
  p1: { x: number; y: number },
  g: GradientSettings,
  scale = 1,
): HTMLCanvasElement {
  const W = Math.max(1, Math.round(w * scale)), H = Math.max(1, Math.round(h * scale));
  const c = document.createElement('canvas');
  c.width = W;
  c.height = H;
  const ctx = c.getContext('2d')!;
  const img = ctx.createImageData(W, H);
  const d = img.data;
  const lut = makeLUT(g);
  const x0 = p0.x * scale, y0 = p0.y * scale;
  const dx = (p1.x - p0.x) * scale, dy = (p1.y - p0.y) * scale;
  const len2 = dx * dx + dy * dy || 1;
  const len = Math.sqrt(len2);
  const ux = dx / len, uy = dy / len; // along
  const ang0 = Math.atan2(dy, dx);
  const dither = g.dither;
  let i = 0;
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++, i += 4) {
      const px = x + 0.5 - x0, py = y + 0.5 - y0;
      let t: number;
      switch (g.shape) {
        case 'linear':
          t = (px * dx + py * dy) / len2;
          break;
        case 'reflected':
          t = Math.abs(px * dx + py * dy) / len2;
          break;
        case 'radial':
          t = Math.sqrt(px * px + py * py) / len;
          break;
        case 'diamond': {
          const a = px * ux + py * uy, b = -px * uy + py * ux;
          t = (Math.abs(a) + Math.abs(b)) / len;
          break;
        }
        default: {
          // angle: one turn around the start point, starting in the drag direction
          let a = Math.atan2(py, px) - ang0;
          a -= Math.floor(a / (2 * Math.PI)) * 2 * Math.PI;
          t = a / (2 * Math.PI);
        }
      }
      const k = Math.round(wrapT(t, g.repeat) * 1023) * 4;
      if (dither) {
        const n = (Math.random() - 0.5) * 2;
        d[i] = lut[k] + n;
        d[i + 1] = lut[k + 1] + n;
        d[i + 2] = lut[k + 2] + n;
      } else {
        d[i] = lut[k];
        d[i + 1] = lut[k + 1];
        d[i + 2] = lut[k + 2];
      }
      d[i + 3] = lut[k + 3];
    }
  }
  ctx.putImageData(img, 0, 0);
  return c;
}

// CSS for showing the gradient in the panel
export function cssGradient(stops: GradStop[], smooth: boolean) {
  // OKLab can't be written in CSS gradients everywhere, so sample it into many stops
  const parts: string[] = [];
  const n = smooth ? 24 : 0;
  if (n) {
    for (let i = 0; i <= n; i++) {
      const c = colorAt(stops, i / n, true);
      parts.push(`rgba(${c[0] | 0}, ${c[1] | 0}, ${c[2] | 0}, ${(c[3] / 255).toFixed(3)}) ${((i / n) * 100).toFixed(1)}%`);
    }
  } else {
    for (const s of sortStops(stops)) {
      const [r, g, b] = hexRGB(s.color);
      parts.push(`rgba(${r}, ${g}, ${b}, ${s.alpha}) ${(s.pos * 100).toFixed(1)}%`);
    }
  }
  return `linear-gradient(90deg, ${parts.join(', ')})`;
}

// For painting with a gradient: the color at distance `d` along a stroke
export function strokeColorAt(stops: GradStop[], smooth: boolean, d: number, length: number, repeat: GradRepeat) {
  const c = colorAt(stops, wrapT(d / Math.max(1, length), repeat === 'none' ? 'none' : repeat), smooth);
  return { hex: rgbHex(c[0], c[1], c[2]), alpha: c[3] / 255 };
}
