<script lang="ts">
  import { onMount, tick, flushSync, untrack } from 'svelte';
  import { previewFilter, cmykById } from './lib/colorprofiles';
  import ColorWheel from './lib/ColorWheel.svelte';
  import BrushPanel from './lib/BrushPanel.svelte';
  import AdjustColors from './lib/AdjustColors.svelte';
  import NewProjectDialog from './lib/NewProjectDialog.svelte';
  import ProjectManager from './lib/ProjectManager.svelte';
  import ExportDialog from './lib/ExportDialog.svelte';
  import PressureEditor from './lib/PressureEditor.svelte';
  import GradientPanel, { type GradApply } from './lib/GradientPanel.svelte';
  import { renderGradient, strokeColorAt, cssGradient, type GradientSettings, type GradRepeat } from './lib/gradient';
  import { recognize } from './lib/shapes';
  import TraceDialog from './lib/TraceDialog.svelte';
  import type { NewProjectResult } from './lib/NewProjectDialog.svelte';
  import { cleanLine, renderVectorLayer, renderVectorRegion, changedArea, eraseStrokes, mapStroke, drawStroke, strokeBounds, drawLivePiece, exactLine, fillBounds, insideFill, mapFill, type VecFill, hitStroke, strokesInBox, smoothStroke, simplifyStroke, setPressures, densifyStroke, nodeCount, nearestNode, moveNode, deleteNode, insertNode, keyNodes, type VecLayer, type VecStroke, type VPt, type EraseMode } from './lib/vector';
  import { applyPrintPreview, setPrintProfile, type ColorMode } from './lib/color';
  import { exportAs, type ExportOptions } from './lib/export';
  import {
    symCopies,
    axisLines,
    sameSpot,
    nextMode,
    MODES,
    MODE_LABEL,
    MODE_HINT,
    type SymmetryState,
    type SymmetryMode,
    type PtMap,
  } from './lib/symmetry';
  import { saveToLibrary, saveRecovery, loadFromLibrary, newId, canvasToBlob, listBrushes, saveBrushes, deleteBrush } from './lib/library';
  import { floodFill, expandMask } from './lib/fill';
  import { saveProject, exportPNG, exportPSD, openAnyFile, ACCEPT, download, type ProjectData } from './lib/fileio';
  import BrushLibrary, { type ShareRequest } from './lib/BrushLibrary.svelte';
  import AccountPanel from './lib/AccountPanel.svelte';
  import FilterDialog from './lib/FilterDialog.svelte';
  import SubView from './lib/SubView.svelte';
  import RecorderPanel from './lib/RecorderPanel.svelte';
  import { recorder } from './lib/recorder.svelte';
  import SettingsDialog, { loadPrefs, savePrefs, mapPressure, comboOf, type Prefs } from './lib/SettingsDialog.svelte';
  import { cloudReady, watchAccount, onPasswordRecovery, type Account } from './lib/cloud';
  import { brushFile, imageToTip, type ImportedBrush } from './lib/brushimport';
  import { PRESETS, ERASER_PRESETS, surpriseBrush, keepSizeFeel, settingsOf, getDab, getPaper, ensureTip, isShaped, dabShape, stampDab, jitterColor, pickVariant, particleSpread, getTexture, getTextureColors, dirTexture, followsStroke, strokeEdgePad, applyStrokeEdgesDirect, setBrushPerf, brushPerf, previewIsLighter, hasEdgeEffects, hasStrokeEdges, applyStrokeEdges, type BrushSettings, type BrushPreset } from './lib/brush';

  // Document size (chosen in New project, or taken from an opened file)
  let docW = $state(2000);
  let docH = $state(1400);
  let projectName = $state('Untitled');
  // Settings (⚙): brush speed/quality, GPU or CPU, undo steps
  let prefs = $state(loadPrefs());
  let settingsOpen = $state(false);
  // Sub view (references / navigator) and the session recorder: floating windows
  let subOpen = $state(readFlag('seviath.subview'));
  let recOpen = $state(false);
  function readFlag(k: string) {
    try {
      return localStorage.getItem(k) === '1';
    } catch {
      return false;
    }
  }
  $effect(() => {
    try {
      localStorage.setItem('seviath.subview', subOpen ? '1' : '0');
    } catch {
      /* not remembered */
    }
  });
  let filterOpen = $state(false);
  let penEraserFrom: Tool | null = null; // tool to go back to after the pen's eraser end
  $effect(() => {
    savePrefs($state.snapshot(prefs) as Prefs);
    setBrushPerf(prefs.quality, prefs.wetEdges);
    // a different press or rendering intent: redraw the print preview
    if (setPrintProfile(cmykById(prefs.cmykProfile), prefs.intent) && colorMode === 'cmyk') untrack(() => render());
  });

  // GPU or CPU is decided when the page starts (canvases keep their kind)
  // svelte-ignore state_referenced_locally
  const cpuCanvases = prefs.graphics === 'cpu';

  type Tool = 'brush' | 'eraser' | 'picker' | 'fill' | 'lassoFill' | 'select' | 'move' | 'vselect' | 'gradient';
  type Layer = {
    id: number;
    name: string;
    canvas: HTMLCanvasElement;
    visible: boolean;
    opacity: number;
    blend: BlendMode;
    clip: boolean; // clipping mask: only shows on top of the layer below
    lockAlpha: boolean; // paint only where the layer already has paint
    kind?: 'raster' | 'vector'; // vector layers keep their lines (see vecData)
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
    | { type: 'layerProps'; layerId: number; before: LayerProps; after: LayerProps }
    | { type: 'vector'; layerId: number; before: VecLayer; after: VecLayer }
    | { type: 'doc'; before: DocSettings; after: DocSettings };

  type DocSettings = { dpi: number; colorMode: ColorMode };

  type PaperState = { exists: boolean; visible: boolean; color: string };
  // The settings of a layer that can be undone (everything except its pixels)
  type LayerProps = Pick<Layer, 'name' | 'visible' | 'opacity' | 'blend' | 'clip' | 'lockAlpha'>;

  // Tool settings
  let tool = $state<Tool>('brush');
  let color = $state('#222222');
  let brush = $state<BrushSettings>(settingsOf(PRESETS[0]));
  let preset = $state(PRESETS[0].name);
  // The brush and the eraser remember their own settings: switching tools swaps them
  let stash = {
    brush: { settings: settingsOf(PRESETS[0]), preset: PRESETS[0].name },
    eraser: { settings: settingsOf(ERASER_PRESETS[0]), preset: ERASER_PRESETS[0].name },
  };
  let settingsFor: 'brush' | 'eraser' = 'brush';
  $effect(() => {
    const want = tool === 'eraser' ? 'eraser' : 'brush';
    if (want === settingsFor) return;
    stash[settingsFor] = { settings: { ...$state.snapshot(brush) }, preset };
    Object.assign(brush, stash[want].settings);
    preset = stash[want].preset;
    settingsFor = want;
  });
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
  let fileMode: 'open' | 'layer' = 'open';
  // Document: print resolution and color mode
  let docDpi = $state(72);
  let colorMode = $state<ColorMode>('rgb');
  // the canvas is shown through the RGB profile (an SVG filter on the graphics card)
  let profileView = $derived(colorMode === 'rgb' ? previewFilter(prefs.rgbProfile) : null);
  let printPreview = $state(true); // CMYK projects: show colors as they will print
  let proofTimer: ReturnType<typeof setTimeout> | undefined;

  // Vector layers: their lines live here (outside Svelte's state, so big lists stay fast).
  // A VecLayer is never changed in place; every edit makes a new one, so undo can keep the old.
  const vecData = new Map<number, VecLayer>();
  let vecTick = $state(0); // changes whenever vector lines change, so the panel updates
  let vecSmooth = $state(0.15); // how much new vector lines are smoothed
  let vecEraseMode = $state<EraseMode>('cut');
  let vecWidthBefore: VecLayer | null = null; // while dragging the line width slider
  let strokeVector = false; // the current stroke is on a vector layer
  let vecPreviewDirty = false;

  // Your own brushes (saved or imported) and the brush library window
  type MyBrush = BrushPreset & { id: string; source?: string };
  let myBrushes = $state.raw<MyBrush[]>([]);
  let libOpen = $state(false);
  // Seviath account (Google or email), used for sharing brushes
  let account = $state.raw<Account | null>(null);
  let accountOpen = $state(false);
  let recovery = $state(false); // opened from a "set a new password" email
  $effect(() => watchAccount((a) => (account = a)));
  $effect(() =>
    onPasswordRecovery(() => {
      recovery = true;
      accountOpen = true;
    }),
  );
  let shareReq = $state<ShareRequest | null>(null); // brush waiting in the share window
  let libTab = $state<'mine' | 'import' | 'community'>('mine');

  // Auto trace window (raster layer → new vector layer)
  let traceFor = $state<null | { layerId: number; bounds: Rect }>(null);
  let vselF = $state.raw<VecFill[]>([]); // selected color shapes (vector select tool)

  // Gradient tool and painting with a gradient
  let grad = $state<GradientSettings>({
    stops: [
      { pos: 0, color: '#3fb8e6', alpha: 1 },
      { pos: 1, color: '#ffd93d', alpha: 1 },
    ],
    shape: 'linear',
    repeat: 'none',
    smooth: true,
    dither: true,
    opacity: 1,
  });
  let gradApply = $state<GradApply>({ target: 'new', where: 'drawing', blend: 'Normal' });
  let color2 = $state('#ffffff');
  let gradDrag = $state<null | { p0: { x: number; y: number }; p1: { x: number; y: number } }>(null);
  let gradCanvas: HTMLCanvasElement | null = null; // the gradient being placed (doc size)
  let gradAutoStops = JSON.stringify([
    { pos: 0, color: '#222222', alpha: 1 },
    { pos: 1, color: '#222222', alpha: 0 },
  ]); // the starting "color → transparent" follows the current color until you change it
  let gradBaseEmpty = false; // "only on the drawing" but there is no drawing: put it everywhere
  let gradMixCtx: CanvasRenderingContext2D | null = null; // layer + gradient, for "into this layer"
  let gradBrush = $state(false); // paint with the gradient
  let gradBrushLen = $state(300); // screen px for one run of the gradient along a stroke (same feel at any zoom)
  let strokeZoom = 1; // the zoom when the stroke started
  let gradBrushRepeat = $state<GradRepeat>('mirror');
  let gradBrushEdit = $state(false); // the gradient colors editor open in the brush panel
  let curDabD = 0; // distance along the stroke of the dab being drawn

  // Hold to straighten: keep the pen still at the end of a line and it becomes a clean shape
  let holdShapes = $state(true);
  let shapeLocked = false; // the line already became a shape; the pen no longer changes it
  let holdTimer: ReturnType<typeof setTimeout> | undefined;
  let holdAnchor = { x: 0, y: 0 };
  let replaying = false; // redrawing a recognized shape: don't record its points again
  let penPts: VPt[] = []; // where the pen really went (before the stabilizer), for shapes
  let shapeClosed = false; // the recognized shape is closed (circle, box…): no pointed ends

  // Vector select tool: the chosen lines (plain references, not Svelte proxies)
  let vsel = $state.raw<VecStroke[]>([]);
  // plain object on purpose: Svelte state would wrap the lines and break "same line" checks
  let vselBox = $state<null | { x0: number; y0: number; x1: number; y1: number }>(null); // box being dragged
  let vselDrag: null | {
    mode: 'move' | 'box' | 'node';
    node?: { s: VecStroke; k: number }; // the grabbed node (node mode)
    dbl?: boolean; // second press of a double-click: acts on release if the pen didn't move
    start: { x: number; y: number };
    cur: { x: number; y: number };
    before: VecLayer; // layer when the drag started
    shown: VecLayer; // what is on screen now
    picked: VecStroke[]; // the selected lines when the drag started
    pickedF: VecFill[]; // and the selected color shapes
    moved: boolean;
    add: boolean;
  } = null;
  const NODE_GAP = 28; // screen px between the node handles shown on dense lines
  let nodeSoft = $state(0); // node dragging: nearby nodes within this many px follow
  let vselLast = { t: 0, x: 0, y: 0 }; // for double-click on a line (adds a node)
  let vselLayerId = 0;
  // A selection belongs to one layer: switching or deleting the layer drops it
  $effect(() => {
    const id = activeId;
    const visible = activeLayer?.visible;
    if (id !== vselLayerId || visible === false) {
      vselLayerId = id;
      vsel = [];
      vselF = [];
      vselBox = null;
      vselDrag = null;
    }
  });
  let prDense: VecStroke | null = null; // pressure editing works on a finely pointed copy
  let prBefore: VecLayer | null = null;
  let vecRaw: VPt[] = []; // the points of the current vector stroke

  let toolsMenu = $state(false);
  // Toolbar menus are placed under their button on the page itself, so the toolbar
  // (which scrolls sideways on narrow windows) can't cut them off
  let dropPos = $state({ x: 0, y: 0 });
  function dropAt(e: MouseEvent, open: boolean) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    dropPos = { x: Math.max(4, Math.min(r.left, window.innerWidth - 300)), y: r.bottom + 4 };
    return open;
  }
  let funcMenu = $state(false);
  let exportDialog = $state(false);
  let exportOpts = $state<ExportOptions>({ format: 'psd', scale: 1, quality: 0.92, transparent: false, cmyk: true, printColors: true });

  // Mirror drawing (symmetry). The center can be dragged on the canvas.
  let sym = $state<SymmetryState>({ mode: 'off', cx: 1000, cy: 700, count: 6, angle: 0 });
  let symDrag = $state(false);
  let symLocked = $state(false); // stops the center from being dragged by accident
  let symLastMode: SymmetryMode = 'vertical'; // what the toolbar mirror button turns on
  let strokeMaps: PtMap[] = []; // mirror copies for the stroke being drawn
  // color-mixing brushes keep separate paint for every mirrored copy
  let mixStates: { p: number[]; ls: { x: number; y: number } }[] = []; // Open as a new project, or add to this one as layers

  // Project library (autosave)
  let showProjects = $state(true); // the project manager is the start page
  let currentId = $state<string | null>(null);
  let saveState = $state<'saved' | 'saving' | 'unsaved'>('saved');
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let saving: Promise<void> | null = null;
  let saveAgain = false;
  const blobCache = new Map<number, Blob>(); // layer id → its last saved PNG
  const dirtyLayers = new Set<number>(); // layers whose pixels changed since the last save

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
  let edgedCtx: CanvasRenderingContext2D; // High quality: the stroke with its finished edges, while drawing
  let liveEdges = false; // this stroke shows its finished look (edges, rim, deform) while you draw
  let liveTmp: CanvasRenderingContext2D | null = null;
  let refCtx: CanvasRenderingContext2D; // everything else you see, for brushes that mix with all layers
  let tintCtx: CanvasRenderingContext2D; // small canvas used to color one brush tip
  let paperPattern: CanvasPattern | null = null;
  let colorPattern: CanvasPattern | null = null; // the texture picture's colors (Keep texture colors)
  let texPixels: { w: number; h: number; d: Uint8ClampedArray } | null = null; // the same, for color-mixing brushes

  // Color-mixing brushes load their paint from the texture picture at the spot being painted
  function paintColorAt(x: number, y: number, base: string) {
    if (!texPixels || brush.texColor <= 0) return base;
    const sc = Math.max(0.25, brush.texScale ?? 1);
    const { w, h, d } = texPixels;
    const u = ((Math.floor((x - grainOffset.x) / sc) % w) + w) % w;
    const v = ((Math.floor((y - grainOffset.y) / sc) % h) + h) % h;
    const o = (v * w + u) * 4;
    const [r, g, b] = hexRGB(base);
    const k = Math.min(1, brush.texColor) * (d[o + 3] / 255);
    return '#' + [r + (d[o] - r) * k, g + (d[o + 1] - g) * k, b + (d[o + 2] - b) * k].map((c) => Math.round(c).toString(16).padStart(2, '0')).join('');
  }
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

  // A layer canvas: its drawing context is made right away so it stays on the graphics card
  // (a canvas keeps the kind of context it got first)
  function makeLayerCanvas() {
    const c = makeCanvas();
    c.getContext('2d', cpuCanvases ? { willReadFrequently: true } : undefined);
    return c;
  }

  function makeLayer(): Layer {
    const id = nextId++;
    return {
      id,
      name: `Layer ${id}`,
      canvas: makeLayerCanvas(),
      visible: true,
      opacity: 1,
      blend: 'Normal',
      clip: false,
      lockAlpha: false,
      kind: 'raster',
    };
  }

  function makeVectorLayer(): Layer {
    const l = makeLayer();
    l.kind = 'vector';
    l.name = `Vector ${l.id}`;
    vecData.set(l.id, { strokes: [], width: 1 });
    return l;
  }

  const isVector = (l: Layer | null | undefined) => !!l && l.kind === 'vector';

  // Redraws a vector layer's pixels from its lines
  function drawVector(l: Layer) {
    const v = vecData.get(l.id);
    if (v) renderVectorLayer(l.canvas, v);
    thumbTick++;
    vecTick++;
  }

  // Replaces a vector layer's lines (with undo)
  function setVector(l: Layer, after: VecLayer, before = vecData.get(l.id)!) {
    const area = showVector(l, before, after);
    push({ type: 'vector', layerId: l.id, before, after });
    render(area);
  }

  // Switches a vector layer to another version, redrawing only the part that changed.
  // Returns the changed area (undefined = everything).
  function showVector(l: Layer, from: VecLayer, to: VecLayer): Rect | undefined {
    vecData.set(l.id, to);
    const area = changedArea(from, to);
    if (area === null) return { x: 0, y: 0, w: 0, h: 0 };
    if (area === 'all') {
      drawVector(l);
      return undefined;
    }
    renderVectorRegion(l.canvas, to, area);
    thumbTick++;
    vecTick++;
    return area;
  }

  // A plain copy of a layer's settings (history keeps these, not the live proxies)
  const copyLayer = (l: Layer): Layer => ({ ...l });

  const activeIndex = () => layers.findIndex((l) => l.id === activeId);

  // The helper canvases are as big as the document, so they are rebuilt when its size changes
  function makeWorkCanvases() {
    strokeCtx = makeCanvas().getContext('2d', cpuCanvases ? { willReadFrequently: true } : undefined)!;
    scratchCtx = makeCanvas().getContext('2d', cpuCanvases ? { willReadFrequently: true } : undefined)!;
    groupCtx = makeCanvas().getContext('2d', cpuCanvases ? { willReadFrequently: true } : undefined)!;
    clipCtx = makeCanvas().getContext('2d', cpuCanvases ? { willReadFrequently: true } : undefined)!;
    texturedCtx = makeCanvas().getContext('2d', cpuCanvases ? { willReadFrequently: true } : undefined)!;
    backupCtx = makeCanvas().getContext('2d', { willReadFrequently: true })!;
    edgedCtx = makeCanvas().getContext('2d')!;
    selCtx = makeCanvas().getContext('2d', cpuCanvases ? { willReadFrequently: true } : undefined)!;
    refCtx = makeCanvas().getContext('2d', { willReadFrequently: true })!;
    paperPattern = texturedCtx.createPattern(getPaper(), 'repeat');
  }

  onMount(() => {
    // The screen canvas and the layers stay on the graphics card (fast to draw and show).
    // Only helper canvases that are read pixel by pixel all the time live in normal memory.
    viewCtx = view!.getContext('2d')!;
    makeWorkCanvases();
    centerSym();
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
    loadMyBrushes();
    window.addEventListener('dragover', onWindowDragOver);
    window.addEventListener('drop', onWindowDrop);
    window.addEventListener('paste', onPaste);
    window.addEventListener('beforeunload', onBeforeUnload);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.removeEventListener('dragover', onWindowDragOver);
      window.removeEventListener('drop', onWindowDrop);
      window.removeEventListener('paste', onPaste);
      window.removeEventListener('beforeunload', onBeforeUnload);
      document.removeEventListener('visibilitychange', onVisibility);
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
  // Redraws the screen on the next frame. With `area`, only that part is redrawn: while
  // drawing that is just around the stroke, which keeps big canvases fast on any computer.
  let pendingArea: Rect | 'all' | null = null;

  function unionRect(a: Rect | null, b: Rect): Rect {
    if (!a) return { ...b };
    const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
    return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
  }

  // Whole pixels inside the canvas, with a small margin for soft edges
  function clampRect(r: Rect): Rect | null {
    const x = Math.max(0, Math.floor(r.x) - 2), y = Math.max(0, Math.floor(r.y) - 2);
    const w = Math.min(docW, Math.ceil(r.x + r.w) + 2) - x;
    const h = Math.min(docH, Math.ceil(r.y + r.h) + 2) - y;
    return w > 0 && h > 0 ? { x, y, w, h } : null;
  }

  function render(area?: Rect | null) {
    if (!viewCtx) return;
    if (!area) pendingArea = 'all';
    else if (pendingArea !== 'all') pendingArea = unionRect(pendingArea, area);
    requestFrame();
  }

  // Adds an area to the next redraw without asking for a frame yet
  function markArea(area: Rect) {
    if (pendingArea !== 'all') pendingArea = unionRect(pendingArea, area);
  }

  let lastFrameAt = 0;
  function requestFrame() {
    if (!viewCtx || frame) return;
    frame = requestAnimationFrame(function draw(now) {
      // "Lighter" redraw (Preferences → Canvas): at most 30 times a second while drawing
      if (prefs.fps === 30 && drawing && now - lastFrameAt < 30) {
        frame = requestAnimationFrame(draw);
        return;
      }
      lastFrameAt = now;
      frame = 0;
      // the vector line is rebuilt once per screen frame, not on every pen event
      if (vecPreviewDirty && drawing && strokeVector) {
        const a = vectorPreview();
        if (a && pendingArea !== 'all') pendingArea = unionRect(pendingArea, a);
      }
      vecPreviewDirty = false;
      const R = pendingArea === 'all' || !pendingArea ? undefined : (clampRect(pendingArea) ?? undefined);
      const all = pendingArea === 'all';
      pendingArea = null;
      if (!all && !R) return;
      compositeTo(viewCtx!, true, true, -999, R);
      if (colorMode === 'cmyk' && printPreview) {
        // a small area is quick; the whole canvas waits until you stop drawing or dragging
        clearTimeout(proofTimer);
        if (R) proofView(R);
        else if (drawing || xfDrag || selDraft || symDrag) proofTimer = setTimeout(() => render(), 200);
        else proofView();
      }
    });
  }

  function proofView(R?: Rect) {
    if (!viewCtx) return;
    const r = R ?? { x: 0, y: 0, w: docW, h: docH };
    const img = viewCtx.getImageData(r.x, r.y, r.w, r.h);
    applyPrintPreview(img.data);
    viewCtx.putImageData(img, r.x, r.y);
  }

  // Draws all layers into ctx. `live` includes the unfinished stroke and transform previews.
  // `R` limits the work to one area (the rest of ctx is left as it is).
  function compositeTo(ctx: CanvasRenderingContext2D, withPaper: boolean, live: boolean, excludeId = -999, R?: Rect) {
    const A = R ?? { x: 0, y: 0, w: docW, h: docH };
    // copies the area A from one canvas to the same place in another
    const blit = (c: CanvasRenderingContext2D, img: CanvasImageSource) => c.drawImage(img, A.x, A.y, A.w, A.h, A.x, A.y, A.w, A.h);
    const strokeView = live && drawing && !directStroke ? previewStrokeLayer(A) : null;
    const xfView = live && xf ? previewTransform() : null;
    // while placing a gradient: shown as an extra layer, or mixed into the active layer
    const gv = live && gradDrag && gradCanvas ? gradientPlan() : null;
    const gradInto = gv && gv.into ? gradientMixed(gv.into) : null;
    const surface = (l: Layer): CanvasImageSource => {
      if (strokeView && l.id === strokeLayerId) return strokeView;
      if (xfView && xf && l.id === xf.layerId) return xfView;
      if (gradInto && gv?.into && l.id === gv.into.id) return gradInto;
      return l.canvas;
    };
    let list = layers;
    if (gv && gv.layer) {
      list = [...layers];
      list.splice(gv.index, 0, gv.layer);
    }

    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
    if (withPaper && paper.exists && paper.visible) {
      ctx.fillStyle = paper.color;
      ctx.fillRect(A.x, A.y, A.w, A.h);
    } else {
      // transparent: the checkerboard behind the canvas shows through
      ctx.clearRect(A.x, A.y, A.w, A.h);
    }

    // Walk up the stack; each normal layer may have clipped layers right above it
    let i = 0;
    while (i < list.length) {
      const base = list[i];
      let j = i + 1;
      while (j < list.length && list[j].clip) j++;
      const clipped = list.slice(i + 1, j).filter((c) => c.visible && c.id !== excludeId);

      if (base.visible && base.id !== excludeId) {
        let image: CanvasImageSource = surface(base);
        if (clipped.length) {
          const g = groupCtx;
          g.globalAlpha = 1;
          g.globalCompositeOperation = 'source-over';
          g.clearRect(A.x, A.y, A.w, A.h);
          blit(g, image);
          for (const c of clipped) {
            // cut the clipped layer to the base layer's shape, then blend it on
            const t = clipCtx;
            t.globalCompositeOperation = 'source-over';
            t.clearRect(A.x, A.y, A.w, A.h);
            blit(t, surface(c));
            t.globalCompositeOperation = 'destination-in';
            blit(t, image);
            t.globalCompositeOperation = 'source-over';
            g.globalAlpha = c.opacity;
            g.globalCompositeOperation = opOf(c.blend);
            blit(g, t.canvas);
          }
          g.globalAlpha = 1;
          g.globalCompositeOperation = 'source-over';
          image = g.canvas;
        }
        ctx.globalAlpha = base.opacity;
        ctx.globalCompositeOperation = opOf(base.blend);
        blit(ctx, image);
      }
      i = j;
    }
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
  }

  // The layer being painted on, with the unfinished stroke shown on top of it (area A only)
  function previewStrokeLayer(A: Rect): HTMLCanvasElement | null {
    const l = layers.find((x) => x.id === strokeLayerId);
    if (!l) return null;
    const s = scratchCtx;
    const blit = (img: CanvasImageSource) => s.drawImage(img, A.x, A.y, A.w, A.h, A.x, A.y, A.w, A.h);
    s.globalCompositeOperation = 'source-over';
    s.globalAlpha = 1;
    s.clearRect(A.x, A.y, A.w, A.h);
    blit(l.canvas);
    if (liveEdges) updateLiveEdges(A);
    s.globalAlpha = brush.opacity;
    s.globalCompositeOperation = strokeTool === 'eraser' ? 'destination-out' : 'source-over';
    blit(strokeSource(l, A, liveEdges));
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

  // Navigator: put a document point in the middle of the screen
  function centerOn(dx: number, dy: number) {
    if (!stage) return;
    const c = stageCenter();
    const p = stageFromDocNoPan(dx, dy);
    panX = c.x - p.x;
    panY = c.y - p.y;
  }
  // the four corners of the screen, in document pixels (the frame in the navigator)
  function viewCorners() {
    if (!stage) return [];
    const r = stage.getBoundingClientRect();
    return [docFromStage(0, 0), docFromStage(r.width, 0), docFromStage(r.width, r.height), docFromStage(0, r.height)];
  }
  function setZoomCenter(z: number) {
    if (!stage) return;
    const c = stageCenter();
    zoomAt(z, c.x, c.y);
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
    // leave any text box or slider, so keyboard shortcuts work again on the canvas
    const focused = document.activeElement as HTMLElement | null;
    if (focused && focused !== document.body) focused.blur();
    // Fingers on a touch screen: draw, move the canvas, or nothing (Preferences → Tablet)
    if (e.pointerType === 'touch' && prefs.touch === 'ignore') return;
    const touchPan = e.pointerType === 'touch' && prefs.touch === 'pan';
    // The eraser end of the pen erases, then the pen goes back to the tool it had
    if (prefs.penEraser && e.pointerType === 'pen' && (e.buttons & 32) && tool !== 'eraser' && !penEraserFrom) {
      penEraserFrom = tool;
      tool = 'eraser';
      flushSync();
    }
    // Middle mouse button or Space + drag moves the canvas
    if (e.button === 1 || spaceDown || touchPan) {
      e.preventDefault();
      capture(e);
      panning = true;
      panStart = { x: e.clientX, y: e.clientY, px: panX, py: panY };
      return;
    }
    if (e.button !== 0) return;
    // No layers left (all deleted): painting makes a new layer to paint on
    if (!layers.length && (activeTool === 'brush' || activeTool === 'fill' || activeTool === 'lassoFill' || activeTool === 'gradient')) {
      addLayer();
      thumbTick++;
    }
    // Mirror drawing: the ◎ handle moves the center of the axes
    if (sym.mode !== 'off' && !xf && !symLocked) {
      const d = toDoc(e.clientX, e.clientY);
      if (Math.hypot(d.x - sym.cx, d.y - sym.cy) <= 12 / zoom) {
        capture(e);
        symDrag = true;
        return;
      }
    }
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
    if (activeTool === 'vselect') {
      vselDown(e, pt);
      return;
    }
    if (activeTool === 'gradient') {
      gradDown(e, pt);
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
    if (symDrag) {
      moveSymCenter(toDoc(e.clientX, e.clientY));
      return;
    }
    if (xfDrag) {
      xfMove(toDoc(e.clientX, e.clientY), e.shiftKey);
      return;
    }
    if (vselDrag) {
      vselMove(toDoc(e.clientX, e.clientY));
      return;
    }
    if (gradDrag) {
      gradMove(toDoc(e.clientX, e.clientY), e.shiftKey);
      return;
    }
    if (selDraft) {
      shapeMove(toDoc(e.clientX, e.clientY), e.shiftKey);
      return;
    }
    move(e);
  }

  function onPointerUp() {
    // pen turned back around: back to the tool before the eraser end
    if (penEraserFrom) {
      const back = penEraserFrom;
      penEraserFrom = null;
      queueMicrotask(() => (tool = back));
    }
    if (panning) {
      panning = false;
      return;
    }
    if (picking) {
      picking = false;
      return;
    }
    if (symDrag) {
      symDrag = false;
      return;
    }
    if (xfDrag) {
      xfUp();
      return;
    }
    if (vselDrag) {
      vselUp();
      return;
    }
    if (gradDrag) {
      gradUp();
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
    // A mouse has no pressure: it paints at the pressure set in Preferences → Tablet.
    // Pen pressure goes through your pressure curve.
    return { x: d.x, y: d.y, p: e.pointerType === 'mouse' ? prefs.mousePressure : mapPressure(e.pressure || 0.01, prefs) };
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
    // Anti-aliasing off: dabs are stamped pixel-sharp (no smoothing when scaled)
    smoothing(layer, brush.antialias !== false || isVector(layer));
    strokeDabs = [];
    // colour-mixing strokes paint straight onto the layer, so they always use full-quality tips
    const mixing = activeTool === 'brush' && brush.mix > 0 && !isVector(layer);
    recordDabs = !mixing && previewIsLighter() && hasEdgeEffects(brush) && !isVector(layer);
    brushPerf.final = mixing;
    resetDabs();
    // the brush texture (paper grain, hair, cloth…). Paper grain shifts a little every stroke so
    // going over an area fills it in; other textures stay put so their pattern lines up.
    paperPattern = texturedCtx.createPattern(getTexture(brush, () => render()), 'repeat');
    // "Keep texture colors": the paint takes the colors of your texture picture
    const texCols = brush.texColor > 0 && strokeTool === 'brush' && !isVector(layer) ? getTextureColors(brush, () => render()) : null;
    colorPattern = texCols ? texturedCtx.createPattern(texCols, 'repeat') : null;
    texPixels = texCols ? { w: texCols.width, h: texCols.height, d: texCols.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, texCols.width, texCols.height).data } : null;
    strokeZoom = zoom;
    grainOffset = (brush.texKind || 'paper') === 'paper' ? { x: Math.random() * 256, y: Math.random() * 256 } : { x: 0, y: 0 };

    strokeMaps = sym.mode === 'off' ? [] : symCopies(sym);
    strokeVector = isVector(layer);
    vecRaw = [{ ...(e.shiftKey && lastEnd && lastEnd.layerId === layer.id ? lastEnd.pt : pt) }];
    // vector lines are clean lines: no color mixing
    directStroke = strokeTool === 'brush' && brush.mix > 0 && !strokeVector;
    // High quality: show the finished edges (outline, rim, deform, grain) live while drawing
    liveEdges = brushPerf.quality === 'high' && !directStroke && !strokeVector && hasStrokeEdges(brush);
    if (liveEdges) edgedCtx.clearRect(0, 0, docW, docH);
    if (directStroke) {
      backupCtx.clearRect(0, 0, docW, docH);
      backupCtx.drawImage(layer.canvas, 0, 0);
      if (brush.mixRefer === 'all') {
        // a picture of everything except this layer; the layer itself is read live on top of it
        compositeTo(refCtx, true, false, layer.id);
      }
      const [r0, g0, b0] = hexRGB(gradBrush ? strokeColorAt(grad.stops, grad.smooth, 0, gradBrushLen / strokeZoom, gradBrushRepeat).hex : color);
      mixP = [r0, g0, b0, 1];
      lastSample = { x: -1e9, y: -1e9 };
      // start with the color that's already under the brush (for every mirrored copy too)
      mixStates = [];
      for (const f of [(q: { x: number; y: number }) => q, ...strokeMaps]) {
        mixP = [r0, g0, b0, 1];
        lastSample = { x: -1e9, y: -1e9 };
        sampleCanvas(layer, { ...pt, ...f(pt) }, brush.mix);
        mixStates.push({ p: mixP, ls: lastSample });
      }
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
    drawnTo = pt;
    curveCtrl = null;
    ropeEnd = { ...pt };
    vecDrawn = 0;
    vecDist = [];
    dabLast = [];
    ensureTip(brush, () => render());
    shapeLocked = false;
    shapeClosed = false;
    penPts = [{ x: pt.x, y: pt.y, p: pt.p }];
    holdAnchor = { x: pt.x, y: pt.y };
    clearTimeout(holdTimer);
    if (holdShapes && !directStroke) holdTimer = setTimeout(tryShape, 600);
    stamp(pt);
    if (strokeVector) vecPreviewDirty = true;
    requestFrame();
  }

  function move(e: PointerEvent) {
    if (!drawing || shapeLocked) return;
    // every pen sample counts (fast strokes would turn into straight pieces otherwise);
    // "Prefer speed" only skips samples that are very close together
    let events = e.getCoalescedEvents ? e.getCoalescedEvents() : [];
    if (events.length === 0) events = [e];
    if (prefs.input === 'speed' && events.length > 2) {
      const minGap = 2.5 / zoom;
      let lx = lastRaw.x, ly = lastRaw.y;
      events = events.filter((ev, i) => {
        const q = toDoc(ev.clientX, ev.clientY);
        if (i < events.length - 1 && Math.hypot(q.x - lx, q.y - ly) < minGap) return false;
        lx = q.x;
        ly = q.y;
        return true;
      });
    }
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
      penPts.push({ x: raw.x, y: raw.y, p: raw.p });
      // the pen moved: start waiting again for it to stand still
      if (holdShapes && !directStroke && Math.hypot(raw.x - holdAnchor.x, raw.y - holdAnchor.y) > 4 / zoom) {
        holdAnchor = { x: raw.x, y: raw.y };
        clearTimeout(holdTimer);
        holdTimer = setTimeout(tryShape, 600);
      }
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
      curveTo(next);
      last = next;
    }
    if (strokeVector) vecPreviewDirty = true;
    requestFrame();
  }

  // Smooth strokes at any speed: the line runs through the middles between pen samples as
  // gentle curves (the samples are the bends), so fast strokes don't become straight pieces.
  let curveCtrl: Point | null = null; // the last pen sample: the next bend
  let drawnTo: Point = { x: 0, y: 0, p: 0.5 }; // where the drawn line ends now
  function curveTo(pt: Point) {
    if (!curveCtrl) {
      const mid = { x: (drawnTo.x + pt.x) / 2, y: (drawnTo.y + pt.y) / 2, p: (drawnTo.p + pt.p) / 2 };
      segment(drawnTo, mid);
      drawnTo = mid;
      curveCtrl = pt;
      return;
    }
    const c = curveCtrl;
    const end = { x: (c.x + pt.x) / 2, y: (c.y + pt.y) / 2, p: (c.p + pt.p) / 2 };
    quad(drawnTo, c, end);
    drawnTo = end;
    curveCtrl = pt;
  }
  function quad(a: Point, c: Point, b: Point) {
    const len = Math.hypot(c.x - a.x, c.y - a.y) + Math.hypot(b.x - c.x, b.y - c.y);
    // short pieces need no curve; long ones are split so the bend is smooth
    const n = len < 6 ? 1 : Math.min(64, Math.ceil(len / 4));
    let prev = a;
    for (let i = 1; i <= n; i++) {
      const t = i / n, u = 1 - t;
      const q = { x: u * u * a.x + 2 * u * t * c.x + t * t * b.x, y: u * u * a.y + 2 * u * t * c.y + t * t * b.y, p: a.p + (b.p - a.p) * t };
      segment(prev, q);
      prev = q;
    }
  }
  // the pen lifted: finish the curve up to the last sample
  function flushCurve() {
    if (curveCtrl) {
      segment(drawnTo, curveCtrl);
      drawnTo = curveCtrl;
      curveCtrl = null;
    }
  }

  function up() {
    clearTimeout(holdTimer);
    if (!drawing) return;
    if (!shapeLocked) flushCurve();
    // Let the stabilized line catch up with where the pen actually stopped
    if (brush.stabilize > 0 && !shapeLocked) segment(drawnTo, lastRaw);
    finishStroke(lastRaw);
  }

  // Adds the finished stroke to the layer and records it for undo
  function smoothing(layer: Layer, on: boolean) {
    for (const c of [strokeCtx, tintCtx, layer.canvas.getContext('2d')!]) c.imageSmoothingEnabled = on;
  }

  function finishStroke(endPt: Point) {
    if (strokeVector) {
      finishVectorStroke(endPt);
      return;
    }
    drawing = false;
    brushPerf.final = false;
    if (!directStroke) redrawStrokeFull();
    recordDabs = false;
    strokeDabs = [];
    if (!directStroke && !shapeClosed) shapeEnds();
    const layer = layers.find((l) => l.id === strokeLayerId);
    if (layer) smoothing(layer, true); // copying the stroke is 1:1, so smoothing is back on already
    if (!layer || !hasRect) {
      render();
      return;
    }

    // the whole-stroke effects (deform, ragged outline…) can reach a little past the dabs
    const pad = strokeEdgePad(brush);
    const x = Math.max(0, Math.floor(minX - pad));
    const y = Math.max(0, Math.floor(minY - pad));
    const w = Math.min(docW, Math.ceil(maxX + pad)) - x;
    const h = Math.min(docH, Math.ceil(maxY + pad)) - y;
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
      // the whole stroke gets its outline, rim and grain (only the new paint changes)
      if (hasStrokeEdges(brush)) applyStrokeEdgesDirect(ctx, before, x, y, brush);
    } else {
      before = ctx.getImageData(x, y, w, h);
      // the whole stroke gets its deform, outline (ragged, torn…), rim and grain as it "dries"
      if (hasStrokeEdges(brush)) applyStrokeEdges(strokeCtx, x, y, w, h, brush);
      ctx.globalAlpha = brush.opacity;
      ctx.globalCompositeOperation = strokeTool === 'eraser' ? 'destination-out' : 'source-over';
      ctx.drawImage(strokeSource(layer, { x, y, w, h }), x, y, w, h, x, y, w, h);
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = 'source-over';
    }
    const after = ctx.getImageData(x, y, w, h);
    directStroke = false;
    liveEdges = false;

    push({ type: 'pixels', layerId: layer.id, rect: { x, y, w, h }, before, after });
    thumbTick++;
    lastEnd = { layerId: layer.id, pt: endPt };
    strokeCtx.clearRect(x, y, w, h);
    render({ x, y, w, h });
  }

  // The pen stood still: if the line looks like a shape, replace it with the clean shape
  function tryShape() {
    if (!drawing || shapeLocked || directStroke) return;
    // shapes follow where the pen really went, not the slower stabilized line
    const found = recognize(penPts.length > 2 ? penPts : vecRaw);
    if (!found) return;
    shapeLocked = true;
    shapeClosed = found.kind !== 'line';
    // remove what was drawn so far
    if (hasRect) {
      const old = { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
      strokeCtx.clearRect(old.x - 2, old.y - 2, old.w + 4, old.h + 4);
      markArea(old);
    }
    vecRaw = found.pts;
    strokeDabs = [];
    resetDabs();
    if (strokeVector && strokeTool === 'brush') {
      vecDrawn = 0;
      vecDist = [];
      vecPreviewDirty = true;
    } else {
      replaying = true;
      stamp(found.pts[0]);
      for (let i = 1; i < found.pts.length; i++) segment(found.pts[i - 1], found.pts[i]);
      replaying = false;
    }
    last = lastRaw = found.pts[found.pts.length - 1];
    showNotice(`${found.kind[0].toUpperCase()}${found.kind.slice(1)} — lift the pen to keep it.`);
    requestFrame();
  }

  // The vector line being drawn, built the same way while drawing and when it's saved,
  // so nothing changes when you lift the pen
  function currentVectorLine(v: VecLayer): VecStroke {
    return {
      pts: shapeLocked ? exactLine(vecRaw) : cleanLine(vecRaw, vecSmooth),
      color,
      // sizes are stored without the layer's width multiplier
      size: brush.size / v.width,
      minSize: brush.minSize,
      sizePressure: brush.sizePressure,
      ends: shapeClosed ? 'round' : brush.ends,
      taper: brush.taper,
      opacity: brush.opacity,
      ...(gradBrush ? { grad: { stops: $state.snapshot(grad.stops), smooth: grad.smooth, length: gradBrushLen / zoom, repeat: gradBrushRepeat } } : {}),
    };
  }

  // Live picture of the vector line (and its mirror copies) in the stroke canvas
  let vecDrawn = 0; // how many points of the line are already on screen
  let vecDist: number[] = []; // distance of each point from the start of the line

  // Draws only the new part of the vector line (and its mirror copies). Returns that area.
  function vectorPreview(): Rect | null {
    if (strokeTool !== 'brush') return null;
    const v = vecData.get(strokeLayerId);
    if (!v) return null;
    for (let i = vecDist.length; i < vecRaw.length; i++) {
      vecDist.push(i === 0 ? 0 : vecDist[i - 1] + Math.hypot(vecRaw[i].x - vecRaw[i - 1].x, vecRaw[i].y - vecRaw[i - 1].y));
    }
    // the last point is held back one frame so the curve through it can bend smoothly
    const to = shapeLocked ? vecRaw.length - 1 : Math.max(vecDrawn, vecRaw.length - 2);
    if (to === vecDrawn && vecDrawn > 0) return null;
    const style = { ...currentVectorLine(v), pts: undefined };
    let area: Rect | null = null;
    for (const f of [(q: { x: number; y: number }) => q, ...strokeMaps]) {
      const b = drawLivePiece(strokeCtx, vecRaw, vecDist, vecDrawn, to, style, v.width, f);
      if (b) {
        grow(b.x, b.y, b.x + b.w, b.y + b.h);
        area = unionRect(area, b);
      }
    }
    vecDrawn = to;
    return area;
  }

  // Vector layer: the drawn line becomes a clean vector line (or the eraser cuts lines)
  function finishVectorStroke(endPt: Point) {
    drawing = false;
    strokeVector = false;
    // the preview disappears: that area is redrawn too
    const strokeArea = hasRect ? { x: minX, y: minY, w: maxX - minX, h: maxY - minY } : null;
    if (strokeArea) {
      strokeCtx.clearRect(strokeArea.x - 2, strokeArea.y - 2, strokeArea.w + 4, strokeArea.h + 4);
      markArea(strokeArea);
    }
    const layer = layers.find((l) => l.id === strokeLayerId);
    const v = layer && vecData.get(layer.id);
    if (!layer || !v) {
      render();
      return;
    }
    lastEnd = { layerId: layer.id, pt: endPt };
    const maps = [(q: { x: number; y: number }) => q, ...strokeMaps];
    if (strokeTool === 'eraser') {
      // eraser size is the same on screen as the brush cursor
      const paths = maps.map((f) => vecRaw.map((q) => ({ ...f(q), r: radiusAt(q.p) })));
      let strokes = v.strokes;
      let fills = v.fills;
      let changed = false;
      for (const path of paths) {
        const next = eraseStrokes(strokes, path, vecEraseMode, v.width);
        if (next) {
          strokes = next;
          changed = true;
        }
        // "Whole lines" also removes color shapes the eraser touches
        if (vecEraseMode === 'whole' && fills?.length) {
          const kept = fills.filter((f) => !path.some((q) => insideFill(f, q.x, q.y)));
          if (kept.length !== fills.length) {
            fills = kept;
            changed = true;
          }
        }
      }
      if (changed) setVector(layer, { ...v, strokes, fills });
      else requestFrame();
      return;
    }
    const line = currentVectorLine(v);
    const added = maps.map((f, i) => (i === 0 ? line : mapStroke(line, f)));
    // new lines go on top, so only they need drawing (redrawing every line would lag)
    const after = { ...v, strokes: [...v.strokes, ...added] };
    vecData.set(layer.id, after);
    const ctx = layer.canvas.getContext('2d')!;
    let area: Rect | null = null;
    for (const s of added) {
      drawStroke(ctx, s, v.width);
      area = unionRect(area, strokeBounds(s, v.width));
    }
    thumbTick++;
    vecTick++;
    push({ type: 'vector', layerId: layer.id, before: v, after });
    if (area) render(area);
    else requestFrame();
  }

  function radiusAt(p: number) {
    const factor = brush.sizePressure ? brush.minSize + (1 - brush.minSize) * p : 1;
    return Math.max((brush.size * factor) / 2, 0.5);
  }

  // The stroke as it should be added to the layer: with paper grain worn into it if the
  // brush has texture. The grain pattern is fixed to the canvas, so it never slides.
  // High quality: the part of the stroke in area A gets its finished edges right away, so
  // what you see while drawing is the real stroke (not a lighter preview fixed at pen-up).
  // The neighbours around A are included so the edges match across pieces.
  function updateLiveEdges(A: Rect) {
    if (A.w * A.h > 2_500_000 && A.w >= docW && A.h >= docH) return; // a full redraw keeps what was already done
    const pad = strokeEdgePad(brush) + Math.ceil(Math.max(4, brush.size) * 0.3) + 3;
    const B = clampRect({ x: A.x - pad, y: A.y - pad, w: A.w + 2 * pad, h: A.h + 2 * pad });
    if (!B) return;
    if (!liveTmp || liveTmp.canvas.width < B.w || liveTmp.canvas.height < B.h) {
      const c = document.createElement('canvas');
      c.width = Math.max(B.w, liveTmp?.canvas.width ?? 0);
      c.height = Math.max(B.h, liveTmp?.canvas.height ?? 0);
      liveTmp = c.getContext('2d', { willReadFrequently: true })!;
    }
    const t = liveTmp;
    t.clearRect(0, 0, B.w, B.h);
    t.drawImage(strokeCtx.canvas, B.x, B.y, B.w, B.h, 0, 0, B.w, B.h);
    applyStrokeEdges(t, 0, 0, B.w, B.h, brush, { x: B.x, y: B.y });
    edgedCtx.clearRect(A.x, A.y, A.w, A.h);
    edgedCtx.drawImage(t.canvas, A.x - B.x, A.y - B.y, A.w, A.h, A.x, A.y, A.w, A.h);
  }

  function strokeSource(layer: Layer, A: Rect = { x: 0, y: 0, w: docW, h: docH }, live = false): HTMLCanvasElement {
    const src = live ? edgedCtx.canvas : strokeCtx.canvas;
    // hair / fur textures are inside the tip (they follow the stroke), not on the canvas
    const useTexture = brush.texture > 0 && paperPattern && !strokeVector && !dirTexture(brush);
    const useColors = !!colorPattern && !strokeVector && !directStroke;
    const useLock = layer.lockAlpha && strokeTool !== 'eraser' && !strokeVector;
    const useSel = hasSel && !strokeVector;
    if (!useTexture && !useColors && !useLock && !useSel) return src;
    const t = texturedCtx;
    t.globalCompositeOperation = 'source-over';
    t.globalAlpha = 1;
    const blit = (img: CanvasImageSource) => t.drawImage(img, A.x, A.y, A.w, A.h, A.x, A.y, A.w, A.h);
    t.clearRect(A.x, A.y, A.w, A.h);
    blit(src);
    if (useColors) {
      // the stroke keeps its shape, but its color comes from the texture picture
      t.globalCompositeOperation = 'source-atop';
      t.globalAlpha = Math.min(1, brush.texColor);
      colorPattern!.setTransform(new DOMMatrix().translate(grainOffset.x, grainOffset.y).scale(Math.max(0.25, brush.texScale ?? 1)));
      t.fillStyle = colorPattern!;
      t.fillRect(A.x, A.y, A.w, A.h);
      t.globalAlpha = 1;
    }
    if (useTexture) {
      t.globalCompositeOperation = 'destination-out';
      t.globalAlpha = brush.texture;
      paperPattern!.setTransform(new DOMMatrix().translate(grainOffset.x, grainOffset.y).scale(Math.max(0.25, brush.texScale ?? 1)));
      t.fillStyle = paperPattern!;
      t.fillRect(A.x, A.y, A.w, A.h);
      t.globalAlpha = 1;
    }
    if (useLock) {
      // Lock alpha: keep the stroke only where the layer already has paint
      t.globalCompositeOperation = 'destination-in';
      blit(layer.canvas);
    }
    if (useSel) {
      // Selection: keep the stroke only inside the selected area
      t.globalCompositeOperation = 'destination-in';
      blit(selCtx.canvas);
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
    sinceDab = 0;
  }

  function stamp(pt: Point) {
    const prev = dabs[dabs.length - 1];
    if (prev) strokeLen += Math.hypot(pt.x - prev.x, pt.y - prev.y);
    const dab = { ...pt, d: strokeLen };
    dabs.push(dab);
    curDabD = dab.d;
    // While drawing only the start can be tapered; the end is tapered when the stroke ends
    drawDab(dab, taperAt(dab.d, Infinity));
  }

  // Draws one dab plus its mirror copies (when mirror drawing is on)
  function drawDab(pt: Point, factor: number) {
    // vector pen: the line is drawn by vectorPreview() exactly as it will be saved
    if (strokeVector && strokeTool === 'brush') return;
    drawDabOne(pt, factor, 0);
    if (!strokeMaps.length) return;
    const r = radiusAt(pt.p) * factor;
    strokeMaps.forEach((f, k) => {
      const q = f(pt);
      if (!sameSpot(pt, q, r)) drawDabOne({ x: q.x, y: q.y, p: pt.p }, factor, k + 1);
    });
  }

  function drawDabOne(pt: Point, factor: number, copy: number) {
    if (directStroke) {
      const st = mixStates[copy];
      if (st) {
        mixP = st.p;
        lastSample = st.ls;
      }
      drawMixDab(pt, factor, copy);
      if (st) {
        st.p = mixP;
        st.ls = lastSample;
      }
      return;
    }
    const r = Math.max(radiusAt(pt.p) * factor, 0.3);
    const dir = dabDirection(pt, copy);
    let dabColor = color;
    let dabAlpha = 1;
    if (gradBrush && strokeTool !== 'eraser') {
      // painting with the gradient: the color depends on how far along the stroke we are
      const q = Math.round(curDabD / 6) * 6; // a few dozen colors are enough and keep it fast
      const c = strokeColorAt(grad.stops, grad.smooth, q, gradBrushLen / strokeZoom, gradBrushRepeat);
      dabColor = c.hex;
      dabAlpha = c.alpha;
    }
    if (brush.colorJitter > 0 && strokeTool !== 'eraser') dabColor = jitterColor(dabColor, brush.colorJitter);
    const tipColor = strokeTool === 'eraser' ? '#000000' : dabColor;
    const variant = pickVariant(brush);
    const tip = getDab(tipColor, brush, variant);
    // Light pressure still leaves some paint (25%), like a real pencil or pen
    const alpha = dabAlpha * brush.flow * (brush.opacityPressure ? 0.25 + 0.75 * pt.p : 1);
    if (!isShaped(brush)) {
      strokeCtx.globalAlpha = alpha;
      strokeCtx.drawImage(tip, pt.x - r, pt.y - r, r * 2, r * 2);
      strokeCtx.globalAlpha = 1;
      grow(pt.x - r - 2, pt.y - r - 2, pt.x + r + 2, pt.y + r + 2);
      return;
    }
    // shaped tips: turned, squashed, scattered and jittered
    const sh = dabShape(brush, r, dir);
    const x = pt.x + sh.ox, y = pt.y + sh.oy;
    strokeCtx.globalAlpha = alpha * sh.alpha;
    // particles: the dab is made of several small separate dabs inside the brush circle
    const parts = brush.particles > 0 ? particleSpread(brush).map((q) => ({ x: x + q.ox * sh.r, y: y + q.oy * sh.r, r: Math.max(0.3, sh.r * q.r), a: q.a + sh.a, v: pickVariant(brush) })) : [{ x, y, r: sh.r, a: sh.a, v: variant }];
    for (const q of parts) {
      stampDab(strokeCtx, q.v === variant ? tip : getDab(tipColor, brush, q.v), q.x, q.y, q.r, q.a, brush.roundness);
      // lighter tips while drawing: remember the dab, the finished stroke is drawn again in full quality
      if (recordDabs) strokeDabs.push({ c: tipColor, v: q.v, x: q.x, y: q.y, r: q.r, a: q.a, al: alpha * sh.alpha });
    }
    strokeCtx.globalAlpha = 1;
    const R = sh.r * 1.45 + 2;
    grow(x - R, y - R, x + R, y + R);
  }

  // Speed setting "Fast"/"Balanced": the pen shows lighter tips while it moves, and when it lifts
  // the stroke is drawn again with full-quality tips, so the finished drawing is always the same.
  let recordDabs = false;
  let strokeDabs: { c: string; v: number; x: number; y: number; r: number; a: number; al: number }[] = [];

  function redrawStrokeFull() {
    if (!recordDabs || !strokeDabs.length || !hasRect) return;
    const x = Math.max(0, Math.floor(minX)), y = Math.max(0, Math.floor(minY));
    strokeCtx.clearRect(x, y, Math.ceil(maxX) - x + 1, Math.ceil(maxY) - y + 1);
    brushPerf.final = true;
    try {
      for (const d of strokeDabs) {
        strokeCtx.globalAlpha = d.al;
        stampDab(strokeCtx, getDab(d.c, brush, d.v), d.x, d.y, d.r, d.a, brush.roundness);
      }
    } finally {
      strokeCtx.globalAlpha = 1;
      brushPerf.final = false;
    }
  }

  // Direction the stroke is going at this dab (each mirrored copy keeps its own)
  let dabLast: ({ x: number; y: number; a: number } | null)[] = [];
  function dabDirection(pt: { x: number; y: number }, copy: number) {
    const prev = dabLast[copy];
    let a = prev ? prev.a : 0;
    if (prev && Math.hypot(pt.x - prev.x, pt.y - prev.y) > 0.5) a = Math.atan2(pt.y - prev.y, pt.x - prev.x);
    if (!prev || Math.hypot(pt.x - prev.x, pt.y - prev.y) > 0.5) dabLast[copy] = { x: pt.x, y: pt.y, a };
    return a;
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

  function drawMixDab(pt: Point, factor: number, copy = 0) {
    const layer = layers.find((l) => l.id === strokeLayerId);
    if (!layer) return;
    const r = Math.max(radiusAt(pt.p) * factor, 0.5);

    // Pick up color every so often (reading pixels is slow, so not on every dab)
    if (Math.hypot(pt.x - lastSample.x, pt.y - lastSample.y) >= Math.max(1, r * 0.4)) {
      // fresh paint from the brush slowly comes back, unless mix is 100%
      // (painting with the gradient: the paint color follows the gradient along the stroke)
      const [br, bg, bb] = hexRGB(paintColorAt(pt.x, pt.y, gradBrush && strokeTool !== 'eraser' ? strokeColorAt(grad.stops, grad.smooth, Math.round(curDabD / 6) * 6, gradBrushLen / strokeZoom, gradBrushRepeat).hex : color));
      const fresh = [br, bg, bb, 1];
      // Color mix % = how much of the paint comes from the canvas, the rest is your color.
      // A pure blender (≈100%) smears what it just picked up; paint mixing reads the layer
      // from before the stroke so it doesn't keep picking up its own fresh paint.
      // with the gradient the brush keeps reloading its new color strongly, so the gradient
      // shows clearly even with mixing paints (oil, acrylic, gouache)
      const g = (gradBrush || (texPixels !== null && brush.texColor > 0)) && strokeTool !== 'eraser' && brush.mix < 0.9;
      const reload = g ? Math.max(0.5, 1 - brush.mix) * 0.6 : (1 - brush.mix) * 0.3;
      for (let i = 0; i < 4; i++) mixP[i] += (fresh[i] - mixP[i]) * reload;
      sampleCanvas(layer, pt, brush.mix * (g ? 0.12 : 0.3), brush.mix < 0.9);
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
    const tip = getDab('#ffffff', brush, pickVariant(brush));
    // the tip turned with the stroke (bristles, flat brushes), squashed by roundness
    const ang =
      (brush.angle * Math.PI) / 180 +
      (followsStroke(brush) ? dabDirection(pt, copy) : 0) +
      (brush.angleJitter > 0 ? (Math.random() - 0.5) * 2 * Math.PI * brush.angleJitter : 0);
    stampDab(t, tip, r, r, r, ang, brush.roundness); // left: mask
    t.globalAlpha = a;
    stampDab(t, tip, size + r, r, r, ang, brush.roundness); // right: paint with its transparency
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
      curDabD = 0;
      dabLast = [];
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
          curDabD = d;
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
    for (const f of [(q: { x: number; y: number }) => q, ...strokeMaps]) cutAt(f(end), f(inner), r);
  }

  function cutAt(end: { x: number; y: number }, inner: { x: number; y: number }, r: number) {
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
  // Dabs are placed every `spacing` along the line, carried over from one piece to the next,
  // so short pieces (smooth curves, slow pens) never squeeze extra dabs in
  let sinceDab = 0;
  function segment(a: Point, b: Point) {
    if (drawing && !replaying) vecRaw.push({ x: b.x, y: b.y, p: b.p });
    const dist = Math.hypot(b.x - a.x, b.y - a.y);
    if (dist <= 1e-6) return;
    const r = radiusAt(Math.min(a.p, b.p)) * taperAt(strokeLen, Infinity);
    const step = Math.max(0.3, r * 2 * brush.spacing);
    let d = Math.max(0, step - sinceDab);
    while (d <= dist) {
      const t = d / dist;
      stamp({
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        p: a.p + (b.p - a.p) * t,
      });
      d += step;
    }
    sinceDab = dist - (d - step);
  }

  function grow(x0: number, y0: number, x1: number, y1: number) {
    if (drawing) markArea({ x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
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
    if (isVector(layer)) {
      showNotice('This is a vector layer: use Brush and Eraser on it. For pixel tools, right-click the layer → Rasterize.');
      return null;
    }
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
    if (activeTool === 'lassoFill' && !isVector(activeLayer) && !editableLayer()) return;
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
    const layer = fillTarget()?.target;
    if (!layer) return;
    // with mirror drawing on, the shape is filled on every mirrored side too
    const shapes = [pts, ...(sym.mode === 'off' ? [] : symCopies(sym)).map((f) => pts.map(f))];
    const xs = shapes.flat().map((p) => p.x);
    const ys = shapes.flat().map((p) => p.y);
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
    for (const sh of shapes) {
      shapePath(t, sh);
      t.fill();
    }
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

  // Vector layers keep only lines, so color goes into a pixel layer right under them
  // ("<name> colors"), the same way lineart and flat colors are usually split.
  const colorsLayerOf = new Map<number, number>(); // vector layer id → its colors layer id

  function fillTarget(): { target: Layer; ref: Layer } | null {
    const l = activeLayer;
    if (l && isVector(l)) {
      if (!l.visible) {
        showNotice('This layer is hidden. Show it (👁) first.');
        return null;
      }
      const known = layers.find((x) => x.id === colorsLayerOf.get(l.id));
      const idx = activeIndex();
      const below = layers[idx - 1];
      let target = known ?? (below && !isVector(below) && below.name === `${l.name} colors` ? below : undefined);
      if (!target) {
        target = makeLayer();
        target.name = `${l.name} colors`;
        insertAt(target, idx);
        push({ type: 'add', layer: target, index: idx });
        activeId = l.id; // keep drawing on the vector layer
        showNotice(`Colors go into "${target.name}" under the lines, so the lines stay editable.`);
      }
      colorsLayerOf.set(l.id, target.id);
      if (!target.visible) target.visible = true;
      return { target, ref: l };
    }
    const e = editableLayer();
    return e ? { target: e, ref: e } : null;
  }

  function doFill(pt: { x: number; y: number }) {
    const ft = fillTarget();
    if (!ft) return;
    const layer = ft.target;

    // What the bucket "looks at": this layer only (for a vector layer: its lines),
    // or everything visible (without paper)
    let ref: ImageData;
    if (fillRefer === 'layer') {
      ref = ft.ref.canvas.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, docW, docH);
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
    const sctx = src.getContext('2d')!;
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
        ...(isVector(l) && vecData.get(l.id) ? { vector: vecData.get(l.id) } : {}),
      })),
      dpi: docDpi,
      colorMode,
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

  const doDownload = () => runFileAction('Saving', () => saveProject(projectData(), `${projectName}.seviath`));
  const doExportPNG = () => runFileAction('PNG export', () => exportPNG(exportComposite(), `${projectName}.png`, docDpi));
  const doExportPSD = () => runFileAction('PSD export', () => exportPSD(projectData(), exportComposite(), `${projectName}.psd`));

  // ---------- Mirror drawing and mirror functions ----------

  function moveSymCenter(p: { x: number; y: number }) {
    let x = Math.min(docW, Math.max(0, p.x));
    let y = Math.min(docH, Math.max(0, p.y));
    // snaps to the middle of the canvas
    const snap = 10 / zoom;
    if (Math.abs(x - docW / 2) < snap) x = docW / 2;
    if (Math.abs(y - docH / 2) < snap) y = docH / 2;
    sym.cx = Math.round(x * 2) / 2;
    sym.cy = Math.round(y * 2) / 2;
  }

  function setSymMode(m: SymmetryMode) {
    funcMenu = false;
    sym.mode = m;
    if (m !== 'off') symLastMode = m;
    if (m !== 'off') showNotice(`Mirror drawing: ${MODE_LABEL[m]}. Drag ◎ to move the center.`);
  }

  function centerSym() {
    sym.cx = docW / 2;
    sym.cy = docH / 2;
  }

  function toggleMirror() {
    setSymMode(sym.mode === 'off' ? symLastMode : 'off');
  }

  // Bounds of what is drawn on a layer (vector layers: their lines)
  function layerBounds(l: Layer) {
    if (isVector(l)) {
      const v = vecData.get(l.id);
      if (!v || (!v.strokes.length && !v.fills?.length)) return null;
      let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
      for (const b of [...v.strokes.map((s) => strokeBounds(s, v.width)), ...(v.fills ?? []).map(fillBounds)]) {
        x0 = Math.min(x0, b.x);
        y0 = Math.min(y0, b.y);
        x1 = Math.max(x1, b.x + b.w);
        y1 = Math.max(y1, b.y + b.h);
      }
      return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    }
    return contentBounds(l.canvas);
  }

  // Puts the mirror center in the middle of the layer's drawing or of the selection
  function symToLayer() {
    const l = activeLayer;
    const b = l && layerBounds(l);
    if (!b) {
      showNotice('This layer is empty.');
      return;
    }
    sym.cx = Math.round((b.x + b.w / 2) * 2) / 2;
    sym.cy = Math.round((b.y + b.h / 2) * 2) / 2;
  }

  function symToSelection() {
    const b = hasSel ? contentBounds(selCtx.canvas) : null;
    if (!b) {
      showNotice('Select an area first.');
      return;
    }
    sym.cx = Math.round((b.x + b.w / 2) * 2) / 2;
    sym.cy = Math.round((b.y + b.h / 2) * 2) / 2;
  }

  // Moves the active layer's drawing (or the selected part) to the middle of the canvas,
  // or onto the mirror center. One undo step.
  function centerLayer(how: 'both' | 'h' | 'v' | 'axis') {
    funcMenu = false;
    if (xf) applyTransform();
    const layer = activeLayer;
    if (!layer || activeId === PAPER_ID) return;
    const toX = how === 'axis' && sym.mode !== 'off' ? sym.cx : docW / 2;
    const toY = how === 'axis' && sym.mode !== 'off' ? sym.cy : docH / 2;
    if (isVector(layer)) {
      const v = vecData.get(layer.id);
      const b = layerBounds(layer);
      if (!v || !b) return showNotice('This layer is empty.');
      const dx = how === 'v' ? 0 : Math.round(toX - (b.x + b.w / 2));
      const dy = how === 'h' ? 0 : Math.round(toY - (b.y + b.h / 2));
      const by = (p: { x: number; y: number }) => ({ x: p.x + dx, y: p.y + dy });
      if (dx || dy) setVector(layer, { ...v, strokes: v.strokes.map((s) => mapStroke(s, by)), fills: v.fills?.map((f) => mapFill(f, by)) });
      return;
    }
    if (!startTransform(false) || !xf) return;
    const b = xf.box;
    if (how !== 'v') xf.tx = Math.round(toX - (b.x + b.w / 2));
    if (how !== 'h') xf.ty = Math.round(toY - (b.y + b.h / 2));
    applyTransform();
  }

  // Flips the active layer (or the selected part) using the transform, so it can be undone
  function flipLayer(dir: 'h' | 'v') {
    funcMenu = false;
    if (xf) applyTransform();
    if (!startTransform(false)) return;
    flipTransform(dir);
    applyTransform();
  }

  // Copies one half of the layer onto the other half, mirrored. The mirror line is the
  // mirror-drawing center when that is on, otherwise the middle of the canvas.
  function mirrorHalf(from: 'left' | 'right' | 'top' | 'bottom') {
    funcMenu = false;
    if (xf) applyTransform();
    const layer = editableLayer();
    if (!layer) return;
    const ax = sym.mode !== 'off' ? sym.cx : docW / 2;
    const ay = sym.mode !== 'off' ? sym.cy : docH / 2;
    const src = makeCanvas();
    src.getContext('2d')!.drawImage(layer.canvas, 0, 0);
    editLayerPixels(layer, fullRect(), (ctx) => {
      ctx.save();
      ctx.beginPath();
      if (from === 'left') ctx.rect(ax, 0, docW - ax, docH);
      if (from === 'right') ctx.rect(0, 0, ax, docH);
      if (from === 'top') ctx.rect(0, ay, docW, docH - ay);
      if (from === 'bottom') ctx.rect(0, 0, docW, ay);
      ctx.clip();
      ctx.clearRect(0, 0, docW, docH);
      if (from === 'left' || from === 'right') {
        ctx.translate(2 * ax, 0);
        ctx.scale(-1, 1);
      } else {
        ctx.translate(0, 2 * ay);
        ctx.scale(1, -1);
      }
      ctx.drawImage(src, 0, 0);
      ctx.restore();
    });
  }

  // ---------- Filters ----------
  function openFilters() {
    if (xf) applyTransform();
    const l = activeLayer;
    if (!l || activeId === PAPER_ID) return showNotice('Select a layer to use a filter on.');
    if (isVector(l)) return showNotice('Filters work on pixel layers. Right-click the layer → Rasterize first.');
    if (!l.visible) return showNotice('This layer is hidden. Show it (👁) first.');
    filterOpen = true;
  }

  const selectionCanvas = () => selCtx.canvas;

  function finishFilter(result: { before: ImageData; after: ImageData } | null) {
    const layer = activeLayer;
    filterOpen = false;
    if (result && layer) push({ type: 'pixels', layerId: layer.id, rect: { x: 0, y: 0, w: docW, h: docH }, before: result.before, after: result.after });
    thumbTick++;
    render();
  }

  function openAdjust() {
    if (!activeLayer) return;
    if (isVector(activeLayer)) {
      showNotice('Adjust colors works on pixel layers. Right-click the layer → Rasterize first.');
      return;
    }
    adjusting = true;
  }

  // ---------- Vector layers ----------

  function addVectorLayer() {
    const l = makeVectorLayer();
    const index = activeIndex() + 1;
    insertAt(l, index);
    push({ type: 'add', layer: l, index });
    if (tool !== 'brush' && tool !== 'eraser') tool = 'brush';
  }

  // Turns a vector layer into a normal pixel layer (the lines stay as they look now).
  // Undo brings the vector layer back.
  function rasterizeLayer() {
    const l = activeLayer;
    if (!l || !isVector(l)) return;
    const index = activeIndex();
    const copy = copyLayer(l);
    const r = makeLayer();
    Object.assign(r, { name: l.name, visible: l.visible, opacity: l.opacity, blend: l.blend, clip: l.clip, lockAlpha: l.lockAlpha });
    r.canvas.getContext('2d')!.drawImage(l.canvas, 0, 0);
    removeAt(index);
    push({ type: 'delete', layer: copy, index });
    insertAt(r, index);
    push({ type: 'add', layer: r, index });
    thumbTick++;
    render();
    showNotice('Rasterized: the layer is pixels now. Ctrl+Z turns it back into vector lines.');
  }

  // Line width slider: changes every line on the layer, live
  function vecWidthInput(w: number) {
    const l = activeLayer;
    const v = l && vecData.get(l.id);
    if (!l || !v) return;
    if (!vecWidthBefore) vecWidthBefore = v;
    vecData.set(l.id, { ...v, width: w });
    drawVector(l);
    render();
  }

  function vecWidthDone() {
    const l = activeLayer;
    const v = l && vecData.get(l.id);
    if (l && v && vecWidthBefore && vecWidthBefore.width !== v.width) setVector(l, v, vecWidthBefore);
    vecWidthBefore = null;
  }

  // ---------- Gradient tool ----------
  // Drag from start to end. The gradient goes on a new layer (default, your drawing is not
  // touched and the gradient stays easy to change) or into the active layer, everywhere or
  // only on what is already drawn, with any mix mode.

  function gradDown(e: PointerEvent, pt: { x: number; y: number }) {
    if (activeId !== PAPER_ID) {
      const l = activeLayer;
      if (!l) return;
      if (!l.visible) return showNotice('This layer is hidden. Show it (👁) first.');
    }
    capture(e);
    // untouched starting gradient: use the color you have picked now
    const now = JSON.stringify($state.snapshot(grad.stops));
    if (now === gradAutoStops) {
      grad.stops = [
        { pos: 0, color, alpha: 1 },
        { pos: 1, color, alpha: 0 },
      ];
      gradAutoStops = JSON.stringify($state.snapshot(grad.stops));
    }
    gradDrag = { p0: pt, p1: pt };
    gradCanvas = null;
    // "Only on the drawing" follows the layer the gradient sits on (the bottom of a
    // clipping group). If that layer is empty there's nothing to follow.
    let i = activeId === PAPER_ID ? -1 : activeIndex();
    while (i > 0 && layers[i].clip) i--;
    const base = i >= 0 ? layers[i] : null;
    gradBaseEmpty = !base || !layerBounds(base);
  }

  function gradMove(pt: { x: number; y: number }, shift: boolean) {
    const d = gradDrag;
    if (!d) return;
    let p1 = pt;
    if (shift) {
      // straight angles: every 15°
      const a = Math.round(Math.atan2(pt.y - d.p0.y, pt.x - d.p0.x) / (Math.PI / 12)) * (Math.PI / 12);
      const len = Math.hypot(pt.x - d.p0.x, pt.y - d.p0.y);
      p1 = { x: d.p0.x + Math.cos(a) * len, y: d.p0.y + Math.sin(a) * len };
    }
    gradDrag = { p0: d.p0, p1 };
    // quick small version while dragging
    gradCanvas = gradientCanvas(Math.min(1, 640 / Math.max(docW, docH)));
    render();
  }

  // The gradient as a doc-size picture (inside the selection only, if there is one)
  function gradientCanvas(scale: number) {
    const d = gradDrag!;
    const g = renderGradient(docW, docH, d.p0, d.p1, $state.snapshot(grad) as GradientSettings, scale);
    const c = makeCanvas();
    const ctx = c.getContext('2d')!;
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(g, 0, 0, docW, docH);
    if (hasSel) {
      ctx.globalCompositeOperation = 'destination-in';
      ctx.drawImage(selCtx.canvas, 0, 0);
      ctx.globalCompositeOperation = 'source-over';
    }
    return c;
  }

  // Where the gradient goes: a new layer (with its index) or into an existing layer
  function gradientPlan(): { layer?: Layer; index: number; into?: Layer } {
    const l = activeId === PAPER_ID ? null : activeLayer;
    const asNew = !l || gradApply.target === 'new' || isVector(l);
    if (!asNew && l) return { into: l, index: activeIndex() };
    const index = l ? activeIndex() + 1 : 0;
    const virtual: Layer = {
      id: -50,
      name: 'Gradient',
      canvas: gradCanvas!,
      visible: true,
      opacity: grad.opacity,
      blend: gradApply.blend as BlendMode,
      // "only on the drawing": clipped to the layer under it, so the drawing keeps its shape
      clip: !!l && gradApply.where === 'drawing' && !gradBaseEmpty,
      lockAlpha: false,
      kind: 'raster',
    };
    return { layer: virtual, index };
  }

  // The layer with the gradient mixed into it
  function gradientMixed(l: Layer): HTMLCanvasElement {
    if (!gradMixCtx || gradMixCtx.canvas.width !== docW || gradMixCtx.canvas.height !== docH) gradMixCtx = makeCanvas().getContext('2d')!;
    const m = gradMixCtx;
    m.globalCompositeOperation = 'source-over';
    m.globalAlpha = 1;
    m.clearRect(0, 0, docW, docH);
    m.drawImage(l.canvas, 0, 0);
    m.globalAlpha = grad.opacity;
    m.globalCompositeOperation = opOf(gradApply.blend as BlendMode);
    m.drawImage(gradCanvas!, 0, 0);
    m.globalAlpha = 1;
    if ((gradApply.where === 'drawing' && !gradBaseEmpty) || l.lockAlpha) {
      // keep exactly the shape (transparency) the layer had
      m.globalCompositeOperation = 'destination-in';
      m.drawImage(l.canvas, 0, 0);
    }
    m.globalCompositeOperation = 'source-over';
    return m.canvas;
  }

  function gradUp() {
    const d = gradDrag;
    if (!d) return;
    if (Math.hypot(d.p1.x - d.p0.x, d.p1.y - d.p0.y) < 2) {
      gradDrag = null;
      gradCanvas = null;
      render();
      return showNotice('Drag from where the gradient starts to where it ends.');
    }
    gradCanvas = gradientCanvas(1); // full quality
    const plan = gradientPlan();
    if (plan.layer) {
      const l = makeLayer();
      l.name = 'Gradient';
      l.canvas.getContext('2d')!.drawImage(gradCanvas, 0, 0);
      l.opacity = plan.layer.opacity;
      l.blend = plan.layer.blend;
      l.clip = plan.layer.clip;
      const keep = activeId;
      insertAt(l, plan.index);
      push({ type: 'add', layer: l, index: plan.index });
      // stay on the drawing so the next gradient also goes on it
      if (keep !== PAPER_ID) activeId = keep;
    } else if (plan.into) {
      const mixed = gradientMixed(plan.into);
      editLayerPixels(plan.into, fullRect(), (ctx) => {
        ctx.clearRect(0, 0, docW, docH);
        ctx.drawImage(mixed, 0, 0);
      });
    }
    gradDrag = null;
    gradCanvas = null;
    thumbTick++;
    render();
    if (gradApply.where === 'drawing' && gradBaseEmpty) {
      showNotice('This layer has no drawing yet, so the gradient was put everywhere. Pick the layer with your drawing for "Only on the drawing".');
    }
  }

  // ---------- My brushes ----------

  async function loadMyBrushes() {
    try {
      const list = await listBrushes();
      myBrushes = list.map((b) => ({
        ...settingsOf(b.settings as Partial<BrushPreset>),
        name: b.name,
        category: 'My brushes',
        about: b.about ?? (b.source ? `Imported from ${b.source}` : 'Your brush'),
        id: b.id,
        source: b.source,
      }));
    } catch {
      // no browser storage (private window): the built-in brushes still work
    }
  }

  // Brush names must be different from each other (they are how a brush is picked)
  function uniqueNames(names: string[]) {
    const taken = new Set([...PRESETS.map((p) => p.name), ...myBrushes.map((b) => b.name)]);
    return names.map((n) => {
      let name = n || 'Brush', k = 2;
      while (taken.has(name)) name = `${n} (${k++})`;
      taken.add(name);
      return name;
    });
  }

  async function saveCurrentBrush() {
    const n = prompt('Name for your brush', `My ${preset}`)?.trim();
    if (!n) return;
    const [name] = uniqueNames([n]);
    try {
      await saveBrushes([{ id: newId(), name, category: 'My brushes', about: `Based on ${preset}`, created: Date.now(), settings: $state.snapshot(brush) as Record<string, unknown> }]);
      await loadMyBrushes();
      preset = name;
      showNotice(`Saved "${name}" in My brushes.`);
    } catch (e) {
      alert(`Could not save the brush: ${e instanceof Error ? e.message : e}`);
    }
  }

  // The selected part of the active layer (or the whole layer) becomes the brush tip
  async function tipFromCanvas() {
    const l = activeLayer;
    if (!l || l.id === PAPER_ID) return showNotice('Pick a layer with a drawing on it first.');
    const src = makeCanvas();
    const c = src.getContext('2d')!;
    c.drawImage(l.canvas, 0, 0);
    if (hasSel) {
      c.globalCompositeOperation = 'destination-in';
      c.drawImage(selCtx.canvas, 0, 0);
      c.globalCompositeOperation = 'source-over';
    }
    const box = contentBounds(src);
    if (!box) return showNotice(hasSel ? 'Nothing is drawn inside the selection.' : 'This layer is empty. Draw the tip shape on it (or select part of a drawing).');
    // a square around it, so the tip keeps its proportions
    const side = Math.max(box.w, box.h);
    const k = Math.min(1, 512 / side);
    const crop = document.createElement('canvas');
    crop.width = crop.height = Math.max(1, Math.round(side * k));
    crop.getContext('2d')!.drawImage(src, box.x - (side - box.w) / 2, box.y - (side - box.h) / 2, side, side, 0, 0, crop.width, crop.height);
    const blob = await new Promise<Blob | null>((r) => crop.toBlob(r, 'image/png'));
    if (!blob) return;
    const t = await imageToTip(blob);
    Object.assign(brush, { tip: 'image', tipImage: t.tip, tipId: 'sel' + Date.now().toString(36), hardness: 1, size: Math.max(2, Math.min(500, Math.round(side))) });
    ensureTip(brush, () => thumbTick++);
    if (tool !== 'brush') tool = 'brush';
    showNotice('Your drawing is now the brush tip. Save it with ★ Save as my brush.');
  }

  async function addImportedBrushes(list: ImportedBrush[]) {
    const names = uniqueNames(list.map((b) => b.name));
    const now = Date.now();
    await saveBrushes(
      list.map((b, i) => ({
        id: newId(),
        name: names[i],
        category: 'My brushes',
        about: b.about ?? `Imported from ${b.source}`,
        source: b.source,
        created: now + i,
        settings: JSON.parse(JSON.stringify(b.settings)) as Record<string, unknown>,
      })),
    );
    await loadMyBrushes();
    showNotice(`Added ${list.length} brush${list.length === 1 ? '' : 'es'} to My brushes.`);
  }

  function useMyBrush(b: MyBrush) {
    if (tool !== 'brush') {
      tool = 'brush';
      flushSync(); // switch the brush/eraser settings first, so they don't overwrite this brush
    }
    preset = b.name;
    const { id, source, ...rest } = b;
    void id;
    void source;
    Object.assign(brush, { tipImage: undefined, tipId: undefined, texImage: undefined }, settingsOf(rest), { size: brush.size });
    ensureTip(brush, () => render());
    libOpen = false;
  }

  async function renameMyBrush(id: string, name: string) {
    const list = await listBrushes();
    const b = list.find((x) => x.id === id);
    if (!b) return;
    const [n] = uniqueNames([name]);
    if (preset === b.name) preset = n;
    b.name = n;
    await saveBrushes([b]);
    await loadMyBrushes();
  }

  async function deleteMyBrush(id: string) {
    await deleteBrush(id);
    await loadMyBrushes();
  }

  async function exportMyBrushes(ids: string[]) {
    const list = (await listBrushes()).filter((b) => ids.includes(b.id));
    if (!list.length) return;
    download(
      brushFile(list.map((b) => ({ name: b.name, about: b.about, settings: b.settings as Partial<BrushSettings> }))),
      list.length === 1 ? `${list[0].name}.seviathbrush` : 'my-brushes.seviathbrush',
    );
  }

  // ---------- Auto trace ----------

  function openTrace() {
    funcMenu = false;
    if (xf) applyTransform();
    const l = activeId === PAPER_ID ? null : activeLayer;
    if (!l) return showNotice('Select the layer to trace.');
    if (isVector(l)) return showNotice('This is already a vector layer. Pick a pixel layer to trace.');
    const b = contentBounds(l.canvas);
    if (!b) return showNotice('This layer is empty.');
    traceFor = { layerId: l.id, bounds: b };
  }

  function finishTrace(r: { layer: VecLayer; hideOriginal: boolean; name: string } | null) {
    const t = traceFor;
    traceFor = null;
    if (!r || !t) return;
    const orig = layers.find((x) => x.id === t.layerId);
    const index = orig ? layers.indexOf(orig) + 1 : layers.length;
    const l = makeLayer();
    l.kind = 'vector';
    l.name = r.name;
    vecData.set(l.id, r.layer);
    drawVector(l);
    insertAt(l, index);
    push({ type: 'add', layer: l, index });
    if (r.hideOriginal && orig) changeLayer(orig, () => (orig.visible = false));
    render();
    showNotice(`Traced: ${r.layer.strokes.length} lines, ${r.layer.fills?.length ?? 0} color shapes. Edit them with Vector select (A).`);
  }

  // ---------- Vector select tool ----------
  // Click a line to select it (Shift adds), drag a box around lines, drag selected lines to
  // move them. The panel then edits them: color, width, pressure, smoothing, nodes.

  function vecLayerOrNotice(): { l: Layer; v: VecLayer } | null {
    const l = activeLayer;
    const v = l && isVector(l) ? vecData.get(l.id) : undefined;
    if (!l || !v) {
      showNotice('Vector select works on vector layers. Pick one in the layer list, or add one with ✒.');
      return null;
    }
    if (!l.visible) {
      showNotice('This layer is hidden. Show it (👁) first.');
      return null;
    }
    return { l, v };
  }

  function vselDown(e: PointerEvent, pt: { x: number; y: number }) {
    const lv = vecLayerOrNotice();
    if (!lv) return;
    capture(e);
    const now = performance.now();
    const dbl = now - vselLast.t < 350 && Math.hypot(pt.x - vselLast.x, pt.y - vselLast.y) < 8 / zoom;
    vselLast = { t: now, x: pt.x, y: pt.y };
    // nodes of the selected lines come first: drag one to reshape the line,
    // Alt+click (or double-click) removes it
    if (vsel.length && vsel.length <= 20 && !e.shiftKey) {
      const node = e.ctrlKey || e.metaKey ? null : nearestNode(vsel, pt.x, pt.y, 4.5 / zoom, NODE_GAP / zoom);
      if (node) {
        if (e.altKey) {
          if (node.s.pts.length <= 6) return showNotice('A line needs at least 2 nodes. Use Delete to remove the whole line.');
          const n = deleteNode(node.s, node.k);
          setVector(lv.l, { ...lv.v, strokes: lv.v.strokes.map((s) => (s === node.s ? n : s)) });
          vsel = vsel.map((s) => (s === node.s ? n : s));
          vselLast.t = 0;
          return;
        }
        vselDrag = { mode: 'node', node, start: pt, cur: pt, before: lv.v, shown: lv.v, picked: vsel, pickedF: vselF, moved: false, add: false, dbl };
        return;
      }
    }
    const hit = hitStroke(lv.v.strokes, pt.x, pt.y, 6 / zoom, lv.v.width);
    // no line there: maybe a color shape (the top-most one under the pen)
    const fillHit = hit ? null : ([...(lv.v.fills ?? [])].reverse().find((f) => insideFill(f, pt.x, pt.y)) ?? null);
    if (fillHit) {
      if (e.shiftKey) vselF = vselF.includes(fillHit) ? vselF.filter((f) => f !== fillHit) : [...vselF, fillHit];
      else if (!vselF.includes(fillHit)) {
        vselF = [fillHit];
        vsel = [];
      }
      vselDrag = { mode: 'move', start: pt, cur: pt, before: lv.v, shown: lv.v, picked: vsel, pickedF: vselF, moved: false, add: e.shiftKey };
      return;
    }
    if (hit && !e.shiftKey && !vsel.includes(hit)) vselF = [];
    if (hit) {
      if (e.shiftKey) vsel = vsel.includes(hit) ? vsel.filter((s) => s !== hit) : [...vsel, hit];
      else if (!vsel.includes(hit)) vsel = [hit];
      vselDrag = { mode: 'move', start: pt, cur: pt, before: lv.v, shown: lv.v, picked: vsel, pickedF: vselF, moved: false, add: e.shiftKey, dbl: dbl && !e.shiftKey };
    } else {
      if (!e.shiftKey) {
        vsel = [];
        vselF = [];
      }
      vselDrag = { mode: 'box', start: pt, cur: pt, before: lv.v, shown: lv.v, picked: vsel, pickedF: vselF, moved: false, add: e.shiftKey };
    }
  }

  function vselMove(pt: { x: number; y: number }) {
    const d = vselDrag;
    const l = activeLayer;
    if (!d || !l) return;
    d.cur = pt;
    if (d.mode === 'box') {
      vselBox = { x0: d.start.x, y0: d.start.y, x1: pt.x, y1: pt.y };
      return;
    }
    const dx = Math.round((pt.x - d.start.x) * 2) / 2, dy = Math.round((pt.y - d.start.y) * 2) / 2;
    if (!d.moved && Math.hypot(dx, dy) < 2 / zoom) return;
    d.moved = true;
    if (d.mode === 'node' && d.node) {
      const src = d.node.s;
      // on dense freehand lines the hidden nodes up to the next handles follow too
      const dense = src.pts.length / 3 > keyNodes(src, NODE_GAP / zoom).length;
      const moved = moveNode(src, d.node.k, pt.x - d.start.x, pt.y - d.start.y, Math.max(nodeSoft, dense ? (NODE_GAP * 1.6) / zoom : 0));
      const next = { ...d.before, strokes: d.before.strokes.map((s) => (s === src ? moved : s)) };
      const area = showVector(l, d.shown, next);
      d.shown = next;
      vsel = d.picked.map((s) => (s === src ? moved : s));
      render(area);
      return;
    }
    const shift = (q: { x: number; y: number }) => ({ x: q.x + dx, y: q.y + dy });
    const map = new Map(d.picked.map((s) => [s, mapStroke(s, shift)]));
    const fmap = new Map(d.pickedF.map((f) => [f, mapFill(f, shift)]));
    const next = {
      ...d.before,
      strokes: d.before.strokes.map((s) => map.get(s) ?? s),
      fills: d.before.fills?.map((f) => fmap.get(f) ?? f),
    };
    const area = showVector(l, d.shown, next);
    d.shown = next;
    vsel = d.picked.map((s) => map.get(s)!);
    vselF = d.pickedF.map((f) => fmap.get(f)!);
    render(area);
  }

  function vselUp() {
    const d = vselDrag;
    vselDrag = null;
    vselBox = null;
    const l = activeLayer;
    if (!d || !l) return;
    if (d.mode === 'move' || d.mode === 'node') {
      if (d.moved) {
        push({ type: 'vector', layerId: l.id, before: d.before, after: d.shown });
        render({ x: 0, y: 0, w: 0, h: 0 });
      } else if (d.dbl) {
        vselDouble(l, d.before, d.node ?? null, d.start);
      }
      return;
    }
    const box = {
      x: Math.min(d.start.x, d.cur.x),
      y: Math.min(d.start.y, d.cur.y),
      w: Math.abs(d.cur.x - d.start.x),
      h: Math.abs(d.cur.y - d.start.y),
    };
    if (box.w < 2 / zoom && box.h < 2 / zoom) return;
    const inside = strokesInBox(d.before.strokes, box);
    vsel = d.add ? [...new Set([...vsel, ...inside])] : inside;
  }

  // Double-click: on a node removes it, on a selected line adds a node there
  function vselDouble(l: Layer, v: VecLayer, node: { s: VecStroke; k: number } | null, pt: { x: number; y: number }) {
    vselLast.t = 0;
    let from: VecStroke, to: VecStroke;
    if (node) {
      if (node.s.pts.length <= 6) return showNotice('A line needs at least 2 nodes. Use Delete to remove the whole line.');
      from = node.s;
      to = deleteNode(node.s, node.k);
    } else {
      const hit = hitStroke(vsel, pt.x, pt.y, 6 / zoom, v.width);
      if (!hit) return;
      from = hit;
      to = insertNode(hit, pt.x, pt.y).s;
    }
    setVector(l, { ...v, strokes: v.strokes.map((s) => (s === from ? to : s)) });
    vsel = vsel.map((s) => (s === from ? to : s));
  }

  // Changes the selected lines (or every line when none is selected), one undo step
  function editLines(fn: (s: VecStroke) => VecStroke) {
    const lv = vecLayerOrNotice();
    if (!lv) return;
    const targets = new Set(vsel.length ? vsel : lv.v.strokes);
    if (!targets.size) return showNotice('This layer has no lines yet.');
    const map = new Map<VecStroke, VecStroke>();
    const strokes = lv.v.strokes.map((s) => {
      if (!targets.has(s)) return s;
      const n = fn(s);
      map.set(s, n);
      return n;
    });
    setVector(lv.l, { ...lv.v, strokes });
    if (vsel.length) vsel = vsel.map((s) => map.get(s) ?? s);
  }

  function deleteLines() {
    const lv = vecLayerOrNotice();
    if (!lv || (!vsel.length && !vselF.length)) return;
    const gone = new Set(vsel);
    const goneF = new Set(vselF);
    setVector(lv.l, { ...lv.v, strokes: lv.v.strokes.filter((s) => !gone.has(s)), fills: lv.v.fills?.filter((f) => !goneF.has(f)) });
    vsel = [];
    vselF = [];
  }

  // Recolors the selected lines and color shapes (everything when nothing is selected)
  function recolorSelected() {
    const lv = vecLayerOrNotice();
    if (!lv) return;
    const all = !vsel.length && !vselF.length;
    const ls = new Set(vsel), fs = new Set(vselF);
    const lmap = new Map<VecStroke, VecStroke>(), fmap = new Map<VecFill, VecFill>();
    const strokes = lv.v.strokes.map((s) => {
      if (!all && !ls.has(s)) return s;
      const n = { ...s, color, grad: undefined };
      lmap.set(s, n);
      return n;
    });
    const fills = lv.v.fills?.map((f) => {
      if (!all && !fs.has(f)) return f;
      const n = { ...f, color };
      fmap.set(f, n);
      return n;
    });
    setVector(lv.l, { ...lv.v, strokes, fills });
    vsel = vsel.map((s) => lmap.get(s) ?? s);
    vselF = vselF.map((f) => fmap.get(f) ?? f);
  }

  function duplicateLines() {
    const lv = vecLayerOrNotice();
    if (!lv || !vsel.length) return;
    const copies = vsel.map((s) => mapStroke(s, (q) => ({ x: q.x + 20, y: q.y + 20 })));
    setVector(lv.l, { ...lv.v, strokes: [...lv.v.strokes, ...copies] });
    vsel = copies;
  }

  function selectAllLines() {
    const lv = vecLayerOrNotice();
    if (lv) vsel = lv.v.strokes;
  }

  // Bounds of the selected lines
  function vselBounds() {
    let box: Rect | null = null;
    const w = activeLayer ? (vecData.get(activeLayer.id)?.width ?? 1) : 1;
    for (const s of vsel) box = unionRect(box, strokeBounds(s, w));
    return box;
  }

  function flipLines(dir: 'h' | 'v') {
    const b = vselBounds();
    if (!b) return;
    const cx = b.x + b.w / 2, cy = b.y + b.h / 2;
    editLines((s) => mapStroke(s, (q) => (dir === 'h' ? { x: 2 * cx - q.x, y: q.y } : { x: q.x, y: 2 * cy - q.y })));
  }

  function nudgeLines(dx: number, dy: number) {
    if (!vsel.length) return;
    editLines((s) => mapStroke(s, (q) => ({ x: q.x + dx, y: q.y + dy })));
  }

  // Pressure graph of the first selected line
  function pressureChange(pressures: number[], done: boolean) {
    const lv = vecLayerOrNotice();
    if (!lv || !vsel.length || !prDense) return;
    const target = vsel[0];
    if (!prBefore) prBefore = lv.v;
    const edited = setPressures(prDense, pressures);
    const next = { ...lv.v, strokes: lv.v.strokes.map((s) => (s === target ? edited : s)) };
    const area = showVector(lv.l, lv.v, next);
    vsel = [edited, ...vsel.slice(1)];
    prDense = edited;
    render(area);
    if (done) {
      push({ type: 'vector', layerId: lv.l.id, before: prBefore, after: next });
      prBefore = null;
    }
  }

  // A finely pointed copy of the first selected line for the pressure graph
  let prSource = $derived.by(() => {
    const s = vsel[0];
    if (!s) return null;
    if (prDense && (prDense === s || prDense.pts === s.pts)) return prDense;
    prDense = nodeCount(s) < 40 ? densifyStroke(s, 4) : s;
    return prDense;
  });

  // ---------- Document settings ----------

  function setDoc(next: DocSettings) {
    fileMenu = false;
    const before = { dpi: docDpi, colorMode };
    if (before.dpi === next.dpi && before.colorMode === next.colorMode) return;
    docDpi = next.dpi;
    colorMode = next.colorMode;
    push({ type: 'doc', before, after: next });
    render();
  }

  function askDpi() {
    const cm = (px: number) => ((px / docDpi) * 2.54).toFixed(1);
    const v = prompt(`Print resolution in DPI (pixels per inch). 300 = print, 72 = screen.\nThe picture keeps its pixels; only the print size changes (now ${cm(docW)} × ${cm(docH)} cm).`, String(docDpi));
    const dpi = Math.round(Number(v));
    if (!v || !isFinite(dpi) || dpi < 1 || dpi > 2400) return;
    setDoc({ dpi, colorMode });
  }

  // ---------- Export ----------

  function openExport() {
    fileMenu = false;
    if (xf) applyTransform();
    exportDialog = true;
  }

  function finishExport(r: ExportOptions | null) {
    exportDialog = false;
    if (!r) return;
    exportOpts = r;
    runFileAction('Export', () =>
      exportAs(
        r,
        projectData(),
        (withPaper) => {
          const c = makeCanvas();
          compositeTo(c.getContext('2d')!, withPaper, false);
          return c;
        },
        projectName,
        { profile: prefs.rgbProfile, embed: prefs.embedProfile, bake: prefs.bakeProfile },
      ),
    );
  }

  // ---------- Autosave to the project library ----------

  // Something changed: remember which layer's pixels changed and save a little later
  function touch(e?: HistoryEntry) {
    if (e?.type === 'pixels') dirtyLayers.add(e.layerId);
    if (e?.type === 'merge') dirtyLayers.add(e.lowerId);
    if (e?.type === 'vector') dirtyLayers.add(e.layerId);
    if (!currentId) return;
    saveState = 'unsaved';
    recoveryDirty = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => saveNow(), prefs.autosaveDelay * 1000);
  }

  // ---------- Canvas recovery ----------
  // Every few minutes (Preferences → File & recovery) a separate copy of the project is kept.
  let recoveryDirty = false;
  let lastRecovery = Date.now();
  async function recoveryTick() {
    if (!prefs.recoveryEvery || !currentId || !recoveryDirty || drawing || xf) return;
    if (Date.now() - lastRecovery < prefs.recoveryEvery * 60_000) return;
    lastRecovery = Date.now();
    recoveryDirty = false;
    try {
      await saveRecovery(currentId, prefs.recoveryKeep, projectName, projectData(), exportComposite());
    } catch {
      recoveryDirty = true; // try again next time
    }
  }
  $effect(() => {
    const t = setInterval(recoveryTick, 30_000);
    return () => clearInterval(t);
  });

  async function saveNow(): Promise<void> {
    clearTimeout(saveTimer);
    if (saving) {
      saveAgain = true;
      return saving;
    }
    if (!currentId || saveState === 'saved') return;
    // don't save half a stroke or an unfinished transform; try again soon
    if (drawing || xf) {
      saveTimer = setTimeout(() => saveNow(), 1500);
      return;
    }
    const id = currentId;
    const ids = layers.map((l) => l.id);
    const changed = new Set(dirtyLayers);
    dirtyLayers.clear();
    saveState = 'saving';
    saving = (async () => {
      try {
        await saveToLibrary(id, projectName, projectData(), exportComposite(), async (l, i) => {
          let blob = blobCache.get(ids[i]);
          if (!blob || changed.has(ids[i])) {
            blob = await canvasToBlob(l.canvas);
            blobCache.set(ids[i], blob);
          }
          return blob;
        });
        if (id === currentId && saveState === 'saving') saveState = 'saved';
      } catch (err) {
        for (const c of changed) dirtyLayers.add(c);
        if (id === currentId) saveState = 'unsaved';
        showNotice(`Autosave failed: ${err instanceof Error ? err.message : err}`);
      } finally {
        saving = null;
      }
    })();
    await saving;
    if (saveAgain) {
      saveAgain = false;
      await saveNow();
    }
  }

  // Ctrl+S: save right now
  async function doSave() {
    fileMenu = false;
    if (xf) applyTransform();
    if (!currentId) {
      currentId = newId();
      saveState = 'unsaved';
    }
    if (saveState === 'saved') {
      showNotice('Saved.');
      return;
    }
    await saveNow();
    if ((saveState as string) === 'saved') showNotice('Saved.');
  }

  function renameCurrent() {
    fileMenu = false;
    const name = prompt('Project name', projectName)?.trim();
    if (!name || name === projectName) return;
    projectName = name;
    touch();
  }

  // A fresh document becomes a new project in the library
  async function startLibraryProject() {
    blobCache.clear();
    dirtyLayers.clear();
    currentId = newId();
    saveState = 'unsaved';
    await saveNow();
  }

  async function openManager() {
    fileMenu = false;
    if (xf) applyTransform();
    await saveNow();
    showProjects = true;
  }

  async function openFromLibrary(id: string) {
    if (id === currentId) {
      showProjects = false;
      return;
    }
    await saveNow();
    try {
      const { meta, data } = await loadFromLibrary(id);
      await loadProject(data, meta.name);
      blobCache.clear();
      dirtyLayers.clear();
      currentId = id;
      saveState = 'saved';
      showProjects = false;
    } catch (err) {
      alert(`Opening failed: ${err instanceof Error ? err.message : err}`);
    }
  }

  function openFilePicker(mode: 'open' | 'layer' = 'open') {
    fileMenu = false;
    if (!fileInput) return;
    fileMode = mode;
    fileInput.value = '';
    fileInput.click();
  }

  function onFileChosen() {
    const files = Array.from(fileInput?.files ?? []);
    if (!files.length) return;
    if (fileMode === 'layer') importAsLayers(files);
    else openFiles(files);
  }

  // Every file opens as its own new project (the last one stays open)
  async function openFiles(files: File[]) {
    if (xf) applyTransform();
    await saveNow();
    let opened = false;
    for (const file of files) {
      try {
        const o = await openAnyFile(file);
        await loadProject(o.data, o.name);
        await startLibraryProject();
        if (o.note) showNotice(o.note);
        opened = true;
      } catch (err) {
        alert(`Could not open "${file.name}": ${err instanceof Error ? err.message : err}`);
      }
    }
    if (opened) showProjects = false;
  }

  // Adds pictures (or every layer of a PSD / ORA / project) on top of the active layer.
  // Bigger pictures are shrunk to fit the canvas; everything is centered.
  async function importAsLayers(files: File[]) {
    if (xf) applyTransform();
    let added = 0;
    for (const file of files) {
      try {
        const o = await openAnyFile(file);
        const d = o.data;
        const s = Math.min(1, docW / d.width, docH / d.height);
        const ox = (docW - d.width * s) / 2;
        const oy = (docH - d.height * s) / 2;
        let index = activeIndex() + 1;
        for (const src of d.layers) {
          const l = makeLayer();
          l.name = d.layers.length === 1 ? o.name : src.name;
          l.visible = src.visible;
          l.opacity = src.opacity;
          l.blend = (BLEND_NAMES as string[]).includes(src.blend) ? (src.blend as BlendMode) : 'Normal';
          l.clip = src.clip && index > 0;
          const ctx = l.canvas.getContext('2d')!;
          ctx.imageSmoothingQuality = 'high';
          ctx.drawImage(src.canvas, ox, oy, d.width * s, d.height * s);
          insertAt(l, index);
          push({ type: 'add', layer: l, index });
          dirtyLayers.add(l.id);
          index++;
          added++;
        }
        if (o.note) showNotice(o.note);
      } catch (err) {
        alert(`Could not import "${file.name}": ${err instanceof Error ? err.message : err}`);
      }
    }
    if (!added) return;
    thumbTick++;
    render();
    // One picture: show the transform box right away so it can be placed
    if (added === 1) {
      await tick();
      if (startTransform(false)) showNotice('Move or resize the picture, then press Enter.');
    }
  }

  function isFileDrag(e: DragEvent) {
    return Array.from(e.dataTransfer?.types ?? []).includes('Files');
  }

  // Dropping files on the drawing adds them as layers (in the project manager: opens them)
  function onWindowDragOver(e: DragEvent) {
    if (isFileDrag(e)) e.preventDefault();
  }
  function onWindowDrop(e: DragEvent) {
    if (!isFileDrag(e)) return;
    e.preventDefault();
    if (showProjects) return; // the project manager handles its own drops
    const files = Array.from(e.dataTransfer?.files ?? []);
    if (files.length) importAsLayers(files);
  }

  // Ctrl+V with a picture on the clipboard (e.g. a screenshot) pastes it as a new layer
  function onPaste(e: ClipboardEvent) {
    if (showProjects || newDialog || adjusting) return;
    const t = e.target as HTMLElement;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return;
    const files = Array.from(e.clipboardData?.files ?? []).filter((f) => f.type.startsWith('image/'));
    if (!files.length) return;
    e.preventDefault();
    importAsLayers(files.map((f, i) => new File([f], f.name && f.name !== 'image.png' ? f.name : `Pasted image${i ? ' ' + (i + 1) : ''}.png`, { type: f.type })));
  }

  function onBeforeUnload(e: BeforeUnloadEvent) {
    if (recorder.busy) e.preventDefault(); // an unsaved session recording would be lost
    if (currentId && saveState !== 'saved') {
      saveNow();
      e.preventDefault();
    }
  }

  function onVisibility() {
    if (document.visibilityState === 'hidden') saveNow();
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
    sym.cx = w / 2;
    sym.cy = h / 2;
    makeWorkCanvases();
    undoStack.length = 0;
    redoStack.length = 0;
    vecData.clear();
    sync();
    lastEnd = null;
  }

  async function loadProject(data: ProjectData, name: string) {
    resetDocument(data.width, data.height);
    docDpi = data.dpi ?? 72;
    colorMode = data.colorMode ?? 'rgb';
    const list: Layer[] = data.layers.map((l) => {
      const id = nextId++;
      if (l.vector) vecData.set(id, { strokes: l.vector.strokes ?? [], width: l.vector.width ?? 1 });
      return {
        id,
        name: l.name,
        canvas: l.canvas,
        visible: l.visible,
        opacity: l.opacity,
        blend: (BLEND_NAMES as string[]).includes(l.blend) ? (l.blend as BlendMode) : 'Normal',
        clip: l.clip,
        lockAlpha: l.lockAlpha,
        kind: l.vector ? 'vector' : 'raster',
      };
    });
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

  async function newProject(r: NewProjectResult) {
    newDialog = false;
    if (!r) return;
    await saveNow();
    resetDocument(r.width, r.height);
    docDpi = r.dpi;
    colorMode = r.colorMode;
    const first = r.layerKind === 'vector' ? makeVectorLayer() : makeLayer();
    layers = [first];
    activeId = first.id;
    Object.assign(paper, { exists: true, visible: r.paper === 'white', color: '#ffffff' });
    projectName = 'Untitled';
    showProjects = false;
    await tick();
    fitToScreen();
    thumbTick++;
    render();
    await startLibraryProject();
  }

  function askNewProject() {
    fileMenu = false;
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
    const index = activeIndex();
    if (index < 0) return;
    const l = layers[index];
    const copy = copyLayer(l);
    removeAt(index);
    push({ type: 'delete', layer: copy, index });
    // the last layer can be deleted too; drawing again makes a new one (see onPointerDown)
    thumbTick++;
    render();
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
    const l = { ...makeLayer(), name: src.name + ' copy', visible: src.visible, opacity: src.opacity, blend: src.blend, clip: src.clip, lockAlpha: src.lockAlpha, kind: src.kind };
    l.canvas.getContext('2d')!.drawImage(src.canvas, 0, 0);
    const v = vecData.get(src.id);
    if (isVector(src) && v) vecData.set(l.id, v);
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
    if (isVector(lower)) {
      showNotice("Can't merge into a vector layer. Rasterize the layer below first (right-click → Rasterize).");
      return;
    }
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
    if (isVector(layer)) {
      const v = vecData.get(layer.id);
      if (v && v.strokes.length) setVector(layer, { ...v, strokes: [] });
      return;
    }
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

  // Svelte action: moves a popup menu up/left so all of it is inside the window
  function fitOnScreen(node: HTMLElement) {
    const fit = () => {
      const r = node.getBoundingClientRect();
      if (r.bottom > window.innerHeight - 8) node.style.top = `${Math.max(8, window.innerHeight - 8 - r.height)}px`;
      if (r.right > window.innerWidth - 8) node.style.left = `${Math.max(8, window.innerWidth - 8 - r.width)}px`;
    };
    requestAnimationFrame(fit);
    fit();
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
    touch(entry);
    undoStack.push(entry);
    while (undoStack.length > prefs.undoSteps) undoStack.shift();
    redoStack.length = 0;
    sync();
  }

  function sync() {
    undoCount = undoStack.length;
    redoCount = redoStack.length;
    recorder.changed(); // timelapse: the picture changed, that's a new video frame
  }

  // Undoes (reverse) or redoes an entry. Returns the area that changed (undefined = all).
  function apply(e: HistoryEntry, reverse: boolean): Rect | undefined {
    touch(e);
    let changed: Rect | undefined;
    switch (e.type) {
      case 'pixels': {
        const l = layers.find((l) => l.id === e.layerId);
        l?.canvas.getContext('2d')!.putImageData(reverse ? e.before : e.after, e.rect.x, e.rect.y);
        changed = e.rect;
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
      case 'vector': {
        const l = layers.find((x) => x.id === e.layerId);
        const from = reverse ? e.after : e.before;
        const to = reverse ? e.before : e.after;
        if (l) changed = showVector(l, from, to);
        else vecData.set(e.layerId, to);
        const still = new Set(to.strokes);
        vsel = vsel.filter((s) => still.has(s));
        const stillF = new Set(to.fills ?? []);
        vselF = vselF.filter((f) => stillF.has(f));
        break;
      }
      case 'doc': {
        const d = reverse ? e.before : e.after;
        docDpi = d.dpi;
        colorMode = d.colorMode;
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
    return changed;
  }

  function undo() {
    const e = undoStack.pop();
    if (!e) return;
    const area = apply(e, true);
    redoStack.push(e);
    sync();
    render(area);
  }

  function redo() {
    const e = redoStack.pop();
    if (!e) return;
    const area = apply(e, false);
    undoStack.push(e);
    sync();
    render(area);
  }

  // ---------- Thumbnails ----------

  // Svelte action: draws a small preview of a layer, again whenever `tick` changes
  function thumb(node: HTMLCanvasElement, args: { layer: Layer; tick: number }) {
    let wait = 0;
    const later = (a: { layer: Layer; tick: number }) => {
      // Preferences → Canvas: layer pictures can wait until you pause drawing
      clearTimeout(wait);
      if (prefs.lazyThumbs) wait = window.setTimeout(() => draw(a), 900);
      else draw(a);
    };
    const draw = ({ layer }: { layer: Layer; tick: number }) => {
      const ctx = node.getContext('2d')!;
      ctx.clearRect(0, 0, node.width, node.height);
      const scale = Math.min(node.width / docW, node.height / docH);
      const w = docW * scale;
      const h = docH * scale;
      ctx.drawImage(layer.canvas, (node.width - w) / 2, (node.height - h) / 2, w, h);
    };
    draw(args);
    return { update: later, destroy: () => clearTimeout(wait) };
  }

  // ---------- Shortcuts ----------

  function onKey(e: KeyboardEvent) {
    // Don't trigger shortcuts while typing in text or number fields
    const target = e.target as HTMLInputElement;
    if (target.tagName === 'INPUT' && (target.type === 'text' || target.type === 'number')) return;
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (adjusting || newDialog || showProjects || exportDialog || traceFor || libOpen || settingsOpen || filterOpen) return; // the dialog handles its own keys

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
      if (e.shiftKey) openManager();
      else openFilePicker('open');
      return;
    }
    if (mod && key === 'e') {
      e.preventDefault();
      if (e.shiftKey) doExportPNG();
      else openExport();
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
    // Your keyboard shortcuts (Preferences → Shortcuts)
    const act = keyAction.get(comboOf(e));
    if (act && ACTION_RUN[act]) {
      e.preventDefault();
      ACTION_RUN[act]();
      return;
    }

    // Selection
    if (activeTool === 'vselect' && (vsel.length || vselF.length)) {
      if (key === 'delete' || key === 'backspace') {
        e.preventDefault();
        deleteLines();
        return;
      }
      if (key === 'escape') {
        vsel = [];
        vselF = [];
        return;
      }
      const step = e.shiftKey ? 10 : 1;
      const arrows: Record<string, [number, number]> = { arrowleft: [-step, 0], arrowright: [step, 0], arrowup: [0, -step], arrowdown: [0, step] };
      if (arrows[key]) {
        e.preventDefault();
        nudgeLines(...arrows[key]);
        return;
      }
    }
    if (mod && e.shiftKey && key === 'j') {
      e.preventDefault();
      selectionToNewLayer(true);
      return;
    }
    if ((key === 'delete' || key === 'backspace') && hasSel) {
      e.preventDefault();
      clearSelected();
      return;
    }

    if (key === 'escape' && (layerMenu || fileMenu || toolsMenu || funcMenu)) {
      layerMenu = null;
      fileMenu = false;
      toolsMenu = false;
      funcMenu = false;
    } else if (key === 'escape' && hasSel) {
      deselect();
    } else if (key === 'alt') {
      e.preventDefault();
      // in Vector select, Alt+click removes nodes instead of picking a color
      if (tool !== 'vselect') altDown = true;
    } else if (key === ' ') {
      e.preventDefault();
      spaceDown = true;
    }
  }

  // What each shortcut does (the keys themselves are in Preferences → Shortcuts)
  const ACTION_RUN: Record<string, () => void> = {
    brush: () => (tool = 'brush'),
    eraser: () => (tool = 'eraser'),
    picker: () => (tool = 'picker'),
    fill: () => (tool = 'fill'),
    gradient: () => (tool = 'gradient'),
    lassoFill: () => (tool = 'lassoFill'),
    select: () => {
      tool = 'select';
      if (selMode === 'lasso') selMode = 'rect';
    },
    lassoSel: () => {
      tool = 'select';
      selMode = 'lasso';
    },
    move: () => (tool = 'move'),
    vselect: () => (tool = 'vselect'),
    sizeDown: () => (brush.size = stepSize(brush.size, -1)),
    sizeUp: () => (brush.size = stepSize(brush.size, 1)),
    randomBrush: () => {
      if (tool !== 'brush') tool = 'brush';
      flushSync();
      Object.assign(brush, { tipImage: undefined, tipId: undefined }, keepSizeFeel(surpriseBrush('paint'), brush));
      preset = 'Random brush';
    },
    mirror: () => setSymMode(nextMode(sym.mode)),
    undo: () => undo(),
    redo: () => redo(),
    redo2: () => redo(),
    transform: () => startTransform(false),
    adjust: () => openAdjust(),
    selectAll: () => (activeTool === 'vselect' ? selectAllLines() : selectAll()),
    deselect: () => deselect(),
    invertSel: () => invertSelection(),
    selToLayer: () => selectionToNewLayer(false),
    newLayer: () => addLayer(),
    deleteLayer: () => deleteLayer(),
    zoomIn: () => zoomCenter(1.25),
    zoomOut: () => zoomCenter(1 / 1.25),
    fit: () => fitToScreen(),
    zoom100: () => zoomCenter(1 / zoom),
    rotateLeft: () => rotateBy(-15),
    rotateRight: () => rotateBy(15),
    rotateReset: () => resetRotation(),
    flip: () => toggleFlip(),
    settings: () => (settingsOpen = true),
    filters: () => openFilters(),
    subView: () => (subOpen = !subOpen),
    record: () => (recOpen = !recOpen),
  };
  let keyAction = $derived(new Map(Object.entries(prefs.keys).filter(([, k]) => k).map(([a, k]) => [k, a])));
  const keyFor = (id: string) => prefs.keys[id] || '';

  // [ and ] change the size by ~10%, but always by at least 0.1 px
  function stepSize(size: number, dir: 1 | -1) {
    let next = dir > 0 ? Math.max(size + 0.1, size * 1.1) : Math.min(size - 0.1, size / 1.1);
    next = Math.min(500, Math.max(0.5, next));
    if (next < 10) return Math.round(next * 10) / 10;
    if (next < 50) return Math.round(next * 2) / 2;
    return Math.round(next);
  }

  // ---------- Toolbar menus ----------

  type ItemId = 'brush' | 'eraser' | 'fill' | 'lassoFill' | 'lassoSel' | 'picker' | 'select' | 'move' | 'vselect' | 'gradient';
  type Item = { id: ItemId; icon: string; label: string; key: string; title: string };
  const TOOL_ITEMS: Item[] = [
    { id: 'brush', icon: '🖌', label: 'Brush', key: 'B', title: 'Paint with the chosen brush' },
    { id: 'eraser', icon: '⌫', label: 'Eraser', key: 'E', title: 'Hard, soft and textured erasers' },
    { id: 'fill', icon: '🪣', label: 'Fill', key: 'G', title: 'Paint bucket' },
    { id: 'lassoFill', icon: '⬠', label: 'Lasso fill', key: 'Shift L', title: 'Draw a shape to fill it. Hold Ctrl to erase the shape' },
    { id: 'lassoSel', icon: '✎', label: 'Lasso', key: 'L', title: 'Lasso selection: draw around an area' },
    { id: 'picker', icon: '💧', label: 'Picker', key: 'I / Alt', title: 'Pick a color from the canvas' },
    { id: 'gradient', icon: '🌈', label: 'Gradient', key: 'Shift G', title: 'Drag to put a gradient on the drawing or a new layer' },
    { id: 'vselect', icon: '◇', label: 'Vector select', key: 'A', title: 'Select, move and edit lines on a vector layer' },
  ];
  const FUNC_ITEMS: Item[] = [
    { id: 'select', icon: '▭', label: 'Select', key: 'M', title: 'Rectangle or ellipse selection' },
    { id: 'move', icon: '✥', label: 'Move', key: 'V', title: 'Move the layer or the selected part' },
  ];

  let currentItem = $derived<ItemId>(
    activeTool === 'select' ? (selMode === 'lasso' ? 'lassoSel' : 'select') : (activeTool as ItemId),
  );
  let toolItem = $derived(TOOL_ITEMS.find((i) => i.id === currentItem) ?? null);
  let funcItem = $derived(FUNC_ITEMS.find((i) => i.id === currentItem) ?? null);

  function pickItem(id: ItemId) {
    toolsMenu = false;
    funcMenu = false;
    if (id === 'lassoSel') {
      tool = 'select';
      selMode = 'lasso';
    } else if (id === 'select') {
      tool = 'select';
      if (selMode === 'lasso') selMode = 'rect';
    } else {
      tool = id;
    }
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
    <button class="settings-btn" onclick={() => (settingsOpen = true)} title="Settings: performance (GPU/CPU, brush quality), undo steps">⚙</button>
    {#if cloudReady}
      <button class="account-btn" class:on={accountOpen} onclick={() => (accountOpen = !accountOpen)} title={account ? `Signed in as ${account.name}` : 'Sign in or create a Seviath account'}>
        {#if account?.avatar}<img src={account.avatar} alt="" referrerpolicy="no-referrer" />{:else}👤{/if}
        {account ? account.name : 'Sign in'}
      </button>
    {/if}
    <div class="file-wrap">
      <button class:on={fileMenu} onclick={(e) => (fileMenu = dropAt(e, !fileMenu))}>File ▾</button>
      {#if fileMenu}
        <div class="menu-backdrop" role="presentation" onpointerdown={() => (fileMenu = false)}></div>
        <div class="context-menu drop file-menu" role="menu" style="left: {dropPos.x}px; top: {dropPos.y}px;">
          <div class="menu-title">{projectName} · {docW} × {docH} px · {docDpi} dpi · {colorMode.toUpperCase()}</div>
          <button role="menuitem" onclick={openManager}>🗂 Projects… <kbd>Ctrl Shift O</kbd></button>
          <button role="menuitem" onclick={() => { fileMenu = false; settingsOpen = true; }}>⚙ Settings…</button>
          <button role="menuitem" onclick={askNewProject}>New project…</button>
          <button role="menuitem" onclick={() => openFilePicker('open')} title="PSD, OpenRaster, Krita, PNG, JPG, WebP, GIF, SVG, .seviath">Open file as new project… <kbd>Ctrl O</kbd></button>
          <button role="menuitem" onclick={() => openFilePicker('layer')} title="Adds pictures as new layers. You can also drop files on the canvas or paste with Ctrl V">Import as layer…</button>
          <hr />
          <button role="menuitem" onclick={doSave}>Save now <kbd>Ctrl S</kbd></button>
          <button role="menuitem" onclick={renameCurrent}>Rename project…</button>
          <button role="menuitem" onclick={doDownload}>Download project (.seviath)</button>
          <button role="menuitem" onclick={askDpi} title="Print size of the picture; pixels stay the same">Resolution: {docDpi} DPI…</button>
          <button role="menuitem" onclick={() => setDoc({ dpi: docDpi, colorMode: colorMode === 'rgb' ? 'cmyk' : 'rgb' })} title="RGB for screens, CMYK for print">
            {colorMode === 'rgb' ? 'Switch to CMYK (print)' : 'Switch to RGB (screen)'}
          </button>
          <hr />
          <button role="menuitem" onclick={openExport} title="PSD, ORA, .seviath, PNG, JPG, WebP, GIF, BMP, TIFF, ICO, SVG">Export… (all file types) <kbd>Ctrl E</kbd></button>
          <button role="menuitem" onclick={doExportPSD}>Quick export PSD</button>
          <button role="menuitem" onclick={doExportPNG}>Quick export PNG <kbd>Ctrl Shift E</kbd></button>
        </div>
      {/if}
    </div>
    <button class="project-name" onclick={renameCurrent} title="Rename project">{projectName}</button>
    <span class="save-state {saveState}" title="Your work is saved automatically in this browser">
      {saveState === 'saved' ? '✓ Saved' : saveState === 'saving' ? 'Saving…' : '● Not saved yet'}
    </span>
    <span class="doc-badge" title="Color mode and print resolution (File menu to change)">{colorMode.toUpperCase()} · {docDpi} dpi</span>
    <button class:on={subOpen} onclick={() => (subOpen = !subOpen)} title="Sub view: your reference pictures (with color picker) and the navigator, in a window you can move">🖼 Sub view</button>
    <button class="rec-btn" class:on={recOpen} class:live={recorder.state === 'recording'} onclick={() => (recOpen = !recOpen)} title="Record your drawing session as an MP4 video (timelapse or real time)">
      {recorder.state === 'recording' ? '● REC' : recorder.state === 'paused' ? '❚❚ REC' : '🎥 Record'}
    </button>
    {#if colorMode === 'cmyk'}
      <button
        class:on={printPreview}
        onclick={() => {
          printPreview = !printPreview;
          render();
        }}
        title="Show colors as they will look printed (approximate)">🖨 Print preview</button
      >
    {/if}
    <div class="file-wrap">
      <button class:on={toolsMenu} class:active-group={toolItem !== null} onclick={(e) => (toolsMenu = dropAt(e, !toolsMenu))} title="Drawing tools">
        {toolItem ? (isVector(activeLayer) && (toolItem.id === 'brush' || toolItem.id === 'eraser') ? (toolItem.id === 'brush' ? '✒ Vector pen' : '⌫ Vector eraser') : `${toolItem.icon} ${toolItem.label}`) : 'Tools'} ▾
      </button>
      {#if toolsMenu}
        <div class="menu-backdrop" role="presentation" onpointerdown={() => (toolsMenu = false)}></div>
        <div class="context-menu drop file-menu" role="menu" style="left: {dropPos.x}px; top: {dropPos.y}px;">
          {#each TOOL_ITEMS as it (it.id)}
            <button role="menuitem" class:checked={toolItem?.id === it.id} onclick={() => pickItem(it.id)} title={it.title}>
              <span>{it.icon} {it.label}</span><kbd>{it.id === 'picker' ? [keyFor('picker'), 'Alt'].filter(Boolean).join(' / ') : keyFor(it.id)}</kbd>
            </button>
          {/each}
        </div>
      {/if}
    </div>
    <button class:on={filterOpen} onclick={openFilters} title="Filters: city blocks, cubes, triangles, spheres, blur, sharpen, halftone, glow, oil paint, make it an illustration…">✨ Filter</button>
    <div class="file-wrap">
      <button class:on={funcMenu} class:active-group={funcItem !== null || !!xf} onclick={(e) => (funcMenu = dropAt(e, !funcMenu))} title="Select, move, transform, mirror">
        {xf ? '⤡ Transform' : funcItem ? `${funcItem.icon} ${funcItem.label}` : 'Functions'} ▾
      </button>
      {#if funcMenu}
        <div class="menu-backdrop" role="presentation" onpointerdown={() => (funcMenu = false)}></div>
        <div class="context-menu drop file-menu" role="menu" style="left: {dropPos.x}px; top: {dropPos.y}px;">
          {#each FUNC_ITEMS as it (it.id)}
            <button role="menuitem" class:checked={funcItem?.id === it.id} onclick={() => pickItem(it.id)} title={it.title}>
              <span>{it.icon} {it.label}</span><kbd>{it.id === 'picker' ? [keyFor('picker'), 'Alt'].filter(Boolean).join(' / ') : keyFor(it.id)}</kbd>
            </button>
          {/each}
          <button
            role="menuitem"
            class:checked={!!xf}
            onclick={() => {
              funcMenu = false;
              if (xf) applyTransform();
              else startTransform(false);
            }}
            title="Scale, rotate and flip the layer or the selection"><span>⤡ Transform</span><kbd>{keyFor('transform')}</kbd></button
          >
          <hr />
          <div class="menu-title">🪞 Mirror drawing</div>
          {#each MODES as m (m)}
            <button role="menuitem" class:checked={sym.mode === m} onclick={() => setSymMode(m)} title={MODE_HINT[m]}>
              <span>{MODE_LABEL[m]}</span>{#if m === 'off'}<kbd>{keyFor('mirror')} cycles</kbd>{/if}
            </button>
          {/each}
          <hr />
          <button role="menuitem" onclick={openTrace} title="Turns the active pixel layer into editable vector lines and color shapes"><span>✒ Auto trace → vector…</span></button>
          <hr />
          <div class="menu-title">Center the layer</div>
          <button role="menuitem" onclick={() => centerLayer('both')} title="Moves the drawing (or the selected part) to the middle of the canvas"><span>⊕ Center on canvas</span></button>
          <button role="menuitem" onclick={() => centerLayer('h')}><span>↔ Center horizontally</span></button>
          <button role="menuitem" onclick={() => centerLayer('v')}><span>↕ Center vertically</span></button>
          <button role="menuitem" onclick={() => centerLayer('axis')} disabled={sym.mode === 'off'}><span>◎ Center on mirror center</span></button>
          <hr />
          <div class="menu-title">Mirror the layer</div>
          <button role="menuitem" onclick={() => flipLayer('h')} title="Also works on a selection"><span>⇋ Flip horizontally</span></button>
          <button role="menuitem" onclick={() => flipLayer('v')} title="Also works on a selection"><span>⇵ Flip vertically</span></button>
          <button role="menuitem" onclick={() => mirrorHalf('left')} title="Makes the right side a mirror copy of the left side"><span>◧ Copy left half → right</span></button>
          <button role="menuitem" onclick={() => mirrorHalf('right')} title="Makes the left side a mirror copy of the right side"><span>◨ Copy right half → left</span></button>
          <button role="menuitem" onclick={() => mirrorHalf('top')}><span>⬒ Copy top half → bottom</span></button>
          <button role="menuitem" onclick={() => mirrorHalf('bottom')}><span>⬓ Copy bottom half → top</span></button>
        </div>
      {/if}
    </div>
    <button
      class="mirror-btn"
      class:on={sym.mode !== 'off'}
      onclick={toggleMirror}
      title="Mirror drawing: draw on one side, it appears on the other (Y cycles the modes)">🪞 {sym.mode === 'off' ? 'Mirror' : MODE_LABEL[sym.mode]}</button
    >
    <div class="current-color" style="background: {color}" title={color}></div>
    <span class="brush-name">{tool === 'eraser' ? 'Eraser' : 'Brush'}: {preset} · {brush.size}px</span>
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
    {#if sym.mode !== 'off'}
      <div class="panel-head">🪞 Mirror drawing</div>
      <div class="tool-options sym-options">
        <div class="seg3">
          {#each MODES.filter((m) => m !== 'off') as m (m)}
            <button class:on={sym.mode === m} onclick={() => (sym.mode = m)} title={MODE_HINT[m]}>{MODE_LABEL[m]}</button>
          {/each}
        </div>
        {#if sym.mode === 'radial' || sym.mode === 'kaleido'}
          <label class="opt">
            <span>Copies</span>
            <input type="range" min="2" max="32" step="1" bind:value={sym.count} />
            <span class="val">{sym.count}</span>
          </label>
        {/if}
        <label class="opt" title="Turn the axes">
          <span>Angle</span>
          <input type="range" min="-180" max="180" step="1" bind:value={sym.angle} ondblclick={() => (sym.angle = 0)} />
          <span class="val">{sym.angle}°</span>
        </label>
        <div class="xy">
          <label>X <input type="number" step="0.5" bind:value={sym.cx} /></label>
          <label>Y <input type="number" step="0.5" bind:value={sym.cy} /></label>
          <button class:on={symLocked} onclick={() => (symLocked = !symLocked)} title="Lock the center so it can't be dragged by accident">{symLocked ? '🔒' : '🔓'}</button>
        </div>
        <div class="panel-sub">Put the center on…</div>
        <div class="btn-grid three">
          <button onclick={centerSym} title="The middle of the canvas">Canvas</button>
          <button onclick={symToLayer} title="The middle of what is drawn on the active layer">Layer</button>
          <button onclick={symToSelection} disabled={!hasSel} title="The middle of the selection">Selection</button>
        </div>
        <div class="panel-sub">Move the drawing…</div>
        <div class="btn-grid">
          <button onclick={() => centerLayer('axis')} title="Moves the active layer's drawing (or the selection) so its middle is on the mirror center">Layer → mirror center</button>
          <button onclick={() => setSymMode('off')}>Turn mirror off</button>
        </div>
        <p class="hint">{MODE_HINT[sym.mode]} Drag the ◎ on the canvas to move the center (it snaps to the middle of the canvas). Whatever you draw on one side appears on the other at the same time: brush, eraser, vector pen and lasso fill. <b>Y</b> switches the mode.</p>
      </div>
    {/if}
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
          <button onclick={selectAll}>Select all <kbd>{keyFor('selectAll')}</kbd></button>
          <button onclick={deselect} disabled={!hasSel}>Deselect <kbd>{keyFor('deselect')}</kbd></button>
          <button onclick={invertSelection}>Invert <kbd>{keyFor('invertSel')}</kbd></button>
          <button onclick={clearSelected} disabled={!hasSel}>Delete inside <kbd>Del</kbd></button>
          <button onclick={deleteOutside} disabled={!hasSel} title="Cut the picture out along the selection">Delete outside (cut out)</button>
          <button onclick={() => selectionToNewLayer(false)} disabled={!hasSel}>Copy to new layer <kbd>{keyFor('selToLayer')}</kbd></button>
          <button onclick={() => selectionToNewLayer(true)} disabled={!hasSel}>Cut to new layer <kbd>Ctrl Shift J</kbd></button>
          <button onclick={fillSelected} disabled={!hasSel}>Fill with color</button>
          <button onclick={() => startTransform(false)} disabled={!hasSel}>Transform <kbd>{keyFor('transform')}</kbd></button>
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
    {:else if activeTool === 'gradient'}
      <div class="panel-head">🌈 Gradient</div>
      <div class="tool-options">
        <GradientPanel bind:grad bind:apply={gradApply} bind:color bind:color2 blends={BLEND_NAMES} vectorActive={isVector(activeLayer)} />
      </div>
    {:else if activeTool === 'vselect'}
      <div class="panel-head">◇ Vector select</div>
      <div class="tool-options">
        {#if !isVector(activeLayer)}
          <p class="hint">Pick a vector layer (VECTOR in the layer list) or make one with ✒. This tool selects and edits its lines.</p>
        {:else}
          <p class="hint small-note">
            {vsel.length || vselF.length
              ? `${vsel.length} line${vsel.length === 1 ? '' : 's'}${vselF.length ? `, ${vselF.length} color shape${vselF.length === 1 ? '' : 's'}` : ''} selected`
              : 'Nothing selected: the buttons change every line on the layer'}
          </p>
          <div class="btn-grid two">
            <button onclick={selectAllLines}>Select all <kbd>Ctrl A</kbd></button>
            <button onclick={() => (vsel = [])} disabled={!vsel.length}>Deselect <kbd>Esc</kbd></button>
            <button onclick={deleteLines} disabled={!vsel.length && !vselF.length}>Delete <kbd>Del</kbd></button>
            <button onclick={duplicateLines} disabled={!vsel.length}>Duplicate</button>
            <button onclick={recolorSelected} title="Paint the selected lines and color shapes with the current color">
              <span class="dot" style="background: {color}"></span> Recolor
            </button>
            <button onclick={() => flipLines('h')} disabled={!vsel.length}>⇋ Flip</button>
          </div>
          <div class="panel-sub">Width</div>
          <div class="btn-grid three">
            <button onclick={() => editLines((s) => ({ ...s, size: s.size * 0.8 }))}>Thinner</button>
            <button onclick={() => editLines((s) => ({ ...s, size: s.size * 1.25 }))}>Thicker</button>
            <button onclick={() => editLines((s) => ({ ...s, sizePressure: !s.sizePressure }))} title="Turns pen pressure on/off for the line width: off = the same width everywhere">Even ⇄ pressure</button>
          </div>
          <div class="panel-sub">Shape</div>
          <div class="btn-grid three">
            <button onclick={() => editLines((s) => smoothStroke(s, 0.5))} title="Smoother curves">Smooth</button>
            <button onclick={() => editLines((s) => simplifyStroke(s, 1.2))} title="Fewer points, same look (reduce nodes)">Fewer nodes</button>
            <button onclick={() => editLines((s) => ({ ...s, ends: s.ends === 'taper' ? 'round' : 'taper' }))} title="Pointed or round ends">Ends ◁ ⇄ ◖</button>
          </div>
          <div class="panel-sub">Nodes</div>
          <label class="opt" title="0 = only the grabbed node moves. Higher = nearby nodes follow smoothly (good for freehand lines with many nodes)">
            <span>Soft pull</span>
            <input type="range" min="0" max="200" step="1" bind:value={nodeSoft} />
            <span class="val">{nodeSoft ? nodeSoft + ' px' : 'off'}</span>
          </label>
          <p class="hint small-note">Drag a ▫ node to reshape a selected line, drag the line between nodes (or hold <b>Ctrl</b>) to move it. Double-click a line to add a node, double-click or <b>Alt</b>+click a node to remove it.</p>
          {#if prSource}
            <div class="panel-sub">Pressure of the first selected line (drag up = thicker)</div>
            <PressureEditor pts={prSource.pts} onchange={pressureChange} />
          {/if}
          <p class="hint">
            Click a line to select it, <b>Shift</b>+click to add more, or drag a box around lines. Drag selected lines to move them, arrow keys nudge (Shift = 10 px).
            {#if vsel.length}{vsel.reduce((a, s) => a + nodeCount(s), 0)} nodes.{/if}
          </p>
        {/if}
      </div>
    {:else if activeTool === 'move'}
      <div class="panel-head">Move</div>
      <div class="tool-options">
        <p class="hint">Drag to move the active layer, or only the selected part if there is a selection. For scaling and rotating use <b>Transform</b> (Ctrl T).</p>
      </div>
    {:else}
      {#if isVector(activeLayer)}
        {@const v = vecTick >= 0 ? vecData.get(activeLayer!.id) : undefined}
        <div class="panel-head">✒ Vector layer</div>
        <div class="tool-options vec-options">
          {#if tool === 'eraser'}
            <div class="seg2">
              <button class:on={vecEraseMode === 'cut'} onclick={() => (vecEraseMode = 'cut')} title="Removes only the part you rub; the rest stays as clean lines">✂ Cut lines</button>
              <button class:on={vecEraseMode === 'whole'} onclick={() => (vecEraseMode = 'whole')} title="Removes every line you touch">Whole lines</button>
            </div>
          {:else}
            <label class="opt" title="0 = keeps your hand exactly, 100 = very smooth curves">
              <span>Smooth</span>
              <input type="range" min="0" max="1" step="0.01" bind:value={vecSmooth} />
              <span class="val">{Math.round(vecSmooth * 100)}%</span>
            </label>
          {/if}
          <label class="opt" title="Makes every line on this layer thinner or thicker, any time">
            <span>Line width</span>
            <input
              type="range"
              min="0.2"
              max="4"
              step="0.05"
              value={v?.width ?? 1}
              oninput={(e) => vecWidthInput(+e.currentTarget.value)}
              onchange={vecWidthDone}
            />
            <span class="val">{Math.round((v?.width ?? 1) * 100)}%</span>
          </label>
          <p class="hint">
            Lines on this layer stay editable: change their width later, cut them cleanly with the eraser, and they never get blurry. {v?.strokes.length ?? 0} lines.
            Paper texture and color mixing are for pixel layers.
          </p>
          <button class="wide" onclick={rasterizeLayer}>Rasterize layer (turn into pixels)</button>
        </div>
      {/if}
      <div class="panel-head">{tool === 'eraser' ? 'Eraser' : 'Brush'}</div>
      {#if tool === 'brush'}
        <label class="hold-check" title="The color changes along each stroke, using the colors of the Gradient tool (Shift G)">
          <input type="checkbox" bind:checked={gradBrush} /> Paint with the gradient
        </label>
        {#if gradBrush}
          <div class="grad-brush">
            <div class="mini-bar" style="background: {cssGradient(grad.stops, grad.smooth)}"></div>
            <label class="opt" title="How long a stroke takes to go through all the colors, measured on screen (feels the same at any zoom)">
              <span>Length</span>
              <input type="range" min="40" max="1500" step="10" bind:value={gradBrushLen} />
              <span class="val">{gradBrushLen}px</span>
            </label>
            <div class="seg3">
              <button class:on={gradBrushRepeat === 'none'} onclick={() => (gradBrushRepeat = 'none')}>Once</button>
              <button class:on={gradBrushRepeat === 'repeat'} onclick={() => (gradBrushRepeat = 'repeat')}>Repeat</button>
              <button class:on={gradBrushRepeat === 'mirror'} onclick={() => (gradBrushRepeat = 'mirror')}>Back & forth</button>
            </div>
            <button class="wide" class:on={gradBrushEdit} onclick={() => (gradBrushEdit = !gradBrushEdit)}>{gradBrushEdit ? '▴ Done editing colors' : '✎ Edit the colors…'}</button>
            {#if gradBrushEdit}
              <GradientPanel bind:grad bind:apply={gradApply} bind:color bind:color2 blends={BLEND_NAMES} colorsOnly />
            {/if}
          </div>
        {/if}
      {/if}
      <label class="hold-check" title="Draw a line, circle, rectangle, triangle or polygon by hand and keep the pen still for a moment at the end: it becomes clean and straight">
        <input type="checkbox" bind:checked={holdShapes} /> Hold still at the end to straighten (line, circle, box…)
      </label>
      <BrushPanel
        bind:brush
        bind:preset
        eraser={tool === 'eraser'}
        mine={myBrushes}
        {color}
        onsave={saveCurrentBrush}
        ontipfromcanvas={tipFromCanvas}
        onshare={(name, about, settings) => {
          shareReq = { name, about, settings };
          libOpen = true;
        }}
        hasSelection={hasSel}
        onlibrary={(t) => {
          libTab = t ?? 'mine';
          libOpen = true;
        }}
      />
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
      style:filter={profileView ? 'url(#rgb-profile)' : null}
      style="width: {docW * zoom}px; height: {docH * zoom}px; transform-origin: {(docW * zoom) / 2}px {(docH * zoom) / 2}px; transform: translate({panX}px, {panY}px) rotate({rotation}deg) scaleX({flipped ? -1 : 1});"
    ></canvas>
    {#if hasSel || selDraft || xf || sym.mode !== 'off' || (activeTool === 'vselect' && (vsel.length || vselF.length || vselBox)) || gradDrag}
      {@const sw = 1.5 / zoom}
      {@const dash = 6 / zoom}
      <svg
        class="overlay"
        viewBox="0 0 {docW} {docH}"
        preserveAspectRatio="none"
        style="width: {docW * zoom}px; height: {docH * zoom}px; transform-origin: {(docW * zoom) / 2}px {(docH * zoom) / 2}px; transform: translate({panX}px, {panY}px) rotate({rotation}deg) scaleX({flipped ? -1 : 1}); --dash: {dash}px;"
      >
        {#if gradDrag}
          <line class="sym-axis-bg" x1={gradDrag.p0.x} y1={gradDrag.p0.y} x2={gradDrag.p1.x} y2={gradDrag.p1.y} stroke-width={sw * 3} />
          <line class="sym-axis" x1={gradDrag.p0.x} y1={gradDrag.p0.y} x2={gradDrag.p1.x} y2={gradDrag.p1.y} stroke-width={sw * 1.5} />
          <circle class="sym-handle" cx={gradDrag.p0.x} cy={gradDrag.p0.y} r={6 / zoom} stroke-width={sw} />
          <circle class="sym-handle" cx={gradDrag.p1.x} cy={gradDrag.p1.y} r={6 / zoom} stroke-width={sw} />
        {/if}
        {#if activeTool === 'vselect'}
          {#each vsel as s, i (i)}
            {@const pts = Array.from({ length: s.pts.length / 3 }, (_, k) => `${s.pts[k * 3]},${s.pts[k * 3 + 1]}`).join(' ')}
            <polyline class="vsel-line" points={pts} stroke-width={sw * 1.5} />
            {#if vsel.length <= 20}
              {#each keyNodes(s, NODE_GAP / zoom).slice(0, 600) as k (k)}
                <rect class="vsel-node" x={s.pts[k * 3] - 3 / zoom} y={s.pts[k * 3 + 1] - 3 / zoom} width={6 / zoom} height={6 / zoom} stroke-width={sw} />
              {/each}
            {/if}
          {/each}
          {#each vselF as f, i (i)}
            {#each f.rings as ring, k (k)}
              <polygon class="vsel-line" points={Array.from({ length: ring.length / 2 }, (_, j) => `${ring[j * 2]},${ring[j * 2 + 1]}`).join(' ')} stroke-width={sw * 1.5} />
            {/each}
          {/each}
          {#if vselBox}
            <rect
              class="ants"
              x={Math.min(vselBox.x0, vselBox.x1)}
              y={Math.min(vselBox.y0, vselBox.y1)}
              width={Math.abs(vselBox.x1 - vselBox.x0)}
              height={Math.abs(vselBox.y1 - vselBox.y0)}
              stroke-width={sw}
              stroke-dasharray="{dash} {dash}"
            />
          {/if}
        {/if}
        {#if sym.mode !== 'off'}
          {#each axisLines(sym, docW + docH) as [a, b], i (i)}
            <line class="sym-axis-bg" x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke-width={sw * 2.5} />
            <line class="sym-axis" x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke-width={sw} stroke-dasharray="{dash * 1.5} {dash}" />
          {/each}
          <circle class="sym-handle" class:dragging={symDrag} cx={sym.cx} cy={sym.cy} r={9 / zoom} stroke-width={sw * 1.5} />
          <circle class="sym-dot" cx={sym.cx} cy={sym.cy} r={2.5 / zoom} />
        {/if}
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
      <div class="xf-bar" role="toolbar" tabindex="-1" onpointerdown={(e) => e.stopPropagation()}>
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
    <ColorWheel bind:color cmyk={colorMode === 'cmyk'} />

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
              {#if layer.kind === 'vector'}<span class="vec-badge" title="Vector layer">VECTOR</span>{/if}
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
      <button onclick={addVectorLayer} title="New vector layer: lines you can re-shape later">✒</button>
      <button onclick={duplicateLayer} title="Duplicate layer">⧉</button>
      <button onclick={openAdjust} title="Adjust colors (Ctrl+U)">🎨</button>
      <button onclick={mergeDown} disabled={activeIndex() <= 0} title="Merge down">⤓</button>
      <button onclick={() => moveLayer(1)} title="Move up">↑</button>
      <button onclick={() => moveLayer(-1)} title="Move down">↓</button>
      <button onclick={deleteLayer} disabled={activeId === PAPER_ID} title="Delete layer">🗑</button>
    </div>
  </aside>
</div>

{#if newDialog}
  <NewProjectDialog ondone={newProject} />
{/if}

{#if libOpen}
  <BrushLibrary
    bind:tab={libTab}
    mine={myBrushes}
    {color}
    onuse={useMyBrush}
    onrename={renameMyBrush}
    ondelete={deleteMyBrush}
    onexport={exportMyBrushes}
    onadd={addImportedBrushes}
    onclose={() => {
      libOpen = false;
      shareReq = null;
    }}
    bind:share={shareReq}
    {account}
  />
{/if}

<!-- the RGB profile preview (Preferences → Color profiles) -->
{#if profileView}
  <svg width="0" height="0" style="position: absolute" aria-hidden="true">
    <filter id="rgb-profile" color-interpolation-filters="linearRGB">
      <feComponentTransfer>
        <feFuncR type="table" tableValues={profileView.tables[0]} />
        <feFuncG type="table" tableValues={profileView.tables[1]} />
        <feFuncB type="table" tableValues={profileView.tables[2]} />
      </feComponentTransfer>
      <feColorMatrix type="matrix" values={profileView.matrix} />
    </filter>
  </svg>
{/if}

{#if subOpen}
  <SubView
    getView={() => view}
    {docW}
    {docH}
    {zoom}
    corners={viewCorners}
    {centerOn}
    setZoom={setZoomCenter}
    fit={fitToScreen}
    onpick={(hex) => (color = hex)}
    onclose={() => (subOpen = false)}
  />
{/if}
{#if recOpen}
  <RecorderPanel getView={() => view} name={projectName} onclose={() => (recOpen = false)} />
{/if}
{#if settingsOpen}
  <SettingsDialog bind:prefs onclose={() => (settingsOpen = false)} />
{/if}

{#if accountOpen}
  <div class="menu-backdrop" role="presentation" onpointerdown={() => (accountOpen = false)}></div>
  <div class="account-pop" role="dialog" aria-label="Seviath account">
    <div class="account-head">
      <strong>Seviath account</strong>
      <button onclick={() => (accountOpen = false)}>✕</button>
    </div>
    <AccountPanel {account} bind:recovery why="One account for sharing brushes (and more later). Free." />
  </div>
{/if}

{#if traceFor}
  {@const src = layers.find((x) => x.id === traceFor!.layerId)}
  {#if src}
    <TraceDialog source={src.canvas} bounds={traceFor.bounds} layerName={src.name} ondone={finishTrace} />
  {/if}
{/if}

{#if exportDialog}
  <ExportDialog width={docW} height={docH} dpi={docDpi} {colorMode} initial={exportOpts} ondone={finishExport} />
{/if}

<input bind:this={fileInput} type="file" multiple accept={ACCEPT} hidden onchange={onFileChosen} />

{#if showProjects}
  <ProjectManager
    {currentId}
    canClose={currentId !== null && !newDialog}
    onopen={openFromLibrary}
    onnew={askNewProject}
    onfiles={openFiles}
    onclose={() => (showProjects = false)}
    onrenamed={(id, name) => id === currentId && (projectName = name)}
    ondeleted={(id) => {
      if (id === currentId) {
        currentId = null;
        saveState = 'saved';
      }
    }}
  />
{/if}

{#if filterOpen && activeLayer}
  <FilterDialog canvas={activeLayer.canvas} mask={hasSel ? selectionCanvas() : null} layerName={activeLayer.name} onchange={render} ondone={finishFilter} />
{/if}

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
  <div class="context-menu" role="menu" use:fitOnScreen style="left: {layerMenu.x}px; top: {layerMenu.y}px;">
    <div class="menu-title">{activeLayer.name}</div>
    <button role="menuitem" onclick={() => menuDo(addLayer)}>New layer above</button>
    <button role="menuitem" onclick={() => menuDo(addVectorLayer)}>New vector layer above</button>
    {#if activeLayer.kind === 'vector'}
      <button role="menuitem" onclick={() => menuDo(rasterizeLayer)}>Rasterize (turn into pixels)</button>
    {:else}
      <button role="menuitem" onclick={() => menuDo(openTrace)}>Auto trace → vector…</button>
    {/if}
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
    <button role="menuitem" onclick={() => menuDo(openAdjust)}>Adjust colors… (Ctrl+U)</button>
    <button role="menuitem" onclick={() => menuDo(() => featherLayerEdges(selFeather || 20))}>
      Feather edges ({selFeather || 20}px)
    </button>
    <button role="menuitem" onclick={() => menuDo(clearLayer)}>Clear layer (make transparent)</button>
    <button role="menuitem" class="danger" onclick={() => menuDo(deleteLayer)}>
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
  .account-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 160px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .account-btn img {
    width: 20px;
    height: 20px;
    border-radius: 50%;
  }
  .account-pop {
    position: fixed;
    z-index: 70;
    top: 52px;
    left: 12px;
    width: min(340px, calc(100vw - 24px));
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    background: #24242b;
    border: 1px solid #444;
    border-radius: 12px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6);
    color: #ddd;
    font-size: 12px;
  }
  .account-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
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
  .toolbar > * {
    flex-shrink: 0;
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
  .doc-badge {
    font-size: 11px;
    color: #9ab;
    border: 1px solid #3a3a44;
    border-radius: 4px;
    padding: 2px 6px;
    white-space: nowrap;
  }
  .vec-badge {
    font-size: 9px;
    font-weight: 700;
    color: #111;
    background: #7fe6f0;
    border-radius: 3px;
    padding: 0 3px;
    margin-right: 4px;
  }
  .seg2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
  }
  .seg2 button {
    padding: 6px 2px;
    font-size: 11px;
  }
  .project-name {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    background: none;
    border-color: transparent;
    font-weight: 600;
  }
  .save-state {
    font-size: 11px;
    color: #7c7;
    white-space: nowrap;
    margin-right: 6px;
  }
  .save-state.saving {
    color: #aaa;
  }
  .save-state.unsaved {
    color: #d9a441;
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
  .context-menu.drop {
    position: fixed;
    max-height: calc(100vh - 60px);
    overflow-y: auto;
  }
  .context-menu > * {
    flex-shrink: 0;
  }
  .context-menu .checked {
    background: #00b5c833;
    color: #7fe6f0;
  }
  .active-group {
    border-color: #00b5c8 !important;
    color: #7fe6f0;
  }
  :global(.sym-axis) {
    stroke: #00d8ee;
    fill: none;
  }
  :global(.sym-axis-bg) {
    stroke: rgba(0, 0, 0, 0.35);
    fill: none;
  }
  :global(.sym-handle) {
    fill: rgba(0, 181, 200, 0.25);
    stroke: #00d8ee;
    pointer-events: none;
  }
  :global(.sym-handle.dragging) {
    fill: rgba(0, 181, 200, 0.6);
  }
  :global(.sym-dot) {
    fill: #00d8ee;
  }
  :global(.vsel-line) {
    fill: none;
    stroke: #ff4fd8;
  }
  :global(.vsel-node) {
    fill: #fff;
    stroke: #ff4fd8;
  }
  .btn-grid.two {
    grid-template-columns: 1fr 1fr;
  }
  .dot {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 1px solid #666;
    vertical-align: -1px;
  }
  .grad-brush {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 10px;
  }
  .mini-bar {
    height: 12px;
    border-radius: 3px;
    border: 1px solid #555;
  }
  .hold-check {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    font-size: 11px;
    color: #aaa;
    margin-bottom: 8px;
    line-height: 1.3;
  }
  .xy {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .xy label {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 4px;
    color: #aaa;
  }
  .xy input {
    width: 100%;
    min-width: 0;
    background: #15151a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    padding: 4px 6px;
    font-size: 12px;
  }
  .xy button {
    padding: 4px 8px;
  }
  .panel-sub {
    color: #888;
    font-size: 11px;
    margin-bottom: -4px;
  }
  .btn-grid.three {
    grid-template-columns: repeat(3, 1fr);
  }
  .sym-options .seg3 {
    grid-template-columns: repeat(3, 1fr);
  }
  .file-menu {
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
    max-height: calc(100vh - 16px);
    overflow-y: auto;
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
