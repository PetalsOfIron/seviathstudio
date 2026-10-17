// SPDX-License-Identifier: GPL-3.0-or-later
// Brush settings and the built-in brush collection. Every brush is made of the same settings,
// so imported and saved brushes work exactly like the built-in ones (same as the web version).

pub mod engine;
pub mod noise;
pub mod random;
pub mod raster;
pub mod textures;
pub mod tips;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tip {
    Round,
    Shape,
    Bristle,
    Chalk,
    Wet,
    Splatter,
    Leaf,
    Grass,
    Star,
    Cloud,
    Strands,
    Speckle,
    Grunge,
    Hatch,
    Scale,
    Sponge,
    Feather,
}

impl Tip {
    pub const ALL: [Tip; 17] = [
        Tip::Round,
        Tip::Shape,
        Tip::Bristle,
        Tip::Chalk,
        Tip::Wet,
        Tip::Splatter,
        Tip::Leaf,
        Tip::Grass,
        Tip::Star,
        Tip::Cloud,
        Tip::Strands,
        Tip::Speckle,
        Tip::Grunge,
        Tip::Hatch,
        Tip::Scale,
        Tip::Sponge,
        Tip::Feather,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Tip::Round => "Round",
            Tip::Shape => "Shape…",
            Tip::Bristle => "Bristles",
            Tip::Chalk => "Chalk",
            Tip::Wet => "Wet edge",
            Tip::Splatter => "Splatter",
            Tip::Leaf => "Leaf",
            Tip::Grass => "Grass",
            Tip::Star => "Sparkle",
            Tip::Cloud => "Cloud",
            Tip::Strands => "Hair / fur",
            Tip::Speckle => "Speckles",
            Tip::Grunge => "Grunge",
            Tip::Hatch => "Hatching",
            Tip::Scale => "Scale",
            Tip::Sponge => "Sea sponge",
            Tip::Feather => "Feather",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ShapeBase {
    Circle,
    Square,
    Rounded,
    Triangle,
    Diamond,
    Polygon,
    Star,
    Heart,
    Crescent,
    Drop,
}

impl ShapeBase {
    pub const ALL: [ShapeBase; 10] = [
        ShapeBase::Circle,
        ShapeBase::Square,
        ShapeBase::Rounded,
        ShapeBase::Triangle,
        ShapeBase::Diamond,
        ShapeBase::Polygon,
        ShapeBase::Star,
        ShapeBase::Heart,
        ShapeBase::Crescent,
        ShapeBase::Drop,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ShapeBase::Circle => "Circle",
            ShapeBase::Square => "Square",
            ShapeBase::Rounded => "Rounded",
            ShapeBase::Triangle => "Triangle",
            ShapeBase::Diamond => "Diamond",
            ShapeBase::Polygon => "Polygon",
            ShapeBase::Star => "Star",
            ShapeBase::Heart => "Heart",
            ShapeBase::Crescent => "Moon",
            ShapeBase::Drop => "Drop",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Edge {
    Smooth,
    Ragged,
    Torn,
    Feathered,
    Bristly,
    Dry,
}

impl Edge {
    pub const ALL: [Edge; 6] = [Edge::Smooth, Edge::Ragged, Edge::Torn, Edge::Feathered, Edge::Bristly, Edge::Dry];
    pub fn label(self) -> &'static str {
        match self {
            Edge::Smooth => "Smooth",
            Edge::Ragged => "Ragged",
            Edge::Torn => "Torn",
            Edge::Feathered => "Feathered",
            Edge::Bristly => "Bristly",
            Edge::Dry => "Dry brush",
        }
    }
    pub fn about(self) -> &'static str {
        match self {
            Edge::Smooth => "Clean outline",
            Edge::Ragged => "Uneven, organic outline like real paint or watercolor",
            Edge::Torn => "Rough, broken outline like charcoal or torn paper",
            Edge::Feathered => "Soft, cloudy outline",
            Edge::Bristly => "Little hairs sticking out of the edge",
            Edge::Dry => "Streaks with gaps, like a brush running out of paint",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ends {
    Round,
    Taper,
    Flat,
}

/// How paint behaves where it meets other paint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paint {
    /// The old way: colors lie over each other (and mix as plain RGB when the brush mixes).
    Normal,
    /// Real paint: colors mix like pigments (blue and yellow make green, not gray).
    Mix,
    /// A thin transparent layer: colors multiply and deepen what is below.
    Glaze,
}

impl Paint {
    pub const ALL: [(Paint, &'static str, &'static str); 3] = [
        (Paint::Normal, "Plain", "Colors lie over each other, as in most drawing programs."),
        (Paint::Mix, "Mix like paint", "Blue and yellow make green with life in it, red and yellow make orange: never muddy gray."),
        (Paint::Glaze, "Glaze", "A transparent layer over dry paint: yellow under blue turns deep green, the paint below shines through."),
    ];
}

/// What changes the color along a stroke (from the brush color toward the second color).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transition {
    Off,
    /// Over the length of the stroke.
    Length,
    /// The faster you draw, the more of the second color.
    Speed,
    /// The harder you press, the more of the second color.
    Pressure,
    /// Back and forth between the two colors along the stroke.
    Cycle,
}

impl Transition {
    pub const ALL: [(Transition, &'static str, &'static str); 5] = [
        (Transition::Off, "Off", "One color all along."),
        (Transition::Length, "Length", "The color slides from the brush color to the second color over the stroke."),
        (Transition::Speed, "Speed", "Fast strokes lean toward the second color, slow ones stay the brush color."),
        (Transition::Pressure, "Pressure", "Light touch is the brush color, firm touch is the second color."),
        (Transition::Cycle, "Cycle", "Back and forth between the two colors, again and again."),
    ];
}

/// The edge fray engine: how the edge of a stroke breaks up (on top of the edge type).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fray {
    Off,
    /// Tiny regular teeth, like a cheap saw.
    Serrated,
    /// Long splits along the stroke, like a dry brush with split hairs.
    Split,
    /// Lumpy, bitten and broken, like a worn crayon or chalk.
    Knotted,
}

impl Fray {
    pub const ALL: [(Fray, &'static str, &'static str); 4] = [
        (Fray::Off, "Smooth", "No extra fraying."),
        (Fray::Serrated, "Serrated", "Tiny teeth along the edge."),
        (Fray::Split, "Split ends", "Long splits along the stroke, like a dry brush with split hairs."),
        (Fray::Knotted, "Knotted", "Lumpy and broken, like worn chalk."),
    ];
}

/// How often a mixing brush is dipped in fresh paint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dip {
    /// Fresh paint slowly comes back as you go (the default).
    Gradual,
    /// Loaded once at the start; it runs out and keeps picking up what it paints over.
    Once,
    /// Dipped again every so many pixels.
    Every,
}

impl Dip {
    pub const ALL: [(Dip, &'static str, &'static str); 3] = [
        (Dip::Gradual, "Gradual", "Fresh paint slowly comes back as you paint."),
        (Dip::Once, "Once", "Loaded at the start of the stroke only: the brush runs out of its own color and carries what it picks up."),
        (Dip::Every, "Every so far", "Dipped again after a set length of stroke."),
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MixRefer {
    All,
    Layer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TexKind {
    Paper,
    Watercolor,
    Canvas,
    Cloth,
    Denim,
    Knit,
    Hair,
    Fur,
    Skin,
    Leather,
    Scales,
    Wood,
    Stone,
    Grunge,
    Crosshatch,
    Dots,
    // nature & materials
    Bark,
    Sand,
    Marble,
    Water,
    Brick,
    Feathers,
    SnakeSkin,
    Foliage,
}

impl TexKind {
    pub const ALL: [TexKind; 24] = [
        TexKind::Paper,
        TexKind::Watercolor,
        TexKind::Canvas,
        TexKind::Cloth,
        TexKind::Denim,
        TexKind::Knit,
        TexKind::Hair,
        TexKind::Fur,
        TexKind::Skin,
        TexKind::Leather,
        TexKind::Scales,
        TexKind::Wood,
        TexKind::Stone,
        TexKind::Grunge,
        TexKind::Crosshatch,
        TexKind::Dots,
        TexKind::Bark,
        TexKind::Sand,
        TexKind::Marble,
        TexKind::Water,
        TexKind::Brick,
        TexKind::Feathers,
        TexKind::SnakeSkin,
        TexKind::Foliage,
    ];
    pub fn label(self) -> &'static str {
        match self {
            TexKind::Paper => "Paper grain",
            TexKind::Watercolor => "Watercolor paper",
            TexKind::Canvas => "Canvas",
            TexKind::Cloth => "Cloth / linen",
            TexKind::Denim => "Denim",
            TexKind::Knit => "Knit / wool",
            TexKind::Hair => "Hair",
            TexKind::Fur => "Fur",
            TexKind::Skin => "Skin / pores",
            TexKind::Leather => "Leather",
            TexKind::Scales => "Scales",
            TexKind::Wood => "Wood grain",
            TexKind::Stone => "Stone / concrete",
            TexKind::Grunge => "Grunge",
            TexKind::Crosshatch => "Crosshatch",
            TexKind::Dots => "Halftone dots",
            TexKind::Bark => "Tree bark",
            TexKind::Sand => "Sand",
            TexKind::Marble => "Marble",
            TexKind::Water => "Water ripples",
            TexKind::Brick => "Brick wall",
            TexKind::Feathers => "Feathers",
            TexKind::SnakeSkin => "Snake skin",
            TexKind::Foliage => "Foliage",
        }
    }
    /// Section in the texture list.
    pub fn section(self) -> &'static str {
        match self {
            TexKind::Paper | TexKind::Watercolor | TexKind::Canvas | TexKind::Crosshatch | TexKind::Dots | TexKind::Grunge => "Basic",
            TexKind::Cloth | TexKind::Denim | TexKind::Knit | TexKind::Leather => "Cloth",
            TexKind::Hair | TexKind::Fur | TexKind::Skin => "Hair & body",
            TexKind::Scales | TexKind::Feathers | TexKind::SnakeSkin => "Creatures",
            TexKind::Wood | TexKind::Stone | TexKind::Bark | TexKind::Sand | TexKind::Marble | TexKind::Water | TexKind::Brick | TexKind::Foliage => {
                "Nature"
            }
        }
    }
    /// Hair and fur lie along the stroke (part of the tip); the rest is fixed to the canvas.
    pub fn directional(self) -> bool {
        matches!(self, TexKind::Hair | TexKind::Fur | TexKind::Feathers)
    }
}

/// Where a texture's colors come from when "keep texture colors" is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TexPalette {
    /// The texture's own natural colors (wood browns, snake greens…).
    Natural,
    /// Light and dark shades generated from your brush color.
    FromBrush,
}

/// Extra tip for dual / triple brushes: it masks, adds to, or cuts the main tip (like CSP's 2-brush tip).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SubTip {
    pub tip: Tip,
    pub shape_base: ShapeBase,
    pub scale: f32,        // size × main tip, 0.1..2
    pub scatter: f32,      // 0..1, moves around inside the main tip
    pub angle_jitter: f32, // 0..1
    pub mode: SubMode,
    pub strength: f32, // 0..1
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubMode {
    Mask,
    Add,
    Cut,
}

impl SubMode {
    pub fn label(self) -> &'static str {
        match self {
            SubMode::Mask => "Mask (only where both are)",
            SubMode::Add => "Add (both shapes)",
            SubMode::Cut => "Cut out",
        }
    }
}

impl Default for SubTip {
    fn default() -> Self {
        Self { tip: Tip::Chalk, shape_base: ShapeBase::Circle, scale: 0.7, scatter: 0.4, angle_jitter: 1.0, mode: SubMode::Mask, strength: 1.0 }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct BrushSettings {
    pub size: f32,
    pub opacity: f32,
    pub flow: f32,
    pub hardness: f32,
    pub spacing: f32,
    pub size_pressure: bool,
    pub min_size: f32,
    pub texture: f32,
    pub opacity_pressure: bool,
    pub stabilize: f32,
    pub ends: Ends,
    pub taper: f32,
    pub mix: f32,
    pub mix_refer: MixRefer,
    // real paint
    pub paint: Paint,
    pub dip: Dip,
    /// Pixels of stroke between dips (Dip::Every).
    pub dip_every: f32,
    // wet paint
    /// 0..1: thins the paint and leaves it wet.
    pub water: f32,
    /// 0..1: how far wet paint spreads into the paint around it.
    pub bleed: f32,
    /// 0..1: pigment pools darker at the edge of a wash.
    pub pool: f32,
    /// 0..1: pigment settles into the paper tooth.
    pub granulation: f32,
    /// An eraser that only dries wet paint (and lifts a little).
    pub blot: bool,
    // width behavior
    /// 0..1: how deep the stroke pinches in.
    pub pinch: f32,
    /// Pixels of stroke from one pinch to the next.
    pub pinch_every: f32,
    /// -1..1: bends the pressure response (negative = light touch is enough, positive = press harder).
    pub pressure_curve: f32,
    /// Width at the very start and end of a tapered stroke, as a share of the full width.
    pub tip_start: f32,
    pub tip_end: f32,
    // color along the stroke
    pub transition: Transition,
    pub trans_color: [f32; 3],
    /// Pixels over which the color goes from one to the other.
    pub trans_len: f32,
    pub trans_back: bool,
    /// -1..1: the paint runs out along the stroke: positive darkens, negative lightens.
    pub drift: f32,
    /// Pixels until the paint has run out.
    pub drift_len: f32,
    // dots
    /// 0..1: how much of the stroke is made of separate dots.
    pub dots: f32,
    pub dot_min: f32,
    pub dot_max: f32,
    pub dot_count: u32,
    /// How far from the line the dots land (x brush size).
    pub dot_spread: f32,
    /// 0..1: dots take colors between the brush color and the second color.
    pub dot_colors: f32,
    // lines and waves inside the stroke
    pub lines: u32,
    /// Sideways swing of each thread, as a share of the stroke's half width.
    pub line_wave: f32,
    /// Pixels of stroke for one wave.
    pub line_len: f32,
    /// Thread width, as a share of the stroke's half width.
    pub line_width: f32,
    pub line_strength: f32,
    /// -1..1: threads are darker (negative) or lighter (positive) than the stroke.
    pub line_shade: f32,
    // edge fray
    pub fray: Fray,
    pub fray_amount: f32,
    /// Pixels: size of the teeth, splits or lumps.
    pub fray_len: f32,
    // dashes
    /// Pixels painted, then `gap_len` pixels left empty (0 = no gaps).
    pub dash_len: f32,
    pub gap_len: f32,
    // tip and shape
    pub tip: Tip,
    pub roundness: f32,
    pub angle: f32, // degrees
    pub follow_direction: bool,
    pub scatter: f32,
    pub size_jitter: f32,
    pub angle_jitter: f32,
    pub opacity_jitter: f32,
    pub color_jitter: f32,
    // tip designer
    pub shape_base: ShapeBase,
    pub sides: u32,
    pub star_inner: f32,
    pub edge: Edge,
    pub edge_amount: f32,
    pub edge_detail: f32,
    pub wet_rim: f32,
    pub grain: f32,
    pub variants: u32,
    pub streaks: f32,
    pub antialias: bool,
    pub deform: f32,
    pub edge_holes: f32,
    pub hollow: f32,
    pub outline_width: f32,
    pub particles: u32,
    pub particle_size: f32,
    pub particle_gap: f32, // 0..1, keeps particles apart so they don't overlap or clump
    // texture
    pub tex_kind: TexKind,
    pub tex_scale: f32,
    pub tex_color: f32, // 0 = brush color only … 1 = the texture's own colors
    pub tex_palette: TexPalette,
    pub seed: u32,
    // dual / triple tips
    pub extra_tips: Vec<SubTip>,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            size: 10.0,
            opacity: 1.0,
            flow: 1.0,
            hardness: 0.85,
            spacing: 0.06,
            size_pressure: true,
            min_size: 0.5,
            texture: 0.0,
            opacity_pressure: false,
            stabilize: 0.2,
            ends: Ends::Round,
            taper: 0.3,
            mix: 0.0,
            mix_refer: MixRefer::All,
            paint: Paint::Normal,
            dip: Dip::Gradual,
            dip_every: 300.0,
            water: 0.0,
            bleed: 0.0,
            pool: 0.0,
            granulation: 0.0,
            blot: false,
            pinch: 0.0,
            pinch_every: 160.0,
            pressure_curve: 0.0,
            tip_start: 0.06,
            tip_end: 0.06,
            transition: Transition::Off,
            trans_color: [0.9, 0.35, 0.2],
            trans_len: 500.0,
            trans_back: false,
            drift: 0.0,
            drift_len: 700.0,
            dots: 0.0,
            dot_min: 0.08,
            dot_max: 0.3,
            dot_count: 3,
            dot_spread: 0.8,
            dot_colors: 0.0,
            lines: 0,
            line_wave: 0.25,
            line_len: 90.0,
            line_width: 0.08,
            line_strength: 0.7,
            line_shade: -0.5,
            fray: Fray::Off,
            fray_amount: 0.5,
            fray_len: 6.0,
            dash_len: 40.0,
            gap_len: 0.0,
            tip: Tip::Round,
            roundness: 1.0,
            angle: 0.0,
            follow_direction: false,
            scatter: 0.0,
            size_jitter: 0.0,
            angle_jitter: 0.0,
            opacity_jitter: 0.0,
            color_jitter: 0.0,
            shape_base: ShapeBase::Circle,
            sides: 5,
            star_inner: 0.45,
            edge: Edge::Smooth,
            edge_amount: 0.5,
            edge_detail: 0.5,
            wet_rim: 0.0,
            grain: 0.0,
            variants: 1,
            streaks: 0.0,
            antialias: true,
            deform: 0.0,
            edge_holes: 0.0,
            hollow: 0.0,
            outline_width: 0.18,
            particles: 0,
            particle_size: 0.25,
            particle_gap: 0.0,
            tex_kind: TexKind::Paper,
            tex_scale: 1.0,
            tex_color: 0.0,
            tex_palette: TexPalette::Natural,
            seed: 1,
            extra_tips: Vec::new(),
        }
    }
}

impl BrushSettings {
    /// Effects worked into the tip (edges, rim, grain, streaks, warp, in-tip texture, extra tips).
    pub fn has_edge_effects(&self) -> bool {
        self.edge != Edge::Smooth
            || self.wet_rim > 0.0
            || self.grain > 0.0
            || self.streaks > 0.0
            || self.deform > 0.0
            || self.dir_texture()
            || !self.extra_tips.is_empty()
    }

    /// Hair/fur textures live inside the tip and follow the stroke.
    pub fn dir_texture(&self) -> bool {
        self.texture > 0.0 && self.tex_kind.directional()
    }

    /// Looks that only make sense along the stroke turn with it.
    pub fn follows_stroke(&self) -> bool {
        self.follow_direction || self.dir_texture() || self.streaks > 0.0 || matches!(self.edge, Edge::Dry | Edge::Bristly)
    }

    /// Whole-stroke effects applied when the pen lifts (wet media edges, outline only, holes…).
    pub fn has_stroke_edges(&self) -> bool {
        if self.hollow > 0.0 || self.edge_holes > 0.0 {
            return true;
        }
        if self.spacing > 0.2 || self.scatter > 0.5 || self.particles > 0 || self.dots >= 0.5 {
            return false;
        }
        self.wet_rim > 0.0
            || self.fray != Fray::Off
            || matches!(self.edge, Edge::Ragged | Edge::Torn | Edge::Feathered | Edge::Bristly)
            || self.grain > 0.0
            || self.deform > 0.0
            || self.opacity_jitter > 0.0
    }
}

#[derive(Clone, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub category: &'static str,
    pub about: &'static str,
    pub s: BrushSettings,
}

pub const CATEGORIES: [&str; 11] = [
    "Pens",
    "Pencils",
    "Markers",
    "Paint",
    "Watercolor",
    "Airbrush",
    "Blend",
    "Textures",
    "Rough & particles",
    "Nature & FX",
    "My brushes",
];

/// The built-in brushes. Settings not mentioned keep their defaults.
pub fn presets() -> Vec<Preset> {
    use Edge::*;
    use Ends::{Flat, Taper};
    use ShapeBase as SB;
    use TexKind as TK;
    use Tip as T;
    let d = BrushSettings::default;
    let p = |name, category, about, s: BrushSettings| Preset { name, category, about, s };
    let layer = MixRefer::Layer;
    vec![
        // ---- Pens
        p("Pen", "Pens", "Ballpoint: even line with a light grain.", BrushSettings { size: 8.0, opacity: 0.95, flow: 0.9, hardness: 0.85, min_size: 0.65, opacity_pressure: true, texture: 0.18, stabilize: 0.3, ends: Taper, taper: 0.25, ..d() }),
        p("Ink", "Pens", "Sharp, clean lineart. Thin to thick with pen pressure.", BrushSettings { size: 4.0, hardness: 1.0, spacing: 0.04, min_size: 0.25, stabilize: 0.55, ends: Taper, taper: 0.5, ..d() }),
        p("G-pen", "Pens", "Manga G-pen: very thin to very thick, crisp ends.", BrushSettings { size: 10.0, hardness: 1.0, spacing: 0.03, min_size: 0.08, stabilize: 0.5, ends: Taper, taper: 0.6, ..d() }),
        p("Maru pen", "Pens", "Fine manga pen for small details and hair.", BrushSettings { size: 3.0, hardness: 1.0, spacing: 0.04, min_size: 0.3, stabilize: 0.45, ends: Taper, taper: 0.4, ..d() }),
        p("Gel pen", "Pens", "Smooth, solid and even.", BrushSettings { size: 6.0, hardness: 0.95, min_size: 0.75, stabilize: 0.3, ..d() }),
        p("Fineliner", "Pens", "Same width everywhere, like a technical pen.", BrushSettings { size: 4.0, hardness: 0.95, size_pressure: false, min_size: 1.0, stabilize: 0.4, ..d() }),
        p("Brush pen", "Pens", "Soft brush tip with ink: expressive thick-thin lines.", BrushSettings { size: 18.0, hardness: 0.95, spacing: 0.03, min_size: 0.08, stabilize: 0.4, ends: Taper, taper: 0.7, texture: 0.05, ..d() }),
        p("Dip pen", "Pens", "Steel nib: very thin to bold with pressure, slightly rough ink edge.", BrushSettings { size: 9.0, hardness: 1.0, edge: Ragged, edge_amount: 0.15, edge_detail: 0.9, variants: 3, spacing: 0.025, min_size: 0.03, stabilize: 0.5, ends: Taper, taper: 0.65, ..d() }),
        p("Double line pen", "Pens", "Draws two clean parallel lines at once (outline only).", BrushSettings { size: 16.0, hardness: 1.0, hollow: 1.0, outline_width: 0.12, spacing: 0.03, size_pressure: false, min_size: 1.0, stabilize: 0.4, ..d() }),
        p("Sumi ink brush", "Pens", "Japanese ink brush: wet, dry-brush ends, rich blacks.", BrushSettings { size: 30.0, tip: T::Bristle, roundness: 0.75, follow_direction: true, edge: Dry, edge_amount: 0.35, edge_detail: 0.85, streaks: 0.4, variants: 4, spacing: 0.03, min_size: 0.1, ends: Taper, taper: 0.6, stabilize: 0.3, ..d() }),
        p("Calligraphy", "Pens", "Flat nib held at 45°: thin and thick depending on direction.", BrushSettings { size: 22.0, hardness: 1.0, spacing: 0.02, min_size: 0.6, roundness: 0.18, angle: 45.0, stabilize: 0.35, ..d() }),
        p("Anime lineart", "Pens", "Clean manga/anime line art: sharp, strong taper, steady.", BrushSettings { size: 5.0, hardness: 1.0, spacing: 0.025, min_size: 0.05, stabilize: 0.6, ends: Taper, taper: 0.7, ..d() }),
        p("Hair strands", "Pens", "Long thin strands for hair: very pointed ends, light pressure stays thin.", BrushSettings { size: 6.0, hardness: 1.0, spacing: 0.025, min_size: 0.02, stabilize: 0.45, ends: Taper, taper: 0.9, opacity_pressure: true, ..d() }),
        p("Quill feather", "Pens", "Real goose-quill pen: scratchy ink that runs out and catches the paper.", BrushSettings { size: 7.0, hardness: 1.0, roundness: 0.35, angle: 30.0, edge: Torn, edge_amount: 0.2, edge_detail: 0.95, variants: 4, opacity_jitter: 0.35, texture: 0.25, spacing: 0.02, min_size: 0.15, ends: Taper, taper: 0.5, stabilize: 0.35, ..d() }),
        // ---- Pencils
        p("Pencil", "Pencils", "HB pencil for sketching. Press harder for darker lines.", BrushSettings { size: 6.0, hardness: 0.7, min_size: 0.5, opacity_pressure: true, texture: 0.45, stabilize: 0.1, ..d() }),
        p("Soft pencil 2B", "Pencils", "Softer, darker, grainier.", BrushSettings { size: 9.0, hardness: 0.55, min_size: 0.45, opacity_pressure: true, texture: 0.4, stabilize: 0.1, ..d() }),
        p("Dark pencil 6B", "Pencils", "Very soft and dark, good for shading.", BrushSettings { size: 14.0, hardness: 0.45, min_size: 0.4, opacity_pressure: true, texture: 0.38, stabilize: 0.05, ..d() }),
        p("Mechanical", "Pencils", "Thin, clean, even pencil line.", BrushSettings { size: 2.5, hardness: 0.8, min_size: 0.8, opacity_pressure: true, texture: 0.3, stabilize: 0.2, ..d() }),
        p("Colored pencil", "Pencils", "Waxy colored pencil with paper grain.", BrushSettings { size: 7.0, hardness: 0.6, min_size: 0.6, opacity_pressure: true, texture: 0.55, stabilize: 0.1, opacity: 0.9, ..d() }),
        p("Charcoal", "Pencils", "Rough, powdery and dark, with a broken edge.", BrushSettings { size: 24.0, tip: T::Chalk, hardness: 0.6, roundness: 0.6, follow_direction: true, edge: Torn, edge_amount: 0.4, edge_detail: 0.7, grain: 0.25, variants: 4, spacing: 0.08, min_size: 0.5, opacity_pressure: true, texture: 0.6, stabilize: 0.05, ..d() }),
        p("Pastel", "Pencils", "Soft chalk pastel that catches the paper texture.", BrushSettings { size: 30.0, tip: T::Chalk, hardness: 0.6, edge: Torn, edge_amount: 0.3, edge_detail: 0.5, grain: 0.3, variants: 4, angle_jitter: 1.0, spacing: 0.08, min_size: 0.6, opacity_pressure: true, texture: 0.5, mix: 0.1, opacity: 0.9, ..d() }),
        p("Wax crayon", "Pencils", "Waxy crayon that skips over the paper grain.", BrushSettings { size: 18.0, tip: T::Chalk, hardness: 0.7, edge: Torn, edge_amount: 0.3, grain: 0.35, variants: 4, opacity: 0.95, spacing: 0.06, min_size: 0.7, texture: 0.65, tex_kind: TK::Canvas, tex_scale: 0.6, ..d() }),
        p("Rough pencil", "Pencils", "Big, loose sketching pencil with a rough edge.", BrushSettings { size: 14.0, tip: T::Chalk, hardness: 0.6, edge: Torn, edge_amount: 0.35, edge_detail: 0.8, grain: 0.25, variants: 4, min_size: 0.4, opacity_pressure: true, texture: 0.5, stabilize: 0.05, ..d() }),
        p("Silverpoint", "Pencils", "Old-master metal stylus: very fine, faint gray-brown lines that build up slowly.", BrushSettings { size: 2.0, hardness: 0.9, opacity: 0.55, flow: 0.6, min_size: 0.7, opacity_pressure: true, texture: 0.35, stabilize: 0.25, ..d() }),
        // ---- Markers
        p("Marker", "Markers", "Flat, see-through color that does not get darker where it overlaps itself.", BrushSettings { size: 30.0, opacity: 0.5, hardness: 0.85, size_pressure: false, min_size: 1.0, ends: Flat, ..d() }),
        p("Highlighter", "Markers", "Wide chisel tip, see-through.", BrushSettings { size: 34.0, opacity: 0.4, hardness: 0.95, size_pressure: false, min_size: 1.0, roundness: 0.3, ends: Flat, stabilize: 0.3, ..d() }),
        p("Alcohol marker", "Markers", "Soft-edged marker that blends a little with what is under it.", BrushSettings { size: 26.0, opacity: 0.65, flow: 0.5, hardness: 0.55, min_size: 0.7, mix: 0.12, opacity_pressure: true, ..d() }),
        p("Outline marker", "Markers", "Hollow marker: a colored outline around an empty middle.", BrushSettings { size: 30.0, hardness: 0.95, hollow: 1.0, outline_width: 0.2, size_pressure: false, min_size: 1.0, stabilize: 0.3, ..d() }),
        // ---- Paint
        p("Oil paint", "Paint", "Thick paint with bristle marks; mixes with the paint already there.", BrushSettings { size: 40.0, tip: T::Shape, shape_base: SB::Rounded, roundness: 0.35, angle: 90.0, streaks: 0.6, follow_direction: true, edge: Bristly, edge_amount: 0.35, edge_detail: 0.6, variants: 3, spacing: 0.05, flow: 0.9, mix: 0.45, min_size: 0.5, opacity_pressure: true, stabilize: 0.1, ..d() }),
        p("Acrylic", "Paint", "Opaque paint with a little bristle texture.", BrushSettings { size: 34.0, tip: T::Shape, shape_base: SB::Square, roundness: 0.3, angle: 90.0, streaks: 0.4, follow_direction: true, spacing: 0.05, mix: 0.2, min_size: 0.5, texture: 0.15, stabilize: 0.1, ..d() }),
        p("Gouache", "Paint", "Matte, flat and opaque, slightly chalky, with soft uneven edges.", BrushSettings { size: 30.0, hardness: 0.75, edge: Ragged, edge_amount: 0.2, edge_detail: 0.5, variants: 4, angle_jitter: 1.0, mix: 0.15, min_size: 0.5, texture: 0.15, flow: 0.9, ..d() }),
        p("Oil flat", "Paint", "Flat bristle brush: square strokes with bristle marks.", BrushSettings { size: 45.0, tip: T::Shape, shape_base: SB::Square, roundness: 0.3, angle: 90.0, follow_direction: true, hardness: 0.9, edge: Bristly, edge_amount: 0.4, edge_detail: 0.7, streaks: 0.7, variants: 3, mix: 0.4, mix_refer: layer, flow: 0.95, spacing: 0.04, min_size: 0.6, ..d() }),
        p("Oil filbert", "Paint", "Rounded flat brush for soft-edged oil strokes.", BrushSettings { size: 45.0, tip: T::Shape, shape_base: SB::Drop, roundness: 0.45, angle: 90.0, follow_direction: true, hardness: 0.85, edge: Ragged, edge_amount: 0.25, streaks: 0.5, variants: 3, mix: 0.35, mix_refer: layer, flow: 0.95, spacing: 0.04, min_size: 0.6, ..d() }),
        p("Oil fan", "Paint", "Fan brush: thin bristle streaks for grass, hair and texture.", BrushSettings { size: 60.0, tip: T::Bristle, roundness: 0.15, angle: 90.0, follow_direction: true, edge: Dry, edge_amount: 0.5, edge_detail: 0.9, streaks: 0.8, variants: 3, mix: 0.25, mix_refer: layer, flow: 0.9, spacing: 0.04, min_size: 0.7, ..d() }),
        p("Palette knife", "Paint", "Thick paint scraped on with a knife: hard edges, smeared color.", BrushSettings { size: 50.0, tip: T::Shape, shape_base: SB::Diamond, roundness: 0.35, angle: 90.0, follow_direction: true, hardness: 1.0, edge: Torn, edge_amount: 0.25, edge_detail: 0.6, variants: 3, mix: 0.6, mix_refer: layer, flow: 1.0, spacing: 0.03, size_pressure: false, min_size: 1.0, ..d() }),
        p("Impasto", "Paint", "Heavy, rich oil paint with deep bristle grooves.", BrushSettings { size: 50.0, tip: T::Bristle, roundness: 0.6, follow_direction: true, edge: Bristly, edge_amount: 0.5, edge_detail: 0.6, streaks: 0.9, grain: 0.1, variants: 4, mix: 0.3, mix_refer: layer, flow: 1.0, spacing: 0.035, min_size: 0.6, ..d() }),
        p("Gouache flat", "Paint", "Flat, matte gouache strokes with a chalky dry edge.", BrushSettings { size: 40.0, tip: T::Shape, shape_base: SB::Rounded, roundness: 0.35, angle: 90.0, follow_direction: true, hardness: 0.8, edge: Dry, edge_amount: 0.3, edge_detail: 0.7, variants: 3, mix: 0.15, mix_refer: layer, flow: 0.95, spacing: 0.04, min_size: 0.7, texture: 0.2, ..d() }),
        p("Dry gouache", "Paint", "Scratchy, broken gouache from a nearly dry brush.", BrushSettings { size: 40.0, hardness: 0.85, edge: Dry, edge_amount: 0.6, edge_detail: 0.8, grain: 0.3, roundness: 0.6, follow_direction: true, variants: 4, mix: 0.1, mix_refer: layer, flow: 0.95, spacing: 0.04, min_size: 0.7, texture: 0.35, ..d() }),
        p("Outlined gouache", "Paint", "Opaque paint with an empty middle: a thick outlined stroke.", BrushSettings { size: 40.0, tip: T::Chalk, roundness: 0.8, hardness: 0.85, hollow: 0.85, outline_width: 0.22, edge: Ragged, edge_amount: 0.35, edge_detail: 0.7, variants: 4, flow: 0.9, spacing: 0.05, min_size: 0.6, texture: 0.15, ..d() }),
        p("Real oil", "Paint", "Oil that mixes like real pigment: blue into yellow makes a living green, and the brush carries the paint along.", BrushSettings { size: 40.0, tip: T::Shape, shape_base: SB::Rounded, roundness: 0.35, angle: 90.0, streaks: 0.5, follow_direction: true, edge: Bristly, edge_amount: 0.35, edge_detail: 0.6, variants: 3, spacing: 0.05, flow: 0.9, mix: 0.5, paint: Paint::Mix, dip: Dip::Once, min_size: 0.5, opacity_pressure: true, stabilize: 0.1, ..d() }),
        p("Real gouache", "Paint", "Opaque gouache that blends like pigment where strokes overlap; dipped fresh every so often.", BrushSettings { size: 32.0, hardness: 0.8, edge: Ragged, edge_amount: 0.2, edge_detail: 0.5, variants: 4, angle_jitter: 1.0, mix: 0.3, paint: Paint::Mix, dip: Dip::Every, dip_every: 260.0, min_size: 0.5, texture: 0.15, ..d() }),
        p("Glaze", "Paint", "A transparent glaze: lay it over dry paint and the colors deepen, as with thin oil or watercolor washes.", BrushSettings { size: 50.0, hardness: 0.6, opacity: 0.55, flow: 0.7, paint: Paint::Glaze, min_size: 0.6, opacity_pressure: true, ..d() }),
        p("Fuzzy bristle", "Paint", "Dense, hairy bristle strokes with fibrous, fuzzy edges.", BrushSettings { size: 60.0, tip: T::Bristle, roundness: 0.7, follow_direction: true, hardness: 0.9, edge: Bristly, edge_amount: 0.85, edge_detail: 0.8, deform: 0.25, streaks: 0.4, variants: 6, flow: 0.85, spacing: 0.04, mix: 0.15, mix_refer: layer, min_size: 0.5, ..d() }),
        p("Dry brush", "Paint", "Streaky paint with gaps, like a brush running out of paint.", BrushSettings { size: 44.0, hardness: 0.85, roundness: 0.55, follow_direction: true, edge: Dry, edge_amount: 0.55, edge_detail: 0.6, variants: 4, spacing: 0.04, mix: 0.15, min_size: 0.6, texture: 0.2, ..d() }),
        p("Paint mix", "Paint", "Wet paint: your color mixes with the colors already on the canvas.", BrushSettings { size: 40.0, hardness: 0.5, spacing: 0.08, flow: 0.8, min_size: 0.4, opacity_pressure: true, mix: 0.5, stabilize: 0.1, ..d() }),
        p("Cel shade", "Paint", "Flat anime coloring and shadow shapes: hard edge, no texture, fills cleanly.", BrushSettings { size: 40.0, hardness: 1.0, spacing: 0.04, size_pressure: false, min_size: 1.0, stabilize: 0.25, ..d() }),
        p("Flat chisel", "Paint", "Square flat brush for buildings, blocks and hard-edged shapes.", BrushSettings { size: 40.0, tip: T::Shape, shape_base: SB::Square, roundness: 0.35, hardness: 0.95, edge: Dry, edge_amount: 0.25, edge_detail: 0.7, variants: 3, streaks: 0.25, spacing: 0.04, min_size: 0.6, mix: 0.1, stabilize: 0.15, ..d() }),
        p("Gloss highlight", "Paint", "Small bright highlights for eyes, hair shine, glass and metal.", BrushSettings { size: 10.0, hardness: 0.9, spacing: 0.04, min_size: 0.1, ends: Taper, taper: 0.6, stabilize: 0.35, ..d() }),
        p("Sea sponge", "Paint", "Dab a natural sea sponge: holey, organic texture for foliage, rust and stone.", BrushSettings { size: 70.0, tip: T::Sponge, hardness: 0.9, spacing: 0.35, angle_jitter: 1.0, size_jitter: 0.25, variants: 6, opacity_jitter: 0.3, size_pressure: false, min_size: 1.0, stabilize: 0.0, ..d() }),
        p("Toothbrush spatter", "Paint", "Flick paint off a toothbrush: a fine mist of tiny drops (stars, snow, dirt).", BrushSettings { size: 120.0, particles: 30, particle_size: 0.03, hardness: 0.95, spacing: 0.6, scatter: 0.5, opacity_jitter: 0.6, size_pressure: false, min_size: 1.0, stabilize: 0.0, ..d() }),
        // ---- Watercolor
        p("Watercolor", "Watercolor", "See-through paint with uneven, darker wet edges and paper grain.", BrushSettings { size: 50.0, hardness: 0.7, edge: Ragged, edge_amount: 0.65, edge_detail: 0.35, wet_rim: 0.7, variants: 6, angle_jitter: 1.0, opacity: 0.75, flow: 0.3, spacing: 0.08, min_size: 0.5, opacity_pressure: true, texture: 0.3, stabilize: 0.1, ..d() }),
        p("Watercolor wash", "Watercolor", "Big soft wash for skies and backgrounds.", BrushSettings { size: 140.0, hardness: 0.4, edge: Feathered, edge_amount: 0.6, edge_detail: 0.2, wet_rim: 0.45, variants: 5, angle_jitter: 1.0, opacity: 0.6, flow: 0.15, size_pressure: false, min_size: 1.0, texture: 0.25, ..d() }),
        p("Watercolor bloom", "Watercolor", "Drops of water spreading into paint: blotchy cauliflower edges.", BrushSettings { size: 70.0, hardness: 0.6, edge: Ragged, edge_amount: 0.6, edge_detail: 0.7, wet_rim: 0.8, variants: 8, angle_jitter: 1.0, size_jitter: 0.3, scatter: 0.25, opacity: 0.7, flow: 0.5, spacing: 0.3, min_size: 0.6, texture: 0.3, ..d() }),
        p("Transparent watercolor", "Watercolor", "Clear, even glaze with soft edges and no rim: builds up where layers overlap.", BrushSettings { size: 50.0, hardness: 0.45, edge: Feathered, edge_amount: 0.35, edge_detail: 0.4, variants: 3, opacity: 0.45, flow: 0.5, spacing: 0.06, min_size: 0.6, opacity_pressure: true, texture: 0.1, tex_kind: TK::Watercolor, ..d() }),
        p("Dark rim watercolor", "Watercolor", "Strong dark edge where the water dried, light middle.", BrushSettings { size: 55.0, hardness: 0.7, edge: Ragged, edge_amount: 0.5, edge_detail: 0.4, wet_rim: 1.0, variants: 5, angle_jitter: 1.0, opacity: 0.8, flow: 0.45, spacing: 0.07, min_size: 0.6, texture: 0.2, ..d() }),
        p("Wet watercolor", "Watercolor", "A real wet wash: colors bleed into each other while wet, the edge darkens as it dries, pigment settles into the paper.", BrushSettings { size: 60.0, hardness: 0.35, opacity: 0.75, flow: 0.6, spacing: 0.06, paint: Paint::Mix, water: 0.8, bleed: 0.65, pool: 0.7, granulation: 0.5, tex_kind: TexKind::Watercolor, texture: 0.2, min_size: 0.6, opacity_pressure: true, ..d() }),
        p("Wet ink wash", "Watercolor", "Thin ink in water: soft clouds that spread into each other and pool at the edges.", BrushSettings { size: 70.0, hardness: 0.3, opacity: 0.6, flow: 0.55, paint: Paint::Mix, water: 0.9, bleed: 0.85, pool: 0.4, granulation: 0.2, tex_kind: TexKind::Watercolor, min_size: 0.5, opacity_pressure: true, ..d() }),
        p("Granulating wash", "Watercolor", "A dry-ish wash where the pigment collects in the valleys of rough paper.", BrushSettings { size: 55.0, hardness: 0.5, opacity: 0.8, flow: 0.65, paint: Paint::Mix, water: 0.45, bleed: 0.2, pool: 0.3, granulation: 1.0, tex_kind: TexKind::Watercolor, texture: 0.35, tex_scale: 1.2, min_size: 0.6, opacity_pressure: true, ..d() }),
        p("Wet blending watercolor", "Watercolor", "Wet into wet: blends softly with the color already there.", BrushSettings { size: 60.0, hardness: 0.35, edge: Feathered, edge_amount: 0.7, edge_detail: 0.3, wet_rim: 0.3, variants: 4, angle_jitter: 1.0, opacity: 0.7, flow: 0.45, spacing: 0.08, mix: 0.55, mix_refer: layer, min_size: 0.6, opacity_pressure: true, ..d() }),
        p("Dry watercolor", "Watercolor", "Little water: broken, grainy strokes that catch the paper.", BrushSettings { size: 45.0, hardness: 0.8, edge: Dry, edge_amount: 0.55, edge_detail: 0.7, grain: 0.35, variants: 4, roundness: 0.7, follow_direction: true, opacity: 0.8, flow: 0.55, spacing: 0.05, min_size: 0.6, texture: 0.45, tex_kind: TK::Watercolor, ..d() }),
        p("Granulating watercolor", "Watercolor", "Pigment settles into the paper grain (like ultramarine).", BrushSettings { size: 60.0, hardness: 0.55, edge: Ragged, edge_amount: 0.45, edge_detail: 0.35, wet_rim: 0.6, grain: 0.45, variants: 5, angle_jitter: 1.0, opacity: 0.75, flow: 0.4, spacing: 0.07, min_size: 0.6, texture: 0.55, tex_kind: TK::Watercolor, tex_scale: 0.8, ..d() }),
        p("Flat wash brush", "Watercolor", "Wide flat brush for even washes and straight edges.", BrushSettings { size: 70.0, tip: T::Shape, shape_base: SB::Rounded, roundness: 0.28, angle: 90.0, follow_direction: true, hardness: 0.75, edge: Ragged, edge_amount: 0.3, edge_detail: 0.5, wet_rim: 0.55, variants: 4, opacity: 0.7, flow: 0.4, spacing: 0.05, min_size: 0.8, texture: 0.2, ..d() }),
        p("Watercolor liner", "Watercolor", "Thin pointed rigger brush for twigs, hair and details.", BrushSettings { size: 10.0, hardness: 0.85, edge: Ragged, edge_amount: 0.25, edge_detail: 0.8, wet_rim: 0.35, variants: 3, opacity: 0.85, flow: 0.6, spacing: 0.04, min_size: 0.08, ends: Taper, taper: 0.7, stabilize: 0.35, ..d() }),
        p("Bleeding watercolor", "Watercolor", "Paint bleeds out into the wet paper: soft, warped, uneven edges.", BrushSettings { size: 70.0, hardness: 0.45, edge: Feathered, edge_amount: 0.85, edge_detail: 0.25, wet_rim: 0.55, deform: 0.5, variants: 6, angle_jitter: 1.0, opacity: 0.7, flow: 0.35, spacing: 0.08, min_size: 0.6, ..d() }),
        p("Mottled wash", "Watercolor", "Patchy, cloudy wash for skies, walls and backgrounds.", BrushSettings { size: 140.0, tip: T::Cloud, hardness: 0.5, edge: Ragged, edge_amount: 0.6, edge_detail: 0.3, wet_rim: 0.5, deform: 0.4, variants: 6, angle_jitter: 1.0, size_jitter: 0.3, opacity: 0.6, flow: 0.3, spacing: 0.12, size_pressure: false, min_size: 1.0, texture: 0.3, tex_kind: TK::Watercolor, ..d() }),
        p("Dried ring watercolor", "Watercolor", "Only the dried rim of a puddle stays: a ragged ring of paint around a pale middle.", BrushSettings { size: 60.0, hardness: 0.7, edge: Ragged, edge_amount: 0.6, edge_detail: 0.45, wet_rim: 0.4, hollow: 0.7, outline_width: 0.14, variants: 6, angle_jitter: 1.0, opacity: 0.8, flow: 0.45, spacing: 0.07, min_size: 0.6, texture: 0.2, tex_kind: TK::Watercolor, ..d() }),
        p("Salt watercolor", "Watercolor", "Salt sprinkled into wet paint: speckled, starry light spots.", BrushSettings { size: 70.0, hardness: 0.55, edge: Ragged, edge_amount: 0.5, edge_detail: 0.3, wet_rim: 0.5, grain: 0.6, variants: 5, angle_jitter: 1.0, opacity: 0.8, flow: 0.45, spacing: 0.07, min_size: 0.6, texture: 0.45, tex_kind: TK::Stone, ..d() }),
        p("Watercolor splatter", "Watercolor", "Drops and spatters of watery paint with dark rims.", BrushSettings { size: 80.0, tip: T::Splatter, hardness: 0.8, edge: Ragged, edge_amount: 0.4, wet_rim: 0.7, variants: 6, angle_jitter: 1.0, size_jitter: 0.6, scatter: 1.0, opacity_jitter: 0.4, opacity: 0.8, flow: 0.6, spacing: 0.7, size_pressure: false, min_size: 1.0, stabilize: 0.0, ..d() }),
        p("Watercolor dots", "Watercolor", "Separate wet dots and blots, each with its own rim.", BrushSettings { size: 60.0, particles: 7, particle_size: 0.22, hardness: 0.7, edge: Ragged, edge_amount: 0.6, wet_rim: 0.8, variants: 6, angle_jitter: 1.0, opacity_jitter: 0.4, opacity: 0.8, flow: 0.6, spacing: 0.5, scatter: 0.3, size_pressure: false, min_size: 1.0, stabilize: 0.0, ..d() }),
        // ---- Airbrush
        p("Soft round", "Airbrush", "Soft edges for shading and smooth color.", BrushSettings { size: 60.0, flow: 0.35, hardness: 0.2, spacing: 0.08, min_size: 0.2, opacity_pressure: true, stabilize: 0.0, ..d() }),
        p("Airbrush", "Airbrush", "Very soft spray that builds up slowly.", BrushSettings { size: 150.0, flow: 0.06, hardness: 0.0, size_pressure: false, min_size: 1.0, opacity_pressure: true, stabilize: 0.0, ..d() }),
        p("Hard airbrush", "Airbrush", "Builds up slowly but keeps a firm edge.", BrushSettings { size: 60.0, flow: 0.12, hardness: 0.65, size_pressure: false, min_size: 1.0, opacity_pressure: true, stabilize: 0.0, ..d() }),
        p("Grainy airbrush", "Airbrush", "Soft spray with visible grain, for gritty shading.", BrushSettings { size: 120.0, flow: 0.15, hardness: 0.1, grain: 0.5, variants: 2, spacing: 0.08, size_pressure: false, min_size: 1.0, opacity_pressure: true, texture: 0.35, stabilize: 0.0, ..d() }),
        p("Spray paint", "Airbrush", "Grainy spray can.", BrushSettings { size: 90.0, tip: T::Splatter, flow: 0.35, spacing: 0.15, scatter: 0.4, size_jitter: 0.4, angle_jitter: 1.0, size_pressure: false, min_size: 1.0, opacity_pressure: true, stabilize: 0.0, ..d() }),
        p("Bark", "Nature & FX", "Wavy threads run inside the stroke: tree bark, wood grain, rope.", BrushSettings { size: 46.0, hardness: 0.8, lines: 6, line_wave: 0.3, line_len: 80.0, line_width: 0.09, line_strength: 0.75, line_shade: -0.6, fray: Fray::Knotted, fray_amount: 0.45, fray_len: 7.0, stabilize: 0.25, min_size: 0.8, ..d() }),
        p("Leopard dots", "Nature & FX", "Big soft dots in several colors: animal fur patterns.", BrushSettings { size: 90.0, hardness: 0.55, spacing: 0.5, dots: 1.0, dot_min: 0.12, dot_max: 0.38, dot_count: 2, dot_spread: 1.1, dot_colors: 0.9, trans_color: [0.25, 0.15, 0.08], transition: Transition::Off, size_pressure: false, stabilize: 0.0, ..d() }),
        p("Skin pores", "Nature & FX", "Tiny scattered dots, soft and faint: pores, freckles, sand.", BrushSettings { size: 60.0, hardness: 0.5, opacity: 0.6, flow: 0.5, spacing: 0.3, dots: 1.0, dot_min: 0.015, dot_max: 0.05, dot_count: 7, dot_spread: 1.0, dot_colors: 0.5, trans_color: [0.55, 0.3, 0.25], size_pressure: false, stabilize: 0.0, ..d() }),
        p("Running dry", "Pens", "Starts dark and full, then the paint runs out: warmer, lighter, thinner.", BrushSettings { size: 26.0, drift: -0.9, drift_len: 450.0, min_size: 0.5, opacity_pressure: true, ..d() }),
        p("Color ribbon", "Pens", "The color glides to a second color and back along the stroke.", BrushSettings { size: 34.0, hardness: 0.9, transition: Transition::Cycle, trans_color: [0.15, 0.65, 0.95], trans_len: 420.0, min_size: 0.7, ..d() }),
        p("Speed ink", "Pens", "Slow strokes keep your color; fast strokes lean toward the second color.", BrushSettings { size: 22.0, hardness: 0.9, transition: Transition::Speed, trans_color: [0.9, 0.2, 0.3], min_size: 0.6, ..d() }),
        p("Bamboo", "Pens", "A line that pinches in at regular points, like a bamboo stem or a bead chain.", BrushSettings { size: 36.0, hardness: 0.9, pinch: 0.85, pinch_every: 110.0, size_pressure: false, stabilize: 0.2, ..d() }),
        p("Dashed pen", "Pens", "A dashed line: set the dash and gap under Dashes.", BrushSettings { size: 8.0, hardness: 0.95, gap_len: 22.0, dash_len: 36.0, size_pressure: false, stabilize: 0.4, ..d() }),
        p("Serrated pen", "Pens", "An edge with tiny regular teeth.", BrushSettings { size: 30.0, hardness: 0.95, fray: Fray::Serrated, fray_amount: 0.7, fray_len: 5.0, ..d() }),
        p("Split brush", "Paint", "A dry brush with split hairs: long splits run along the stroke.", BrushSettings { size: 56.0, hardness: 0.8, fray: Fray::Split, fray_amount: 0.85, fray_len: 5.0, stabilize: 0.3, ..d() }),
        p("Worn chalk", "Pencils", "Lumpy, bitten, broken edges like a worn piece of chalk.", BrushSettings { size: 34.0, hardness: 0.85, fray: Fray::Knotted, fray_amount: 0.8, fray_len: 4.0, texture: 0.25, ..d() }),
        p("Splatter", "Airbrush", "Random paint drops.", BrushSettings { size: 70.0, tip: T::Splatter, spacing: 0.7, scatter: 1.4, size_jitter: 0.8, angle_jitter: 1.0, opacity_jitter: 0.3, min_size: 0.4, stabilize: 0.0, ..d() }),
        p("Soft blush", "Airbrush", "Gentle color for cheeks, glow and soft shading in anime art.", BrushSettings { size: 80.0, flow: 0.12, hardness: 0.1, spacing: 0.08, size_pressure: false, min_size: 1.0, opacity_pressure: true, opacity: 0.7, stabilize: 0.0, ..d() }),
        // ---- Blend
        p("Blender", "Blend", "Adds no color. Smears and blends the colors already there.", BrushSettings { size: 50.0, flow: 0.6, hardness: 0.3, spacing: 0.08, min_size: 0.4, opacity_pressure: true, mix: 1.0, stabilize: 0.1, ..d() }),
        p("Smudge", "Blend", "Pushes paint along like a finger.", BrushSettings { size: 36.0, flow: 0.9, hardness: 0.5, spacing: 0.05, min_size: 0.5, mix: 0.95, stabilize: 0.05, ..d() }),
        p("Bristle blender", "Blend", "Blends with brush marks, like a dry brush.", BrushSettings { size: 46.0, tip: T::Bristle, roundness: 0.6, follow_direction: true, flow: 0.7, spacing: 0.05, mix: 0.9, min_size: 0.5, ..d() }),
        p("Hair blend", "Blend", "Pulls the colors already there into fine hair strands along your stroke.", BrushSettings { size: 40.0, tip: T::Strands, hardness: 1.0, follow_direction: true, flow: 0.75, spacing: 0.04, mix: 1.0, min_size: 0.5, opacity_pressure: true, ends: Taper, taper: 0.5, stabilize: 0.15, ..d() }),
        p("Fur blend", "Blend", "Fluffs the edge between two colors into short fur.", BrushSettings { size: 34.0, tip: T::Strands, hardness: 1.0, follow_direction: true, flow: 0.8, spacing: 0.08, scatter: 0.25, angle_jitter: 0.1, size_jitter: 0.3, mix: 1.0, min_size: 0.5, ends: Taper, taper: 0.4, stabilize: 0.0, ..d() }),
        p("Finger tip", "Blend", "Smooth finger smear: drags paint softly in the stroke direction.", BrushSettings { size: 30.0, flow: 0.95, hardness: 0.35, spacing: 0.03, mix: 0.97, min_size: 0.6, opacity_pressure: true, stabilize: 0.1, ..d() }),
        p("Textured blend", "Blend", "Blends with a grainy chalk texture, keeps a painterly look.", BrushSettings { size: 50.0, tip: T::Chalk, flow: 0.6, hardness: 0.6, spacing: 0.06, grain: 0.25, angle_jitter: 1.0, variants: 4, mix: 0.95, min_size: 0.5, opacity_pressure: true, ..d() }),
        p("Soft blur", "Blend", "Very soft blending that just melts edges together.", BrushSettings { size: 80.0, flow: 0.3, hardness: 0.0, spacing: 0.08, mix: 1.0, size_pressure: false, min_size: 1.0, opacity_pressure: true, ..d() }),
        p("Water blend", "Blend", "Softly blends and lightens, like a wet brush.", BrushSettings { size: 60.0, tip: T::Wet, flow: 0.4, hardness: 0.3, mix: 0.85, min_size: 0.5, texture: 0.15, ..d() }),
        // ---- Textures
        p("Hair texture", "Textures", "Paints with fine hair strands inside: shade hair quickly.", BrushSettings { size: 60.0, hardness: 0.7, flow: 0.55, texture: 0.95, tex_kind: TK::Hair, min_size: 0.6, opacity_pressure: true, ends: Taper, taper: 0.4, stabilize: 0.15, ..d() }),
        p("Fur texture", "Textures", "Short fur texture for animals and fluffy clothes.", BrushSettings { size: 60.0, hardness: 0.6, flow: 0.55, texture: 0.9, tex_kind: TK::Fur, scatter: 0.1, angle_jitter: 0.04, min_size: 0.6, opacity_pressure: true, ends: Taper, ..d() }),
        p("Skin texture", "Textures", "Soft color with subtle pores and mottling for realistic skin.", BrushSettings { size: 70.0, hardness: 0.2, flow: 0.35, texture: 0.45, tex_kind: TK::Skin, min_size: 0.7, opacity_pressure: true, ..d() }),
        p("Cloth / linen", "Textures", "Woven fabric texture for clothes.", BrushSettings { size: 50.0, hardness: 0.75, flow: 0.8, texture: 0.65, tex_kind: TK::Cloth, min_size: 0.7, ..d() }),
        p("Denim", "Textures", "Diagonal twill texture for jeans and jackets.", BrushSettings { size: 50.0, hardness: 0.75, flow: 0.8, texture: 0.6, tex_kind: TK::Denim, min_size: 0.7, ..d() }),
        p("Knit wool", "Textures", "Knitted sweater texture.", BrushSettings { size: 50.0, hardness: 0.7, flow: 0.8, texture: 0.65, tex_kind: TK::Knit, min_size: 0.7, ..d() }),
        p("Leather", "Textures", "Cracked leather grain.", BrushSettings { size: 50.0, hardness: 0.7, flow: 0.8, texture: 0.55, tex_kind: TK::Leather, min_size: 0.7, ..d() }),
        p("Scales", "Textures", "Dragon, fish and snake scales with clear gaps, so every scale reads on its own.", BrushSettings { size: 80.0, hardness: 0.8, flow: 0.95, texture: 1.0, tex_kind: TK::Scales, tex_scale: 1.5, min_size: 0.8, ..d() }),
        p("Wood grain", "Textures", "Wood grain for furniture, floors and staffs.", BrushSettings { size: 50.0, hardness: 0.7, flow: 0.8, texture: 0.6, tex_kind: TK::Wood, min_size: 0.7, ..d() }),
        p("Stone / concrete", "Textures", "Rough stone and concrete surface.", BrushSettings { size: 60.0, hardness: 0.6, flow: 0.75, texture: 0.65, tex_kind: TK::Stone, min_size: 0.7, ..d() }),
        p("Canvas oil", "Textures", "Oil paint on canvas weave.", BrushSettings { size: 45.0, tip: T::Bristle, roundness: 0.6, follow_direction: true, streaks: 0.3, edge: Bristly, edge_amount: 0.3, variants: 3, mix: 0.3, mix_refer: layer, texture: 0.45, tex_kind: TK::Canvas, min_size: 0.5, ..d() }),
        p("Watercolor paper", "Textures", "Watercolor on rough cold-press paper.", BrushSettings { size: 60.0, hardness: 0.6, edge: Ragged, edge_amount: 0.6, edge_detail: 0.35, wet_rim: 0.7, variants: 5, angle_jitter: 1.0, opacity: 0.75, flow: 0.35, spacing: 0.08, texture: 0.55, tex_kind: TK::Watercolor, min_size: 0.5, opacity_pressure: true, ..d() }),
        p("Grunge texture", "Textures", "Dirty, worn surface texture.", BrushSettings { size: 70.0, hardness: 0.6, flow: 0.8, texture: 0.7, tex_kind: TK::Grunge, min_size: 0.8, ..d() }),
        p("Crosshatch shading", "Textures", "Shade with crossed ink lines.", BrushSettings { size: 60.0, hardness: 0.6, flow: 0.8, texture: 0.85, tex_kind: TK::Crosshatch, min_size: 0.8, opacity_pressure: true, ..d() }),
        p("Halftone shading", "Textures", "Manga screentone dots.", BrushSettings { size: 60.0, hardness: 0.7, flow: 0.9, texture: 0.9, tex_kind: TK::Dots, min_size: 0.8, ..d() }),
        p("Tree bark", "Textures", "Rough vertical bark in natural browns (Keep texture colors is on).", BrushSettings { size: 70.0, hardness: 0.7, flow: 0.85, texture: 0.7, tex_kind: TK::Bark, tex_color: 1.0, min_size: 0.8, ..d() }),
        p("Sand", "Textures", "Fine sandy grain for beaches, deserts and dusty ground.", BrushSettings { size: 70.0, hardness: 0.5, flow: 0.6, texture: 0.6, tex_kind: TK::Sand, tex_color: 0.6, min_size: 0.8, opacity_pressure: true, ..d() }),
        p("Marble", "Textures", "Veined marble in its own colors, for floors and statues.", BrushSettings { size: 80.0, hardness: 0.7, flow: 0.9, texture: 0.5, tex_kind: TK::Marble, tex_color: 1.0, min_size: 0.9, ..d() }),
        p("Water ripples", "Textures", "Rippling water surface in blues (or your color with Keep texture colors off).", BrushSettings { size: 90.0, hardness: 0.5, flow: 0.7, texture: 0.6, tex_kind: TK::Water, tex_color: 0.8, min_size: 0.8, ..d() }),
        p("Brick wall", "Textures", "Bricks with mortar lines.", BrushSettings { size: 80.0, hardness: 0.8, flow: 0.95, texture: 0.85, tex_kind: TK::Brick, tex_color: 0.9, tex_scale: 1.2, min_size: 0.9, ..d() }),
        p("Feathers", "Textures", "Soft overlapping feathers that follow the stroke.", BrushSettings { size: 70.0, hardness: 0.7, flow: 0.8, texture: 0.85, tex_kind: TK::Feathers, min_size: 0.7, ..d() }),
        p("Snake skin", "Textures", "Seviath's own: overlapping snake scales with green and gold shades.", BrushSettings { size: 80.0, hardness: 0.85, flow: 0.95, texture: 0.9, tex_kind: TK::SnakeSkin, tex_color: 1.0, tex_scale: 1.2, min_size: 0.9, ..d() }),
        p("Foliage", "Textures", "Dense leafy texture in fresh greens for trees and bushes.", BrushSettings { size: 90.0, hardness: 0.6, flow: 0.85, texture: 0.75, tex_kind: TK::Foliage, tex_color: 1.0, min_size: 0.8, ..d() }),
        // ---- Rough & particles
        p("Grunge stamp", "Rough & particles", "Worn, blotchy stamps for dirt, rust and wear.", BrushSettings { size: 90.0, tip: T::Grunge, spacing: 0.7, scatter: 0.6, size_jitter: 0.4, angle_jitter: 1.0, opacity_jitter: 0.3, deform: 0.3, variants: 4, size_pressure: false, min_size: 1.0, stabilize: 0.0, ..d() }),
        p("Dust particles", "Rough & particles", "Separate fine particles: dust, snow, sparkle spray.", BrushSettings { size: 80.0, particles: 16, particle_size: 0.07, hardness: 0.8, spacing: 0.4, scatter: 0.3, opacity_jitter: 0.5, size_pressure: false, min_size: 1.0, stabilize: 0.0, ..d() }),
        p("Speckle spray", "Rough & particles", "Clusters of separate dots of different sizes.", BrushSettings { size: 60.0, tip: T::Speckle, spacing: 0.45, scatter: 0.5, size_jitter: 0.4, angle_jitter: 1.0, opacity_jitter: 0.3, min_size: 0.6, stabilize: 0.0, ..d() }),
        p("Rough rock", "Rough & particles", "Bent, torn chunks for rocks, rubble and broken edges.", BrushSettings { size: 50.0, hardness: 0.95, edge: Torn, edge_amount: 0.6, edge_detail: 0.55, deform: 0.7, variants: 6, angle_jitter: 1.0, size_jitter: 0.35, spacing: 0.35, scatter: 0.3, min_size: 0.6, stabilize: 0.0, ..d() }),
        p("Splotch", "Rough & particles", "Warped paint splotches, each one different.", BrushSettings { size: 70.0, hardness: 0.8, edge: Ragged, edge_amount: 0.7, edge_detail: 0.4, deform: 0.9, variants: 8, angle_jitter: 1.0, size_jitter: 0.5, scatter: 0.6, spacing: 0.6, opacity_jitter: 0.3, min_size: 0.5, stabilize: 0.0, ..d() }),
        p("Crosshatch", "Rough & particles", "Pencil hatching lines for shading.", BrushSettings { size: 40.0, tip: T::Hatch, hardness: 1.0, follow_direction: true, angle: 45.0, spacing: 0.35, opacity_jitter: 0.25, texture: 0.25, min_size: 0.7, opacity_pressure: true, stabilize: 0.1, ..d() }),
        p("Dry scratch", "Rough & particles", "Scratchy, broken dry strokes with bent bristles.", BrushSettings { size: 40.0, tip: T::Bristle, roundness: 0.4, follow_direction: true, edge: Dry, edge_amount: 0.7, edge_detail: 0.85, deform: 0.4, grain: 0.15, variants: 5, streaks: 0.5, spacing: 0.04, min_size: 0.6, stabilize: 0.05, ..d() }),
        p("Particle sparkle", "Rough & particles", "Tiny four-point stars scattered as particles.", BrushSettings { size: 70.0, tip: T::Star, particles: 6, particle_size: 0.22, spacing: 0.9, scatter: 0.4, opacity_jitter: 0.4, size_pressure: false, min_size: 1.0, stabilize: 0.0, ..d() }),
        p("Chalk + sponge", "Rough & particles", "Dual tip: a chalk tip masked by a sea sponge for broken, weathered strokes.", BrushSettings { size: 50.0, tip: T::Chalk, hardness: 0.8, spacing: 0.06, variants: 4, angle_jitter: 1.0, texture: 0.2, min_size: 0.6, extra_tips: vec![SubTip { tip: T::Sponge, scale: 0.8, scatter: 0.5, ..SubTip::default() }], ..d() }),
        // ---- Nature & FX
        p("Leaves", "Nature & FX", "Scattered leaves in slightly different colors.", BrushSettings { size: 40.0, tip: T::Leaf, spacing: 0.45, scatter: 0.9, size_jitter: 0.5, angle_jitter: 1.0, color_jitter: 0.35, min_size: 0.5, stabilize: 0.0, ..d() }),
        p("Grass", "Nature & FX", "Blades of grass that follow your stroke upwards.", BrushSettings { size: 45.0, tip: T::Grass, spacing: 0.12, scatter: 0.5, size_jitter: 0.5, angle_jitter: 0.12, color_jitter: 0.25, min_size: 0.5, stabilize: 0.0, ..d() }),
        p("Clouds", "Nature & FX", "Soft puffy clouds; build them up in layers.", BrushSettings { size: 120.0, tip: T::Cloud, flow: 0.35, spacing: 0.2, scatter: 0.5, size_jitter: 0.5, angle_jitter: 1.0, opacity_jitter: 0.4, size_pressure: false, min_size: 1.0, stabilize: 0.0, ..d() }),
        p("Sparkles", "Nature & FX", "Twinkling stars and glitter.", BrushSettings { size: 40.0, tip: T::Star, spacing: 1.2, scatter: 2.0, size_jitter: 0.8, angle_jitter: 0.15, opacity_jitter: 0.4, min_size: 0.3, stabilize: 0.0, ..d() }),
        p("Hearts", "Nature & FX", "Little hearts scattered along the stroke.", BrushSettings { size: 30.0, tip: T::Shape, shape_base: SB::Heart, hardness: 1.0, spacing: 1.1, scatter: 1.2, size_jitter: 0.6, angle_jitter: 0.15, color_jitter: 0.2, min_size: 0.5, stabilize: 0.0, ..d() }),
        p("Confetti", "Nature & FX", "Colorful scattered confetti shapes.", BrushSettings { size: 24.0, tip: T::Shape, shape_base: SB::Square, hardness: 1.0, roundness: 0.45, spacing: 0.9, scatter: 1.6, size_jitter: 0.5, angle_jitter: 1.0, color_jitter: 0.9, min_size: 0.6, stabilize: 0.0, ..d() }),
        p("Scale stamps", "Nature & FX", "Single scales placed one by one along the stroke (dragons, fish, armor).", BrushSettings { size: 34.0, tip: T::Scale, hardness: 1.0, follow_direction: true, spacing: 0.95, size_jitter: 0.12, angle_jitter: 0.02, color_jitter: 0.12, opacity_jitter: 0.1, min_size: 0.6, stabilize: 0.2, ..d() }),
        p("Glass sparkle", "Nature & FX", "Four-point star sparkles for glass, eyes and magic.", BrushSettings { size: 26.0, tip: T::Star, spacing: 1.6, scatter: 1.4, size_jitter: 0.7, angle_jitter: 0.05, opacity_jitter: 0.3, min_size: 0.3, stabilize: 0.0, ..d() }),
        p("Fur", "Nature & FX", "Soft fur: fine hairs that follow your stroke. Use short flicks.", BrushSettings { size: 60.0, tip: T::Strands, hardness: 1.0, follow_direction: true, spacing: 0.14, scatter: 0.3, size_jitter: 0.3, angle_jitter: 0.1, opacity_jitter: 0.35, color_jitter: 0.12, flow: 0.85, min_size: 0.55, ends: Taper, taper: 0.5, stabilize: 0.1, ..d() }),
        p("Hair strands brush", "Nature & FX", "Many fine strands at once for flowing hair.", BrushSettings { size: 60.0, tip: T::Strands, hardness: 1.0, follow_direction: true, spacing: 0.05, opacity_jitter: 0.15, flow: 0.7, min_size: 0.5, ends: Taper, taper: 0.7, stabilize: 0.35, ..d() }),
        p("Feather stamps", "Nature & FX", "Single feathers along the stroke for wings and boas.", BrushSettings { size: 50.0, tip: T::Feather, hardness: 1.0, follow_direction: true, spacing: 0.5, size_jitter: 0.3, angle_jitter: 0.08, color_jitter: 0.15, min_size: 0.6, stabilize: 0.1, ..d() }),
    ]
}

pub fn eraser_presets() -> Vec<Preset> {
    let d = BrushSettings::default;
    vec![
        Preset { name: "Hard eraser", category: "Eraser", about: "Removes everything it touches, completely.", s: BrushSettings { size: 20.0, hardness: 0.9, spacing: 0.05, min_size: 0.5, stabilize: 0.0, ..d() } },
        Preset { name: "Soft eraser", category: "Eraser", about: "Fades paint out gently. Go over again to erase more.", s: BrushSettings { size: 80.0, flow: 0.3, hardness: 0.1, min_size: 0.4, opacity_pressure: true, stabilize: 0.0, ..d() } },
        Preset { name: "Lift", category: "Eraser", about: "Lifts paint back off the paper a little at a time, like a damp brush: the stain stays.", s: BrushSettings { size: 60.0, flow: 0.25, hardness: 0.3, min_size: 0.4, opacity_pressure: true, texture: 0.3, tex_kind: TexKind::Watercolor, stabilize: 0.0, ..d() } },
        Preset { name: "Blot", category: "Eraser", about: "A tissue on wet paint: stops it bleeding and soaks a little up. Dry paint stays as it is.", s: BrushSettings { size: 50.0, flow: 0.12, hardness: 0.35, min_size: 0.4, blot: true, stabilize: 0.0, ..d() } },
        Preset { name: "Scrape", category: "Eraser", about: "A blade scraping paint back to bare paper: hard edge, the paper tooth shows.", s: BrushSettings { size: 18.0, hardness: 1.0, spacing: 0.04, min_size: 0.8, texture: 0.5, tex_kind: TexKind::Paper, tex_scale: 0.8, stabilize: 0.0, ..d() } },
        Preset { name: "Textured eraser", category: "Eraser", about: "Grainy, like a rubber on pencil.", s: BrushSettings { size: 30.0, hardness: 0.7, min_size: 0.5, opacity_pressure: true, texture: 0.45, stabilize: 0.0, ..d() } },
    ]
}
