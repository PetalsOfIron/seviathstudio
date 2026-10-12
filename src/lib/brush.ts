// Brush settings, the brush collection, and the brush tip ("dab") used for painting.
// Every brush is made of the same settings, so imported brushes (Photoshop, Krita, CSP)
// and your own saved brushes work exactly like the built-in ones.

export type TipShape = 'round' | 'shape' | 'bristle' | 'chalk' | 'wet' | 'splatter' | 'leaf' | 'grass' | 'star' | 'cloud' | 'image';
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

export const CATEGORIES = ['Pens', 'Pencils', 'Markers', 'Paint', 'Airbrush', 'Blend', 'Nature & FX', 'My brushes'];

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
  b('Calligraphy', 'Pens', 'Flat nib held at 45°: thin and thick depending on direction.', { size: 22, hardness: 1, spacing: 0.02, minSize: 0.6, roundness: 0.18, angle: 45, stabilize: 0.35 }),
  // ---- Pencils
  b('Pencil', 'Pencils', 'HB pencil for sketching. Press harder for darker lines.', { size: 6, hardness: 0.7, minSize: 0.5, opacityPressure: true, texture: 0.45, stabilize: 0.1 }),
  b('Soft pencil 2B', 'Pencils', 'Softer, darker, grainier.', { size: 9, hardness: 0.55, minSize: 0.45, opacityPressure: true, texture: 0.4, stabilize: 0.1 }),
  b('Dark pencil 6B', 'Pencils', 'Very soft and dark, good for shading.', { size: 14, hardness: 0.45, minSize: 0.4, opacityPressure: true, texture: 0.38, stabilize: 0.05 }),
  b('Mechanical', 'Pencils', 'Thin, clean, even pencil line.', { size: 2.5, hardness: 0.8, minSize: 0.8, opacityPressure: true, texture: 0.3, stabilize: 0.2 }),
  b('Colored pencil', 'Pencils', 'Waxy colored pencil with paper grain.', { size: 7, hardness: 0.6, minSize: 0.6, opacityPressure: true, texture: 0.55, stabilize: 0.1, opacity: 0.9 }),
  b('Charcoal', 'Pencils', 'Rough, powdery and dark, with a broken edge.', { size: 24, tip: 'chalk', hardness: 0.6, roundness: 0.6, followDirection: true, edge: 'torn', edgeAmount: 0.4, edgeDetail: 0.7, grain: 0.25, variants: 4, spacing: 0.08, minSize: 0.5, opacityPressure: true, texture: 0.6, stabilize: 0.05 }),
  b('Pastel', 'Pencils', 'Soft chalk pastel that catches the paper texture.', { size: 30, tip: 'chalk', hardness: 0.6, edge: 'torn', edgeAmount: 0.3, edgeDetail: 0.5, grain: 0.3, variants: 4, angleJitter: 1, spacing: 0.08, minSize: 0.6, opacityPressure: true, texture: 0.5, mix: 0.1, opacity: 0.9 }),
  // ---- Markers
  b('Marker', 'Markers', 'Flat, see-through color that does not get darker where it overlaps itself.', { size: 30, opacity: 0.5, hardness: 0.85, sizePressure: false, minSize: 1, ends: 'flat' }),
  b('Highlighter', 'Markers', 'Wide chisel tip, see-through.', { size: 34, opacity: 0.4, hardness: 0.95, sizePressure: false, minSize: 1, roundness: 0.3, angle: 0, ends: 'flat', stabilize: 0.3 }),
  b('Alcohol marker', 'Markers', 'Soft-edged marker that blends a little with what is under it.', { size: 26, opacity: 0.65, flow: 0.5, hardness: 0.55, minSize: 0.7, mix: 0.12, opacityPressure: true }),
  // ---- Paint
  b('Oil paint', 'Paint', 'Thick paint with bristle marks; mixes with the paint already there.', { size: 40, tip: 'bristle', roundness: 0.6, followDirection: true, edge: 'bristly', edgeAmount: 0.35, edgeDetail: 0.6, variants: 3, spacing: 0.05, flow: 0.9, mix: 0.45, minSize: 0.5, opacityPressure: true, stabilize: 0.1 }),
  b('Acrylic', 'Paint', 'Opaque paint with a little bristle texture.', { size: 34, tip: 'bristle', roundness: 0.7, followDirection: true, spacing: 0.05, mix: 0.2, minSize: 0.5, texture: 0.15, stabilize: 0.1 }),
  b('Gouache', 'Paint', 'Matte, flat and opaque, slightly chalky, with soft uneven edges.', { size: 30, hardness: 0.75, edge: 'ragged', edgeAmount: 0.2, edgeDetail: 0.5, variants: 4, angleJitter: 1, mix: 0.15, minSize: 0.5, texture: 0.15, flow: 0.9 }),
  b('Watercolor', 'Paint', 'See-through paint with uneven, darker wet edges and paper grain.', { size: 50, tip: 'round', hardness: 0.7, edge: 'ragged', edgeAmount: 0.65, edgeDetail: 0.35, wetRim: 0.7, variants: 6, angleJitter: 1, opacity: 0.75, flow: 0.3, spacing: 0.08, mix: 0, minSize: 0.5, opacityPressure: true, texture: 0.3, stabilize: 0.1 }),
  b('Watercolor wash', 'Paint', 'Big soft wash for skies and backgrounds.', { size: 140, tip: 'round', hardness: 0.4, edge: 'feathered', edgeAmount: 0.6, edgeDetail: 0.2, wetRim: 0.45, variants: 5, angleJitter: 1, opacity: 0.6, flow: 0.15, mix: 0, sizePressure: false, minSize: 1, texture: 0.25 }),
  b('Watercolor bloom', 'Paint', 'Drops of water spreading into paint: blotchy cauliflower edges.', { size: 70, tip: 'round', hardness: 0.6, edge: 'ragged', edgeAmount: 0.6, edgeDetail: 0.7, wetRim: 0.8, variants: 8, angleJitter: 1, sizeJitter: 0.3, scatter: 0.25, opacity: 0.7, flow: 0.5, spacing: 0.3, mix: 0, minSize: 0.6, texture: 0.3 }),
  b('Dry brush', 'Paint', 'Streaky paint with gaps, like a brush running out of paint.', { size: 44, tip: 'round', hardness: 0.85, roundness: 0.55, followDirection: true, edge: 'dry', edgeAmount: 0.55, edgeDetail: 0.6, variants: 4, spacing: 0.04, mix: 0.15, minSize: 0.6, texture: 0.2 }),
  b('Paint mix', 'Paint', 'Wet paint: your color mixes with the colors already on the canvas.', { size: 40, hardness: 0.5, spacing: 0.08, flow: 0.8, minSize: 0.4, opacityPressure: true, mix: 0.5, stabilize: 0.1 }),
  // ---- Airbrush
  b('Soft round', 'Airbrush', 'Soft edges for shading and smooth color.', { size: 60, flow: 0.35, hardness: 0.2, spacing: 0.08, minSize: 0.2, opacityPressure: true, stabilize: 0 }),
  b('Airbrush', 'Airbrush', 'Very soft spray that builds up slowly.', { size: 150, flow: 0.06, hardness: 0, sizePressure: false, minSize: 1, opacityPressure: true, stabilize: 0 }),
  b('Hard airbrush', 'Airbrush', 'Builds up slowly but keeps a firm edge.', { size: 60, flow: 0.12, hardness: 0.65, sizePressure: false, minSize: 1, opacityPressure: true, stabilize: 0 }),
  b('Spray paint', 'Airbrush', 'Grainy spray can.', { size: 90, tip: 'splatter', flow: 0.35, spacing: 0.15, scatter: 0.4, sizeJitter: 0.4, angleJitter: 1, sizePressure: false, minSize: 1, opacityPressure: true, stabilize: 0 }),
  b('Splatter', 'Airbrush', 'Random paint drops.', { size: 70, tip: 'splatter', spacing: 0.7, scatter: 1.4, sizeJitter: 0.8, angleJitter: 1, opacityJitter: 0.3, sizePressure: true, minSize: 0.4, stabilize: 0 }),
  // ---- Blend
  b('Blender', 'Blend', 'Adds no color. Smears and blends the colors already there.', { size: 50, flow: 0.6, hardness: 0.3, spacing: 0.08, minSize: 0.4, opacityPressure: true, mix: 1, stabilize: 0.1 }),
  b('Smudge', 'Blend', 'Pushes paint along like a finger.', { size: 36, flow: 0.9, hardness: 0.5, spacing: 0.05, minSize: 0.5, mix: 0.95, stabilize: 0.05 }),
  b('Bristle blender', 'Blend', 'Blends with brush marks, like a dry brush.', { size: 46, tip: 'bristle', roundness: 0.6, followDirection: true, flow: 0.7, spacing: 0.05, mix: 0.9, minSize: 0.5 }),
  b('Water blend', 'Blend', 'Softly blends and lightens, like a wet brush.', { size: 60, tip: 'wet', flow: 0.4, hardness: 0.3, mix: 0.85, minSize: 0.5, texture: 0.15 }),
  // ---- Nature & effects
  b('Leaves', 'Nature & FX', 'Scattered leaves in slightly different colors.', { size: 40, tip: 'leaf', spacing: 0.45, scatter: 0.9, sizeJitter: 0.5, angleJitter: 1, colorJitter: 0.35, sizePressure: true, minSize: 0.5, stabilize: 0 }),
  b('Grass', 'Nature & FX', 'Blades of grass that follow your stroke upwards.', { size: 45, tip: 'grass', spacing: 0.12, scatter: 0.5, sizeJitter: 0.5, angleJitter: 0.12, colorJitter: 0.25, minSize: 0.5, stabilize: 0 }),
  b('Clouds', 'Nature & FX', 'Soft puffy clouds; build them up in layers.', { size: 120, tip: 'cloud', flow: 0.35, spacing: 0.2, scatter: 0.5, sizeJitter: 0.5, angleJitter: 1, opacityJitter: 0.4, sizePressure: false, minSize: 1, stabilize: 0 }),
  b('Sparkles', 'Nature & FX', 'Twinkling stars and glitter.', { size: 40, tip: 'star', spacing: 1.2, scatter: 2, sizeJitter: 0.8, angleJitter: 0.15, opacityJitter: 0.4, minSize: 0.3, stabilize: 0 }),
  b('Hearts', 'Nature & FX', 'Little hearts scattered along the stroke.', { size: 30, tip: 'shape', shapeBase: 'heart', hardness: 1, spacing: 1.1, scatter: 1.2, sizeJitter: 0.6, angleJitter: 0.15, colorJitter: 0.2, minSize: 0.5, stabilize: 0 }),
  b('Confetti', 'Nature & FX', 'Colorful scattered confetti shapes.', { size: 24, tip: 'shape', shapeBase: 'square', hardness: 1, roundness: 0.45, spacing: 0.9, scatter: 1.6, sizeJitter: 0.5, angleJitter: 1, colorJitter: 0.9, minSize: 0.6, stabilize: 0 }),
  b('Fur', 'Nature & FX', 'Short hair strokes for fur and fluff.', { size: 30, tip: 'grass', followDirection: true, angle: 90, spacing: 0.1, scatter: 0.4, sizeJitter: 0.4, angleJitter: 0.08, colorJitter: 0.1, minSize: 0.4, stabilize: 0.1 }),
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
  return (s.edge && s.edge !== 'smooth') || s.wetRim > 0 || s.grain > 0;
}

function edgeKey(s: BrushSettings) {
  return [s.edge, s.edgeAmount.toFixed(2), s.edgeDetail.toFixed(2), s.wetRim.toFixed(2), s.grain.toFixed(2), s.seed].join(',');
}

// Gives any tip a new outline (ragged, torn…), a wet rim and inner grain
function shapeEdges(base: HTMLCanvasElement, s: BrushSettings, variant: number): HTMLCanvasElement {
  const N = base.width;
  const seed = Math.round(s.seed) * 101 + variant * 977;
  const amt = Math.max(0, Math.min(1, s.edgeAmount));
  const freq = 2 + s.edgeDetail * 26; // wobbles across the tip
  const blurPx = s.edge === 'feathered' ? 14 : s.edge === 'torn' ? 5 : 12;
  // a blurred copy works like "how far inside the shape is this pixel"
  const field = alphaOf(s.edge === 'smooth' || s.edge === 'dry' ? base : blurred(base, blurPx));
  const orig = alphaOf(base);
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
          // streaks along the stroke direction (the tip's x axis)
          const n = fbm(u * 2, v * freq * 1.6, seed);
          a = orig[i] * smoothstep(amt * 0.75 - 0.08, amt * 0.75 + 0.08, n);
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
    const inner = alphaOf(blurred(tmp, 9));
    for (let i = 0; i < out.length; i++) {
      const deep = smoothstep(0.55, 0.98, inner[i]);
      out[i] *= 1 - s.wetRim * 0.65 * deep;
    }
  }
  // grain: tiny holes inside the tip
  if (s.grain > 0) {
    for (let y = 0; y < N; y++)
      for (let x = 0; x < N; x++) {
        const n = noise(x / 3.2, y / 3.2, seed + 7) * 0.6 + noise(x / 1.4, y / 1.4, seed + 9) * 0.4;
        out[y * N + x] *= 1 - s.grain * smoothstep(0.35, 0.75, n);
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
  if (masks.size > 80) masks.clear();
  masks.set(key, made);
  return made;
}

// The white mask of the brush tip. `variant` picks one of the slightly different versions.
export function getTipMask(s: BrushSettings, variant = 0): HTMLCanvasElement {
  const base = baseMask(s);
  if (!hasEdgeEffects(s)) return base;
  const v = Math.max(0, Math.min(7, variant));
  const key = tipKey(s) + '|' + edgeKey(s) + '|' + v;
  const m = masks.get(key);
  if (m) return m;
  const made = shapeEdges(base, s, v);
  // an image tip that isn't loaded yet: don't keep the stand-in
  if (!(s.tip === 'image' && !masks.has(tipKey(s)))) {
    if (masks.size > 80) masks.clear();
    masks.set(key, made);
  }
  return made;
}

// Which version of the tip the next dab uses
export function pickVariant(s: BrushSettings) {
  return s.variants > 1 && hasEdgeEffects(s) ? Math.floor(Math.random() * Math.min(8, s.variants)) : 0;
}

// The tip in a color, ready to stamp. `s` may also be a number (hardness of a round tip).
export function getDab(color: string, s: BrushSettings | number, variant = 0): HTMLCanvasElement {
  const settings: BrushSettings = typeof s === 'number' ? { ...BASE, hardness: s } : s;
  const key = color + '|' + tipKey(settings) + (hasEdgeEffects(settings) ? '|' + edgeKey(settings) + '|' + variant : '');
  const cached = colored.get(key);
  if (cached) return cached;
  if (colored.size > 200) colored.clear();
  const mask = getTipMask(settings, variant);
  const c = canvas(mask.width);
  const ctx = c.getContext('2d')!;
  ctx.drawImage(mask, 0, 0);
  ctx.globalCompositeOperation = 'source-in';
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, c.width, c.height);
  // an image tip may not be ready yet: don't keep the stand-in
  if (!(settings.tip === 'image' && !masks.has(tipKey(settings)))) colored.set(key, c);
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
  if (s.followDirection) a += dir;
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

// Whether dabs need turning/squashing (otherwise the faster plain stamp is used)
export function isShaped(s: BrushSettings) {
  return s.tip !== 'round' || s.roundness < 0.999 || s.angle % 360 !== 0 || s.scatter > 0 || s.sizeJitter > 0 || s.angleJitter > 0 || s.followDirection || hasEdgeEffects(s);
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
      stampDab(t, getDab(col, s, pickVariant(s)), a.x + (q.x - a.x) * u + shape.ox, a.y + (q.y - a.y) * u + shape.oy, Math.max(0.4, shape.r), shape.a, s.roundness);
      carry = d + step - len;
    }
  }
  if (s.texture > 0) {
    t.globalAlpha = s.texture;
    t.globalCompositeOperation = 'destination-out';
    const pat = t.createPattern(getPaper(), 'repeat');
    if (pat) {
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
  'scatter', 'sizeJitter', 'angleJitter', 'opacityJitter', 'colorJitter', 'sides', 'starInner', 'edgeAmount', 'edgeDetail', 'wetRim', 'grain', 'variants',
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

// A surprise brush: random but sensible
export function surpriseBrush(): BrushSettings {
  const R = Math.random;
  const pick = <T,>(xs: T[]) => xs[Math.floor(R() * xs.length)];
  const tip = pick<TipShape>(['round', 'shape', 'shape', 'bristle', 'chalk', 'splatter', 'cloud', 'star', 'leaf']);
  return {
    ...BASE,
    tip,
    shapeBase: pick<ShapeBase>(['circle', 'square', 'triangle', 'polygon', 'star', 'diamond', 'heart', 'drop', 'crescent', 'rounded']),
    sides: 3 + Math.floor(R() * 6),
    starInner: 0.3 + R() * 0.4,
    size: Math.round(10 + R() * 60),
    hardness: Math.round(R() * 100) / 100,
    spacing: Math.round((0.05 + Math.pow(R(), 2) * 1.2) * 100) / 100,
    roundness: Math.round((0.3 + R() * 0.7) * 100) / 100,
    angle: Math.round(R() * 180 - 90),
    followDirection: R() < 0.5,
    scatter: Math.round(Math.pow(R(), 2) * 150) / 100,
    sizeJitter: Math.round(R() * 70) / 100,
    angleJitter: Math.round(Math.pow(R(), 2) * 100) / 100,
    opacityJitter: Math.round(R() * 50) / 100,
    colorJitter: Math.round(Math.pow(R(), 2) * 60) / 100,
    edge: pick<EdgeStyle>(['smooth', 'ragged', 'torn', 'feathered', 'bristly', 'dry']),
    edgeAmount: Math.round((0.2 + R() * 0.6) * 100) / 100,
    edgeDetail: Math.round(R() * 100) / 100,
    wetRim: R() < 0.3 ? Math.round(R() * 80) / 100 : 0,
    grain: R() < 0.3 ? Math.round(R() * 50) / 100 : 0,
    variants: 1 + Math.floor(R() * 6),
    seed: Math.floor(R() * 9999),
    flow: Math.round((0.3 + R() * 0.7) * 100) / 100,
    texture: R() < 0.3 ? Math.round(R() * 50) / 100 : 0,
    minSize: Math.round((0.2 + R() * 0.7) * 100) / 100,
  };
}

// ---------- Stroke edges (wet media) ----------
// Per-dab edges disappear when dabs overlap closely, so wet brushes also get their edges
// on the whole stroke when the pen lifts, like paint drying: a ragged outline that is
// fixed to the canvas, and paint collecting at the rim with a lighter middle.

export function hasStrokeEdges(s: BrushSettings) {
  // spaced-out or scattered dabs already show their own edges
  if (s.spacing > 0.2 || s.scatter > 0.5) return false;
  return s.wetRim > 0 || s.edge === 'ragged' || s.edge === 'torn' || s.edge === 'feathered';
}

export function applyStrokeEdges(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, s: BrushSettings) {
  if (!hasStrokeEdges(s) || w <= 0 || h <= 0 || w * h > 16_000_000) return;
  const img = ctx.getImageData(x, y, w, h);
  const d = img.data;
  let amax = 0;
  for (let i = 3; i < d.length; i += 4) if (d[i] > amax) amax = d[i];
  if (!amax) return;
  // blurred copies of where the stroke has paint: "how deep inside the stroke is this pixel".
  // Light paint counts as covered too, so thin glazes are not eaten away.
  const cover = document.createElement('canvas');
  cover.width = w;
  cover.height = h;
  const cimg = new ImageData(w, h);
  for (let i = 3; i < d.length; i += 4) cimg.data[i] = Math.min(255, (d[i] * 3 * 255) / amax);
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
  const ragged = s.edge === 'ragged' || s.edge === 'torn' || s.edge === 'feathered';
  const near = ragged ? blurAlpha(Math.max(1.5, size * (s.edge === 'feathered' ? 0.2 : 0.12))) : null;
  const deep = s.wetRim > 0 ? blurAlpha(Math.max(2, size * 0.12)) : null;
  const amt = Math.max(0, Math.min(1, s.edgeAmount));
  const scale = size * (0.9 - Math.max(0, Math.min(1, s.edgeDetail)) * 0.75); // px per noise cell
  const seed = Math.round(s.seed) * 31 + 5;
  const soft = s.edge === 'torn' ? 0.05 : s.edge === 'feathered' ? 0.3 : 0.12;
  for (let j = 0, p = 0; j < h; j++) {
    for (let i = 0; i < w; i++, p++) {
      const k = p * 4 + 3;
      if (!d[k]) continue;
      let f = 1;
      if (near) {
        // big wobbles plus a fine fringe, both fixed to the canvas
        const n = (fbm((x + i) / scale, (y + j) / scale, seed) - 0.5) * 0.75 + (noise((x + i) / (scale * 0.18), (y + j) / (scale * 0.18), seed + 3) - 0.5) * 0.45;
        f = smoothstep(0.5 - soft, 0.5 + soft, near[p] + n * amt * (s.edge === 'torn' ? 1.6 : 1.3));
      }
      if (deep) {
        const dp = deep[p];
        // lighter middle, and a darker line where the paint collected at the rim
        const rim = smoothstep(0.12, 0.45, dp) * (1 - smoothstep(0.55, 0.9, dp));
        f *= (1 - s.wetRim * 0.6 * smoothstep(0.55, 0.97, dp)) * (1 + s.wetRim * 0.6 * rim);
      }
      if (f !== 1) d[k] = Math.min(255, Math.round(d[k] * f));
    }
  }
  ctx.putImageData(img, x, y);
}
