<script lang="ts">
  import { onMount } from 'svelte';

  // Document size (will be selectable when creating a new project later)
  const DOC_W = 2000;
  const DOC_H = 1400;
  const MAX_HISTORY = 50;

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
  let tool = $state<'brush' | 'eraser'>('brush');
  let color = $state('#222222');
  let size = $state(16);

  // Layers: index 0 is the bottom layer
  let layers = $state<Layer[]>([]);
  let activeId = $state(0);
  let activeLayer = $derived(layers.find((l) => l.id === activeId));

  // History
  const undoStack: HistoryEntry[] = [];
  const redoStack: HistoryEntry[] = [];
  let undoCount = $state(0);
  let redoCount = $state(0);

  // View: zoom and pan
  const MIN_ZOOM = 0.05;
  const MAX_ZOOM = 32;
  let stage = $state<HTMLElement>();
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let spaceDown = $state(false);
  let panning = $state(false);
  let panStart = { x: 0, y: 0, px: 0, py: 0 };

  let view = $state<HTMLCanvasElement>();
  let viewCtx: CanvasRenderingContext2D | undefined;
  let backupCtx: CanvasRenderingContext2D;
  let nextId = 1;

  // Stroke state
  let drawing = false;
  let last: Point = { x: 0, y: 0, p: 0.5 };
  let hasRect = false;
  let minX = 0, minY = 0, maxX = 0, maxY = 0;

  function makeLayer(): Layer {
    const c = document.createElement('canvas');
    c.width = DOC_W;
    c.height = DOC_H;
    const id = nextId++;
    return { id, name: `Layer ${id}`, canvas: c, visible: true, opacity: 1 };
  }

  const activeIndex = () => layers.findIndex((l) => l.id === activeId);

  onMount(() => {
    viewCtx = view!.getContext('2d')!;
    const backup = document.createElement('canvas');
    backup.width = DOC_W;
    backup.height = DOC_H;
    backupCtx = backup.getContext('2d', { willReadFrequently: true })!;

    const first = makeLayer();
    layers.push(first);
    activeId = first.id;

    fitToScreen();

    // Wheel must be non-passive so we can stop the page from scrolling
    stage!.addEventListener('wheel', onWheel, { passive: false });
    window.addEventListener('keydown', onKey);
    window.addEventListener('keyup', onKeyUp);
    return () => {
      stage?.removeEventListener('wheel', onWheel);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('keyup', onKeyUp);
    };
  });

  // ---------- Zoom and pan ----------

  function fitToScreen() {
    if (!stage) return;
    const r = stage.getBoundingClientRect();
    zoom = Math.min((r.width - 32) / DOC_W, (r.height - 32) / DOC_H);
    panX = (r.width - DOC_W * zoom) / 2;
    panY = (r.height - DOC_H * zoom) / 2;
  }

  // Zoom so that the point under (sx, sy) stays under the cursor
  function zoomAt(newZoom: number, sx: number, sy: number) {
    newZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, newZoom));
    const docX = (sx - panX) / zoom;
    const docY = (sy - panY) / zoom;
    zoom = newZoom;
    panX = sx - docX * zoom;
    panY = sy - docY * zoom;
  }

  function zoomCenter(factor: number) {
    if (!stage) return;
    const r = stage.getBoundingClientRect();
    zoomAt(zoom * factor, r.width / 2, r.height / 2);
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const r = stage!.getBoundingClientRect();
    const factor = Math.exp(-e.deltaY * 0.0015);
    zoomAt(zoom * factor, e.clientX - r.left, e.clientY - r.top);
  }

  // Middle mouse button or Space + drag moves the canvas
  function onPointerDown(e: PointerEvent) {
    if (e.button === 1 || spaceDown) {
      e.preventDefault();
      stage!.setPointerCapture(e.pointerId);
      panning = true;
      panStart = { x: e.clientX, y: e.clientY, px: panX, py: panY };
      return;
    }
    if (e.button !== 0) return;
    down(e);
  }

  function onPointerMove(e: PointerEvent) {
    if (panning) {
      panX = panStart.px + (e.clientX - panStart.x);
      panY = panStart.py + (e.clientY - panStart.y);
      return;
    }
    move(e);
  }

  function onPointerUp() {
    if (panning) {
      panning = false;
      return;
    }
    up();
  }

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
        ctx.globalAlpha = l.opacity;
        ctx.drawImage(l.canvas, 0, 0);
      }
      ctx.globalAlpha = 1;
    });
  }

  // ---------- Drawing ----------

  function point(e: PointerEvent): Point {
    const r = view!.getBoundingClientRect();
    return {
      x: (e.clientX - r.left) * (DOC_W / r.width),
      y: (e.clientY - r.top) * (DOC_H / r.height),
      p: e.pointerType === 'mouse' ? 0.5 : e.pressure || 0.01,
    };
  }

  function down(e: PointerEvent) {
    const layer = activeLayer;
    if (!layer || !layer.visible) return;
    stage!.setPointerCapture(e.pointerId);
    drawing = true;
    hasRect = false;

    // Copy before the stroke so undo can restore it
    backupCtx.clearRect(0, 0, DOC_W, DOC_H);
    backupCtx.drawImage(layer.canvas, 0, 0);

    last = point(e);
    stamp(layer.canvas, last);
    render();
  }

  function move(e: PointerEvent) {
    if (!drawing || !activeLayer) return;
    const events = e.getCoalescedEvents ? e.getCoalescedEvents() : [e];
    for (const ev of events) {
      const next = point(ev);
      segment(activeLayer.canvas, last, next);
      last = next;
    }
    render();
  }

  function up() {
    if (!drawing) return;
    drawing = false;
    const layer = activeLayer;
    if (!layer || !hasRect) return;

    // Undo stores only the area you drew on, not the whole layer
    const x = Math.max(0, Math.floor(minX));
    const y = Math.max(0, Math.floor(minY));
    const w = Math.min(DOC_W, Math.ceil(maxX)) - x;
    const h = Math.min(DOC_H, Math.ceil(maxY)) - y;
    if (w <= 0 || h <= 0) return;

    const before = backupCtx.getImageData(x, y, w, h);
    const after = layer.canvas.getContext('2d')!.getImageData(x, y, w, h);
    push({ type: 'pixels', layerId: layer.id, rect: { x, y, w, h }, before, after });
  }

  function stamp(target: HTMLCanvasElement, pt: Point) {
    const ctx = target.getContext('2d')!;
    const r = Math.max((size * pt.p) / 2, 0.5);
    ctx.globalCompositeOperation = tool === 'eraser' ? 'destination-out' : 'source-over';
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(pt.x, pt.y, r, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalCompositeOperation = 'source-over';
    grow(pt.x - r - 2, pt.y - r - 2, pt.x + r + 2, pt.y + r + 2);
  }

  function segment(target: HTMLCanvasElement, a: Point, b: Point) {
    const dist = Math.hypot(b.x - a.x, b.y - a.y);
    const step = Math.max(1, (size * Math.min(a.p, b.p)) / 6);
    const n = Math.ceil(dist / step);
    for (let i = 1; i <= n; i++) {
      const t = i / n;
      stamp(target, {
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
    const target = e.target as HTMLInputElement;
    if (target.tagName === 'INPUT' && target.type === 'text') return;
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (key === ' ') {
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
    }
  }

  function onKeyUp(e: KeyboardEvent) {
    if (e.key === ' ') spaceDown = false;
  }
</script>

<div class="app">
  <header class="toolbar">
    <strong>SeviathStudio</strong>
    <div class="group">
      <button class:on={tool === 'brush'} onclick={() => (tool = 'brush')}>Brush (B)</button>
      <button class:on={tool === 'eraser'} onclick={() => (tool = 'eraser')}>Eraser (E)</button>
    </div>
    <label>Color <input type="color" bind:value={color} /></label>
    <label>Size <input type="range" min="1" max="200" bind:value={size} /> {size}px</label>
    <div class="group">
      <button onclick={undo} disabled={undoCount === 0}>↶ Undo</button>
      <button onclick={redo} disabled={redoCount === 0}>↷ Redo</button>
    </div>
    <div class="group zoom">
      <button onclick={() => zoomCenter(1 / 1.25)} title="Zoom out (Ctrl -)">−</button>
      <span class="zoom-value">{Math.round(zoom * 100)}%</span>
      <button onclick={() => zoomCenter(1.25)} title="Zoom in (Ctrl +)">+</button>
      <button onclick={fitToScreen} title="Fit to screen (Ctrl 0)">Fit</button>
      <button onclick={() => zoomCenter(1 / zoom)} title="Actual size (Ctrl 1)">100%</button>
    </div>
  </header>

  <main
    class="stage"
    class:pan={spaceDown}
    class:panning
    bind:this={stage}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
  >
    <canvas
      bind:this={view}
      width={DOC_W}
      height={DOC_H}
      class:pixelated={zoom >= 2}
      style="width: {DOC_W * zoom}px; height: {DOC_H * zoom}px; transform: translate({panX}px, {panY}px);"
    ></canvas>
  </main>

  <aside class="panel">
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
    grid-template-columns: 1fr 260px;
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
    gap: 20px;
    padding: 0 16px;
    background: #1e1e24;
    border-bottom: 1px solid #333;
  }
  .group {
    display: flex;
    gap: 6px;
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
  .zoom {
    align-items: center;
    margin-left: auto;
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
    transform-origin: 0 0;
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
  }
  .panel-head {
    font-weight: 600;
  }
  .layer-list {
    flex: 1;
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
