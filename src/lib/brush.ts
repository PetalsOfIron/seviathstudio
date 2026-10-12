// Brush settings, the brush collection, and the brush tip ("dab") used for painting.
// Every brush is made of the same settings, so imported brushes (Photoshop, Krita, CSP)
// and your own saved brushes work exactly like the built-in ones.

export type TipShape = 'round' | 'shape' | 'bristle' | 'chalk' | 'wet' | 'splatter' | 'leaf' | 'grass' | 'star' | 'cloud' | 'strands' | 'speckle' | 'grunge' | 'hatch' | 'scale' | 'image';
export type ShapeBase = 'circle' | 'square' | 'rounded' | 'triangle' | 'polygon' | 'star' | 'diamond' | 'heart' | 'crescent' | 'drop';
export type EdgeStyle = 'smooth' | 'ragged' | 'torn' | 'feathered' | 'bristly' | 'dry';

export type BrushSettings = {
  size: number; // diameter in pixels
  opacity: number; // 0..1, the most a single stroke can cover
  flow: number; // 0..1, how much paint each dab adds (low = airbrush build-up)
  hardness: number; // 0..1, 1 = sharp edge, 0 = very soft edge (round tips)
  spacing: number; // distance between dabs, as a fraction of the size
  sizePressure: boolean; // pen pressure changes the size
  minSize: number; // 0..1, size at the lightest pressure (1 = no change)
  texture: number; // 0..1, paper grain strength (fixed to the canvas, like CSP)
  opacityPressure: boolean; // pen pressure changes how much paint is laid down
  stabilize: number; // 0..1, smooths shaky lines
  ends: 'round' | 'taper' | 'flat'; // shape of both ends of a line
  taper: number; // 0..1, how long the tapered tips are
  mix: number; // 0..1, color mixing: 0 = clean paint, 1 = only blends the colors already there
  mixRefer: 'all' | 'layer'; // mix with everything you see, or only with the active layer
  // ---- tip and shape ----
  tip: TipShape;
  tipImage?: string; // PNG (data URL) for 'image' tips: white with transparency
  tipId?: string; // short id of the tip image (for caching)
  roundness: number; // 0.05..1, 1 = round, small = flat like a chisel or calligraphy nib
  angle: number; // degrees
  followDirection: boolean; // the tip turns with the stroke (bristles, flat nibs, grass)
  scatter: number; // 0..3, dabs spread sideways (× size)
  sizeJitter: number; // 0..1, random size changes
  angleJitter: number; // 0..1, random turning (1 = any angle)
  opacityJitter: number; // 0..1, random paint amount
  colorJitter: number; // 0..1, small random color changes (leaves, grass)
  // ---- tip designer ----
  shapeBase: ShapeBase; // for the 'shape' tip
  sides: number; // polygon sides / star points
  starInner: number; // 0.1..0.9, how deep the star's points are cut
  edge: EdgeStyle; // what the outline of the tip looks like (any tip)
  edgeAmount: number; // 0..1, how strong the edge effect is
  edgeDetail: number; // 0..1, big wobbles (0) … fine fringe (1)
  wetRim: number; // 0..1, paint collects at the edge like watercolor (lighter middle)
  grain: number; // 0..1, grainy holes inside the tip (dry media)
  variants: number; // 1..8, each dab uses a slightly different version of the tip
  streaks: number; // 0..1, bristle marks: lines along the stroke, like an oil brush leaves
  antialias: boolean; // smooth edges; off = crisp pixel edges (pixel art, clean flats)
  deform: number; // 0..1, bends and warps the tip shape (every version differently)
  hollow: number; // 0..1, outline only: the middle of the stroke empties out (double lines, outlined strokes)
  outlineWidth: number; // 0.05..0.6, how thick that outline is (× brush size)
  particles: number; // 0 = off; else how many separate small dabs make up each dab (spray, dust, sparkle)
  particleSize: number; // 0.05..0.8, size of each particle (× brush size)
  texKind: string; // which texture the paint takes ('paper', 'hair', 'skin', 'cloth', … or 'custom')
  texImage?: string; // your own texture picture (PNG data URL) for 'custom'
  texScale: number; // 0.25..4, size of the texture
  texColor: number; // 0..1, paint with the texture picture's own colors (0 = off, the brush color only)
  seed: number; // changes the random pattern of edges and grain
};

export type BrushPreset = BrushSettings & { name: string; category: string; about?: string };

// Settings that older saved brushes don't have
export const EXTRA_DEFAULTS = {
  tip: 'round' as TipShape,
  roundness: 1,
  angle: 0,
  followDirection: false,
  scatter: 0,
  sizeJitter: 0,
  angleJitter: 0,
  opacityJitter: 0,
  colorJitter: 0,
  shapeBase: 'circle' as ShapeBase,
  sides: 5,
  starInner: 0.45,
  edge: 'smooth' as EdgeStyle,
  edgeAmount: 0.5,
  edgeDetail: 0.5,
  wetRim: 0,
  grain: 0,
  variants: 1,
  seed: 1,
  streaks: 0,
  antialias: true,
  deform: 0,
  hollow: 0,
  outlineWidth: 0.18,
  particles: 0,
  particleSize: 0.25,
  texKind: 'paper',
  texScale: 1,
  texColor: 0,
};

const BASE: BrushSettings = {
  size: 10,
  opacity: 1,
  flow: 1,
  hardness: 0.85,
  spacing: 0.06,
  sizePressure: true,
  minSize: 0.5,
  texture: 0,
  opacityPressure: false,
  stabilize: 0.2,
  ends: 'round',
  taper: 0.3,
  mix: 0,
  mixRefer: 'all',
  ...EXTRA_DEFAULTS,
};

export const TIP_SHAPES: { id: TipShape; label: string }[] = [
  { id: 'round', label: 'Round' },
  { id: 'shape', label: 'Shape…' },
  { id: 'bristle', label: 'Bristles' },
  { id: 'chalk', label: 'Chalk' },
  { id: 'wet', label: 'Wet edge' },
  { id: 'splatter', label: 'Splatter' },
  { id: 'leaf', label: 'Leaf' },
  { id: 'grass', label: 'Grass' },
  { id: 'star', label: 'Sparkle' },
  { id: 'cloud', label: 'Cloud' },
  { id: 'strands', label: 'Hair / fur' },
  { id: 'speckle', label: 'Speckles' },
  { id: 'grunge', label: 'Grunge' },
  { id: 'hatch', label: 'Hatching' },
  { id: 'scale', label: 'Scale' },
  { id: 'image', label: 'Image' },
];

export const SHAPE_BASES: { id: ShapeBase; label: string; icon: string }[] = [
  { id: 'circle', label: 'Circle', icon: '●' },
  { id: 'square', label: 'Square', icon: '■' },
  { id: 'rounded', label: 'Rounded', icon: '▢' },
  { id: 'triangle', label: 'Triangle', icon: '▲' },
  { id: 'diamond', label: 'Diamond', icon: '◆' },
  { id: 'polygon', label: 'Polygon', icon: '⬟' },
  { id: 'star', label: 'Star', icon: '★' },
  { id: 'heart', label: 'Heart', icon: '♥' },
  { id: 'crescent', label: 'Moon', icon: '☾' },
  { id: 'drop', label: 'Drop', icon: '💧' },
];

export const EDGES: { id: EdgeStyle; label: string; about: string }[] = [
  { id: 'smooth', label: 'Smooth', about: 'Clean outline' },
  { id: 'ragged', label: 'Ragged', about: 'Uneven, organic outline like real paint or watercolor' },
  { id: 'torn', label: 'Torn', about: 'Rough, broken outline like charcoal or torn paper' },
  { id: 'feathered', label: 'Feathered', about: 'Soft, cloudy outline' },
  { id: 'bristly', label: 'Bristly', about: 'Little hairs sticking out of the edge' },
  { id: 'dry', label: 'Dry brush', about: 'Streaks with gaps, like a brush running out of paint' },
];

export const CATEGORIES = ['Pens', 'Pencils', 'Markers', 'Paint', 'Watercolor', 'Airbrush', 'Blend', 'Textures', 'Rough & particles', 'Nature & FX', 'My brushes'];

const b = (name: string, category: string, about: string, s: Partial<BrushSettings>): BrushPreset => ({ ...BASE, ...s, name, category, about });

export const PRESETS: BrushPreset[] = [
  // ---- Pens
  b('Pen', 'Pens', 'Ballpoint: even line with a light grain.', { size: 8, opacity: 0.95, flow: 0.9, hardness: 0.85, minSize: 0.65, opacityPressure: true, texture: 0.18, stabilize: 0.3, ends: 'taper', taper: 0.25 }),
  b('Ink', 'Pens', 'Sharp, clean lineart. Thin to thick with pen pressure.', { size: 4, hardness: 1, spacing: 0.04, minSize: 0.25, stabilize: 0.55, ends: 'taper', taper: 0.5 }),
  b('G-pen', 'Pens', 'Manga G-pen: very thin to very thick, crisp ends.', { size: 10, hardness: 1, spacing: 0.03, minSize: 0.08, stabilize: 0.5, ends: 'taper', taper: 0.6 }),
  b('Maru pen', 'Pens', 'Fine manga pen for small details and hair.', { size: 3, hardness: 1, spacing: 0.04, minSize: 0.3, stabilize: 0.45, ends: 'taper', taper: 0.4 }),
  b('Gel pen', 'Pens', 'Smooth, solid and even.', { size: 6, hardness: 0.95, minSize: 0.75, stabilize: 0.3 }),
  b('Fineliner', 'Pens', 'Same width everywhere, like a technical pen.', { size: 4, hardness: 0.95, sizePressure: false, minSize: 1, stabilize: 0.4 }),
  b('Brush pen', 'Pens', 'Soft brush tip with ink: expressive thick-thin lines.', { size: 18, hardness: 0.95, spacing: 0.03, minSize: 0.08, stabilize: 0.4, ends: 'taper', taper: 0.7, texture: 0.05 }),
  b('Dip pen', 'Pens', 'Steel nib: very thin to bold with pressure, slightly rough ink edge.', { size: 9, hardness: 1, edge: 'ragged', edgeAmount: 0.15, edgeDetail: 0.9, variants: 3, spacing: 0.025, minSize: 0.03, stabilize: 0.5, ends: 'taper', taper: 0.65 }),
  b('Double line pen', 'Pens', 'Draws two clean parallel lines at once (outline only).', { size: 16, hardness: 1, hollow: 1, outlineWidth: 0.12, spacing: 0.03, sizePressure: false, minSize: 1, stabilize: 0.4 }),
  b('Sumi ink brush', 'Pens', 'Japanese ink brush: wet, dry-brush ends, rich blacks.', { size: 30, tip: 'bristle', roundness: 0.75, followDirection: true, edge: 'dry', edgeAmount: 0.35, edgeDetail: 0.85, streaks: 0.4, variants: 4, spacing: 0.03, minSize: 0.1, ends: 'taper', taper: 0.6, stabilize: 0.3 }),
  b('Calligraphy', 'Pens', 'Flat nib held at 45°: thin and thick depending on direction.', { size: 22, hardness: 1, spacing: 0.02, minSize: 0.6, roundness: 0.18, angle: 45, stabilize: 0.35 }),
  b('Anime lineart', 'Pens', 'Clean manga/anime line art: sharp, strong taper, steady.', { size: 5, hardness: 1, spacing: 0.025, minSize: 0.05, stabilize: 0.6, ends: 'taper', taper: 0.7 }),
  b('Hair strands', 'Pens', 'Long thin strands for hair: very pointed ends, light pressure stays thin.', { size: 6, hardness: 1, spacing: 0.025, minSize: 0.02, stabilize: 0.45, ends: 'taper', taper: 0.9, opacityPressure: true }),
  // ---- Pencils
  b('Pencil', 'Pencils', 'HB pencil for sketching. Press harder for darker lines.', { size: 6, hardness: 0.7, minSize: 0.5, opacityPressure: true, texture: 0.45, stabilize: 0.1 }),
  b('Soft pencil 2B', 'Pencils', 'Softer, darker, grainier.', { size: 9, hardness: 0.55, minSize: 0.45, opacityPressure: true, texture: 0.4, stabilize: 0.1 }),
  b('Dark pencil 6B', 'Pencils', 'Very soft and dark, good for shading.', { size: 14, hardness: 0.45, minSize: 0.4, opacityPressure: true, texture: 0.38, stabilize: 0.05 }),
  b('Mechanical', 'Pencils', 'Thin, clean, even pencil line.', { size: 2.5, hardness: 0.8, minSize: 0.8, opacityPressure: true, texture: 0.3, stabilize: 0.2 }),
  b('Colored pencil', 'Pencils', 'Waxy colored pencil with paper grain.', { size: 7, hardness: 0.6, minSize: 0.6, opacityPressure: true, texture: 0.55, stabilize: 0.1, opacity: 0.9 }),
  b('Charcoal', 'Pencils', 'Rough, powdery and dark, with a broken edge.', { size: 24, tip: 'chalk', hardness: 0.6, roundness: 0.6, followDirection: true, edge: 'torn', edgeAmount: 0.4, edgeDetail: 0.7, grain: 0.25, variants: 4, spacing: 0.08, minSize: 0.5, opacityPressure: true, texture: 0.6, stabilize: 0.05 }),
  b('Pastel', 'Pencils', 'Soft chalk pastel that catches the paper texture.', { size: 30, tip: 'chalk', hardness: 0.6, edge: 'torn', edgeAmount: 0.3, edgeDetail: 0.5, grain: 0.3, variants: 4, angleJitter: 1, spacing: 0.08, minSize: 0.6, opacityPressure: true, texture: 0.5, mix: 0.1, opacity: 0.9 }),
  b('Wax crayon', 'Pencils', 'Waxy crayon that skips over the paper grain.', { size: 18, tip: 'chalk', hardness: 0.7, edge: 'torn', edgeAmount: 0.3, grain: 0.35, variants: 4, opacity: 0.95, spacing: 0.06, minSize: 0.7, texture: 0.65, texKind: 'canvas', texScale: 0.6 }),
  b('Rough pencil', 'Pencils', 'Big, loose sketching pencil with a rough edge.', { size: 14, tip: 'chalk', hardness: 0.6, edge: 'torn', edgeAmount: 0.35, edgeDetail: 0.8, grain: 0.25, variants: 4, minSize: 0.4, opacityPressure: true, texture: 0.5, stabilize: 0.05 }),
  // ---- Markers
  b('Marker', 'Markers', 'Flat, see-through color that does not get darker where it overlaps itself.', { size: 30, opacity: 0.5, hardness: 0.85, sizePressure: false, minSize: 1, ends: 'flat' }),
  b('Highlighter', 'Markers', 'Wide chisel tip, see-through.', { size: 34, opacity: 0.4, hardness: 0.95, sizePressure: false, minSize: 1, roundness: 0.3, angle: 0, ends: 'flat', stabilize: 0.3 }),
  b('Alcohol marker', 'Markers', 'Soft-edged marker that blends a little with what is under it.', { size: 26, opacity: 0.65, flow: 0.5, hardness: 0.55, minSize: 0.7, mix: 0.12, opacityPressure: true }),
  b('Outline marker', 'Markers', 'Hollow marker: a colored outline around an empty middle.', { size: 30, hardness: 0.95, hollow: 1, outlineWidth: 0.2, sizePressure: false, minSize: 1, ends: 'round', stabilize: 0.3 }),
  // ---- Paint
  b('Oil paint', 'Paint', 'Thick paint with bristle marks; mixes with the paint already there.', { size: 40, tip: 'shape', shapeBase: 'rounded', roundness: 0.35, angle: 90, streaks: 0.6, followDirection: true, edge: 'bristly', edgeAmount: 0.35, edgeDetail: 0.6, variants: 3, spacing: 0.05, flow: 0.9, mix: 0.45, minSize: 0.5, opacityPressure: true, stabilize: 0.1 }),
  b('Acrylic', 'Paint', 'Opaque paint with a little bristle texture.', { size: 34, tip: 'shape', shapeBase: 'square', roundness: 0.3, angle: 90, streaks: 0.4, followDirection: true, spacing: 0.05, mix: 0.2, minSize: 0.5, texture: 0.15, stabilize: 0.1 }),
  b('Gouache', 'Paint', 'Matte, flat and opaque, slightly chalky, with soft uneven edges.', { size: 30, hardness: 0.75, edge: 'ragged', edgeAmount: 0.2, edgeDetail: 0.5, variants: 4, angleJitter: 1, mix: 0.15, minSize: 0.5, texture: 0.15, flow: 0.9 }),
  b('Watercolor', 'Watercolor', 'See-through paint with uneven, darker wet edges and paper grain.', { size: 50, tip: 'round', hardness: 0.7, edge: 'ragged', edgeAmount: 0.65, edgeDetail: 0.35, wetRim: 0.7, variants: 6, angleJitter: 1, opacity: 0.75, flow: 0.3, spacing: 0.08, mix: 0, minSize: 0.5, opacityPressure: true, texture: 0.3, stabilize: 0.1 }),
  b('Watercolor wash', 'Watercolor', 'Big soft wash for skies and backgrounds.', { size: 140, tip: 'round', hardness: 0.4, edge: 'feathered', edgeAmount: 0.6, edgeDetail: 0.2, wetRim: 0.45, variants: 5, angleJitter: 1, opacity: 0.6, flow: 0.15, mix: 0, sizePressure: false, minSize: 1, texture: 0.25 }),
  b('Watercolor bloom', 'Watercolor', 'Drops of water spreading into paint: blotchy cauliflower edges.', { size: 70, tip: 'round', hardness: 0.6, edge: 'ragged', edgeAmount: 0.6, edgeDetail: 0.7, wetRim: 0.8, variants: 8, angleJitter: 1, sizeJitter: 0.3, scatter: 0.25, opacity: 0.7, flow: 0.5, spacing: 0.3, mix: 0, minSize: 0.6, texture: 0.3 }),
  // ---- Watercolor: many different waters, like CSP's set
  b('Transparent watercolor', 'Watercolor', 'Clear, even glaze with soft edges and no rim: builds up where layers overlap.', { size: 50, hardness: 0.45, edge: 'feathered', edgeAmount: 0.35, edgeDetail: 0.4, wetRim: 0, variants: 3, opacity: 0.45, flow: 0.5, spacing: 0.06, minSize: 0.6, opacityPressure: true, texture: 0.1, texKind: 'watercolor' }),
  b('Dark rim watercolor', 'Watercolor', 'Strong dark edge where the water dried, light middle.', { size: 55, hardness: 0.7, edge: 'ragged', edgeAmount: 0.5, edgeDetail: 0.4, wetRim: 1, variants: 5, angleJitter: 1, opacity: 0.8, flow: 0.45, spacing: 0.07, minSize: 0.6, texture: 0.2 }),
  b('Wet blending watercolor', 'Watercolor', 'Wet into wet: blends softly with the color already there.', { size: 60, hardness: 0.35, edge: 'feathered', edgeAmount: 0.7, edgeDetail: 0.3, wetRim: 0.3, variants: 4, angleJitter: 1, opacity: 0.7, flow: 0.45, spacing: 0.08, mix: 0.55, mixRefer: 'layer', minSize: 0.6, opacityPressure: true }),
  b('Dry watercolor', 'Watercolor', 'Little water: broken, grainy strokes that catch the paper.', { size: 45, hardness: 0.8, edge: 'dry', edgeAmount: 0.55, edgeDetail: 0.7, grain: 0.35, variants: 4, roundness: 0.7, followDirection: true, opacity: 0.8, flow: 0.55, spacing: 0.05, minSize: 0.6, texture: 0.45, texKind: 'watercolor' }),
  b('Granulating watercolor', 'Watercolor', 'Pigment settles into the paper grain (like ultramarine).', { size: 60, hardness: 0.55, edge: 'ragged', edgeAmount: 0.45, edgeDetail: 0.35, wetRim: 0.6, grain: 0.45, variants: 5, angleJitter: 1, opacity: 0.75, flow: 0.4, spacing: 0.07, minSize: 0.6, texture: 0.55, texKind: 'watercolor', texScale: 0.8 }),
  b('Flat wash brush', 'Watercolor', 'Wide flat brush for even washes and straight edges.', { size: 70, tip: 'shape', shapeBase: 'rounded', roundness: 0.28, angle: 90, followDirection: true, hardness: 0.75, edge: 'ragged', edgeAmount: 0.3, edgeDetail: 0.5, wetRim: 0.55, variants: 4, opacity: 0.7, flow: 0.4, spacing: 0.05, minSize: 0.8, texture: 0.2 }),
  b('Watercolor liner', 'Watercolor', 'Thin pointed rigger brush for twigs, hair and details.', { size: 10, hardness: 0.85, edge: 'ragged', edgeAmount: 0.25, edgeDetail: 0.8, wetRim: 0.35, variants: 3, opacity: 0.85, flow: 0.6, spacing: 0.04, minSize: 0.08, ends: 'taper', taper: 0.7, stabilize: 0.35 }),
  b('Bleeding watercolor', 'Watercolor', 'Paint bleeds out into the wet paper: soft, warped, uneven edges.', { size: 70, hardness: 0.45, edge: 'feathered', edgeAmount: 0.85, edgeDetail: 0.25, wetRim: 0.55, deform: 0.5, variants: 6, angleJitter: 1, opacity: 0.7, flow: 0.35, spacing: 0.08, minSize: 0.6 }),
  b('Mottled wash', 'Watercolor', 'Patchy, cloudy wash for skies, walls and backgrounds.', { size: 140, tip: 'cloud', hardness: 0.5, edge: 'ragged', edgeAmount: 0.6, edgeDetail: 0.3, wetRim: 0.5, deform: 0.4, variants: 6, angleJitter: 1, sizeJitter: 0.3, opacity: 0.6, flow: 0.3, spacing: 0.12, sizePressure: false, minSize: 1, texture: 0.3, texKind: 'watercolor' }),
  b('Dried ring watercolor', 'Watercolor', 'Only the dried rim of a puddle stays: a ragged ring of paint around a pale middle.', { size: 60, hardness: 0.7, edge: 'ragged', edgeAmount: 0.6, edgeDetail: 0.45, wetRim: 0.4, hollow: 0.7, outlineWidth: 0.14, variants: 6, angleJitter: 1, opacity: 0.8, flow: 0.45, spacing: 0.07, minSize: 0.6, texture: 0.2, texKind: 'watercolor' }),
  b('Salt watercolor', 'Watercolor', 'Salt sprinkled into wet paint: speckled, starry light spots.', { size: 70, hardness: 0.55, edge: 'ragged', edgeAmount: 0.5, edgeDetail: 0.3, wetRim: 0.5, grain: 0.6, variants: 5, angleJitter: 1, opacity: 0.8, flow: 0.45, spacing: 0.07, minSize: 0.6, texture: 0.45, texKind: 'stone' }),
  b('Watercolor splatter', 'Watercolor', 'Drops and spatters of watery paint with dark rims.', { size: 80, tip: 'splatter', hardness: 0.8, edge: 'ragged', edgeAmount: 0.4, wetRim: 0.7, variants: 6, angleJitter: 1, sizeJitter: 0.6, scatter: 1, opacityJitter: 0.4, opacity: 0.8, flow: 0.6, spacing: 0.7, sizePressure: false, minSize: 1, stabilize: 0 }),
  b('Watercolor dots', 'Watercolor', 'Separate wet dots and blots, each with its own rim.', { size: 60, particles: 7, particleSize: 0.22, hardness: 0.7, edge: 'ragged', edgeAmount: 0.6, wetRim: 0.8, variants: 6, angleJitter: 1, opacityJitter: 0.4, opacity: 0.8, flow: 0.6, spacing: 0.5, scatter: 0.3, sizePressure: false, minSize: 1, stabilize: 0 }),
  // ---- more paint
  b('Oil flat', 'Paint', 'Flat bristle brush: square strokes with bristle marks.', { size: 45, tip: 'shape', shapeBase: 'square', roundness: 0.3, angle: 90, followDirection: true, hardness: 0.9, edge: 'bristly', edgeAmount: 0.4, edgeDetail: 0.7, streaks: 0.7, variants: 3, mix: 0.4, mixRefer: 'layer', flow: 0.95, spacing: 0.04, minSize: 0.6 }),
  b('Oil filbert', 'Paint', 'Rounded flat brush for soft-edged oil strokes.', { size: 45, tip: 'shape', shapeBase: 'drop', roundness: 0.45, angle: 90, followDirection: true, hardness: 0.85, edge: 'ragged', edgeAmount: 0.25, streaks: 0.5, variants: 3, mix: 0.35, mixRefer: 'layer', flow: 0.95, spacing: 0.04, minSize: 0.6 }),
  b('Oil fan', 'Paint', 'Fan brush: thin bristle streaks for grass, hair and texture.', { size: 60, tip: 'bristle', roundness: 0.15, angle: 90, followDirection: true, edge: 'dry', edgeAmount: 0.5, edgeDetail: 0.9, streaks: 0.8, variants: 3, mix: 0.25, mixRefer: 'layer', flow: 0.9, spacing: 0.04, minSize: 0.7 }),
  b('Palette knife', 'Paint', 'Thick paint scraped on with a knife: hard edges, smeared color.', { size: 50, tip: 'shape', shapeBase: 'diamond', roundness: 0.35, angle: 90, followDirection: true, hardness: 1, edge: 'torn', edgeAmount: 0.25, edgeDetail: 0.6, variants: 3, mix: 0.6, mixRefer: 'layer', flow: 1, spacing: 0.03, sizePressure: false, minSize: 1 }),
  b('Impasto', 'Paint', 'Heavy, rich oil paint with deep bristle grooves.', { size: 50, tip: 'bristle', roundness: 0.6, followDirection: true, edge: 'bristly', edgeAmount: 0.5, edgeDetail: 0.6, streaks: 0.9, grain: 0.1, variants: 4, mix: 0.3, mixRefer: 'layer', flow: 1, spacing: 0.035, minSize: 0.6 }),
  b('Gouache flat', 'Paint', 'Flat, matte gouache strokes with a chalky dry edge.', { size: 40, tip: 'shape', shapeBase: 'rounded', roundness: 0.35, angle: 90, followDirection: true, hardness: 0.8, edge: 'dry', edgeAmount: 0.3, edgeDetail: 0.7, variants: 3, mix: 0.15, mixRefer: 'layer', flow: 0.95, spacing: 0.04, minSize: 0.7, texture: 0.2 }),
  b('Dry gouache', 'Paint', 'Scratchy, broken gouache from a nearly dry brush.', { size: 40, hardness: 0.85, edge: 'dry', edgeAmount: 0.6, edgeDetail: 0.8, grain: 0.3, roundness: 0.6, followDirection: true, variants: 4, mix: 0.1, mixRefer: 'layer', flow: 0.95, spacing: 0.04, minSize: 0.7, texture: 0.35 }),
  b('Outlined gouache', 'Paint', 'Opaque paint with an empty middle: a thick outlined stroke.', { size: 40, tip: 'chalk', roundness: 0.8, hardness: 0.85, hollow: 0.85, outlineWidth: 0.22, edge: 'ragged', edgeAmount: 0.35, edgeDetail: 0.7, variants: 4, flow: 0.9, spacing: 0.05, minSize: 0.6, texture: 0.15 }),
  b('Fuzzy bristle', 'Paint', 'Dense, hairy bristle strokes with fibrous, fuzzy edges.', { size: 60, tip: 'bristle', roundness: 0.7, followDirection: true, hardness: 0.9, edge: 'bristly', edgeAmount: 0.85, edgeDetail: 0.8, deform: 0.25, streaks: 0.4, variants: 6, flow: 0.85, spacing: 0.04, mix: 0.15, mixRefer: 'layer', minSize: 0.5 }),
  b('Dry brush', 'Paint', 'Streaky paint with gaps, like a brush running out of paint.', { size: 44, tip: 'round', hardness: 0.85, roundness: 0.55, followDirection: true, edge: 'dry', edgeAmount: 0.55, edgeDetail: 0.6, variants: 4, spacing: 0.04, mix: 0.15, minSize: 0.6, texture: 0.2 }),
  b('Paint mix', 'Paint', 'Wet paint: your color mixes with the colors already on the canvas.', { size: 40, hardness: 0.5, spacing: 0.08, flow: 0.8, minSize: 0.4, opacityPressure: true, mix: 0.5, stabilize: 0.1 }),
  b('Cel shade', 'Paint', 'Flat anime coloring and shadow shapes: hard edge, no texture, fills cleanly.', { size: 40, hardness: 1, spacing: 0.04, sizePressure: false, minSize: 1, stabilize: 0.25, antialias: true }),
  b('Flat chisel', 'Paint', 'Square flat brush for buildings, blocks and hard-edged shapes (like the city paintings).', { size: 40, tip: 'shape', shapeBase: 'square', roundness: 0.35, angle: 0, followDirection: false, hardness: 0.95, edge: 'dry', edgeAmount: 0.25, edgeDetail: 0.7, variants: 3, streaks: 0.25, spacing: 0.04, minSize: 0.6, mix: 0.1, stabilize: 0.15 }),
  b('Gloss highlight', 'Paint', 'Small bright highlights for eyes, hair shine, glass and metal.', { size: 10, hardness: 0.9, spacing: 0.04, minSize: 0.1, ends: 'taper', taper: 0.6, stabilize: 0.35 }),
  // ---- Airbrush
  b('Soft round', 'Airbrush', 'Soft edges for shading and smooth color.', { size: 60, flow: 0.35, hardness: 0.2, spacing: 0.08, minSize: 0.2, opacityPressure: true, stabilize: 0 }),
  b('Airbrush', 'Airbrush', 'Very soft spray that builds up slowly.', { size: 150, flow: 0.06, hardness: 0, sizePressure: false, minSize: 1, opacityPressure: true, stabilize: 0 }),
  b('Hard airbrush', 'Airbrush', 'Builds up slowly but keeps a firm edge.', { size: 60, flow: 0.12, hardness: 0.65, sizePressure: false, minSize: 1, opacityPressure: true, stabilize: 0 }),
  b('Grainy airbrush', 'Airbrush', 'Soft spray with visible grain, for gritty shading.', { size: 120, flow: 0.15, hardness: 0.1, grain: 0.5, edge: 'smooth', variants: 2, spacing: 0.08, sizePressure: false, minSize: 1, opacityPressure: true, texture: 0.35, stabilize: 0 }),
  b('Spray paint', 'Airbrush', 'Grainy spray can.', { size: 90, tip: 'splatter', flow: 0.35, spacing: 0.15, scatter: 0.4, sizeJitter: 0.4, angleJitter: 1, sizePressure: false, minSize: 1, opacityPressure: true, stabilize: 0 }),
  b('Splatter', 'Airbrush', 'Random paint drops.', { size: 70, tip: 'splatter', spacing: 0.7, scatter: 1.4, sizeJitter: 0.8, angleJitter: 1, opacityJitter: 0.3, sizePressure: true, minSize: 0.4, stabilize: 0 }),
  b('Soft blush', 'Airbrush', 'Gentle color for cheeks, glow and soft shading in anime art.', { size: 80, flow: 0.12, hardness: 0.1, spacing: 0.08, sizePressure: false, minSize: 1, opacityPressure: true, opacity: 0.7, stabilize: 0 }),
  // ---- Blend
  b('Blender', 'Blend', 'Adds no color. Smears and blends the colors already there.', { size: 50, flow: 0.6, hardness: 0.3, spacing: 0.08, minSize: 0.4, opacityPressure: true, mix: 1, stabilize: 0.1 }),
  b('Smudge', 'Blend', 'Pushes paint along like a finger.', { size: 36, flow: 0.9, hardness: 0.5, spacing: 0.05, minSize: 0.5, mix: 0.95, stabilize: 0.05 }),
  b('Bristle blender', 'Blend', 'Blends with brush marks, like a dry brush.', { size: 46, tip: 'bristle', roundness: 0.6, followDirection: true, flow: 0.7, spacing: 0.05, mix: 0.9, minSize: 0.5 }),
  b('Hair blend', 'Blend', 'Pulls the colors already there into fine hair strands along your stroke (like CSP hair blending).', { size: 40, tip: 'strands', hardness: 1, followDirection: true, angle: 0, flow: 0.75, spacing: 0.04, mix: 1, minSize: 0.5, opacityPressure: true, ends: 'taper', taper: 0.5, stabilize: 0.15 }),
  b('Fur blend', 'Blend', 'Fluffs the edge between two colors into short fur.', { size: 34, tip: 'strands', hardness: 1, followDirection: true, angle: 0, flow: 0.8, spacing: 0.08, scatter: 0.25, angleJitter: 0.1, sizeJitter: 0.3, mix: 1, minSize: 0.5, ends: 'taper', taper: 0.4, stabilize: 0 }),
  b('Finger tip', 'Blend', 'Smooth finger smear: drags paint softly in the stroke direction.', { size: 30, flow: 0.95, hardness: 0.35, spacing: 0.03, mix: 0.97, minSize: 0.6, opacityPressure: true, stabilize: 0.1 }),
  b('Textured blend', 'Blend', 'Blends with a grainy chalk texture, keeps a painterly look.', { size: 50, tip: 'chalk', flow: 0.6, hardness: 0.6, spacing: 0.06, grain: 0.25, angleJitter: 1, variants: 4, mix: 0.95, minSize: 0.5, opacityPressure: true }),
  b('Soft blur', 'Blend', 'Very soft blending that just melts edges together.', { size: 80, flow: 0.3, hardness: 0, spacing: 0.08, mix: 1, sizePressure: false, minSize: 1, opacityPressure: true }),
  b('Water blend', 'Blend', 'Softly blends and lightens, like a wet brush.', { size: 60, tip: 'wet', flow: 0.4, hardness: 0.3, mix: 0.85, minSize: 0.5, texture: 0.15 }),
  // ---- Nature & effects
  b('Leaves', 'Nature & FX', 'Scattered leaves in slightly different colors.', { size: 40, tip: 'leaf', spacing: 0.45, scatter: 0.9, sizeJitter: 0.5, angleJitter: 1, colorJitter: 0.35, sizePressure: true, minSize: 0.5, stabilize: 0 }),
  b('Grass', 'Nature & FX', 'Blades of grass that follow your stroke upwards.', { size: 45, tip: 'grass', spacing: 0.12, scatter: 0.5, sizeJitter: 0.5, angleJitter: 0.12, colorJitter: 0.25, minSize: 0.5, stabilize: 0 }),
  b('Clouds', 'Nature & FX', 'Soft puffy clouds; build them up in layers.', { size: 120, tip: 'cloud', flow: 0.35, spacing: 0.2, scatter: 0.5, sizeJitter: 0.5, angleJitter: 1, opacityJitter: 0.4, sizePressure: false, minSize: 1, stabilize: 0 }),
  b('Sparkles', 'Nature & FX', 'Twinkling stars and glitter.', { size: 40, tip: 'star', spacing: 1.2, scatter: 2, sizeJitter: 0.8, angleJitter: 0.15, opacityJitter: 0.4, minSize: 0.3, stabilize: 0 }),
  b('Hearts', 'Nature & FX', 'Little hearts scattered along the stroke.', { size: 30, tip: 'shape', shapeBase: 'heart', hardness: 1, spacing: 1.1, scatter: 1.2, sizeJitter: 0.6, angleJitter: 0.15, colorJitter: 0.2, minSize: 0.5, stabilize: 0 }),
  b('Confetti', 'Nature & FX', 'Colorful scattered confetti shapes.', { size: 24, tip: 'shape', shapeBase: 'square', hardness: 1, roundness: 0.45, spacing: 0.9, scatter: 1.6, sizeJitter: 0.5, angleJitter: 1, colorJitter: 0.9, minSize: 0.6, stabilize: 0 }),
  b('Scale stamps', 'Nature & FX', 'Single scales placed one by one along the stroke, with gaps (dragons, fish, armor). Draw along the body.', { size: 34, tip: 'scale', hardness: 1, followDirection: true, angle: 0, spacing: 0.95, sizeJitter: 0.12, angleJitter: 0.02, colorJitter: 0.12, opacityJitter: 0.1, minSize: 0.6, stabilize: 0.2 }),
  b('Glass sparkle', 'Nature & FX', 'Four-point star sparkles for glass, eyes and magic (like the glass spheres).', { size: 26, tip: 'star', spacing: 1.6, scatter: 1.4, sizeJitter: 0.7, angleJitter: 0.05, opacityJitter: 0.3, minSize: 0.3, stabilize: 0 }),
  b('Fur', 'Nature & FX', 'Soft fur: fine hairs that follow your stroke. Use short flicks in the direction the fur grows.', { size: 60, tip: 'strands', hardness: 1, followDirection: true, angle: 0, spacing: 0.14, scatter: 0.3, sizeJitter: 0.3, angleJitter: 0.1, opacityJitter: 0.35, colorJitter: 0.12, flow: 0.85, minSize: 0.55, ends: 'taper', taper: 0.5, stabilize: 0.1 }),
  b('Hair strands brush', 'Nature & FX', 'Many fine strands at once for flowing hair.', { size: 60, tip: 'strands', hardness: 1, followDirection: true, angle: 0, spacing: 0.05, opacityJitter: 0.15, flow: 0.7, minSize: 0.5, ends: 'taper', taper: 0.7, stabilize: 0.35 }),
  // ---- Rough & particles (like CSP's texture and spray brushes)
  b('Grunge stamp', 'Rough & particles', 'Worn, blotchy stamps for dirt, rust and wear.', { size: 90, tip: 'grunge', spacing: 0.7, scatter: 0.6, sizeJitter: 0.4, angleJitter: 1, opacityJitter: 0.3, deform: 0.3, variants: 4, sizePressure: false, minSize: 1, stabilize: 0 }),
  b('Dust particles', 'Rough & particles', 'Separate fine particles: dust, snow, sparkle spray.', { size: 80, particles: 16, particleSize: 0.07, hardness: 0.8, spacing: 0.4, scatter: 0.3, opacityJitter: 0.5, sizePressure: false, minSize: 1, stabilize: 0 }),
  b('Speckle spray', 'Rough & particles', 'Clusters of separate dots of different sizes.', { size: 60, tip: 'speckle', spacing: 0.45, scatter: 0.5, sizeJitter: 0.4, angleJitter: 1, opacityJitter: 0.3, minSize: 0.6, stabilize: 0 }),
  b('Rough rock', 'Rough & particles', 'Bent, torn chunks for rocks, rubble and broken edges.', { size: 50, tip: 'round', hardness: 0.95, edge: 'torn', edgeAmount: 0.6, edgeDetail: 0.55, deform: 0.7, variants: 6, angleJitter: 1, sizeJitter: 0.35, spacing: 0.35, scatter: 0.3, minSize: 0.6, stabilize: 0 }),
  b('Splotch', 'Rough & particles', 'Warped paint splotches, each one different.', { size: 70, tip: 'round', hardness: 0.8, edge: 'ragged', edgeAmount: 0.7, edgeDetail: 0.4, deform: 0.9, variants: 8, angleJitter: 1, sizeJitter: 0.5, scatter: 0.6, spacing: 0.6, opacityJitter: 0.3, minSize: 0.5, stabilize: 0 }),
  b('Crosshatch', 'Rough & particles', 'Pencil hatching lines for shading.', { size: 40, tip: 'hatch', hardness: 1, followDirection: true, angle: 45, spacing: 0.35, opacityJitter: 0.25, texture: 0.25, minSize: 0.7, opacityPressure: true, stabilize: 0.1 }),
  b('Dry scratch', 'Rough & particles', 'Scratchy, broken dry strokes with bent bristles.', { size: 40, tip: 'bristle', roundness: 0.4, followDirection: true, edge: 'dry', edgeAmount: 0.7, edgeDetail: 0.85, deform: 0.4, grain: 0.15, variants: 5, streaks: 0.5, spacing: 0.04, minSize: 0.6, stabilize: 0.05 }),
  b('Particle sparkle', 'Rough & particles', 'Tiny four-point stars scattered as particles.', { size: 70, tip: 'star', particles: 6, particleSize: 0.22, spacing: 0.9, scatter: 0.4, opacityJitter: 0.4, sizePressure: false, minSize: 1, stabilize: 0 }),
  // ---- Textures: brushes that paint with a surface texture (fixed to the canvas)
  b('Hair texture', 'Textures', 'Paints with fine hair strands inside: shade hair quickly.', { size: 60, hardness: 0.7, flow: 0.55, texture: 0.95, texKind: 'hair', texScale: 1, minSize: 0.6, opacityPressure: true, ends: 'taper', taper: 0.4, stabilize: 0.15 }),
  b('Fur texture', 'Textures', 'Short fur texture for animals and fluffy clothes.', { size: 60, hardness: 0.6, flow: 0.55, texture: 0.9, texKind: 'fur', texScale: 1, scatter: 0.1, angleJitter: 0.04, minSize: 0.6, opacityPressure: true, ends: 'taper', taper: 0.3 }),
  b('Skin texture', 'Textures', 'Soft color with subtle pores and mottling for realistic skin.', { size: 70, hardness: 0.2, flow: 0.35, texture: 0.45, texKind: 'skin', texScale: 1, minSize: 0.7, opacityPressure: true }),
  b('Cloth / linen', 'Textures', 'Woven fabric texture for clothes.', { size: 50, hardness: 0.75, flow: 0.8, texture: 0.65, texKind: 'cloth', texScale: 1, minSize: 0.7 }),
  b('Denim', 'Textures', 'Diagonal twill texture for jeans and jackets.', { size: 50, hardness: 0.75, flow: 0.8, texture: 0.6, texKind: 'denim', texScale: 1, minSize: 0.7 }),
  b('Knit wool', 'Textures', 'Knitted sweater texture.', { size: 50, hardness: 0.7, flow: 0.8, texture: 0.65, texKind: 'knit', texScale: 1, minSize: 0.7 }),
  b('Leather', 'Textures', 'Cracked leather grain.', { size: 50, hardness: 0.7, flow: 0.8, texture: 0.55, texKind: 'leather', texScale: 1, minSize: 0.7 }),
  b('Scales', 'Textures', 'Dragon, fish and snake scales with clear gaps, so every scale reads on its own.', { size: 80, hardness: 0.8, flow: 0.95, texture: 1, texKind: 'scales', texScale: 1.5, minSize: 0.8 }),
  b('Wood grain', 'Textures', 'Wood grain for furniture, floors and staffs.', { size: 50, hardness: 0.7, flow: 0.8, texture: 0.6, texKind: 'wood', texScale: 1, minSize: 0.7 }),
  b('Stone / concrete', 'Textures', 'Rough stone and concrete surface.', { size: 60, hardness: 0.6, flow: 0.75, texture: 0.65, texKind: 'stone', texScale: 1, minSize: 0.7 }),
  b('Canvas oil', 'Textures', 'Oil paint on canvas weave.', { size: 45, tip: 'bristle', roundness: 0.6, followDirection: true, streaks: 0.3, edge: 'bristly', edgeAmount: 0.3, variants: 3, mix: 0.3, mixRefer: 'layer', texture: 0.45, texKind: 'canvas', texScale: 1, minSize: 0.5 }),
  b('Watercolor paper', 'Textures', 'Watercolor on rough cold-press paper.', { size: 60, hardness: 0.6, edge: 'ragged', edgeAmount: 0.6, edgeDetail: 0.35, wetRim: 0.7, variants: 5, angleJitter: 1, opacity: 0.75, flow: 0.35, spacing: 0.08, texture: 0.55, texKind: 'watercolor', texScale: 1, minSize: 0.5, opacityPressure: true }),
  b('Grunge texture', 'Textures', 'Dirty, worn surface texture.', { size: 70, hardness: 0.6, flow: 0.8, texture: 0.7, texKind: 'grunge', texScale: 1, minSize: 0.8 }),
  b('Crosshatch shading', 'Textures', 'Shade with crossed ink lines.', { size: 60, hardness: 0.6, flow: 0.8, texture: 0.85, texKind: 'crosshatch', texScale: 1, minSize: 0.8, opacityPressure: true }),
  b('Halftone shading', 'Textures', 'Manga screentone dots.', { size: 60, hardness: 0.7, flow: 0.9, texture: 0.9, texKind: 'dots', texScale: 1, minSize: 0.8, antialias: true }),
];

// The eraser keeps its own settings, separate from the brush
export const ERASER_PRESETS: BrushPreset[] = [
  b('Hard eraser', 'Eraser', 'Removes everything it touches, completely.', { size: 20, hardness: 0.9, spacing: 0.05, minSize: 0.5, stabilize: 0 }),
  b('Soft eraser', 'Eraser', 'Fades paint out gently. Go over again to erase more.', { size: 80, flow: 0.3, hardness: 0.1, minSize: 0.4, opacityPressure: true, stabilize: 0 }),
  b('Textured eraser', 'Eraser', 'Grainy, like a rubber on pencil.', { size: 30, hardness: 0.7, minSize: 0.5, opacityPressure: true, texture: 0.45, stabilize: 0 }),
];

// Brush settings without the name/category, with every newer setting filled in
export function settingsOf(p: Partial<BrushPreset>): BrushSettings {
  const { name, category, about, ...settings } = p as BrushPreset;
  void name;
  void category;
  void about;
  return { ...BASE, ...settings };
}

// ---------- Brush tips ----------
// Each tip shape is drawn once as a white mask (256 px) and then stamped, scaled,
// turned and colored along the stroke.

const DAB_SIZE = 256;
const masks = new Map<string, HTMLCanvasElement>();
const colored = new Map<string, HTMLCanvasElement>();

// Speed settings (Settings → Performance). Slower computers use smaller, fewer tip versions.
export type BrushQuality = 'high' | 'balanced' | 'fast';
// `final`: the finished stroke is drawn again at full quality, so the result never depends
// on the speed setting (only the live preview while the pen moves does)
export const brushPerf = { quality: 'balanced' as BrushQuality, strokeEdges: true, final: false };
const edgeQ = (): BrushQuality => (brushPerf.final ? 'high' : brushPerf.quality);
// true when the live preview uses lighter tips than the finished stroke
export const previewIsLighter = () => brushPerf.quality !== 'high';

export function setBrushPerf(quality: BrushQuality, strokeEdges: boolean) {
  if (brushPerf.quality !== quality) {
    masks.clear();
    colored.clear();
  }
  brushPerf.quality = quality;
  brushPerf.strokeEdges = strokeEdges;
}

const maxVariants = () => (brushPerf.quality === 'high' ? 8 : brushPerf.quality === 'balanced' ? 4 : 2);

// Keeps a cache small by forgetting the oldest entries (not all of them at once,
// which made every dab be rebuilt and the brush stutter)
function keep(map: Map<string, HTMLCanvasElement>, key: string, value: HTMLCanvasElement, max: number) {
  map.delete(key);
  map.set(key, value);
  while (map.size > max) map.delete(map.keys().next().value as string);
}

function canvas(size = DAB_SIZE) {
  const c = document.createElement('canvas');
  c.width = size;
  c.height = size;
  return c;
}

// small repeatable random numbers, so procedural tips look the same every time
function rng(seed: number) {
  return () => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed / 4294967296;
  };
}

function roundMask(hardness: number) {
  const c = canvas();
  const ctx = c.getContext('2d')!;
  const m = DAB_SIZE / 2;
  const h = Math.min(Math.max(hardness, 0), 0.97); // keep a tiny soft edge so lines are anti-aliased
  const g = ctx.createRadialGradient(m, m, 0, m, m, m);
  g.addColorStop(0, 'rgba(255,255,255,1)');
  g.addColorStop(h, 'rgba(255,255,255,1)');
  g.addColorStop(h + (1 - h) * 0.5, 'rgba(255,255,255,0.35)');
  g.addColorStop(1, 'rgba(255,255,255,0)');
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, DAB_SIZE, DAB_SIZE);
  return c;
}

function proceduralMask(tip: TipShape, hardness: number): HTMLCanvasElement {
  const c = canvas();
  const ctx = c.getContext('2d')!;
  const m = DAB_SIZE / 2;
  const R = rng(7 + tip.length * 31);
  ctx.fillStyle = '#fff';
  switch (tip) {
    case 'bristle': {
      // many small hair tips in a bundle
      for (let i = 0; i < 70; i++) {
        const a = R() * Math.PI * 2, d = Math.sqrt(R()) * m * 0.85;
        ctx.globalAlpha = 0.35 + R() * 0.65;
        ctx.beginPath();
        ctx.arc(m + Math.cos(a) * d, m + Math.sin(a) * d, 6 + R() * 12, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
      return blurred(c, 2);
    }
    case 'chalk': {
      ctx.drawImage(roundMask(Math.max(0.4, hardness)), 0, 0);
      // knock random holes in it
      ctx.globalCompositeOperation = 'destination-out';
      for (let i = 0; i < 900; i++) {
        ctx.globalAlpha = R() * 0.8;
        ctx.fillRect(R() * DAB_SIZE, R() * DAB_SIZE, 2 + R() * 6, 2 + R() * 6);
      }
      ctx.globalCompositeOperation = 'source-over';
      ctx.globalAlpha = 1;
      return blurred(c, 1);
    }
    case 'wet': {
      // see-through middle, darker rim: watercolor pooling at the edge
      const g = ctx.createRadialGradient(m, m, 0, m, m, m);
      const h = Math.min(0.9, Math.max(0.5, hardness));
      g.addColorStop(0, 'rgba(255,255,255,0.45)');
      g.addColorStop(h * 0.85, 'rgba(255,255,255,0.6)');
      g.addColorStop(h, 'rgba(255,255,255,0.95)');
      g.addColorStop(Math.min(0.99, h + 0.08), 'rgba(255,255,255,0.5)');
      g.addColorStop(1, 'rgba(255,255,255,0)');
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, DAB_SIZE, DAB_SIZE);
      return c;
    }
    case 'splatter': {
      for (let i = 0; i < 26; i++) {
        const a = R() * Math.PI * 2, d = Math.sqrt(R()) * m * 0.85;
        ctx.beginPath();
        ctx.arc(m + Math.cos(a) * d, m + Math.sin(a) * d, 2 + Math.pow(R(), 3) * 22, 0, Math.PI * 2);
        ctx.fill();
      }
      return c;
    }
    case 'leaf': {
      ctx.beginPath();
      ctx.moveTo(m, 10);
      ctx.bezierCurveTo(m + 95, 70, m + 70, 190, m, DAB_SIZE - 10);
      ctx.bezierCurveTo(m - 70, 190, m - 95, 70, m, 10);
      ctx.fill();
      ctx.globalCompositeOperation = 'destination-out';
      ctx.strokeStyle = 'rgba(0,0,0,0.5)';
      ctx.lineWidth = 5;
      ctx.beginPath();
      ctx.moveTo(m, 30);
      ctx.lineTo(m, DAB_SIZE - 20);
      ctx.stroke();
      ctx.globalCompositeOperation = 'source-over';
      return c;
    }
    case 'grass': {
      // a few blades growing up from the bottom middle
      for (let i = 0; i < 4; i++) {
        const x = m + (R() - 0.5) * 60, lean = (R() - 0.5) * 80, top = 10 + R() * 60, w = 7 + R() * 7;
        ctx.globalAlpha = 0.75 + R() * 0.25;
        ctx.beginPath();
        ctx.moveTo(x - w, DAB_SIZE);
        ctx.quadraticCurveTo(x - w * 0.5 + lean * 0.3, (DAB_SIZE + top) / 2, x + lean, top);
        ctx.quadraticCurveTo(x + w * 0.5 + lean * 0.3, (DAB_SIZE + top) / 2, x + w, DAB_SIZE);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
      return c;
    }
    case 'star': {
      const g = ctx.createRadialGradient(m, m, 0, m, m, m * 0.35);
      g.addColorStop(0, 'rgba(255,255,255,1)');
      g.addColorStop(1, 'rgba(255,255,255,0)');
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, DAB_SIZE, DAB_SIZE);
      ctx.fillStyle = '#fff';
      for (const [w, l] of [[10, m], [6, m * 0.6]] as [number, number][]) {
        for (let k = 0; k < 4; k++) {
          ctx.save();
          ctx.translate(m, m);
          ctx.rotate((k * Math.PI) / 2 + (l < m ? Math.PI / 4 : 0));
          ctx.beginPath();
          ctx.moveTo(0, -w);
          ctx.lineTo(l, 0);
          ctx.lineTo(0, w);
          ctx.closePath();
          ctx.fill();
          ctx.restore();
        }
      }
      return c;
    }
    case 'strands': {
      // fine hairs along the tip's length (x axis): thick at the root, pointed at the tip
      ctx.strokeStyle = '#fff';
      ctx.lineCap = 'round';
      for (let i = 0; i < 46; i++) {
        const y0 = m + (R() - 0.5) * m * 1.3;
        const x0 = m - m * (0.85 - R() * 0.5);
        const len = m * (0.7 + R() * 1.0);
        const bend = (R() - 0.5) * m * 0.35;
        const wid = 1.2 + R() * 2.4;
        ctx.globalAlpha = 0.35 + R() * 0.65;
        const seg = 12;
        for (let k = 0; k < seg; k++) {
          const t0 = k / seg, t1 = (k + 1) / seg;
          ctx.lineWidth = Math.max(0.3, wid * (1 - t0));
          ctx.beginPath();
          ctx.moveTo(x0 + len * t0, y0 + bend * t0 * t0);
          ctx.lineTo(x0 + len * t1, y0 + bend * t1 * t1);
          ctx.stroke();
        }
      }
      ctx.globalAlpha = 1;
      return c;
    }
    case 'speckle': {
      // separate small dots of different sizes, more in the middle
      for (let i = 0; i < 60; i++) {
        const a = R() * Math.PI * 2, d = Math.pow(R(), 0.7) * m * 0.92;
        ctx.globalAlpha = 0.5 + R() * 0.5;
        ctx.beginPath();
        ctx.arc(m + Math.cos(a) * d, m + Math.sin(a) * d, 1.5 + Math.pow(R(), 2.5) * 10, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
      return c;
    }
    case 'grunge': {
      // a worn, blotchy stamp: soft round base, eaten away by noise, cracked
      const img = ctx.createImageData(DAB_SIZE, DAB_SIZE);
      for (let y = 0; y < DAB_SIZE; y++)
        for (let x = 0; x < DAB_SIZE; x++) {
          const dx = (x - m) / m, dy = (y - m) / m;
          const d = Math.hypot(dx, dy);
          const n = fbm(x / 22, y / 22, 41) * 0.6 + noise(x / 5, y / 5, 43) * 0.4;
          const edge = 1 - smoothstep(0.75, 1.0, d + (n - 0.5) * 0.5);
          const a = edge * smoothstep(0.32, 0.6, n);
          img.data[(y * DAB_SIZE + x) * 4 + 3] = Math.round(a * 255);
          img.data[(y * DAB_SIZE + x) * 4] = img.data[(y * DAB_SIZE + x) * 4 + 1] = img.data[(y * DAB_SIZE + x) * 4 + 2] = 255;
        }
      ctx.putImageData(img, 0, 0);
      return c;
    }
    case 'scale': {
      // one scale, pointing along the stroke: solid rim, slightly lighter middle, a ridge line
      const R2 = m * 0.92;
      const shape = () => {
        ctx.beginPath();
        ctx.moveTo(m - R2 * 0.75, m - R2 * 0.72);
        ctx.quadraticCurveTo(m + R2 * 0.55, m - R2 * 0.85, m + R2, m);
        ctx.quadraticCurveTo(m + R2 * 0.55, m + R2 * 0.85, m - R2 * 0.75, m + R2 * 0.72);
        ctx.quadraticCurveTo(m - R2 * 0.45, m, m - R2 * 0.75, m - R2 * 0.72);
        ctx.closePath();
      };
      shape();
      ctx.fill();
      ctx.globalCompositeOperation = 'destination-out';
      const g = ctx.createRadialGradient(m - R2 * 0.1, m, 0, m - R2 * 0.1, m, R2 * 0.8);
      g.addColorStop(0, 'rgba(0,0,0,0.35)');
      g.addColorStop(0.75, 'rgba(0,0,0,0.12)');
      g.addColorStop(1, 'rgba(0,0,0,0)');
      ctx.fillStyle = g;
      shape();
      ctx.fill();
      // the ridge down the middle
      ctx.globalCompositeOperation = 'source-over';
      ctx.strokeStyle = 'rgba(255,255,255,0.6)';
      ctx.lineWidth = 5;
      ctx.beginPath();
      ctx.moveTo(m - R2 * 0.45, m);
      ctx.quadraticCurveTo(m + R2 * 0.3, m, m + R2 * 0.85, m);
      ctx.stroke();
      return c;
    }
    case 'hatch': {
      // parallel pencil hatching lines
      ctx.strokeStyle = '#fff';
      ctx.lineCap = 'round';
      for (let i = -6; i <= 6; i++) {
        const off = i * (m / 6.5) + (R() - 0.5) * 4;
        const half = Math.sqrt(Math.max(0, m * m * 0.85 - off * off));
        ctx.globalAlpha = 0.55 + R() * 0.45;
        ctx.lineWidth = 2 + R() * 2.5;
        ctx.beginPath();
        ctx.moveTo(m - half * (0.7 + R() * 0.3), m + off);
        ctx.lineTo(m + half * (0.7 + R() * 0.3), m + off + (R() - 0.5) * 6);
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
      return c;
    }
    case 'cloud': {
      for (let i = 0; i < 9; i++) {
        const a = R() * Math.PI * 2, d = R() * m * 0.45, r = m * (0.3 + R() * 0.25);
        const g = ctx.createRadialGradient(m + Math.cos(a) * d, m + Math.sin(a) * d, 0, m + Math.cos(a) * d, m + Math.sin(a) * d, r);
        g.addColorStop(0, 'rgba(255,255,255,0.8)');
        g.addColorStop(1, 'rgba(255,255,255,0)');
        ctx.fillStyle = g;
        ctx.fillRect(0, 0, DAB_SIZE, DAB_SIZE);
      }
      return c;
    }
    default:
      return roundMask(hardness);
  }
}

function blurred(src: HTMLCanvasElement, px: number) {
  const c = canvas(src.width);
  const ctx = c.getContext('2d')!;
  ctx.filter = `blur(${px}px)`;
  ctx.drawImage(src, 0, 0);
  ctx.filter = 'none';
  return c;
}

// ---------- Tip designer: shapes ----------

function shapeMask(s: BrushSettings): HTMLCanvasElement {
  const c = canvas();
  const ctx = c.getContext('2d')!;
  const m = DAB_SIZE / 2;
  const R = m * 0.92;
  ctx.fillStyle = '#fff';
  ctx.beginPath();
  const poly = (n: number, inner = 1, rot = -Math.PI / 2) => {
    const k = inner < 1 ? n * 2 : n;
    for (let i = 0; i < k; i++) {
      const rr = inner < 1 && i % 2 ? R * inner : R;
      const a = rot + (i * Math.PI * 2) / k;
      if (i) ctx.lineTo(m + Math.cos(a) * rr, m + Math.sin(a) * rr);
      else ctx.moveTo(m + Math.cos(a) * rr, m + Math.sin(a) * rr);
    }
    ctx.closePath();
  };
  switch (s.shapeBase) {
    case 'square':
      ctx.rect(m - R * 0.8, m - R * 0.8, R * 1.6, R * 1.6);
      break;
    case 'rounded':
      ctx.roundRect(m - R * 0.82, m - R * 0.82, R * 1.64, R * 1.64, R * 0.35);
      break;
    case 'triangle':
      poly(3);
      break;
    case 'diamond':
      poly(4);
      break;
    case 'polygon':
      poly(Math.max(3, Math.min(12, Math.round(s.sides))));
      break;
    case 'star':
      poly(Math.max(3, Math.min(12, Math.round(s.sides))), Math.max(0.1, Math.min(0.9, s.starInner)));
      break;
    case 'heart':
      ctx.moveTo(m, m + R * 0.85);
      ctx.bezierCurveTo(m - R * 1.25, m + R * 0.05, m - R * 0.75, m - R * 0.95, m, m - R * 0.38);
      ctx.bezierCurveTo(m + R * 0.75, m - R * 0.95, m + R * 1.25, m + R * 0.05, m, m + R * 0.85);
      break;
    case 'crescent':
      ctx.arc(m, m, R * 0.9, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = 'destination-out';
      ctx.beginPath();
      ctx.arc(m + R * 0.42, m - R * 0.18, R * 0.78, 0, Math.PI * 2);
      break;
    case 'drop':
      ctx.moveTo(m, m - R);
      ctx.bezierCurveTo(m + R * 0.25, m - R * 0.4, m + R * 0.75, m + R * 0.05, m + R * 0.72, m + R * 0.38);
      ctx.arc(m, m + R * 0.38, R * 0.72, 0, Math.PI);
      ctx.bezierCurveTo(m - R * 0.75, m + R * 0.05, m - R * 0.25, m - R * 0.4, m, m - R);
      break;
    default:
      ctx.arc(m, m, R, 0, Math.PI * 2);
  }
  ctx.fill();
  ctx.globalCompositeOperation = 'source-over';
  // hardness = how soft the outline is
  const soft = (1 - Math.min(1, Math.max(0, s.hardness))) * 26;
  return soft > 0.3 ? blurred(c, soft) : c;
}

// ---------- Tip designer: edges ----------
// Smooth value noise (the same seed always gives the same pattern)

function hash(x: number, y: number, seed: number) {
  let h = (x * 374761393 + y * 668265263 + seed * 2147483647) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967295;
}

function noise(x: number, y: number, seed: number) {
  const xi = Math.floor(x), yi = Math.floor(y);
  const fx = x - xi, fy = y - yi;
  const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
  const a = hash(xi, yi, seed), b = hash(xi + 1, yi, seed), c = hash(xi, yi + 1, seed), d = hash(xi + 1, yi + 1, seed);
  return a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy;
}

function fbm(x: number, y: number, seed: number) {
  return (noise(x, y, seed) * 0.55 + noise(x * 2.1, y * 2.1, seed + 11) * 0.3 + noise(x * 4.3, y * 4.3, seed + 23) * 0.15);
}

const smoothstep = (a: number, b: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};

function alphaOf(c: HTMLCanvasElement) {
  const d = c.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, c.width, c.height).data;
  const a = new Float32Array(c.width * c.height);
  for (let i = 0; i < a.length; i++) a[i] = d[i * 4 + 3] / 255;
  return a;
}

export function hasEdgeEffects(s: BrushSettings) {
  return (s.edge && s.edge !== 'smooth') || s.wetRim > 0 || s.grain > 0 || s.streaks > 0 || s.deform > 0 || dirTexture(s);
}

function edgeKey(s: BrushSettings) {
  return [s.edge, s.edgeAmount.toFixed(2), s.edgeDetail.toFixed(2), s.wetRim.toFixed(2), s.grain.toFixed(2), (s.streaks ?? 0).toFixed(2), (s.deform ?? 0).toFixed(2), dirTexture(s) ? s.texKind + (s.texture ?? 0).toFixed(2) + (s.texScale ?? 1).toFixed(2) : '', s.seed, edgeQ()].join(',');
}

// Gives any tip a new outline (ragged, torn…), a wet rim and inner grain
function shapeEdges(full: HTMLCanvasElement, s: BrushSettings, variant: number): HTMLCanvasElement {
  // small and medium brushes get their edges worked out at half size: 4× less work,
  // and the dab is drawn smaller than that anyway
  const q = edgeQ();
  const N = q === 'high' ? (s.size > 140 ? full.width : full.width / 2) : q === 'balanced' ? (s.size > 300 ? full.width : full.width / 2) : s.size > 80 ? full.width / 2 : full.width / 4;
  let base = full;
  if (N !== full.width) {
    base = canvas(N);
    base.getContext('2d')!.drawImage(full, 0, 0, N, N);
  }
  const k = N / 256; // blur sizes below are for a 256 px tip
  const seed = Math.round(s.seed) * 101 + variant * 977;
  const amt = Math.max(0, Math.min(1, s.edgeAmount));
  const freq = 2 + s.edgeDetail * 26; // wobbles across the tip
  const blurPx = s.edge === 'feathered' ? 14 : s.edge === 'torn' ? 5 : 12;
  // a blurred copy works like "how far inside the shape is this pixel"
  let field = alphaOf(s.edge === 'smooth' || s.edge === 'dry' ? base : blurred(base, blurPx * k));
  let orig = alphaOf(base);
  // deform: bend the whole tip with a smooth random warp (each version differently)
  if (s.deform > 0) {
    const warp = (src: Float32Array) => {
      const out = new Float32Array(src.length);
      const amp = s.deform * N * 0.22;
      for (let y = 0; y < N; y++)
        for (let x = 0; x < N; x++) {
          const u = x / N, v = y / N;
          const sx = Math.round(x + (fbm(u * 2.2, v * 2.2, seed + 71) - 0.5) * 2 * amp);
          const sy = Math.round(y + (fbm(u * 2.2 + 9, v * 2.2, seed + 73) - 0.5) * 2 * amp);
          out[y * N + x] = sx < 0 || sy < 0 || sx >= N || sy >= N ? 0 : src[sy * N + sx];
        }
      return out;
    };
    orig = warp(orig);
    field = warp(field);
  }
  const out = new Float32Array(N * N);
  const soft = Math.max(0.03, (1 - s.hardness) * 0.3);
  for (let y = 0; y < N; y++) {
    for (let x = 0; x < N; x++) {
      const i = y * N + x;
      const u = x / N, v = y / N;
      let a = orig[i];
      switch (s.edge) {
        case 'ragged':
          a = smoothstep(0.5 - soft, 0.5 + soft, field[i] + (fbm(u * freq, v * freq, seed) - 0.5) * amt * 1.3);
          break;
        case 'torn':
          a = smoothstep(0.5 - 0.03, 0.5 + 0.03, field[i] + (fbm(u * freq * 2, v * freq * 2, seed) - 0.5) * amt * 1.3);
          break;
        case 'feathered':
          a = smoothstep(0.5 - 0.3, 0.5 + 0.3, field[i] + (fbm(u * freq * 0.5, v * freq * 0.5, seed) - 0.5) * amt * 0.7);
          break;
        case 'bristly': {
          // hairs: the noise follows the direction around the middle
          const ang = Math.atan2(v - 0.5, u - 0.5);
          const n = fbm(ang * freq * 1.3 + 50, Math.hypot(u - 0.5, v - 0.5) * 3, seed);
          a = smoothstep(0.5 - soft, 0.5 + soft, field[i] + (n - 0.5) * amt * 1.4);
          break;
        }
        case 'dry': {
          // streaks along the stroke direction (the tip's x axis). The gaps are at the same place
          // in every dab version (brush seed only), and nearly empty, so overlapping dabs keep them
          const n = noise(v * freq * 1.6, 0.5, Math.round(s.seed) * 7 + 1) * 0.75 + noise(v * freq * 3.3, 1.5, Math.round(s.seed) * 7 + 2) * 0.25;
          const keep = smoothstep(amt * 0.6 - 0.08, amt * 0.6 + 0.08, n);
          a = orig[i] * keep * keep * (0.85 + 0.15 * noise(u * 3, v * 4, seed));
          break;
        }
      }
      out[i] = a;
    }
  }
  // paint collects at the edge (watercolor): the middle gets lighter
  if (s.wetRim > 0) {
    const tmp = canvas(N);
    put(tmp, out);
    const inner = alphaOf(blurred(tmp, 9 * k));
    for (let i = 0; i < out.length; i++) {
      const deep = smoothstep(0.55, 0.98, inner[i]);
      out[i] *= 1 - s.wetRim * 0.65 * deep;
    }
  }
  // grain: tiny holes inside the tip
  if (s.grain > 0) {
    for (let y = 0; y < N; y++)
      for (let x = 0; x < N; x++) {
        // fine, soft grain (big holes looked like a broken brush)
        const n = noise(x / (1.6 * k), y / (1.6 * k), seed + 7) * 0.5 + noise(x / (0.8 * k), y / (0.8 * k), seed + 9) * 0.5;
        out[y * N + x] *= 1 - s.grain * 0.6 * smoothstep(0.45, 0.85, n);
      }
  }
  // hair / fur textures live inside the tip, lying along the stroke: overlapping dabs line
  // their strands up into long hairs that bend with the stroke
  if (dirTexture(s)) {
    const tex = textureAlpha(s.texKind);
    // about 35 strands across the tip; gaps go deep so overlapping dabs keep the strands apart
    const sc = (256 / N) * 0.38 / Math.max(0.25, s.texScale ?? 1);
    for (let y = 0; y < N; y++)
      for (let x = 0; x < N; x++) {
        const tx = Math.floor(x * sc) & 255, ty = Math.floor(y * sc) & 255;
        const k = 1 - Math.min(1, s.texture * 1.15) * tex[ty * 256 + tx];
        out[y * N + x] *= k * k;
      }
  }
  // bristle marks: grooves across the width of the tip (its x axis). They depend on the
  // brush's seed only, not on the dab version, so they line up into long streaks.
  if (s.streaks > 0) {
    const lines = 14 + Math.round(s.edgeDetail * 26);
    const ss = Math.round(s.seed) * 13 + 3;
    for (let x = 0; x < N; x++) {
      const u = (x / N) * lines;
      const groove = smoothstep(0.25, 0.75, noise(u, 0.5, ss) * 0.7 + noise(u * 2.3, 1.5, ss + 1) * 0.3);
      const k0 = 1 - Math.min(1, s.streaks * 1.1) * (1 - groove) * (0.85 + 0.15 * noise(u, variant + 3.5, ss + 2));
      const k = k0 * k0; // deep grooves: they survive many overlapping dabs
      for (let y = 0; y < N; y++) out[y * N + x] *= k;
    }
  }
  const c = canvas(N);
  put(c, out);
  return c;
}

function put(c: HTMLCanvasElement, a: Float32Array) {
  const ctx = c.getContext('2d')!;
  const img = ctx.createImageData(c.width, c.height);
  for (let i = 0; i < a.length; i++) {
    img.data[i * 4] = img.data[i * 4 + 1] = img.data[i * 4 + 2] = 255;
    img.data[i * 4 + 3] = Math.round(Math.min(1, Math.max(0, a[i])) * 255);
  }
  ctx.putImageData(img, 0, 0);
}

// Image tips are loaded once (they come from imported brushes)
const loading = new Set<string>();

export function tipKey(s: Pick<BrushSettings, 'tip' | 'hardness' | 'tipId' | 'tipImage'>) {
  if (s.tip === 'image') return 'img:' + (s.tipId ?? (s.tipImage ? s.tipImage.length + ':' + s.tipImage.slice(-40) : 'none'));
  if (s.tip === 'round' || s.tip === 'chalk' || s.tip === 'wet') return s.tip + ':' + s.hardness.toFixed(2);
  if (s.tip === 'shape') {
    const t = s as BrushSettings;
    return ['shape', t.shapeBase, t.sides, t.starInner, s.hardness.toFixed(2)].join(':');
  }
  return s.tip;
}

// Starts loading an image tip. Returns true when the tip can be used right now.
export function ensureTip(s: BrushSettings, onReady?: () => void): boolean {
  if (s.tip !== 'image' || !s.tipImage) return true;
  const key = tipKey(s);
  if (masks.has(key)) return true;
  if (!loading.has(key)) {
    loading.add(key);
    const img = new Image();
    img.onload = () => {
      const c = canvas();
      const ctx = c.getContext('2d')!;
      // keep the tip's proportions inside the square
      const k = Math.min(DAB_SIZE / img.width, DAB_SIZE / img.height);
      const w = img.width * k, h = img.height * k;
      ctx.drawImage(img, (DAB_SIZE - w) / 2, (DAB_SIZE - h) / 2, w, h);
      // make sure it is white (only the transparency matters)
      ctx.globalCompositeOperation = 'source-in';
      ctx.fillStyle = '#fff';
      ctx.fillRect(0, 0, DAB_SIZE, DAB_SIZE);
      masks.set(key, c);
      loading.delete(key);
      onReady?.();
    };
    img.onerror = () => loading.delete(key);
    img.src = s.tipImage;
  }
  return false;
}

// The tip before edge effects
function baseMask(s: BrushSettings): HTMLCanvasElement {
  const key = tipKey(s);
  const m = masks.get(key);
  if (m) return m;
  if (s.tip === 'image') {
    ensureTip(s);
    return baseMask({ ...s, tip: 'round' }); // until the image is loaded
  }
  const made = s.tip === 'round' ? roundMask(s.hardness) : s.tip === 'shape' ? shapeMask(s) : proceduralMask(s.tip, s.hardness);
  keep(masks, key, made, 160);
  return made;
}

// Anti-aliasing off: every pixel of the tip is either full paint or none
function hardEdges(src: HTMLCanvasElement) {
  const c = canvas(src.width);
  const ctx = c.getContext('2d', { willReadFrequently: true })!;
  ctx.drawImage(src, 0, 0);
  const img = ctx.getImageData(0, 0, c.width, c.height);
  for (let i = 3; i < img.data.length; i += 4) img.data[i] = img.data[i] >= 110 ? 255 : 0;
  ctx.putImageData(img, 0, 0);
  return c;
}

// The white mask of the brush tip. `variant` picks one of the slightly different versions.
export function getTipMask(s: BrushSettings, variant = 0): HTMLCanvasElement {
  if (s.antialias === false) {
    const key = tipKey(s) + (hasEdgeEffects(s) ? '|' + edgeKey(s) + '|' + variant : '') + '|aa0';
    const m = masks.get(key);
    if (m) return m;
    const made = hardEdges(getTipMask({ ...s, antialias: true }, variant));
    keep(masks, key, made, 160);
    return made;
  }
  const base = baseMask(s);
  if (!hasEdgeEffects(s)) return base;
  const v = Math.max(0, Math.min(7, variant));
  const key = tipKey(s) + '|' + edgeKey(s) + '|' + v;
  const m = masks.get(key);
  if (m) return m;
  const made = shapeEdges(base, s, v);
  // an image tip that isn't loaded yet: don't keep the stand-in
  if (!(s.tip === 'image' && !masks.has(tipKey(s)))) {
    keep(masks, key, made, 160);
  }
  return made;
}

// Makes the tip versions ahead of time, a little at a time while the computer is idle,
// so drawing doesn't stutter when a new version is needed the first time
let warmJob = 0;
export function warmTip(s: BrushSettings) {
  const job = ++warmJob;
  if (!hasEdgeEffects(s)) return;
  const n = Math.max(1, Math.min(maxVariants(), Math.round(s.variants)));
  const idle: (cb: () => void) => void = (cb) =>
    'requestIdleCallback' in window ? (window as unknown as { requestIdleCallback: (f: () => void, o: object) => void }).requestIdleCallback(cb, { timeout: 300 }) : setTimeout(cb, 16);
  let v = 0;
  const step = () => {
    if (job !== warmJob || v >= n) return;
    getTipMask(s, v++);
    idle(step);
  };
  idle(step);
}

// Which version of the tip the next dab uses
export function pickVariant(s: BrushSettings) {
  return s.variants > 1 && hasEdgeEffects(s) ? Math.floor(Math.random() * Math.min(maxVariants(), s.variants)) : 0;
}

// The tip in a color, ready to stamp. `s` may also be a number (hardness of a round tip).
export function getDab(color: string, s: BrushSettings | number, variant = 0): HTMLCanvasElement {
  const settings: BrushSettings = typeof s === 'number' ? { ...BASE, hardness: s } : s;
  const key = color + '|' + tipKey(settings) + (hasEdgeEffects(settings) ? '|' + edgeKey(settings) + '|' + variant : '') + (settings.antialias === false ? '|aa0' : '');
  const cached = colored.get(key);
  if (cached) return cached;
  const mask = getTipMask(settings, variant);
  const c = canvas(mask.width);
  const ctx = c.getContext('2d')!;
  ctx.drawImage(mask, 0, 0);
  ctx.globalCompositeOperation = 'source-in';
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, c.width, c.height);
  // an image tip may not be ready yet: don't keep the stand-in
  if (!(settings.tip === 'image' && !masks.has(tipKey(settings)))) keep(colored, key, c, 400);
  return c;
}

// A slightly different color for brushes with color jitter (a few steps, so tips stay cached)
export function jitterColor(hex: string, amount: number): string {
  if (amount <= 0) return hex;
  const n = parseInt(hex.replace('#', ''), 16) || 0;
  let r = (n >> 16) & 255, g = (n >> 8) & 255, bl = n & 255;
  const step = Math.round((Math.random() - 0.5) * 6) / 6; // -0.5..0.5 in 7 steps
  const light = step * amount * 90;
  const shift = Math.round((Math.random() - 0.5) * 4) / 4 * amount * 40; // a little toward yellow/blue
  r += light + shift;
  g += light + shift * 0.5;
  bl += light - shift;
  const c = (v: number) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, '0');
  return '#' + c(r) + c(g) + c(bl);
}

// Where and how one dab lands: size, angle, sideways scatter, paint amount.
// `dir` is the stroke direction in radians.
export function dabShape(s: BrushSettings, r: number, dir: number) {
  let size = r;
  if (s.sizeJitter > 0) size *= 1 - Math.random() * s.sizeJitter;
  let a = (s.angle * Math.PI) / 180;
  if (followsStroke(s)) a += dir;
  if (s.angleJitter > 0) a += (Math.random() - 0.5) * 2 * Math.PI * s.angleJitter;
  let ox = 0, oy = 0;
  if (s.scatter > 0) {
    const d = (Math.random() - 0.5) * 2 * s.scatter * r * 2;
    ox = -Math.sin(dir) * d;
    oy = Math.cos(dir) * d;
  }
  const alpha = s.opacityJitter > 0 ? 1 - Math.random() * s.opacityJitter : 1;
  return { r: size, a, ox, oy, alpha };
}

// Particles: one dab becomes several small, separate dabs spread inside the brush circle.
// Returns offsets (× radius), sizes (× radius) and turns.
export function particleSpread(s: BrushSettings): { ox: number; oy: number; r: number; a: number }[] {
  const n = Math.max(1, Math.round(s.particles));
  const out = [];
  for (let i = 0; i < n; i++) {
    const ang = Math.random() * Math.PI * 2, d = Math.sqrt(Math.random());
    out.push({ ox: Math.cos(ang) * d, oy: Math.sin(ang) * d, r: Math.max(0.03, s.particleSize * (0.45 + Math.random())), a: Math.random() * Math.PI * 2 });
  }
  return out;
}

// Whether dabs need turning/squashing (otherwise the faster plain stamp is used)
export function isShaped(s: BrushSettings) {
  return s.particles > 0 || s.tip !== 'round' || s.roundness < 0.999 || s.angle % 360 !== 0 || s.scatter > 0 || s.sizeJitter > 0 || s.angleJitter > 0 || s.followDirection || hasEdgeEffects(s);
}

// Stamps one dab: turned by `a`, squashed by roundness
export function stampDab(ctx: CanvasRenderingContext2D, img: CanvasImageSource, x: number, y: number, r: number, a: number, roundness: number) {
  if (a === 0 && roundness >= 0.999) {
    ctx.drawImage(img, x - r, y - r, r * 2, r * 2);
    return;
  }
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(a);
  ctx.scale(1, Math.max(0.05, roundness));
  ctx.drawImage(img, -r, -r, r * 2, r * 2);
  ctx.restore();
}

// ---------- Brush preview ----------
// A short S-curve painted with the brush, for the brush list.

export function renderBrushPreview(c: HTMLCanvasElement, s: BrushSettings, color = '#1d1d24') {
  const ctx = c.getContext('2d')!;
  ctx.clearRect(0, 0, c.width, c.height);
  const W = c.width, H = c.height;
  const size = Math.min(H * 0.5, Math.max(3, s.size > 60 ? H * 0.45 : Math.max(s.size * (H / 70), 4)));
  const pts: { x: number; y: number; p: number }[] = [];
  for (let i = 0; i <= 60; i++) {
    const t = i / 60;
    pts.push({ x: W * 0.08 + t * W * 0.84, y: H / 2 + Math.sin(t * Math.PI * 2) * H * 0.22, p: Math.sin(t * Math.PI) * 0.9 + 0.1 });
  }
  const tmp = document.createElement('canvas');
  tmp.width = W;
  tmp.height = H;
  const t = tmp.getContext('2d')!;
  const step = Math.max(0.6, size * Math.max(0.02, s.spacing));
  let carry = 0;
  for (let i = 1; i < pts.length; i++) {
    const a = pts[i - 1], q = pts[i];
    const len = Math.hypot(q.x - a.x, q.y - a.y);
    const dir = Math.atan2(q.y - a.y, q.x - a.x);
    if (carry >= len) {
      carry -= len; // the next dab is further along than this short piece
      continue;
    }
    for (let d = carry; d < len; d += step) {
      const u = d / len;
      const p = a.p + (q.p - a.p) * u;
      const f = s.sizePressure ? s.minSize + (1 - s.minSize) * p : 1;
      const shape = dabShape(s, (size * f) / 2, dir);
      const col = s.colorJitter > 0 ? jitterColor(color, s.colorJitter) : color;
      t.globalAlpha = Math.min(1, s.flow * shape.alpha * (s.opacityPressure ? 0.25 + 0.75 * p : 1));
      const px = a.x + (q.x - a.x) * u + shape.ox, py = a.y + (q.y - a.y) * u + shape.oy;
      if (s.particles > 0) {
        for (const pt of particleSpread(s)) stampDab(t, getDab(col, s, pickVariant(s)), px + pt.ox * shape.r, py + pt.oy * shape.r, Math.max(0.4, shape.r * pt.r), pt.a, s.roundness);
      } else stampDab(t, getDab(col, s, pickVariant(s)), px, py, Math.max(0.4, shape.r), shape.a, s.roundness);
      carry = d + step - len;
    }
  }
  // the whole-stroke look (outline only, ragged outline, rim, deform), at preview size
  if (hasStrokeEdges(s)) applyStrokeEdges(t, 0, 0, W, H, { ...s, size });
  const colors = s.texColor > 0 ? getTextureColors(s) : null;
  if (colors) {
    const pat = t.createPattern(colors, 'repeat');
    if (pat) {
      pat.setTransform(new DOMMatrix().scale(Math.max(0.25, s.texScale ?? 1) * 0.5));
      t.globalAlpha = Math.min(1, s.texColor);
      t.globalCompositeOperation = 'source-atop';
      t.fillStyle = pat;
      t.fillRect(0, 0, W, H);
      t.globalCompositeOperation = 'source-over';
      t.globalAlpha = 1;
    }
  }
  if (s.texture > 0 && !dirTexture(s)) {
    t.globalAlpha = s.texture;
    t.globalCompositeOperation = 'destination-out';
    const pat = t.createPattern(getTexture(s), 'repeat');
    if (pat) {
      pat.setTransform(new DOMMatrix().scale(Math.max(0.25, s.texScale ?? 1) * 0.5));
      t.fillStyle = pat;
      t.fillRect(0, 0, W, H);
    }
    t.globalCompositeOperation = 'source-over';
  }
  ctx.globalAlpha = s.mix >= 0.9 ? 0.35 : s.opacity; // blenders add no color: show them faint
  ctx.drawImage(tmp, 0, 0);
  ctx.globalAlpha = 1;
}

// ---------- Paper texture ----------
// A tileable grain pattern. Strokes are "worn away" by it, and because the pattern is
// fixed to the canvas, the grain stays in place no matter where or how you draw.

let paper: HTMLCanvasElement | null = null;

export function getPaper(): HTMLCanvasElement {
  if (paper) return paper;
  const SIZE = 256;

  // 1. One tile of noise: coarse blocks (paper bumps) + fine per-pixel grain
  const tile = document.createElement('canvas');
  tile.width = SIZE;
  tile.height = SIZE;
  const tctx = tile.getContext('2d')!;
  tctx.imageSmoothingEnabled = false;
  const noise = (cells: number, alpha: number) => {
    const small = document.createElement('canvas');
    small.width = cells;
    small.height = cells;
    const sctx = small.getContext('2d')!;
    const img = sctx.createImageData(cells, cells);
    for (let i = 0; i < img.data.length; i += 4) {
      img.data[i + 3] = Math.pow(Math.random(), 3) * 255; // black with random transparency
    }
    sctx.putImageData(img, 0, 0);
    tctx.globalAlpha = alpha;
    tctx.drawImage(small, 0, 0, SIZE, SIZE);
  };
  noise(64, 0.65);
  noise(256, 0.8);

  // 2. Blur a 3x3 copy of the tile and keep the middle one, so the edges wrap seamlessly
  const big = document.createElement('canvas');
  big.width = SIZE * 3;
  big.height = SIZE * 3;
  const bctx = big.getContext('2d')!;
  for (let y = 0; y < 3; y++) for (let x = 0; x < 3; x++) bctx.drawImage(tile, x * SIZE, y * SIZE);

  const out = document.createElement('canvas');
  out.width = SIZE;
  out.height = SIZE;
  const octx = out.getContext('2d')!;
  octx.filter = 'blur(1.2px)';
  octx.drawImage(big, -SIZE, -SIZE);
  octx.filter = 'none';

  paper = out;
  return out;
}

// ---------- Brush ideas ----------

const NUMERIC: (keyof BrushSettings)[] = [
  'size', 'opacity', 'flow', 'hardness', 'spacing', 'minSize', 'texture', 'stabilize', 'taper', 'mix', 'roundness', 'angle',
  'scatter', 'sizeJitter', 'angleJitter', 'opacityJitter', 'colorJitter', 'sides', 'starInner', 'edgeAmount', 'edgeDetail', 'wetRim', 'grain', 'variants', 'streaks', 'deform', 'hollow', 'outlineWidth', 'particles', 'particleSize', 'texScale',
];

// Brush remix: a mix of two brushes. t = 0 is all of A, 1 is all of B.
export function remixBrushes(a: BrushSettings, b: BrushSettings, t: number): BrushSettings {
  const out = { ...(t < 0.5 ? a : b) } as Record<string, unknown>;
  for (const k of NUMERIC) {
    const x = Number(a[k]), y = Number(b[k]);
    if (isFinite(x) && isFinite(y)) out[k] = Math.round((x + (y - x) * t) * 1000) / 1000;
  }
  out.variants = Math.max(1, Math.round(Number(out.variants)));
  out.sides = Math.max(3, Math.round(Number(out.sides)));
  return out as BrushSettings;
}

// A surprise brush: random but sensible. Each kind starts from a real medium and then
// picks a tip form (round, flat, filbert, chisel, fan, bristles, chalk…), an edge and a
// character, so two random brushes of the same kind still look clearly different.
export type RandomKind = 'paint' | 'watercolor' | 'oil' | 'gouache' | 'dry' | 'ink' | 'pen' | 'pencil' | 'pastel' | 'marker' | 'airbrush' | 'texture' | 'rough' | 'particles' | 'fx' | 'outline' | 'fuzzy';

export const RANDOM_KINDS: { id: RandomKind; label: string }[] = [
  { id: 'paint', label: 'Any paint' },
  { id: 'watercolor', label: 'Watercolor' },
  { id: 'oil', label: 'Oil / acrylic' },
  { id: 'gouache', label: 'Gouache' },
  { id: 'dry', label: 'Dry brush' },
  { id: 'ink', label: 'Ink brush' },
  { id: 'pen', label: 'Pen' },
  { id: 'pencil', label: 'Pencil / charcoal' },
  { id: 'pastel', label: 'Pastel / chalk' },
  { id: 'marker', label: 'Marker' },
  { id: 'airbrush', label: 'Airbrush' },
  { id: 'texture', label: 'Texture' },
  { id: 'rough', label: 'Rough / deformed' },
  { id: 'outline', label: 'Outline / hollow' },
  { id: 'fuzzy', label: 'Fuzzy bristle' },
  { id: 'particles', label: 'Particles' },
  { id: 'fx', label: 'Effects' },
];

type Form = 'round' | 'flat' | 'filbert' | 'chisel' | 'fan' | 'bristle' | 'chalk' | 'wet' | 'cloud' | 'nib' | 'triangle' | 'star';

// Randomizing must not change how thick your line is: the size, the pressure → size setting
// and the smallest size stay yours, and random size jumps stay small.
export function keepSizeFeel<T extends Partial<BrushSettings>>(r: T, cur: Pick<BrushSettings, 'size' | 'sizePressure' | 'minSize'>): T {
  return { ...r, size: cur.size, sizePressure: cur.sizePressure, minSize: cur.minSize, sizeJitter: Math.min(r.sizeJitter ?? 0, 0.12) };
}

export function surpriseBrush(kind: RandomKind = 'paint'): BrushSettings {
  const R = Math.random;
  const pick = <T,>(xs: T[]) => xs[Math.floor(R() * xs.length)];
  const rr = (a: number, b: number, d = 2) => Math.round((a + R() * (b - a)) * 10 ** d) / 10 ** d;
  if (kind === 'paint') kind = pick<RandomKind>(['watercolor', 'watercolor', 'oil', 'oil', 'gouache', 'gouache', 'dry', 'pastel', 'ink', 'outline', 'fuzzy']);
  const r = surpriseBase(kind);
  // extra twists now and then, so painting brushes don't all come out alike
  if (r.hollow === 0 && ['gouache', 'oil', 'dry', 'marker', 'pastel', 'ink', 'pen', 'pencil'].includes(kind) && R() < 0.16)
    Object.assign(r, { hollow: rr(0.55, 1), outlineWidth: rr(0.06, 0.3), opacity: Math.max(r.opacity, 0.8) });
  if (r.deform === 0 && ['gouache', 'oil', 'dry', 'pastel', 'watercolor'].includes(kind) && R() < 0.22) r.deform = rr(0.1, 0.4);
  if (['gouache', 'oil', 'dry', 'pastel'].includes(kind) && r.edge !== 'bristly' && R() < 0.15)
    Object.assign(r, { edge: 'bristly' as EdgeStyle, edgeAmount: rr(0.5, 1), edgeDetail: rr(0.5, 1), variants: Math.max(r.variants, 4) });
  return r;
}

function surpriseBase(kind: RandomKind): BrushSettings {
  const R = Math.random;
  const pick = <T,>(xs: T[]) => xs[Math.floor(R() * xs.length)];
  const rr = (a: number, b: number, d = 2) => Math.round((a + R() * (b - a)) * 10 ** d) / 10 ** d;

  // the tip itself: not only round
  const form = (f: Form): Partial<BrushSettings> => {
    switch (f) {
      case 'flat': // flat brush: wide, thin, turns with the stroke
        return { tip: 'shape', shapeBase: pick<ShapeBase>(['square', 'rounded']), roundness: rr(0.12, 0.4), followDirection: true, angle: 90 };
      case 'filbert': // rounded flat brush
        return { tip: 'shape', shapeBase: pick<ShapeBase>(['drop', 'rounded', 'circle']), roundness: rr(0.3, 0.6), followDirection: true, angle: 90 };
      case 'chisel': // fixed-angle flat tip: thick and thin by direction
        return { tip: 'shape', shapeBase: pick<ShapeBase>(['square', 'diamond', 'rounded']), roundness: rr(0.12, 0.35), followDirection: false, angle: Math.round(rr(20, 70, 0)) * (R() < 0.5 ? -1 : 1) };
      case 'fan':
        return { tip: 'bristle', roundness: rr(0.12, 0.25), followDirection: true, angle: 90, spacing: rr(0.03, 0.06) };
      case 'bristle':
        return { tip: 'bristle', roundness: rr(0.4, 0.9), followDirection: true, angle: 0 };
      case 'chalk':
        return { tip: 'chalk', roundness: rr(0.45, 0.9), followDirection: R() < 0.5, angleJitter: R() < 0.5 ? 1 : 0 };
      case 'wet':
        return { tip: 'wet', roundness: rr(0.7, 1), angleJitter: 1 };
      case 'cloud':
        return { tip: 'cloud', roundness: rr(0.6, 1), angleJitter: 1 };
      case 'nib': // calligraphy / brush-pen nib
        return { tip: 'shape', shapeBase: pick<ShapeBase>(['drop', 'diamond', 'circle']), roundness: rr(0.15, 0.45), angle: Math.round(rr(25, 65, 0)) };
      case 'triangle':
        return { tip: 'shape', shapeBase: 'triangle', roundness: rr(0.4, 0.9), followDirection: true, angle: 90 };
      case 'star':
        return { tip: 'shape', shapeBase: pick<ShapeBase>(['star', 'polygon']), sides: 4 + Math.floor(R() * 5), starInner: rr(0.25, 0.6), angleJitter: 1 };
      default:
        return { tip: 'round', roundness: R() < 0.35 ? rr(0.45, 0.85) : 1, angleJitter: R() < 0.5 ? 1 : 0 };
    }
  };
  // the outline of the tip
  const edged = (styles: EdgeStyle[], strong = 1): Partial<BrushSettings> => {
    const edge = pick(styles);
    return edge === 'smooth' ? { edge } : { edge, edgeAmount: Math.min(1, rr(0.3, 0.85) * strong), edgeDetail: rr(0.1, 0.95), variants: 3 + Math.floor(R() * 6) };
  };
  // how the line starts, ends and reacts to the pen
  const character = (): Partial<BrushSettings> => ({
    ends: pick<BrushSettings['ends']>(['round', 'taper', 'taper', 'flat']),
    taper: rr(0.2, 0.8),
    sizePressure: R() < 0.85,
    minSize: rr(0.15, 0.85),
    opacityPressure: R() < 0.5,
    stabilize: rr(0, 0.35),
    seed: Math.floor(R() * 9999) + 1,
    mixRefer: 'layer', // paint mixes with paint, not with the white paper
  });
  const mk = (...parts: Partial<BrushSettings>[]): BrushSettings => Object.assign({ ...BASE }, character(), ...parts);

  switch (kind) {
    case 'watercolor': {
      const wc = mk(
        form(pick<Form>(['round', 'round', 'wet', 'cloud', 'filbert', 'flat'])),
        edged(['ragged', 'ragged', 'ragged', 'feathered'], 1.15),
        {
          size: Math.round(rr(30, 140, 0)),
          hardness: rr(0.3, 0.85),
          wetRim: rr(0.5, 0.95), // the watercolor edge: paint collects at the rim
          grain: R() < 0.35 ? rr(0, 0.15) : 0,
          sizeJitter: rr(0, 0.25),
          opacity: rr(0.6, 0.9),
          flow: rr(0.25, 0.6),
          spacing: rr(0.05, 0.12),
          texture: rr(0.1, 0.45),
          colorJitter: R() < 0.4 ? rr(0, 0.3) : 0,
          mix: 0,
        },
      );
      // one of several watercolor characters, so random watercolors differ a lot
      const style = pick(['plain', 'wash', 'dark rim', 'bloom', 'granulating', 'dry', 'liner', 'bleed', 'mottled', 'wet mix', 'flat']);
      const styles: Record<string, Partial<BrushSettings>> = {
        plain: {},
        wash: { hardness: rr(0.1, 0.35), flow: rr(0.12, 0.3), wetRim: rr(0.2, 0.45), edge: 'feathered', edgeAmount: rr(0.5, 0.9) },
        'dark rim': { wetRim: 1, flow: rr(0.25, 0.4), hardness: rr(0.6, 0.85), edge: 'ragged', edgeAmount: rr(0.3, 0.6) },
        bloom: { tip: 'cloud', angleJitter: 1, deform: rr(0.3, 0.6), wetRim: rr(0.7, 1), variants: 6, edge: 'ragged', edgeAmount: rr(0.6, 1) },
        granulating: { texture: rr(0.5, 0.8), texKind: pick(['watercolor', 'watercolor', 'paper', 'canvas', 'stone']), grain: rr(0.15, 0.35), wetRim: rr(0.3, 0.6) },
        dry: { edge: 'dry', edgeAmount: rr(0.5, 0.9), streaks: rr(0.3, 0.6), followDirection: true, flow: rr(0.5, 0.8), wetRim: rr(0.2, 0.5), grain: rr(0.05, 0.2) },
        liner: { tip: 'round', roundness: 1, hardness: rr(0.6, 0.85), ends: 'taper', taper: rr(0.5, 0.9), flow: rr(0.4, 0.7), wetRim: rr(0.4, 0.8) },
        bleed: { edge: 'feathered', edgeAmount: rr(0.7, 1), edgeDetail: rr(0.1, 0.35), deform: rr(0.35, 0.7), hardness: rr(0.3, 0.5), variants: 6 },
        mottled: { tip: 'cloud', texture: rr(0.3, 0.55), deform: rr(0.2, 0.45), opacityJitter: rr(0.2, 0.45), angleJitter: 1, variants: 6 },
        'wet mix': { mix: rr(0.4, 0.75), mixRefer: 'layer', flow: rr(0.3, 0.5), wetRim: rr(0.3, 0.6) },
        flat: { tip: 'shape', shapeBase: 'rounded', roundness: rr(0.15, 0.35), followDirection: true, angle: 90, edge: 'ragged', edgeAmount: rr(0.4, 0.8), wetRim: rr(0.5, 0.9) },
      };
      return Object.assign(wc, styles[style]);
    }
    case 'outline': {
      // outlined / hollow strokes: the middle empties, double lines and rims stay
      const base = surpriseBase(pick<RandomKind>(['gouache', 'marker', 'pen', 'oil', 'ink', 'dry', 'pastel']));
      return { ...base, hollow: rr(0.65, 1), outlineWidth: pick([rr(0.05, 0.12), rr(0.12, 0.25), rr(0.25, 0.4)]), wetRim: 0, opacity: Math.max(base.opacity, 0.85), spacing: Math.min(base.spacing, 0.1), scatter: 0, particles: 0 };
    }
    case 'fuzzy':
      // dense, hairy bristle masses with fuzzy, fibrous edges
      return mk(form(pick<Form>(['bristle', 'bristle', 'fan', 'chalk', 'round', 'filbert'])), {
        size: Math.round(rr(30, 110, 0)),
        edge: 'bristly',
        edgeAmount: rr(0.6, 1),
        edgeDetail: rr(0.55, 1),
        variants: 6,
        hardness: rr(0.7, 1),
        deform: rr(0.1, 0.45),
        streaks: rr(0.2, 0.6),
        grain: R() < 0.4 ? rr(0.05, 0.2) : 0,
        flow: rr(0.65, 1),
        opacity: rr(0.75, 1),
        spacing: rr(0.03, 0.07),
        angleJitter: R() < 0.5 ? 1 : 0,
        mix: rr(0, 0.35),
        texture: R() < 0.4 ? rr(0.1, 0.35) : 0,
      });
    case 'oil':
      return mk(form(pick<Form>(['flat', 'flat', 'filbert', 'filbert', 'fan', 'bristle', 'chisel'])), edged(['bristly', 'bristly', 'ragged', 'smooth'], 0.6), {
        size: Math.round(rr(18, 90, 0)),
        hardness: rr(0.6, 1),
        streaks: rr(0.35, 0.85), // oil paint shows the bristles
        grain: R() < 0.3 ? rr(0, 0.15) : 0,
        flow: rr(0.7, 1),
        spacing: rr(0.03, 0.08),
        mix: rr(0.1, 0.45),
        texture: R() < 0.5 ? rr(0.05, 0.25) : 0,
        colorJitter: R() < 0.3 ? rr(0, 0.15) : 0,
      });
    case 'gouache':
      return mk(form(pick<Form>(['round', 'flat', 'filbert', 'chalk', 'chisel', 'triangle'])), edged(['ragged', 'smooth', 'dry', 'torn'], 0.6), {
        size: Math.round(rr(16, 75, 0)),
        hardness: rr(0.6, 0.95),
        streaks: R() < 0.4 ? rr(0.1, 0.35) : 0,
        angleJitter: R() < 0.5 ? 1 : 0,
        flow: rr(0.75, 1),
        spacing: rr(0.04, 0.09),
        mix: rr(0, 0.3),
        texture: rr(0.05, 0.3),
      });
    case 'dry':
      return mk(form(pick<Form>(['flat', 'bristle', 'fan', 'chalk', 'chisel', 'round'])), edged(['dry', 'dry', 'bristly', 'torn']), {
        size: Math.round(rr(22, 100, 0)),
        hardness: rr(0.7, 1),
        streaks: rr(0.3, 0.7),
        grain: rr(0.0, 0.2),
        flow: rr(0.65, 1),
        spacing: rr(0.03, 0.07),
        mix: rr(0, 0.25),
        texture: rr(0.1, 0.4),
      });
    case 'ink':
      return mk(form(pick<Form>(['round', 'nib', 'nib', 'filbert', 'chisel', 'bristle'])), edged(['smooth', 'smooth', 'ragged', 'dry', 'bristly'], 0.5), {
        size: Math.round(rr(6, 40, 0)),
        hardness: rr(0.85, 1),
        spacing: rr(0.02, 0.05),
        minSize: rr(0.03, 0.35),
        ends: 'taper',
        taper: rr(0.3, 0.9),
        stabilize: rr(0.25, 0.6),
        texture: R() < 0.3 ? rr(0, 0.15) : 0,
      });
    case 'pen':
      return mk(form(pick<Form>(['round', 'round', 'nib', 'chisel'])), edged(['smooth', 'smooth', 'smooth', 'ragged'], 0.25), {
        size: rr(1.5, 14, 1),
        hardness: rr(0.85, 1),
        spacing: rr(0.02, 0.05),
        minSize: pick([rr(0.05, 0.3), rr(0.5, 0.9), 1]),
        sizePressure: R() < 0.8,
        ends: pick<BrushSettings['ends']>(['taper', 'taper', 'round']),
        stabilize: rr(0.25, 0.65),
        texture: R() < 0.35 ? rr(0.05, 0.25) : 0,
        opacity: rr(0.85, 1),
      });
    case 'pencil':
      return mk(form(pick<Form>(['round', 'chalk', 'chalk', 'chisel', 'flat'])), edged(['torn', 'smooth', 'ragged', 'dry']), {
        size: Math.round(rr(4, 30, 0)),
        hardness: rr(0.5, 0.85),
        grain: rr(0.05, 0.2),
        flow: rr(0.5, 1),
        spacing: rr(0.05, 0.1),
        opacityPressure: true,
        texture: rr(0.2, 0.5),
        opacity: rr(0.85, 1),
        stabilize: rr(0, 0.2),
      });
    case 'pastel':
      return mk(form(pick<Form>(['chalk', 'chalk', 'round', 'cloud', 'flat', 'chisel'])), edged(['torn', 'ragged', 'feathered', 'dry']), {
        size: Math.round(rr(16, 70, 0)),
        hardness: rr(0.45, 0.85),
        grain: rr(0.05, 0.2),
        angleJitter: 1,
        spacing: rr(0.06, 0.12),
        mix: rr(0, 0.25),
        opacityPressure: true,
        texture: rr(0.3, 0.65),
        opacity: rr(0.75, 1),
      });
    case 'marker':
      return mk(form(pick<Form>(['round', 'chisel', 'chisel', 'flat'])), edged(['smooth', 'smooth', 'ragged'], 0.3), {
        size: Math.round(rr(8, 50, 0)),
        hardness: rr(0.7, 0.95),
        opacity: rr(0.35, 0.75),
        flow: rr(0.6, 1),
        spacing: rr(0.03, 0.07),
        sizePressure: R() < 0.3,
        minSize: rr(0.7, 1),
        ends: pick<BrushSettings['ends']>(['flat', 'round']),
        mix: R() < 0.3 ? rr(0.05, 0.2) : 0,
        stabilize: rr(0.2, 0.5),
      });
    case 'airbrush':
      return mk(form(pick<Form>(['round', 'round', 'cloud', 'chalk'])), edged(['smooth', 'smooth', 'feathered'], 0.6), {
        size: Math.round(rr(40, 220, 0)),
        hardness: rr(0, 0.45),
        flow: rr(0.04, 0.3),
        spacing: rr(0.06, 0.15),
        sizePressure: R() < 0.4,
        opacityPressure: true,
        minSize: rr(0.5, 1),
        scatter: R() < 0.3 ? rr(0.1, 0.5) : 0,
        sizeJitter: R() < 0.3 ? rr(0, 0.4) : 0,
        texture: R() < 0.3 ? rr(0.05, 0.3) : 0,
        ends: 'round',
        stabilize: 0,
      });
    case 'texture': {
      // a painting brush that paints with a surface texture
      const base = surpriseBrush(pick<RandomKind>(['gouache', 'oil', 'airbrush', 'pastel', 'watercolor', 'marker']));
      return { ...base, texKind: pick(TEXTURES.filter((t) => t.id !== 'custom' && t.id !== 'paper')).id, texture: rr(0.45, 0.85), texScale: rr(0.6, 1.8), mix: Math.min(base.mix, 0.2) };
    }
    case 'rough':
      return mk(form(pick<Form>(['round', 'chalk', 'flat', 'bristle', 'triangle'])), edged(['torn', 'ragged', 'dry', 'bristly'], 1.2), {
        tip: pick<TipShape>(['round', 'chalk', 'grunge', 'bristle', 'shape']),
        size: Math.round(rr(25, 90, 0)),
        hardness: rr(0.7, 1),
        deform: rr(0.35, 1),
        variants: 4 + Math.floor(R() * 5),
        angleJitter: R() < 0.6 ? 1 : rr(0, 0.3),
        sizeJitter: rr(0, 0.5),
        spacing: rr(0.05, 0.6),
        scatter: rr(0, 0.6),
        grain: rr(0, 0.15),
        texture: R() < 0.4 ? rr(0.2, 0.5) : 0,
      });
    case 'particles':
      return mk(edged(['smooth', 'smooth', 'ragged']), {
        tip: pick<TipShape>(['round', 'round', 'speckle', 'star', 'shape', 'chalk']),
        shapeBase: pick<ShapeBase>(['circle', 'square', 'triangle', 'star', 'diamond', 'heart']),
        particles: 4 + Math.floor(R() * 24),
        particleSize: rr(0.04, 0.3),
        size: Math.round(rr(30, 120, 0)),
        hardness: rr(0.5, 1),
        spacing: rr(0.2, 1),
        scatter: rr(0, 0.8),
        sizeJitter: rr(0, 0.6),
        opacityJitter: rr(0.1, 0.6),
        colorJitter: R() < 0.4 ? rr(0.1, 0.5) : 0,
        sizePressure: R() < 0.4,
        minSize: rr(0.5, 1),
        stabilize: 0,
      });
    default:
      // effects: scattered stamps
      return mk(form(pick<Form>(['star', 'triangle', 'chalk', 'cloud', 'round', 'flat'])), edged(['smooth', 'smooth', 'ragged', 'torn']), {
        tip: pick<TipShape>(['shape', 'shape', 'splatter', 'cloud', 'star', 'leaf', 'grass']),
        shapeBase: pick<ShapeBase>(['circle', 'square', 'triangle', 'polygon', 'star', 'diamond', 'heart', 'drop', 'crescent', 'rounded']),
        size: Math.round(rr(15, 70, 0)),
        hardness: rr(0.5, 1),
        spacing: rr(0.3, 1.3),
        scatter: rr(0.3, 1.8),
        sizeJitter: rr(0.2, 0.8),
        angleJitter: rr(0, 1),
        opacityJitter: rr(0, 0.5),
        colorJitter: rr(0, 0.6),
        flow: rr(0.6, 1),
      });
  }
}

// ---------- Stroke edges (wet media) ----------
// Per-dab edges disappear when dabs overlap closely, so wet brushes also get their edges
// on the whole stroke when the pen lifts, like paint drying: a ragged outline that is
// fixed to the canvas, and paint collecting at the rim with a lighter middle.

export function hasStrokeEdges(s: BrushSettings) {
  if (s.hollow > 0) return true; // outline only works on any stroke (spaced dabs become rings)
  // spaced-out or scattered dabs already show their own edges
  if (s.spacing > 0.2 || s.scatter > 0.5 || s.particles > 0) return false;
  return (
    (brushPerf.strokeEdges && s.wetRim > 0) ||
    s.edge === 'ragged' ||
    s.edge === 'torn' ||
    s.edge === 'feathered' ||
    s.edge === 'bristly' ||
    s.grain > 0 ||
    s.deform > 0 ||
    s.hollow > 0
  );
}

// how far a deformed stroke can move outside where it was drawn
export function strokeEdgePad(s: BrushSettings) {
  return hasStrokeEdges(s) ? Math.ceil(Math.max(4, s.size) * (0.08 + (s.deform || 0) * 0.35)) + 2 : 0;
}

// Per pixel, how much of the stroke stays (0..1): the outline (ragged, torn, feathered,
// bristly), the watercolor rim and grain. `cov` = where the stroke has paint (0..1).
function edgeFactors(cov: Float32Array, w: number, h: number, x: number, y: number, s: BrushSettings): Float32Array {
  const f = new Float32Array(w * h).fill(1);
  const cover = document.createElement('canvas');
  cover.width = w;
  cover.height = h;
  const cimg = new ImageData(w, h);
  for (let i = 0; i < cov.length; i++) cimg.data[i * 4 + 3] = Math.min(255, cov[i] * 3 * 255);
  cover.getContext('2d')!.putImageData(cimg, 0, 0);
  const blurAlpha = (r: number) => {
    const c = document.createElement('canvas');
    c.width = w;
    c.height = h;
    const t = c.getContext('2d', { willReadFrequently: true })!;
    t.filter = `blur(${r}px)`;
    t.drawImage(cover, 0, 0);
    const b = t.getImageData(0, 0, w, h).data;
    const out = new Float32Array(w * h);
    for (let i = 0; i < out.length; i++) out[i] = b[i * 4 + 3] / 255;
    return out;
  };
  const size = Math.max(4, s.size);
  const edgy = s.edge === 'ragged' || s.edge === 'torn' || s.edge === 'feathered' || s.edge === 'bristly';
  const near = edgy ? blurAlpha(Math.max(1.5, size * (s.edge === 'feathered' ? 0.2 : s.edge === 'bristly' ? 0.08 : 0.12))) : null;
  const deep = brushPerf.strokeEdges && s.wetRim > 0 ? blurAlpha(Math.max(2, size * 0.12)) : null;
  const amt = Math.max(0, Math.min(1, s.edgeAmount));
  const scale = size * (0.9 - Math.max(0, Math.min(1, s.edgeDetail)) * 0.75); // px per noise cell
  const seed = Math.round(s.seed) * 31 + 5;
  const soft = s.edge === 'torn' || s.edge === 'bristly' ? 0.05 : s.edge === 'feathered' ? 0.3 : 0.12;
  const grain = Math.max(0, Math.min(1, s.grain));
  // outline only: how far inside the stroke each pixel is
  const hollow = Math.max(0, Math.min(1, s.hollow || 0));
  const inner = hollow > 0 ? blurAlpha(Math.max(1, size * Math.max(0.05, Math.min(0.6, s.outlineWidth || 0.18)) * 0.55)) : null;
  for (let j = 0, p = 0; j < h; j++) {
    for (let i = 0; i < w; i++, p++) {
      if (cov[p] <= 0) continue;
      let k = 1;
      if (near) {
        if (near[p] <= 0.5 - soft - amt * 1.3) k = 0;
        else if (near[p] < 0.5 + soft + amt * 1.3) {
          // big wobbles plus a fine fringe, both fixed to the canvas; bristly = fine hairs
          const fine = s.edge === 'bristly' ? 0.07 : 0.18;
          const n =
            (fbm((x + i) / scale, (y + j) / scale, seed) - 0.5) * (s.edge === 'bristly' ? 0.3 : 0.75) +
            (noise((x + i) / (scale * fine), (y + j) / (scale * fine), seed + 3) - 0.5) * (s.edge === 'bristly' ? 1.1 : 0.45);
          k = smoothstep(0.5 - soft, 0.5 + soft, near[p] + n * amt * (s.edge === 'torn' || s.edge === 'bristly' ? 1.6 : 1.3));
        }
      }
      if (deep) {
        const dp = deep[p];
        // lighter middle, and a darker line where the paint collected at the rim
        const rim = smoothstep(0.12, 0.45, dp) * (1 - smoothstep(0.55, 0.9, dp));
        k *= (1 - s.wetRim * 0.6 * smoothstep(0.55, 0.97, dp)) * (1 + s.wetRim * 0.6 * rim);
      }
      if (inner) k *= 1 - hollow * smoothstep(0.72, 0.97, inner[p]);
      if (grain > 0) {
        // fine grain fixed to the canvas, so it shows however much the dabs overlap
        const g = noise((x + i) / 1.7, (y + j) / 1.7, seed + 7) * 0.55 + noise((x + i) / 4.5, (y + j) / 4.5, seed + 9) * 0.45;
        k *= 1 - grain * 0.85 * smoothstep(0.42, 0.78, g);
      }
      f[p] = k;
    }
  }
  return f;
}

// Moves the paint around with a smooth canvas-fixed warp (deform), colors included
function deformPixels(d: Uint8ClampedArray, w: number, h: number, x: number, y: number, s: BrushSettings) {
  const amp = Math.max(4, s.size) * s.deform * 0.32;
  const cell = Math.max(6, s.size * 1.3);
  const seed = Math.round(s.seed) * 17 + 3;
  const src = new Uint8ClampedArray(d);
  for (let j = 0; j < h; j++)
    for (let i = 0; i < w; i++) {
      const u = (x + i) / cell, v = (y + j) / cell;
      const sx = Math.round(i + (fbm(u, v, seed) - 0.5) * 2 * amp);
      const sy = Math.round(j + (fbm(u + 7.3, v + 2.1, seed + 1) - 0.5) * 2 * amp);
      const o = (j * w + i) * 4;
      if (sx < 0 || sy < 0 || sx >= w || sy >= h) {
        d[o + 3] = 0;
        continue;
      }
      const q = (sy * w + sx) * 4;
      d[o] = src[q];
      d[o + 1] = src[q + 1];
      d[o + 2] = src[q + 2];
      d[o + 3] = src[q + 3];
    }
}

// A finished stroke on its own canvas: deform, outline, rim and grain for the whole stroke
// `at` = where this piece sits on the canvas, when ctx holds only a piece of the stroke
// (the live High-quality preview), so the pattern still lines up with the finished stroke.
export function applyStrokeEdges(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, s: BrushSettings, at?: { x: number; y: number }) {
  if (!hasStrokeEdges(s) || w <= 0 || h <= 0 || w * h > 9_000_000) return;
  const img = ctx.getImageData(x, y, w, h);
  const d = img.data;
  const wx = at ? at.x : x, wy = at ? at.y : y;
  if (s.deform > 0) deformPixels(d, w, h, wx, wy, s);
  let amax = 0;
  for (let i = 3; i < d.length; i += 4) if (d[i] > amax) amax = d[i];
  if (!amax) return ctx.putImageData(img, x, y);
  const cov = new Float32Array(w * h);
  for (let i = 0; i < cov.length; i++) cov[i] = d[i * 4 + 3] / amax;
  const f = edgeFactors(cov, w, h, wx, wy, s);
  for (let i = 0; i < cov.length; i++) if (f[i] !== 1) d[i * 4 + 3] = Math.min(255, Math.round(d[i * 4 + 3] * f[i]));
  ctx.putImageData(img, x, y);
}

// A finished color-mixing stroke (painted straight onto the layer): only the new paint gets
// the outline, rim and grain; `before` is the layer as it was before the stroke
export function applyStrokeEdgesDirect(ctx: CanvasRenderingContext2D, before: ImageData, x: number, y: number, s: BrushSettings) {
  const w = before.width, h = before.height;
  if (!hasStrokeEdges(s) || w <= 0 || h <= 0 || w * h > 9_000_000) return;
  const after = ctx.getImageData(x, y, w, h);
  const a = after.data, b = before.data;
  const cov = new Float32Array(w * h);
  let cmax = 0;
  for (let i = 0; i < cov.length; i++) {
    const o = i * 4;
    const c = Math.max(Math.abs(a[o] - b[o]), Math.abs(a[o + 1] - b[o + 1]), Math.abs(a[o + 2] - b[o + 2]), Math.abs(a[o + 3] - b[o + 3])) / 255;
    cov[i] = c;
    if (c > cmax) cmax = c;
  }
  if (!cmax) return;
  for (let i = 0; i < cov.length; i++) cov[i] = Math.min(1, cov[i] / cmax);
  // deform: move the new paint (and where it is) with the warp, the old paint stays put
  if (s.deform > 0) {
    const moved = new Uint8ClampedArray(a);
    deformPixels(moved, w, h, x, y, s);
    const covPx = new Uint8ClampedArray(w * h * 4);
    for (let i = 0; i < cov.length; i++) covPx[i * 4 + 3] = Math.round(cov[i] * 255);
    deformPixels(covPx, w, h, x, y, s);
    for (let i = 0; i < cov.length; i++) {
      const m = covPx[i * 4 + 3] / 255, o = i * 4;
      for (let c = 0; c < 4; c++) a[o + c] = b[o + c] + (moved[o + c] - b[o + c]) * m;
      cov[i] = m;
    }
  }
  const f = edgeFactors(cov, w, h, x, y, s);
  for (let i = 0; i < cov.length; i++) {
    if (f[i] === 1) continue;
    const o = i * 4;
    for (let c = 0; c < 4; c++) a[o + c] = b[o + c] + (a[o + c] - b[o + c]) * Math.min(1, f[i]);
  }
  ctx.putImageData(after, x, y);
}

// ---------- Brush textures ----------
// The texture a brush paints with: paint is worn away where the texture is light (alpha = how
// much is taken away), the same way the paper grain works. Textures repeat seamlessly and
// stay fixed to the canvas, so hair, skin or cloth patterns line up across strokes.

// Looks that only make sense along the stroke (bristle marks, dry streaks, hair textures)
// turn with it even when "Tip turns with the stroke" is off
export function followsStroke(s: BrushSettings) {
  return s.followDirection || dirTexture(s) || s.streaks > 0 || s.edge === 'dry' || s.edge === 'bristly';
}

// Hair and fur textures follow the stroke direction (they are part of the tip), the others
// stay fixed to the canvas like paper
const DIRECTIONAL = new Set(['hair', 'fur']);
export function dirTexture(s: Pick<BrushSettings, 'texKind' | 'texture'>) {
  return (s.texture ?? 0) > 0 && DIRECTIONAL.has(s.texKind ?? '');
}
const texAlphas = new Map<string, Float32Array>();
function textureAlpha(id: string): Float32Array {
  let a = texAlphas.get(id);
  if (a) return a;
  const c = getTexture({ texKind: id });
  const d = c.getContext('2d', { willReadFrequently: true })!.getImageData(0, 0, 256, 256).data;
  a = new Float32Array(256 * 256);
  for (let i = 0; i < a.length; i++) a[i] = d[i * 4 + 3] / 255;
  texAlphas.set(id, a);
  return a;
}

export const TEXTURES: { id: string; label: string; icon: string }[] = [
  { id: 'paper', label: 'Paper grain', icon: '📄' },
  { id: 'watercolor', label: 'Watercolor paper', icon: '💧' },
  { id: 'canvas', label: 'Canvas', icon: '🧵' },
  { id: 'cloth', label: 'Cloth / linen', icon: '👕' },
  { id: 'denim', label: 'Denim', icon: '👖' },
  { id: 'knit', label: 'Knit / wool', icon: '🧶' },
  { id: 'hair', label: 'Hair', icon: '💇' },
  { id: 'fur', label: 'Fur', icon: '🐾' },
  { id: 'skin', label: 'Skin / pores', icon: '🙂' },
  { id: 'leather', label: 'Leather', icon: '👜' },
  { id: 'scales', label: 'Scales', icon: '🐉' },
  { id: 'wood', label: 'Wood grain', icon: '🪵' },
  { id: 'stone', label: 'Stone / concrete', icon: '🪨' },
  { id: 'grunge', label: 'Grunge', icon: '🩶' },
  { id: 'crosshatch', label: 'Crosshatch', icon: '#️⃣' },
  { id: 'dots', label: 'Halftone dots', icon: '⚫' },
  { id: 'custom', label: 'My picture…', icon: '🖼' },
];

const TEX = 256;
const textures = new Map<string, HTMLCanvasElement>();

// value noise that repeats every `p` cells, so the texture tiles without seams
function pnoise(x: number, y: number, p: number, seed: number) {
  const xi = Math.floor(x), yi = Math.floor(y);
  const fx = x - xi, fy = y - yi;
  const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
  const w = (v: number) => ((v % p) + p) % p;
  const a = hash(w(xi), w(yi), seed), b = hash(w(xi + 1), w(yi), seed), c = hash(w(xi), w(yi + 1), seed), d = hash(w(xi + 1), w(yi + 1), seed);
  return a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy;
}
// fractal version: cells = how many noise cells across the tile
function pfbm(u: number, v: number, cells: number, seed: number, oct = 3) {
  let sum = 0, amp = 0.55, tot = 0, f = 1;
  for (let o = 0; o < oct; o++) {
    sum += pnoise(u * cells * f, v * cells * f, cells * f, seed + o * 13) * amp;
    tot += amp;
    amp *= 0.5;
    f *= 2;
  }
  return sum / tot;
}

function makeTexture(id: string): HTMLCanvasElement {
  if (id === 'paper') return getPaper();
  const c = document.createElement('canvas');
  c.width = c.height = TEX;
  const ctx = c.getContext('2d')!;
  const img = ctx.createImageData(TEX, TEX);
  const TAU = Math.PI * 2;
  for (let y = 0; y < TEX; y++)
    for (let x = 0; x < TEX; x++) {
      const u = x / TEX, v = y / TEX;
      let a = 0; // how much paint is taken away here (0..1)
      switch (id) {
        case 'watercolor': {
          const n = pfbm(u, v, 8, 3, 4);
          a = smoothstep(0.42, 0.75, n) * 0.9;
          break;
        }
        case 'canvas': {
          // over-under weave of two thread directions
          const t1 = 0.5 + 0.5 * Math.sin(u * TAU * 32), t2 = 0.5 + 0.5 * Math.sin(v * TAU * 32);
          const over = (Math.floor(u * 32) + Math.floor(v * 32)) % 2 ? t1 : t2;
          a = (1 - over) * 0.75 + (pfbm(u, v, 16, 5) - 0.5) * 0.3;
          break;
        }
        case 'cloth': {
          const thx = 0.5 + 0.5 * Math.sin(u * TAU * 48 + pfbm(u, v, 4, 6) * 3);
          const thy = 0.5 + 0.5 * Math.sin(v * TAU * 48 + pfbm(u, v, 4, 7) * 3);
          a = (1 - Math.max(thx, thy)) * 0.8 + pnoise(u * 64, v * 64, 64, 8) * 0.2;
          break;
        }
        case 'denim': {
          // diagonal twill
          const d = 0.5 + 0.5 * Math.sin((u + v) * TAU * 40 + pfbm(u, v, 8, 9) * 2);
          a = (1 - d) * 0.7 + smoothstep(0.6, 0.9, pnoise(u * 128, v * 16, 128, 10)) * 0.35;
          break;
        }
        case 'knit': {
          // rows of little V loops
          const row = v * 24, col = u * 24;
          const fy = row - Math.floor(row), fx = col - Math.floor(col);
          const vshape = Math.abs(fx - 0.5) * 2 - (1 - fy);
          a = smoothstep(-0.15, 0.25, Math.abs(vshape) - 0.3) * 0.8 + pnoise(u * 48, v * 48, 48, 11) * 0.15;
          break;
        }
        case 'hair': {
          // long fine strands (stretched noise), slightly wavy
          const n = pnoise(u * 4, v * 96 + Math.sin(u * TAU * 2) * 2, 96, 12) * 0.65 + pnoise(u * 8, v * 180, 180, 13) * 0.35;
          a = smoothstep(0.42, 0.62, n);
          break;
        }
        case 'fur': {
          const n = pnoise(u * 16, v * 64, 64, 14) * 0.6 + pnoise(u * 32, v * 128, 128, 15) * 0.4;
          a = smoothstep(0.38, 0.62, n) * 0.9 + pfbm(u, v, 4, 16) * 0.1;
          break;
        }
        case 'skin': {
          // soft mottling and tiny pores
          const mott = pfbm(u, v, 6, 17, 3);
          const pore = smoothstep(0.78, 0.95, pnoise(u * 90, v * 90, 90, 18));
          a = (mott - 0.4) * 0.35 + pore * 0.6;
          break;
        }
        case 'leather': {
          // cracked cells (closest-point distance) with grain
          const cells = 14;
          const cx = u * cells, cy = v * cells;
          let d1 = 9, d2 = 9;
          for (let j = -1; j <= 1; j++)
            for (let i = -1; i <= 1; i++) {
              const gx = Math.floor(cx) + i, gy = Math.floor(cy) + j;
              const wx = ((gx % cells) + cells) % cells, wy = ((gy % cells) + cells) % cells;
              const px = gx + hash(wx, wy, 19), py = gy + hash(wx, wy, 20);
              const dd = Math.hypot(px - cx, py - cy);
              if (dd < d1) [d2, d1] = [d1, dd];
              else if (dd < d2) d2 = dd;
            }
          a = (1 - smoothstep(0.02, 0.12, d2 - d1)) * 0.85 + pnoise(u * 80, v * 80, 80, 21) * 0.2;
          break;
        }
        case 'scales': {
          // fish / dragon scales in overlapping rows, each with a clear gap around it,
          // so every scale reads on its own (6 across, 10 rows, tiles seamlessly)
          const X = u * 6, Y = v * 6;
          const r0 = Math.floor(Y / 0.6);
          let best = -1, bd = 9;
          for (let r = r0 - 1; r <= r0 + 1; r++) {
            const off = ((r % 2) + 2) % 2 ? 0.5 : 0;
            const cy = r * 0.6;
            for (const cx of [Math.floor(X - off) + off + 0.5 - 1, Math.floor(X - off) + off + 0.5, Math.floor(X - off) + off + 0.5 + 1]) {
              const dd = Math.hypot(X - cx, (Y - cy) * 1.05) / 0.56;
              // upper rows lie on top: the first (highest) scale that covers the pixel wins
              if (dd < 1 && (best < 0 || r < best)) {
                best = r;
                bd = dd;
              }
            }
          }
          const gap = best < 0 ? 1 : smoothstep(0.84, 0.9, bd);
          const shade = best < 0 ? 0 : smoothstep(0.35, 0.84, bd) * 0.35;
          a = gap + (1 - gap) * shade;
          break;
        }
        case 'wood': {
          const warp = pfbm(u, v, 3, 22) * 3;
          const ring = 0.5 + 0.5 * Math.sin((v * 18 + warp) * TAU);
          a = smoothstep(0.55, 1, ring) * 0.7 + pnoise(u * 160, v * 8, 160, 23) * 0.3;
          break;
        }
        case 'stone': {
          const n = pfbm(u, v, 10, 24, 4);
          const pit = smoothstep(0.82, 0.92, pnoise(u * 40, v * 40, 40, 25));
          a = smoothstep(0.35, 0.8, n) * 0.6 + pit * 0.5;
          break;
        }
        case 'grunge': {
          const n = pfbm(u, v, 6, 26, 4);
          const scratch = smoothstep(0.92, 0.97, pnoise(u * 2, v * 120, 120, 27));
          a = smoothstep(0.45, 0.62, n) * 0.85 + scratch * 0.8;
          break;
        }
        case 'crosshatch': {
          const l1 = 0.5 + 0.5 * Math.sin((u + v) * TAU * 20), l2 = 0.5 + 0.5 * Math.sin((u - v) * TAU * 20);
          a = Math.min(l1, l2) > 0.25 ? 0.85 : 0.1;
          a = smoothstep(0.15, 0.4, Math.min(l1, l2)) * 0.85;
          break;
        }
        case 'dots': {
          const fx = u * 32 - Math.floor(u * 32) - 0.5, fy = v * 32 - Math.floor(v * 32) - 0.5;
          a = smoothstep(0.25, 0.32, Math.hypot(fx, fy)) * 0.9;
          break;
        }
      }
      img.data[(y * TEX + x) * 4 + 3] = Math.round(Math.min(1, Math.max(0, a)) * 255);
    }
  ctx.putImageData(img, 0, 0);
  return c;
}

// your own texture: light parts of the picture take paint away, dark parts keep it
const texLoading = new Set<string>();
function customTexture(src: string, onReady?: () => void): HTMLCanvasElement | null {
  const key = 'custom:' + src.length + ':' + src.slice(-48);
  const have = textures.get(key);
  if (have) return have;
  if (!texLoading.has(key)) {
    texLoading.add(key);
    const img = new Image();
    img.onload = () => {
      const k = Math.min(1, 512 / Math.max(img.width, img.height));
      const c = document.createElement('canvas');
      c.width = Math.max(1, Math.round(img.width * k));
      c.height = Math.max(1, Math.round(img.height * k));
      const ctx = c.getContext('2d', { willReadFrequently: true })!;
      ctx.drawImage(img, 0, 0, c.width, c.height);
      const d = ctx.getImageData(0, 0, c.width, c.height);
      for (let i = 0; i < d.data.length; i += 4) {
        const l = (0.299 * d.data[i] + 0.587 * d.data[i + 1] + 0.114 * d.data[i + 2]) / 255;
        const al = d.data[i + 3] / 255;
        d.data[i] = d.data[i + 1] = d.data[i + 2] = 0;
        d.data[i + 3] = Math.round((al ? l : 1) * 255);
      }
      ctx.putImageData(d, 0, 0);
      textures.set(key, c);
      texLoading.delete(key);
      onReady?.();
    };
    img.onerror = () => texLoading.delete(key);
    img.src = src;
  }
  return null;
}

// Your texture picture in its own colors (for "Keep texture colors")
function customTextureColors(src: string, onReady?: () => void): HTMLCanvasElement | null {
  const key = 'customc:' + src.length + ':' + src.slice(-48);
  const have = textures.get(key);
  if (have) return have;
  if (!texLoading.has(key)) {
    texLoading.add(key);
    const img = new Image();
    img.onload = () => {
      const k = Math.min(1, 512 / Math.max(img.width, img.height));
      const c = document.createElement('canvas');
      c.width = Math.max(1, Math.round(img.width * k));
      c.height = Math.max(1, Math.round(img.height * k));
      c.getContext('2d')!.drawImage(img, 0, 0, c.width, c.height);
      textures.set(key, c);
      texLoading.delete(key);
      onReady?.();
    };
    img.onerror = () => texLoading.delete(key);
    img.src = src;
  }
  return null;
}

// The colored texture the paint takes when "Keep texture colors" is on. Only your own pictures
// have colors; the built-in textures are grayscale, so they give null.
export function getTextureColors(s: Pick<BrushSettings, 'texKind' | 'texImage'>, onReady?: () => void): HTMLCanvasElement | null {
  return s.texKind === 'custom' && s.texImage ? customTextureColors(s.texImage, onReady) : null;
}

export function getTexture(s: Pick<BrushSettings, 'texKind' | 'texImage'>, onReady?: () => void): HTMLCanvasElement {
  const id = s.texKind || 'paper';
  if (id === 'custom') return (s.texImage && customTexture(s.texImage, onReady)) || getPaper();
  let t = textures.get(id);
  if (!t) {
    t = makeTexture(id);
    textures.set(id, t);
  }
  return t;
}
