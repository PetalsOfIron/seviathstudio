// SPDX-License-Identifier: GPL-3.0-or-later
// The AI helpers of the Generate tab: pictures from a text (Stable Diffusion XL, the pose of a body exact through a pose ControlNet)
// and a game asset (a textured .glb with a real size and a pivot) from a picture of one object (TripoSR). Both run as Python programs
// next to the other helpers (`ai.rs`), one at a time.

use std::path::Path;

use image::RgbaImage;

use crate::ai::{helper_folder, run_json};

const TEXT2IMAGE_SCRIPT: &str = include_str!("../tools/ai/text2image.py");
const ASSET_SCRIPT: &str = include_str!("../tools/ai/asset_triposr.py");
const RIG_MIA_SCRIPT: &str = include_str!("../tools/ai/rig_mia.py");

/// What the picture AI is asked.
#[derive(Clone, Debug)]
pub struct PictureRequest {
    pub prompt: String,
    pub negative: String,
    pub count: usize,
    pub seed: Option<u32>,
    pub steps: u32,
    pub guidance: f32,
    pub size: (u32, u32),
    /// "a-pose" or "t-pose": the body stands exactly so (a pose ControlNet); "none" for things that are not a body.
    pub pose: &'static str,
    /// A picture model of your own (a .safetensors file) instead of Stable Diffusion XL.
    pub checkpoint: Option<String>,
    /// A style file (LoRA) and how strongly it is used.
    pub lora: Option<(String, f32)>,
}

/// The arguments of the picture helper for a request.
pub fn picture_args(r: &PictureRequest, out_dir: &Path) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--prompt".into(),
        r.prompt.clone(),
        "--negative".into(),
        r.negative.clone(),
        "--out-dir".into(),
        out_dir.to_string_lossy().to_string(),
        "--count".into(),
        r.count.clamp(1, 8).to_string(),
        "--steps".into(),
        r.steps.clamp(10, 60).to_string(),
        "--guidance".into(),
        format!("{:.1}", r.guidance.clamp(1.0, 14.0)),
        "--width".into(),
        r.size.0.to_string(),
        "--height".into(),
        r.size.1.to_string(),
        "--pose".into(),
        r.pose.into(),
    ];
    if let Some(s) = r.seed {
        a.push("--seed".into());
        a.push(s.to_string());
    }
    if let Some(m) = &r.checkpoint {
        a.push("--model".into());
        a.push(m.clone());
    }
    if let Some((f, w)) = &r.lora {
        a.push("--lora".into());
        a.push(f.clone());
        a.push("--lora-weight".into());
        a.push(format!("{w:.2}"));
    }
    a
}

/// Make pictures from a text: the pictures that were made and the seed of the first. The first run downloads the models (about 10 GB).
pub fn generate_pictures(python: &str, r: &PictureRequest) -> Result<(Vec<RgbaImage>, u32), String> {
    let dir = helper_folder();
    let out = dir.join("generated");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| format!("Could not make the helper folder: {e}"))?;
    let script = dir.join("text2image.py");
    std::fs::write(&script, TEXT2IMAGE_SCRIPT).map_err(|e| format!("Could not write the helper: {e}"))?;
    let v = run_json(python, &script, &picture_args(r, &out))?;
    let seed = v.get("seed").and_then(|s| s.as_u64()).unwrap_or(0) as u32;
    let mut pictures = Vec::new();
    for p in v.get("images").and_then(|a| a.as_array()).ok_or("The helper made no pictures.")? {
        let path = p.as_str().ok_or("The answer of the helper is not right.")?;
        pictures.push(image::open(path).map_err(|e| format!("A picture could not be read: {e}"))?.to_rgba8());
    }
    if pictures.is_empty() {
        return Err("The helper made no pictures.".into());
    }
    Ok((pictures, seed))
}

/// The wishes for the 3D model of a game asset.
#[derive(Clone, Debug)]
pub struct AssetRequest {
    pub height_m: f32,
    pub triangles: u32,
    pub texture: u32,
    pub detail: u32,
    pub pivot_bottom: bool,
    pub normal_map: bool,
}

pub fn asset_args(r: &AssetRequest, png: &Path, out: &Path, repo: &Path) -> Vec<String> {
    let mut a = vec![
        "--image".to_string(),
        png.to_string_lossy().to_string(),
        "--out".into(),
        out.to_string_lossy().to_string(),
        "--repo".into(),
        repo.to_string_lossy().to_string(),
        "--height".into(),
        format!("{:.3}", r.height_m.clamp(0.01, 200.0)),
        "--faces".into(),
        r.triangles.clamp(200, 200_000).to_string(),
        "--texture".into(),
        r.texture.clamp(128, 4096).to_string(),
        "--resolution".into(),
        r.detail.clamp(128, 384).to_string(),
        "--pivot".into(),
        if r.pivot_bottom { "bottom" } else { "centre" }.into(),
        "--normal-map".into(),
        if r.normal_map { "1" } else { "0" }.into(),
    ];
    let model = crate::aisetup::triposr_model_dir();
    if model.join("model.ckpt").exists() {
        a.push("--model".into());
        a.push(model.to_string_lossy().to_string());
    }
    a
}

/// Make a game asset (a textured .glb with a real size and a pivot) from a picture of one object.
pub fn make_asset(python: &str, picture: &RgbaImage, r: &AssetRequest) -> Result<Vec<u8>, String> {
    let repo = crate::aisetup::triposr_code();
    if !repo.join("tsr").exists() {
        return Err(format!("The shape model code is not in {} (press Download the AI files in the Rig tab).", repo.display()));
    }
    let dir = helper_folder();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not make the helper folder: {e}"))?;
    let script = dir.join("asset_triposr.py");
    std::fs::write(&script, ASSET_SCRIPT).map_err(|e| format!("Could not write the helper: {e}"))?;
    let png = dir.join("asset_input.png");
    picture.save(&png).map_err(|e| format!("Could not write the picture: {e}"))?;
    let out = dir.join("asset.glb");
    let _ = std::fs::remove_file(&out);
    run_json(python, &script, &asset_args(r, &png, &out, &repo))?;
    std::fs::read(&out).map_err(|e| format!("The model file was not made: {e}"))
}

/// The folder of Make-It-Animatable (the AI that works out the skin weights), next to the other AI files.
pub fn mia_folder() -> std::path::PathBuf {
    crate::aisetup::root().join("MIA")
}

/// Is Make-It-Animatable installed with its own Python, its models and the bones file.
pub fn mia_ready() -> bool {
    let d = mia_folder();
    d.join("venv").join("Scripts").join("python.exe").exists() && d.join("output").join("best").join("new").join("bw.pth").exists() && d.join("data").join("Mixamo").join("bones.fbx").exists()
}

/// The skin weights of a 3D shape (a .glb file that `make_shape` wrote) from Make-It-Animatable: about twenty seconds.
pub fn mia_skin(shape_glb: &Path) -> Result<crate::vrm::aiskin::AiSkin, String> {
    if !mia_ready() {
        return Err("Make-It-Animatable is not installed in the AI folder.".into());
    }
    let mia = mia_folder();
    let python = mia.join("venv").join("Scripts").join("python.exe");
    let dir = helper_folder();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not make the helper folder: {e}"))?;
    let script = dir.join("rig_mia.py");
    std::fs::write(&script, RIG_MIA_SCRIPT).map_err(|e| format!("Could not write the helper: {e}"))?;
    let out = dir.join("mia_weights.json");
    let _ = std::fs::remove_file(&out);
    let args = vec!["--mia".to_string(), mia.to_string_lossy().to_string(), "--mesh".into(), shape_glb.to_string_lossy().to_string(), "--out".into(), out.to_string_lossy().to_string()];
    run_json(&python.to_string_lossy(), &script, &args)?;
    let text = std::fs::read_to_string(&out).map_err(|e| format!("The weights were not written: {e}"))?;
    crate::vrm::aiskin::AiSkin::from_json(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_request_becomes_helper_arguments_inside_safe_limits() {
        let r = PictureRequest { prompt: "a lamp".into(), negative: "blurry".into(), count: 99, seed: Some(5), steps: 1000, guidance: 99.0, size: (832, 1216), pose: "none", checkpoint: None, lora: Some(("style.safetensors".into(), 0.7)) };
        let a = picture_args(&r, Path::new("out"));
        let at = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
        assert_eq!(at("--count").as_deref(), Some("8"));
        assert_eq!(at("--steps").as_deref(), Some("60"));
        assert_eq!(at("--guidance").as_deref(), Some("14.0"));
        assert_eq!(at("--pose").as_deref(), Some("none"));
        assert_eq!(at("--seed").as_deref(), Some("5"));
        assert_eq!(at("--lora-weight").as_deref(), Some("0.70"));
        assert!(at("--model").is_none());
    }

    #[test]
    fn the_asset_request_becomes_helper_arguments_inside_safe_limits() {
        let r = AssetRequest { height_m: 0.0, triangles: 5, texture: 100_000, detail: 9999, pivot_bottom: false, normal_map: true };
        let a = asset_args(&r, Path::new("in.png"), Path::new("out.glb"), Path::new("repo"));
        let at = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
        assert_eq!(at("--faces").as_deref(), Some("200"));
        assert_eq!(at("--texture").as_deref(), Some("4096"));
        assert_eq!(at("--resolution").as_deref(), Some("384"));
        assert_eq!(at("--pivot").as_deref(), Some("centre"));
        assert_eq!(at("--height").as_deref(), Some("0.010"));
        assert_eq!(at("--normal-map").as_deref(), Some("1"));
    }
}
