<script lang="ts">
  import { onMount } from 'svelte';
  import ColorWheel from './lib/ColorWheel.svelte';
  import BrushPanel from './lib/BrushPanel.svelte';
  import AdjustColors from './lib/AdjustColors.svelte';
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
    blend: BlendMode;
    clip: boolean; // clipping mask: only shows on top of the layer below
    lockAlpha: boolean; // paint only where the layer already has paint
  };
  type Rect = { x: number; y: number; w: number; h: number };

  // Layer blend modes and the canvas operation that draws each one
  const BLEND_MODES = {
    Normal: 'source-over',
    Multiply: 'multiply',
    Screen: 'screen',
    Overlay: 'overlay',
    Add: 'lighter',
    'Soft light': 'soft-light',
    'Hard light': 'hard-light',
    'Color dodge': 'color-dodge',
    'Color burn': 'color-burn',
    Darken: 'darken',
    Lighten: 'lighten',
    Difference: 'difference',
    Exclusion: 'exclusion',
    Hue: 'hue',
    Saturation: 'saturation',
    Color: 'color',
    Luminosity: 'luminosity',
  } as const;
  type BlendMode = keyof typeof BLEND_MODES;
  const BLEND_NAMES = Object.keys(BLEND_MODES) as BlendMode[];
  const opOf = (b: BlendMode) => BLEND_MODES[b] as GlobalCompositeOperation;

  // What each mode does, shown under the blend menu
  const BLEND_HINTS: Record<BlendMode, string> = {
    Normal: 'Covers what is below.',
    Multiply: 'Darkens. Good for shadows. White does nothing.',
    Screen: 'Lightens. Good for light. Black does nothing.',
    Overlay: 'Light colors lighten, dark colors darken. 50% gray does nothing.',
    Add: 'Strong glow. Use a light color on a dark area.',
    'Soft light': 'Gentle Overlay: soft light and shade.',
    'Hard light': 'Strong Overlay, based on this layer\'s colors.',
    'Color dodge': 'Bright glow and shine. Use a mid-to-light color.',
    'Color burn': 'Deep, saturated shadows.',
    Darken: 'Picks the darker of the two colors. The same color changes nothing; for shadows use Multiply.',
    Lighten: 'Picks the lighter of the two colors. The same color changes nothing; for light use Screen or Add.',
    Difference: 'Subtracts colors. Mostly for effects.',
    Exclusion: 'Softer Difference.',
    Hue: 'Takes only the hue from this layer.',
    Saturation: 'Takes only the saturation from this layer.',
    Color: 'Recolors what is below, keeps its shading.',
    Luminosity: 'Takes only the brightness from this layer.',
  };
  type Point = { x: number; y: number; p: number };
  type HistoryEntry =
    | { type: 'pixels'; layerId: number; rect: Rect; before: ImageData; after: ImageData }
    | { type: 'add'; layer: Layer; index: number }
    | { type: 'delete'; layer: Layer; index: number }
    | { type: 'move'; from: number; to: number }
    | { type: 'merge'; upper: Layer; upperIndex: number; lowerId: number; before: ImageData; after: ImageData }
    | { type: 'paper'; before: PaperState; after: PaperState }
    | { type: 'layerProps'; layerId: number; before: LayerProps; after: LayerProps };

  type PaperState = { exists: boolean; visible: boolean; color: string };
  // The settings of a layer that can be undone (everything except its pixels)
  type LayerProps = Pick<Layer, 'name' | 'visible' | 'opacity' | 'blend' | 'clip' | 'lockAlpha'>;

  // Tool settings
  let tool = $state<Tool>('brush');
  let color = $state('#222222');
  let brush = $state<BrushSettings>(settingsOf(PRESETS[0]));
  let preset = $state(PRESETS[0].name);
  let altDown = $state(false);
  // Holding Alt temporarily switches to the color picker
  let activeTool = $derived<Tool>(altDown ? 'picker' : tool);

  // Paper: the background under all layers. Hide it to see (and later export) transparency.
  let paper = $state({ exists: true, visible: true, color: '#ffffff' });
  const PAPER_ID = -1; // activeId when the paper is selected
  // A short message shown over the canvas (e.g. "can't draw on paper")
  let notice = $state('');
  let noticeTimer = 0;
  function deletePaper() {
    changePaper(() => (paper.exists = false));
    if (activeId === PAPER_ID) activeId = layers[layers.length - 1]?.id ?? 0;
  }

  function addPaper() {
    changePaper(() => {
      paper.exists = true;
      paper.visible = true;
    });
    activeId = PAPER_ID;
  }

  // ---------- Undoable settings changes ----------
  // Each change is recorded as "before" and "after", so undo can switch back.

  const paperState = (): PaperState => ({ exists: paper.exists, visible: paper.visible, color: paper.color });
  const propsOf = (l: Layer): LayerProps => ({
    name: l.name,
    visible: l.visible,
    opacity: l.opacity,
    blend: l.blend,
    clip: l.clip,
    lockAlpha: l.lockAlpha,
  });
  const same = (a: object, b: object) => JSON.stringify(a) === JSON.stringify(b);

  function changePaper(change: () => void) {
    const before = paperState();
    change();
    const after = paperState();
    if (!same(before, after)) push({ type: 'paper', before, after });
  }

  function changeLayer(layer: Layer | undefined, change: () => void) {
    if (!layer) return;
    const before = propsOf(layer);
    change();
    const after = propsOf(layer);
    if (!same(before, after)) push({ type: 'layerProps', layerId: layer.id, before, after });
  }

  // Sliders and color pickers change continuously while dragging: remember the value
  // when the drag starts, and record one undo step when it ends.
  let dragStart: { kind: 'paper'; state: PaperState } | { kind: 'layer'; id: number; props: LayerProps } | null = null;

  function beginPaperDrag() {
    if (!dragStart) dragStart = { kind: 'paper', state: paperState() };
  }
  function endPaperDrag() {
    if (dragStart?.kind === 'paper') {
      const after = paperState();
      if (!same(dragStart.state, after)) push({ type: 'paper', before: dragStart.state, after });
    }
    dragStart = null;
  }
  function beginLayerDrag(layer: Layer) {
    if (!dragStart) dragStart = { kind: 'layer', id: layer.id, props: propsOf(layer) };
  }
  function endLayerDrag() {
    if (dragStart?.kind === 'layer') {
      const l = layers.find((x) => x.id === (dragStart as { id: number }).id);
      if (l) {
        const after = propsOf(l);
        if (!same(dragStart.props, after)) push({ type: 'layerProps', layerId: l.id, before: dragStart.props, after });
      }
    }
    dragStart = null;
  }

  function showNotice(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = window.setTimeout(() => (notice = ''), 2200);
  }

  // Layers: index 0 is the bottom layer
  let layers = $state<Layer[]>([]);
  let activeId = $state(0);
  let activeLayer = $derived(layers.find((l) => l.id === activeId));

  // History
  const undoStack: HistoryEntry[] = [];
  const redoStack: HistoryEntry[] = [];
  let undoCount = $state(0);
  // Bumped whenever layer pixels change, so the layer thumbnails redraw
  let thumbTick = $state(0);
  // Which layer's name is being edited (null = none)
  let renamingId = $state<number | null>(null);
  // Adjust colors dialog for the active layer
  let adjusting = $state(false);
  // Right-click menu on a layer
  let layerMenu = $state<{ x: number; y: number } | null>(null);
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
  let groupCtx: CanvasRenderingContext2D; // a clipping group: base layer + layers clipped to it
  let clipCtx: CanvasRenderingContext2D; // one clipped layer, cut to the base's shape
  let texturedCtx: CanvasRenderingContext2D; // the stroke with paper grain applied
  let backupCtx: CanvasRenderingContext2D; // the layer before a color-mixing stroke (for undo)
  let tintCtx: CanvasRenderingContext2D; // small canvas used to color one brush tip
  let paperPattern: CanvasPattern | null = null;
  let nextId = 1;

  // Stroke state
  let drawing = false;
  let picking = false;
  let strokeLayerId = 0;
  let strokeTool: Tool = 'brush';
  // Color-mixing brushes paint straight onto the layer so they can pick up its colors
  let directStroke = false;
  let mixRGB = [0, 0, 0];
  let lastSample = { x: -1e9, y: -1e9 };
  // The paper grain shifts a little for every new stroke, so painting over an area fills it in
  let grainOffset = { x: 0, y: 0 };
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
    return {
      id,
      name: `Layer ${id}`,
      canvas: makeCanvas(),
      visible: true,
      opacity: 1,
      blend: 'Normal',
      clip: false,
      lockAlpha: false,
    };
  }

  // A plain copy of a layer's settings (history keeps these, not the live proxies)
  const copyLayer = (l: Layer): Layer => ({ ...l });

  const activeIndex = () => layers.findIndex((l) => l.id === activeId);

  onMount(() => {
    viewCtx = view!.getContext('2d', { willReadFrequently: true })!;
    strokeCtx = makeCanvas().getContext('2d')!;
    scratchCtx = makeCanvas().getContext('2d')!;
    groupCtx = makeCanvas().getContext('2d')!;
    clipCtx = makeCanvas().getContext('2d')!;
    texturedCtx = makeCanvas().getContext('2d')!;
    backupCtx = makeCanvas().getContext('2d', { willReadFrequently: true })!;
    tintCtx = document.createElement('canvas').getContext('2d')!;
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
      l.blend;
      l.clip;
    }
    paper.exists;
    paper.visible;
    paper.color;
    render();
  });

  let frame = 0;
  function render() {
    if (!viewCtx || frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const ctx = viewCtx!;
      const strokeView = drawing && !directStroke ? previewStrokeLayer() : null;
      const surface = (l: Layer) => (strokeView && l.id === strokeLayerId ? strokeView : l.canvas);

      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = 'source-over';
      if (paper.exists && paper.visible) {
        ctx.fillStyle = paper.color;
        ctx.fillRect(0, 0, DOC_W, DOC_H);
      } else {
        // transparent: the checkerboard behind the canvas shows through
        ctx.clearRect(0, 0, DOC_W, DOC_H);
      }

      // Walk up the stack; each normal layer may have clipped layers right above it
      let i = 0;
      while (i < layers.length) {
        const base = layers[i];
        let j = i + 1;
        while (j < layers.length && layers[j].clip) j++;
        const clipped = layers.slice(i + 1, j).filter((c) => c.visible);

        if (base.visible) {
          let image: CanvasImageSource = surface(base);
          if (clipped.length) {
            const g = groupCtx;
            g.globalAlpha = 1;
            g.globalCompositeOperation = 'source-over';
            g.clearRect(0, 0, DOC_W, DOC_H);
            g.drawImage(image, 0, 0);
            for (const c of clipped) {
              // cut the clipped layer to the base layer's shape, then blend it on
              const t = clipCtx;
              t.globalCompositeOperation = 'source-over';
              t.clearRect(0, 0, DOC_W, DOC_H);
              t.drawImage(surface(c), 0, 0);
              t.globalCompositeOperation = 'destination-in';
              t.drawImage(image, 0, 0);
              t.globalCompositeOperation = 'source-over';
              g.globalAlpha = c.opacity;
              g.globalCompositeOperation = opOf(c.blend);
              g.drawImage(t.canvas, 0, 0);
            }
            g.globalAlpha = 1;
            g.globalCompositeOperation = 'source-over';
            image = g.canvas;
          }
          ctx.globalAlpha = base.opacity;
          ctx.globalCompositeOperation = opOf(base.blend);
          ctx.drawImage(image, 0, 0);
        }
        i = j;
      }
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = 'source-over';
    });
  }

  // The layer being painted on, with the unfinished stroke shown on top of it
  function previewStrokeLayer(): HTMLCanvasElement | null {
    const l = layers.find((x) => x.id === strokeLayerId);
    if (!l) return null;
    const s = scratchCtx;
    s.globalCompositeOperation = 'source-over';
    s.globalAlpha = 1;
    s.clearRect(0, 0, DOC_W, DOC_H);
    s.drawImage(l.canvas, 0, 0);
    s.globalAlpha = brush.opacity;
    s.globalCompositeOperation = strokeTool === 'eraser' ? 'destination-out' : 'source-over';
    s.drawImage(strokeSource(l), 0, 0);
    s.globalCompositeOperation = 'source-over';
    s.globalAlpha = 1;
    return s.canvas;
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
    const [r, g, b, a] = viewCtx.getImageData(x, y, 1, 1).data;
    if (a === 0) return; // nothing there (transparent)
    color = '#' + [r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('');
  }

  // ---------- Drawing ----------

  function point(e: PointerEvent): Point {
    const d = toDoc(e.clientX, e.clientY);
    // A mouse has no pressure, so it always paints at full size
    return { x: d.x, y: d.y, p: e.pointerType === 'mouse' ? 1 : e.pressure || 0.01 };
  }

  function down(e: PointerEvent) {
    if (activeId === PAPER_ID) {
      showNotice("Paper can't be drawn on. Select a layer or press ＋ to add one.");
      return;
    }
    const layer = activeLayer;
    if (!layer) return;
    if (!layer.visible) {
      showNotice('This layer is hidden. Show it (👁) to draw on it.');
      return;
    }
    const pt = point(e);

    drawing = true;
    hasRect = false;
    strokeLayerId = layer.id;
    strokeTool = activeTool;
    strokeCtx.clearRect(0, 0, DOC_W, DOC_H);
    resetDabs();
    grainOffset = { x: Math.random() * 256, y: Math.random() * 256 };

    directStroke = strokeTool === 'brush' && brush.mix > 0;
    if (directStroke) {
      backupCtx.clearRect(0, 0, DOC_W, DOC_H);
      backupCtx.drawImage(layer.canvas, 0, 0);
      mixRGB = hexRGB(color);
      lastSample = { x: -1e9, y: -1e9 };
      // start with the color that's already under the brush
      sampleCanvas(layer, pt, brush.mix);
    }

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
    if (!directStroke) shapeEnds();
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
    let before: ImageData;
    if (directStroke) {
      // already painted onto the layer; the backup holds how it was before
      before = backupCtx.getImageData(x, y, w, h);
      if (layer.lockAlpha) {
        ctx.globalCompositeOperation = 'destination-in';
        ctx.drawImage(backupCtx.canvas, 0, 0);
        ctx.globalCompositeOperation = 'source-over';
      }
    } else {
      before = ctx.getImageData(x, y, w, h);
      ctx.globalAlpha = brush.opacity;
      ctx.globalCompositeOperation = strokeTool === 'eraser' ? 'destination-out' : 'source-over';
      ctx.drawImage(strokeSource(layer), 0, 0);
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = 'source-over';
    }
    const after = ctx.getImageData(x, y, w, h);
    directStroke = false;

    push({ type: 'pixels', layerId: layer.id, rect: { x, y, w, h }, before, after });
    thumbTick++;
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
  function strokeSource(layer: Layer): HTMLCanvasElement {
    const useTexture = brush.texture > 0 && paperPattern;
    const useLock = layer.lockAlpha && strokeTool !== 'eraser';
    if (!useTexture && !useLock) return strokeCtx.canvas;
    const t = texturedCtx;
    t.globalCompositeOperation = 'source-over';
    t.globalAlpha = 1;
    t.clearRect(0, 0, DOC_W, DOC_H);
    t.drawImage(strokeCtx.canvas, 0, 0);
    if (useTexture) {
      t.globalCompositeOperation = 'destination-out';
      t.globalAlpha = brush.texture;
      paperPattern!.setTransform(new DOMMatrix().translate(grainOffset.x, grainOffset.y));
      t.fillStyle = paperPattern!;
      t.fillRect(0, 0, DOC_W, DOC_H);
      t.globalAlpha = 1;
    }
    if (useLock) {
      // Lock alpha: keep the stroke only where the layer already has paint
      t.globalCompositeOperation = 'destination-in';
      t.drawImage(layer.canvas, 0, 0);
    }
    t.globalCompositeOperation = 'source-over';
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
    if (directStroke) {
      drawMixDab(pt, factor);
      return;
    }
    const r = Math.max(radiusAt(pt.p) * factor, 0.3);
    const tip = getDab(strokeTool === 'eraser' ? '#000000' : color, brush.hardness);
    // Light pressure still leaves some paint (25%), like a real pencil or pen
    strokeCtx.globalAlpha = brush.flow * (brush.opacityPressure ? 0.25 + 0.75 * pt.p : 1);
    strokeCtx.drawImage(tip, pt.x - r, pt.y - r, r * 2, r * 2);
    strokeCtx.globalAlpha = 1;
    grow(pt.x - r - 2, pt.y - r - 2, pt.x + r + 2, pt.y + r + 2);
  }

  // ---------- Color mixing ----------

  function hexRGB(hex: string) {
    const n = parseInt(hex.replace('#', ''), 16) || 0;
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }

  // Reads the average color under the brush and mixes it into the paint on the brush
  function sampleCanvas(layer: Layer, pt: Point, amount: number) {
    const ctx = layer.canvas.getContext('2d', { willReadFrequently: true })!;
    const r = Math.max(1, Math.min(4, Math.round(radiusAt(pt.p) * 0.3)));
    const x = Math.round(pt.x) - r;
    const y = Math.round(pt.y) - r;
    if (x + 2 * r < 0 || y + 2 * r < 0 || x >= DOC_W || y >= DOC_H) return;
    const d = ctx.getImageData(x, y, r * 2 + 1, r * 2 + 1).data;
    let R = 0, G = 0, B = 0, A = 0;
    for (let i = 0; i < d.length; i += 4) {
      const a = d[i + 3];
      R += d[i] * a;
      G += d[i + 1] * a;
      B += d[i + 2] * a;
      A += a;
    }
    // empty (transparent) areas don't change the paint
    if (A / (d.length / 4) < 20) return;
    const canvasRGB = [R / A, G / A, B / A];
    for (let i = 0; i < 3; i++) mixRGB[i] += (canvasRGB[i] - mixRGB[i]) * amount;
  }

  function drawMixDab(pt: Point, factor: number) {
    const layer = layers.find((l) => l.id === strokeLayerId);
    if (!layer) return;
    const r = Math.max(radiusAt(pt.p) * factor, 0.5);

    // Pick up color every so often (reading pixels is slow, so not on every dab)
    if (Math.hypot(pt.x - lastSample.x, pt.y - lastSample.y) >= Math.max(1, r * 0.4)) {
      // fresh paint from the brush slowly comes back, unless mix is 100%
      const brushRGB = hexRGB(color);
      for (let i = 0; i < 3; i++) mixRGB[i] += (brushRGB[i] - mixRGB[i]) * (1 - brush.mix) * 0.3;
      sampleCanvas(layer, pt, brush.mix * 0.5);
      lastSample = { x: pt.x, y: pt.y };
    }

    // Color one brush tip with the mixed color
    const size = Math.ceil(r * 2) + 2;
    const t = tintCtx;
    if (t.canvas.width < size || t.canvas.height < size) {
      t.canvas.width = size;
      t.canvas.height = size;
    }
    t.globalCompositeOperation = 'source-over';
    t.clearRect(0, 0, size, size);
    t.drawImage(getDab('#ffffff', brush.hardness), 0, 0, r * 2, r * 2);
    t.globalCompositeOperation = 'source-in';
    t.fillStyle = `rgb(${mixRGB[0]}, ${mixRGB[1]}, ${mixRGB[2]})`;
    t.fillRect(0, 0, size, size);
    t.globalCompositeOperation = 'source-over';

    const ctx = layer.canvas.getContext('2d', { willReadFrequently: true })!;
    ctx.globalAlpha = brush.opacity * brush.flow * (brush.opacityPressure ? 0.25 + 0.75 * pt.p : 1);
    ctx.drawImage(t.canvas, 0, 0, r * 2, r * 2, pt.x - r, pt.y - r, r * 2, r * 2);
    ctx.globalAlpha = 1;
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
    if (index < 0) return;
    const l = layers[index];
    const copy = copyLayer(l);
    removeAt(index);
    push({ type: 'delete', layer: copy, index });
  }

  // A new layer above, already set up for shading: clipped to the layer below with a blend mode
  function addShadingLayer(kind: 'shadow' | 'light') {
    const l = makeLayer();
    l.name = kind === 'shadow' ? 'Shadow' : 'Light';
    l.blend = kind === 'shadow' ? 'Multiply' : 'Add';
    l.clip = true;
    const index = activeIndex() + 1;
    insertAt(l, index);
    push({ type: 'add', layer: l, index });
  }

  function duplicateLayer() {
    const src = activeLayer;
    if (!src) return;
    const l = { ...makeLayer(), name: src.name + ' copy', visible: src.visible, opacity: src.opacity, blend: src.blend, clip: src.clip, lockAlpha: src.lockAlpha };
    l.canvas.getContext('2d')!.drawImage(src.canvas, 0, 0);
    const index = activeIndex() + 1;
    insertAt(l, index);
    push({ type: 'add', layer: l, index });
  }

  // Merges the active layer into the one below, keeping how it looked
  function mergeDown() {
    const index = activeIndex();
    if (index <= 0) return;
    const upper = layers[index];
    const lower = layers[index - 1];
    const ctx = lower.canvas.getContext('2d', { willReadFrequently: true })!;
    const before = ctx.getImageData(0, 0, DOC_W, DOC_H);

    if (upper.visible) {
      let image: CanvasImageSource = upper.canvas;
      if (upper.clip) {
        // a clipped layer only shows inside the lower layer's shape
        const t = clipCtx;
        t.globalCompositeOperation = 'source-over';
        t.clearRect(0, 0, DOC_W, DOC_H);
        t.drawImage(upper.canvas, 0, 0);
        t.globalCompositeOperation = 'destination-in';
        t.drawImage(lower.canvas, 0, 0);
        t.globalCompositeOperation = 'source-over';
        image = t.canvas;
      }
      ctx.globalAlpha = upper.opacity;
      ctx.globalCompositeOperation = opOf(upper.blend);
      ctx.drawImage(image, 0, 0);
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = 'source-over';
    }

    const after = ctx.getImageData(0, 0, DOC_W, DOC_H);
    const copy = copyLayer(upper);
    removeAt(index);
    activeId = lower.id;
    push({ type: 'merge', upper: copy, upperIndex: index, lowerId: lower.id, before, after });
    thumbTick++;
    render();
  }

  // Makes the whole layer transparent (undoable)
  function clearLayer() {
    const layer = activeLayer;
    if (!layer) return;
    const ctx = layer.canvas.getContext('2d', { willReadFrequently: true })!;
    const before = ctx.getImageData(0, 0, DOC_W, DOC_H);
    ctx.clearRect(0, 0, DOC_W, DOC_H);
    const after = ctx.getImageData(0, 0, DOC_W, DOC_H);
    push({ type: 'pixels', layerId: layer.id, rect: { x: 0, y: 0, w: DOC_W, h: DOC_H }, before, after });
    thumbTick++;
    render();
  }

  function finishAdjust(result: { before: ImageData; after: ImageData } | null) {
    const layer = activeLayer;
    adjusting = false;
    if (result && layer) {
      push({ type: 'pixels', layerId: layer.id, rect: { x: 0, y: 0, w: DOC_W, h: DOC_H }, before: result.before, after: result.after });
    }
    thumbTick++;
    render();
  }

  // Svelte action: focus a text field and select its text as soon as it appears
  function focusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  let nameBeforeRename = '';
  function startRename(layer: Layer) {
    nameBeforeRename = layer.name;
    activeId = layer.id;
    renamingId = layer.id;
  }

  function endRename(layer: Layer, keep: boolean) {
    if (renamingId === null) return;
    renamingId = null;
    if (!keep || !layer.name.trim()) {
      layer.name = nameBeforeRename || `Layer ${layer.id}`;
      return;
    }
    if (layer.name !== nameBeforeRename) {
      const after = propsOf(layer);
      push({ type: 'layerProps', layerId: layer.id, before: { ...after, name: nameBeforeRename }, after });
    }
  }

  function openLayerMenu(e: MouseEvent, id: number) {
    e.preventDefault();
    activeId = id;
    // keep the menu inside the window
    layerMenu = {
      x: Math.min(e.clientX, window.innerWidth - 240),
      y: Math.min(e.clientY, window.innerHeight - 400),
    };
  }

  // Runs a menu action and closes the menu
  function menuDo(action: () => void) {
    layerMenu = null;
    action();
  }

  // +1 = up, -1 = down
  function moveLayer(dir: 1 | -1) {
    const from = activeIndex();
    const to = from + dir;
    if (from < 0 || to < 0 || to >= layers.length) return;
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
      case 'paper':
        Object.assign(paper, reverse ? e.before : e.after);
        break;
      case 'layerProps': {
        const l = layers.find((x) => x.id === e.layerId);
        if (l) Object.assign(l, reverse ? e.before : e.after);
        break;
      }
      case 'merge': {
        const lower = layers.find((l) => l.id === e.lowerId);
        lower?.canvas.getContext('2d')!.putImageData(reverse ? e.before : e.after, 0, 0);
        if (reverse) insertAt(e.upper, e.upperIndex);
        else {
          removeAt(e.upperIndex);
          activeId = e.lowerId;
        }
        break;
      }
    }
    thumbTick++;
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

  // ---------- Thumbnails ----------

  // Svelte action: draws a small preview of a layer, again whenever `tick` changes
  function thumb(node: HTMLCanvasElement, args: { layer: Layer; tick: number }) {
    const draw = ({ layer }: { layer: Layer; tick: number }) => {
      const ctx = node.getContext('2d')!;
      ctx.clearRect(0, 0, node.width, node.height);
      const scale = Math.min(node.width / DOC_W, node.height / DOC_H);
      const w = DOC_W * scale;
      const h = DOC_H * scale;
      ctx.drawImage(layer.canvas, (node.width - w) / 2, (node.height - h) / 2, w, h);
    };
    draw(args);
    return { update: draw };
  }

  // ---------- Shortcuts ----------

  function onKey(e: KeyboardEvent) {
    // Don't trigger shortcuts while typing in text or number fields
    const target = e.target as HTMLInputElement;
    if (target.tagName === 'INPUT' && (target.type === 'text' || target.type === 'number')) return;
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (adjusting) return; // the dialog handles its own keys
    if (mod && key === 'u') {
      e.preventDefault();
      if (activeLayer) adjusting = true;
      return;
    }

    if (key === 'escape' && layerMenu) {
      layerMenu = null;
    } else if (key === 'alt') {
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
      class="doc-view"
      bind:this={view}
      width={DOC_W}
      height={DOC_H}
      class:pixelated={zoom >= 2}
      style="width: {DOC_W * zoom}px; height: {DOC_H * zoom}px; transform-origin: {(DOC_W * zoom) / 2}px {(DOC_H * zoom) / 2}px; transform: translate({panX}px, {panY}px) rotate({rotation}deg) scaleX({flipped ? -1 : 1});"
    ></canvas>
    {#if notice}
      <div class="notice">{notice}</div>
    {/if}
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
          class:clipped={layer.clip}
          role="button"
          tabindex="0"
          onclick={() => (activeId = layer.id)}
          onkeydown={(e) => e.key === 'Enter' && (activeId = layer.id)}
          oncontextmenu={(e) => openLayerMenu(e, layer.id)}
        >
          <button
            class="eye"
            title="Show / hide"
            onclick={(e) => {
              e.stopPropagation();
              changeLayer(layer, () => (layer.visible = !layer.visible));
            }}>{layer.visible ? '👁' : '—'}</button
          >
          {#if layer.clip}<span class="clip-mark" title="Clipped to the layer below">↳</span>{/if}
          <canvas class="thumb" width="40" height="40" use:thumb={{ layer, tick: thumbTick }}></canvas>
          <div class="layer-text">
            {#if renamingId === layer.id}
              <input
                class="name editing"
                type="text"
                bind:value={layer.name}
                use:focusSelect
                onclick={(e) => e.stopPropagation()}
                onblur={() => endRename(layer, true)}
                onkeydown={(e) => {
                  e.stopPropagation();
                  if (e.key === 'Enter') endRename(layer, true);
                  if (e.key === 'Escape') endRename(layer, false);
                }}
              />
            {:else}
              <span
                class="name"
                role="textbox"
                tabindex="-1"
                title="Double-click to rename"
                ondblclick={() => startRename(layer)}>{layer.name}</span
              >
            {/if}
            <span class="sub">
              {layer.blend}{layer.opacity < 1 ? ` · ${Math.round(layer.opacity * 100)}%` : ''}{layer.lockAlpha ? ' · 🔒' : ''}
            </span>
          </div>
        </div>
      {/each}
      {#if paper.exists}
      <div
        class="layer paper-row"
        class:active={activeId === PAPER_ID}
        role="button"
        tabindex="0"
        title="Paper: the background under all layers"
        onclick={() => (activeId = PAPER_ID)}
        onkeydown={(e) => e.key === 'Enter' && (activeId = PAPER_ID)}
        oncontextmenu={(e) => {
          e.preventDefault();
          activeId = PAPER_ID;
        }}
      >
        <button
          class="eye"
          title="Show / hide paper (hidden = transparent background)"
          onclick={(e) => {
            e.stopPropagation();
            changePaper(() => (paper.visible = !paper.visible));
          }}>{paper.visible ? '👁' : '—'}</button
        >
        <div class="paper-swatch" style="background: {paper.color}"></div>
        <div class="layer-text">
          <span class="name">Paper</span>
          <span class="sub">{paper.visible ? 'Background' : 'Transparent'}</span>
        </div>
      </div>
      {:else}
        <button class="add-paper" onclick={addPaper} title="Add a background paper under all layers">＋ Add paper</button>
      {/if}
    </div>

    {#if activeId === PAPER_ID}
      <div class="layer-props">
        <label class="row">
          <span>Color</span>
          <input
            class="paper-color"
            type="color"
            value={paper.color}
            oninput={(e) => {
              beginPaperDrag();
              paper.color = e.currentTarget.value;
            }}
            onchange={endPaperDrag}
          />
          <button class="small" onclick={() => changePaper(() => (paper.color = color))} title="Use the color from the color wheel">Use current color</button>
        </label>
        <div class="paper-actions">
          <button class="small" onclick={() => changePaper(() => (paper.visible = !paper.visible))}>
            {paper.visible ? 'Make transparent' : 'Show paper'}
          </button>
          <button class="small danger" onclick={deletePaper}>Delete paper</button>
        </div>
        <p class="blend-hint">Paper is the background. You can't draw on it. Transparent or deleted paper = transparent background.</p>
      </div>
    {:else if activeLayer}
      <div class="layer-props">
        <label class="row">
          <span>Blend</span>
          <select
            value={activeLayer.blend}
            onchange={(e) => {
              const v = e.currentTarget.value as BlendMode;
              changeLayer(activeLayer, () => (activeLayer!.blend = v));
            }}
          >
            {#each BLEND_NAMES as b (b)}
              <option value={b}>{b}</option>
            {/each}
          </select>
        </label>
        <p class="blend-hint">{BLEND_HINTS[activeLayer.blend]}</p>
        <label class="row">
          <span>Opacity</span>
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={activeLayer.opacity}
            oninput={(e) => {
              beginLayerDrag(activeLayer!);
              activeLayer!.opacity = +e.currentTarget.value;
            }}
            onchange={endLayerDrag}
          />
          <span class="val">{Math.round(activeLayer.opacity * 100)}%</span>
        </label>
        <div class="toggles">
          <label title="Show this layer only on top of the layer below (Alt + click in CSP/Photoshop)">
            <input
              type="checkbox"
              checked={activeLayer.clip}
              onchange={(e) => {
                const v = e.currentTarget.checked;
                changeLayer(activeLayer, () => (activeLayer!.clip = v));
              }}
            /> Clipping mask
          </label>
          <label title="Paint only where this layer already has paint">
            <input
              type="checkbox"
              checked={activeLayer.lockAlpha}
              onchange={(e) => {
                const v = e.currentTarget.checked;
                changeLayer(activeLayer, () => (activeLayer!.lockAlpha = v));
              }}
            /> Lock alpha
          </label>
        </div>
      </div>
    {/if}

    <div class="layer-buttons">
      <button onclick={addLayer} title="New layer">＋</button>
      <button onclick={duplicateLayer} title="Duplicate layer">⧉</button>
      <button onclick={() => (adjusting = true)} title="Adjust colors (Ctrl+U)">🎨</button>
      <button onclick={mergeDown} disabled={activeIndex() <= 0} title="Merge down">⤓</button>
      <button onclick={() => moveLayer(1)} title="Move up">↑</button>
      <button onclick={() => moveLayer(-1)} title="Move down">↓</button>
      <button onclick={deleteLayer} disabled={layers.length <= 1} title="Delete layer">🗑</button>
    </div>
  </aside>
</div>

{#if adjusting && activeLayer}
  <AdjustColors canvas={activeLayer.canvas} layerName={activeLayer.name} onchange={render} ondone={finishAdjust} />
{/if}

{#if layerMenu && activeLayer}
  <!-- clicking anywhere outside closes the menu -->
  <div
    class="menu-backdrop"
    role="presentation"
    onpointerdown={() => (layerMenu = null)}
    oncontextmenu={(e) => {
      e.preventDefault();
      layerMenu = null;
    }}
  ></div>
  <div class="context-menu" role="menu" style="left: {layerMenu.x}px; top: {layerMenu.y}px;">
    <div class="menu-title">{activeLayer.name}</div>
    <button role="menuitem" onclick={() => menuDo(addLayer)}>New layer above</button>
    <button role="menuitem" onclick={() => menuDo(() => addShadingLayer('shadow'))}>New shadow layer (Multiply)</button>
    <button role="menuitem" onclick={() => menuDo(() => addShadingLayer('light'))}>New light layer (Add)</button>
    <button role="menuitem" onclick={() => menuDo(duplicateLayer)}>Duplicate</button>
    <button role="menuitem" onclick={() => menuDo(() => activeLayer && startRename(activeLayer))}>Rename</button>
    <hr />
    <button role="menuitem" onclick={() => menuDo(() => changeLayer(activeLayer, () => (activeLayer!.clip = !activeLayer!.clip)))}>
      {activeLayer.clip ? 'Release clipping mask' : 'Create clipping mask'}
    </button>
    <button role="menuitem" onclick={() => menuDo(() => changeLayer(activeLayer, () => (activeLayer!.lockAlpha = !activeLayer!.lockAlpha)))}>
      {activeLayer.lockAlpha ? '✓ Lock transparent pixels' : 'Lock transparent pixels'}
    </button>
    <button role="menuitem" onclick={() => menuDo(() => changeLayer(activeLayer, () => (activeLayer!.visible = !activeLayer!.visible)))}>
      {activeLayer.visible ? 'Hide layer' : 'Show layer'}
    </button>
    <hr />
    <button role="menuitem" disabled={activeIndex() <= 0} onclick={() => menuDo(mergeDown)}>Merge down</button>
    <button role="menuitem" onclick={() => menuDo(() => (adjusting = true))}>Adjust colors… (Ctrl+U)</button>
    <button role="menuitem" onclick={() => menuDo(clearLayer)}>Clear layer (make transparent)</button>
    <button role="menuitem" class="danger" disabled={layers.length <= 1} onclick={() => menuDo(deleteLayer)}>
      Delete layer
    </button>
  </div>
{/if}

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
  .doc-view {
    position: absolute;
    top: 0;
    left: 0;
    /* checkerboard = transparent areas */
    background-color: #fff;
    background-image: linear-gradient(45deg, #ccc 25%, transparent 25%, transparent 75%, #ccc 75%),
      linear-gradient(45deg, #ccc 25%, transparent 25%, transparent 75%, #ccc 75%);
    background-size: 16px 16px;
    background-position: 0 0, 8px 8px;
    box-shadow: 0 0 0 1px #333, 0 8px 24px rgba(0, 0, 0, 0.5);
  }
  .doc-view.pixelated {
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
    user-select: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name.editing {
    user-select: text;
    background: #15151a;
    border: 1px solid #00b5c8;
    border-radius: 4px;
    padding: 1px 4px;
  }
  .layer.clipped {
    margin-left: 14px;
  }
  .clip-mark {
    color: #00b5c8;
    margin-right: -4px;
  }
  .thumb {
    width: 40px;
    height: 40px;
    flex-shrink: 0;
    border-radius: 4px;
    background-color: #fff;
    background-image: linear-gradient(45deg, #ddd 25%, transparent 25%, transparent 75%, #ddd 75%),
      linear-gradient(45deg, #ddd 25%, transparent 25%, transparent 75%, #ddd 75%);
    background-size: 10px 10px;
    background-position: 0 0, 5px 5px;
  }
  .layer-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .sub {
    color: #888;
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .paper-row {
    cursor: default;
    user-select: none;
    margin-top: 4px;
    border-style: dashed;
    border-color: #3a3a44;
  }
  .paper-swatch {
    width: 40px;
    height: 40px;
    flex-shrink: 0;
    border-radius: 4px;
    border: 1px solid #444;
  }
  .paper-actions {
    display: flex;
    gap: 6px;
  }
  .danger {
    color: #ff7b7b;
  }
  .add-paper {
    margin-top: 4px;
    padding: 8px;
    border-style: dashed;
    color: #aaa;
    background: transparent;
  }
  .small {
    padding: 4px 8px;
    font-size: 11px;
  }
  .notice {
    position: absolute;
    left: 50%;
    bottom: 24px;
    transform: translateX(-50%);
    padding: 8px 14px;
    border-radius: 8px;
    background: rgba(20, 20, 26, 0.92);
    border: 1px solid #00b5c8;
    color: #eee;
    font-size: 13px;
    pointer-events: none;
    white-space: nowrap;
  }
  .paper-color {
    width: 40px;
    height: 28px;
    padding: 0;
    border: 1px solid #444;
    border-radius: 4px;
    background: none;
    cursor: pointer;
    flex-shrink: 0;
  }
  .layer-props {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
  }
  .layer-props .row {
    display: grid;
    grid-template-columns: 52px 1fr auto;
    align-items: center;
    gap: 6px;
    color: #bbb;
  }
  .layer-props select {
    grid-column: span 2;
    background: #15151a;
    color: #eee;
    border: 1px solid #444;
    border-radius: 4px;
    padding: 3px;
  }
  .layer-props .val {
    color: #888;
    min-width: 32px;
    text-align: right;
  }
  .blend-hint {
    margin: -2px 0 0;
    color: #888;
    font-size: 11px;
    line-height: 1.35;
  }
  .toggles {
    display: flex;
    gap: 12px;
    color: #bbb;
  }
  .toggles label {
    display: flex;
    gap: 4px;
  }
  .menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
  }
  .context-menu {
    position: fixed;
    z-index: 51;
    min-width: 220px;
    display: flex;
    flex-direction: column;
    padding: 4px;
    background: #2a2a32;
    border: 1px solid #444;
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
    font-family: system-ui, sans-serif;
    font-size: 13px;
  }
  .context-menu button {
    text-align: left;
    border: none;
    background: transparent;
    padding: 7px 10px;
    border-radius: 5px;
  }
  .context-menu button:hover:not(:disabled) {
    background: #00b5c8;
    color: #111;
  }
  .context-menu .danger {
    color: #ff7b7b;
  }
  .context-menu hr {
    border: none;
    border-top: 1px solid #444;
    margin: 4px 2px;
  }
  .menu-title {
    padding: 6px 10px 4px;
    color: #888;
    font-size: 11px;
  }
  .layer-buttons {
    display: flex;
    gap: 6px;
  }
  .layer-buttons button {
    flex: 1;
    padding: 6px 0;
  }
</style>
