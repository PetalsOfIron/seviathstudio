// SPDX-License-Identifier: GPL-3.0-or-later
// Everything that the program uses but does not contain, in one list: what it is, how big, under which licence, whether it is on this
// computer, and how to fetch it (File, Downloads). The files come from the pages of their makers (GitHub, Hugging Face, Google's MediaPipe
// storage); nothing is run after a download, and the installers are only saved (you start them yourself).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;

use crate::aisetup;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The face, body and hand models of the camera tracking (MediaPipe).
    TrackerModels,
    /// Everything the Generate tab needs, with one button.
    Everything,
    /// TripoSR: the code and the model that make a 3D shape from a picture.
    TripoSr,
    /// The picture AI: Stable Diffusion XL and its pose model.
    PictureAi,
    /// UniRig: the models that find a skeleton and skin weights (not used by the app yet).
    UniRig,
    /// TRELLIS: a 3D shape with colours from a picture (MIT). Not connected to the app yet.
    Trellis,
    /// CharacterGen: a 3D anime character in an A pose from one picture (Apache 2.0): the code, the models, its own Python and the graphics part.
    CharacterGen,
    /// VRM4U: the plugin that opens a VRM file in Unreal Engine.
    Vrm4u,
    /// OBS Studio: the installer (it brings the virtual camera).
    Obs,
    /// Unity Capture: a second virtual camera, "Unity Video Capture" (no OBS needed).
    UnityCapture,
}

pub struct Item {
    pub kind: Kind,
    pub name: &'static str,
    pub what: &'static str,
    pub size: &'static str,
    pub licence: &'static str,
    /// A warning that is shown with it, if any.
    pub warning: Option<&'static str>,
}

pub fn items() -> Vec<Item> {
    vec![
        Item {
            kind: Kind::Everything,
            name: "Everything for the Generate tab (one button)",
            what: "The base AI files, TripoSR, the picture AI and CharacterGen, one after the other, and what is there already is kept. It needs Python 3.10 to 3.12, an NVIDIA graphics card and about 45 GB of free space, and it takes a long while. The Visual Studio 2022 Build Tools are installed too if they are missing (Windows asks to allow it). Everything is free.",
            size: "about 45 GB",
            licence: "see each item below",
            warning: None,
        },
        Item { kind: Kind::TrackerModels, name: "Tracking models (MediaPipe)", what: "The face, body and hand models for the camera tracking.", size: "20 MB", licence: "Apache 2.0", warning: None },
        Item { kind: Kind::TripoSr, name: "TripoSR (3D shape from a picture)", what: "Its code and its model. Needs the Python files of the AI helpers (File, Rig tab, Rig for games).", size: "1.7 GB", licence: "MIT", warning: None },
        Item { kind: Kind::PictureAi, name: "Picture AI (Stable Diffusion XL and the pose model)", what: "Makes the pictures of the Generate tab from a text. Needs the AI files (the first card of the Rig tab) to be installed first.", size: "9.5 GB", licence: "CreativeML Open RAIL++-M, Apache 2.0", warning: None },
        Item { kind: Kind::UniRig, name: "UniRig models (skeleton and skin weights)", what: "Not used by the app yet. Needs about 16 GB of video memory to run comfortably.", size: "5.5 GB", licence: "MIT", warning: None },
        Item {
            kind: Kind::Trellis,
            name: "TRELLIS (3D shape and colours from a picture)",
            what: "The models of Microsoft's TRELLIS: a shape with its own colours, and it can run on 8 GB of video memory with a low-memory setting.",
            size: "3.3 GB",
            licence: "MIT",
            warning: Some("Not connected to the app yet: for trying by hand (it needs its own Python packages)."),
        },
        Item {
            kind: Kind::CharacterGen,
            name: "CharacterGen (anime character in an A pose)",
            what: "Makes views of an anime character in an A pose from one picture, and a 3D model from them: made for characters that are rigged afterwards. This installs all of it: its code, its models, its own Python environment, nvdiffrast, and Stable Diffusion 2.1 for it.",
            size: "about 25 GB",
            licence: "Apache 2.0",
            warning: Some("Takes a long while and installs the Visual Studio 2022 Build Tools if they are missing (Windows asks to allow it): nvdiffrast, the graphics part, is built with them. When it is done, CharacterGen is a Shape AI in the Generate tab."),
        },
        Item { kind: Kind::Vrm4u, name: "VRM4U (VRM files in Unreal Engine)", what: "The plugin for your version of Unreal Engine, saved in Downloads. Copy its VRM4U folder into the Plugins folder of your project.", size: "70 MB", licence: "MIT", warning: None },
        Item {
            kind: Kind::UnityCapture,
            name: "Unity Capture (another virtual camera)",
            what: "A free virtual camera called Unity Video Capture that needs no OBS. It is saved in Downloads\\UnityCapture: run Install.bat inside the Install folder once (it asks to allow changes), then use Stream, Virtual camera.",
            size: "1 MB",
            licence: "MIT",
            warning: None,
        },
        Item { kind: Kind::Obs, name: "OBS Studio (virtual camera)", what: "The installer, saved in Downloads. You start it yourself; the virtual camera comes with OBS.", size: "130 MB", licence: "GPL 2", warning: None },
    ]
}

/// The folder where downloaded programs and plugins are put.
pub fn downloads_folder() -> PathBuf {
    dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."))
}

fn unreal_versions() -> Vec<(u32, u32)> {
    let mut found: Vec<(u32, u32)> = Vec::new();
    for base in ["C:/Program Files/Epic Games", "C:/Program Files (x86)/Epic Games"] {
        let Ok(rd) = std::fs::read_dir(base) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(v) = name.strip_prefix("UE_") {
                let mut p = v.split('.');
                if let (Some(a), Some(b)) = (p.next().and_then(|x| x.parse().ok()), p.next().and_then(|x| x.parse().ok())) {
                    found.push((a, b));
                }
            }
        }
    }
    found.sort();
    found
}

fn vrm4u_folder(v: (u32, u32)) -> PathBuf {
    downloads_folder().join(format!("VRM4U_UE{}.{}", v.0, v.1))
}

fn obs_installed() -> bool {
    Path::new("C:/Program Files/obs-studio/bin/64bit/obs64.exe").exists()
}

fn obs_installer() -> Option<PathBuf> {
    let rd = std::fs::read_dir(downloads_folder()).ok()?;
    rd.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("OBS-Studio-") && n.to_string_lossy().ends_with("-Installer.exe")))
}

fn unirig_files() -> [PathBuf; 2] {
    let d = aisetup::root().join("models").join("UniRig");
    [d.join("skeleton").join("model.ckpt"), d.join("skin").join("model.ckpt")]
}

/// Is it on this computer.
pub fn installed(kind: Kind) -> bool {
    match kind {
        Kind::TrackerModels => aisetup::tracker_model().exists() && aisetup::pose_model().exists() && aisetup::hand_model().exists(),
        Kind::Everything => installed(Kind::TrackerModels) && installed(Kind::TripoSr) && installed(Kind::PictureAi) && installed(Kind::CharacterGen),
        Kind::TripoSr => aisetup::status().triposr,
        Kind::PictureAi => crate::setupall::picture_ready(),
        Kind::UniRig => unirig_files().iter().all(|p| p.exists()),
        Kind::Trellis => aisetup::root().join("models").join("TRELLIS-image-large").join("ckpts").join("slat_flow_img_dit_L_64l8p2_fp16.safetensors").exists(),
        Kind::CharacterGen => aisetup::status().charactergen,
        Kind::Vrm4u => unreal_versions().last().is_some_and(|v| vrm4u_folder(*v).join("Plugins").join("VRM4U").join("VRM4U.uplugin").exists()),
        Kind::Obs => obs_installed() || obs_installer().is_some(),
        Kind::UnityCapture => crate::vcam::registered() || downloads_folder().join("UnityCapture").join("Install").join("UnityCaptureFilter64.dll").exists(),
    }
}

/// What to show for "installed": where it is.
pub fn place(kind: Kind) -> Option<PathBuf> {
    match kind {
        Kind::TrackerModels => Some(aisetup::root().join("models")),
        Kind::Everything => Some(aisetup::root()),
        Kind::TripoSr => Some(aisetup::triposr_model_dir()),
        Kind::PictureAi => Some(aisetup::root()),
        Kind::UniRig => Some(aisetup::root().join("models").join("UniRig")),
        Kind::Trellis => Some(aisetup::root().join("models").join("TRELLIS-image-large")),
        Kind::CharacterGen => Some(crate::setupall::charactergen_folder()),
        Kind::Vrm4u => unreal_versions().last().map(|v| vrm4u_folder(*v)),
        Kind::Obs => obs_installer().or_else(|| Some(PathBuf::from("C:/Program Files/obs-studio"))),
        Kind::UnityCapture => Some(downloads_folder().join("UnityCapture").join("Install")),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).build()
}

/// Save a file from the web: it is written as `.part` and renamed when it is whole, so a stopped download never looks finished. `step` gets
/// the progress now and then.
pub fn fetch_file(url: &str, dest: &Path, what: &str, step: &Sender<String>) -> Result<(), String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not make {}: {e}", dir.display()))?;
    }
    let resp = agent().get(url).set("User-Agent", "SeviathStudio").call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("The download of {what} was refused ({code})."),
        _ => format!("Could not reach the page to download {what}. Is the internet on?"),
    })?;
    let total: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| format!("Could not write {}: {e}", part.display()))?;
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut last_report) = (0u64, 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("The download of {what} stopped: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("Could not write {}: {e}", part.display()))?;
        done += n as u64;
        if done - last_report >= 25 << 20 {
            last_report = done;
            let _ = step.send(if total > 0 { format!("{what}: {:.1} of {:.1} GB", done as f64 / 1e9, total as f64 / 1e9) } else { format!("{what}: {:.1} GB", done as f64 / 1e9) });
        }
    }
    drop(file);
    if total > 0 && done != total {
        let _ = std::fs::remove_file(&part);
        return Err(format!("The download of {what} is not whole ({done} of {total} bytes). Try again."));
    }
    std::fs::rename(&part, dest).map_err(|e| format!("Could not finish {}: {e}", dest.display()))
}

/// A repository on Hugging Face: every file whose path starts with one of `prefixes` is saved under `dest` (the same paths), the ones that are
/// there already are kept.
pub fn fetch_hf_repo(repo: &str, prefixes: &[&str], dest: &Path, step: &Sender<String>) -> Result<(), String> {
    let _ = step.send(format!("Listing {repo}"));
    let tree: Value = agent()
        .get(&format!("https://huggingface.co/api/models/{repo}/tree/main?recursive=true"))
        .set("User-Agent", "SeviathStudio")
        .call()
        .map_err(|_| format!("Could not list {repo} on Hugging Face. Is the internet on?"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let files: Vec<(String, u64)> = tree
        .as_array()
        .ok_or("Hugging Face answered with something that is not a list of files.")?
        .iter()
        .filter(|f| f.get("type").and_then(|t| t.as_str()) == Some("file"))
        .filter_map(|f| {
            let path = f.get("path")?.as_str()?.to_owned();
            let size = f.get("lfs").and_then(|l| l.get("size")).and_then(|s| s.as_u64()).or_else(|| f.get("size").and_then(|s| s.as_u64())).unwrap_or(0);
            prefixes.iter().any(|p| path.starts_with(p)).then_some((path, size))
        })
        .collect();
    if files.is_empty() {
        return Err(format!("{repo} has none of the files that were asked for."));
    }
    for (i, (path, size)) in files.iter().enumerate() {
        let to = dest.join(path);
        if to.exists() && std::fs::metadata(&to).map(|m| m.len() == *size || *size == 0).unwrap_or(false) {
            continue;
        }
        let url = format!("https://huggingface.co/{repo}/resolve/main/{}", path.split('/').map(|s| s.replace(' ', "%20")).collect::<Vec<_>>().join("/"));
        fetch_file(&url, &to, &format!("{repo} ({} of {}) {path}", i + 1, files.len()), step)?;
    }
    Ok(())
}

/// The asset of a GitHub release whose name starts with `prefix` and ends with `suffix`: its name and address.
pub fn pick_asset(release: &Value, prefix: &str, suffix: &str) -> Option<(String, String)> {
    release.get("assets")?.as_array()?.iter().find_map(|a| {
        let name = a.get("name")?.as_str()?;
        if !(name.starts_with(prefix) && name.ends_with(suffix)) {
            return None;
        }
        Some((name.to_owned(), a.get("browser_download_url")?.as_str()?.to_owned()))
    })
}

fn github_latest(repo: &str) -> Result<Value, String> {
    agent()
        .get(&format!("https://api.github.com/repos/{repo}/releases/latest"))
        .set("User-Agent", "SeviathStudio")
        .call()
        .map_err(|_| format!("Could not ask GitHub for the newest {repo}. Is the internet on?"))?
        .into_json::<Value>()
        .map_err(|e| e.to_string())
}

/// Fetch one item. Returns a sentence that says what was done and where it is.
pub fn download(kind: Kind, step: &Sender<String>) -> Result<String, String> {
    let root = aisetup::root();
    match kind {
        Kind::TrackerModels => {
            for (path, url, what) in [(aisetup::tracker_model(), aisetup::FACE_MODEL_URL, "face model"), (aisetup::pose_model(), aisetup::POSE_MODEL_URL, "body model"), (aisetup::hand_model(), aisetup::HAND_MODEL_URL, "hand model")] {
                if !path.exists() {
                    fetch_file(url, &path, what, step)?;
                }
            }
            Ok(format!("The tracking models are in {}.", root.join("models").display()))
        }
        Kind::Everything => crate::setupall::install_everything(step),
        Kind::PictureAi => {
            crate::setupall::fetch_picture_models(step)?;
            Ok("The picture AI is in the cache of Hugging Face on this computer.".into())
        }
        Kind::TripoSr => {
            aisetup::fetch_triposr(step)?;
            Ok(format!("TripoSR is in {}. It also needs the Python files of the AI helpers (the card in the Rig tab).", root.display()))
        }
        Kind::UniRig => {
            let [skeleton, skin] = unirig_files();
            if !skeleton.exists() {
                fetch_file("https://huggingface.co/VAST-AI/UniRig/resolve/main/skeleton/articulation-xl_quantization_256/model.ckpt", &skeleton, "UniRig skeleton model", step)?;
            }
            if !skin.exists() {
                fetch_file("https://huggingface.co/VAST-AI/UniRig/resolve/main/skin/articulation-xl/model.ckpt", &skin, "UniRig skin model", step)?;
            }
            Ok(format!("The UniRig models are in {}.", root.join("models").join("UniRig").display()))
        }
        Kind::Trellis => {
            let dest = root.join("models").join("TRELLIS-image-large");
            fetch_hf_repo("microsoft/TRELLIS-image-large", &["ckpts/", "pipeline.json"], &dest, step)?;
            Ok(format!("The TRELLIS models are in {}. The code (github.com/microsoft/TRELLIS) is not downloaded: it is for trying by hand.", dest.display()))
        }
        Kind::CharacterGen => {
            crate::setupall::setup_charactergen(step)?;
            Ok(format!("CharacterGen is in {}. It is a Shape AI in the Generate tab now.", crate::setupall::charactergen_folder().display()))
        }
        Kind::Vrm4u => {
            let v = *unreal_versions().last().ok_or("Unreal Engine was not found in Program Files, Epic Games. Install it with the Epic Games Launcher first.")?;
            let _ = step.send("Asking GitHub for the newest VRM4U".into());
            let release = github_latest("ruyo/VRM4U")?;
            let prefix = format!("VRM4U_{}_{}_", v.0, v.1);
            let (name, url) = pick_asset(&release, &prefix, ".zip").ok_or_else(|| format!("VRM4U has no ready plugin for Unreal Engine {}.{} yet. See github.com/ruyo/VRM4U.", v.0, v.1))?;
            let folder = vrm4u_folder(v);
            let zip_path = folder.join(&name);
            fetch_file(&url, &zip_path, "VRM4U", step)?;
            let _ = step.send("Unpacking VRM4U".into());
            let bytes = std::fs::read(&zip_path).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("The VRM4U file is damaged: {e}"))?;
            zip.extract(folder.join("unpacked")).map_err(|e| format!("Could not unpack VRM4U: {e}"))?;
            // the plugin as it will be used: <folder>/Plugins/VRM4U
            let from = folder.join("unpacked").join("Plugins");
            let to = folder.join("Plugins");
            if to.exists() {
                let _ = std::fs::remove_dir_all(&to);
            }
            std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(folder.join("unpacked"));
            Ok(format!("VRM4U for Unreal Engine {}.{} is in {}. Copy the VRM4U folder from its Plugins folder into the Plugins folder of your project.", v.0, v.1, folder.display()))
        }
        Kind::UnityCapture => {
            let folder = downloads_folder().join("UnityCapture");
            let zip_path = folder.join("UnityCapture.zip");
            fetch_file("https://github.com/schellingb/UnityCapture/archive/refs/heads/master.zip", &zip_path, "Unity Capture", step)?;
            let bytes = std::fs::read(&zip_path).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("The Unity Capture file is damaged: {e}"))?;
            // only the Install folder (the filters and the scripts that register them) is wanted
            for i in 0..zip.len() {
                let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
                let name = f.name().map(|n| n.to_string()).unwrap_or_default();
                let Some(rel) = name.split_once("/Install/").map(|x| x.1.to_owned()) else { continue };
                if rel.is_empty() || f.is_dir() || rel.contains("..") {
                    continue;
                }
                let to = folder.join("Install").join(&rel);
                if let Some(d) = to.parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                let mut out = std::fs::File::create(&to).map_err(|e| e.to_string())?;
                std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
            }
            let _ = std::fs::remove_file(&zip_path);
            Ok(format!("Unity Capture is in {}. Run Install.bat in the Install folder (it asks to allow changes), then use Stream, Virtual camera, Unity Capture.", folder.display()))
        }
        Kind::Obs => {
            if let Some(p) = obs_installer() {
                return Ok(format!("The OBS installer is already in {}. Start it yourself.", p.display()));
            }
            let _ = step.send("Asking GitHub for the newest OBS Studio".into());
            let release = github_latest("obsproject/obs-studio")?;
            let (name, url) = pick_asset(&release, "OBS-Studio-", "-Windows-x64-Installer.exe").ok_or("The installer of OBS Studio was not found on its release page. Use obsproject.com/download.")?;
            let dest = downloads_folder().join(&name);
            fetch_file(&url, &dest, "OBS Studio", step)?;
            Ok(format!("The OBS installer is {}. Start it yourself, then press Start Virtual Camera once in OBS.", dest.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn every_download_has_a_name_a_size_and_a_licence_and_the_risky_one_says_so() {
        let list = items();
        assert_eq!(list.len(), 10);
        assert!(list.iter().all(|i| !i.name.is_empty() && !i.size.is_empty() && !i.licence.is_empty()));
        let warned: Vec<Kind> = list.iter().filter(|i| i.warning.is_some()).map(|i| i.kind).collect();
        assert_eq!(warned, vec![Kind::Trellis, Kind::CharacterGen], "the one that is not connected and the one that needs more set-up say so");
    }

    #[test]
    fn the_right_release_file_is_picked() {
        let v: Value = serde_json::from_str(r#"{"assets": [{"name": "VRM4U_5_7_1.zip", "browser_download_url": "http://x/57"}, {"name": "VRM4U_5_8_1.zip", "browser_download_url": "http://x/58"}, {"name": "OBS-Studio-32.0.1-Windows-x64-Installer.exe", "browser_download_url": "http://x/obs"}]}"#).unwrap();
        assert_eq!(pick_asset(&v, "VRM4U_5_8_", ".zip"), Some(("VRM4U_5_8_1.zip".into(), "http://x/58".into())));
        assert_eq!(pick_asset(&v, "OBS-Studio-", "-Windows-x64-Installer.exe").unwrap().1, "http://x/obs");
        assert_eq!(pick_asset(&v, "VRM4U_5_9_", ".zip"), None);
    }

    /// A small web server that sends a file of 3 MB, or stops short.
    fn serve(listener: TcpListener, short: bool) {
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten().take(2) {
                let mut s = stream;
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let body = vec![7u8; 3 << 20];
                let cut = if short { body.len() / 2 } else { body.len() };
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body[..cut]);
            }
        });
    }

    #[test]
    fn a_file_is_saved_whole_or_not_at_all() {
        let dir = std::env::temp_dir().join(format!("seviath_dl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, _rx) = std::sync::mpsc::channel();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        serve(l, false);
        let dest = dir.join("sub").join("file.bin");
        fetch_file(&format!("http://127.0.0.1:{port}/f"), &dest, "a test file", &tx).unwrap();
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), 3 << 20);
        assert!(!dest.with_extension("part").exists());
        // a download that stops short leaves nothing that looks finished
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        serve(l, true);
        let dest2 = dir.join("short.bin");
        let err = fetch_file(&format!("http://127.0.0.1:{port}/f"), &dest2, "a test file", &tx).unwrap_err();
        assert!(err.contains("stopped") || err.contains("not whole"), "{err}");
        assert!(!dest2.exists());
        // an address that is not there is said in words
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        assert!(fetch_file(&format!("http://127.0.0.1:{port}/f"), &dir.join("x.bin"), "a test file", &tx).unwrap_err().contains("Could not reach"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
