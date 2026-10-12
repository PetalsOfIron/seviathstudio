// Color modes: RGB (screen) and CMYK (print).
//
// The browser can only paint in RGB, so a CMYK project is drawn in RGB and shown through
// a "print preview": colors that ink on paper can't reach (very bright blues, greens,
// neon colors) are pulled back to the nearest printable color. Export can then write a
// real CMYK TIFF. This is an approximation of a typical coated-paper press (like
// FOGRA39 / SWOP); a print shop with an ICC profile gives the exact result.

export type ColorMode = 'rgb' | 'cmyk';

// ---------- OKLab (perceptual color space) ----------

const toLin = (c: number) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
const toGamma = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);

export function rgbToOklab(r: number, g: number, b: number): [number, number, number] {
  r = toLin(r / 255);
  g = toLin(g / 255);
  b = toLin(b / 255);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

export function oklabToRgb(L: number, a: number, b: number): [number, number, number] {
  const l = Math.pow(L + 0.3963377774 * a + 0.2158037573 * b, 3);
  const m = Math.pow(L - 0.1055613458 * a - 0.0638541728 * b, 3);
  const s = Math.pow(L - 0.0894841775 * a - 1.291485548 * b, 3);
  const r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
  const g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
  const bb = -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s;
  const c = (x: number) => Math.round(Math.min(1, Math.max(0, toGamma(x))) * 255);
  return [c(r), c(g), c(bb)];
}

// ---------- Printable range ("gamut") ----------
// For each hue, the most colorful printable color (the "cusp"), taken from what the
// inks and their 2-ink mixes look like on coated paper.

// process cyan, blue (C+M), magenta, red (M+Y), yellow, green (C+Y) as they look on coated paper
const INKS = ['#00aeef', '#2e3192', '#ec008c', '#ed1c24', '#fff200', '#00a651'];
const PAPER_BLACK_L = 0.2; // the darkest a printer gets (rich black is not pure black)

type Cusp = { h: number; L: number; C: number };
let cusps: Cusp[] | null = null;

function hexRGB(hex: string): [number, number, number] {
  const n = parseInt(hex.replace('#', ''), 16) || 0;
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function getCusps(): Cusp[] {
  if (cusps) return cusps;
  cusps = INKS.map((hex) => {
    const [L, a, b] = rgbToOklab(...hexRGB(hex));
    return { h: Math.atan2(b, a), L, C: Math.hypot(a, b) };
  }).sort((p, q) => p.h - q.h);
  return cusps;
}

// The cusp for any hue, blended between the two nearest inks
function cuspAt(h: number): { L: number; C: number } {
  const cs = getCusps();
  const n = cs.length;
  for (let i = 0; i < n; i++) {
    const a = cs[i];
    const b = cs[(i + 1) % n];
    let span = b.h - a.h;
    if (span <= 0) span += 2 * Math.PI;
    let d = h - a.h;
    if (d < 0) d += 2 * Math.PI;
    if (d <= span) {
      const t = d / span;
      return { L: a.L + (b.L - a.L) * t, C: a.C + (b.C - a.C) * t };
    }
  }
  return { L: cs[0].L, C: cs[0].C };
}

// Pulls a color into the printable range. Colors that already print stay the same.
// Others move toward the middle gray of their hue (keeping the hue) only as far as
// needed, so they stay as colorful as ink allows instead of just washing out.
export function toPrintable(r: number, g: number, b: number): [number, number, number] {
  let [L, A, B] = rgbToOklab(r, g, b);
  const Lb = PAPER_BLACK_L * 0.9;
  L = Math.max(Lb, Math.min(1, L));
  const C = Math.hypot(A, B);
  if (C > 1e-4) {
    const cusp = cuspAt(Math.atan2(B, A));
    // the most colorful printable color at lightness l (edges bulge outward a little)
    const maxAt = (l: number) => {
      if (l >= cusp.L) return cusp.C * (1 - Math.pow(Math.min(1, (l - cusp.L) / Math.max(1e-3, 1 - cusp.L)), 1.6));
      return cusp.C * (1 - Math.pow(Math.min(1, (cusp.L - l) / Math.max(1e-3, cusp.L - Lb)), 1.6));
    };
    if (C > maxAt(L)) {
      // light colors aim a bit darker, toward where ink is most colorful
      const L0 = L > cusp.L ? (cusp.L + 0.4) / 2 : cusp.L;
      let lo = 0, hi = 1;
      for (let k = 0; k < 18; k++) {
        const t = (lo + hi) / 2;
        if (t * C <= maxAt(L0 + t * (L - L0))) lo = t;
        else hi = t;
      }
      L = L0 + lo * (L - L0);
      A *= lo;
      B *= lo;
    }
  }
  return oklabToRgb(L, A, B);
}

// True if the color would print (almost) as it looks on screen
export function isPrintable(hex: string) {
  const c = hexRGB(hex);
  const p = toPrintable(...c);
  return Math.abs(p[0] - c[0]) + Math.abs(p[1] - c[1]) + Math.abs(p[2] - c[2]) < 18;
}

// ---------- CMYK numbers ----------

// Simple conversion with gray replaced by black ink, as percentages 0..100
export function rgbToCmyk(r: number, g: number, b: number): [number, number, number, number] {
  const R = r / 255, G = g / 255, B = b / 255;
  const k = 1 - Math.max(R, G, B);
  if (k >= 0.999) return [0, 0, 0, 100];
  const c = (1 - R - k) / (1 - k);
  const m = (1 - G - k) / (1 - k);
  const y = (1 - B - k) / (1 - k);
  return [c, m, y, k].map((v) => Math.round(v * 100)) as [number, number, number, number];
}

export function cmykToRgb(c: number, m: number, y: number, k: number): [number, number, number] {
  const K = k / 100;
  return [c, m, y].map((v) => Math.round(255 * (1 - v / 100) * (1 - K))) as [number, number, number];
}

// ---------- Fast print preview of a whole picture ----------
// A lookup table: 64 steps per channel, built once.

let lut: Uint8Array | null = null;

function getLUT() {
  if (lut) return lut;
  lut = new Uint8Array(64 * 64 * 64 * 3);
  let i = 0;
  for (let r = 0; r < 64; r++)
    for (let g = 0; g < 64; g++)
      for (let b = 0; b < 64; b++) {
        const p = toPrintable(r * 4 + 2, g * 4 + 2, b * 4 + 2);
        lut[i++] = p[0];
        lut[i++] = p[1];
        lut[i++] = p[2];
      }
  return lut;
}

// The same table packed as whole pixels (red, green, blue, no alpha) for speed
let lut32: Uint32Array | null = null;

function getLUT32() {
  if (lut32) return lut32;
  const t = getLUT();
  lut32 = new Uint32Array(64 * 64 * 64);
  for (let i = 0; i < lut32.length; i++) {
    // canvas pixels are stored as bytes R, G, B, A, which is A<<24 | B<<16 | G<<8 | R on
    // little-endian computers (all PCs, Macs and phones)
    lut32[i] = (t[i * 3 + 2] << 16) | (t[i * 3 + 1] << 8) | t[i * 3];
  }
  return lut32;
}

// Changes RGBA pixels in place to how they would look printed (fast: one lookup per pixel)
export function applyPrintPreview(d: Uint8ClampedArray) {
  const t = getLUT32();
  const px = new Uint32Array(d.buffer, d.byteOffset, d.length >> 2);
  for (let i = 0; i < px.length; i++) {
    const v = px[i];
    if (v >>> 24 === 0) continue;
    // 6 bits of each of red, green, blue → table index
    const idx = ((v & 0xfc) << 10) | ((v & 0xfc00) >> 4) | ((v & 0xfc0000) >> 18);
    px[i] = (v & 0xff000000) | t[idx];
  }
}

// RGBA pixels → CMYK bytes (4 per pixel, 0 = no ink, 255 = full ink), printable colors first.
// Transparent pixels become paper (no ink); `alpha` adds a 5th byte with the transparency.
export function toCmykBytes(d: Uint8ClampedArray, alpha: boolean): Uint8Array {
  const t = getLUT();
  const n = d.length / 4;
  const step = alpha ? 5 : 4;
  const out = new Uint8Array(n * step);
  for (let p = 0, i = 0, o = 0; p < n; p++, i += 4, o += step) {
    const a = d[i + 3] / 255;
    // without alpha, put the color on white paper
    const r0 = alpha ? d[i] : d[i] * a + 255 * (1 - a);
    const g0 = alpha ? d[i + 1] : d[i + 1] * a + 255 * (1 - a);
    const b0 = alpha ? d[i + 2] : d[i + 2] * a + 255 * (1 - a);
    const j = ((((r0 | 0) >> 2) << 12) | (((g0 | 0) >> 2) << 6) | ((b0 | 0) >> 2)) * 3;
    const R = t[j] / 255, G = t[j + 1] / 255, B = t[j + 2] / 255;
    const k = 1 - Math.max(R, G, B);
    const inv = k >= 0.999 ? 0 : 1 / (1 - k);
    out[o] = Math.round((1 - R - k) * inv * 255);
    out[o + 1] = Math.round((1 - G - k) * inv * 255);
    out[o + 2] = Math.round((1 - B - k) * inv * 255);
    out[o + 3] = Math.round(k * 255);
    if (alpha) out[o + 4] = d[i + 3];
  }
  return out;
}

// ---------- Units for print sizes ----------

export type Unit = 'px' | 'mm' | 'cm' | 'in';
export const UNIT_PER_INCH: Record<Unit, number> = { px: 1, mm: 25.4, cm: 2.54, in: 1 };

export function toPixels(value: number, unit: Unit, dpi: number) {
  return unit === 'px' ? value : (value / UNIT_PER_INCH[unit]) * dpi;
}

export function fromPixels(px: number, unit: Unit, dpi: number) {
  return unit === 'px' ? px : (px / dpi) * UNIT_PER_INCH[unit];
}
