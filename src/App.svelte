<script lang="ts">
  import { onMount } from 'svelte';
  import ColorWheel from './lib/ColorWheel.svelte';
  import BrushPanel from './lib/BrushPanel.svelte';
  import { PRESETS, settingsOf, getDab, getPaper, type BrushSettings } from './lib/brush';

  // Document size (will be selectable when creating a new project later)
  const DOC_W = 2000;
  const DOC_H = 1400;
  const MAX_HISTORY = 50;

  type Tool = 'brush' | 'eraser' | 'picker';
  type Layer = {
    id: number;
    name: string;
    canvas: HTMLCanvasElement;
    visible: boolean;
    opacity: number;
  };
  type Rect = { x: number; y: number; w: number; h: number };
  type Point = { x: number; y: number; p: number };
  type HistoryEntry =
    | { type: 'pixels'; layerId: number; rect: Rect; before: ImageData; after: ImageData }
    | { type: 'add'; layer: Layer; index: number }
    | { type: 'delete'; layer: Layer; index: number }
    | { type: 'move'; from: number; to: number };

  // Tool settings
  let tool = $state<Tool>('brush');
  let color = $state('#222222');
  let brush = $state<BrushSettings>(settingsOf(PRESETS[0]));
  let preset = $state(PRESETS[0].name);
  let altDown = $state(false);
  // Holding Alt temporarily switches to the color picker
  let activeTool = $derived<Tool>(altDown ? 'picker' : tool);

  // Layers: index 0 is the bottom layer
  let layers = $state<Layer[]>([]);
  let activeId = $state(0);
  let activeLayer = $derived(layers.find((l) => l.id === activeId));

  // History
  const undoStack: HistoryEntry[] = [];
  const redoStack: HistoryEntry[] = [];
  let undoCount = $state(0);
  let redoCount = $state(0);

  // View: zoom, pan, rotation and flip
  const MIN_ZOOM = 0.05;
  const MAX_ZOOM = 32;
  let stage = $state<HTMLElement>();
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let rotation = $state(0); // degrees
  let flipped = $state(false);
  let spaceDown = $state(false);
  let panning = $state(false);
  // Brush outline that follows the pointer
  let hover = $state({ x: 0, y: 0, on: false });
  let panStart = { x: 0, y: 0, px: 0, py: 0 };

  let view = $state<HTMLCanvasElement>();
  let viewCtx: CanvasRenderingContext2D | undefined;
  // A stroke is painted into strokeCtx first and only added to the layer when it ends.
  // That way stroke opacity works like in Photoshop/CSP (overlapping dabs don't get darker).
  let strokeCtx: CanvasRenderingContext2D;
  let scratchCtx: CanvasRenderingContext2D;
  let texturedCtx: CanvasRenderingContext2D; // the stroke with paper grain applied
  let paperPattern: CanvasPattern | null = null;
  let nextId = 1;

  // Stroke state
  let drawing = false;
  let picking = false;
  let strokeLayerId = 0;
  let strokeTool: Tool = 'brush';
  let last: Point = { x: 0, y: 0, p: 0.5 };
  let lastRaw: Point = { x: 0, y: 0, p: 0.5 };
  let ropeEnd: Point = { x: 0, y: 0, p: 0.5 }; // stabilizer: where the rope pulls the brush
  let lastEnd: { layerId: number; pt: Point } | null = null; // for Shift + click lines
  let hasRect = false;
  let minX = 0, minY = 0, maxX = 0, maxY = 0;

  function makeCanvas() {
    const c = document.createElement('canvas');
    c.width = DOC_W;
    c.height = DOC_H;
    return c;
  }

  function makeLayer(): Layer {
    const id = nextId++;
    return { id, name: `Layer ${id}`, canvas: makeCanvas(), visible: true, opacity: 1 };
  }

  const activeIndex = () => layers.findIndex((l) => l.id === activeId);

  onMount(() => {
    viewCtx = view!.getContext('2d', { willReadFrequently: true })!;
    strokeCtx = makeCanvas().getContext('2d')!;
    scratchCtx = makeCanvas().getContext('2d')!;
    texturedCtx = makeCanvas().getContext('2d')!;
    paperPattern = texturedCtx.createPattern(getPaper(), 'repeat');

    const first = makeLayer();
    layers.push(first);
    activeId = first.id;

    fitToScreen();

    // Wheel must be non-passive so we can stop the page from scrolling
    stage!.addEventListener('wheel', onWheel, { passive: false });
    window.addEventListener('keydown', onKey);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', releaseKeys);
    return () => {
      stage?.removeEventListener('wheel', onWheel);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', releaseKeys);
    };
  });

  // Redraw when layer visibility, opacity or order changes
  $effect(() => {
    for (const l of layers) {
      l.visible;
      l.opacity;
    }
    render();
  });

  let frame = 0;
  function render() {
    if (!viewCtx || frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const ctx = viewCtx!;
      ctx.globalAlpha = 1;
      ctx.fillStyle = '#ffffff';
      ctx.fillRect(0, 0, DOC_W, DOC_H);
      for (const l of layers) {
        if (!l.visible) continue;
        if (drawing && l.id === strokeLayerId) {
          // Show the layer with the unfinished stroke on top of it
          const s = scratchCtx;
          s.globalCompositeOperation = 'source-over';
          s.globalAlpha = 1;
          s.clearRect(0, 0, DOC_W, DOC_H);
          s.drawImage(l.canvas, 0, 0);
          s.globalAlpha = brush.opacity;
          s.globalCompositeOperation = strokeTool === 'eraser' ? 'destination-out' : 'source-over';
          s.drawImage(strokeSource(), 0, 0);
          s.globalCompositeOperation = 'source-over';
          s.globalAlpha = 1;
          ctx.globalAlpha = l.opacity;
          ctx.drawImage(s.canvas, 0, 0);
        } else {
          ctx.globalAlpha = l.opacity;
          ctx.drawImage(l.canvas, 0, 0);
        }
      }
      ctx.globalAlpha = 1;
    });
  }

  // ---------- View transform ----------
  // The canvas is drawn at (panX, panY), scaled by zoom, then rotated and
  // flipped around its own center. These two functions convert between
  // stage (screen) coordinates and document (pixel) coordinates.

  function docFromStage(sx: number, sy: number) {
    const cx = (DOC_W * zoom) / 2;
    const cy = (DOC_H * zoom) / 2;
    const x = sx - panX - cx;
    const y = sy - panY - cy;
    const a = (-rotation * Math.PI) / 180;
    let rx = x * Math.cos(a) - y * Math.sin(a);
    const ry = x * Math.sin(a) + y * Math.cos(a);
    if (flipped) rx = -rx;
    return { x: (rx + cx) / zoom, y: (ry + cy) / zoom };
  }

  // Where a document point lands on the stage, ignoring pan
  function stageFromDocNoPan(dx: number, dy: number) {
    const cx = (DOC_W * zoom) / 2;
    const cy = (DOC_H * zoom) / 2;
    let x = dx * zoom - cx;
    const y = dy * zoom - cy;
    if (flipped) x = -x;
    const a = (rotation * Math.PI) / 180;
    return {
      x: cx + x * Math.cos(a) - y * Math.sin(a),
      y: cy + x * Math.sin(a) + y * Math.cos(a),
    };
  }

  // Run a view change while keeping the document point under (sx, sy) in place
  function keepPoint(sx: number, sy: number, change: () => void) {
    const d = docFromStage(sx, sy);
    change();
    const p = stageFromDocNoPan(d.x, d.y);
    panX = sx - p.x;
    panY = sy - p.y;
  }

  function stageCenter() {
    const r = stage!.getBoundingClientRect();
    return { x: r.width / 2, y: r.height / 2 };
  }

  function toDoc(clientX: number, clientY: number) {
    const r = stage!.getBoundingClientRect();
    return docFromStage(clientX - r.left, clientY - r.top);
  }

  // ---------- Zoom, pan, rotate, flip ----------

  function fitToScreen() {
    if (!stage) return;
    const r = stage.getBoundingClientRect();
    const a = (rotation * Math.PI) / 180;
    const bw = Math.abs(DOC_W * Math.cos(a)) + Math.abs(DOC_H * Math.sin(a));
    const bh = Math.abs(DOC_W * Math.sin(a)) + Math.abs(DOC_H * Math.cos(a));
    zoom = Math.min((r.width - 32) / bw, (r.height - 32) / bh);
    panX = r.width / 2 - (DOC_W * zoom) / 2;
    panY = r.height / 2 - (DOC_H * zoom) / 2;
  }

  function zoomAt(newZoom: number, sx: number, sy: number) {
    keepPoint(sx, sy, () => {
      zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, newZoom));
    });
  }

  function zoomCenter(factor: number) {
    if (!stage) return;
    const c = stageCenter();
    zoomAt(zoom * factor, c.x, c.y);
  }

  function rotateAt(deg: number, sx: number, sy: number) {
    keepPoint(sx, sy, () => {
      rotation = (((rotation + deg) % 360) + 540) % 360 - 180; // keep in -180..180
    });
  }

  function rotateBy(deg: number) {
    if (!stage) return;
    const c = stageCenter();
    rotateAt(deg, c.x, c.y);
  }

  function resetRotation() {
    rotateBy(-rotation);
  }

  function toggleFlip() {
    if (!stage) return;
    const c = stageCenter();
    keepPoint(c.x, c.y, () => {
      flipped = !flipped;
    });
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const r = stage!.getBoundingClientRect();
    const sx = e.clientX - r.left;
    const sy = e.clientY - r.top;
    if (e.shiftKey) {
      // Shift + wheel rotates (some browsers report it as horizontal scroll)
      const d = e.deltaY || e.deltaX;
      rotateAt(d > 0 ? 5 : -5, sx, sy);
    } else {
      zoomAt(zoom * Math.exp(-e.deltaY * 0.0015), sx, sy);
    }
  }

  // ---------- Pointer input ----------

  // Keep receiving pointer events even if the pen leaves the canvas mid-stroke
  function capture(e: PointerEvent) {
    try {
      stage!.setPointerCapture(e.pointerId);
    } catch {
      // ignore: some synthetic or already-released pointers can't be captured
    }
  }

  function onPointerDown(e: PointerEvent) {
    // Middle mouse button or Space + drag moves the canvas
    if (e.button === 1 || spaceDown) {
      e.preventDefault();
      capture(e);
      panning = true;
      panStart = { x: e.clientX, y: e.clientY, px: panX, py: panY };
      return;
    }
    if (e.button !== 0) return;
    if (activeTool === 'picker') {
      capture(e);
      picking = true;
      pickColor(e);
      return;
    }
    down(e);
  }

  function onPointerMove(e: PointerEvent) {
    const r = stage!.getBoundingClientRect();
    hover = { x: e.clientX - r.left, y: e.clientY - r.top, on: true };
    if (panning) {
      panX = panStart.px + (e.clientX - panStart.x);
      panY = panStart.py + (e.clientY - panStart.y);
      return;
    }
    if (picking) {
      pickColor(e);
      return;
    }
    move(e);
  }

  function onPointerUp() {
    if (panning) {
      panning = false;
      return;
    }
    if (picking) {
      picking = false;
      return;
    }
    up();
  }

  // Takes the color you see on screen (all visible layers combined)
  function pickColor(e: PointerEvent) {
    const d = toDoc(e.clientX, e.clientY);
    const x = Math.floor(d.x);
    const y = Math.floor(d.y);
    if (x < 0 || y < 0 || x >= DOC_W || y >= DOC_H || !viewCtx) return;
    const [r, g, b] = viewCtx.getImageData(x, y, 1, 1).data;
    color = '#' + [r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('');
  }

  // ---------- Drawing ----------

  function point(e: PointerEvent): Point {
    const d = toDoc(e.clientX, e.clientY);
    // A mouse has no pressure, so it always paints at full size
    return { x: d.x, y: d.y, p: e.pointerType === 'mouse' ? 1 : e.pressure || 0.01 };
  }

  function down(e: PointerEvent) {
    const layer = activeLayer;
    if (!layer || !layer.visible) return;
    const pt = point(e);

    drawing = true;
    hasRect = false;
    strokeLayerId = layer.id;
    strokeTool = activeTool;
    strokeCtx.clearRect(0, 0, DOC_W, DOC_H);
    resetDabs();

    // Shift + click: straight line from where the last stroke ended
    if (e.shiftKey && lastEnd && lastEnd.layerId === layer.id) {
      stamp(lastEnd.pt);
      segment(lastEnd.pt, pt);
      finishStroke(pt);
      return;
    }

    capture(e);
    last = pt;
    lastRaw = pt;
    ropeEnd = { ...pt };
    stamp(pt);
    render();
  }

  function move(e: PointerEvent) {
    if (!drawing) return;
    let events = e.getCoalescedEvents ? e.getCoalescedEvents() : [];
    if (events.length === 0) events = [e];
    // Stabilizer ("lazy rope", like Krita and CSP): the brush hangs behind the pen on an
    // invisible rope and only moves when the pen pulls the rope tight. Shaky movements
    // smaller than the rope are ignored, so curves come out smooth.
    // The rope length is measured on screen, so it feels the same at any zoom.
    const s = brush.stabilize;
    const rope = (Math.pow(s, 1.5) * 90) / zoom;
    const k = 1 - s * 0.6; // extra light smoothing that rounds off corners
    for (const ev of events) {
      const raw = point(ev);
      lastRaw = raw;
      const dx = raw.x - ropeEnd.x;
      const dy = raw.y - ropeEnd.y;
      const d = Math.hypot(dx, dy);
      // pressure is smoothed but never waits for the rope
      ropeEnd.p += (raw.p - ropeEnd.p) * Math.max(k, 0.3);
      if (d > rope) {
        const t = (d - rope) / d;
        ropeEnd.x += dx * t;
        ropeEnd.y += dy * t;
      } else if (rope > 0) {
        continue;
      }
      const next = {
        x: last.x + (ropeEnd.x - last.x) * k,
        y: last.y + (ropeEnd.y - last.y) * k,
        p: ropeEnd.p,
      };
      segment(last, next);
      last = next;
    }
    render();
  }

  function up() {
    if (!drawing) return;
    // Let the stabilized line catch up with where the pen actually stopped
    if (brush.stabilize > 0) segment(last, lastRaw);
    finishStroke(lastRaw);
  }

  // Adds the finished stroke to the layer and records it for undo
  function finishStroke(endPt: Point) {
    drawing = false;
    shapeEnds();
    const layer = layers.find((l) => l.id === strokeLayerId);
    if (!layer || !hasRect) {
      render();
      return;
    }

    const x = Math.max(0, Math.floor(minX));
    const y = Math.max(0, Math.floor(minY));
    const w = Math.min(DOC_W, Math.ceil(maxX)) - x;
    const h = Math.min(DOC_H, Math.ceil(maxY)) - y;
    if (w <= 0 || h <= 0) {
      render();
      return;
    }

    // Undo stores only the area you drew on, not the whole layer
    const ctx = layer.canvas.getContext('2d', { willReadFrequently: true })!;
    const before = ctx.getImageData(x, y, w, h);
    ctx.globalAlpha = brush.opacity;
    ctx.globalCompositeOperation = strokeTool === 'eraser' ? 'destination-out' : 'source-over';
    ctx.drawImage(strokeSource(), 0, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
    const after = ctx.getImageData(x, y, w, h);

    push({ type: 'pixels', layerId: layer.id, rect: { x, y, w, h }, before, after });
    lastEnd = { layerId: layer.id, pt: endPt };
    strokeCtx.clearRect(0, 0, DOC_W, DOC_H);
    render();
  }

  function radiusAt(p: number) {
    const factor = brush.sizePressure ? brush.minSize + (1 - brush.minSize) * p : 1;
    return Math.max((brush.size * factor) / 2, 0.5);
  }

  // The stroke as it should be added to the layer: with paper grain worn into it if the
  // brush has texture. The grain pattern is fixed to the canvas, so it never slides.
  function strokeSource(): HTMLCanvasElement {
    if (brush.texture <= 0 || !paperPattern) return strokeCtx.canvas;
    const t = texturedCtx;
    t.globalCompositeOperation = 'source-over';
    t.globalAlpha = 1;
    t.clearRect(0, 0, DOC_W, DOC_H);
    t.drawImage(strokeCtx.canvas, 0, 0);
    t.globalCompositeOperation = 'destination-out';
    t.globalAlpha = brush.texture;
    t.fillStyle = paperPattern;
    t.fillRect(0, 0, DOC_W, DOC_H);
    t.globalCompositeOperation = 'source-over';
    t.globalAlpha = 1;
    return t.canvas;
  }

  // ---------- Dabs and stroke ends ----------
  // Every dab of the current stroke is remembered with its distance from the start.
  // That lets us taper the end of the line once we know how long the whole stroke is.

  type Dab = { x: number; y: number; p: number; d: number };
  let dabs: Dab[] = [];
  let strokeLen = 0;

  function resetDabs() {
    dabs = [];
    strokeLen = 0;
  }

  function stamp(pt: Point) {
    const prev = dabs[dabs.length - 1];
    if (prev) strokeLen += Math.hypot(pt.x - prev.x, pt.y - prev.y);
    const dab = { ...pt, d: strokeLen };
    dabs.push(dab);
    // While drawing only the start can be tapered; the end is tapered when the stroke ends
    drawDab(dab, taperAt(dab.d, Infinity));
  }

  function drawDab(pt: Point, factor: number) {
    const r = Math.max(radiusAt(pt.p) * factor, 0.3);
    const tip = getDab(strokeTool === 'eraser' ? '#000000' : color, brush.hardness);
    // Light pressure still leaves some paint (25%), like a real pencil or pen
    strokeCtx.globalAlpha = brush.flow * (brush.opacityPressure ? 0.25 + 0.75 * pt.p : 1);
    strokeCtx.drawImage(tip, pt.x - r, pt.y - r, r * 2, r * 2);
    strokeCtx.globalAlpha = 1;
    grow(pt.x - r - 2, pt.y - r - 2, pt.x + r + 2, pt.y + r + 2);
  }

  function taperLength() {
    return Math.max(4, brush.taper * brush.size * 15);
  }

  // 1 = full size, smaller towards the tapered tips
  function taperAt(d: number, total: number) {
    if (brush.ends !== 'taper') return 1;
    const L = taperLength();
    const ease = (t: number) => 1 - Math.pow(1 - Math.min(Math.max(t, 0), 1), 2);
    const tipSize = 0.06;
    let f = tipSize + (1 - tipSize) * ease(d / L);
    if (isFinite(total)) f = Math.min(f, tipSize + (1 - tipSize) * ease((total - d) / L));
    return f;
  }

  // Called when the stroke ends: applies the end shape to the whole stroke
  function shapeEnds() {
    if (dabs.length < 2) return;
    if (brush.ends === 'taper') {
      // Redraw the whole stroke with both ends tapered, filling gaps where dabs got small
      strokeCtx.clearRect(0, 0, DOC_W, DOC_H);
      const total = strokeLen;
      drawDab(dabs[0], taperAt(0, total));
      for (let i = 1; i < dabs.length; i++) {
        const a = dabs[i - 1];
        const b = dabs[i];
        const ra = radiusAt(a.p) * taperAt(a.d, total);
        const rb = radiusAt(b.p) * taperAt(b.d, total);
        const step = Math.max(0.3, Math.min(ra, rb) * 2 * brush.spacing);
        const n = Math.max(1, Math.ceil(Math.hypot(b.x - a.x, b.y - a.y) / step));
        for (let j = 1; j <= n; j++) {
          const t = j / n;
          const d = a.d + (b.d - a.d) * t;
          drawDab({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, p: a.p + (b.p - a.p) * t }, taperAt(d, total));
        }
      }
    } else if (brush.ends === 'flat') {
      cutFlat(dabs[0], dabs);
      cutFlat(dabs[dabs.length - 1], [...dabs].reverse());
    }
  }

  // Cuts off the round cap beyond an end point, along the stroke direction
  function cutFlat(end: Dab, fromEnd: Dab[]) {
    const r = radiusAt(end.p);
    // look a little way into the stroke to find its direction at this end
    const inner = fromEnd.find((q) => Math.hypot(q.x - end.x, q.y - end.y) >= Math.max(r, 2)) ?? fromEnd[fromEnd.length - 1];
    const dx = end.x - inner.x;
    const dy = end.y - inner.y;
    if (dx === 0 && dy === 0) return;
    const R = r + 3;
    strokeCtx.save();
    strokeCtx.globalCompositeOperation = 'destination-out';
    strokeCtx.translate(end.x, end.y);
    strokeCtx.rotate(Math.atan2(dy, dx));
    strokeCtx.fillRect(0, -R, R, R * 2);
    strokeCtx.restore();
  }

  // Places dabs evenly between two points, blending size and pressure along the way
  function segment(a: Point, b: Point) {
    const dist = Math.hypot(b.x - a.x, b.y - a.y);
    const r = radiusAt(Math.min(a.p, b.p)) * taperAt(strokeLen, Infinity);
    const step = Math.max(0.3, r * 2 * brush.spacing);
    const n = Math.ceil(dist / step);
    for (let i = 1; i <= n; i++) {
      const t = i / n;
      stamp({
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        p: a.p + (b.p - a.p) * t,
      });
    }
  }

  function grow(x0: number, y0: number, x1: number, y1: number) {
    if (!hasRect) {
      minX = x0; minY = y0; maxX = x1; maxY = y1;
      hasRect = true;
    } else {
      minX = Math.min(minX, x0); minY = Math.min(minY, y0);
      maxX = Math.max(maxX, x1); maxY = Math.max(maxY, y1);
    }
  }

  // ---------- Layers ----------

  function insertAt(layer: Layer, index: number) {
    layers.splice(index, 0, layer);
    activeId = layer.id;
  }

  function removeAt(index: number) {
    const [removed] = layers.splice(index, 1);
    if (removed && removed.id === activeId) {
      activeId = layers[Math.min(index, layers.length - 1)]?.id ?? 0;
    }
  }

  function moveRaw(from: number, to: number) {
    const [l] = layers.splice(from, 1);
    layers.splice(to, 0, l);
  }

  function addLayer() {
    const l = makeLayer();
    const index = activeIndex() + 1;
    insertAt(l, index);
    push({ type: 'add', layer: l, index });
  }

  function deleteLayer() {
    if (layers.length <= 1) return;
    const index = activeIndex();
    const l = layers[index];
    const copy: Layer = { id: l.id, name: l.name, canvas: l.canvas, visible: l.visible, opacity: l.opacity };
    removeAt(index);
    push({ type: 'delete', layer: copy, index });
  }

  // +1 = up, -1 = down
  function moveLayer(dir: 1 | -1) {
    const from = activeIndex();
    const to = from + dir;
    if (to < 0 || to >= layers.length) return;
    moveRaw(from, to);
    push({ type: 'move', from, to });
  }

  // ---------- Undo / redo ----------

  function push(entry: HistoryEntry) {
    undoStack.push(entry);
    if (undoStack.length > MAX_HISTORY) undoStack.shift();
    redoStack.length = 0;
    sync();
  }

  function sync() {
    undoCount = undoStack.length;
    redoCount = redoStack.length;
  }

  function apply(e: HistoryEntry, reverse: boolean) {
    switch (e.type) {
      case 'pixels': {
        const l = layers.find((l) => l.id === e.layerId);
        l?.canvas.getContext('2d')!.putImageData(reverse ? e.before : e.after, e.rect.x, e.rect.y);
        break;
      }
      case 'add':
        reverse ? removeAt(e.index) : insertAt(e.layer, e.index);
        break;
      case 'delete':
        reverse ? insertAt(e.layer, e.index) : removeAt(e.index);
        break;
      case 'move':
        reverse ? moveRaw(e.to, e.from) : moveRaw(e.from, e.to);
        break;
    }
  }

  function undo() {
    const e = undoStack.pop();
    if (!e) return;
    apply(e, true);
    redoStack.push(e);
    sync();
    render();
  }

  function redo() {
    const e = redoStack.pop();
    if (!e) return;
    apply(e, false);
    undoStack.push(e);
    sync();
    render();
  }

  // ---------- Shortcuts ----------

  function onKey(e: KeyboardEvent) {
    // Don't trigger shortcuts while typing in text or number fields
    const target = e.target as HTMLInputElement;
    if (target.tagName === 'INPUT' && (target.type === 'text' || target.type === 'number')) return;
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();

    if (key === 'alt') {
      e.preventDefault();
      altDown = true;
    } else if (key === ' ') {
      e.preventDefault();
      spaceDown = true;
    } else if (mod && key === '0') {
      e.preventDefault();
      fitToScreen();
    } else if (mod && key === '1') {
      e.preventDefault();
      zoomCenter(1 / zoom);
    } else if (mod && (key === '=' || key === '+')) {
      e.preventDefault();
      zoomCenter(1.25);
    } else if (mod && key === '-') {
      e.preventDefault();
      zoomCenter(1 / 1.25);
    } else if (mod && key === 'z' && !e.shiftKey) {
      e.preventDefault();
      undo();
    } else if (mod && (key === 'y' || (key === 'z' && e.shiftKey))) {
      e.preventDefault();
      redo();
    } else if (!mod && key === 'b') {
      tool = 'brush';
    } else if (!mod && key === 'e') {
      tool = 'eraser';
    } else if (!mod && key === 'i') {
      tool = 'picker';
    } else if (!mod && key === '4') {
      rotateBy(-15);
    } else if (!mod && key === '6') {
      rotateBy(15);
    } else if (!mod && key === '5') {
      resetRotation();
    } else if (!mod && key === 'm') {
      toggleFlip();
    } else if (!mod && key === '[') {
      brush.size = stepSize(brush.size, -1);
    } else if (!mod && key === ']') {
      brush.size = stepSize(brush.size, 1);
    }
  }

  // [ and ] change the size by ~10%, but always by at least 0.1 px
  function stepSize(size: number, dir: 1 | -1) {
    let next = dir > 0 ? Math.max(size + 0.1, size * 1.1) : Math.min(size - 0.1, size / 1.1);
    next = Math.min(500, Math.max(0.5, next));
    if (next < 10) return Math.round(next * 10) / 10;
    if (next < 50) return Math.round(next * 2) / 2;
    return Math.round(next);
  }

  function onKeyUp(e: KeyboardEvent) {
    if (e.key === ' ') spaceDown = false;
    if (e.key === 'Alt') {
      e.preventDefault();
      altDown = false;
    }
  }

  // Keys held while switching windows would otherwise stay "stuck"
  function releaseKeys() {
    spaceDown = false;
    altDown = false;
  }
</script>

<div class="app">
  <header class="toolbar">
    <strong>SeviathStudio</strong>
    <div class="group">
      <button class:on={activeTool === 'brush'} onclick={() => (tool = 'brush')}>Brush (B)</button>
      <button class:on={activeTool === 'eraser'} onclick={() => (tool = 'eraser')}>Eraser (E)</button>
      <button class:on={activeTool === 'picker'} onclick={() => (tool = 'picker')} title="Color picker (I, or hold Alt)">Picker (I)</button>
    </div>
    <div class="current-color" style="background: {color}" title={color}></div>
    <span class="brush-name">Brush: {preset} · {brush.size}px</span>
    <div class="group">
      <button onclick={undo} disabled={undoCount === 0}>↶ Undo</button>
      <button onclick={redo} disabled={redoCount === 0}>↷ Redo</button>
    </div>

    <div class="group view">
      <button onclick={() => rotateBy(-15)} title="Rotate left (4)">⟲</button>
      <button class="value" onclick={resetRotation} title="Reset rotation (5)">{Math.round(rotation)}°</button>
      <button onclick={() => rotateBy(15)} title="Rotate right (6)">⟳</button>
      <button class:on={flipped} onclick={toggleFlip} title="Flip horizontally (M)">⇋ Flip</button>
    </div>
    <div class="group">
      <span class="zoom-label">Zoom</span>
      <button onclick={() => zoomCenter(1 / 1.25)} title="Zoom out (Ctrl -)">−</button>
      <input
        class="zoom-slider"
        type="range"
        title="Canvas zoom (mouse wheel)"
        min="-4.32"
        max="5"
        step="0.01"
        value={Math.log2(zoom)}
        oninput={(e) => {
          const c = stageCenter();
          zoomAt(2 ** +e.currentTarget.value, c.x, c.y);
        }}
      />
      <span class="zoom-value">{Math.round(zoom * 100)}%</span>
      <button onclick={() => zoomCenter(1.25)} title="Zoom in (Ctrl +)">+</button>
      <button onclick={fitToScreen} title="Fit to screen (Ctrl 0)">Fit</button>
      <button onclick={() => zoomCenter(1 / zoom)} title="Actual size (Ctrl 1)">100%</button>
    </div>
  </header>

  <aside class="panel left">
    <div class="panel-head">Brush</div>
    <BrushPanel bind:brush bind:preset />
  </aside>

  <main
    class="stage"
    class:pan={spaceDown}
    class:panning
    class:picker={activeTool === 'picker'}
    bind:this={stage}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    onpointerleave={() => (hover.on = false)}
  >
    <canvas
      bind:this={view}
      width={DOC_W}
      height={DOC_H}
      class:pixelated={zoom >= 2}
      style="width: {DOC_W * zoom}px; height: {DOC_H * zoom}px; transform-origin: {(DOC_W * zoom) / 2}px {(DOC_H * zoom) / 2}px; transform: translate({panX}px, {panY}px) rotate({rotation}deg) scaleX({flipped ? -1 : 1});"
    ></canvas>
    {#if hover.on && !spaceDown && !panning && activeTool !== 'picker'}
      {@const d = Math.max(brush.size * zoom, 4)}
      <div
        class="brush-cursor"
        style="left: {hover.x - d / 2}px; top: {hover.y - d / 2}px; width: {d}px; height: {d}px;"
      ></div>
    {/if}
  </main>

  <aside class="panel">
    <div class="panel-head">Color</div>
    <ColorWheel bind:color />

    <div class="panel-head">Layers</div>

    <div class="layer-list">
      {#each [...layers].reverse() as layer (layer.id)}
        <div
          class="layer"
          class:active={layer.id === activeId}
          role="button"
          tabindex="0"
          onclick={() => (activeId = layer.id)}
          onkeydown={(e) => e.key === 'Enter' && (activeId = layer.id)}
        >
          <button
            class="eye"
            title="Show / hide"
            onclick={(e) => {
              e.stopPropagation();
              layer.visible = !layer.visible;
            }}>{layer.visible ? '👁' : '—'}</button
          >
          <input class="name" type="text" bind:value={layer.name} />
          <span class="pct">{Math.round(layer.opacity * 100)}%</span>
        </div>
      {/each}
    </div>

    {#if activeLayer}
      <label class="opacity">
        Opacity
        <input
          type="range"
          min="0"
          max="1"
          step="0.01"
          value={activeLayer.opacity}
          oninput={(e) => (activeLayer!.opacity = +e.currentTarget.value)}
        />
      </label>
    {/if}

    <div class="layer-buttons">
      <button onclick={addLayer} title="New layer">＋</button>
      <button onclick={() => moveLayer(1)} title="Move up">↑</button>
      <button onclick={() => moveLayer(-1)} title="Move down">↓</button>
      <button onclick={deleteLayer} disabled={layers.length <= 1} title="Delete layer">🗑</button>
    </div>
  </aside>
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: 220px 1fr 248px;
    grid-template-rows: 56px 1fr;
    height: 100vh;
    font-family: system-ui, sans-serif;
    font-size: 14px;
    color: #eee;
  }
  .toolbar {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 16px;
    background: #1e1e24;
    border-bottom: 1px solid #333;
    overflow-x: auto;
    white-space: nowrap;
  }
  .group {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .view {
    margin-left: auto;
  }
  .value {
    min-width: 52px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  button {
    padding: 6px 10px;
    border-radius: 6px;
    border: 1px solid #444;
    background: #2b2b33;
    color: #eee;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  button.on {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
  }
  .current-color {
    width: 28px;
    height: 28px;
    border-radius: 6px;
    border: 1px solid #555;
    flex-shrink: 0;
  }
  .zoom-label {
    color: #aaa;
  }
  .zoom-slider {
    width: 110px;
  }
  .brush-cursor {
    position: absolute;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.9);
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6);
    pointer-events: none;
  }
  .zoom-value {
    min-width: 48px;
    text-align: center;
    color: #bbb;
  }
  .stage {
    position: relative;
    background: #15151a;
    overflow: hidden;
    min-height: 0;
    touch-action: none;
    cursor: crosshair;
  }
  .stage.picker {
    cursor: copy;
  }
  .stage.pan {
    cursor: grab;
  }
  .stage.panning {
    cursor: grabbing;
  }
  canvas {
    position: absolute;
    top: 0;
    left: 0;
    background: #fff;
    box-shadow: 0 0 0 1px #333, 0 8px 24px rgba(0, 0, 0, 0.5);
  }
  canvas.pixelated {
    image-rendering: pixelated;
  }
  .panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    background: #1e1e24;
    border-left: 1px solid #333;
    min-height: 0;
    overflow-y: auto;
  }
  .panel.left {
    border-left: none;
    border-right: 1px solid #333;
  }
  .panel-head {
    font-weight: 600;
  }
  .brush-name {
    color: #aaa;
    min-width: 120px;
  }
  .layer-list {
    flex: 1;
    min-height: 80px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .layer {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px;
    border-radius: 6px;
    background: #26262e;
    cursor: pointer;
    border: 1px solid transparent;
  }
  .layer.active {
    border-color: #00b5c8;
    background: #2c3438;
  }
  .eye {
    width: 32px;
    padding: 4px;
  }
  .name {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    color: #eee;
    font: inherit;
  }
  .pct {
    color: #999;
    font-size: 12px;
  }
  .opacity {
    flex-direction: column;
    align-items: stretch;
    gap: 4px;
  }
  .layer-buttons {
    display: flex;
    gap: 6px;
  }
  .layer-buttons button {
    flex: 1;
  }
</style>
