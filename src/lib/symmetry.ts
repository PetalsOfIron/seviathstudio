// lib/symmetry.ts
// Live mirror ("symmetry") drawing: every dab is drawn together with its mirror
// image(s). The axes go through a center point that you can drag anywhere.
//
// Modes
//   vertical   – left = right (the usual one for faces and bodies)
//   horizontal – top = bottom
//   quad       – both axes, 4 copies
//   radial     – N copies turned around the center (mandala / flowers / lace)
//   kaleido    – radial + every copy also mirrored (kaleidoscope, 2·N copies)
//
// App.svelte runs every dab through `symCopies()`, so brush, eraser, tapered and flat
// ends, color mixing and lasso fill all mirror the same way.

export type SymmetryMode = 'off' | 'vertical' | 'horizontal' | 'quad' | 'radial' | 'kaleido';

export type SymmetryState = {
  mode: SymmetryMode;
  cx: number; // center of the axes, in document pixels
  cy: number;
  count: number; // radial / kaleido: number of slices (2..32)
  angle: number; // turns the axes, in degrees (0 = straight up)
};

export type Pt = { x: number; y: number };
export type PtMap = (p: Pt) => Pt;

export const MODES: SymmetryMode[] = ['off', 'vertical', 'horizontal', 'quad', 'radial', 'kaleido'];

export const MODE_LABEL: Record<SymmetryMode, string> = {
  off: 'Off',
  vertical: 'Left ↔ right',
  horizontal: 'Top ↕ bottom',
  quad: 'Both (4)',
  radial: 'Radial',
  kaleido: 'Kaleidoscope',
};

export const MODE_HINT: Record<SymmetryMode, string> = {
  off: 'Draw normally.',
  vertical: 'Whatever you draw on one side appears mirrored on the other. Good for faces, bodies, armor.',
  horizontal: 'Top and bottom mirror each other.',
  quad: 'Mirrors across both axes: 4 copies.',
  radial: 'Copies turn around the center, like a flower or a mandala.',
  kaleido: 'Radial copies that are also mirrored, like a kaleidoscope or lace.',
};

// Cycle used by the shortcut: off → vertical → horizontal → both → radial → kaleido → off
export function nextMode(m: SymmetryMode): SymmetryMode {
  return MODES[(MODES.indexOf(m) + 1) % MODES.length];
}

// Rotates p around (cx, cy) by `a` radians
function rot(p: Pt, cx: number, cy: number, a: number): Pt {
  const c = Math.cos(a);
  const s = Math.sin(a);
  const x = p.x - cx;
  const y = p.y - cy;
  return { x: cx + x * c - y * s, y: cy + x * s + y * c };
}

// Mirrors p across the line through (cx, cy) with direction angle `a` (radians, 0 = pointing right)
function reflect(p: Pt, cx: number, cy: number, a: number): Pt {
  const c = Math.cos(2 * a);
  const s = Math.sin(2 * a);
  const x = p.x - cx;
  const y = p.y - cy;
  return { x: cx + x * c + y * s, y: cy + x * s - y * c };
}

// Every copy as a point → point function, WITHOUT the original (identity).
// All of them are rotations or reflections, so lines stay lines and circles stay circles.
export function symCopies(s: SymmetryState): PtMap[] {
  const { cx, cy } = s;
  const a = (s.angle * Math.PI) / 180;
  const out: PtMap[] = [];
  // the vertical axis points up (90°), turned by `angle`
  const vAxis = Math.PI / 2 + a;
  const hAxis = a;
  switch (s.mode) {
    case 'vertical':
      out.push((p) => reflect(p, cx, cy, vAxis));
      break;
    case 'horizontal':
      out.push((p) => reflect(p, cx, cy, hAxis));
      break;
    case 'quad':
      out.push((p) => reflect(p, cx, cy, vAxis));
      out.push((p) => reflect(p, cx, cy, hAxis));
      out.push((p) => rot(p, cx, cy, Math.PI));
      break;
    case 'radial':
    case 'kaleido': {
      const n = Math.max(2, Math.min(32, Math.round(s.count)));
      const step = (2 * Math.PI) / n;
      for (let i = 1; i < n; i++) out.push((p) => rot(p, cx, cy, step * i));
      if (s.mode === 'kaleido') {
        for (let i = 0; i < n; i++) out.push((p) => reflect(p, cx, cy, vAxis + (step * i) / 2));
      }
      break;
    }
  }
  return out;
}

// The mirror image(s) of a point, WITHOUT the original point.
export function mirrorPoints(p: Pt, s: SymmetryState): Pt[] {
  return symCopies(s).map((f) => f(p));
}

// Lines to draw on screen so you can see where the axes are (each as two end points).
// `reach` = how long the lines are; use the canvas diagonal.
export function axisLines(s: SymmetryState, reach: number): [Pt, Pt][] {
  const { cx, cy } = s;
  const a = (s.angle * Math.PI) / 180;
  const ray = (ang: number): [Pt, Pt] => [
    { x: cx, y: cy },
    { x: cx + Math.cos(ang) * reach, y: cy + Math.sin(ang) * reach },
  ];
  const line = (ang: number): [Pt, Pt] => [
    { x: cx - Math.cos(ang) * reach, y: cy - Math.sin(ang) * reach },
    { x: cx + Math.cos(ang) * reach, y: cy + Math.sin(ang) * reach },
  ];
  switch (s.mode) {
    case 'vertical':
      return [line(Math.PI / 2 + a)];
    case 'horizontal':
      return [line(a)];
    case 'quad':
      return [line(Math.PI / 2 + a), line(a)];
    case 'radial':
    case 'kaleido': {
      const n = Math.max(2, Math.min(32, Math.round(s.count)));
      // slice borders (screen "up" = -90°)
      return Array.from({ length: n }, (_, i) => ray(-Math.PI / 2 + a + (2 * Math.PI * i) / n));
    }
    default:
      return [];
  }
}

// True if a copy lands (almost) on the original point. Drawing both would make the
// line heavier exactly on the axis, so such copies are skipped.
export function sameSpot(p: Pt, q: Pt, radius: number) {
  return Math.hypot(p.x - q.x, p.y - q.y) < Math.max(0.5, radius * 0.25);
}
