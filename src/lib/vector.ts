// Vector layers: lines are kept as points with pen pressure, not as pixels.
// The layer's pixels are drawn from the lines, so you can change the line width later,
// cut lines cleanly with the vector eraser, and nothing gets blurry when scaled.

import { strokeColorAt, type GradStop, type GradRepeat } from './gradient';

// A line painted with a gradient: the color changes along its length
export type StrokeGradient = { stops: GradStop[]; smooth: boolean; length: number; repeat: GradRepeat };

export type VecStroke = {
  pts: number[]; // x, y, pressure, x, y, pressure, ...
  color: string;
  grad?: StrokeGradient;
  size: number; // diameter at full pressure
  minSize: number; // 0..1, size at the lightest pressure
  sizePressure: boolean;
  ends: 'round' | 'taper' | 'flat';
  taper: number;
  opacity: number;
};

// A flat color shape (from Auto trace): one or more outlines filled with one color.
// Outlines inside other outlines are holes.
export type VecFill = { rings: number[][]; color: string };

export type VecLayer = {
  strokes: VecStroke[];
  width: number; // line width multiplier for the whole layer (1 = as drawn)
  fills?: VecFill[]; // color shapes, drawn under the lines
};

export function fillBounds(f: VecFill): Box {
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const ring of f.rings)
    for (let i = 0; i < ring.length; i += 2) {
      minX = Math.min(minX, ring[i]);
      maxX = Math.max(maxX, ring[i]);
      minY = Math.min(minY, ring[i + 1]);
      maxY = Math.max(maxY, ring[i + 1]);
    }
  return { x: minX - 2, y: minY - 2, w: maxX - minX + 4, h: maxY - minY + 4 };
}

export function drawFill(ctx: CanvasRenderingContext2D, f: VecFill) {
  ctx.beginPath();
  for (const ring of f.rings) {
    if (ring.length < 6) continue;
    ctx.moveTo(ring[0], ring[1]);
    for (let i = 2; i < ring.length; i += 2) ctx.lineTo(ring[i], ring[i + 1]);
    ctx.closePath();
  }
  ctx.fillStyle = f.color;
  ctx.fill('evenodd');
  // a hairline in the same color hides the thin seams between neighbouring shapes
  ctx.strokeStyle = f.color;
  ctx.lineWidth = 0.8;
  ctx.lineJoin = 'round';
  ctx.stroke();
}

// Is (x, y) inside the shape?
export function insideFill(f: VecFill, x: number, y: number) {
  let inside = false;
  for (const ring of f.rings) {
    for (let i = 0, j = ring.length - 2; i < ring.length; j = i, i += 2) {
      const xi = ring[i], yi = ring[i + 1], xj = ring[j], yj = ring[j + 1];
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
    }
  }
  return inside;
}

export function mapFill(f: VecFill, fn: (p: { x: number; y: number }) => { x: number; y: number }): VecFill {
  return {
    ...f,
    rings: f.rings.map((ring) => {
      const out = ring.slice();
      for (let i = 0; i < out.length; i += 2) {
        const q = fn({ x: out[i], y: out[i + 1] });
        out[i] = Math.round(q.x * 100) / 100;
        out[i + 1] = Math.round(q.y * 100) / 100;
      }
      return out;
    }),
  };
}

export type VPt = { x: number; y: number; p: number };

const toPts = (flat: number[]): VPt[] => {
  const out: VPt[] = [];
  for (let i = 0; i < flat.length; i += 3) out.push({ x: flat[i], y: flat[i + 1], p: flat[i + 2] });
  return out;
};
const toFlat = (pts: VPt[]) => pts.flatMap((q) => [round2(q.x), round2(q.y), Math.round(q.p * 1000) / 1000]);
const round2 = (x: number) => Math.round(x * 100) / 100;

// ---------- Cleaning up a drawn line ----------

// Ramer–Douglas–Peucker: removes points that don't change the shape by more than `eps` px
export function simplify(pts: VPt[], eps: number): VPt[] {
  if (pts.length <= 2) return pts.slice();
  const keep = new Uint8Array(pts.length);
  keep[0] = keep[pts.length - 1] = 1;
  const stack: [number, number][] = [[0, pts.length - 1]];
  while (stack.length) {
    const [a, b] = stack.pop()!;
    const A = pts[a], B = pts[b];
    const dx = B.x - A.x, dy = B.y - A.y;
    const len = Math.hypot(dx, dy) || 1;
    let worst = -1, at = -1;
    for (let i = a + 1; i < b; i++) {
      const P = pts[i];
      // distance from the line, plus pressure changes count a little so tapers survive
      const d = Math.abs((P.x - A.x) * dy - (P.y - A.y) * dx) / len;
      const pd = Math.abs(P.p - (A.p + ((B.p - A.p) * (i - a)) / (b - a))) * 4;
      const e = Math.max(d, pd);
      if (e > worst) {
        worst = e;
        at = i;
      }
    }
    if (worst > eps && at > 0) {
      keep[at] = 1;
      stack.push([a, at], [at, b]);
    }
  }
  return pts.filter((_, i) => keep[i]);
}

// Turns the raw pen points into a clean line. `smooth` 0..1:
// 0 = keeps your hand exactly, 1 = very smooth curves with few points.
export function cleanLine(raw: VPt[], smooth: number): number[] {
  let pts = raw.filter((q, i) => i === 0 || Math.hypot(q.x - raw[i - 1].x, q.y - raw[i - 1].y) > 0.05);
  if (pts.length === 1) pts = [pts[0], { ...pts[0], x: pts[0].x + 0.01 }];
  // 1. even out the shake (ends stay where you put them)
  const passes = Math.round(smooth * 10);
  for (let k = 0; k < passes; k++) {
    pts = pts.map((q, i) =>
      i === 0 || i === pts.length - 1
        ? q
        : {
            x: (pts[i - 1].x + 2 * q.x + pts[i + 1].x) / 4,
            y: (pts[i - 1].y + 2 * q.y + pts[i + 1].y) / 4,
            p: (pts[i - 1].p + 2 * q.p + pts[i + 1].p) / 4,
          },
    );
  }
  // 2. keep only the points that matter (the curve between them is rebuilt when drawing)
  pts = simplify(pts, 0.12 + smooth * 1.2);
  return toFlat(pts);
}

// ---------- Drawing ----------

// Smooth curve through the points (Catmull-Rom), with points about `step` px apart
function densify(pts: VPt[], step: number): VPt[] {
  if (pts.length < 2) return pts.slice();
  const out: VPt[] = [pts[0]];
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[Math.max(0, i - 1)], p1 = pts[i], p2 = pts[i + 1], p3 = pts[Math.min(pts.length - 1, i + 2)];
    const n = Math.max(1, Math.ceil(Math.hypot(p2.x - p1.x, p2.y - p1.y) / step));
    for (let j = 1; j <= n; j++) {
      const t = j / n, t2 = t * t, t3 = t2 * t;
      const cr = (a: number, b: number, c: number, d: number) =>
        0.5 * (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3);
      out.push({ x: cr(p0.x, p1.x, p2.x, p3.x), y: cr(p0.y, p1.y, p2.y, p3.y), p: p1.p + (p2.p - p1.p) * t });
    }
  }
  return out;
}

function taperFactor(d: number, total: number, s: VecStroke) {
  if (s.ends !== 'taper') return 1;
  const L = Math.max(4, s.taper * s.size * 15);
  const ease = (t: number) => 1 - Math.pow(1 - Math.min(Math.max(t, 0), 1), 2);
  const tip = 0.06;
  return Math.min(tip + (1 - tip) * ease(d / L), tip + (1 - tip) * ease((total - d) / L));
}

// The stroke as points with their radius
function outline(s: VecStroke, widthScale: number): { x: number; y: number; r: number; d: number }[] {
  const base = toPts(s.pts);
  // a point every ~2 px is plenty for a smooth curve (joints are rounded); fewer points = faster
  const step = Math.max(0.75, Math.min(2.5, (s.size * widthScale) / 3));
  const pts = densify(base, step);
  let total = 0;
  const dist = [0];
  for (let i = 1; i < pts.length; i++) {
    total += Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y);
    dist.push(total);
  }
  return pts.map((q, i) => {
    const f = s.sizePressure ? s.minSize + (1 - s.minSize) * q.p : 1;
    return { x: q.x, y: q.y, r: Math.max(0.25, ((s.size * f) / 2) * widthScale * taperFactor(dist[i], total, s)), d: dist[i] };
  });
}

// Adds a polygon to the path, always turning the same way so overlapping parts don't cancel out
function addPoly(ctx: CanvasRenderingContext2D, poly: [number, number][]) {
  let area = 0;
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i], b = poly[(i + 1) % poly.length];
    area += a[0] * b[1] - b[0] * a[1];
  }
  const list = area < 0 ? [...poly].reverse() : poly;
  ctx.moveTo(list[0][0], list[0][1]);
  for (let i = 1; i < list.length; i++) ctx.lineTo(list[i][0], list[i][1]);
  ctx.closePath();
}

function strokePath(ctx: CanvasRenderingContext2D, s: VecStroke, widthScale: number) {
  const o = outline(s, widthScale);
  ctx.beginPath();
  for (let i = 0; i < o.length; i++) {
    const a = o[i];
    const isEnd = i === 0 || i === o.length - 1;
    // round joints everywhere; flat lines have no round cap at the two ends
    if (!(s.ends === 'flat' && isEnd && o.length > 1)) {
      ctx.moveTo(a.x + a.r, a.y);
      ctx.arc(a.x, a.y, a.r, 0, Math.PI * 2);
      ctx.closePath();
    }
    if (i === 0) continue;
    const b = o[i - 1];
    const dx = a.x - b.x, dy = a.y - b.y;
    const len = Math.hypot(dx, dy);
    if (len < 1e-6) continue;
    const nx = -dy / len, ny = dx / len;
    addPoly(ctx, [
      [b.x + nx * b.r, b.y + ny * b.r],
      [a.x + nx * a.r, a.y + ny * a.r],
      [a.x - nx * a.r, a.y - ny * a.r],
      [b.x - nx * b.r, b.y - ny * b.r],
    ]);
  }
}

export function strokeBounds(s: VecStroke, widthScale = 1) {
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (let i = 0; i < s.pts.length; i += 3) {
    minX = Math.min(minX, s.pts[i]);
    maxX = Math.max(maxX, s.pts[i]);
    minY = Math.min(minY, s.pts[i + 1]);
    maxY = Math.max(maxY, s.pts[i + 1]);
  }
  const pad = (s.size * widthScale) / 2 + 4;
  return { x: minX - pad, y: minY - pad, w: maxX - minX + pad * 2, h: maxY - minY + pad * 2 };
}

let temp: CanvasRenderingContext2D | null = null;

// Gradient lines: every little piece gets its own color
function drawGradStroke(ctx: CanvasRenderingContext2D, s: VecStroke, widthScale: number) {
  const g = s.grad!;
  const o = outline(s, widthScale);
  for (let i = 0; i < o.length; i++) {
    const a = o[i];
    ctx.fillStyle = strokeColorAt(g.stops, g.smooth, a.d, g.length, g.repeat).hex;
    ctx.beginPath();
    ctx.moveTo(a.x + a.r, a.y);
    ctx.arc(a.x, a.y, a.r, 0, Math.PI * 2);
    ctx.closePath();
    if (i > 0) {
      const b = o[i - 1];
      const dx = a.x - b.x, dy = a.y - b.y;
      const len = Math.hypot(dx, dy);
      if (len > 1e-6) {
        const nx = -dy / len, ny = dx / len;
        addPoly(ctx, [
          [b.x + nx * b.r, b.y + ny * b.r],
          [a.x + nx * a.r, a.y + ny * a.r],
          [a.x - nx * a.r, a.y - ny * a.r],
          [b.x - nx * b.r, b.y - ny * b.r],
        ]);
      }
    }
    ctx.fill('nonzero');
  }
}

export function drawStroke(ctx: CanvasRenderingContext2D, s: VecStroke, widthScale = 1) {
  if (s.pts.length < 3) return;
  if (s.grad) {
    if (s.opacity >= 0.999) return drawGradStroke(ctx, s, widthScale);
  }
  if (s.opacity >= 0.999) {
    ctx.fillStyle = s.color;
    strokePath(ctx, s, widthScale);
    ctx.fill('nonzero');
    return;
  }
  // see-through lines: draw solid first, then put the whole line down at its opacity
  // (so it doesn't get darker where the line crosses itself)
  const b = strokeBounds(s, widthScale);
  const w = Math.ceil(b.w), h = Math.ceil(b.h);
  if (w <= 0 || h <= 0) return;
  if (!temp) temp = document.createElement('canvas').getContext('2d')!;
  const t = temp;
  if (t.canvas.width < w || t.canvas.height < h) {
    t.canvas.width = Math.max(t.canvas.width, w);
    t.canvas.height = Math.max(t.canvas.height, h);
  }
  t.setTransform(1, 0, 0, 1, 0, 0);
  t.clearRect(0, 0, w, h);
  t.translate(-b.x, -b.y);
  if (s.grad) drawGradStroke(t, s, widthScale);
  else {
    t.fillStyle = s.color;
    strokePath(t, s, widthScale);
    t.fill('nonzero');
  }
  t.setTransform(1, 0, 0, 1, 0, 0);
  ctx.save();
  ctx.globalAlpha *= s.opacity;
  ctx.drawImage(t.canvas, 0, 0, w, h, b.x, b.y, w, h);
  ctx.restore();
}

// Draws the whole vector layer into its canvas
export function renderVectorLayer(canvas: HTMLCanvasElement, v: VecLayer) {
  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = 'source-over';
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  for (const f of v.fills ?? []) drawFill(ctx, f);
  for (const s of v.strokes) drawStroke(ctx, s, v.width);
}

// ---------- Editing ----------

// The same line moved by a point function (used for mirror drawing)
export function mapStroke(s: VecStroke, f: (p: { x: number; y: number }) => { x: number; y: number }): VecStroke {
  const pts = s.pts.slice();
  for (let i = 0; i < pts.length; i += 3) {
    const q = f({ x: pts[i], y: pts[i + 1] });
    pts[i] = round2(q.x);
    pts[i + 1] = round2(q.y);
  }
  return { ...s, pts };
}

export type EraseMode = 'cut' | 'whole';

// The vector eraser. `path` = where the eraser went, with its radius.
// 'cut': removes only the touched part of each line, the rest stays as separate clean lines.
// 'whole': removes every line it touches.
// Returns null when nothing was touched.
export function eraseStrokes(strokes: VecStroke[], path: { x: number; y: number; r: number }[], mode: EraseMode, widthScale = 1): VecStroke[] | null {
  if (!path.length) return null;
  // the eraser path, filled in so fast movements don't skip
  const er: { x: number; y: number; r: number }[] = [path[0]];
  for (let i = 1; i < path.length; i++) {
    const a = path[i - 1], b = path[i];
    const n = Math.max(1, Math.ceil(Math.hypot(b.x - a.x, b.y - a.y) / Math.max(0.5, Math.min(a.r, b.r) * 0.5)));
    for (let j = 1; j <= n; j++) {
      const t = j / n;
      er.push({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, r: a.r + (b.r - a.r) * t });
    }
  }
  let ex0 = Infinity, ey0 = Infinity, ex1 = -Infinity, ey1 = -Infinity;
  for (const e of er) {
    ex0 = Math.min(ex0, e.x - e.r);
    ey0 = Math.min(ey0, e.y - e.r);
    ex1 = Math.max(ex1, e.x + e.r);
    ey1 = Math.max(ey1, e.y + e.r);
  }

  let changed = false;
  const out: VecStroke[] = [];
  for (const s of strokes) {
    const b = strokeBounds(s, widthScale);
    if (b.x > ex1 || b.y > ey1 || b.x + b.w < ex0 || b.y + b.h < ey0) {
      out.push(s);
      continue;
    }
    const lineR = (s.size * widthScale) / 2;
    // points every ~1 px along the line, so the cut is exact
    const dense = densify(toPts(s.pts), 1);
    const hit = dense.map((q) => er.some((e) => Math.hypot(q.x - e.x, q.y - e.y) <= e.r + (mode === 'whole' ? lineR : 0)));
    if (!hit.some(Boolean)) {
      out.push(s);
      continue;
    }
    changed = true;
    if (mode === 'whole') continue;
    let piece: VPt[] = [];
    const flush = () => {
      if (piece.length >= 2) out.push({ ...s, pts: toFlat(simplify(piece, 0.2)) });
      piece = [];
    };
    dense.forEach((q, i) => (hit[i] ? flush() : piece.push(q)));
    flush();
  }
  return changed ? out : null;
}

// ---------- Redrawing only what changed ----------

export type Box = { x: number; y: number; w: number; h: number };

function unionBox(a: Box | null, b: Box): Box {
  if (!a) return { ...b };
  const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
  return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
}

// The area where two versions of a layer differ (lines added or removed).
// null = nothing changed; 'all' = the whole layer changed (line width).
export function changedArea(a: VecLayer, b: VecLayer): Box | null | 'all' {
  if (a.width !== b.width) return 'all';
  const inA = new Set(a.strokes);
  const inB = new Set(b.strokes);
  let box: Box | null = null;
  for (const s of a.strokes) if (!inB.has(s)) box = unionBox(box, strokeBounds(s, a.width));
  for (const s of b.strokes) if (!inA.has(s)) box = unionBox(box, strokeBounds(s, b.width));
  const fa = new Set(a.fills ?? []), fb = new Set(b.fills ?? []);
  for (const f of a.fills ?? []) if (!fb.has(f)) box = unionBox(box, fillBounds(f));
  for (const f of b.fills ?? []) if (!fa.has(f)) box = unionBox(box, fillBounds(f));
  return box;
}

// Redraws only the lines inside `r` (fast undo and erasing on layers with many lines)
export function renderVectorRegion(canvas: HTMLCanvasElement, v: VecLayer, r: Box) {
  const x = Math.max(0, Math.floor(r.x)), y = Math.max(0, Math.floor(r.y));
  const w = Math.min(canvas.width, Math.ceil(r.x + r.w)) - x;
  const h = Math.min(canvas.height, Math.ceil(r.y + r.h)) - y;
  if (w <= 0 || h <= 0) return;
  const ctx = canvas.getContext('2d')!;
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = 'source-over';
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.clearRect(x, y, w, h);
  for (const f of v.fills ?? []) {
    const b = fillBounds(f);
    if (b.x > x + w || b.y > y + h || b.x + b.w < x || b.y + b.h < y) continue;
    drawFill(ctx, f);
  }
  for (const s of v.strokes) {
    const b = strokeBounds(s, v.width);
    if (b.x > x + w || b.y > y + h || b.x + b.w < x || b.y + b.h < y) continue;
    drawStroke(ctx, s, v.width);
  }
  ctx.restore();
}

// ---------- Fast drawing while the pen is moving ----------
// Only the newest piece of the line is drawn each frame (redrawing a long line on every
// frame made fast drawing freeze). The end taper is added when the pen lifts.

export type LineStyle = Omit<VecStroke, 'pts' | 'opacity'>;

function radiusOf(st: LineStyle, p: number, widthScale: number) {
  const f = st.sizePressure ? st.minSize + (1 - st.minSize) * p : 1;
  return Math.max(0.25, ((st.size * f) / 2) * widthScale);
}

function startTaper(st: LineStyle, d: number) {
  if (st.ends !== 'taper') return 1;
  const L = Math.max(4, st.taper * st.size * 15);
  const t = Math.min(Math.max(d / L, 0), 1);
  return 0.06 + 0.94 * (1 - Math.pow(1 - t, 2));
}

// Draws the line between point i0 and i1 (curved through the neighbouring points).
// `dist` = distance of each point from the start. Returns the area drawn.
export function drawLivePiece(
  ctx: CanvasRenderingContext2D,
  pts: VPt[],
  dist: number[],
  i0: number,
  i1: number,
  st: LineStyle,
  widthScale: number,
  map: (p: { x: number; y: number }) => { x: number; y: number } = (p) => p,
): Box | null {
  const n = pts.length;
  if (!n) return null;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  const add = (x: number, y: number, r: number) => {
    minX = Math.min(minX, x - r);
    minY = Math.min(minY, y - r);
    maxX = Math.max(maxX, x + r);
    maxY = Math.max(maxY, y + r);
  };
  ctx.fillStyle = st.color;
  ctx.beginPath();
  let prev: { x: number; y: number; r: number } | null = null;
  const g = st.grad;
  const put = (x: number, y: number, p: number, d: number) => {
    const q = map({ x, y });
    const r = radiusOf(st, p, widthScale) * startTaper(st, d);
    if (g) {
      // gradient line: finish the last piece in its color, start a new one in this color
      ctx.fill('nonzero');
      ctx.fillStyle = strokeColorAt(g.stops, g.smooth, d, g.length, g.repeat).hex;
      ctx.beginPath();
    }
    ctx.moveTo(q.x + r, q.y);
    ctx.arc(q.x, q.y, r, 0, Math.PI * 2);
    ctx.closePath();
    if (prev) {
      const dx = q.x - prev.x, dy = q.y - prev.y;
      const len = Math.hypot(dx, dy);
      if (len > 1e-6) {
        const nx = -dy / len, ny = dx / len;
        addPoly(ctx, [
          [prev.x + nx * prev.r, prev.y + ny * prev.r],
          [q.x + nx * r, q.y + ny * r],
          [q.x - nx * r, q.y - ny * r],
          [prev.x - nx * prev.r, prev.y - ny * prev.r],
        ]);
      }
    }
    prev = { x: q.x, y: q.y, r };
    add(q.x, q.y, r);
  };
  if (i0 === 0) put(pts[0].x, pts[0].y, pts[0].p, 0);
  const step = Math.max(0.75, Math.min(2.5, (st.size * widthScale) / 3));
  for (let i = Math.max(0, i0); i < Math.min(i1, n - 1); i++) {
    const p0 = pts[Math.max(0, i - 1)], p1 = pts[i], p2 = pts[i + 1], p3 = pts[Math.min(n - 1, i + 2)];
    const segLen = dist[i + 1] - dist[i];
    const m = Math.max(1, Math.ceil(segLen / step));
    if (i !== i0 || i0 !== 0) {
      // start this piece exactly where the last one ended
      if (!prev) {
        const q = map({ x: p1.x, y: p1.y });
        prev = { x: q.x, y: q.y, r: radiusOf(st, p1.p, widthScale) * startTaper(st, dist[i]) };
      }
    }
    for (let j = 1; j <= m; j++) {
      const t = j / m, t2 = t * t, t3 = t2 * t;
      const cr = (a: number, b: number, c: number, d: number) =>
        0.5 * (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3);
      put(cr(p0.x, p1.x, p2.x, p3.x), cr(p0.y, p1.y, p2.y, p3.y), p1.p + (p2.p - p1.p) * t, dist[i] + segLen * t);
    }
  }
  ctx.fill('nonzero');
  return isFinite(minX) ? { x: minX, y: minY, w: maxX - minX, h: maxY - minY } : null;
}

// ---------- Editing existing lines ----------

// The points exactly as given (shapes from "hold to straighten" must keep their corners)
export function exactLine(pts: VPt[]): number[] {
  return toFlat(pts);
}

// The top-most line under (x, y), within `tol` px of its edge
export function hitStroke(strokes: VecStroke[], x: number, y: number, tol: number, widthScale = 1): VecStroke | null {
  for (let k = strokes.length - 1; k >= 0; k--) {
    const s = strokes[k];
    const b = strokeBounds(s, widthScale);
    if (x < b.x - tol || y < b.y - tol || x > b.x + b.w + tol || y > b.y + b.h + tol) continue;
    const pts = densify(toPts(s.pts), 2);
    const r = (s.size * widthScale) / 2 + tol;
    if (pts.some((q) => Math.hypot(q.x - x, q.y - y) <= r)) return s;
  }
  return null;
}

// Lines that have a point inside the box
export function strokesInBox(strokes: VecStroke[], box: Box): VecStroke[] {
  return strokes.filter((s) => {
    for (let i = 0; i < s.pts.length; i += 3) {
      const x = s.pts[i], y = s.pts[i + 1];
      if (x >= box.x && y >= box.y && x <= box.x + box.w && y <= box.y + box.h) return true;
    }
    return false;
  });
}

// Smoother curve (amount 0..1)
export function smoothStroke(s: VecStroke, amount: number): VecStroke {
  // add points first so smoothing has something to work with, then clean up again
  const dense = densify(toPts(s.pts), 3);
  return { ...s, pts: cleanLine(dense, amount) };
}

// Fewer points, same look ("reduce nodes")
export function simplifyStroke(s: VecStroke, eps: number): VecStroke {
  return { ...s, pts: toFlat(simplify(toPts(s.pts), eps)) };
}

export function setPressures(s: VecStroke, pressures: number[]): VecStroke {
  const pts = s.pts.slice();
  for (let i = 0; i < pressures.length && i * 3 + 2 < pts.length; i++) pts[i * 3 + 2] = Math.round(pressures[i] * 1000) / 1000;
  return { ...s, pts };
}

export function nodeCount(s: VecStroke) {
  return s.pts.length / 3;
}

// The same line with a point every `step` px (so its pressure can be edited finely)
export function densifyStroke(s: VecStroke, step: number): VecStroke {
  return { ...s, pts: toFlat(densify(toPts(s.pts), step)) };
}

// ---------- Node editing (Vector select) ----------

// The node of these lines nearest to (x, y), within `tol` px
// `spacing` > 0: only the handles keyNodes() shows can be grabbed
export function nearestNode(strokes: VecStroke[], x: number, y: number, tol: number, spacing = 0): { s: VecStroke; k: number } | null {
  let best: { s: VecStroke; k: number } | null = null;
  let bd = tol;
  for (const s of strokes) {
    for (const k of spacing > 0 ? keyNodes(s, spacing) : Array.from({ length: s.pts.length / 3 }, (_, i) => i)) {
      const d = Math.hypot(s.pts[k * 3] - x, s.pts[k * 3 + 1] - y);
      if (d <= bd) {
        bd = d;
        best = { s, k };
      }
    }
  }
  return best;
}

// Moves node k by (dx, dy). With `soft` > 0, the nodes next to it along the line (up to
// `soft` px away, measured along the line) follow smoothly, so freehand lines bend instead of spiking.
export function moveNode(s: VecStroke, k: number, dx: number, dy: number, soft = 0): VecStroke {
  const pts = s.pts.slice();
  const n = pts.length / 3;
  const shift = (i: number, w: number) => {
    pts[i * 3] = round2(s.pts[i * 3] + dx * w);
    pts[i * 3 + 1] = round2(s.pts[i * 3 + 1] + dy * w);
  };
  shift(k, 1);
  if (soft > 0) {
    for (const step of [-1, 1]) {
      let d = 0;
      for (let i = k + step; i >= 0 && i < n; i += step) {
        const j = i - step;
        d += Math.hypot(s.pts[i * 3] - s.pts[j * 3], s.pts[i * 3 + 1] - s.pts[j * 3 + 1]);
        if (d >= soft) break;
        const t = 1 - (d / soft) * (d / soft);
        shift(i, t * t);
      }
    }
  }
  return { ...s, pts };
}

// The line without node k (a line keeps at least 2 nodes)
export function deleteNode(s: VecStroke, k: number): VecStroke {
  if (s.pts.length <= 6) return s;
  const pts = s.pts.slice();
  pts.splice(k * 3, 3);
  return { ...s, pts };
}

// Adds a node on the line where it passes nearest to (x, y). Returns the new line and the node index.
export function insertNode(s: VecStroke, x: number, y: number): { s: VecStroke; k: number } {
  let best = 0, bd = Infinity, bt = 0;
  for (let i = 0; i + 1 < s.pts.length / 3; i++) {
    const ax = s.pts[i * 3], ay = s.pts[i * 3 + 1], bx = s.pts[i * 3 + 3], by = s.pts[i * 3 + 4];
    const vx = bx - ax, vy = by - ay;
    const len2 = vx * vx + vy * vy || 1;
    const t = Math.max(0, Math.min(1, ((x - ax) * vx + (y - ay) * vy) / len2));
    const d = Math.hypot(ax + vx * t - x, ay + vy * t - y);
    if (d < bd) {
      bd = d;
      best = i;
      bt = t;
    }
  }
  const i = best * 3;
  const p = s.pts[i + 2] + ((s.pts[i + 5] ?? s.pts[i + 2]) - s.pts[i + 2]) * bt;
  const nx = s.pts[i] + ((s.pts[i + 3] ?? s.pts[i]) - s.pts[i]) * bt;
  const ny = s.pts[i + 1] + ((s.pts[i + 4] ?? s.pts[i + 1]) - s.pts[i + 1]) * bt;
  const pts = s.pts.slice();
  pts.splice(i + 3, 0, round2(nx), round2(ny), p);
  return { s: { ...s, pts }, k: best + 1 };
}

// The nodes worth showing as handles: on dense freehand lines only every node that is at
// least `minDist` px from the previous handle (plus both ends), so handles don't cover the line.
export function keyNodes(s: VecStroke, minDist: number): number[] {
  const n = s.pts.length / 3;
  if (n <= 2) return Array.from({ length: n }, (_, i) => i);
  const out = [0];
  let lx = s.pts[0], ly = s.pts[1];
  for (let i = 1; i < n - 1; i++) {
    const x = s.pts[i * 3], y = s.pts[i * 3 + 1];
    if (Math.hypot(x - lx, y - ly) >= minDist && Math.hypot(x - s.pts[(n - 1) * 3], y - s.pts[(n - 1) * 3 + 1]) >= minDist * 0.6) {
      out.push(i);
      lx = x;
      ly = y;
    }
  }
  out.push(n - 1);
  return out;
}
