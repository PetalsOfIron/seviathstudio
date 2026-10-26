// SPDX-License-Identifier: GPL-3.0-or-later
// App settings, saved as JSON in your user config folder
// (Windows: %APPDATA%\SeviathStudio\settings.json) and loaded on start.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CursorStyle {
    /// Brush-size circle.
    Outline,
    /// Circle with a small cross in the middle.
    OutlineCross,
    /// Only a small cross (precise lineart).
    Cross,
    /// A single dot.
    Dot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WheelAction {
    /// Wheel zooms at the pointer (Clip Studio style).
    Zoom,
    /// Wheel scrolls up/down, Shift+wheel sideways, Ctrl+wheel zooms (Krita/Photoshop style).
    Scroll,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // ---- Interface
    pub ui_scale: f32,
    pub high_contrast: bool,
    pub reduced_motion: bool,
    pub show_tooltips: bool,
    /// A small ! on every button, slider and switch that has a help text (hover it to read what it does).
    pub help_marks: bool,
    pub left_handed: bool,
    /// How sharp the character is drawn in the Rig, Physics, Animate and Stream tabs: 0 Potato to 4 Max (see `preview_px`).
    pub rig_quality: u8,
    /// How many pictures a second the Stream tab sends to the browser source (OBS).
    pub stream_fps: u32,
    /// The AIs that are set up (see aibridge.rs) and the one in use.
    pub ai_providers: Vec<crate::aibridge::Provider>,
    pub ai_active: usize,
    /// The address of the ComfyUI that makes pictures (see comfy.rs).
    pub comfy_url: String,
    /// Keys you changed: action id → shortcut text ("" = no key). See actions.rs.
    pub shortcuts: std::collections::BTreeMap<String, String>,
    // ---- Canvas & view
    pub wheel: WheelAction,
    pub zoom_speed: f32,
    pub invert_wheel: bool,
    pub rotate_snap: f32, // degrees for Shift+R-drag and the rotate buttons
    pub pixel_grid: bool,
    pub show_guides: bool, // the project's guide lines (File, Project info)
    pub pixel_grid_from: f32, // zoom (e.g. 8 = 800%) where the pixel grid appears
    pub smooth_zoomed_in: bool, // false = see crisp pixels when zoomed in
    pub canvas_bg: [u8; 3],
    pub show_scales: bool,
    pub checker_size: u32,
    pub page_shadow: bool,
    // ---- Cursor
    pub cursor: CursorStyle,
    pub cursor_min_size: f32, // keep the circle at least this big on screen (px)
    pub hide_cursor_while_drawing: bool,
    // ---- Pen & tablet
    pub pressure_curve: f32, // gamma: <1 soft (light pressure counts more), >1 firm
    pub pressure_min: f32,   // pen pressure at or below this counts as 0
    pub pressure_max: f32,   // at or above this counts as full
    pub mouse_pressure: f32,
    pub pressure_smoothing: f32, // 0..1, calms jumpy pressure readings
    pub tremor_help: f32,        // 0..1, extra steadying for shaky hands (adds to every brush's stabilizer)
    pub min_stroke_move: f32,    // screen px the pen must move before a stroke starts (stops accidental dots)
    // ---- Brushes
    pub keep_size_on_switch: bool,
    pub brush_size_max: f32,
    pub size_step: f32, // [ and ] change the size by this factor
    // ---- Color
    pub picker_samples: u32, // 1 = exact pixel, 3 = average of 3×3, 5 = 5×5
    pub swatches_max: usize,
    // ---- Undo
    pub undo_steps: usize,
    pub undo_memory_mb: usize,
    pub warn_other_tab_undo: bool,
    // ---- Performance
    pub brush_preview_per_frame: u32,
    pub navigator_fps: f32,
    // ---- Files
    /// Where projects are kept (empty = Documents\SeviathStudio\Projects).
    pub projects_dir: String,
    pub autosave_minutes: f32, // 0 = off; recovery copies are made this often while you work
    pub recovery_copies: usize,
    pub autosave_project_file: bool, // also save the project file itself automatically
    pub jpeg_quality: u8,
    pub start_with_projects: bool,
    /// Write "made with SeviathStudio" into exported PNGs (software field, not author).
    pub export_credit: bool,
    pub recent: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            high_contrast: false,
            reduced_motion: false,
            show_tooltips: true,
            help_marks: true,
            left_handed: false,
            rig_quality: 2,
            stream_fps: 30,
            ai_providers: Vec::new(),
            ai_active: 0,
            comfy_url: "http://127.0.0.1:8188".into(),
            shortcuts: Default::default(),
            wheel: WheelAction::Zoom,
            zoom_speed: 1.0,
            invert_wheel: false,
            rotate_snap: 15.0,
            pixel_grid: true,
            show_guides: true,
            pixel_grid_from: 10.0,
            smooth_zoomed_in: false,
            canvas_bg: [10, 14, 12],
            show_scales: true,
            checker_size: 16,
            page_shadow: true,
            cursor: CursorStyle::OutlineCross,
            cursor_min_size: 3.0,
            hide_cursor_while_drawing: false,
            pressure_curve: 1.0,
            pressure_min: 0.0,
            pressure_max: 1.0,
            mouse_pressure: 1.0,
            pressure_smoothing: 0.0,
            tremor_help: 0.0,
            min_stroke_move: 0.0,
            keep_size_on_switch: true,
            brush_size_max: 1000.0,
            size_step: 1.15,
            picker_samples: 1,
            swatches_max: 40,
            undo_steps: 200,
            undo_memory_mb: 2048,
            warn_other_tab_undo: true,
            brush_preview_per_frame: 3,
            navigator_fps: 6.0,
            projects_dir: String::new(),
            autosave_minutes: 3.0,
            recovery_copies: 10,
            autosave_project_file: false,
            jpeg_quality: 92,
            start_with_projects: true,
            export_credit: true,
            recent: Vec::new(),
        }
    }
}

impl Settings {
    fn path() -> Option<std::path::PathBuf> {
        Some(dirs::config_dir()?.join("SeviathStudio").join("settings.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(|s| s.sanitized())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let p = Self::path().ok_or("No settings folder")?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // write to a temporary file first, so a crash never leaves a broken settings file
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
    }

    /// Keep every value in a safe range (hand-edited or older files).
    pub fn sanitized(mut self) -> Self {
        self.ui_scale = self.ui_scale.clamp(0.6, 2.5);
        self.rig_quality = self.rig_quality.min(4);
        self.stream_fps = self.stream_fps.clamp(1, 120);
        self.zoom_speed = self.zoom_speed.clamp(0.2, 4.0);
        self.rotate_snap = self.rotate_snap.clamp(1.0, 90.0);
        self.pixel_grid_from = self.pixel_grid_from.clamp(4.0, 64.0);
        self.checker_size = self.checker_size.clamp(4, 64);
        self.cursor_min_size = self.cursor_min_size.clamp(0.0, 30.0);
        self.shortcuts.retain(|id, s| crate::actions::def(id).is_some() && (s.is_empty() || crate::actions::Shortcut::parse(s).is_some()));
        self.pressure_curve = self.pressure_curve.clamp(0.2, 5.0);
        self.pressure_min = self.pressure_min.clamp(0.0, 0.9);
        self.pressure_max = self.pressure_max.clamp(self.pressure_min + 0.05, 1.0);
        self.mouse_pressure = self.mouse_pressure.clamp(0.05, 1.0);
        self.pressure_smoothing = self.pressure_smoothing.clamp(0.0, 0.95);
        self.tremor_help = self.tremor_help.clamp(0.0, 1.0);
        self.min_stroke_move = self.min_stroke_move.clamp(0.0, 20.0);
        self.brush_size_max = self.brush_size_max.clamp(50.0, 5000.0);
        self.size_step = self.size_step.clamp(1.02, 2.0);
        self.picker_samples = [1u32, 3, 5].into_iter().min_by_key(|v| v.abs_diff(self.picker_samples)).unwrap_or(1);
        self.swatches_max = self.swatches_max.clamp(8, 200);
        self.undo_steps = self.undo_steps.clamp(5, 2000);
        self.undo_memory_mb = self.undo_memory_mb.clamp(64, 32768);
        self.brush_preview_per_frame = self.brush_preview_per_frame.clamp(1, 20);
        self.navigator_fps = self.navigator_fps.clamp(1.0, 30.0);
        self.autosave_minutes = self.autosave_minutes.clamp(0.0, 120.0);
        self.recovery_copies = self.recovery_copies.clamp(1, 200);
        self.jpeg_quality = self.jpeg_quality.clamp(10, 100);
        self.recent.truncate(12);
        self
    }

    /// The folder where projects live.
    pub fn projects_folder(&self) -> std::path::PathBuf {
        if !self.projects_dir.trim().is_empty() {
            return std::path::PathBuf::from(self.projects_dir.trim());
        }
        dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| std::path::PathBuf::from(".")).join("SeviathStudio").join("Projects")
    }

    /// Where automatic recovery copies go.
    pub fn recovery_folder() -> std::path::PathBuf {
        dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("recovery")
    }

    /// Remember a file in the recent list (newest first).
    pub fn add_recent(&mut self, path: &std::path::Path) {
        let p = path.to_string_lossy().into_owned();
        self.recent.retain(|r| r != &p);
        self.recent.insert(0, p);
        self.recent.truncate(12);
    }

    /// Raw pen pressure → pressure used by brushes (range, then curve).
    pub fn pressure(&self, raw: f32) -> f32 {
        let span = (self.pressure_max - self.pressure_min).max(0.01);
        let t = ((raw - self.pressure_min) / span).clamp(0.0, 1.0);
        t.powf(self.pressure_curve)
    }
}

/// The names of the quality steps and the longest side of the picture of the character at each (the drawing is done by the
/// processor, so a weaker computer can pick a lower step).
pub const RIG_QUALITY: [(&str, f32); 5] = [("Potato", 400.0), ("Low", 600.0), ("Medium", 900.0), ("High", 1400.0), ("Max", 2000.0)];

impl Settings {
    pub fn preview_px(&self) -> f32 {
        RIG_QUALITY[(self.rig_quality as usize).min(RIG_QUALITY.len() - 1)].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_curve_and_range() {
        let mut s = Settings::default();
        assert!((s.pressure(0.5) - 0.5).abs() < 1e-6);
        s.pressure_curve = 2.0;
        assert!((s.pressure(0.5) - 0.25).abs() < 1e-6);
        s.pressure_min = 0.2;
        s.pressure_max = 0.6;
        assert_eq!(s.pressure(0.1), 0.0);
        assert_eq!(s.pressure(0.9), 1.0);
    }

    #[test]
    fn json_round_trip_and_missing_fields() {
        let s = Settings { ui_scale: 1.3, cursor: CursorStyle::Dot, ..Settings::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        // an older file with fewer fields still loads, the rest gets defaults
        let old: Settings = serde_json::from_str(r#"{"ui_scale": 1.5}"#).unwrap();
        assert_eq!(old.ui_scale, 1.5);
        assert_eq!(old.undo_steps, 200);
        // silly values are clamped
        assert_eq!((old.rig_quality, old.stream_fps, old.preview_px()), (2, 30, 900.0));
        assert_eq!(Settings { rig_quality: 200, ..Settings::default() }.preview_px(), 2000.0, "a silly step is the highest");
        let bad = Settings { ui_scale: 99.0, pressure_min: 0.95, ..Settings::default() }.sanitized();
        assert!(bad.ui_scale <= 2.5 && bad.pressure_max > bad.pressure_min);
    }
}
