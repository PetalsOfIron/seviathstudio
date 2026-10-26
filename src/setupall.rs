// SPDX-License-Identifier: GPL-3.0-or-later
// Everything the Generate tab needs, installed from inside the app with one button (the Downloads window): the base AI files (`aisetup.rs`),
// the picture AI (Stable Diffusion XL and its pose model, from Hugging Face), and CharacterGen (its code, its models, its own Python
// environment, the Visual Studio 2022 Build Tools that the graphics part is built with, and nvdiffrast). Every step is skipped when it is there
// already, so the button can be pressed again after a failure. What the programs say goes to `setup.log` in the folder of the AI files.
// Nothing here costs money; the tools are free, with the licences listed in the Downloads window.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;

use crate::aisetup::{find_python, log_path, root, run_logged, unzip_stripped, venv_python};

const CHARACTERGEN_ZIP: &str = "https://github.com/zjp-shadow/CharacterGen/archive/refs/heads/main.zip";
const NVDIFFRAST_ZIP: &str = "https://github.com/NVlabs/nvdiffrast/archive/refs/heads/main.zip";
/// Stable Diffusion 2.1, a part of CharacterGen. The first-party repository was taken down; this mirror is the same model (same licence).
pub const SD21_REPO: &str = "sd2-community/stable-diffusion-2-1";

/// CharacterGen asks for older packages than the other helpers (torch 2.1, diffusers 0.24), so it has an environment of its own.
const CG_TORCH: &[&str] = &["torch==2.1.2", "torchvision==0.16.2"];
const CG_PACKAGES: &[&str] = &[
    "numpy<2", "huggingface_hub<0.26", "diffusers==0.24.0", "transformers==4.36.2", "tokenizers<0.19", "accelerate==0.25.0", "omegaconf", "einops", "imageio", "onnxruntime-gpu",
    "opencv-python-headless", "pytorch_lightning==2.1.4", "jaxtyping", "lpips", "ninja", "trimesh", "pymeshlab", "pygltflib", "tqdm", "safetensors", "scipy", "scikit-image", "matplotlib",
    "pyyaml", "typeguard<3", "pillow",
];

pub fn charactergen_folder() -> PathBuf {
    root().join("CharacterGen-code")
}

fn cg_python() -> PathBuf {
    root().join("CG-venv").join("Scripts").join("python.exe")
}

/// The build tools of Visual Studio 2022 (the CUDA compiler does not work with the newest Visual Studio): where `vcvars64.bat` is.
pub fn vcvars() -> Option<PathBuf> {
    [r"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat", r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat", r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// Are the picture models (Stable Diffusion XL and the pose model) in the Hugging Face cache.
pub fn picture_ready() -> bool {
    let cache = hf_cache();
    let has = |repo: &str, file: &str| {
        let snaps = cache.join(format!("models--{}", repo.replace('/', "--"))).join("snapshots");
        std::fs::read_dir(snaps).map(|d| d.flatten().any(|e| e.path().join(file).exists())).unwrap_or(false)
    };
    has("stabilityai/stable-diffusion-xl-base-1.0", "unet/diffusion_pytorch_model.fp16.safetensors") && has("xinsir/controlnet-openpose-sdxl-1.0", "diffusion_pytorch_model.safetensors")
}

fn hf_cache() -> PathBuf {
    if let Ok(p) = std::env::var("HF_HUB_CACHE") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("HF_HOME") {
        return PathBuf::from(p).join("hub");
    }
    dirs::home_dir().unwrap_or_default().join(".cache").join("huggingface").join("hub")
}

/// Download files of a Hugging Face repository into the cache the helpers read from (with the Python of an AI environment).
fn hf_snapshot(python: &Path, repo: &str, patterns: &[&str], step: &Sender<String>, what: &str) -> Result<(), String> {
    let _ = step.send(what.to_string());
    let mut cmd = Command::new(python);
    cmd.args(["-c", "import sys; from huggingface_hub import snapshot_download as d; d(sys.argv[1], allow_patterns=sys.argv[2:])", repo]).args(patterns);
    run_logged(cmd, &log_path())
}

/// The picture AI: Stable Diffusion XL (half precision, about 7 GB) and its pose model (2.5 GB).
pub fn fetch_picture_models(step: &Sender<String>) -> Result<(), String> {
    let python = venv_python(&root());
    if !python.exists() {
        return Err("The Python environment of the AI files is not made yet. Press Download the AI files first.".into());
    }
    if picture_ready() {
        return Ok(());
    }
    hf_snapshot(
        &python,
        "stabilityai/stable-diffusion-xl-base-1.0",
        &[
            "model_index.json", "scheduler/*", "tokenizer/*", "tokenizer_2/*", "text_encoder/config.json", "text_encoder/model.fp16.safetensors", "text_encoder_2/config.json", "text_encoder_2/model.fp16.safetensors",
            "unet/config.json", "unet/diffusion_pytorch_model.fp16.safetensors", "vae/config.json", "vae/diffusion_pytorch_model.fp16.safetensors",
        ],
        step,
        "Downloading Stable Diffusion XL (about 7 GB)",
    )?;
    hf_snapshot(&python, "xinsir/controlnet-openpose-sdxl-1.0", &["config.json", "diffusion_pytorch_model.safetensors"], step, "Downloading the pose model (2.5 GB)")
}

fn pip(python: &Path, args: &[&str], extra: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new(python);
    cmd.args(["-m", "pip", "install"]).args(args).args(extra);
    run_logged(cmd, &log_path())
}

/// CharacterGen, all of it: code, models, Python environment, build tools, nvdiffrast. Takes a long while the first time (about 20 GB).
pub fn setup_charactergen(step: &Sender<String>) -> Result<(), String> {
    let root = root();
    std::fs::create_dir_all(&root).map_err(|e| format!("Could not make {}: {e}", root.display()))?;
    let code = charactergen_folder();
    if !code.join("webui.py").exists() {
        let _ = step.send("Downloading the CharacterGen code".into());
        let zip = root.join("charactergen.zip");
        crate::downloads::fetch_file(CHARACTERGEN_ZIP, &zip, "CharacterGen code", step)?;
        let bytes = std::fs::read(&zip).map_err(|e| e.to_string())?;
        unzip_stripped(&bytes, &code)?;
        let _ = std::fs::remove_file(&zip);
    }
    // its models (14 GB), into the folders of its code
    crate::downloads::fetch_hf_repo("zjpshadow/CharacterGen", &["2D_Stage/", "3D_Stage/"], &code, step)?;
    // its own Python environment
    let py = cg_python();
    if !py.exists() {
        let _ = step.send("Making the Python environment of CharacterGen".into());
        let found = find_python().ok_or("Python 3.10, 3.11 or 3.12 is needed (python.org). Install it and press the button again.")?;
        let mut cmd = Command::new(&found[0]);
        cmd.args(&found[1..]).args(["-m", "venv"]).arg(root.join("CG-venv"));
        run_logged(cmd, &log_path())?;
        let mut cmd = Command::new(&py);
        cmd.args(["-m", "pip", "install", "--upgrade", "pip", "setuptools", "wheel"]);
        run_logged(cmd, &log_path())?;
    }
    let have_torch = root.join("CG-venv").join("Lib").join("site-packages").join("torch").exists();
    if !have_torch {
        let _ = step.send("Installing PyTorch for CharacterGen (about 2.5 GB)".into());
        pip(&py, CG_TORCH, &["--index-url", crate::aisetup::TORCH_INDEX])?;
        let _ = step.send("Installing the other packages of CharacterGen".into());
        pip(&py, CG_PACKAGES, &[])?;
        pip(&py, &["xformers==0.0.23.post1"], &["--index-url", crate::aisetup::TORCH_INDEX])?;
    }
    // the parts of Stable Diffusion 2.1 and the picture cutter that CharacterGen uses
    hf_snapshot(
        &py,
        SD21_REPO,
        &["model_index.json", "scheduler/*", "tokenizer/*", "text_encoder/config.json", "text_encoder/model.safetensors", "vae/config.json", "vae/diffusion_pytorch_model.safetensors", "unet/config.json", "unet/diffusion_pytorch_model.safetensors"],
        step,
        "Downloading Stable Diffusion 2.1 for CharacterGen (about 5 GB)",
    )?;
    hf_snapshot(&py, "skytnt/anime-seg", &["isnetis.onnx"], step, "Downloading the picture cutter (170 MB)")?;
    // nvdiffrast, the graphics part: it is built with the Visual Studio 2022 build tools
    if !root.join("CG-venv").join("Lib").join("site-packages").join("nvdiffrast").exists() {
        if vcvars().is_none() {
            let _ = step.send("Installing the Visual Studio 2022 Build Tools (Windows asks to allow it)".into());
            let mut cmd = Command::new("winget");
            cmd.args(["install", "-e", "--id", "Microsoft.VisualStudio.2022.BuildTools", "--accept-package-agreements", "--accept-source-agreements", "--override", "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"]);
            run_logged(cmd, &log_path())?;
        }
        let vcvars = vcvars().ok_or("The Visual Studio 2022 Build Tools are not installed. Install them (visualstudio.microsoft.com/downloads, Build Tools, C++ tools) and press the button again.")?;
        let _ = step.send("Downloading nvdiffrast".into());
        let zip = root.join("nvdiffrast.zip");
        crate::downloads::fetch_file(NVDIFFRAST_ZIP, &zip, "nvdiffrast", step)?;
        let bytes = std::fs::read(&zip).map_err(|e| e.to_string())?;
        let src = root.join("nvdiffrast-src");
        unzip_stripped(&bytes, &src)?;
        let _ = std::fs::remove_file(&zip);
        let _ = step.send("Building nvdiffrast for the graphics card (a few minutes)".into());
        let script = root.join("build_nvdiffrast.cmd");
        std::fs::write(&script, format!("@echo off\r\ncall \"{}\" >nul\r\nset TORCH_CUDA_ARCH_LIST=8.6\r\nset DISTUTILS_USE_SDK=1\r\nset MSSdk=1\r\n\"{}\" -m pip install --no-build-isolation \"{}\"\r\n", vcvars.display(), py.display(), src.display())).map_err(|e| e.to_string())?;
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(&script);
        run_logged(cmd, &log_path())?;
    }
    Ok(())
}

/// Is CharacterGen complete.
pub fn charactergen_ready() -> bool {
    crate::aisetup::status().charactergen
}

/// Everything: the base AI files, the picture AI and CharacterGen. Each step is skipped when it is there already.
pub fn install_everything(step: &Sender<String>) -> Result<String, String> {
    crate::aisetup::install(step)?;
    fetch_picture_models(step)?;
    setup_charactergen(step)?;
    Ok("Everything is installed: the base AI files, the picture AI and CharacterGen. The Generate tab can use all of it.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_places_are_next_to_the_other_ai_files() {
        assert!(charactergen_folder().starts_with(root()));
        assert!(cg_python().starts_with(root()));
        assert!(cg_python().ends_with("python.exe"));
    }

    #[test]
    fn the_packages_of_charactergen_are_pinned_where_it_matters() {
        assert!(CG_PACKAGES.iter().any(|p| p.starts_with("diffusers==0.24")));
        assert!(CG_PACKAGES.iter().any(|p| p.starts_with("numpy<2")));
        assert_eq!(CG_TORCH, ["torch==2.1.2", "torchvision==0.16.2"]);
        assert!(SD21_REPO.contains("stable-diffusion-2-1"));
    }

    #[test]
    fn the_hugging_face_cache_is_found_from_the_settings_of_the_computer() {
        let c = hf_cache();
        assert!(c.ends_with("hub") || std::env::var("HF_HUB_CACHE").is_ok());
    }
}
