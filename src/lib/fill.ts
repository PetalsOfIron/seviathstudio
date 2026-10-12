// Flood fill ("paint bucket") on pixel data

export type FillResult = {
  mask: Uint8Array; // 1 = filled, 0 = not
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  open?: Uint8Array; // pixels that looked like the clicked color (1) vs. lines (0)
};

// Fills the area connected to (sx, sy) whose color is within `tolerance` (0..255)
// of the color at the start point. Works on any image: one layer or all layers combined.
// `gap` closes small openings in the lines: the lines are made `gap` px thicker while
// looking for the area, so the fill can't leak through gaps smaller than about 2 × gap.
export function floodFill(img: ImageData, sx: number, sy: number, tolerance: number, gap = 0): FillResult | null {
  const { width: w, height: h, data } = img;
  sx = Math.floor(sx);
  sy = Math.floor(sy);
  if (sx < 0 || sy < 0 || sx >= w || sy >= h) return null;

  const i0 = (sy * w + sx) * 4;
  const r0 = data[i0], g0 = data[i0 + 1], b0 = data[i0 + 2], a0 = data[i0 + 3];

  // 1. Which pixels look like the clicked color
  const open = new Uint8Array(w * h);
  for (let p = 0, i = 0; p < w * h; p++, i += 4) {
    if (a0 === 0 && data[i + 3] <= tolerance) open[p] = 1; // transparent matches transparent
    else if (
      Math.abs(data[i] - r0) <= tolerance &&
      Math.abs(data[i + 1] - g0) <= tolerance &&
      Math.abs(data[i + 2] - b0) <= tolerance &&
      Math.abs(data[i + 3] - a0) <= tolerance
    )
      open[p] = 1;
  }

  // 2. Close gaps: thicken the lines (everything that isn't open)
  let allowed = open;
  if (gap > 0) {
    allowed = open.slice();
    for (let step = 0; step < gap; step++) {
      const next = allowed.slice();
      for (let y = 0; y < h; y++) {
        for (let x = 0; x < w; x++) {
          const p = y * w + x;
          if (!allowed[p]) continue;
          if (
            (x > 0 && !allowed[p - 1]) ||
            (x < w - 1 && !allowed[p + 1]) ||
            (y > 0 && !allowed[p - w]) ||
            (y < h - 1 && !allowed[p + w])
          )
            next[p] = 0;
        }
      }
      allowed = next;
    }
    // clicked right next to a line: fall back to no gap closing
    if (!allowed[sy * w + sx]) allowed = open;
  }

  // 3. Scanline flood fill inside the allowed pixels
  const mask = new Uint8Array(w * h);
  const ok = (p: number) => allowed[p] === 1 && mask[p] === 0;
  let minX = sx, maxX = sx, minY = sy, maxY = sy;
  const stack: number[] = [sx, sy];
  while (stack.length) {
    const y = stack.pop()!;
    let x = stack.pop()!;
    let p = y * w + x;
    if (!ok(p)) continue;
    while (x > 0 && ok(p - 1)) {
      x--;
      p--;
    }
    let upOpen = false;
    let downOpen = false;
    while (x < w && ok(p)) {
      mask[p] = 1;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
      if (y > 0) {
        const up = ok(p - w);
        if (up && !upOpen) stack.push(x, y - 1);
        upOpen = up;
      }
      if (y < h - 1) {
        const dn = ok(p + w);
        if (dn && !downOpen) stack.push(x, y + 1);
        downOpen = dn;
      }
      x++;
      p++;
    }
  }
  return { mask, minX, minY, maxX, maxY, open };
}

// Grows the filled area by `radius` pixels, so the fill slides under the edge of the lineart
// and no white gap is left between line and color.
export function expandMask(res: FillResult, w: number, h: number, radius: number): FillResult {
  let { mask, minX, minY, maxX, maxY } = res;
  const open = res.open;
  for (let step = 0; step < radius; step++) {
    const next = mask.slice();
    const x0 = Math.max(0, minX - 1), x1 = Math.min(w - 1, maxX + 1);
    const y0 = Math.max(0, minY - 1), y1 = Math.min(h - 1, maxY + 1);
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const p = y * w + x;
        if (mask[p]) continue;
        // Grow into line pixels from anywhere, but into open (background) pixels only from
        // filled open pixels: the color goes under the line but never comes out the other side.
        const fromOk = (q: number) => mask[q] === 1 && (!open || !open[p] || open[q] === 1);
        if (
          (x > 0 && fromOk(p - 1)) ||
          (x < w - 1 && fromOk(p + 1)) ||
          (y > 0 && fromOk(p - w)) ||
          (y < h - 1 && fromOk(p + w))
        ) {
          next[p] = 1;
        }
      }
    }
    mask = next;
    minX = x0; minY = y0; maxX = x1; maxY = y1;
  }
  return { mask, minX, minY, maxX, maxY, open };
}
