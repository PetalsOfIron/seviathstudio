<script lang="ts">
  import { onMount, tick } from 'svelte';
  import ColorWheel from './lib/ColorWheel.svelte';
  import BrushPanel from './lib/BrushPanel.svelte';
  import AdjustColors from './lib/AdjustColors.svelte';
  import NewProjectDialog from './lib/NewProjectDialog.svelte';
  import { floodFill, expandMask } from './lib/fill';
  import { saveProject, openProject, exportPNG, exportPSD, importPSD, type ProjectData } from './lib/fileio';
  import { PRESETS, settingsOf, getDab, getPaper, type BrushSettings } from './lib/brush';

  // Document size (chosen in New project, or taken from an opened file)
  let docW = $state(2000);
  let docH = $state(1400);
  let projectName = $state('Untitled');
  const MAX_HISTORY = 50;

  type Tool = 'brush' | 'eraser' | 'picker' | 'fill' | 'lassoFill' | 'select' | 'move';
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
    | { type: 'selection'; before: SelShape[]; after: SelShape[] }
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

  // Fill (paint bucket) settings
  let fillRefer = $state<'layer' | 'all'>('all'); // look at this layer only, or all visible layers
  let fillTolerance = $state(40); // 0..255: how different a color may be and still be filled
  let fillExpand = $state(2); // px the fill grows under the lineart
  let fillGap = $state(3); // px: closes gaps in the lineart so the fill doesn't leak

  // Selection: a mask canvas (opaque = selected) plus the outlines drawn on screen
  type SelShape = { pts: { x: number; y: number }[]; op: 'add' | 'sub' | 'invert'; feather?: number };
  type ShapeMode = 'rect' | 'ellipse' | 'lasso';
  let selMode = $state<ShapeMode>('rect'); // shape used by the Select tool
  let lfMode = $state<ShapeMode>('lasso'); // shape used by Lasso fill
  let selFeather = $state(0); // px of soft edge on new selections
  let lfFeather = $state(0); // px of soft edge for Lasso fill
  let selDraftOpen = $state(false); // a lasso that hasn't been closed yet
  let shapeDragging = $state(false);
  let draftMarks: number[] = []; // where each pen-down started, for Ctrl+Z inside a lasso
  let selCtx: CanvasRenderingContext2D;
  let hasSel = $state(false);
  let selShapes = $state<SelShape[]>([]);
  let selInverted = $derived(selShapes.filter((s) => s.op === 'invert').length % 2 === 1);
  let selDraft = $state<{ x: number; y: number }[] | null>(null); // the shape being dragged right now
  let selOp: 'new' | 'add' | 'sub' = 'new';
  let lassoErase = $state(false); // Lasso fill with Ctrl held: erases the shape instead
  let selStart = { x: 0, y: 0 };

  // Transform (Ctrl+T) and Move tool: the content floats above the layer until applied
  type Xf = {
    layerId: number;
    src: HTMLCanvasElement; // the part being transformed
    base: HTMLCanvasElement; // the rest of the layer
    before: ImageData;
    box: { x: number; y: number; w: number; h: number }; // bounds of the floating part
    tx: number;
    ty: number;
    sx: number;
    sy: number;
    rot: number; // radians
    quick: boolean; // started by the Move tool: applied as soon as the drag ends
  };
  let xf = $state<Xf | null>(null);
  let xfDrag: null | {
    mode: 'move' | 'scale' | 'rotate';
    start: { x: number; y: number };
    orig: { tx: number; ty: number; sx: number; sy: number; rot: number };
    anchor?: { x: number; y: number };
    sign?: { x: number; y: number };
  } = null;

  // Dialogs
  let newDialog = $state(false);
  let fileMenu = $state(false);
  let fileInput = $state<HTMLInputElement>();

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
  let refCtx: CanvasRenderingContext2D; // everything else you see, for brushes that mix with all layers
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
  // The paint on the brush, premultiplied: [red·a, green·a, blue·a, a], colors 0..255, a 0..1.
  // Keeping alpha means smudging into empty areas thins the paint instead of turning it black.
  let mixP = [0, 0, 0, 1];
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
    c.width = docW;
    c.height = docH;
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

  // The helper canvases are as big as the document, so they are rebuilt when its size changes
  function makeWorkCanvases() {
    strokeCtx = makeCanvas().getContext('2d')!;
    scratchCtx = makeCanvas().getContext('2d')!;
    groupCtx = makeCanvas().getContext('2d')!;
    clipCtx = makeCanvas().getContext('2d')!;
    texturedCtx = makeCanvas().getContext('2d')!;
    backupCtx = makeCanvas().getContext('2d', { willReadFrequently: true })!;
    selCtx = makeCanvas().getContext('2d', { willReadFrequently: true })!;
    refCtx = makeCanvas().getContext('2d', { willReadFrequently: true })!;
    paperPattern = texturedCtx.createPattern(getPaper(), 'repeat');
  }

  onMount(() => {
    viewCtx = view!.getContext('2d', { willReadFrequently: true })!;
    makeWorkCanvases();
    tintCtx = document.createElement('canvas').getContext('2d')!;

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
      compositeTo(viewCtx!, true, true);
    });
  }

  // Draws all layers into ctx. `live` includes the unfinished stroke and transform previews.
  function compositeTo(ctx: CanvasRenderingContext2D, withPaper: boolean, live: boolean, excludeId = -999) {
    const strokeView = live && drawing && !directStroke ? previewStrokeLayer() : null;
    const xfView = live && xf ? previewTransform() : null;
    const surface = (l: Layer): CanvasImageSource => {
      if (strokeView && l.id === strokeLayerId) return strokeView;
      if (xfView && xf && l.id === xf.layerId) return xfView;
      return l.canvas;
    };

    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
    if (withPaper && paper.exists && paper.visible) {
      ctx.fillStyle = paper.color;
      ctx.fillRect(0, 0, docW, docH);
    } else {
      // transparent: the checkerboard behind the canvas shows through
      ctx.clearRect(0, 0, docW, docH);
    }

    // Walk up the stack; each normal layer may have clipped layers right above it
    let i = 0;
    while (i < layers.length) {
      const base = layers[i];
      let j = i + 1;
      while (j < layers.length && layers[j].clip) j++;
      const clipped = layers.slice(i + 1, j).filter((c) => c.visible && c.id !== excludeId);

      if (base.visible && base.id !== excludeId) {
        let image: CanvasImageSource = surface(base);
        if (clipped.length) {
          const g = groupCtx;
          g.globalAlpha = 1;
          g.globalCompositeOperation = 'source-over';
          g.clearRect(0, 0, docW, docH);
          g.drawImage(image, 0, 0);
          for (const c of clipped) {
            // cut the clipped layer to the base layer's shape, then blend it on
            const t = clipCtx;
            t.globalCompositeOperation = 'source-over';
            t.clearRect(0, 0, docW, docH);
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
  }

  // The layer being painted on, with the unfinished stroke shown on top of it
  function previewStrokeLayer(): HTMLCanvasElement | null {
    const l = layers.find((x) => x.id === strokeLayerId);
    if (!l) return null;
    const s = scratchCtx;
    s.globalCompositeOperation = 'source-over';
    s.globalAlpha = 1;
    s.clearRect(0, 0, docW, docH);
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
    const cx = (docW * zoom) / 2;
    const cy = (docH * zoom) / 2;
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
    const cx = (docW * zoom) / 2;
    const cy = (docH * zoom) / 2;
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
    const bw = Math.abs(docW * Math.cos(a)) + Math.abs(docH * Math.sin(a));
    const bh = Math.abs(docW * Math.sin(a)) + Math.abs(docH * Math.cos(a));
    zoom = Math.min((r.width - 32) / bw, (r.height - 32) / bh);
    panX = r.width / 2 - (docW * zoom) / 2;
    panY = r.height / 2 - (docH * zoom) / 2;
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
    const pt = toDoc(e.clientX, e.clientY);
    // While transforming, the pen only works on the transform box
    if (xf) {
      xfDown(e, pt, tool === 'move' && xf.quick);
      return;
    }
    if (activeTool === 'move') {
      if (startTransform(true)) xfDown(e, pt, true);
      return;
    }
    if (activeTool === 'select' || activeTool === 'lassoFill') {
      shapeDown(e, pt);
      return;
    }
    if (activeTool === 'fill') {
      doFill(pt);
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
    if (xfDrag) {
      xfMove(toDoc(e.clientX, e.clientY), e.shiftKey);
      return;
    }
    if (selDraft) {
      shapeMove(toDoc(e.clientX, e.clientY), e.shiftKey);
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
    if (xfDrag) {
      xfUp();
      return;
    }
    if (selDraft) {
      shapeUp();
      return;
    }
    up();
  }

  // Takes the color you see on screen (all visible layers combined)
  function pickColor(e: PointerEvent) {
    const d = toDoc(e.clientX, e.clientY);
    const x = Math.floor(d.x);
    const y = Math.floor(d.y);
    if (x < 0 || y < 0 || x >= docW || y >= docH || !viewCtx) return;
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
    strokeCtx.clearRect(0, 0, docW, docH);
    resetDabs();
    grainOffset = { x: Math.random() * 256, y: Math.random() * 256 };

    directStroke = strokeTool === 'brush' && brush.mix > 0;
    if (directStroke) {
      backupCtx.clearRect(0, 0, docW, docH);
      backupCtx.drawImage(layer.canvas, 0, 0);
      if (brush.mixRefer === 'all') {
        // a picture of everything except this layer; the layer itself is read live on top of it
        compositeTo(refCtx, true, false, layer.id);
      }
      const [r0, g0, b0] = hexRGB(color);
      mixP = [r0, g0, b0, 1];
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
    const w = Math.min(docW, Math.ceil(maxX)) - x;
    const h = Math.min(docH, Math.ceil(maxY)) - y;
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
      if (hasSel) {
        // keep the new paint only inside the selection, the old pixels everywhere else
        const t = clipCtx;
        t.globalCompositeOperation = 'source-over';
        t.clearRect(0, 0, docW, docH);
        t.drawImage(layer.canvas, 0, 0);
        t.globalCompositeOperation = 'destination-in';
        t.drawImage(selCtx.canvas, 0, 0);
        t.globalCompositeOperation = 'source-over';
        ctx.clearRect(0, 0, docW, docH);
        ctx.drawImage(backupCtx.canvas, 0, 0);
        ctx.globalCompositeOperation = 'destination-out';
        ctx.drawImage(selCtx.canvas, 0, 0);
        ctx.globalCompositeOperation = 'source-over';
        ctx.drawImage(t.canvas, 0, 0);
      }
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
    strokeCtx.clearRect(0, 0, docW, docH);
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
    const useSel = hasSel;
    if (!useTexture && !useLock && !useSel) return strokeCtx.canvas;
    const t = texturedCtx;
    t.globalCompositeOperation = 'source-over';
    t.globalAlpha = 1;
    t.clearRect(0, 0, docW, docH);
    t.drawImage(strokeCtx.canvas, 0, 0);
    if (useTexture) {
      t.globalCompositeOperation = 'destination-out';
      t.globalAlpha = brush.texture;
      paperPattern!.setTransform(new DOMMatrix().translate(grainOffset.x, grainOffset.y));
      t.fillStyle = paperPattern!;
      t.fillRect(0, 0, docW, docH);
      t.globalAlpha = 1;
    }
    if (useLock) {
      // Lock alpha: keep the stroke only where the layer already has paint
      t.globalCompositeOperation = 'destination-in';
      t.drawImage(layer.canvas, 0, 0);
    }
    if (useSel) {
      // Selection: keep the stroke only inside the selected area
      t.globalCompositeOperation = 'destination-in';
      t.drawImage(selCtx.canvas, 0, 0);
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

  // Reads the average color (and transparency) under the brush and mixes it into the paint
  // `fromBefore`: read the layer as it was before this stroke (so paint doesn't pick up itself)
  function sampleCanvas(layer: Layer, pt: Point, amount: number, fromBefore = false) {
    const ctx = fromBefore ? backupCtx : layer.canvas.getContext('2d', { willReadFrequently: true })!;
    const r = Math.max(1, Math.min(4, Math.round(radiusAt(pt.p) * 0.3)));
    const x = Math.round(pt.x) - r;
    const y = Math.round(pt.y) - r;
    if (x + 2 * r < 0 || y + 2 * r < 0 || x >= docW || y >= docH) return;
    const size = r * 2 + 1;
    const d = ctx.getImageData(x, y, size, size).data;
    // "All layers": what you see = this layer on top of everything else
    const ref = brush.mixRefer === 'all' ? refCtx.getImageData(x, y, size, size).data : null;
    let R = 0, G = 0, B = 0, A = 0;
    const n = d.length / 4;
    for (let i = 0; i < d.length; i += 4) {
      const a = d[i + 3] / 255;
      let pr = d[i] * a, pg = d[i + 1] * a, pb = d[i + 2] * a, pa = a;
      if (ref) {
        const ra = (ref[i + 3] / 255) * (1 - a);
        pr += ref[i] * ra;
        pg += ref[i + 1] * ra;
        pb += ref[i + 2] * ra;
        pa += ra;
      }
      R += pr;
      G += pg;
      B += pb;
      A += pa;
    }
    // average in premultiplied form, so empty pixels count as "no paint", not as black paint
    const sample = [R / n, G / n, B / n, A / n];
    for (let i = 0; i < 4; i++) mixP[i] += (sample[i] - mixP[i]) * amount;
  }

  function drawMixDab(pt: Point, factor: number) {
    const layer = layers.find((l) => l.id === strokeLayerId);
    if (!layer) return;
    const r = Math.max(radiusAt(pt.p) * factor, 0.5);

    // Pick up color every so often (reading pixels is slow, so not on every dab)
    if (Math.hypot(pt.x - lastSample.x, pt.y - lastSample.y) >= Math.max(1, r * 0.4)) {
      // fresh paint from the brush slowly comes back, unless mix is 100%
      const [br, bg, bb] = hexRGB(color);
      const fresh = [br, bg, bb, 1];
      // Color mix % = how much of the paint comes from the canvas, the rest is your color.
      // A pure blender (≈100%) smears what it just picked up; paint mixing reads the layer
      // from before the stroke so it doesn't keep picking up its own fresh paint.
      for (let i = 0; i < 4; i++) mixP[i] += (fresh[i] - mixP[i]) * (1 - brush.mix) * 0.3;
      sampleCanvas(layer, pt, brush.mix * 0.3, brush.mix < 0.9);
      lastSample = { x: pt.x, y: pt.y };
    }

    const a = Math.min(1, Math.max(0, mixP[3]));
    const rgb = a > 0.001 ? [mixP[0] / a, mixP[1] / a, mixP[2] / a] : [0, 0, 0];

    // Brush tip mask (white, soft edge) and the same tip colored with the paint
    const size = Math.ceil(r * 2) + 2;
    const t = tintCtx;
    if (t.canvas.width < size * 2 || t.canvas.height < size) {
      t.canvas.width = size * 2;
      t.canvas.height = size;
    }
    t.globalCompositeOperation = 'source-over';
    t.globalAlpha = 1;
    t.clearRect(0, 0, t.canvas.width, t.canvas.height);
    const tip = getDab('#ffffff', brush.hardness);
    t.drawImage(tip, 0, 0, r * 2, r * 2); // left: mask
    t.globalAlpha = a;
    t.drawImage(tip, size, 0, r * 2, r * 2); // right: paint with its transparency
    t.globalAlpha = 1;
    t.globalCompositeOperation = 'source-atop';
    t.fillStyle = `rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})`;
    t.fillRect(size, 0, size, size);
    t.globalCompositeOperation = 'source-over';

    // Blend the layer towards the paint: new = old·(1−f) + paint·f.
    // Step 1 removes f of what's there, step 2 adds f of the paint (premultiplied).
    const f = brush.opacity * brush.flow * (brush.opacityPressure ? 0.25 + 0.75 * pt.p : 1);
    const ctx = layer.canvas.getContext('2d', { willReadFrequently: true })!;
    ctx.globalAlpha = f;
    ctx.globalCompositeOperation = 'destination-out';
    ctx.drawImage(t.canvas, 0, 0, r * 2, r * 2, pt.x - r, pt.y - r, r * 2, r * 2);
    ctx.globalCompositeOperation = 'lighter';
    ctx.drawImage(t.canvas, size, 0, r * 2, r * 2, pt.x - r, pt.y - r, r * 2, r * 2);
    ctx.globalCompositeOperation = 'source-over';
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
      strokeCtx.clearRect(0, 0, docW, docH);
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

  // ---------- Undoable pixel edits ----------

  // Runs an edit on a layer and records the changed area for undo
  function editLayerPixels(layer: Layer, rect: Rect, edit: (ctx: CanvasRenderingContext2D) => void) {
    const ctx = layer.canvas.getContext('2d', { willReadFrequently: true })!;
    const before = ctx.getImageData(rect.x, rect.y, rect.w, rect.h);
    edit(ctx);
    ctx.globalCompositeOperation = 'source-over';
    ctx.globalAlpha = 1;
    const after = ctx.getImageData(rect.x, rect.y, rect.w, rect.h);
    push({ type: 'pixels', layerId: layer.id, rect, before, after });
    thumbTick++;
    render();
  }

  const fullRect = (): Rect => ({ x: 0, y: 0, w: docW, h: docH });

  // A layer you can paint on right now, or a message explaining why not
  function editableLayer(): Layer | null {
    if (activeId === PAPER_ID) {
      showNotice("Paper can't be edited. Select a layer.");
      return null;
    }
    const layer = activeLayer;
    if (!layer) return null;
    if (!layer.visible) {
      showNotice('This layer is hidden. Show it (👁) first.');
      return null;
    }
    return layer;
  }

  // ---------- Selection ----------
  // The selection is a list of shapes (added, removed, or "invert"). The mask canvas is
  // rebuilt from that list, so undo only needs to remember the list.

  type Pt = { x: number; y: number };

  function shapePath(ctx: CanvasRenderingContext2D, pts: Pt[]) {
    ctx.beginPath();
    pts.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)));
    ctx.closePath();
  }

  function rectPts(a: Pt, b: Pt) {
    const x0 = Math.min(a.x, b.x), x1 = Math.max(a.x, b.x);
    const y0 = Math.min(a.y, b.y), y1 = Math.max(a.y, b.y);
    return [{ x: x0, y: y0 }, { x: x1, y: y0 }, { x: x1, y: y1 }, { x: x0, y: y1 }];
  }

  // Rectangle or ellipse from the drag start to the pointer. Shift keeps it square / round.
  function dragShape(mode: ShapeMode, a: Pt, b: Pt, keepRatio: boolean): Pt[] {
    let x1 = b.x, y1 = b.y;
    if (keepRatio) {
      const side = Math.max(Math.abs(b.x - a.x), Math.abs(b.y - a.y));
      x1 = a.x + Math.sign(b.x - a.x || 1) * side;
      y1 = a.y + Math.sign(b.y - a.y || 1) * side;
    }
    if (mode === 'rect') return rectPts(a, { x: x1, y: y1 });
    const cx = (a.x + x1) / 2, cy = (a.y + y1) / 2;
    const rx = Math.abs(x1 - a.x) / 2, ry = Math.abs(y1 - a.y) / 2;
    const n = 96;
    return Array.from({ length: n }, (_, i) => {
      const t = (i / n) * Math.PI * 2;
      return { x: cx + rx * Math.cos(t), y: cy + ry * Math.sin(t) };
    });
  }

  function invertMask() {
    const t = clipCtx;
    t.globalCompositeOperation = 'source-over';
    t.clearRect(0, 0, docW, docH);
    t.fillStyle = '#000';
    t.fillRect(0, 0, docW, docH);
    t.globalCompositeOperation = 'destination-out';
    t.drawImage(selCtx.canvas, 0, 0);
    t.globalCompositeOperation = 'source-over';
    selCtx.clearRect(0, 0, docW, docH);
    selCtx.drawImage(t.canvas, 0, 0);
  }

  function rebuildSelMask() {
    const c = selCtx;
    if (!c) return;
    c.globalCompositeOperation = 'source-over';
    c.clearRect(0, 0, docW, docH);
    for (const sh of selShapes) {
      if (sh.op === 'invert') {
        invertMask();
        continue;
      }
      c.globalCompositeOperation = sh.op === 'sub' ? 'destination-out' : 'source-over';
      c.fillStyle = '#000';
      // feather = a soft edge, made by blurring the shape
      c.filter = sh.feather ? `blur(${sh.feather / 2}px)` : 'none';
      shapePath(c, sh.pts);
      c.fill();
      c.filter = 'none';
    }
    c.globalCompositeOperation = 'source-over';
    hasSel = selShapes.length > 0;
  }

  // Replaces the selection; `record` adds it to undo
  function setSelection(next: SelShape[], record = true) {
    const before = $state.snapshot(selShapes) as SelShape[];
    selShapes = next;
    rebuildSelMask();
    if (record && !same(before, next)) push({ type: 'selection', before, after: $state.snapshot(selShapes) as SelShape[] });
  }

  function addSelShape(pts: Pt[], op: 'new' | 'add' | 'sub') {
    const current = $state.snapshot(selShapes) as SelShape[];
    const feather = selFeather || undefined;
    setSelection(op === 'new' ? [{ pts, op: 'add', feather }] : [...current, { pts, op, feather }]);
  }

  // Feather slider: also softens the selection you already have (one undo step per drag)
  let featherBefore: SelShape[] | null = null;
  function featherInput(v: number) {
    selFeather = v;
    if (!selShapes.length) return;
    const snap = $state.snapshot(selShapes) as SelShape[];
    if (!featherBefore) featherBefore = snap;
    selShapes = snap.map((sh) => (sh.op === 'invert' ? sh : { ...sh, feather: v || undefined }));
    rebuildSelMask();
  }
  function featherDone() {
    if (featherBefore) {
      const after = $state.snapshot(selShapes) as SelShape[];
      if (!same(featherBefore, after)) push({ type: 'selection', before: featherBefore, after });
    }
    featherBefore = null;
  }

  function selectAll() {
    setSelection([{ pts: rectPts({ x: 0, y: 0 }, { x: docW, y: docH }), op: 'add' }]);
  }

  function deselect() {
    if (selShapes.length) setSelection([]);
  }

  // Inverting nothing selects everything, like in Photoshop
  function invertSelection() {
    setSelection([...($state.snapshot(selShapes) as SelShape[]), { pts: [], op: 'invert' }]);
  }

  // Delete: erase what is inside the selection on the active layer
  function clearSelected() {
    const layer = editableLayer();
    if (!layer || !hasSel) return;
    editLayerPixels(layer, fullRect(), (ctx) => {
      ctx.globalCompositeOperation = 'destination-out';
      ctx.drawImage(selCtx.canvas, 0, 0);
    });
  }

  // Keep only what is inside the selection (soft edges if the selection is feathered)
  function deleteOutside() {
    const layer = editableLayer();
    if (!layer || !hasSel) return;
    editLayerPixels(layer, fullRect(), (ctx) => {
      ctx.globalCompositeOperation = 'destination-in';
      ctx.drawImage(selCtx.canvas, 0, 0);
    });
  }

  // Copies (or cuts) the selected part into a new layer right above
  function selectionToNewLayer(cut: boolean) {
    const layer = editableLayer();
    if (!layer) return;
    if (!hasSel) {
      showNotice('Select an area first.');
      return;
    }
    const l = makeLayer();
    l.name = `${layer.name} ${cut ? 'cut' : 'copy'}`;
    const c = l.canvas.getContext('2d')!;
    c.drawImage(layer.canvas, 0, 0);
    c.globalCompositeOperation = 'destination-in';
    c.drawImage(selCtx.canvas, 0, 0);
    c.globalCompositeOperation = 'source-over';
    if (cut) {
      editLayerPixels(layer, fullRect(), (ctx) => {
        ctx.globalCompositeOperation = 'destination-out';
        ctx.drawImage(selCtx.canvas, 0, 0);
      });
    }
    const index = layers.findIndex((x) => x.id === layer.id) + 1;
    insertAt(l, index);
    push({ type: 'add', layer: l, index });
    thumbTick++;
    render();
  }

  // Softens the outer edge of everything on the layer: it fades out over `px` pixels
  // towards the edge, while the colors and the inside stay as they are.
  function featherLayerEdges(px: number) {
    const layer = editableLayer();
    if (!layer || px <= 0) return;
    // blurred copy of the layer: its alpha tells how far inside the shape each pixel is
    const m = makeCanvas();
    const mctx = m.getContext('2d', { willReadFrequently: true })!;
    mctx.filter = `blur(${px / 2}px)`;
    mctx.drawImage(layer.canvas, 0, 0);
    mctx.filter = 'none';
    const mask = mctx.getImageData(0, 0, docW, docH).data;
    editLayerPixels(layer, fullRect(), (ctx) => {
      const img = ctx.getImageData(0, 0, docW, docH);
      const d = img.data;
      for (let i = 3; i < d.length; i += 4) {
        if (!d[i]) continue;
        // blurred alpha is ~50% on the edge and ~100% deep inside: map that to 0..1
        const f = Math.min(1, Math.max(0, (mask[i] / 255 - 0.5) * 2));
        d[i] = d[i] * f;
      }
      ctx.putImageData(img, 0, 0);
    });
  }

  // Fill the selected area with the current color
  function fillSelected() {
    const layer = editableLayer();
    if (!layer || !hasSel) return;
    const t = clipCtx;
    t.globalCompositeOperation = 'source-over';
    t.clearRect(0, 0, docW, docH);
    t.fillStyle = color;
    t.fillRect(0, 0, docW, docH);
    t.globalCompositeOperation = 'destination-in';
    t.drawImage(selCtx.canvas, 0, 0);
    if (layer.lockAlpha) t.drawImage(layer.canvas, 0, 0);
    t.globalCompositeOperation = 'source-over';
    editLayerPixels(layer, fullRect(), (ctx) => ctx.drawImage(t.canvas, 0, 0));
  }

  // ---------- Drawing a shape (Select and Lasso fill) ----------
  // Rectangle / ellipse: drag. Lasso: draw a line (or click for straight segments; you can
  // lift the pen and go on). It closes when the line comes back to its start point, or with
  // a double-click or Enter. Esc cancels, Ctrl+Z removes the last part.

  const shapeMode = (): ShapeMode => (activeTool === 'lassoFill' ? lfMode : selMode);

  function shapeDown(e: PointerEvent, pt: Pt) {
    if (activeTool === 'lassoFill' && !editableLayer()) return;
    capture(e);
    shapeDragging = true;
    const p = { x: pt.x, y: pt.y };
    if (shapeMode() === 'lasso' && selDraft && selDraftOpen) {
      // continue the open lasso: a straight segment to here, then keep drawing
      draftMarks.push(selDraft.length);
      selDraft = [...selDraft, p];
      return;
    }
    selOp = e.shiftKey ? 'add' : e.ctrlKey || e.metaKey ? 'sub' : 'new';
    lassoErase = e.ctrlKey || e.metaKey;
    selStart = p;
    selDraft = [p];
    selDraftOpen = shapeMode() === 'lasso';
    draftMarks = [0];
  }

  function shapeMove(pt: Pt, shift: boolean) {
    if (!selDraft || !shapeDragging) return;
    const mode = shapeMode();
    if (mode === 'lasso') {
      const lastPt = selDraft[selDraft.length - 1];
      if (Math.hypot(pt.x - lastPt.x, pt.y - lastPt.y) > 2 / zoom) selDraft = [...selDraft, { x: pt.x, y: pt.y }];
    } else {
      selDraft = dragShape(mode, selStart, pt, shift);
    }
  }

  function shapeUp() {
    shapeDragging = false;
    const d = selDraft;
    if (!d) return;
    if (shapeMode() === 'lasso') {
      // closed when the line comes back near its start
      const first = d[0], lastPt = d[d.length - 1];
      if (d.length >= 3 && Math.hypot(lastPt.x - first.x, lastPt.y - first.y) < 14 / zoom) finishShape();
      return;
    }
    const xs = d.map((p) => p.x), ys = d.map((p) => p.y);
    const tiny = Math.max(...xs) - Math.min(...xs) < 2 && Math.max(...ys) - Math.min(...ys) < 2;
    if (tiny || d.length < 3) {
      cancelDraft();
      // a click without dragging clears the selection, like in Photoshop and CSP
      if (activeTool === 'select' && selOp === 'new') deselect();
      return;
    }
    finishShape();
  }

  function finishShape() {
    const d = selDraft;
    cancelDraft();
    if (!d || d.length < 3) return;
    if (activeTool === 'lassoFill') lassoFill(d, lassoErase);
    else addSelShape(d, selOp);
  }

  function cancelDraft() {
    selDraft = null;
    selDraftOpen = false;
    draftMarks = [];
    shapeDragging = false;
  }

  // Ctrl+Z while drawing a lasso: remove the last part instead of undoing other things
  function undoDraftPart() {
    if (!selDraft) return;
    const m = draftMarks.pop();
    if (!m) cancelDraft();
    else selDraft = selDraft.slice(0, m);
  }

  // ---------- Lasso fill ----------

  // Fills (or with Ctrl erases) the drawn shape on the active layer
  function lassoFill(pts: { x: number; y: number }[], erase: boolean) {
    const layer = editableLayer();
    if (!layer) return;
    const xs = pts.map((p) => p.x);
    const ys = pts.map((p) => p.y);
    const pad = 2 + lfFeather * 2; // room for the soft edge
    const bx = Math.max(0, Math.floor(Math.min(...xs) - pad));
    const by = Math.max(0, Math.floor(Math.min(...ys) - pad));
    const bw = Math.min(docW, Math.ceil(Math.max(...xs) + pad)) - bx;
    const bh = Math.min(docH, Math.ceil(Math.max(...ys) + pad)) - by;
    if (bw <= 0 || bh <= 0) return;

    const tmp = document.createElement('canvas');
    tmp.width = bw;
    tmp.height = bh;
    const t = tmp.getContext('2d')!;
    t.translate(-bx, -by);
    t.fillStyle = color;
    t.filter = lfFeather ? `blur(${lfFeather / 2}px)` : 'none';
    shapePath(t, pts);
    t.fill();
    t.filter = 'none';
    t.setTransform(1, 0, 0, 1, 0, 0);
    t.globalCompositeOperation = 'destination-in';
    if (hasSel) t.drawImage(selCtx.canvas, -bx, -by);
    if (layer.lockAlpha && !erase) t.drawImage(layer.canvas, -bx, -by);
    t.globalCompositeOperation = 'source-over';

    editLayerPixels(layer, { x: bx, y: by, w: bw, h: bh }, (ctx) => {
      ctx.globalCompositeOperation = erase ? 'destination-out' : 'source-over';
      ctx.drawImage(tmp, bx, by);
    });
  }

  // ---------- Fill (paint bucket) ----------

  function doFill(pt: { x: number; y: number }) {
    const layer = editableLayer();
    if (!layer) return;

    // What the bucket "looks at": this layer only, or everything visible (without paper)
    let ref: ImageData;
    if (fillRefer === 'layer') {
      ref = layer.canvas.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, docW, docH);
    } else {
      const c = makeCanvas();
      const cctx = c.getContext('2d', { willReadFrequently: true })!;
      compositeTo(cctx, false, false);
      ref = cctx.getImageData(0, 0, docW, docH);
    }
    const found = floodFill(ref, pt.x, pt.y, fillTolerance, fillGap);
    if (!found) return;
    // grow back what gap closing took away, plus the extra expand under the lines
    const grow = fillGap + fillExpand;
    const res = grow > 0 ? expandMask(found, docW, docH, grow) : found;

    const bx = res.minX, by = res.minY;
    const bw = res.maxX - bx + 1, bh = res.maxY - by + 1;
    const img = new ImageData(bw, bh);
    const [r, g, b] = hexRGB(color);
    const sel = hasSel ? selCtx.getImageData(bx, by, bw, bh).data : null;
    for (let y = 0; y < bh; y++) {
      for (let x = 0; x < bw; x++) {
        if (!res.mask[(by + y) * docW + bx + x]) continue;
        const i = (y * bw + x) * 4;
        img.data[i] = r;
        img.data[i + 1] = g;
        img.data[i + 2] = b;
        img.data[i + 3] = sel ? sel[i + 3] : 255;
      }
    }
    const tmp = document.createElement('canvas');
    tmp.width = bw;
    tmp.height = bh;
    const tctx = tmp.getContext('2d')!;
    tctx.putImageData(img, 0, 0);
    if (layer.lockAlpha) {
      tctx.globalCompositeOperation = 'destination-in';
      tctx.drawImage(layer.canvas, -bx, -by);
    }
    editLayerPixels(layer, { x: bx, y: by, w: bw, h: bh }, (ctx) => ctx.drawImage(tmp, bx, by));
  }

  // ---------- Transform (Ctrl+T) and Move ----------

  // Bounds of everything that isn't transparent
  function contentBounds(canvas: HTMLCanvasElement) {
    const data = canvas.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, docW, docH).data;
    let minX = docW, minY = docH, maxX = -1, maxY = -1;
    for (let y = 0; y < docH; y++) {
      const row = y * docW * 4;
      for (let x = 0; x < docW; x++) {
        if (data[row + x * 4 + 3]) {
          if (x < minX) minX = x;
          if (x > maxX) maxX = x;
          if (y < minY) minY = y;
          if (y > maxY) maxY = y;
        }
      }
    }
    return maxX < 0 ? null : { x: minX, y: minY, w: maxX - minX + 1, h: maxY - minY + 1 };
  }

  // Lifts the selected part (or the whole layer) so it can be moved, scaled and rotated
  function startTransform(quick: boolean): boolean {
    if (xf) return true;
    const layer = editableLayer();
    if (!layer) return false;
    const src = makeCanvas();
    const base = makeCanvas();
    const sctx = src.getContext('2d', { willReadFrequently: true })!;
    const bctx = base.getContext('2d')!;
    sctx.drawImage(layer.canvas, 0, 0);
    if (hasSel) {
      sctx.globalCompositeOperation = 'destination-in';
      sctx.drawImage(selCtx.canvas, 0, 0);
      sctx.globalCompositeOperation = 'source-over';
      bctx.drawImage(layer.canvas, 0, 0);
      bctx.globalCompositeOperation = 'destination-out';
      bctx.drawImage(selCtx.canvas, 0, 0);
      bctx.globalCompositeOperation = 'source-over';
    }
    const box = contentBounds(src);
    if (!box) {
      showNotice(hasSel ? 'Nothing to move inside the selection.' : 'This layer is empty.');
      return false;
    }
    const before = layer.canvas.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, docW, docH);
    xf = { layerId: layer.id, src, base, before, box, tx: 0, ty: 0, sx: 1, sy: 1, rot: 0, quick };
    render();
    return true;
  }

  function xfMatrix(x: Xf) {
    const cx = x.box.x + x.box.w / 2;
    const cy = x.box.y + x.box.h / 2;
    return new DOMMatrix()
      .translate(cx + x.tx, cy + x.ty)
      .rotate((x.rot * 180) / Math.PI)
      .scale(x.sx, x.sy)
      .translate(-cx, -cy);
  }

  function xfCorners(x: Xf) {
    const m = xfMatrix(x);
    const b = x.box;
    return [
      [b.x, b.y],
      [b.x + b.w, b.y],
      [b.x + b.w, b.y + b.h],
      [b.x, b.y + b.h],
    ].map(([px, py]) => {
      const q = m.transformPoint(new DOMPoint(px, py));
      return { x: q.x, y: q.y };
    });
  }

  function xfCenter(x: Xf) {
    return { x: x.box.x + x.box.w / 2 + x.tx, y: x.box.y + x.box.h / 2 + x.ty };
  }

  // The round handle above the box used for rotating
  function xfRotHandle(x: Xf) {
    const [c0, c1] = xfCorners(x);
    const top = { x: (c0.x + c1.x) / 2, y: (c0.y + c1.y) / 2 };
    const c = xfCenter(x);
    let dx = top.x - c.x;
    let dy = top.y - c.y;
    const len = Math.hypot(dx, dy) || 1;
    dx /= len;
    dy /= len;
    return { top, handle: { x: top.x + (dx * 30) / zoom, y: top.y + (dy * 30) / zoom } };
  }

  function drawTransformed(ctx: CanvasRenderingContext2D, x: Xf) {
    ctx.drawImage(x.base, 0, 0);
    ctx.save();
    ctx.imageSmoothingQuality = 'high';
    ctx.setTransform(xfMatrix(x));
    ctx.drawImage(x.src, 0, 0);
    ctx.restore();
  }

  function previewTransform(): HTMLCanvasElement | null {
    if (!xf) return null;
    const s = scratchCtx;
    s.globalCompositeOperation = 'source-over';
    s.globalAlpha = 1;
    s.clearRect(0, 0, docW, docH);
    drawTransformed(s, xf);
    return s.canvas;
  }

  function insidePolygon(p: { x: number; y: number }, poly: { x: number; y: number }[]) {
    let inside = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      const a = poly[i], b = poly[j];
      if (a.y > p.y !== b.y > p.y && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) inside = !inside;
    }
    return inside;
  }

  function xfDown(e: PointerEvent, pt: { x: number; y: number }, forceMove = false) {
    const x = xf;
    if (!x) return;
    capture(e);
    const orig = { tx: x.tx, ty: x.ty, sx: x.sx, sy: x.sy, rot: x.rot };
    const start = { x: pt.x, y: pt.y };
    if (forceMove) {
      xfDrag = { mode: 'move', start, orig };
      return;
    }
    const tol = 10 / zoom;
    const corners = xfCorners(x);
    const rot = xfRotHandle(x).handle;
    const signs = [{ x: -1, y: -1 }, { x: 1, y: -1 }, { x: 1, y: 1 }, { x: -1, y: 1 }];
    if (Math.hypot(pt.x - rot.x, pt.y - rot.y) < tol * 1.3) {
      xfDrag = { mode: 'rotate', start, orig };
      return;
    }
    const ci = corners.findIndex((c) => Math.hypot(pt.x - c.x, pt.y - c.y) < tol);
    if (ci >= 0) {
      xfDrag = { mode: 'scale', start, orig, anchor: corners[(ci + 2) % 4], sign: signs[ci] };
      return;
    }
    // inside the box = move, outside = rotate (like Photoshop)
    xfDrag = { mode: insidePolygon(pt, corners) ? 'move' : 'rotate', start, orig };
  }

  function xfMove(pt: { x: number; y: number }, shift: boolean) {
    const x = xf;
    const d = xfDrag;
    if (!x || !d) return;
    const o = d.orig;
    const cx = x.box.x + x.box.w / 2;
    const cy = x.box.y + x.box.h / 2;
    if (d.mode === 'move') {
      x.tx = o.tx + pt.x - d.start.x;
      x.ty = o.ty + pt.y - d.start.y;
    } else if (d.mode === 'rotate') {
      const c = { x: cx + o.tx, y: cy + o.ty };
      let a = o.rot + Math.atan2(pt.y - c.y, pt.x - c.x) - Math.atan2(d.start.y - c.y, d.start.x - c.x);
      if (shift) a = Math.round(a / (Math.PI / 12)) * (Math.PI / 12); // 15° steps
      x.rot = a;
    } else if (d.mode === 'scale' && d.anchor && d.sign) {
      // work in the box's own (unrotated) directions, measured from the opposite corner
      const cos = Math.cos(-o.rot), sin = Math.sin(-o.rot);
      const vx = pt.x - d.anchor.x, vy = pt.y - d.anchor.y;
      const lx = vx * cos - vy * sin;
      const ly = vx * sin + vy * cos;
      const W = d.sign.x * x.box.w;
      const H = d.sign.y * x.box.h;
      let nsx: number, nsy: number;
      if (shift) {
        // Shift: free scaling
        nsx = lx / W;
        nsy = ly / H;
      } else {
        // keep proportions
        const ux = W * o.sx, uy = H * o.sy;
        const k = (lx * ux + ly * uy) / (ux * ux + uy * uy);
        nsx = o.sx * k;
        nsy = o.sy * k;
      }
      const clampScale = (v: number) => (Math.abs(v) < 0.01 ? (v < 0 ? -0.01 : 0.01) : v);
      nsx = clampScale(nsx);
      nsy = clampScale(nsy);
      x.sx = nsx;
      x.sy = nsy;
      // place the box so the opposite corner stays where it was
      const ox = (W * nsx) / 2, oy = (H * nsy) / 2;
      const rc = Math.cos(o.rot), rs = Math.sin(o.rot);
      x.tx = d.anchor.x + ox * rc - oy * rs - cx;
      x.ty = d.anchor.y + ox * rs + oy * rc - cy;
    }
    render();
  }

  function xfUp() {
    xfDrag = null;
    if (xf?.quick) applyTransform();
  }

  function applyTransform() {
    const x = xf;
    if (!x) return;
    const layer = layers.find((l) => l.id === x.layerId);
    if (layer) {
      const ctx = layer.canvas.getContext('2d', { willReadFrequently: true })!;
      ctx.clearRect(0, 0, docW, docH);
      drawTransformed(ctx, x);
      const after = ctx.getImageData(0, 0, docW, docH);
      push({ type: 'pixels', layerId: layer.id, rect: fullRect(), before: x.before, after });
      // the selection moves along with what was selected (one undo step with the pixels)
      if (hasSel) {
        const m = xfMatrix(x);
        const moved = ($state.snapshot(selShapes) as SelShape[]).map((sh) => ({
          ...sh,
          pts: sh.pts.map((p) => {
            const q = m.transformPoint(new DOMPoint(p.x, p.y));
            return { x: q.x, y: q.y };
          }),
        }));
        setSelection(moved, false);
      }
    }
    xf = null;
    xfDrag = null;
    thumbTick++;
    render();
  }

  function cancelTransform() {
    xf = null;
    xfDrag = null;
    render();
  }

  function flipTransform(axis: 'h' | 'v') {
    if (!xf) return;
    if (axis === 'h') xf.sx = -xf.sx;
    else xf.sy = -xf.sy;
    render();
  }

  // ---------- Files ----------

  function projectData(): ProjectData {
    return {
      width: docW,
      height: docH,
      paper: { exists: paper.exists, visible: paper.visible, color: paper.color },
      layers: layers.map((l) => ({
        name: l.name,
        visible: l.visible,
        opacity: l.opacity,
        blend: l.blend,
        clip: l.clip,
        lockAlpha: l.lockAlpha,
        canvas: l.canvas,
      })),
    };
  }

  function exportComposite() {
    const c = makeCanvas();
    compositeTo(c.getContext('2d')!, true, false);
    return c;
  }

  async function runFileAction(label: string, action: () => Promise<void> | void) {
    fileMenu = false;
    if (xf) applyTransform();
    try {
      await action();
    } catch (err) {
      alert(`${label} failed: ${err instanceof Error ? err.message : err}`);
    }
  }

  const doSave = () => runFileAction('Saving', () => saveProject(projectData(), `${projectName}.seviath`));
  const doExportPNG = () => runFileAction('PNG export', () => exportPNG(exportComposite(), `${projectName}.png`));
  const doExportPSD = () => runFileAction('PSD export', () => exportPSD(projectData(), exportComposite(), `${projectName}.psd`));

  function openFilePicker() {
    fileMenu = false;
    if (!fileInput) return;
    fileInput.value = '';
    fileInput.click();
  }

  async function onFileChosen() {
    const file = fileInput?.files?.[0];
    if (!file) return;
    if (undoCount > 0 && !confirm('Open this file? The current drawing will be replaced (save it first if you need it).')) return;
    await runFileAction('Opening', async () => {
      const data = file.name.toLowerCase().endsWith('.psd') ? await importPSD(file) : await openProject(file);
      await loadProject(data, file.name.replace(/\.[^.]+$/, ''));
    });
  }

  // Clears everything and sets a new document size
  function resetDocument(w: number, h: number) {
    xf = null;
    xfDrag = null;
    hasSel = false;
    selShapes = [];
    cancelDraft();
    docW = w;
    docH = h;
    makeWorkCanvases();
    undoStack.length = 0;
    redoStack.length = 0;
    sync();
    lastEnd = null;
  }

  async function loadProject(data: ProjectData, name: string) {
    resetDocument(data.width, data.height);
    const list: Layer[] = data.layers.map((l) => ({
      id: nextId++,
      name: l.name,
      canvas: l.canvas,
      visible: l.visible,
      opacity: l.opacity,
      blend: (BLEND_NAMES as string[]).includes(l.blend) ? (l.blend as BlendMode) : 'Normal',
      clip: l.clip,
      lockAlpha: l.lockAlpha,
    }));
    if (!list.length) list.push(makeLayer());
    layers = list;
    activeId = list[list.length - 1].id;
    Object.assign(paper, data.paper);
    projectName = name;
    await tick();
    fitToScreen();
    thumbTick++;
    render();
  }

  async function newProject(r: { width: number; height: number; paper: 'white' | 'transparent' } | null) {
    newDialog = false;
    if (!r) return;
    resetDocument(r.width, r.height);
    const first = makeLayer();
    layers = [first];
    activeId = first.id;
    Object.assign(paper, { exists: true, visible: r.paper === 'white', color: '#ffffff' });
    projectName = 'Untitled';
    await tick();
    fitToScreen();
    thumbTick++;
    render();
  }

  function askNewProject() {
    fileMenu = false;
    if (undoCount > 0 && !confirm('Start a new project? The current drawing will be lost (save it first if you need it).')) return;
    newDialog = true;
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
    const before = ctx.getImageData(0, 0, docW, docH);

    if (upper.visible) {
      let image: CanvasImageSource = upper.canvas;
      if (upper.clip) {
        // a clipped layer only shows inside the lower layer's shape
        const t = clipCtx;
        t.globalCompositeOperation = 'source-over';
        t.clearRect(0, 0, docW, docH);
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

    const after = ctx.getImageData(0, 0, docW, docH);
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
    const before = ctx.getImageData(0, 0, docW, docH);
    ctx.clearRect(0, 0, docW, docH);
    const after = ctx.getImageData(0, 0, docW, docH);
    push({ type: 'pixels', layerId: layer.id, rect: { x: 0, y: 0, w: docW, h: docH }, before, after });
    thumbTick++;
    render();
  }

  function finishAdjust(result: { before: ImageData; after: ImageData } | null) {
    const layer = activeLayer;
    adjusting = false;
    if (result && layer) {
      push({ type: 'pixels', layerId: layer.id, rect: { x: 0, y: 0, w: docW, h: docH }, before: result.before, after: result.after });
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
      case 'selection':
        selShapes = reverse ? e.before : e.after;
        rebuildSelMask();
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
      const scale = Math.min(node.width / docW, node.height / docH);
      const w = docW * scale;
      const h = docH * scale;
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
    if (adjusting || newDialog) return; // the dialog handles its own keys

    // While a shape is being drawn: Enter closes it, Esc cancels, Ctrl+Z removes the last part
    if (selDraft) {
      if (key === 'enter') {
        e.preventDefault();
        finishShape();
        return;
      }
      if (key === 'escape') {
        cancelDraft();
        return;
      }
      if (mod && key === 'z') {
        e.preventDefault();
        undoDraftPart();
        return;
      }
    }

    // File shortcuts
    if (mod && key === 's') {
      e.preventDefault();
      doSave();
      return;
    }
    if (mod && key === 'o') {
      e.preventDefault();
      openFilePicker();
      return;
    }
    if (mod && key === 'e') {
      e.preventDefault();
      if (e.shiftKey) doExportPNG();
      else doExportPSD();
      return;
    }

    // Transform: Enter applies, Esc cancels; other tools wait until it's done
    if (xf) {
      if (key === 'enter') {
        e.preventDefault();
        applyTransform();
      } else if (key === 'escape') {
        cancelTransform();
      } else if (mod && key === 'z') {
        e.preventDefault();
        cancelTransform();
      }
      return;
    }
    if (mod && key === 't') {
      e.preventDefault();
      startTransform(false);
      return;
    }

    // Selection
    if (mod && key === 'a') {
      e.preventDefault();
      selectAll();
      return;
    }
    if (mod && key === 'j') {
      e.preventDefault();
      selectionToNewLayer(e.shiftKey);
      return;
    }
    if (mod && key === 'd') {
      e.preventDefault();
      deselect();
      return;
    }
    if (mod && e.shiftKey && key === 'i') {
      e.preventDefault();
      invertSelection();
      return;
    }
    if ((key === 'delete' || key === 'backspace') && hasSel) {
      e.preventDefault();
      clearSelected();
      return;
    }
    if (mod && key === 'u') {
      e.preventDefault();
      if (activeLayer) adjusting = true;
      return;
    }

    if (key === 'escape' && (layerMenu || fileMenu)) {
      layerMenu = null;
      fileMenu = false;
    } else if (key === 'escape' && hasSel) {
      deselect();
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
    } else if (!mod && key === 'h') {
      toggleFlip();
    } else if (!mod && key === 'g') {
      tool = 'fill';
    } else if (!mod && key === 'm') {
      tool = 'select';
      if (selMode === 'lasso') selMode = 'rect';
    } else if (!mod && key === 'l' && e.shiftKey) {
      tool = 'lassoFill';
    } else if (!mod && key === 'l') {
      tool = 'select';
      selMode = 'lasso';
    } else if (!mod && key === 'v') {
      tool = 'move';
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
    <div class="file-wrap">
      <button class:on={fileMenu} onclick={() => (fileMenu = !fileMenu)}>File ▾</button>
      {#if fileMenu}
        <div class="menu-backdrop" role="presentation" onpointerdown={() => (fileMenu = false)}></div>
        <div class="context-menu file-menu" role="menu">
          <div class="menu-title">{projectName} · {docW} × {docH}</div>
          <button role="menuitem" onclick={askNewProject}>New project…</button>
          <button role="menuitem" onclick={openFilePicker}>Open… (.seviath or .psd) <kbd>Ctrl O</kbd></button>
          <hr />
          <button role="menuitem" onclick={doSave}>Save project (.seviath) <kbd>Ctrl S</kbd></button>
          <hr />
          <button role="menuitem" onclick={doExportPSD}>Export PSD <kbd>Ctrl E</kbd></button>
          <button role="menuitem" onclick={doExportPNG}>Export PNG <kbd>Ctrl Shift E</kbd></button>
        </div>
      {/if}
    </div>
    <div class="group">
      <button class:on={activeTool === 'brush'} onclick={() => (tool = 'brush')} title="Brush (B)">Brush</button>
      <button class:on={activeTool === 'eraser'} onclick={() => (tool = 'eraser')} title="Eraser (E)">Eraser</button>
      <button class:on={activeTool === 'fill'} onclick={() => (tool = 'fill')} title="Fill / paint bucket (G)">Fill</button>
      <button class:on={activeTool === 'lassoFill'} onclick={() => (tool = 'lassoFill')} title="Lasso fill: draw a shape to fill it (Shift L). Hold Ctrl to erase the shape">⬠ Lasso fill</button>
      <button class:on={activeTool === 'picker'} onclick={() => (tool = 'picker')} title="Color picker (I, or hold Alt)">Picker</button>
      <button
        class:on={activeTool === 'select' && selMode !== 'lasso'}
        onclick={() => {
          tool = 'select';
          if (selMode === 'lasso') selMode = 'rect';
        }}
        title="Select: rectangle or ellipse (M)">▭ Select</button
      >
      <button
        class:on={activeTool === 'select' && selMode === 'lasso'}
        onclick={() => {
          tool = 'select';
          selMode = 'lasso';
        }}
        title="Lasso select (L)">✎ Lasso</button
      >
      <button class:on={activeTool === 'move'} onclick={() => (tool = 'move')} title="Move (V)">✥ Move</button>
      <button class:on={!!xf} onclick={() => (xf ? applyTransform() : startTransform(false))} title="Transform: scale and rotate (Ctrl T)">⤡ Transform</button>
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
      <button class:on={flipped} onclick={toggleFlip} title="Flip the view horizontally (H)">⇋ Flip</button>
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
    {#if activeTool === 'fill'}
      <div class="panel-head">Fill</div>
      <div class="tool-options">
        <div class="seg2">
          <button class:on={fillRefer === 'all'} onclick={() => (fillRefer = 'all')} title="Fill areas outlined by any visible layer (e.g. lineart on another layer)">All layers</button>
          <button class:on={fillRefer === 'layer'} onclick={() => (fillRefer = 'layer')} title="Look only at the active layer">This layer</button>
        </div>
        <label class="opt">
          <span>Tolerance</span>
          <input type="range" min="0" max="255" bind:value={fillTolerance} />
          <span class="val">{fillTolerance}</span>
        </label>
        <label class="opt" title="Closes small gaps in the lines so the color doesn't leak out">
          <span>Close gaps</span>
          <input type="range" min="0" max="20" bind:value={fillGap} />
          <span class="val">{fillGap}px</span>
        </label>
        <label class="opt">
          <span>Expand</span>
          <input type="range" min="0" max="8" bind:value={fillExpand} />
          <span class="val">{fillExpand}px</span>
        </label>
        <p class="hint">Click inside an area to fill it with the current color. "All layers" lets you color under lineart that is on a separate layer. Expand slides the color under the lines so no white gaps are left.</p>
      </div>
    {:else if activeTool === 'lassoFill'}
      <div class="panel-head">Lasso fill</div>
      <div class="tool-options">
        <div class="seg3">
          <button class:on={lfMode === 'lasso'} onclick={() => (lfMode = 'lasso')}>✎ Lasso</button>
          <button class:on={lfMode === 'rect'} onclick={() => (lfMode = 'rect')}>▭ Rectangle</button>
          <button class:on={lfMode === 'ellipse'} onclick={() => (lfMode = 'ellipse')}>◯ Ellipse</button>
        </div>
        <label class="opt" title="Soft edge: 0 = sharp">
          <span>Feather</span>
          <input type="range" min="0" max="100" bind:value={lfFeather} />
          <span class="val">{lfFeather}px</span>
        </label>
        <p class="hint">
          {#if lfMode === 'lasso'}Draw a line around the area; when it reaches the start point, the shape is filled. You can lift the pen and continue, or click for straight lines. Double-click or Enter closes it, Esc cancels, Ctrl+Z removes the last part.{:else}Drag to draw the shape; it is filled when you let go. Hold Shift for a square / circle.{/if}
          Hold <b>Ctrl</b> when you start to erase instead of fill.
        </p>
      </div>
    {:else if activeTool === 'select'}
      <div class="panel-head">Select</div>
      <div class="tool-options">
        <div class="seg3">
          <button class:on={selMode === 'rect'} onclick={() => (selMode = 'rect')}>▭ Rectangle</button>
          <button class:on={selMode === 'ellipse'} onclick={() => (selMode = 'ellipse')}>◯ Ellipse</button>
          <button class:on={selMode === 'lasso'} onclick={() => (selMode = 'lasso')}>✎ Lasso</button>
        </div>
        <label class="opt" title="Soft edge: 0 = sharp. Painting, fill and delete fade out softly at the edge.">
          <span>Feather</span>
          <input
            type="range"
            min="0"
            max="100"
            value={selFeather}
            oninput={(e) => featherInput(+e.currentTarget.value)}
            onchange={featherDone}
          />
          <span class="val">{selFeather}px</span>
        </label>
        <p class="hint small-note">{hasSel ? 'Changes the current selection right away.' : 'Used for the next selection you draw.'}</p>
        <p class="hint">
          {#if selMode === 'lasso'}Draw a line around the area; when it reaches the start point, it is selected. You can lift the pen and continue, or click for straight lines. Double-click or Enter closes it, Esc cancels, Ctrl+Z removes the last part.{:else}Drag to select. Shift while dragging keeps it square / round. Click once to deselect.{/if}
          Start with <b>Shift</b> to add, <b>Ctrl</b> to remove. Painting, fill and Move then only affect the selected area.
        </p>
        <div class="btn-grid">
          <button onclick={selectAll}>Select all <kbd>Ctrl A</kbd></button>
          <button onclick={deselect} disabled={!hasSel}>Deselect <kbd>Ctrl D</kbd></button>
          <button onclick={invertSelection}>Invert <kbd>Ctrl Shift I</kbd></button>
          <button onclick={clearSelected} disabled={!hasSel}>Delete inside <kbd>Del</kbd></button>
          <button onclick={deleteOutside} disabled={!hasSel} title="Cut the picture out along the selection">Delete outside (cut out)</button>
          <button onclick={() => selectionToNewLayer(false)} disabled={!hasSel}>Copy to new layer <kbd>Ctrl J</kbd></button>
          <button onclick={() => selectionToNewLayer(true)} disabled={!hasSel}>Cut to new layer <kbd>Ctrl Shift J</kbd></button>
          <button onclick={fillSelected} disabled={!hasSel}>Fill with color</button>
          <button onclick={() => startTransform(false)} disabled={!hasSel}>Transform <kbd>Ctrl T</kbd></button>
        </div>
        <div class="panel-head sub-head">Existing drawing</div>
        <button
          class="wide"
          onclick={() => featherLayerEdges(selFeather)}
          disabled={selFeather <= 0}
          title="Make the outer edges of the active layer's drawing soft, using the Feather value"
          >Feather layer edges ({selFeather}px)</button
        >
        <p class="hint small-note">Softens the outline of what is already drawn on the active layer. Set the amount with Feather above.</p>
      </div>
    {:else if activeTool === 'move'}
      <div class="panel-head">Move</div>
      <div class="tool-options">
        <p class="hint">Drag to move the active layer, or only the selected part if there is a selection. For scaling and rotating use <b>Transform</b> (Ctrl T).</p>
      </div>
    {:else}
      <div class="panel-head">Brush</div>
      <BrushPanel bind:brush bind:preset />
    {/if}
  </aside>

  <main
    class="stage"
    class:pan={spaceDown}
    class:panning
    class:picker={activeTool === 'picker'}
    class:fill-cursor={activeTool === 'fill'}
    class:move-cursor={activeTool === 'move' || !!xf}
    bind:this={stage}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    onpointerleave={() => (hover.on = false)}
    ondblclick={() => selDraft && selDraftOpen && finishShape()}
  >
    <canvas
      class="doc-view"
      bind:this={view}
      width={docW}
      height={docH}
      class:pixelated={zoom >= 2}
      style="width: {docW * zoom}px; height: {docH * zoom}px; transform-origin: {(docW * zoom) / 2}px {(docH * zoom) / 2}px; transform: translate({panX}px, {panY}px) rotate({rotation}deg) scaleX({flipped ? -1 : 1});"
    ></canvas>
    {#if hasSel || selDraft || xf}
      {@const sw = 1.5 / zoom}
      {@const dash = 6 / zoom}
      <svg
        class="overlay"
        viewBox="0 0 {docW} {docH}"
        preserveAspectRatio="none"
        style="width: {docW * zoom}px; height: {docH * zoom}px; transform-origin: {(docW * zoom) / 2}px {(docH * zoom) / 2}px; transform: translate({panX}px, {panY}px) rotate({rotation}deg) scaleX({flipped ? -1 : 1}); --dash: {dash}px;"
      >
        {#if hasSel && !xf}
          {#if selInverted}
            <rect class="ants-bg" x="0" y="0" width={docW} height={docH} stroke-width={sw} />
            <rect class="ants" x="0" y="0" width={docW} height={docH} stroke-width={sw} stroke-dasharray="{dash} {dash}" />
          {/if}
          {#each selShapes as sh, i (i)}
            {@const pts = sh.pts.map((p) => `${p.x},${p.y}`).join(' ')}
            <polygon class="ants-bg" points={pts} stroke-width={sw} />
            <polygon class="ants" class:sub={sh.op === 'sub'} points={pts} stroke-width={sw} stroke-dasharray="{dash} {dash}" />
          {/each}
        {/if}
        {#if selDraft}
          {@const pts = selDraft.map((p) => `${p.x},${p.y}`).join(' ')}
          {#if activeTool === 'lassoFill'}
            <polygon points={pts} fill={lassoErase ? '#ffffff' : color} fill-opacity="0.5" stroke="none" />
          {/if}
          {#if selDraftOpen}
            {@const lastPt = selDraft[selDraft.length - 1]}
            {@const cur = docFromStage(hover.x, hover.y)}
            <polyline class="ants-bg" points={pts} stroke-width={sw} />
            <polyline class="ants" points={pts} stroke-width={sw} stroke-dasharray="{dash} {dash}" />
            {#if !shapeDragging && hover.on}
              <line class="rubber" x1={lastPt.x} y1={lastPt.y} x2={cur.x} y2={cur.y} stroke-width={sw} stroke-dasharray="{dash} {dash}" />
            {/if}
            <circle class="start-dot" cx={selDraft[0].x} cy={selDraft[0].y} r={7 / zoom} stroke-width={sw} />
          {:else}
            <polygon class="ants-bg" points={pts} stroke-width={sw} />
            <polygon class="ants" points={pts} stroke-width={sw} stroke-dasharray="{dash} {dash}" />
          {/if}
        {/if}
        {#if xf && !xf.quick}
          {@const corners = xfCorners(xf)}
          {@const rh = xfRotHandle(xf)}
          <polygon class="xf-box" points={corners.map((p) => `${p.x},${p.y}`).join(' ')} stroke-width={sw} />
          <line class="xf-box" x1={rh.top.x} y1={rh.top.y} x2={rh.handle.x} y2={rh.handle.y} stroke-width={sw} />
          <circle class="xf-handle rot" cx={rh.handle.x} cy={rh.handle.y} r={6 / zoom} stroke-width={sw} />
          {#each corners as c, i (i)}
            <circle class="xf-handle" cx={c.x} cy={c.y} r={6 / zoom} stroke-width={sw} />
          {/each}
        {/if}
      </svg>
    {/if}
    {#if xf && !xf.quick}
      <div class="xf-bar">
        <span>Transform: drag inside to move, corners to scale (Shift = free), outside or ◯ to rotate</span>
        <button onclick={() => flipTransform('h')} title="Flip horizontally">⇋</button>
        <button onclick={() => flipTransform('v')} title="Flip vertically">⇵</button>
        <button onclick={cancelTransform}>Cancel <kbd>Esc</kbd></button>
        <button class="primary" onclick={applyTransform}>Apply <kbd>Enter</kbd></button>
      </div>
    {/if}
    {#if notice}
      <div class="notice">{notice}</div>
    {/if}
    {#if hover.on && !spaceDown && !panning && (activeTool === 'brush' || activeTool === 'eraser') && !xf}
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

{#if newDialog}
  <NewProjectDialog ondone={newProject} />
{/if}

<input bind:this={fileInput} type="file" accept=".seviath,.psd,application/json" hidden onchange={onFileChosen} />

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
    <button role="menuitem" onclick={() => menuDo(() => featherLayerEdges(selFeather || 20))}>
      Feather edges ({selFeather || 20}px)
    </button>
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
  .overlay {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
    overflow: visible;
  }
  .ants-bg {
    fill: none;
    stroke: #fff;
  }
  .ants {
    fill: none;
    stroke: #000;
    animation: ants 0.6s linear infinite;
  }
  .ants.sub {
    stroke: #c0392b;
  }
  @keyframes ants {
    to {
      stroke-dashoffset: calc(var(--dash) * -2);
    }
  }
  .rubber {
    stroke: #000;
    opacity: 0.6;
  }
  .start-dot {
    fill: #fff;
    stroke: #00b5c8;
  }
  .seg3 {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 4px;
  }
  .seg3 button {
    padding: 6px 2px;
    font-size: 11px;
  }
  .xf-box {
    fill: none;
    stroke: #00b5c8;
  }
  .xf-handle {
    fill: #fff;
    stroke: #00b5c8;
  }
  .xf-handle.rot {
    fill: #00b5c8;
  }
  .xf-bar {
    position: absolute;
    top: 10px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 6px 12px;
    background: rgba(30, 30, 36, 0.95);
    border: 1px solid #00b5c8;
    border-radius: 8px;
    font-size: 12px;
    color: #bbb;
    white-space: nowrap;
  }
  .xf-bar .primary {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
  }
  .xf-bar .primary kbd {
    color: #134;
  }
  kbd {
    font-family: inherit;
    font-size: 10px;
    color: #888;
    margin-left: 6px;
  }
  .stage.fill-cursor {
    cursor: cell;
  }
  .stage.move-cursor {
    cursor: move;
  }
  .file-wrap {
    position: relative;
  }
  .file-menu {
    position: absolute;
    top: 36px;
    left: 0;
    min-width: 280px;
  }
  .file-menu button {
    display: flex;
    justify-content: space-between;
  }
  .tool-options {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 12px;
    color: #bbb;
  }
  .seg2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
  }
  .opt {
    display: grid;
    grid-template-columns: 64px 1fr 36px;
    align-items: center;
    gap: 6px;
  }
  .opt input {
    width: 100%;
    min-width: 0;
  }
  .val {
    text-align: right;
    color: #888;
  }
  .hint {
    margin: 0;
    color: #888;
    line-height: 1.45;
  }
  .sub-head {
    margin-top: 8px;
    font-size: 12px;
    color: #ccc;
  }
  .wide {
    width: 100%;
    font-size: 12px;
  }
  .small-note {
    margin-top: -6px;
    font-size: 11px;
  }
  .btn-grid {
    display: grid;
    grid-template-columns: 1fr;
    gap: 4px;
  }
  .btn-grid button {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
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
