// "Hold to straighten": draw a shape by hand, keep the pen still at the end, and it
// becomes a clean line, circle, ellipse, triangle, rectangle or polygon.
// Works on any brush and on vector layers (the points are the same).

export type SPt = { x: number; y: number; p: number };
export type ShapeKind = 'line' | 'circle' | 'ellipse' | 'triangle' | 'rectangle' | 'polygon';

function rdp(pts: SPt[], eps: number): SPt[] {
  if (pts.length <= 2) return pts.slice();
  const keep = new Uint8Array(pts.length);
  keep[0] = keep[pts.length - 1] = 1;
  const stack: [number, number][] = [[0, pts.length - 1]];
  while (stack.length) {
    const [a, b] = stack.pop()!;
    const A = pts[a], B = pts[b];
    const dx = B.x - A.x, dy = B.y - A.y;
    const len = Math.hypot(dx, dy);
    let worst = -1, at = -1;
    for (let i = a + 1; i < b; i++) {
      const d = len < 1e-6 ? Math.hypot(pts[i].x - A.x, pts[i].y - A.y) : Math.abs((pts[i].x - A.x) * dy - (pts[i].y - A.y) * dx) / len;
      if (d > worst) {
        worst = d;
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

// Points every ~`step` px along straight edges; corners are doubled so curves through
// them stay sharp
function edges(corners: { x: number; y: number }[], p: number, closed: boolean, step = 4): SPt[] {
  const out: SPt[] = [];
  const list = closed ? [...corners, corners[0]] : corners;
  for (let i = 0; i < list.length - 1; i++) {
    const a = list[i], b = list[i + 1];
    const n = Math.max(1, Math.ceil(Math.hypot(b.x - a.x, b.y - a.y) / step));
    out.push({ x: a.x, y: a.y, p });
    for (let j = 0; j < n; j++) out.push({ x: a.x + ((b.x - a.x) * j) / n, y: a.y + ((b.y - a.y) * j) / n, p });
  }
  const last = list[list.length - 1];
  out.push({ x: last.x, y: last.y, p }, { x: last.x, y: last.y, p });
  return out;
}

function ellipsePts(cx: number, cy: number, rx: number, ry: number, rot: number, p: number, start: number): SPt[] {
  const n = Math.max(48, Math.min(240, Math.round((rx + ry) * 0.6)));
  const out: SPt[] = [];
  const c = Math.cos(rot), s = Math.sin(rot);
  for (let i = 0; i <= n; i++) {
    const a = start + (i / n) * Math.PI * 2;
    const x = Math.cos(a) * rx, y = Math.sin(a) * ry;
    out.push({ x: cx + x * c - y * s, y: cy + x * s + y * c, p });
  }
  return out;
}

// Recognizes what the drawn points look like. Returns the clean shape, or null when the
// line doesn't look like any shape (then nothing changes).
export function recognize(raw: SPt[]): { kind: ShapeKind; pts: SPt[] } | null {
  const pts = raw.filter((q, i) => i === 0 || Math.hypot(q.x - raw[i - 1].x, q.y - raw[i - 1].y) > 0.5);
  if (pts.length < 3) return null;
  let len = 0;
  for (let i = 1; i < pts.length; i++) len += Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y);
  if (len < 12) return null;
  const p = pts.reduce((a, q) => a + q.p, 0) / pts.length; // even pressure for clean shapes
  const xs = pts.map((q) => q.x), ys = pts.map((q) => q.y);
  const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
  const size = Math.max(maxX - minX, maxY - minY);
  const first = pts[0], last = pts[pts.length - 1];
  const gap = Math.hypot(last.x - first.x, last.y - first.y);

  // ---- Open: a straight line?
  if (gap > Math.max(size * 0.25, len * 0.12)) {
    const dx = last.x - first.x, dy = last.y - first.y;
    const chord = Math.hypot(dx, dy);
    let worst = 0;
    for (const q of pts) worst = Math.max(worst, Math.abs((q.x - first.x) * dy - (q.y - first.y) * dx) / chord);
    if (worst <= chord * 0.06 + 2) return { kind: 'line', pts: edges([first, last], p, false) };
    return null;
  }

  // ---- Closed shapes
  const closedPts = [...pts, { ...first }];
  let perim = 0;
  for (let i = 1; i < closedPts.length; i++) perim += Math.hypot(closedPts[i].x - closedPts[i - 1].x, closedPts[i].y - closedPts[i - 1].y);
  let corners = rdp(closedPts, perim * 0.045);
  // the start point is usually not a real corner: drop it if it lies on a straight edge
  if (corners.length > 3) {
    const a = corners[corners.length - 2], b = corners[0], c = corners[1];
    const ab = Math.hypot(b.x - a.x, b.y - a.y), cl = Math.hypot(c.x - a.x, c.y - a.y);
    const dev = cl > 0 ? Math.abs((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)) / cl : ab;
    if (dev < perim * 0.03) corners = corners.slice(1, -1).concat([corners[1]]);
  }
  const n = corners.length - 1; // the last one repeats the first

  // How far the drawn points are from each candidate (average, relative to the shape size)
  const segDist = (q: SPt, a: { x: number; y: number }, b: { x: number; y: number }) => {
    const dx = b.x - a.x, dy = b.y - a.y;
    const l2 = dx * dx + dy * dy || 1;
    const t = Math.max(0, Math.min(1, ((q.x - a.x) * dx + (q.y - a.y) * dy) / l2));
    return Math.hypot(q.x - (a.x + dx * t), q.y - (a.y + dy * t));
  };
  const polyErr = (cs: { x: number; y: number }[]) => {
    let sum = 0;
    for (const q of pts) {
      let best = Infinity;
      for (let i = 0; i < cs.length; i++) best = Math.min(best, segDist(q, cs[i], cs[(i + 1) % cs.length]));
      sum += best;
    }
    return sum / pts.length / size;
  };
  const ecx = (minX + maxX) / 2, ecy = (minY + maxY) / 2;
  const rx = Math.max(1, (maxX - minX) / 2), ry = Math.max(1, (maxY - minY) / 2);
  let eSum = 0;
  for (const q of pts) eSum += Math.abs(Math.hypot((q.x - ecx) / rx, (q.y - ecy) / ry) - 1) * Math.min(rx, ry);
  const ellErr = eSum / pts.length / size;
  const pErr = n >= 3 && n <= 8 ? polyErr(corners.slice(0, n)) : Infinity;

  if (Math.min(ellErr, pErr) > 0.05) return null; // doesn't look like a clean shape

  if (ellErr <= pErr) {
    const start = Math.atan2(first.y - ecy, first.x - ecx);
    if (Math.abs(rx - ry) / Math.max(rx, ry) < 0.12) {
      const r = (rx + ry) / 2;
      return { kind: 'circle', pts: ellipsePts(ecx, ecy, r, r, 0, p, start) };
    }
    return { kind: 'ellipse', pts: ellipsePts(ecx, ecy, rx, ry, 0, p, start) };
  }

  if (n === 3) return { kind: 'triangle', pts: edges(corners.slice(0, 3), p, true) };
  if (n === 4) {
    // nearly right angles → a true rectangle (kept at the angle it was drawn)
    const c4 = corners.slice(0, 4);
    const mx = (c4[0].x + c4[1].x + c4[2].x + c4[3].x) / 4, my = (c4[0].y + c4[1].y + c4[2].y + c4[3].y) / 4;
    const ang = Math.atan2(c4[1].y - c4[0].y, c4[1].x - c4[0].x);
    const snapped = Math.round(ang / (Math.PI / 2)) * (Math.PI / 2);
    const rot = Math.abs(ang - snapped) < 0.12 ? snapped : ang;
    const ux = Math.cos(rot), uy = Math.sin(rot);
    const proj = c4.map((q) => ({ u: (q.x - mx) * ux + (q.y - my) * uy, v: -(q.x - mx) * uy + (q.y - my) * ux }));
    const u0 = Math.min(...proj.map((q) => q.u)), u1 = Math.max(...proj.map((q) => q.u));
    const v0 = Math.min(...proj.map((q) => q.v)), v1 = Math.max(...proj.map((q) => q.v));
    const toXY = (u: number, v: number) => ({ x: mx + u * ux - v * uy, y: my + u * uy + v * ux });
    return { kind: 'rectangle', pts: edges([toXY(u0, v0), toXY(u1, v0), toXY(u1, v1), toXY(u0, v1)], p, true) };
  }
  return { kind: 'polygon', pts: edges(corners.slice(0, n), p, true) };
}
