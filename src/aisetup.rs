// SPDX-License-Identifier: GPL-3.0-or-later
// The AI tools are not part of the app (they are large and have their own licences). They live in a folder of their own
// (`SeviathAI` in the user's home folder, or where `SEVIATH_AI_ROOT` says): a private Python environment with the packages, and the
// code of Hunyuan3D. When they are missing the app says so and offers a button that downloads them: it makes the environment, installs
// the packages with pip, and fetches the Hunyuan3D code as a zip from GitHub. The models themselves (a few GB) are fetched from
// Hugging Face by the helpers the first time they run. Everything is done by programs the computer already has (Python, curl).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

/// The folder of the AI tools.
#[cfg(test)]
thread_local! {
    /// A folder to use as the AI folder in the tests of the thread (so they do not look at the real one).
    pub static TEST_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub fn root() -> PathBuf {
    #[cfg(test)]
    if let Some(p) = TEST_ROOT.with(|r| r.borrow().clone()) {
        return p;
    }
    if let Ok(p) = std::env::var("SEVIATH_AI_ROOT") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("SeviathAI")
}

pub fn venv_python(root: &Path) -> PathBuf {
    if cfg!(windows) { root.join("venv").join("Scripts").join("python.exe") } else { root.join("venv").join("bin").join("python") }
}

fn site_packages(root: &Path) -> PathBuf {
    if cfg!(windows) { root.join("venv").join("Lib").join("site-packages") } else { root.join("venv").join("lib") }
}

/// What is there and what is not.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub python_env: bool,
    pub packages: bool,
    pub hunyuan: bool,
}

impl Status {
    pub fn ready(&self) -> bool {
        self.python_env && self.packages && self.hunyuan
    }

    /// What is missing, in words.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.python_env {
            v.push("a Python environment");
        }
        if !self.packages {
            v.push("the AI packages (PyTorch, MediaPipe, Transformers...)");
        }
        if !self.hunyuan {
            v.push("the Hunyuan3D code");
        }
        v
    }
}

const FACE_MODEL_URL: &str = "https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/latest/face_landmarker.task";

/// The face model of the camera tracker.
pub fn tracker_model() -> PathBuf {
    root().join("models").join("face_landmarker.task")
}

/// Can the camera tracker start: the Python environment and the face model are there.
pub fn tracker_ready() -> bool {
    venv_python(&root()).exists() && tracker_model().exists()
}

/// Look at the folder (only at the files, nothing is started).
pub fn status() -> Status {
    status_in(&root())
}

pub fn status_in(root: &Path) -> Status {
    let python_env = venv_python(root).exists();
    let packages = python_env && (site_packages(root).join("torch").exists() || site_packages(root).join("python3.10").join("site-packages").join("torch").exists()) && root.join("venv").exists();
    let hunyuan = root.join("Hunyuan3D-2").join("hy3dgen").exists();
    Status { python_env, packages, hunyuan }
}

/// A Python of a version the AI packages exist for (3.10 to 3.12), found among the ones the computer has: the command to start it.
pub fn find_python() -> Option<Vec<String>> {
    let tries: [&[&str]; 6] = [&["py", "-3.10"], &["py", "-3.11"], &["py", "-3.12"], &["python3.10"], &["python3"], &["python"]];
    for t in tries {
        let mut cmd = Command::new(t[0]);
        cmd.args(&t[1..]).arg("--version").stdout(Stdio::piped()).stderr(Stdio::piped());
        hide_window(&mut cmd);
        if let Ok(out) = cmd.output() {
            let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
            if out.status.success() && version_ok(&text) {
                return Some(t.iter().map(|s| s.to_string()).collect());
            }
        }
    }
    None
}

/// Is this "Python 3.x.y" one of 3.10, 3.11, 3.12?
pub fn version_ok(text: &str) -> bool {
    let v = text.trim().trim_start_matches("Python").trim();
    let mut parts = v.split('.');
    let (major, minor) = (parts.next().and_then(|p| p.parse::<u32>().ok()), parts.next().and_then(|p| p.parse::<u32>().ok()));
    major == Some(3) && matches!(minor, Some(10..=12))
}

fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd;
}

/// The packages the helpers need, with the versions that were tested together.
pub const PACKAGES: &[&str] = &["numpy<2", "pillow", "mediapipe==0.10.14", "transformers==4.51.3", "tokenizers==0.21.4", "trimesh", "pygltflib", "diffusers", "einops", "omegaconf", "tqdm", "accelerate", "pymeshlab", "xatlas", "safetensors", "scikit-image", "opencv-python<4.12"];
pub const TORCH: &[&str] = &["torch==2.5.1", "torchvision==0.20.1"];
pub const TORCH_INDEX: &str = "https://download.pytorch.org/whl/cu121";
pub const HUNYUAN_ZIP: &str = "https://github.com/Tencent/Hunyuan3D-2/archive/refs/heads/main.zip";

/// Unpack a zip into `dest`, dropping the first folder of every path (a GitHub archive has one folder with the name of the branch).
pub fn unzip_stripped(bytes: &[u8], dest: &Path) -> Result<usize, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("The download is not a zip file: {e}"))?;
    let mut n = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = f.enclosed_name() else { continue };
        let rel: PathBuf = name.components().skip(1).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            std::fs::write(&out, buf).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

/// Run a program and write what it says into the log file; Err with the end of the log when it fails.
fn run_logged(mut cmd: Command, log: &Path) -> Result<(), String> {
    let file = std::fs::OpenOptions::new().create(true).append(true).open(log).map_err(|e| e.to_string())?;
    let file2 = file.try_clone().map_err(|e| e.to_string())?;
    cmd.stdout(Stdio::from(file)).stderr(Stdio::from(file2));
    hide_window(&mut cmd);
    let status = cmd.status().map_err(|e| format!("Could not start {:?}: {e}", cmd.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("A step failed ({:?}). The end of the log:\n{}", cmd.get_program(), tail(log, 6)))
    }
}

/// The last lines of the log file.
pub fn tail(log: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let v: Vec<&str> = text.lines().rev().filter(|l| !l.trim().is_empty()).take(lines).collect();
    v.into_iter().rev().collect::<Vec<_>>().join("\n")
}

pub fn log_path() -> PathBuf {
    root().join("setup.log")
}

/// Download and install everything that is missing. `step` tells what is being done now (for the window); the long output goes to
/// `setup.log` in the folder of the tools. Takes a good while (several GB).
pub fn install(step: &Sender<String>) -> Result<(), String> {
    let root = root();
    std::fs::create_dir_all(&root).map_err(|e| format!("Could not make {}: {e}", root.display()))?;
    let log = log_path();
    let have = status_in(&root);
    if !have.python_env {
        let _ = step.send("Making a Python environment".into());
        let py = find_python().ok_or("Python 3.10, 3.11 or 3.12 is needed (python.org). Install it and press the button again.")?;
        let mut cmd = Command::new(&py[0]);
        cmd.args(&py[1..]).args(["-m", "venv"]).arg(root.join("venv"));
        run_logged(cmd, &log)?;
    }
    let python = venv_python(&root);
    if !have.packages {
        let _ = step.send("Installing PyTorch (about 2.5 GB)".into());
        let mut cmd = Command::new(&python);
        cmd.args(["-m", "pip", "install", "--upgrade", "pip"]);
        run_logged(cmd, &log)?;
        let mut cmd = Command::new(&python);
        cmd.args(["-m", "pip", "install"]).args(TORCH).args(["--index-url", TORCH_INDEX]);
        run_logged(cmd, &log)?;
        let _ = step.send("Installing the other packages".into());
        let mut cmd = Command::new(&python);
        cmd.args(["-m", "pip", "install"]).args(PACKAGES);
        run_logged(cmd, &log)?;
    }
    if !have.hunyuan {
        let _ = step.send("Downloading the Hunyuan3D code".into());
        let zip_path = root.join("hunyuan3d.zip");
        let mut cmd = Command::new("curl");
        cmd.args(["-L", "--fail", "--silent", "--show-error", "-o"]).arg(&zip_path).arg(HUNYUAN_ZIP);
        run_logged(cmd, &log)?;
        let bytes = std::fs::read(&zip_path).map_err(|e| e.to_string())?;
        unzip_stripped(&bytes, &root.join("Hunyuan3D-2"))?;
        let _ = std::fs::remove_file(&zip_path);
    }
    if !tracker_model().exists() {
        let _ = step.send("Downloading the face model for the camera (4 MB)".into());
        std::fs::create_dir_all(root.join("models")).map_err(|e| e.to_string())?;
        let mut cmd = Command::new("curl");
        cmd.args(["-L", "--fail", "--silent", "--show-error", "-o"]).arg(tracker_model()).arg(FACE_MODEL_URL);
        run_logged(cmd, &log)?;
    }
    // a last look: do the packages start?
    let _ = step.send("Checking that it works".into());
    let mut cmd = Command::new(&python);
    cmd.args(["-c", "import torch, mediapipe, transformers, diffusers, trimesh; print('ok', torch.cuda.is_available())"]);
    run_logged(cmd, &log)?;
    let now = status_in(&root);
    if now.ready() { Ok(()) } else { Err(format!("Something is still missing: {}", now.missing().join(", "))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("seviath_aisetup_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_files_are_found_and_said_in_words_and_complete_ones_are_ready() {
        let d = temp("status");
        let s = status_in(&d);
        assert!(!s.ready());
        assert_eq!(s.missing().len(), 3);
        // a made environment with the packages and the code
        let py = venv_python(&d);
        std::fs::create_dir_all(py.parent().unwrap()).unwrap();
        std::fs::write(&py, b"").unwrap();
        std::fs::create_dir_all(site_packages(&d).join("torch")).unwrap();
        let s = status_in(&d);
        assert!(s.python_env && s.packages && !s.hunyuan);
        assert_eq!(s.missing(), vec!["the Hunyuan3D code"]);
        std::fs::create_dir_all(d.join("Hunyuan3D-2").join("hy3dgen")).unwrap();
        assert!(status_in(&d).ready());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_the_python_versions_the_packages_exist_for_are_taken() {
        for ok in ["Python 3.10.11", "Python 3.11.4\n", "Python 3.12.0"] {
            assert!(version_ok(ok), "{ok}");
        }
        for bad in ["Python 3.9.7", "Python 3.15.0", "Python 2.7.18", "nonsense", ""] {
            assert!(!version_ok(bad), "{bad}");
        }
    }

    #[test]
    fn a_github_zip_is_unpacked_without_its_first_folder() {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.add_directory("Hunyuan3D-2-main/", o).unwrap();
            z.start_file("Hunyuan3D-2-main/hy3dgen/__init__.py", o).unwrap();
            z.write_all(b"x = 1\n").unwrap();
            z.start_file("Hunyuan3D-2-main/README.md", o).unwrap();
            z.write_all(b"hello").unwrap();
            z.finish().unwrap();
        }
        let d = temp("unzip");
        let n = unzip_stripped(&buf.into_inner(), &d.join("Hunyuan3D-2")).unwrap();
        assert_eq!(n, 2);
        assert!(d.join("Hunyuan3D-2").join("hy3dgen").join("__init__.py").exists());
        assert!(d.join("Hunyuan3D-2").join("README.md").exists());
        assert!(unzip_stripped(b"not a zip", &d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_end_of_a_log_is_what_the_user_sees_when_a_step_fails() {
        let d = temp("log");
        let f = d.join("setup.log");
        std::fs::write(&f, "one\ntwo\n\nthree\nfour\n").unwrap();
        assert_eq!(tail(&f, 2), "three\nfour");
        assert_eq!(tail(&d.join("nothing.log"), 3), "");
        let _ = std::fs::remove_dir_all(&d);
    }
}
