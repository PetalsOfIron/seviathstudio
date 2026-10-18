// SPDX-License-Identifier: GPL-3.0-or-later
// The files of a Live2D model that is ready to use (the "runtime" files, as in a VTube Studio model folder): `.model3.json` (what
// belongs together), `.cdi3.json` (the names of the sliders), `.physics3.json` (the swinging), `.exp3.json` (expressions),
// `.motion3.json` (motions) and VTube Studio's own `.vtube.json` (how tracking drives the sliders, hotkeys, physics strength).
// They are read here and put onto a rig: the swinging becomes parameter physics, the expressions and motions are kept with the rig
// (the motions as clips that play), and VTube Studio's settings are kept for the tracking mapping. The binary `.moc3` is not read here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::rig::paramphys::{Kind, ParamPhysics, PhysInput, PhysOutput, PhysSetting, PhysVertex, Range};
use crate::rig::{Clip, Id, Rig};

/// Read JSON that may start with a byte order mark.
fn json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, String> {
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("The file could not be read: {e}"))
}

// ---- model3.json

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Model3 {
    pub file_references: FileRefs,
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct FileRefs {
    pub moc: String,
    pub textures: Vec<String>,
    pub physics: String,
    pub display_info: String,
    pub expressions: Vec<ExpRef>,
    pub motions: BTreeMap<String, Vec<MotionRef>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ExpRef {
    pub name: String,
    pub file: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MotionRef {
    pub file: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Group {
    pub target: String,
    pub name: String,
    pub ids: Vec<String>,
}

pub fn parse_model3(bytes: &[u8]) -> Result<Model3, String> {
    json(bytes)
}

// ---- cdi3.json

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Cdi3 {
    pub parameters: Vec<CdiParam>,
    pub parts: Vec<CdiPart>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct CdiParam {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct CdiPart {
    pub id: String,
    pub name: String,
}

pub fn parse_cdi3(bytes: &[u8]) -> Result<Cdi3, String> {
    json(bytes)
}

// ---- physics3.json

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Physics3 {
    meta: PMeta,
    physics_settings: Vec<PSetting>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct PMeta {
    fps: f32,
    effective_forces: PForces,
    physics_dictionary: Vec<PDict>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct PForces {
    gravity: XY,
    wind: XY,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "PascalCase", default)]
struct XY {
    x: f32,
    y: f32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct PDict {
    id: String,
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct PSetting {
    id: String,
    input: Vec<PInput>,
    output: Vec<POutput>,
    vertices: Vec<PVertex>,
    normalization: PNorm,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct PSource {
    id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct PInput {
    source: PSource,
    weight: f32,
    #[serde(rename = "Type")]
    kind: String,
    reflect: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct POutput {
    destination: PSource,
    vertex_index: usize,
    scale: f32,
    weight: f32,
    #[serde(rename = "Type")]
    kind: String,
    reflect: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct PVertex {
    position: XY,
    mobility: f32,
    delay: f32,
    acceleration: f32,
    radius: f32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct PNorm {
    position: PRange,
    angle: PRange,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "PascalCase", default)]
struct PRange {
    minimum: f32,
    default: f32,
    maximum: f32,
}

fn kind_of(s: &str) -> Kind {
    match s {
        "Y" => Kind::Y,
        "Angle" => Kind::Angle,
        _ => Kind::X,
    }
}

pub fn parse_physics3(bytes: &[u8]) -> Result<ParamPhysics, String> {
    let p: Physics3 = json(bytes)?;
    let name_of = |id: &str| p.meta.physics_dictionary.iter().find(|d| d.id == id).map(|d| d.name.clone()).unwrap_or_else(|| id.to_string());
    let rng = |r: PRange| Range { min: r.minimum, default: r.default, max: r.maximum };
    let settings = p
        .physics_settings
        .iter()
        .map(|s| PhysSetting {
            name: name_of(&s.id),
            inputs: s.input.iter().map(|i| PhysInput { id: i.source.id.clone(), weight: i.weight, kind: kind_of(&i.kind), reflect: i.reflect }).collect(),
            outputs: s.output.iter().map(|o| PhysOutput { id: o.destination.id.clone(), vertex: o.vertex_index, scale: o.scale, weight: o.weight, kind: kind_of(&o.kind), reflect: o.reflect }).collect(),
            vertices: s.vertices.iter().map(|v| PhysVertex { pos: [v.position.x, v.position.y], mobility: v.mobility, delay: v.delay, acceleration: v.acceleration, radius: v.radius }).collect(),
            position: rng(s.normalization.position),
            angle: rng(s.normalization.angle),
        })
        .collect();
    let g = p.meta.effective_forces.gravity;
    Ok(ParamPhysics { settings, gravity: if g.x == 0.0 && g.y == 0.0 { [0.0, -1.0] } else { [g.x, g.y] }, wind: [p.meta.effective_forces.wind.x, p.meta.effective_forces.wind.y], fps: if p.meta.fps > 0.0 { p.meta.fps } else { 60.0 }, ..Default::default() })
}

// ---- exp3.json

/// An expression: slider values that are put on (added, multiplied or written over) when it is shown.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Expression {
    pub name: String,
    pub fade_in: f32,
    pub fade_out: f32,
    pub params: Vec<ExpParam>,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExpParam {
    pub id: String,
    pub value: f32,
    /// "Add", "Multiply" or "Overwrite".
    pub blend: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Exp3 {
    fade_in_time: f32,
    fade_out_time: f32,
    parameters: Vec<Exp3Param>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Exp3Param {
    id: String,
    value: f32,
    blend: String,
}

pub fn parse_exp3(name: &str, bytes: &[u8]) -> Result<Expression, String> {
    let e: Exp3 = json(bytes)?;
    Ok(Expression { name: name.to_string(), fade_in: e.fade_in_time, fade_out: e.fade_out_time, params: e.parameters.into_iter().map(|p| ExpParam { id: p.id, value: p.value, blend: if p.blend.is_empty() { "Add".into() } else { p.blend } }).collect() })
}

impl Expression {
    /// The value a slider gets when this expression is put on top of its value `current`.
    pub fn blend(&self, id: &str, current: f32) -> Option<f32> {
        let p = self.params.iter().find(|p| p.id == id)?;
        Some(match p.blend.as_str() {
            "Multiply" => current * p.value,
            "Overwrite" => p.value,
            _ => current + p.value,
        })
    }
}

// ---- motion3.json

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Motion {
    pub name: String,
    pub duration: f32,
    pub fps: f32,
    pub looped: bool,
    pub curves: Vec<Curve>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Curve {
    /// "Parameter", "PartOpacity" or "Model".
    pub target: String,
    pub id: String,
    pub segments: Vec<f32>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Motion3 {
    meta: MMeta,
    curves: Vec<MCurve>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct MMeta {
    duration: f32,
    fps: f32,
    #[serde(rename = "Loop")]
    looped: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct MCurve {
    target: String,
    id: String,
    segments: Vec<f32>,
}

pub fn parse_motion3(name: &str, bytes: &[u8]) -> Result<Motion, String> {
    let m: Motion3 = json(bytes)?;
    Ok(Motion { name: name.to_string(), duration: m.meta.duration.max(0.0), fps: if m.meta.fps > 0.0 { m.meta.fps } else { 30.0 }, looped: m.meta.looped, curves: m.curves.into_iter().map(|c| Curve { target: c.target, id: c.id, segments: c.segments }).collect() })
}

impl Curve {
    /// The value of the curve at `t` seconds. The segments are the first point (time, value) and then, for every piece, a kind
    /// (0 straight, 1 smooth, 2 stepped, 3 stepped backwards) and its points.
    pub fn value_at(&self, t: f32) -> f32 {
        let s = &self.segments;
        if s.len() < 2 {
            return 0.0;
        }
        let mut prev = (s[0], s[1]);
        if t <= prev.0 {
            return prev.1;
        }
        let mut i = 2;
        while i < s.len() {
            let kind = s[i] as i32;
            let n = if kind == 1 { 3 } else { 1 };
            if i + 1 + n * 2 > s.len() {
                break;
            }
            let pts: Vec<(f32, f32)> = (0..n).map(|k| (s[i + 1 + k * 2], s[i + 2 + k * 2])).collect();
            let end = *pts.last().unwrap();
            if t <= end.0 {
                let span = (end.0 - prev.0).max(1e-6);
                let u = ((t - prev.0) / span).clamp(0.0, 1.0);
                return match kind {
                    0 => prev.1 + (end.1 - prev.1) * u,
                    // a smooth piece: a cubic curve through four points; the time is found along it
                    1 => {
                        let (p0, p1, p2, p3) = (prev, pts[0], pts[1], pts[2]);
                        let (mut lo, mut hi) = (0.0f32, 1.0f32);
                        for _ in 0..24 {
                            let m = (lo + hi) / 2.0;
                            let x = bez(p0.0, p1.0, p2.0, p3.0, m);
                            if x < t { lo = m } else { hi = m }
                        }
                        bez(p0.1, p1.1, p2.1, p3.1, (lo + hi) / 2.0)
                    }
                    2 => prev.1,
                    _ => end.1,
                };
            }
            prev = end;
            i += 1 + n * 2;
        }
        prev.1
    }
}

fn bez(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
}

// ---- vtube.json

/// The settings VTube Studio saved for a model.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Vtube {
    pub name: String,
    pub physics_on: bool,
    pub physics_strength: f32,
    pub wind_strength: f32,
    /// How a tracking value drives a slider.
    pub params: Vec<VtubeParam>,
    pub hotkeys: Vec<VtubeHotkey>,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VtubeParam {
    pub name: String,
    pub input: String,
    pub input_lower: f32,
    pub input_upper: f32,
    pub output_lower: f32,
    pub output_upper: f32,
    pub clamp_input: bool,
    pub clamp_output: bool,
    pub output: String,
    pub smoothing: f32,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VtubeHotkey {
    pub name: String,
    pub action: String,
    pub file: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Vt {
    name: String,
    physics_settings: VtPhysics,
    parameter_settings: Vec<VtParam>,
    hotkeys: Vec<VtHot>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct VtPhysics {
    #[serde(rename = "Use")]
    on: bool,
    physics_strength: f32,
    wind_strength: f32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct VtParam {
    name: String,
    input: String,
    input_range_lower: f32,
    input_range_upper: f32,
    output_range_lower: f32,
    output_range_upper: f32,
    clamp_input: bool,
    clamp_output: bool,
    #[serde(rename = "OutputLive2D")]
    output: String,
    smoothing: f32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct VtHot {
    name: String,
    action: String,
    file: String,
}

pub fn parse_vtube(bytes: &[u8]) -> Result<Vtube, String> {
    let v: Vt = json(bytes)?;
    Ok(Vtube {
        name: v.name,
        physics_on: v.physics_settings.on,
        physics_strength: v.physics_settings.physics_strength,
        wind_strength: v.physics_settings.wind_strength,
        params: v.parameter_settings.into_iter().map(|p| VtubeParam { name: p.name, input: p.input, input_lower: p.input_range_lower, input_upper: p.input_range_upper, output_lower: p.output_range_lower, output_upper: p.output_range_upper, clamp_input: p.clamp_input, clamp_output: p.clamp_output, output: p.output, smoothing: p.smoothing }).collect(),
        hotkeys: v.hotkeys.into_iter().map(|h| VtubeHotkey { name: h.name, action: h.action, file: h.file }).collect(),
    })
}

// ---- a whole folder

/// Everything read from the runtime files of a model.
#[derive(Default)]
pub struct RuntimeFiles {
    pub model: Model3,
    pub cdi: Option<Cdi3>,
    pub physics: Option<ParamPhysics>,
    pub expressions: Vec<Expression>,
    pub motions: Vec<Motion>,
    pub vtube: Option<Vtube>,
    /// Texture files the model names, as paths on this computer.
    pub textures: Vec<PathBuf>,
    pub notes: Vec<String>,
}

fn read(dir: &Path, rel: &str) -> Option<Vec<u8>> {
    if rel.is_empty() {
        return None;
    }
    std::fs::read(dir.join(rel.replace('\\', "/"))).ok()
}

/// Read a `.model3.json` and the files it names, and the motions in an `animations` folder next to it and the `.vtube.json` of the
/// same name.
pub fn load_folder(model3: &Path) -> Result<RuntimeFiles, String> {
    let dir = model3.parent().unwrap_or(Path::new("."));
    let model = parse_model3(&std::fs::read(model3).map_err(|e| format!("Could not read {}: {e}", model3.display()))?)?;
    let mut out = RuntimeFiles { model: model.clone(), ..Default::default() };
    let r = &model.file_references;
    out.textures = r.textures.iter().map(|t| dir.join(t.replace('\\', "/"))).filter(|p| p.exists()).collect();
    match read(dir, &r.display_info) {
        Some(b) => match parse_cdi3(&b) {
            Ok(c) => out.cdi = Some(c),
            Err(e) => out.notes.push(format!("The names of the sliders: {e}")),
        },
        None if !r.display_info.is_empty() => out.notes.push(format!("{} is missing.", r.display_info)),
        None => {}
    }
    match read(dir, &r.physics) {
        Some(b) => match parse_physics3(&b) {
            Ok(p) => out.physics = Some(p),
            Err(e) => out.notes.push(format!("The physics: {e}")),
        },
        None if !r.physics.is_empty() => out.notes.push(format!("{} is missing.", r.physics)),
        None => {}
    }
    for e in &r.expressions {
        match read(dir, &e.file).map(|b| parse_exp3(&e.name, &b)) {
            Some(Ok(x)) => out.expressions.push(x),
            Some(Err(err)) => out.notes.push(format!("Expression {}: {err}", e.name)),
            None => out.notes.push(format!("Expression file {} is missing.", e.file)),
        }
    }
    // the expressions of a VTube Studio folder are often only files next to the model
    let mut found: Vec<PathBuf> = Vec::new();
    for sub in [dir.to_path_buf(), dir.join("expressions"), dir.join("animations"), dir.join("motions")] {
        if let Ok(rd) = std::fs::read_dir(&sub) {
            found.extend(rd.flatten().map(|e| e.path()));
        }
    }
    found.sort();
    for p in found {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let short = name.split('.').next().unwrap_or("").to_string();
        if let Some(stem) = name.strip_suffix(".exp3.json") {
            if !out.expressions.iter().any(|e| e.name == stem) {
                if let Ok(x) = std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| parse_exp3(stem, &b)) {
                    out.expressions.push(x);
                }
            }
        } else if name.ends_with(".motion3.json") {
            if let Ok(m) = std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| parse_motion3(&short, &b)) {
                if !out.motions.iter().any(|x| x.name == m.name) {
                    out.motions.push(m);
                }
            }
        }
    }
    for (group, refs) in &r.motions {
        for (i, m) in refs.iter().enumerate() {
            let label = if refs.len() > 1 { format!("{group} {}", i + 1) } else { group.clone() };
            if let Some(Ok(x)) = read(dir, &m.file).map(|b| parse_motion3(&label, &b)) {
                if !out.motions.iter().any(|y| y.curves == x.curves) {
                    out.motions.push(x);
                }
            }
        }
    }
    // the file VTube Studio saves next to the model
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".vtube.json")).collect();
        names.sort();
        if let Some(p) = names.first() {
            match std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| parse_vtube(&b)) {
                Ok(v) => out.vtube = Some(v),
                Err(e) => out.notes.push(format!("The VTube Studio settings: {e}")),
            }
        }
    }
    Ok(out)
}

/// What putting the files onto a rig did.
#[derive(Default, Debug, PartialEq)]
pub struct Report {
    pub physics_settings: usize,
    pub expressions: usize,
    pub motions: usize,
    pub sliders_matched: usize,
    pub sliders_missing: Vec<String>,
    pub notes: Vec<String>,
}

/// Put the runtime files onto a rig. Sliders are matched by their Live2D id (found by name from the `.cdi3.json` when the rig does
/// not have the ids). The swinging becomes parameter physics, the expressions and the VTube Studio settings are kept with the rig,
/// and every motion becomes a clip (the curves of sliders the rig has, sampled at the frame rate of the motion).
pub fn apply(rig: &mut Rig, files: &RuntimeFiles) -> Report {
    let mut rep = Report { notes: files.notes.clone(), ..Default::default() };
    // the ids: from the names the model displays
    if let Some(cdi) = &files.cdi {
        for c in &cdi.parameters {
            if rig.params.iter().any(|p| p.cid == c.id) {
                continue;
            }
            let norm = |s: &str| s.to_lowercase().chars().filter(|ch| ch.is_alphanumeric()).collect::<String>();
            if let Some(p) = rig.params.iter_mut().find(|p| p.cid.is_empty() && (norm(&p.name) == norm(&c.name) || norm(&p.name) == norm(&c.id))) {
                p.cid = c.id.clone();
            }
        }
    }
    let have = |rig: &Rig, id: &str| rig.params.iter().any(|p| p.cid == id);
    let mut missing: Vec<String> = Vec::new();
    if let Some(p) = &files.physics {
        rig.param_physics = p.clone();
        rep.physics_settings = p.settings.len();
        for s in &p.settings {
            for id in s.inputs.iter().map(|i| &i.id).chain(s.outputs.iter().map(|o| &o.id)) {
                if !have(rig, id) && !missing.contains(id) {
                    missing.push(id.clone());
                }
            }
        }
    }
    rig.expressions = files.expressions.clone();
    rep.expressions = rig.expressions.len();
    if let Some(v) = &files.vtube {
        rig.vtube = Some(v.clone());
        if !rig.param_physics.is_empty() {
            rig.param_physics.on = v.physics_on;
        }
    }
    // the motions as clips
    rig.live_motions = files.motions.clone();
    for m in &files.motions {
        let ids: Vec<(Id, &Curve)> = m.curves.iter().filter(|c| c.target == "Parameter").filter_map(|c| rig.params.iter().find(|p| p.cid == c.id).map(|p| (p.id, c))).collect();
        if ids.is_empty() {
            continue;
        }
        let frames = ((m.duration * m.fps).round() as usize).max(1);
        let clip = Clip { name: m.name.clone(), fps: m.fps, frames: (0..frames).map(|f| ids.iter().map(|(id, c)| (*id, c.value_at(f as f32 / m.fps))).collect()).collect() };
        rig.clips.retain(|c| c.name != clip.name);
        rig.clips.push(clip);
        rep.motions += 1;
    }
    rep.sliders_matched = rig.params.iter().filter(|p| !p.cid.is_empty()).count();
    rep.sliders_missing = missing;
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL3: &str = "\u{feff}{\"Version\":3,\"FileReferences\":{\"Moc\":\"a.moc3\",\"Textures\":[\"a.2048/texture_00.png\"],\"Physics\":\"a.physics3.json\",\"DisplayInfo\":\"a.cdi3.json\"},\"Groups\":[{\"Target\":\"Parameter\",\"Name\":\"EyeBlink\",\"Ids\":[\"ParamEyeROpen\"]}]}";
    const CDI3: &str = "{\"Version\":3,\"Parameters\":[{\"Id\":\"ParamAngleX\",\"GroupId\":\"g\",\"Name\":\"Angle X\"},{\"Id\":\"Param5\",\"GroupId\":\"g\",\"Name\":\"Hairpsychics\"}],\"Parts\":[{\"Id\":\"PartArmL\",\"Name\":\"Arm L\"}]}";
    const PHYS3: &str = "{\"Version\":3,\"Meta\":{\"Fps\":60,\"EffectiveForces\":{\"Gravity\":{\"X\":0,\"Y\":-1},\"Wind\":{\"X\":0,\"Y\":0}},\"PhysicsDictionary\":[{\"Id\":\"PhysicsSetting1\",\"Name\":\"Hair\"}]},\"PhysicsSettings\":[{\"Id\":\"PhysicsSetting1\",\"Input\":[{\"Source\":{\"Target\":\"Parameter\",\"Id\":\"ParamAngleX\"},\"Weight\":60,\"Type\":\"X\",\"Reflect\":false}],\"Output\":[{\"Destination\":{\"Target\":\"Parameter\",\"Id\":\"Param5\"},\"VertexIndex\":1,\"Scale\":30,\"Weight\":100,\"Type\":\"Angle\",\"Reflect\":true}],\"Vertices\":[{\"Position\":{\"X\":0,\"Y\":0},\"Mobility\":1,\"Delay\":1,\"Acceleration\":1,\"Radius\":0},{\"Position\":{\"X\":0,\"Y\":10},\"Mobility\":0.9,\"Delay\":1,\"Acceleration\":1,\"Radius\":10}],\"Normalization\":{\"Position\":{\"Minimum\":-10,\"Default\":0,\"Maximum\":10},\"Angle\":{\"Minimum\":-10,\"Default\":0,\"Maximum\":10}}}]}";
    const MOTION: &str = "{\"Version\":3,\"Meta\":{\"Duration\":2.0,\"Fps\":30.0,\"Loop\":true},\"Curves\":[{\"Target\":\"Parameter\",\"Id\":\"ParamAngleX\",\"Segments\":[0,0, 0,1,10, 2,2,10]},{\"Target\":\"PartOpacity\",\"Id\":\"PartArmL\",\"Segments\":[0,1,0,2,1]}]}";

    #[test]
    fn the_runtime_files_are_read() {
        let m = parse_model3(MODEL3.as_bytes()).unwrap();
        assert_eq!(m.file_references.moc, "a.moc3");
        assert_eq!(m.file_references.textures, vec!["a.2048/texture_00.png"]);
        assert_eq!(m.groups[0].ids, vec!["ParamEyeROpen"]);
        let c = parse_cdi3(CDI3.as_bytes()).unwrap();
        assert_eq!(c.parameters[1].name, "Hairpsychics");
        assert_eq!(c.parts[0].id, "PartArmL");
        let p = parse_physics3(PHYS3.as_bytes()).unwrap();
        assert_eq!(p.settings.len(), 1);
        assert_eq!(p.settings[0].name, "Hair");
        assert_eq!(p.settings[0].inputs[0].weight, 60.0);
        assert!(p.settings[0].outputs[0].reflect && p.settings[0].outputs[0].kind == Kind::Angle);
        assert_eq!(p.settings[0].vertices[1].radius, 10.0);
        assert_eq!(p.settings[0].position.max, 10.0);
        let e = parse_exp3("smile", b"{\"Type\":\"Live2D Expression\",\"FadeInTime\":0.5,\"FadeOutTime\":0.4,\"Parameters\":[{\"Id\":\"ParamMouthForm\",\"Value\":1,\"Blend\":\"Add\"},{\"Id\":\"ParamEyeLOpen\",\"Value\":0.5,\"Blend\":\"Multiply\"},{\"Id\":\"ParamCheek\",\"Value\":1,\"Blend\":\"Overwrite\"}]}").unwrap();
        assert_eq!(e.params.len(), 3);
        assert_eq!(e.blend("ParamMouthForm", 0.25), Some(1.25));
        assert_eq!(e.blend("ParamEyeLOpen", 0.8), Some(0.4));
        assert_eq!(e.blend("ParamCheek", 0.3), Some(1.0));
        assert_eq!(e.blend("Nothing", 0.3), None);
        assert!(parse_model3(b"not json").is_err());
    }

    #[test]
    fn motion_curves_are_straight_smooth_or_stepped() {
        let mo = parse_motion3("walk", MOTION.as_bytes()).unwrap();
        assert!(mo.looped && mo.duration == 2.0 && mo.curves.len() == 2);
        // a straight piece: from (0, 0) to (1, 10): at half a second it is 5
        let c = &mo.curves[0];
        assert!((c.value_at(0.5) - 5.0).abs() < 1e-4);
        assert!((c.value_at(1.0) - 10.0).abs() < 1e-4);
        assert!((c.value_at(-1.0)).abs() < 1e-6, "before the start: the first value");
        assert!((c.value_at(9.0) - 10.0).abs() < 1e-4, "after the end: the last value");
        // a smooth piece goes through its end points and stays between the controls
        let smooth = Curve { target: "Parameter".into(), id: "x".into(), segments: vec![0.0, 0.0, 1.0, 0.3, 0.0, 0.7, 10.0, 1.0, 10.0] };
        assert!((smooth.value_at(1.0) - 10.0).abs() < 1e-3);
        let mid = smooth.value_at(0.5);
        assert!(mid > 3.0 && mid < 7.0, "{mid}");
        // stepped pieces hold the value, and the backward kind jumps at once
        let stepped = Curve { target: "Parameter".into(), id: "x".into(), segments: vec![0.0, 1.0, 2.0, 1.0, 5.0, 3.0, 2.0, 7.0] };
        assert_eq!(stepped.value_at(0.5), 1.0, "stepped: holds the first value until the end of the piece");
        assert_eq!(stepped.value_at(1.5), 7.0, "inverse stepped: takes the end value at once");
        assert_eq!(stepped.value_at(2.5), 7.0);
    }

    #[test]
    fn the_files_are_put_on_a_rig_by_slider_id_and_name() {
        let mut rig = Rig::default();
        let x = rig.add_param("Angle X", -30.0, 30.0, 0.0);
        let hair = rig.add_param("Hairpsychics", -1.0, 1.0, 0.0);
        rig.add_param("Something else", 0.0, 1.0, 0.0);
        let files = RuntimeFiles {
            model: parse_model3(MODEL3.as_bytes()).unwrap(),
            cdi: Some(parse_cdi3(CDI3.as_bytes()).unwrap()),
            physics: Some(parse_physics3(PHYS3.as_bytes()).unwrap()),
            expressions: vec![parse_exp3("smile", b"{\"Parameters\":[{\"Id\":\"ParamAngleX\",\"Value\":5}]}").unwrap()],
            motions: vec![parse_motion3("sway", MOTION.as_bytes()).unwrap()],
            vtube: Some(parse_vtube(b"{\"Name\":\"A\",\"PhysicsSettings\":{\"Use\":true,\"PhysicsStrength\":40,\"WindStrength\":10},\"ParameterSettings\":[{\"Name\":\"Angle X\",\"Input\":\"FaceAngleX\",\"InputRangeLower\":-30,\"InputRangeUpper\":30,\"OutputRangeLower\":-30,\"OutputRangeUpper\":30,\"OutputLive2D\":\"ParamAngleX\",\"Smoothing\":15}],\"Hotkeys\":[{\"Name\":\"Wave\",\"Action\":\"TriggerAnimation\",\"File\":\"wave.motion3.json\"}]}").unwrap()),
            ..Default::default()
        };
        let rep = apply(&mut rig, &files);
        assert_eq!(rig.params.iter().find(|p| p.id == x).unwrap().cid, "ParamAngleX");
        assert_eq!(rig.params.iter().find(|p| p.id == hair).unwrap().cid, "Param5");
        assert_eq!(rep.sliders_matched, 2);
        assert_eq!(rep.physics_settings, 1);
        assert_eq!((rep.expressions, rep.motions), (1, 1));
        assert!(rep.sliders_missing.is_empty());
        // the motion is a clip over the sliders the rig has: ParamAngleX goes 0 to 10 in a second
        let clip = rig.clips.iter().find(|c| c.name == "sway").unwrap();
        assert_eq!(clip.fps, 30.0);
        assert_eq!(clip.frames.len(), 60);
        let v = |f: usize| clip.frames[f].iter().find(|(i, _)| *i == x).unwrap().1;
        assert!(v(0).abs() < 1e-4 && (v(30) - 10.0).abs() < 0.1 && (v(59) - 10.0).abs() < 0.2);
        // the VTube Studio settings are kept
        let vt = rig.vtube.as_ref().unwrap();
        assert_eq!((vt.physics_strength, vt.params[0].output.as_str(), vt.hotkeys[0].name.as_str()), (40.0, "ParamAngleX", "Wave"));
        // a slider the physics needs but the rig has not is said
        let mut small = Rig::default();
        small.add_param("Angle X", -30.0, 30.0, 0.0);
        let rep2 = apply(&mut small, &files);
        assert_eq!(rep2.sliders_missing, vec!["Param5".to_string()]);
    }

    /// The user's real VTube Studio models, when they are on this computer.
    #[test]
    fn real_vtube_studio_folders_are_read() {
        let base = std::path::PathBuf::from("C:/Program Files (x86)/Steam/steamapps/common/VTube Studio/VTube Studio_Data/StreamingAssets/Live2DModels");
        let mut seen = 0;
        for (dir, model3) in [("Beija", "Beija.model3.json"), ("Selena3", "vtuberscale.model3.json"), ("hiyori_vts", "hiyori.model3.json")] {
            let path = base.join(dir).join(model3);
            if !path.exists() {
                continue;
            }
            let f = load_folder(&path).unwrap_or_else(|e| panic!("{dir}: {e}"));
            let phys = f.physics.as_ref().unwrap_or_else(|| panic!("{dir}: physics"));
            assert!(!phys.settings.is_empty() && phys.settings.iter().all(|s| s.vertices.len() >= 2 && !s.inputs.is_empty() && !s.outputs.is_empty()), "{dir}");
            assert!(f.cdi.as_ref().is_some_and(|c| c.parameters.len() > 5), "{dir}: names");
            assert!(f.vtube.is_some(), "{dir}: vtube.json");
            assert!(!f.textures.is_empty(), "{dir}: textures");
            eprintln!("{dir}: {} physics settings, {} sliders named, {} motions, {} vtube mappings", phys.settings.len(), f.cdi.as_ref().unwrap().parameters.len(), f.motions.len(), f.vtube.as_ref().unwrap().params.len());
            if dir == "hiyori_vts" {
                assert!(f.motions.len() >= 5, "motions: {}", f.motions.len());
                let m = &f.motions[0];
                assert!(m.duration > 1.0 && m.curves.iter().any(|c| c.target == "Parameter" && c.value_at(m.duration / 2.0).is_finite()));
            }
            seen += 1;
        }
        let _ = seen;
    }
}
