// SPDX-License-Identifier: GPL-3.0-or-later
// Filters and color adjustments (port of the first app's filters.ts and AdjustColors). They all
// work on plain RGBA pictures (not premultiplied, 8 bit), so the same code runs on a small
// preview copy and on the real layer, on a background thread.

use std::collections::HashMap;

use image::RgbaImage;
use rayon::prelude::*;

use crate::blend;
use crate::doc::{Layer, TILE, TileKey};
use crate::oklab::rgb_to_oklab;
use crate::select::Selection;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Blur,
    Stylize,
    Shapes,
    Illustration,
    Color,
    Lines,
    Adjust,
}

impl Group {
    pub const ALL: [(Group, &'static str); 7] = [
        (Group::Blur, "Blur & sharpen"),
        (Group::Stylize, "Stylize"),
        (Group::Shapes, "Shapes"),
        (Group::Illustration, "Illustration"),
        (Group::Color, "Color"),
        (Group::Lines, "Line art"),
        (Group::Adjust, "Adjust colors"),
    ];
}

pub enum Kind {
    Slider { min: f32, max: f32, step: f32, def: f32, /** a size in canvas pixels: scales with the preview */ px: bool, suffix: &'static str },
    Choice { options: &'static [&'static str], def: usize },
    Check { def: bool },
    Color { def: [u8; 3] },
}

pub struct ParamDef {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
}

pub struct FilterDef {
    pub id: &'static str,
    pub group: Group,
    pub label: &'static str,
    pub about: &'static str,
    pub params: &'static [ParamDef],
    /// Has an editable tone curve.
    pub curve: bool,
}

const fn sl(key: &'static str, label: &'static str, min: f32, max: f32, step: f32, def: f32) -> ParamDef {
    ParamDef { key, label, kind: Kind::Slider { min, max, step, def, px: false, suffix: "" } }
}
const fn px(key: &'static str, label: &'static str, min: f32, max: f32, step: f32, def: f32) -> ParamDef {
    ParamDef { key, label, kind: Kind::Slider { min, max, step, def, px: true, suffix: " px" } }
}
const fn unit(key: &'static str, label: &'static str, min: f32, max: f32, step: f32, def: f32, suffix: &'static str) -> ParamDef {
    ParamDef { key, label, kind: Kind::Slider { min, max, step, def, px: false, suffix } }
}

pub const FILTERS: &[FilterDef] = &[
    FilterDef { id: "blur", group: Group::Blur, label: "Gaussian blur", about: "Soft, even blur.", params: &[px("radius", "Radius", 0.5, 100.0, 0.5, 6.0)], curve: false },
    FilterDef {
        id: "motion",
        group: Group::Blur,
        label: "Motion blur",
        about: "Blur in one direction, like fast movement.",
        params: &[px("distance", "Distance", 1.0, 200.0, 1.0, 30.0), unit("angle", "Angle", -180.0, 180.0, 1.0, 0.0, "°")],
        curve: false,
    },
    FilterDef {
        id: "sharpen",
        group: Group::Blur,
        label: "Sharpen (unsharp mask)",
        about: "Crisper edges and details.",
        params: &[sl("amount", "Amount", 0.0, 3.0, 0.05, 0.8), px("radius", "Radius", 0.5, 10.0, 0.5, 1.5)],
        curve: false,
    },
    FilterDef { id: "oil", group: Group::Stylize, label: "Oil paint (smooth)", about: "Smooths areas into painterly patches while keeping edges (Kuwahara).", params: &[px("radius", "Brush size", 1.0, 20.0, 1.0, 5.0)], curve: false },
    FilterDef {
        id: "halftone",
        group: Group::Stylize,
        label: "Halftone (manga tone)",
        about: "Dots like printed manga screentone.",
        params: &[
            px("size", "Dot spacing", 3.0, 60.0, 1.0, 8.0),
            unit("angle", "Angle", 0.0, 90.0, 1.0, 45.0, "°"),
            ParamDef { key: "color", label: "Dots", kind: Kind::Choice { options: &["Black", "Picture colors"], def: 0 } },
        ],
        curve: false,
    },
    FilterDef { id: "pixelate", group: Group::Stylize, label: "Pixelate", about: "Big square pixels.", params: &[px("size", "Pixel size", 2.0, 120.0, 1.0, 12.0)], curve: false },
    FilterDef {
        id: "glow",
        group: Group::Stylize,
        label: "Glow",
        about: "Bright parts glow softly (bloom), great for light effects and magic.",
        params: &[px("radius", "Glow size", 2.0, 120.0, 1.0, 24.0), sl("strength", "Strength", 0.0, 2.0, 0.05, 0.8), sl("threshold", "From brightness", 0.0, 1.0, 0.01, 0.6)],
        curve: false,
    },
    FilterDef {
        id: "noise",
        group: Group::Stylize,
        label: "Add noise / grain",
        about: "Film grain or paper texture.",
        params: &[sl("amount", "Amount", 0.0, 1.0, 0.01, 0.15), ParamDef { key: "mono", label: "Gray grain", kind: Kind::Check { def: true } }],
        curve: false,
    },
    FilterDef {
        id: "emboss",
        group: Group::Stylize,
        label: "Emboss",
        about: "Raised, carved look.",
        params: &[sl("strength", "Strength", 0.0, 3.0, 0.05, 1.0), unit("angle", "Light from", 0.0, 360.0, 1.0, 135.0, "°"), sl("keep", "Keep colors", 0.0, 1.0, 0.01, 0.6)],
        curve: false,
    },
    FilterDef {
        id: "blocks",
        group: Group::Shapes,
        label: "City blocks",
        about: "Splits the picture into rectangles with thin light lines and digital pixel blocks (like a glitchy architectural print).",
        params: &[
            px("size", "Smallest block", 20.0, 600.0, 1.0, 140.0),
            sl("flatten", "Flatten blocks", 0.0, 1.0, 0.01, 0.55),
            sl("lines", "Lines", 0.0, 1.0, 0.01, 0.7),
            px("lineWidth", "Line width", 1.0, 12.0, 0.5, 3.0),
            ParamDef { key: "lineColor", label: "Line color", kind: Kind::Choice { options: &["Light", "Warm glow", "Dark", "From the picture"], def: 0 } },
            sl("pixels", "Pixel blocks", 0.0, 1.0, 0.01, 0.35),
            sl("glitch", "Shifted blocks", 0.0, 1.0, 0.01, 0.2),
            sl("seed", "Pattern", 1.0, 999.0, 1.0, 7.0),
        ],
        curve: false,
    },
    FilterDef {
        id: "cubes",
        group: Group::Shapes,
        label: "Cubes & tiles",
        about: "Rebuilds the picture out of square tiles with beveled edges, or out of 3D cubes.",
        params: &[
            ParamDef { key: "mode", label: "Kind", kind: Kind::Choice { options: &["3D cubes", "Beveled tiles", "Flat squares"], def: 0 } },
            px("size", "Size", 4.0, 200.0, 1.0, 36.0),
            sl("depth", "Light & shade", 0.0, 1.0, 0.01, 0.6),
            sl("gap", "Gap", 0.0, 0.4, 0.01, 0.04),
            ParamDef { key: "bg", label: "Gaps show", kind: Kind::Choice { options: &["Dark", "Light", "Transparent"], def: 0 } },
        ],
        curve: false,
    },
    FilterDef {
        id: "triangles",
        group: Group::Shapes,
        label: "Triangles (low poly)",
        about: "Facets of flat color, with optional ink outlines and light/shadow on each facet.",
        params: &[
            px("size", "Triangle size", 6.0, 300.0, 1.0, 60.0),
            sl("jitter", "Irregular", 0.0, 1.0, 0.01, 0.7),
            px("outline", "Outline", 0.0, 8.0, 0.5, 0.0),
            sl("facets", "Facet shading", 0.0, 1.0, 0.01, 0.35),
            ParamDef { key: "colors", label: "Colors", kind: Kind::Choice { options: &["From the picture", "Black & gray"], def: 0 } },
            sl("seed", "Pattern", 1.0, 999.0, 1.0, 3.0),
        ],
        curve: false,
    },
    FilterDef {
        id: "spheres",
        group: Group::Shapes,
        label: "Spheres & bubbles",
        about: "The picture as balls: matte spheres, or glass spheres that show the picture upside down inside, with shine and rainbow edges.",
        params: &[
            ParamDef { key: "mode", label: "Kind", kind: Kind::Choice { options: &["Glass", "Matte", "Flat dots"], def: 0 } },
            px("size", "Size", 6.0, 600.0, 1.0, 60.0),
            sl("gloss", "Shine", 0.0, 1.0, 0.01, 0.7),
            sl("gap", "Gap", 0.0, 0.5, 0.01, 0.08),
            ParamDef { key: "bg", label: "Background", kind: Kind::Choice { options: &["Faded picture", "White", "Dark", "Transparent"], def: 0 } },
        ],
        curve: false,
    },
    FilterDef {
        id: "illustrate",
        group: Group::Illustration,
        label: "Make it an illustration",
        about: "Photo or rough painting to clean illustration: smooth shapes, a limited palette of flat colors, and line art on the edges.",
        params: &[
            px("smooth", "Simplify", 1.0, 16.0, 1.0, 5.0),
            sl("colors", "Colors", 3.0, 32.0, 1.0, 12.0),
            sl("lines", "Line art", 0.0, 1.0, 0.01, 0.75),
            px("thickness", "Line thickness", 0.5, 6.0, 0.25, 1.5),
            sl("vivid", "Color lift", 0.0, 1.0, 0.01, 0.25),
            ParamDef { key: "shading", label: "Cel shading", kind: Kind::Check { def: true } },
        ],
        curve: false,
    },
    FilterDef { id: "posterize", group: Group::Color, label: "Posterize", about: "Fewer color steps, flat poster look.", params: &[sl("levels", "Steps", 2.0, 16.0, 1.0, 5.0)], curve: false },
    FilterDef { id: "threshold", group: Group::Color, label: "Threshold (black & white)", about: "Only black and white, like ink.", params: &[sl("level", "Level", 0.0, 1.0, 0.01, 0.5)], curve: false },
    FilterDef { id: "invert", group: Group::Color, label: "Invert", about: "Negative colors.", params: &[], curve: false },
    FilterDef {
        id: "lineart",
        group: Group::Lines,
        label: "Extract line art",
        about: "Only the lines, ready to color on layers below (like CSP “Extract line”).",
        params: &[
            px("thickness", "Line thickness", 0.5, 6.0, 0.25, 1.2),
            sl("detail", "Detail", 0.0, 1.0, 0.01, 0.55),
            ParamDef { key: "clear", label: "Transparent background", kind: Kind::Check { def: true } },
        ],
        curve: false,
    },
    FilterDef {
        id: "hsl",
        group: Group::Adjust,
        label: "Hue / saturation / lightness",
        about: "Shift the colors of everything on the layer.",
        params: &[unit("hue", "Hue", -180.0, 180.0, 1.0, 0.0, "°"), unit("sat", "Saturation", -100.0, 100.0, 1.0, 0.0, "%"), unit("light", "Lightness", -100.0, 100.0, 1.0, 0.0, "%")],
        curve: false,
    },
    FilterDef {
        id: "recolor",
        group: Group::Adjust,
        label: "Recolor",
        about: "Keep the light and shade, take hue and saturation from a new color.",
        params: &[ParamDef { key: "target", label: "New color", kind: Kind::Color { def: [59, 130, 246] } }, unit("strength", "Strength", 0.0, 100.0, 1.0, 100.0, "%")],
        curve: false,
    },
    FilterDef {
        id: "brightness",
        group: Group::Adjust,
        label: "Brightness / contrast",
        about: "Lighter or darker, flatter or punchier.",
        params: &[unit("brightness", "Brightness", -100.0, 100.0, 1.0, 0.0, "%"), unit("contrast", "Contrast", -100.0, 100.0, 1.0, 0.0, "%")],
        curve: false,
    },
    FilterDef {
        id: "levels",
        group: Group::Adjust,
        label: "Levels",
        about: "Set the darkest and lightest values and the middle tones.",
        params: &[
            sl("in_black", "Input black", 0.0, 254.0, 1.0, 0.0),
            sl("in_white", "Input white", 1.0, 255.0, 1.0, 255.0),
            sl("gamma", "Middle tones", 0.2, 4.0, 0.01, 1.0),
            sl("out_black", "Output black", 0.0, 254.0, 1.0, 0.0),
            sl("out_white", "Output white", 1.0, 255.0, 1.0, 255.0),
        ],
        curve: false,
    },
    FilterDef { id: "curves", group: Group::Adjust, label: "Curves", about: "Draw the tone curve: click to add a point, drag to bend, right-click to remove.", params: &[], curve: true },
];

pub fn find(id: &str) -> Option<&'static FilterDef> {
    FILTERS.iter().find(|f| f.id == id)
}

/// The values of a filter's settings.
#[derive(Clone, PartialEq, Debug)]
pub struct Params {
    pub v: HashMap<&'static str, f32>,
    pub colors: HashMap<&'static str, [u8; 3]>,
    /// Tone curve points (x, y in 0..1), sorted by x.
    pub curve: Vec<[f32; 2]>,
}

impl Params {
    pub fn defaults(def: &FilterDef) -> Params {
        let mut p = Params { v: HashMap::new(), colors: HashMap::new(), curve: vec![[0.0, 0.0], [1.0, 1.0]] };
        for d in def.params {
            match &d.kind {
                Kind::Slider { def, .. } => {
                    p.v.insert(d.key, *def);
                }
                Kind::Choice { def, .. } => {
                    p.v.insert(d.key, *def as f32);
                }
                Kind::Check { def } => {
                    p.v.insert(d.key, *def as u8 as f32);
                }
                Kind::Color { def } => {
                    p.colors.insert(d.key, *def);
                }
            }
        }
        p
    }

    pub fn f(&self, k: &str) -> f32 {
        self.v.get(k).copied().unwrap_or(0.0)
    }

    pub fn b(&self, k: &str) -> bool {
        self.f(k) > 0.5
    }

    pub fn color(&self, k: &str) -> [u8; 3] {
        self.colors.get(k).copied().unwrap_or([0, 0, 0])
    }
}

// ---------- helpers ----------

fn clamp8(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

fn lum(d: &[u8], i: usize) -> f32 {
    (0.299 * d[i] as f32 + 0.587 * d[i + 1] as f32 + 0.114 * d[i + 2] as f32) / 255.0
}

struct Planes {
    c: [Vec<f32>; 4],
}

fn to_premul(img: &RgbaImage) -> Planes {
    let n = (img.width() * img.height()) as usize;
    let mut c: [Vec<f32>; 4] = [vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]];
    for (i, p) in img.pixels().enumerate() {
        let a = p.0[3] as f32 / 255.0;
        c[0][i] = p.0[0] as f32 * a;
        c[1][i] = p.0[1] as f32 * a;
        c[2][i] = p.0[2] as f32 * a;
        c[3][i] = a;
    }
    Planes { c }
}

fn from_premul(f: &Planes, out: &mut RgbaImage) {
    for (i, p) in out.pixels_mut().enumerate() {
        let a = f.c[3][i];
        if a <= 1e-4 {
            p.0[3] = 0;
            continue;
        }
        p.0 = [clamp8(f.c[0][i] / a), clamp8(f.c[1][i] / a), clamp8(f.c[2][i] / a), clamp8(a * 255.0)];
    }
}

/// One box blur pass (horizontal then vertical), radius `r`, on one channel.
fn box_pass(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r < 1 {
        return src.to_vec();
    }
    let k = 1.0 / (2 * r + 1) as f32;
    let mut tmp = vec![0f32; src.len()];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let base = y * w;
        let at = |x: isize| src[base + x.clamp(0, w as isize - 1) as usize];
        let mut acc: f32 = (-(r as isize)..=r as isize).map(at).sum();
        for (x, out) in row.iter_mut().enumerate() {
            *out = acc * k;
            acc += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
        }
    });
    // vertical: walk columns
    let mut out = vec![0f32; src.len()];
    let cols: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let at = |y: isize| tmp[y.clamp(0, h as isize - 1) as usize * w + x];
            let mut acc: f32 = (-(r as isize)..=r as isize).map(at).sum();
            let mut col = vec![0f32; h];
            for (y, o) in col.iter_mut().enumerate() {
                *o = acc * k;
                acc += at(y as isize + r as isize + 1) - at(y as isize - r as isize);
            }
            col
        })
        .collect();
    for (x, col) in cols.iter().enumerate() {
        for (y, v) in col.iter().enumerate() {
            out[y * w + x] = *v;
        }
    }
    out
}

/// Gaussian blur of one channel, as three box blurs.
fn gauss_channel(c: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    if sigma < 0.3 {
        return c.to_vec();
    }
    let n = 3.0f32;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0)).round() as i32;
    let mut out = c.to_vec();
    for i in 0..3 {
        let size = if i < m { wl } else { wl + 2 };
        out = box_pass(&out, w, h, ((size - 1) / 2) as usize);
    }
    out
}

fn gauss(img: &RgbaImage, sigma: f32) -> Planes {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let f = to_premul(img);
    let c: Vec<Vec<f32>> = f.c.par_iter().map(|ch| gauss_channel(ch, w, h, sigma)).collect();
    let mut it = c.into_iter();
    Planes { c: [it.next().unwrap(), it.next().unwrap(), it.next().unwrap(), it.next().unwrap()] }
}

fn lum_channel(img: &RgbaImage) -> Vec<f32> {
    let raw = img.as_raw();
    (0..raw.len() / 4).map(|i| lum(raw, i * 4)).collect()
}

fn rng(seed: u32) -> impl FnMut() -> f32 {
    let mut a = seed.wrapping_mul(2_654_435_761);
    move || {
        a = a.wrapping_add(0x6d2b_79f5);
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        ((t ^ (t >> 14)) as f64 / 4_294_967_296.0) as f32
    }
}

// ---------- the filters ----------

/// Run a filter. `scale` = how much smaller this copy is than the canvas (sizes in pixels shrink with it).
pub fn run(id: &str, src: &RgbaImage, p: &Params, scale: f32) -> RgbaImage {
    let def = find(id);
    let s = |k: &str| {
        let is_px = def.and_then(|d| d.params.iter().find(|q| q.key == k)).is_some_and(|q| matches!(q.kind, Kind::Slider { px: true, .. }));
        p.f(k) * if is_px { scale } else { 1.0 }
    };
    let (w, h) = (src.width() as usize, src.height() as usize);
    let mut out = src.clone();
    match id {
        "blur" => {
            from_premul(&gauss(src, (s("radius") / 2.0).max(0.3)), &mut out);
        }
        "motion" => return motion_blur(src, s("distance").max(1.0), p.f("angle").to_radians()),
        "sharpen" => {
            let bl = gauss(src, s("radius").max(0.3));
            let amt = p.f("amount");
            let raw = src.as_raw();
            out.as_mut().par_chunks_mut(4).enumerate().for_each(|(i, px)| {
                let a = if bl.c[3][i] == 0.0 { 1.0 } else { bl.c[3][i] };
                for c in 0..3 {
                    px[c] = clamp8(raw[i * 4 + c] as f32 + amt * (raw[i * 4 + c] as f32 - bl.c[c][i] / a));
                }
            });
        }
        "blocks" => return blocks(src, p, s("lineWidth"), s("size")),
        "cubes" => return mosaic(src, s("size"), p.f("mode") as usize, p.f("depth"), p.f("gap"), p.f("bg") as usize),
        "triangles" => return triangles(src, p, s("size"), s("outline")),
        "spheres" => return spheres(src, p.f("mode") as usize, s("size"), p.f("gloss"), p.f("gap"), p.f("bg") as usize),
        "illustrate" => return illustrate(src, p, s("smooth").round().max(1.0) as usize, s("thickness")),
        "oil" => return kuwahara(src, s("radius").round().max(1.0) as usize),
        "pixelate" => return pixelate(src, s("size").round().max(2.0) as usize),
        "halftone" => return halftone(src, s("size").max(3.0), p.f("angle").to_radians(), p.f("color") > 0.5),
        "glow" => return glow(src, s("radius").max(1.0), p.f("strength"), p.f("threshold")),
        "noise" => {
            let mut r = rng(11);
            let amt = p.f("amount") * 120.0;
            let mono = p.b("mono");
            for px in out.pixels_mut() {
                if px.0[3] == 0 {
                    continue;
                }
                let g = (r() + r() + r() - 1.5) * amt;
                for c in 0..3 {
                    let v = if mono { g } else { (r() + r() + r() - 1.5) * amt };
                    px.0[c] = clamp8(px.0[c] as f32 + v);
                }
            }
        }
        "emboss" => {
            let l = lum_channel(src);
            let ang = p.f("angle").to_radians();
            let k = scale.max(1.0);
            let (dx, dy) = ((ang.cos() * k).round() as isize, (-ang.sin() * k).round() as isize);
            let (st, keep) = (p.f("strength") * 255.0, p.f("keep"));
            let raw = src.as_raw();
            out.as_mut().par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
                for x in 0..w {
                    let at = |xx: isize, yy: isize| l[yy.clamp(0, h as isize - 1) as usize * w + xx.clamp(0, w as isize - 1) as usize];
                    let v = (at(x as isize + dx, y as isize + dy) - at(x as isize - dx, y as isize - dy)) * st;
                    for c in 0..3 {
                        row[x * 4 + c] = clamp8((128.0 + v) * (1.0 - keep) + (raw[(y * w + x) * 4 + c] as f32 + v) * keep);
                    }
                }
            });
        }
        "posterize" => {
            let lv = (p.f("levels").max(2.0) - 1.0).max(1.0);
            for px in out.pixels_mut() {
                for c in 0..3 {
                    px.0[c] = ((px.0[c] as f32 / 255.0 * lv).round() / lv * 255.0).round() as u8;
                }
            }
        }
        "threshold" => {
            let t = p.f("level");
            for px in out.pixels_mut() {
                let l = (0.299 * px.0[0] as f32 + 0.587 * px.0[1] as f32 + 0.114 * px.0[2] as f32) / 255.0;
                let v = if l >= t { 255 } else { 0 };
                px.0[..3].fill(v);
            }
        }
        "invert" => {
            for px in out.pixels_mut() {
                for c in 0..3 {
                    px.0[c] = 255 - px.0[c];
                }
            }
        }
        "lineart" => {
            let lines = line_art(src, s("thickness").max(0.5), p.f("detail"));
            let clear = p.b("clear");
            let raw = src.as_raw();
            for (i, px) in out.pixels_mut().enumerate() {
                let ink = lines[i];
                if clear {
                    px.0 = [20, 20, 24, (ink * (raw[i * 4 + 3] as f32 + 255.0).min(255.0)).round() as u8];
                } else {
                    px.0 = [clamp8(255.0 - ink * 235.0), clamp8(255.0 - ink * 235.0), clamp8(255.0 - ink * 231.0), 255];
                }
            }
        }
        "hsl" => adjust_hsl(&mut out, p.f("hue") / 360.0, p.f("sat") / 100.0, p.f("light") / 100.0),
        "recolor" => recolor(&mut out, p.color("target"), p.f("strength") / 100.0),
        "brightness" => {
            let (b, c) = (p.f("brightness") / 100.0, p.f("contrast") / 100.0);
            let k = ((c.clamp(-0.99, 0.99) + 1.0) * std::f32::consts::FRAC_PI_4).tan();
            let lut: Vec<u8> = (0..256).map(|v| clamp8(((v as f32 / 255.0 - 0.5) * k + 0.5 + b * 0.5) * 255.0)).collect();
            apply_lut(&mut out, &lut);
        }
        "levels" => {
            let (ib, iw) = (p.f("in_black"), p.f("in_white").max(p.f("in_black") + 1.0));
            let (ob, ow) = (p.f("out_black"), p.f("out_white"));
            let g = p.f("gamma").max(0.05);
            let lut: Vec<u8> = (0..256)
                .map(|v| {
                    let t = ((v as f32 - ib) / (iw - ib)).clamp(0.0, 1.0).powf(1.0 / g);
                    clamp8(ob + t * (ow - ob))
                })
                .collect();
            apply_lut(&mut out, &lut);
        }
        "curves" => apply_lut(&mut out, &curve_lut(&p.curve)),
        _ => {}
    }
    out
}

fn apply_lut(img: &mut RgbaImage, lut: &[u8]) {
    for px in img.pixels_mut() {
        if px.0[3] == 0 {
            continue;
        }
        for c in 0..3 {
            px.0[c] = lut[px.0[c] as usize];
        }
    }
}

/// A smooth tone curve through the points (monotone cubic, so it never overshoots), as 256 values.
pub fn curve_lut(points: &[[f32; 2]]) -> Vec<u8> {
    let mut pts: Vec<[f32; 2]> = points.iter().map(|p| [p[0].clamp(0.0, 1.0), p[1].clamp(0.0, 1.0)]).collect();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-4);
    if pts.len() < 2 {
        return (0..=255).collect();
    }
    let n = pts.len();
    let dx: Vec<f32> = (0..n - 1).map(|i| pts[i + 1][0] - pts[i][0]).collect();
    let m: Vec<f32> = (0..n - 1).map(|i| (pts[i + 1][1] - pts[i][1]) / dx[i]).collect();
    let mut t = vec![0f32; n];
    t[0] = m[0];
    t[n - 1] = m[n - 2];
    for i in 1..n - 1 {
        t[i] = if m[i - 1] * m[i] <= 0.0 { 0.0 } else { (m[i - 1] + m[i]) / 2.0 };
    }
    for i in 0..n - 1 {
        if m[i].abs() < 1e-9 {
            t[i] = 0.0;
            t[i + 1] = 0.0;
        } else {
            let (a, b) = (t[i] / m[i], t[i + 1] / m[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let k = 3.0 / s.sqrt();
                t[i] = k * a * m[i];
                t[i + 1] = k * b * m[i];
            }
        }
    }
    (0..256)
        .map(|v| {
            let x = v as f32 / 255.0;
            if x <= pts[0][0] {
                return clamp8(pts[0][1] * 255.0);
            }
            if x >= pts[n - 1][0] {
                return clamp8(pts[n - 1][1] * 255.0);
            }
            let i = pts.iter().rposition(|p| p[0] <= x).unwrap().min(n - 2);
            let (h, s) = (dx[i], (x - pts[i][0]) / dx[i]);
            let (s2, s3) = (s * s, s * s * s);
            let y = (2.0 * s3 - 3.0 * s2 + 1.0) * pts[i][1] + (s3 - 2.0 * s2 + s) * h * t[i] + (-2.0 * s3 + 3.0 * s2) * pts[i + 1][1] + (s3 - s2) * h * t[i + 1];
            clamp8(y * 255.0)
        })
        .collect()
}

// ---- colors

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> [f32; 3] {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let (mut h, mut s) = (0.0, 0.0);
    if d > 0.0 {
        s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h /= 6.0;
    }
    [h, s, l]
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s == 0.0 {
        return [l * 255.0; 3];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        t -= t.floor();
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        v * 255.0
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

fn adjust_hsl(img: &mut RgbaImage, dh: f32, sat: f32, light: f32) {
    img.as_mut().par_chunks_mut(4).for_each(|px| {
        if px[3] == 0 {
            return;
        }
        let [h, s, l] = rgb_to_hsl(px[0] as f32, px[1] as f32, px[2] as f32);
        let h = (h + dh).rem_euclid(1.0);
        let s = (s * (1.0 + sat)).clamp(0.0, 1.0);
        let l = if light > 0.0 { l + (1.0 - l) * light } else { l + l * light };
        let c = hsl_to_rgb(h, s, l);
        px[..3].copy_from_slice(&[clamp8(c[0]), clamp8(c[1]), clamp8(c[2])]);
    });
}

fn recolor(img: &mut RgbaImage, target: [u8; 3], strength: f32) {
    let t = rgb_to_hsl(target[0] as f32, target[1] as f32, target[2] as f32);
    img.as_mut().par_chunks_mut(4).for_each(|px| {
        if px[3] == 0 {
            return;
        }
        let hsl = rgb_to_hsl(px[0] as f32, px[1] as f32, px[2] as f32);
        // keep the light and shade: mid-tones match the new color, black stays black, white stays white
        let big_l = hsl[2];
        let l = if big_l <= 0.5 { big_l / 0.5 * t[2] } else { t[2] + (big_l - 0.5) / 0.5 * (1.0 - t[2]) };
        let c = hsl_to_rgb(t[0], t[1], l);
        for k in 0..3 {
            px[k] = clamp8(px[k] as f32 + (c[k] - px[k] as f32) * strength);
        }
    });
}

// ---- blurs and friends

fn motion_blur(src: &RgbaImage, dist: f32, ang: f32) -> RgbaImage {
    let f = to_premul(src);
    let (w, h) = (src.width() as usize, src.height() as usize);
    let n = (dist.round() as usize).clamp(3, 64);
    let (cx, cy) = (ang.cos(), ang.sin());
    let offs: Vec<(isize, isize)> = (0..n)
        .map(|s| {
            let t = (s as f32 / (n - 1) as f32 - 0.5) * dist;
            ((cx * t).round() as isize, (cy * t).round() as isize)
        })
        .collect();
    let mut planes: Vec<Vec<f32>> = (0..4).map(|_| vec![0f32; w * h]).collect();
    planes.par_iter_mut().enumerate().for_each(|(c, out)| {
        out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, v) in row.iter_mut().enumerate() {
                let mut acc = 0.0;
                for &(ox, oy) in &offs {
                    let sy = (y as isize + oy).clamp(0, h as isize - 1) as usize;
                    let sx = (x as isize + ox).clamp(0, w as isize - 1) as usize;
                    acc += f.c[c][sy * w + sx];
                }
                *v = acc / n as f32;
            }
        });
    });
    let mut it = planes.into_iter();
    let res = Planes { c: [it.next().unwrap(), it.next().unwrap(), it.next().unwrap(), it.next().unwrap()] };
    let mut out = RgbaImage::new(w as u32, h as u32);
    from_premul(&res, &mut out);
    out
}

/// Kuwahara: each pixel takes the mean of the calmest of its four neighbouring squares.
fn kuwahara(src: &RgbaImage, r: usize) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let w1 = w + 1;
    let raw = src.as_raw();
    let mut sum = [vec![0f64; w1 * (h + 1)], vec![0f64; w1 * (h + 1)], vec![0f64; w1 * (h + 1)]];
    let mut sq = vec![0f64; w1 * (h + 1)];
    let mut lsum = vec![0f64; w1 * (h + 1)];
    for y in 0..h {
        let (mut rs, mut rq, mut rl) = ([0f64; 3], 0f64, 0f64);
        for x in 0..w {
            let j = (y * w + x) * 4;
            for c in 0..3 {
                rs[c] += raw[j + c] as f64;
            }
            let l = lum(raw, j) as f64 * 255.0;
            rq += l * l;
            rl += l;
            let k = (y + 1) * w1 + x + 1;
            for c in 0..3 {
                sum[c][k] = sum[c][k - w1] + rs[c];
            }
            sq[k] = sq[k - w1] + rq;
            lsum[k] = lsum[k - w1] + rl;
        }
    }
    let bx = |a: &[f64], x0: usize, y0: usize, x1: usize, y1: usize| a[y1 * w1 + x1] - a[y0 * w1 + x1] - a[y1 * w1 + x0] + a[y0 * w1 + x0];
    let mut out = src.clone();
    out.as_mut().par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let quads = [
                (x.saturating_sub(r), y.saturating_sub(r), x + 1, y + 1),
                (x, y.saturating_sub(r), (x + r + 1).min(w), y + 1),
                (x.saturating_sub(r), y, x + 1, (y + r + 1).min(h)),
                (x, y, (x + r + 1).min(w), (y + r + 1).min(h)),
            ];
            let (mut best, mut bi) = (f64::MAX, 0);
            for (q, &(x0, y0, x1, y1)) in quads.iter().enumerate() {
                let n = ((x1 - x0) * (y1 - y0)) as f64;
                let m = bx(&lsum, x0, y0, x1, y1) / n;
                let v = bx(&sq, x0, y0, x1, y1) / n - m * m;
                if v < best {
                    best = v;
                    bi = q;
                }
            }
            let (x0, y0, x1, y1) = quads[bi];
            let n = ((x1 - x0) * (y1 - y0)) as f64;
            for c in 0..3 {
                row[x * 4 + c] = (bx(&sum[c], x0, y0, x1, y1) / n).round() as u8;
            }
        }
    });
    out
}

fn avg_rect(raw: &[u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> Option<[u8; 4]> {
    let (x1, y1) = (x1.min(w), y1.min(h));
    let (mut r, mut g, mut b, mut a, mut n) = (0f32, 0f32, 0f32, 0f32, 0f32);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 4;
            let al = raw[i + 3] as f32;
            r += raw[i] as f32 * al;
            g += raw[i + 1] as f32 * al;
            b += raw[i + 2] as f32 * al;
            a += al;
            n += 1.0;
        }
    }
    (a > 0.0).then(|| [(r / a).round() as u8, (g / a).round() as u8, (b / a).round() as u8, (a / n).round() as u8])
}

fn pixelate(src: &RgbaImage, size: usize) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let raw = src.as_raw();
    let mut out = RgbaImage::new(w as u32, h as u32);
    for y0 in (0..h).step_by(size) {
        for x0 in (0..w).step_by(size) {
            let Some(c) = avg_rect(raw, w, h, x0, y0, x0 + size, y0 + size) else { continue };
            for y in y0..(y0 + size).min(h) {
                for x in x0..(x0 + size).min(w) {
                    out.put_pixel(x as u32, y as u32, image::Rgba(c));
                }
            }
        }
    }
    out
}

fn halftone(src: &RgbaImage, size: f32, ang: f32, color: bool) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let raw = src.as_raw();
    let (ca, sa) = (ang.cos(), ang.sin());
    let mut out = src.clone();
    out.as_mut().par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            // position in the rotated dot grid
            let (u, v) = ((x as f32 * ca + y as f32 * sa) / size, (-(x as f32) * sa + y as f32 * ca) / size);
            let (fu, fv) = (u - u.floor() - 0.5, v - v.floor() - 0.5);
            // center of this dot, back in picture coordinates, to read the tone there
            let (cu, cv) = ((u.floor() + 0.5) * size, (v.floor() + 0.5) * size);
            let sx = ((cu * ca - cv * sa).round()).clamp(0.0, w as f32 - 1.0) as usize;
            let sy = ((cu * sa + cv * ca).round()).clamp(0.0, h as f32 - 1.0) as usize;
            let k = (sy * w + sx) * 4;
            let dark = 1.0 - lum(raw, k);
            let rad = dark.sqrt() * 0.72;
            let ink = ((rad - fu.hypot(fv)) * size + 0.5).clamp(0.0, 1.0);
            let a = raw[(y * w + x) * 4 + 3];
            let px = if color {
                [clamp8(255.0 + (raw[k] as f32 - 255.0) * ink), clamp8(255.0 + (raw[k + 1] as f32 - 255.0) * ink), clamp8(255.0 + (raw[k + 2] as f32 - 255.0) * ink), a]
            } else {
                [clamp8(255.0 - 235.0 * ink), clamp8(255.0 - 235.0 * ink), clamp8(255.0 - 230.0 * ink), a]
            };
            row[x * 4..x * 4 + 4].copy_from_slice(&px);
        }
    });
    out
}

fn glow(src: &RgbaImage, radius: f32, strength: f32, threshold: f32) -> RgbaImage {
    let raw = src.as_raw();
    let mut bright = src.clone();
    for (i, px) in bright.pixels_mut().enumerate() {
        let k = ((lum(raw, i * 4) - threshold) / (1.0 - threshold).max(0.01)).max(0.0);
        for c in 0..3 {
            px.0[c] = clamp8(px.0[c] as f32 * k);
        }
    }
    let f = gauss(&bright, radius / 2.0);
    let mut out = src.clone();
    for (i, px) in out.pixels_mut().enumerate() {
        for c in 0..3 {
            let g = (f.c[c][i] / f.c[3][i].max(0.01)) * f.c[3][i] * strength;
            px.0[c] = clamp8(255.0 - (255.0 - px.0[c] as f32) * (255.0 - g.clamp(0.0, 255.0)) / 255.0); // screen
        }
    }
    out
}

fn line_art(src: &RgbaImage, thick: f32, detail: f32) -> Vec<f32> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let l = lum_channel(src);
    let a = gauss_channel(&l, w, h, thick);
    let b = gauss_channel(&l, w, h, thick * 1.6);
    let sens = 0.012 + (1.0 - detail) * 0.05;
    l.iter().enumerate().map(|(i, _)| (((b[i] - a[i]) - sens * 0.4) / sens).clamp(0.0, 1.0)).collect()
}

/// The lightness (OKLab L) of a color 0..255, handy for tests and gradient maps.
#[allow(dead_code)]
pub fn lightness(rgb: [u8; 3]) -> f32 {
    rgb_to_oklab([rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0])[0]
}

// ---------- shape filters: the picture rebuilt out of blocks, cubes, triangles or spheres ----------

fn background(bg: usize, light_first: bool) -> [u8; 4] {
    // dark / light / clear (or for spheres: picture, white, dark, clear: handled by the caller)
    let _ = light_first;
    match bg {
        0 => [22, 22, 28, 255],
        1 => [246, 245, 240, 255],
        _ => [0, 0, 0, 0],
    }
}

fn avg_rect_step(raw: &[u8], w: usize, h: usize, x0: f32, y0: f32, x1: f32, y1: f32, step: usize) -> [f32; 4] {
    let (x0, y0) = (x0.max(0.0).floor() as usize, y0.max(0.0).floor() as usize);
    let (x1, y1) = ((x1.ceil().max(0.0) as usize).min(w), (y1.ceil().max(0.0) as usize).min(h));
    let (mut r, mut g, mut b, mut a, mut n) = (0f32, 0f32, 0f32, 0f32, 0f32);
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let i = (y * w + x) * 4;
            let al = raw[i + 3] as f32;
            r += raw[i] as f32 * al;
            g += raw[i + 1] as f32 * al;
            b += raw[i + 2] as f32 * al;
            a += al;
            n += 1.0;
            x += step.max(1);
        }
        y += step.max(1);
    }
    if a <= 0.0 { [0.0; 4] } else { [r / a, g / a, b / a, a / n] }
}

fn set_px(d: &mut [u8], i: usize, r: f32, g: f32, b: f32, a: f32, mix: f32) {
    let c = [r, g, b, a];
    for k in 0..4 {
        let v = if mix >= 1.0 { c[k] } else { d[i + k] as f32 + (c[k] - d[i + k] as f32) * mix };
        d[i + k] = clamp8(v);
    }
}

/// Squares: flat mosaic, beveled tiles, or isometric 3D cubes.
fn mosaic(src: &RgbaImage, size: f32, mode: usize, depth: f32, gap: f32, bg: usize) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let raw = src.as_raw();
    let size = if mode != 0 { size.max(3.0) } else { size.round().max(2.0) };
    let mut out = RgbaImage::new(w as u32, h as u32);
    let b = background(bg, false);
    for px in out.pixels_mut() {
        px.0 = b;
    }
    if mode == 0 {
        return iso_cubes(src, out, size, depth, gap);
    }
    let size_i = size.round() as usize;
    let g = (size * gap).round() as usize;
    let d = out.as_mut();
    for y0 in (0..h).step_by(size_i) {
        for x0 in (0..w).step_by(size_i) {
            let [r, gg, bb, a] = avg_rect_step(raw, w, h, x0 as f32, y0 as f32, (x0 + size_i) as f32, (y0 + size_i) as f32, if size_i > 24 { 2 } else { 1 });
            if a <= 0.0 {
                continue;
            }
            let bev = ((size * 0.14).round() as usize).max(1);
            for y in y0 + g..(y0 + size_i).saturating_sub(g).min(h) {
                for x in x0 + g..(x0 + size_i).saturating_sub(g).min(w) {
                    let mut k = 0.0;
                    if mode == 1 {
                        let (lx, ly, s) = (x - x0 - g, y - y0 - g, size_i - 2 * g);
                        if lx < bev || ly < bev {
                            k = depth * 0.45; // lit edge
                        }
                        if s - 1 - lx.min(s - 1) < bev || s - 1 - ly.min(s - 1) < bev {
                            k = -depth * 0.45; // shaded edge
                        }
                    }
                    let f = |c: f32| if k >= 0.0 { c + (255.0 - c) * k } else { c * (1.0 + k) };
                    set_px(d, (y * w + x) * 4, f(r), f(gg), f(bb), a, 1.0);
                }
            }
        }
    }
    out
}

/// 3D cubes in isometric view: every hexagon is a cube with a light top, a mid left and a dark right face.
fn iso_cubes(src: &RgbaImage, mut out: RgbaImage, size: f32, depth: f32, gap: f32) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let raw = src.as_raw();
    let r = size / 2.0; // hexagon radius
    let hw = r * 3f32.sqrt(); // hexagon width
    let shrink = 1.0 - gap;
    let s3 = 3f32.sqrt();
    let d = out.as_mut();
    let mut row = -1i32;
    while (row as f32) * r * 1.5 < h as f32 + r {
        let cy = row as f32 * r * 1.5;
        let mut col = -1i32;
        while (col as f32) * hw < w as f32 + hw {
            let cx = col as f32 * hw + if row.rem_euclid(2) == 1 { hw / 2.0 } else { 0.0 };
            let [rr, gg, bb, a] = avg_rect_step(raw, w, h, cx - hw / 2.0, cy - r, cx + hw / 2.0, cy + r, 2);
            col += 1;
            if a <= 0.0 {
                continue;
            }
            let top = [rr + (255.0 - rr) * depth * 0.35, gg + (255.0 - gg) * depth * 0.35, bb + (255.0 - bb) * depth * 0.35];
            let left = [rr, gg, bb];
            let right = [rr * (1.0 - depth * 0.45), gg * (1.0 - depth * 0.45), bb * (1.0 - depth * 0.45)];
            let rad = r * shrink;
            let (ya, yb) = ((cy - rad).floor().max(0.0) as usize, ((cy + rad).ceil().max(0.0) as usize).min(h));
            let (xa, xb) = ((cx - hw / 2.0).floor().max(0.0) as usize, ((cx + hw / 2.0).ceil().max(0.0) as usize).min(w));
            for y in ya..yb {
                for x in xa..xb {
                    let (dx, dy) = ((x as f32 + 0.5 - cx) / rad, (y as f32 + 0.5 - cy) / rad);
                    let (ax, ay) = (dx.abs(), dy.abs());
                    // inside the hexagon (pointy top)
                    if ax > s3 / 2.0 || ay + ax / s3 > 1.0 {
                        continue;
                    }
                    let c = if dy < -ax / s3 { top } else if dx < 0.0 { left } else { right };
                    set_px(d, (y * w + x) * 4, c[0], c[1], c[2], a, 1.0);
                }
            }
        }
        row += 1;
    }
    out
}

fn split_blocks(cells: &mut Vec<[usize; 4]>, rand: &mut impl FnMut() -> f32, x: usize, y: usize, cw: usize, ch: usize, depth: u32, min_s: f32) {
    let can_w = cw as f32 > min_s * 2.0;
    let can_h = ch as f32 > min_s * 2.0;
    if (!can_w && !can_h) || (depth > 2 && rand() < 0.22) {
        cells.push([x, y, cw, ch]);
        return;
    }
    let ratio = cw as f32 / ch as f32;
    let vertical = can_w && (!can_h || ratio > 1.3 || (ratio > 0.75 && rand() < 0.5));
    let t = 0.3 + rand() * 0.4;
    if vertical {
        let a = (cw as f32 * t).round() as usize;
        split_blocks(cells, rand, x, y, a, ch, depth + 1, min_s);
        split_blocks(cells, rand, x + a, y, cw - a, ch, depth + 1, min_s);
    } else {
        let a = (ch as f32 * t).round() as usize;
        split_blocks(cells, rand, x, y, cw, a, depth + 1, min_s);
        split_blocks(cells, rand, x, y + a, cw, ch - a, depth + 1, min_s);
    }
}

/// City blocks: rectangles split again and again, flat-ish, with thin lines and pixel blocks.
fn blocks(src: &RgbaImage, p: &Params, line_w: f32, size: f32) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let raw = src.as_raw();
    let mut rand = rng(p.f("seed").max(1.0) as u32);
    let mut cells: Vec<[usize; 4]> = Vec::new();
    split_blocks(&mut cells, &mut rand, 0, 0, w, h, 0, size.max(8.0));
    let mut out = src.clone();
    let (flatten, pixels, glitch) = (p.f("flatten"), p.f("pixels"), p.f("glitch"));
    for &[x0, y0, cw, ch] in &cells {
        // some blocks show a shifted part of the picture
        let shift = if rand() < glitch * 0.5 { ((rand() - 0.5) * cw as f32 * 0.6).round() as isize } else { 0 };
        let [mr, mg, mb, _] = avg_rect_step(raw, w, h, x0 as f32, y0 as f32, (x0 + cw) as f32, (y0 + ch) as f32, 3);
        let pix = if rand() < pixels { ((cw.min(ch) as f32 / (4 + (rand() * 8.0).floor() as usize) as f32).round() as usize).max(3) } else { 0 };
        for y in y0..(y0 + ch).min(h) {
            for x in x0..(x0 + cw).min(w) {
                let (mut sx, mut sy) = ((x as isize + shift).clamp(0, w as isize - 1) as usize, y);
                if pix > 0 {
                    sx = (x0 + (sx.saturating_sub(x0)) / pix * pix + (pix >> 1)).min(w - 1);
                    sy = (y0 + (y - y0) / pix * pix + (pix >> 1)).min(h - 1);
                }
                let (k, j) = ((sy * w + sx) * 4, (y * w + x) * 4);
                let f = if pix > 0 { flatten * 0.5 } else { flatten };
                let d = out.as_mut();
                d[j] = clamp8(raw[k] as f32 + (mr - raw[k] as f32) * f);
                d[j + 1] = clamp8(raw[k + 1] as f32 + (mg - raw[k + 1] as f32) * f);
                d[j + 2] = clamp8(raw[k + 2] as f32 + (mb - raw[k + 2] as f32) * f);
                d[j + 3] = raw[k + 3];
            }
        }
    }
    // thin lines on part of the block edges
    let lw = line_w.max(1.0).round() as usize;
    let lines = p.f("lines");
    let mode = p.f("lineColor") as usize;
    for &[x0, y0, cw, ch] in &cells {
        for side in 0..2 {
            if rand() > lines {
                continue;
            }
            let len = if rand() < 0.4 { 1.0 } else { 0.35 + rand() * 0.65 }; // the whole edge or a part of it
            let horiz = side == 0;
            let start = rand() * (1.0 - len);
            let total = (if horiz { cw } else { ch }) as f32;
            let mut t = 0.0;
            while t < total * len {
                for q in 0..lw {
                    let (x, y) = if horiz { ((x0 as f32 + total * start + t).round() as isize, (y0 + q) as isize) } else { ((x0 + q) as isize, (y0 as f32 + ch as f32 * start + t).round() as isize) };
                    if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
                        continue;
                    }
                    let j = (y as usize * w + x as usize) * 4;
                    let soft = 1.0 - (q as f32 - (lw as f32 - 1.0) / 2.0).abs() / (lw as f32 / 2.0 + 0.5);
                    let d = out.as_mut();
                    let (mut r, mut g, mut b) = (250.0, 246.0, 236.0);
                    match mode {
                        1 => (r, g, b) = (255.0, 196.0, 140.0),
                        2 => (r, g, b) = (24.0, 22.0, 30.0),
                        3 => (r, g, b) = ((d[j] as f32 * 1.5 + 40.0).min(255.0), (d[j + 1] as f32 * 1.3 + 30.0).min(255.0), (d[j + 2] as f32 * 1.2 + 20.0).min(255.0)),
                        _ => {}
                    }
                    let a = (d[j + 3] as f32).max(255.0 * soft);
                    set_px(d, j, r, g, b, a, 0.85 * soft + 0.15);
                }
                t += 1.0;
            }
        }
    }
    out
}

/// Triangles: a jittered grid split into triangles, each one flat color.
fn triangles(src: &RgbaImage, p: &Params, size: f32, outline: f32) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let raw = src.as_raw();
    let size = size.max(4.0);
    let mut rand = rng(p.f("seed").max(1.0) as u32);
    let jit = p.f("jitter");
    let (cols, rows) = ((w as f32 / size).ceil() as usize + 1, (h as f32 / size).ceil() as usize + 1);
    let mut pts: Vec<[f32; 2]> = Vec::new();
    for r in 0..=rows {
        for c in 0..=cols {
            let edge = r == 0 || c == 0 || r == rows || c == cols;
            let (jx, jy) = if edge { (0.0, 0.0) } else { ((rand() - 0.5) * size * jit, (rand() - 0.5) * size * jit) };
            pts.push([c as f32 * size + jx, r as f32 * size + jy]);
        }
    }
    let at = |r: usize, c: usize| pts[r * (cols + 1) + c];
    let mut out = src.clone();
    let mono = p.f("colors") > 0.5;
    let facets = p.f("facets");
    let mut fill_tri = |a: [f32; 2], b: [f32; 2], c: [f32; 2], rand: &mut dyn FnMut() -> f32| {
        // color: a few samples inside the triangle
        let (mut rr, mut gg, mut bb, mut aa, mut n) = (0f32, 0f32, 0f32, 0f32, 0f32);
        for (u, v) in [(1.0 / 3.0, 1.0 / 3.0), (0.6, 0.2), (0.2, 0.6), (0.2, 0.2)] {
            let (x, y) = ((a[0] + (b[0] - a[0]) * u + (c[0] - a[0]) * v).round(), (a[1] + (b[1] - a[1]) * u + (c[1] - a[1]) * v).round());
            if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
                continue;
            }
            let k = (y as usize * w + x as usize) * 4;
            rr += raw[k] as f32;
            gg += raw[k + 1] as f32;
            bb += raw[k + 2] as f32;
            aa += raw[k + 3] as f32;
            n += 1.0;
        }
        if n == 0.0 {
            return;
        }
        let (mut rr, mut gg, mut bb, aa) = (rr / n, gg / n, bb / n, aa / n);
        if mono {
            let l = (0.299 * rr + 0.587 * gg + 0.114 * bb) / 255.0;
            let v = if l < 0.45 { 8.0 } else if l < 0.8 { 100.0 } else { 238.0 };
            (rr, gg, bb) = (v, v, v);
        }
        // facet shading: each face turned a little toward or away from the light
        let s = (rand() - 0.5) * 2.0 * facets;
        let sh = |v: f32| if s >= 0.0 { v + (255.0 - v) * s * 0.35 } else { v * (1.0 + s * 0.45) };
        let (rr, gg, bb) = (sh(rr), sh(gg), sh(bb));
        let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
        let max_y = (a[1].max(b[1]).max(c[1]).ceil().max(0.0) as usize).min(h - 1);
        let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
        let max_x = (a[0].max(b[0]).max(c[0]).ceil().max(0.0) as usize).min(w - 1);
        let e = |p0: [f32; 2], p1: [f32; 2], x: f32, y: f32| (p1[0] - p0[0]) * (y - p0[1]) - (p1[1] - p0[1]) * (x - p0[0]);
        let area = e(a, b, c[0], c[1]);
        if area.abs() < 1e-6 {
            return;
        }
        let lens = [(b[0] - a[0]).hypot(b[1] - a[1]), (c[0] - b[0]).hypot(c[1] - b[1]), (a[0] - c[0]).hypot(a[1] - c[1])];
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let (w0, w1, w2) = (e(b, c, px, py) / area, e(c, a, px, py) / area, e(a, b, px, py) / area);
                if w0 < -0.001 || w1 < -0.001 || w2 < -0.001 {
                    continue;
                }
                let j = (y * w + x) * 4;
                // distance to the nearest edge, for the outline
                let d = (w0 * (area.abs() / lens[1])).min(w1 * (area.abs() / lens[2])).min(w2 * (area.abs() / lens[0]));
                let dst = out.as_mut();
                if outline > 0.0 && d < outline / 2.0 {
                    set_px(dst, j, 10.0, 10.0, 12.0, 255.0, 1.0);
                } else {
                    set_px(dst, j, rr, gg, bb, aa, 1.0);
                }
            }
        }
    };
    for r in 0..rows {
        for c in 0..cols {
            let (p00, p10, p01, p11) = (at(r, c), at(r, c + 1), at(r + 1, c), at(r + 1, c + 1));
            if rand() < 0.5 {
                fill_tri(p00, p10, p11, &mut rand);
                fill_tri(p00, p11, p01, &mut rand);
            } else {
                fill_tri(p00, p10, p01, &mut rand);
                fill_tri(p10, p11, p01, &mut rand);
            }
        }
    }
    out
}

/// Spheres: matte balls, glass balls (the picture upside down inside), or flat dots.
fn spheres(src: &RgbaImage, mode: usize, size: f32, gloss: f32, gap: f32, bg: usize) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let raw = src.as_raw();
    let size = size.max(4.0);
    let mut out = RgbaImage::new(w as u32, h as u32);
    if bg == 0 {
        for (i, px) in out.pixels_mut().enumerate() {
            let k = i * 4;
            px.0 = [clamp8(raw[k] as f32 * 0.35 + 165.0), clamp8(raw[k + 1] as f32 * 0.35 + 165.0), clamp8(raw[k + 2] as f32 * 0.35 + 165.0), raw[k + 3]];
        }
    } else {
        // white, dark, transparent
        let c = match bg {
            1 => [246, 245, 240, 255],
            2 => [22, 22, 28, 255],
            _ => [0, 0, 0, 0],
        };
        for px in out.pixels_mut() {
            px.0 = c;
        }
    }
    let rad = size / 2.0 * (1.0 - gap);
    let d = out.as_mut();
    let mut cy = size / 2.0;
    while cy < h as f32 + size / 2.0 {
        let mut cx = size / 2.0;
        while cx < w as f32 + size / 2.0 {
            let [mr, mg, mb, ma] = avg_rect_step(raw, w, h, cx - size / 2.0, cy - size / 2.0, cx + size / 2.0, cy + size / 2.0, if size > 30.0 { 2 } else { 1 });
            if ma > 0.0 {
                let (ya, yb) = ((cy - rad).floor().max(0.0) as usize, ((cy + rad).ceil().max(0.0) as usize).min(h));
                let (xa, xb) = ((cx - rad).floor().max(0.0) as usize, ((cx + rad).ceil().max(0.0) as usize).min(w));
                for y in ya..yb {
                    for x in xa..xb {
                        let (dx, dy) = ((x as f32 + 0.5 - cx) / rad, (y as f32 + 0.5 - cy) / rad);
                        let d2 = dx * dx + dy * dy;
                        if d2 > 1.0 {
                            continue;
                        }
                        let edge_a = ((1.0 - d2.sqrt()) * rad).min(1.0); // smooth rim
                        let z = (1.0 - d2).sqrt();
                        let (mut r, mut g, mut b) = (mr, mg, mb);
                        if mode == 1 {
                            // matte: light from the upper left
                            let lit = (-0.5 * dx - 0.6 * dy + 0.62 * z).max(0.0);
                            let k = 0.35 + 0.75 * lit;
                            let spec = lit.max(0.0).powf(30.0) * gloss * 255.0;
                            (r, g, b) = (r * k + spec, g * k + spec, b * k + spec);
                        } else if mode == 0 {
                            // glass: inside, the world looks upside down and squeezed toward the middle
                            let k = 0.55 + 0.45 * (1.0 - z);
                            let sx = (cx - dx * rad * k * 1.6).round().clamp(0.0, w as f32 - 1.0) as usize;
                            let sy = (cy - dy * rad * k * 1.6).round().clamp(0.0, h as f32 - 1.0) as usize;
                            let q = (sy * w + sx) * 4;
                            (r, g, b) = (raw[q] as f32, raw[q + 1] as f32, raw[q + 2] as f32);
                            // darker rim, rainbow fringe near the bottom edge
                            let rim = (1.0 - z).powf(2.2);
                            (r, g, b) = (r * (1.0 - rim * 0.55), g * (1.0 - rim * 0.55), b * (1.0 - rim * 0.55));
                            if dy > 0.3 && 1.0 - z > 0.35 {
                                let hue = (dy.atan2(dx) * 3.0 + 1.0).rem_euclid(std::f32::consts::TAU);
                                let t = (1.0 - z - 0.35) * dy * 0.9;
                                r += (hue.sin() * 0.5 + 0.5) * 255.0 * t;
                                g += ((hue + 2.1).sin() * 0.5 + 0.5) * 255.0 * t;
                                b += ((hue + 4.2).sin() * 0.5 + 0.5) * 255.0 * t;
                            }
                            // window reflection (a soft oval highlight) and a sharp sparkle
                            let (hx, hy) = (dx + 0.35, dy + 0.45);
                            let hl = (1.0 - (hx * hx * 6.0 + hy * hy * 14.0)).max(0.0) * gloss;
                            let sp = (1.0 - (dx + 0.55).hypot(dy + 0.55) * 7.0).max(0.0).powi(2) * gloss;
                            let m = (hl * 0.8 + sp).min(1.0);
                            (r, g, b) = (r + (255.0 - r) * m, g + (255.0 - g) * m, b + (255.0 - b) * m);
                        }
                        set_px(d, (y * w + x) * 4, r, g, b, 255.0, edge_a);
                    }
                }
            }
            cx += size;
        }
        cy += size;
    }
    out
}

/// Illustration: simplify, a limited palette (k-means), optional cel shading, then line art on the edges.
fn illustrate(src: &RgbaImage, p: &Params, smooth_r: usize, thickness: f32) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let smooth = kuwahara(src, smooth_r.max(1));
    let sraw = smooth.as_raw();
    let k = (p.f("colors").round() as usize).max(3);
    let mut rand = rng(5);
    let mut samples: Vec<[f32; 3]> = Vec::new();
    let step = ((w * h) / 6000).max(1);
    let mut i = 0;
    while i < w * h {
        let j = i * 4;
        if sraw[j + 3] >= 20 {
            samples.push([sraw[j] as f32, sraw[j + 1] as f32, sraw[j + 2] as f32]);
        }
        i += step;
    }
    let mut cents: Vec<[f32; 3]> = (0..k).map(|_| samples.get((rand() * samples.len() as f32) as usize).copied().unwrap_or([128.0; 3])).collect();
    let dist = |a: &[f32; 3], b: &[f32; 3]| 0.3 * (a[0] - b[0]).powi(2) + 0.59 * (a[1] - b[1]).powi(2) + 0.11 * (a[2] - b[2]).powi(2);
    for _ in 0..10 {
        let mut acc = vec![[0f32; 4]; k];
        for s in &samples {
            let bi = (0..k).min_by(|&a, &b| dist(s, &cents[a]).total_cmp(&dist(s, &cents[b]))).unwrap_or(0);
            for c in 0..3 {
                acc[bi][c] += s[c];
            }
            acc[bi][3] += 1.0;
        }
        for (i, a) in acc.iter().enumerate() {
            if a[3] > 0.0 {
                cents[i] = [a[0] / a[3], a[1] / a[3], a[2] / a[3]];
            }
        }
    }
    // color lift: a bit more saturation in the palette
    let vivid = p.f("vivid");
    for c in cents.iter_mut() {
        let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
        *c = [(l + (c[0] - l) * (1.0 + vivid)).clamp(0.0, 255.0), (l + (c[1] - l) * (1.0 + vivid)).clamp(0.0, 255.0), (l + (c[2] - l) * (1.0 + vivid)).clamp(0.0, 255.0)];
    }
    let mut out = RgbaImage::new(w as u32, h as u32);
    let cel = p.b("shading");
    let raw = src.as_raw();
    out.as_mut().par_chunks_mut(4).enumerate().for_each(|(i, px)| {
        let j = i * 4;
        let s = [sraw[j] as f32, sraw[j + 1] as f32, sraw[j + 2] as f32];
        let bi = (0..k).min_by(|&a, &b| dist(&s, &cents[a]).total_cmp(&dist(&s, &cents[b]))).unwrap_or(0);
        let [mut r, mut g, mut b] = cents[bi];
        if cel {
            // two-tone cel shading: darker pixels of a color area get a cool shadow tone
            let ls = 0.299 * s[0] + 0.587 * s[1] + 0.114 * s[2];
            let lc = 0.299 * r + 0.587 * g + 0.114 * b;
            if ls < lc - 18.0 {
                (r, g, b) = (r * 0.78, g * 0.8, b * 0.88);
            } else if ls > lc + 22.0 {
                (r, g, b) = (r + (255.0 - r) * 0.25, g + (255.0 - g) * 0.22, b + (255.0 - b) * 0.18);
            }
        }
        px.copy_from_slice(&[clamp8(r), clamp8(g), clamp8(b), raw[j + 3]]);
    });
    let lines = p.f("lines");
    if lines > 0.0 {
        let ink = line_art(src, thickness.max(0.5), 0.5);
        for (i, px) in out.pixels_mut().enumerate() {
            let t = ink[i] * lines;
            if t <= 0.0 {
                continue;
            }
            px.0[0] = clamp8(px.0[0] as f32 * (1.0 - t) + 22.0 * t);
            px.0[1] = clamp8(px.0[1] as f32 * (1.0 - t) + 18.0 * t);
            px.0[2] = clamp8(px.0[2] as f32 * (1.0 - t) + 26.0 * t);
        }
    }
    out
}

// ---------- putting the result on a layer ----------

/// Write a filtered picture (covering `(x0, y0)` and on) back onto the layer: limited by the
/// selection (soft edges blend), and by "lock alpha". Returns the changed tiles.
pub fn put_back(layer: &mut Layer, img: &RgbaImage, x0: i32, y0: i32, sel: Option<&Selection>) -> Vec<TileKey> {
    let t = TILE as i32;
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (x1, y1) = (x0 + w, y0 + h);
    let keys: Vec<TileKey> = (y0.div_euclid(t)..=(y1 - 1).div_euclid(t)).flat_map(|ty| (x0.div_euclid(t)..=(x1 - 1).div_euclid(t)).map(move |tx| (tx, ty))).collect();
    let lock_alpha = layer.lock_alpha;
    let results: Vec<(TileKey, Vec<eframe::egui::Color32>)> = keys
        .par_iter()
        .filter_map(|&key| {
            let mask = match sel {
                Some(s) => Some(s.tile(key)?),
                None => None,
            };
            let old = layer.tiles.get(&key);
            if lock_alpha && old.is_none() {
                return None;
            }
            let mut px = old.map(|o| (**o).clone()).unwrap_or_else(|| vec![eframe::egui::Color32::TRANSPARENT; TILE * TILE]);
            let mut changed = false;
            for j in 0..t {
                let y = key.1 * t + j;
                if y < y0 || y >= y1 {
                    continue;
                }
                for i in 0..t {
                    let x = key.0 * t + i;
                    if x < x0 || x >= x1 {
                        continue;
                    }
                    let k = (j * t + i) as usize;
                    let cov = mask.map(|m| m[k] as f32 / 255.0).unwrap_or(1.0);
                    if cov <= 0.0 {
                        continue;
                    }
                    let n = img.get_pixel((x - x0) as u32, (y - y0) as u32).0;
                    let dst = blend::to_px(px[k]);
                    let a = if lock_alpha { dst[3] } else { n[3] as f32 / 255.0 };
                    let new = [n[0] as f32 / 255.0 * a, n[1] as f32 / 255.0 * a, n[2] as f32 / 255.0 * a, a];
                    let out = blend::lerp(dst, new, cov);
                    let c = blend::from_px(out);
                    if c != px[k] {
                        px[k] = c;
                        changed = true;
                    }
                }
            }
            changed.then_some((key, px))
        })
        .collect();
    let mut out = Vec::with_capacity(results.len());
    for (key, px) in results {
        if px.iter().all(|c| c.a() == 0) {
            layer.tiles.remove(&key);
        } else {
            *layer.tile_mut(key) = px;
        }
        out.push(key);
    }
    if !out.is_empty() {
        layer.rev += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;

    fn flat(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba(c))
    }

    fn defaults(id: &str) -> Params {
        Params::defaults(find(id).unwrap())
    }

    #[test]
    fn blur_spreads_a_dot_and_keeps_the_total() {
        let mut img = flat(81, 81, [0, 0, 0, 0]);
        for y in 36..45 {
            for x in 36..45 {
                img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let mut p = defaults("blur");
        p.v.insert("radius", 8.0);
        let out = run("blur", &img, &p, 1.0);
        let (centre, near, far) = (out.get_pixel(40, 40).0, out.get_pixel(46, 40).0, out.get_pixel(5, 5).0);
        assert!(centre[3] > near[3] && near[3] > 0 && far[3] == 0, "{centre:?} {near:?} {far:?}");
        assert!(centre[0] > 240 && near[0] > 240, "color stays red, it does not go dark: {centre:?} {near:?}");
        let before: f32 = img.pixels().map(|p| p.0[3] as f32 / 255.0).sum();
        let total: f32 = out.pixels().map(|p| p.0[3] as f32 / 255.0).sum();
        assert!((total - before).abs() < before * 0.1, "paint is spread, not lost: {before} -> {total}");
    }

    #[test]
    fn sharpen_makes_edges_crisper() {
        let img = RgbaImage::from_fn(40, 10, |x, _| if x < 20 { image::Rgba([100, 100, 100, 255]) } else { image::Rgba([160, 160, 160, 255]) });
        let soft = run("blur", &img, &defaults("blur"), 1.0);
        let mut sp = defaults("sharpen");
        sp.v.insert("amount", 2.0);
        sp.v.insert("radius", 4.0);
        let sharp = run("sharpen", &soft, &sp, 1.0);
        let step = |i: &RgbaImage| i.get_pixel(21, 5).0[0] as i32 - i.get_pixel(18, 5).0[0] as i32;
        assert!(step(&sharp) > step(&soft), "{} vs {}", step(&sharp), step(&soft));
    }

    #[test]
    fn simple_color_filters() {
        let img = flat(4, 4, [200, 100, 50, 255]);
        assert_eq!(run("invert", &img, &defaults("invert"), 1.0).get_pixel(0, 0).0, [55, 155, 205, 255]);
        let mut tp = defaults("threshold");
        tp.v.insert("level", 0.4);
        assert_eq!(run("threshold", &img, &tp, 1.0).get_pixel(0, 0).0, [255, 255, 255, 255]);
        tp.v.insert("level", 0.9);
        assert_eq!(run("threshold", &img, &tp, 1.0).get_pixel(0, 0).0, [0, 0, 0, 255]);
        let mut pp = defaults("posterize");
        pp.v.insert("levels", 2.0);
        assert!(run("posterize", &img, &pp, 1.0).pixels().all(|p| p.0[..3].iter().all(|v| *v == 0 || *v == 255)));
        let clear = flat(2, 2, [10, 20, 30, 0]);
        assert_eq!(run("invert", &clear, &defaults("invert"), 1.0).get_pixel(0, 0).0[3], 0, "alpha is never touched");
    }

    #[test]
    fn pixelate_makes_blocks() {
        let img = RgbaImage::from_fn(24, 24, |x, y| image::Rgba([(x * 10) as u8, (y * 10) as u8, 0, 255]));
        let mut p = defaults("pixelate");
        p.v.insert("size", 8.0);
        let out = run("pixelate", &img, &p, 1.0);
        assert_eq!(out.get_pixel(1, 1), out.get_pixel(6, 6));
        assert_ne!(out.get_pixel(1, 1), out.get_pixel(9, 1));
    }

    #[test]
    fn hsl_and_recolor() {
        let red = flat(2, 2, [255, 0, 0, 255]);
        let mut p = defaults("hsl");
        p.v.insert("hue", 120.0);
        let g = run("hsl", &red, &p, 1.0).get_pixel(0, 0).0;
        assert!(g[1] > 200 && g[0] < 40, "red turned green: {g:?}");
        let mut p = defaults("hsl");
        p.v.insert("sat", -100.0);
        let gray = run("hsl", &red, &p, 1.0).get_pixel(0, 0).0;
        assert!(gray[0].abs_diff(gray[1]) < 3 && gray[1].abs_diff(gray[2]) < 3, "{gray:?}");
        let shaded = RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([40, 40, 40, 255]) } else { image::Rgba([220, 220, 220, 255]) });
        let mut rp = defaults("recolor");
        rp.colors.insert("target", [30, 90, 220]);
        let out = run("recolor", &shaded, &rp, 1.0);
        let (dark, light) = (out.get_pixel(0, 0).0, out.get_pixel(1, 0).0);
        assert!(dark[2] > dark[0] && light[2] > light[0], "both are blueish");
        assert!(light[2] as i32 + light[1] as i32 > dark[2] as i32 + dark[1] as i32, "light stays lighter");
    }

    #[test]
    fn levels_brightness_and_curves() {
        let img = RgbaImage::from_fn(256, 1, |x, _| image::Rgba([x as u8, x as u8, x as u8, 255]));
        let mut l = defaults("levels");
        l.v.insert("in_black", 50.0);
        l.v.insert("in_white", 200.0);
        let out = run("levels", &img, &l, 1.0);
        assert_eq!((out.get_pixel(50, 0).0[0], out.get_pixel(200, 0).0[0], out.get_pixel(125, 0).0[0]), (0, 255, 128));
        let mut b = defaults("brightness");
        b.v.insert("brightness", 40.0);
        let out = run("brightness", &img, &b, 1.0);
        assert!(out.get_pixel(100, 0).0[0] > 100 + 30);
        let mut b = defaults("brightness");
        b.v.insert("contrast", 60.0);
        let out = run("brightness", &img, &b, 1.0);
        assert!(out.get_pixel(60, 0).0[0] < 60 && out.get_pixel(200, 0).0[0] > 200, "darks darker, lights lighter");
        // a straight curve changes nothing; a bent one lifts the middle
        assert!(curve_lut(&[[0.0, 0.0], [1.0, 1.0]]).iter().enumerate().all(|(i, v)| (*v as i32 - i as i32).abs() <= 1));
        let lut = curve_lut(&[[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]]);
        assert!(lut[128] > 185 && lut[0] == 0 && lut[255] == 255);
        assert!(lut.windows(2).all(|w| w[1] >= w[0]), "never goes backwards");
        let mut c = defaults("curves");
        c.curve = vec![[0.0, 1.0], [1.0, 0.0]];
        assert_eq!(run("curves", &img, &c, 1.0).get_pixel(10, 0).0[0], 245, "an inverted curve inverts");
    }

    #[test]
    fn lineart_finds_edges_and_clears_the_rest() {
        let img = RgbaImage::from_fn(80, 40, |x, _| if (36..44).contains(&x) { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([255, 255, 255, 255]) });
        let out = run("lineart", &img, &defaults("lineart"), 1.0);
        let strongest = (33..47).map(|x| out.get_pixel(x, 20).0[3]).max().unwrap();
        assert!(strongest > 100, "the edges of the dark stroke are line art: {strongest}");
        assert_eq!(out.get_pixel(5, 20).0[3], 0, "paper is transparent");
    }

    #[test]
    fn halftone_and_glow_and_emboss_run() {
        let img = RgbaImage::from_fn(60, 60, |x, y| { let v = ((x + y) * 2).min(255) as u8; image::Rgba([v, v, v, 255]) });
        for id in ["halftone", "glow", "emboss", "oil", "noise", "motion"] {
            let out = run(id, &img, &defaults(id), 1.0);
            assert_eq!(out.dimensions(), img.dimensions(), "{id}");
            assert_ne!(out.as_raw(), img.as_raw(), "{id} changed something");
        }
        let h = run("halftone", &img, &defaults("halftone"), 1.0);
        let inked = h.pixels().filter(|p| p.0[0] < 128).count();
        assert!(inked > 100 && inked < 3000, "dots, not a flat fill: {inked}");
    }

    #[test]
    fn put_back_respects_selection_and_lock() {
        let mut d = Document::new(200, 200);
        let src = RgbaImage::from_pixel(100, 100, image::Rgba([10, 200, 10, 255]));
        let sel = Selection::polygon(&[eframe::egui::pos2(0.0, 0.0), eframe::egui::pos2(50.0, 0.0), eframe::egui::pos2(50.0, 200.0), eframe::egui::pos2(0.0, 200.0)], 200, 200, false);
        let keys = put_back(&mut d.layers[0], &src, 20, 20, sel.as_ref());
        assert!(!keys.is_empty());
        let img = crate::files::layer_rgba(&d.layers[0], 0, 0, 200, 200);
        assert_eq!(img.get_pixel(30, 30).0, [10, 200, 10, 255]);
        assert_eq!(img.get_pixel(80, 30).0[3], 0, "outside the selection");
        // lock alpha: colors change, transparent stays transparent
        let mut e = Document::new(100, 100);
        e.layers[0].tiles = crate::files::tiles_from_rgba(&RgbaImage::from_fn(100, 100, |x, _| if x < 50 { image::Rgba([200, 0, 0, 255]) } else { image::Rgba([0, 0, 0, 0]) }), 0, 0, 100, 100);
        e.layers[0].lock_alpha = true;
        put_back(&mut e.layers[0], &RgbaImage::from_pixel(100, 100, image::Rgba([0, 0, 255, 255])), 0, 0, None);
        let img = crate::files::layer_rgba(&e.layers[0], 0, 0, 100, 100);
        assert_eq!(img.get_pixel(10, 10).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(80, 10).0[3], 0);
    }


    #[test]
    fn shape_filters_rebuild_the_picture() {
        let img = RgbaImage::from_fn(160, 120, |x, y| image::Rgba([(x * 255 / 159) as u8, (y * 255 / 119) as u8, 120, 255]));
        let distinct = |i: &RgbaImage| i.pixels().map(|p| p.0).collect::<std::collections::HashSet<_>>().len();
        // flat squares: few colors, and gaps can be transparent
        let mut p = defaults("cubes");
        p.v.insert("mode", 2.0);
        p.v.insert("size", 20.0);
        p.v.insert("gap", 0.1);
        p.v.insert("bg", 2.0);
        let m = run("cubes", &img, &p, 1.0);
        assert!(distinct(&m) < 100 && m.pixels().any(|p| p.0[3] == 0), "a mosaic with see-through gaps: {}", distinct(&m));
        // 3D cubes: three shades of each hexagon
        let c = run("cubes", &img, &defaults("cubes"), 1.0);
        assert_ne!(c.as_raw(), img.as_raw());
        assert!(c.pixels().any(|p| p.0 == [22, 22, 28, 255]), "dark gaps between the cubes");
        // triangles: a handful of flat colors with the outline in ink
        let mut t = defaults("triangles");
        t.v.insert("outline", 2.0);
        let tri = run("triangles", &img, &t, 1.0);
        assert!(tri.pixels().any(|p| p.0 == [10, 10, 12, 255]), "ink outlines");
        // spheres on transparent: round dots, and corners stay empty
        let mut sp = defaults("spheres");
        sp.v.insert("bg", 3.0);
        sp.v.insert("size", 40.0);
        sp.v.insert("mode", 2.0);
        let s = run("spheres", &img, &sp, 1.0);
        assert_eq!(s.get_pixel(0, 0).0[3], 0, "the corner between the balls is empty");
        assert_eq!(s.get_pixel(20, 20).0[3], 255, "the middle of the first ball");
        // glass shows something different from the matte look
        sp.v.insert("mode", 0.0);
        let glass = run("spheres", &img, &sp, 1.0);
        sp.v.insert("mode", 1.0);
        let matte = run("spheres", &img, &sp, 1.0);
        assert_ne!(glass.get_pixel(20, 20), matte.get_pixel(20, 20));
        // blocks: lines in the light color, the same size
        let mut b = defaults("blocks");
        b.v.insert("size", 20.0);
        let blocks_out = run("blocks", &img, &b, 1.0);
        assert_eq!(blocks_out.dimensions(), img.dimensions());
        assert!(blocks_out.pixels().filter(|p| p.0[0] > 235 && p.0[1] > 230).count() > 20, "thin light lines");
    }

    #[test]
    fn illustrate_limits_the_palette_and_draws_lines() {
        let img = RgbaImage::from_fn(120, 120, |x, y| {
            let v = ((x as f32 / 119.0) * 200.0) as u8;
            if (x as f32 - 60.0).hypot(y as f32 - 60.0) < 30.0 { image::Rgba([200, 60, 60, 255]) } else { image::Rgba([v, v, 230 - v / 2, 255]) }
        });
        let mut p = defaults("illustrate");
        p.v.insert("colors", 4.0);
        p.v.insert("lines", 0.0);
        p.v.insert("shading", 0.0);
        let flat = run("illustrate", &img, &p, 1.0);
        let n: std::collections::HashSet<[u8; 4]> = flat.pixels().map(|p| p.0).collect();
        assert!(n.len() <= 4 + 3, "a few flat colors: {}", n.len());
        p.v.insert("lines", 1.0);
        let inked = run("illustrate", &img, &p, 1.0);
        assert!(inked.pixels().any(|p| p.0[0] < 60 && p.0[1] < 60), "dark ink on the edges");
    }

    #[test]
    fn every_filter_has_defaults_and_runs_without_panicking() {
        let img = RgbaImage::from_fn(30, 20, |x, y| image::Rgba([(x * 8) as u8, (y * 12) as u8, 90, if x > 3 { 255 } else { 0 }]));
        for f in FILTERS {
            let out = run(f.id, &img, &Params::defaults(f), 0.5);
            assert_eq!(out.dimensions(), img.dimensions(), "{}", f.id);
        }
    }
}
