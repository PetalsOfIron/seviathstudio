// SPDX-License-Identifier: GPL-3.0-or-later
// Optional AI helpers. They are small Python programs that the user installs the models for; the app writes the helper
// next to its settings, runs it as a separate program on one picture, and reads one line of JSON back. Only one helper
// runs at a time (the graphics card is small), and the app works without any of them: it falls back to its own guess.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use image::RgbaImage;
use serde_json::Value;

use crate::vrm::face::FaceMarks;

const FACE_SCRIPT: &str = include_str!("../tools/ai/face_points.py");
const SHAPE_SCRIPT: &str = include_str!("../tools/ai/shape3d.py");

/// Only one helper at a time.
static BUSY: Mutex<()> = Mutex::new(());

pub fn helper_folder() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("ai")
}

/// Run a Python program with arguments and read the last line of its output as JSON.
pub fn run_json(python: &str, script: &Path, args: &[String]) -> Result<Value, String> {
    let _one_at_a_time = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    let mut cmd = Command::new(python.trim());
    cmd.arg(script).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| format!("Could not start {python} ({e}). Install Python 3.10 or newer and give its command here."))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().rev().find(|l| l.trim_start().starts_with('{'));
    match line.and_then(|l| serde_json::from_str::<Value>(l).ok()) {
        Some(v) => match v.get("error").and_then(|e| e.as_str()) {
            Some(e) => Err(e.to_string()),
            None => Ok(v),
        },
        None => {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = err.lines().rev().take(3).collect();
            Err(format!("The helper gave no answer. {}", tail.into_iter().rev().collect::<Vec<_>>().join(" ")))
        }
    }
}

/// The face points from the helper's answer.
pub fn parse_face(v: &Value) -> Result<FaceMarks, String> {
    let pt = |k: &str| -> Option<[f32; 2]> {
        let a = v.get(k)?.as_array()?;
        Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32])
    };
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32);
    let (eye_l, eye_r, mouth) = (pt("eye_l"), pt("eye_r"), pt("mouth"));
    let (Some(eye_l), Some(eye_r), Some(mouth)) = (eye_l, eye_r, mouth) else { return Err("The helper's answer has no eyes and mouth.".into()) };
    let dist = (eye_l[0] - eye_r[0]).hypot(eye_l[1] - eye_r[1]);
    let eye_w = num("eye_w").filter(|w| *w > 1.0).unwrap_or(dist * 0.36);
    let mouth_w = num("mouth_w").filter(|w| *w > 1.0).unwrap_or(dist * 0.8);
    FaceMarks { eye_l, eye_r, mouth, eye_w, mouth_w }.sanitized().ok_or_else(|| "The helper's answer has numbers that make no sense.".to_string())
}

/// Find the eyes and the mouth in a picture. `crop` is (x, y, width, height) of the head in page pixels; `backend` is
/// "auto", "mediapipe" (real faces) or "dino" (drawings).
pub fn find_face(python: &str, page: &RgbaImage, crop: [f32; 4], backend: &str) -> Result<FaceMarks, String> {
    let dir = helper_folder();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not make the helper folder: {e}"))?;
    let script = dir.join("face_points.py");
    std::fs::write(&script, FACE_SCRIPT).map_err(|e| format!("Could not write the helper: {e}"))?;
    let png = dir.join("face_input.png");
    image::DynamicImage::ImageRgba8(page.clone()).to_rgb8().save(&png).map_err(|e| format!("Could not write the picture: {e}"))?;
    let args = vec![
        "--image".to_string(),
        png.to_string_lossy().to_string(),
        "--backend".to_string(),
        backend.to_string(),
        "--crop".to_string(),
        format!("{:.0}", crop[0]),
        format!("{:.0}", crop[1]),
        format!("{:.0}", crop[2]),
        format!("{:.0}", crop[3]),
    ];
    let v = run_json(python, &script, &args)?;
    parse_face(&v)
}

/// Where the Hunyuan3D code is: next to the Python environment, in the SeviathAI folder.
pub fn hunyuan_folder() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("SeviathAI").join("Hunyuan3D-2")
}

/// Make the shape of the character from the picture with Hunyuan3D 2 mini: the bytes of a GLB with the points and triangles.
/// It takes a minute or two on the graphics card, and the first time it downloads the model.
pub fn make_shape(python: &str, page: &RgbaImage, turbo: bool) -> Result<Vec<u8>, String> {
    let repo = hunyuan_folder();
    if !repo.join("hy3dgen").exists() {
        return Err(format!("The Hunyuan3D code is not in {} (clone https://github.com/Tencent/Hunyuan3D-2 there).", repo.display()));
    }
    let dir = helper_folder();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not make the helper folder: {e}"))?;
    let script = dir.join("shape3d.py");
    std::fs::write(&script, SHAPE_SCRIPT).map_err(|e| format!("Could not write the helper: {e}"))?;
    let png = dir.join("shape_input.png");
    page.save(&png).map_err(|e| format!("Could not write the picture: {e}"))?;
    let out = dir.join("shape.glb");
    let _ = std::fs::remove_file(&out);
    let mut args = vec!["--image".to_string(), png.to_string_lossy().to_string(), "--out".to_string(), out.to_string_lossy().to_string(), "--repo".to_string(), repo.to_string_lossy().to_string()];
    if turbo {
        args.push("--turbo".into());
    }
    run_json(python, &script, &args)?;
    std::fs::read(&out).map_err(|e| format!("The shape file was not made: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn python() -> Option<String> {
        let ok = Command::new("python").arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
        ok.then(|| "python".to_string())
    }

    #[test]
    fn the_answer_of_a_helper_becomes_face_points() {
        let v: Value = serde_json::from_str(r#"{"eye_l":[600,400],"eye_r":[400,402],"mouth":[500,600],"eye_w":70,"mouth_w":90,"backend":"x"}"#).unwrap();
        let f = parse_face(&v).unwrap();
        assert_eq!(f.eye_l, [600.0, 400.0]);
        assert_eq!((f.eye_w, f.mouth_w), (70.0, 90.0));
        // sizes missing: taken from the distance between the eyes
        let v: Value = serde_json::from_str(r#"{"eye_l":[600,400],"eye_r":[400,400],"mouth":[500,600]}"#).unwrap();
        let f = parse_face(&v).unwrap();
        assert!((f.eye_w - 72.0).abs() < 0.1 && (f.mouth_w - 160.0).abs() < 0.1);
        assert!(parse_face(&serde_json::from_str::<Value>(r#"{"eye_l":[1,2]}"#).unwrap()).is_err());
    }

    #[test]
    fn a_helper_program_is_run_and_its_json_read_and_its_errors_passed_on() {
        let Some(py) = python() else { return };
        let dir = std::env::temp_dir().join("seviath_ai_test");
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.py");
        std::fs::write(&ok, "import sys, json\nprint('loading...')\nprint(json.dumps({'eye_l':[3,1],'eye_r':[1,1],'mouth':[2,3],'args':sys.argv[1:]}))\n").unwrap();
        let v = run_json(&py, &ok, &["a".into(), "b".into()]).unwrap();
        assert_eq!(v["args"][1], "b");
        assert!(parse_face(&v).is_ok());
        let bad = dir.join("bad.py");
        std::fs::write(&bad, "import json, sys\nprint(json.dumps({'error': 'no face here'}))\nsys.exit(1)\n").unwrap();
        assert_eq!(run_json(&py, &bad, &[]).unwrap_err(), "no face here");
        let crash = dir.join("crash.py");
        std::fs::write(&crash, "raise RuntimeError('boom')\n").unwrap();
        assert!(run_json(&py, &crash, &[]).unwrap_err().contains("boom"));
        // a program that does not exist
        assert!(run_json("definitely-not-python-xyz", &ok, &[]).unwrap_err().contains("Install Python"));
    }

    #[test]
    fn the_face_helper_says_plainly_what_is_missing() {
        let Some(py) = python() else { return };
        let page = RgbaImage::from_pixel(64, 64, image::Rgba([200, 180, 160, 255]));
        match find_face(&py, &page, [0.0, 0.0, 64.0, 64.0], "mediapipe") {
            // nothing installed: a clear message; installed: no face in a flat picture is also a clear message
            Err(e) => assert!(e.contains("mediapipe") || e.contains("Pillow"), "{e}"),
            Ok(_) => panic!("a flat picture has no face"),
        }
    }
}
