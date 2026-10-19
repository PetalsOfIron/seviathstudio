// SPDX-License-Identifier: GPL-3.0-or-later
// The AI helper window (File, AI helper): set up one or more AIs (a program on this computer such as Ollama, or a service with a free
// key such as Gemini), then ask for a brush from words, colours from words, or a look at the drawing. The AI only ever writes text;
// what it writes is read and checked by aibridge.rs before anything changes, and a brush or a palette is added to what is there,
// never over a drawing.

use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, Align2, Color32, RichText};

use super::SeviathApp;
use crate::aibridge::{self, Kind, Provider};
use crate::theme::*;
use crate::widgets::{self, section_title};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Purpose {
    Brush,
    Colors,
    Check,
    Test,
}

/// The window part that asks a ComfyUI for a picture.
pub struct ComfyUi {
    checkpoints: Vec<String>,
    ckpt: usize,
    prompt: String,
    negative: String,
    width: u32,
    height: u32,
    /// A workflow of the user's own (saved from ComfyUI as API format), else the plain one.
    workflow: Option<std::path::PathBuf>,
    job: Option<Receiver<Result<image::RgbaImage, String>>>,
    listing: Option<Receiver<Result<Vec<String>, String>>>,
}

impl Default for ComfyUi {
    fn default() -> Self {
        Self { checkpoints: Vec::new(), ckpt: 0, prompt: String::new(), negative: "blurry, low quality".into(), width: 768, height: 1024, workflow: None, job: None, listing: None }
    }
}

#[derive(Default)]
pub struct AiUi {
    comfy: ComfyUi,
    pub open: bool,
    prompt_brush: String,
    prompt_colors: String,
    job: Option<(Purpose, Receiver<Result<String, String>>)>,
    pub message: Option<String>,
    pub check: Option<String>,
    show_setup: bool,
}

impl SeviathApp {
    pub(super) fn open_ai_helper(&mut self) {
        self.ai.open = true;
        if self.settings.ai_providers.is_empty() {
            self.ai.show_setup = true;
        }
    }

    fn ai_provider(&self) -> Option<Provider> {
        self.settings.ai_providers.get(self.settings.ai_active).or_else(|| self.settings.ai_providers.first()).cloned()
    }

    /// Ask the AI in a thread of its own; the answer is picked up by `ai_poll`.
    fn ai_ask(&mut self, purpose: Purpose, system: String, user: String, images: Vec<Vec<u8>>) {
        let Some(p) = self.ai_provider() else {
            self.ai.message = Some("Set up an AI first.".into());
            return;
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(p.chat(&system, &user, &images));
        });
        self.ai.job = Some((purpose, rx));
        self.ai.message = Some("Asking the AI...".into());
    }

    /// ComfyUI: the list of checkpoints and the finished picture are picked up here.
    fn comfy_poll(&mut self, ctx: &egui::Context) {
        if self.ai.comfy.job.is_some() || self.ai.comfy.listing.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if let Some(rx) = &self.ai.comfy.listing {
            if let Ok(r) = rx.try_recv() {
                self.ai.comfy.listing = None;
                match r {
                    Ok(list) => {
                        self.ai.message = Some(format!("ComfyUI has {} checkpoints.", list.len()));
                        self.ai.comfy.ckpt = 0;
                        self.ai.comfy.checkpoints = list;
                    }
                    Err(e) => self.ai.message = Some(e),
                }
            }
        }
        if let Some(rx) = &self.ai.comfy.job {
            if let Ok(r) = rx.try_recv() {
                self.ai.comfy.job = None;
                match r {
                    Ok(img) => {
                        self.draw.import_picture_layer(&img, "ComfyUI");
                        self.ai.message = Some("The picture is a new layer in the Draw tab.".into());
                    }
                    Err(e) => self.ai.message = Some(e),
                }
            }
        }
    }

    fn comfy_ui(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "A picture from ComfyUI");
        ui.label(RichText::new("Asks the ComfyUI that runs on this computer to make a picture, and puts it on a new layer. Start ComfyUI first; nothing is sent anywhere else.").size(11.5).color(TEXT_DIM));
        let busy = self.ai.comfy.job.is_some();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.settings.comfy_url).desired_width(220.0));
            if ui.add_enabled(self.ai.comfy.listing.is_none(), egui::Button::new("Find its checkpoints")).clicked() {
                let url = self.settings.comfy_url.clone();
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::comfy::Comfy::new(&url).checkpoints());
                });
                self.ai.comfy.listing = Some(rx);
                self.ai.message = Some("Asking ComfyUI...".into());
            }
        });
        let c = &mut self.ai.comfy;
        let using_own = c.workflow.is_some();
        if !using_own && !c.checkpoints.is_empty() {
            egui::ComboBox::from_id_salt("comfy_ckpt").selected_text(c.checkpoints.get(c.ckpt).cloned().unwrap_or_default()).width(300.0).show_ui(ui, |ui| {
                for (i, n) in c.checkpoints.iter().enumerate() {
                    ui.selectable_value(&mut c.ckpt, i, n);
                }
            });
        }
        ui.add(egui::TextEdit::singleline(&mut c.prompt).hint_text("a fox in the snow, soft light").desired_width(400.0));
        ui.add(egui::TextEdit::singleline(&mut c.negative).hint_text("what to leave out").desired_width(400.0));
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut c.width).range(256..=2048).speed(8.0).prefix("width "));
            ui.add(egui::DragValue::new(&mut c.height).range(256..=2048).speed(8.0).prefix("height "));
        });
        ui.horizontal(|ui| {
            if ui.button("Use my own workflow…").on_hover_text("A workflow saved from ComfyUI with Save (API format). Put {{prompt}}, {{negative}}, {{seed}}, {{width}}, {{height}} and {{checkpoint}} where they belong.").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("ComfyUI workflow", &["json"]).pick_file() {
                    c.workflow = Some(p);
                }
            }
            if let Some(p) = &c.workflow {
                ui.label(RichText::new(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).size(11.5).color(TEXT_DIM));
                if ui.small_button("Use the plain one").clicked() {
                    c.workflow = None;
                }
            }
        });
        let ready = !c.prompt.trim().is_empty() && (using_own || !c.checkpoints.is_empty());
        if ui.add_enabled(ready && !busy, egui::Button::new(RichText::new("Make the picture").strong())).on_disabled_hover_text("Write what you want, and find the checkpoints first").clicked() {
            let url = self.settings.comfy_url.clone();
            let template = match &c.workflow {
                Some(p) => std::fs::read_to_string(p).map_err(|e| e.to_string()),
                None => Ok(crate::comfy::DEFAULT_WORKFLOW.to_string()),
            };
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) % 4_000_000_000;
            let fill = crate::comfy::Fill { prompt: c.prompt.trim().to_owned(), negative: c.negative.clone(), checkpoint: c.checkpoints.get(c.ckpt).cloned().unwrap_or_default(), seed, width: c.width / 8 * 8, height: c.height / 8 * 8 };
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(template.and_then(|t| crate::comfy::Comfy::new(&url).make(&t, &fill)));
            });
            c.job = Some(rx);
            self.ai.message = Some("ComfyUI is making the picture (it can take a minute)...".into());
        }
    }

    /// What the AI answered becomes a brush, colours, or text to read.
    pub(super) fn ai_poll(&mut self, ctx: &egui::Context) {
        self.comfy_poll(ctx);
        let Some((purpose, rx)) = &self.ai.job else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
        let Ok(answer) = rx.try_recv() else { return };
        let purpose = *purpose;
        self.ai.job = None;
        self.ai.message = Some(match answer {
            Err(e) => e,
            Ok(text) => match purpose {
                Purpose::Test => format!("The AI answered: {}", text.chars().take(80).collect::<String>().replace('\n', " ")),
                Purpose::Brush => match aibridge::brush_from(&text) {
                    Ok(b) => {
                        *self.draw.brush_mut() = b;
                        "The brush is set up: try it on the canvas. (Save it in My brushes if you like it.)".to_string()
                    }
                    Err(e) => e,
                },
                Purpose::Colors => {
                    let cols: Vec<Color32> = aibridge::colors_from(&text).iter().map(|c| Color32::from_rgb(c[0], c[1], c[2])).collect();
                    if cols.is_empty() {
                        "The AI did not give colours I can read.".to_string()
                    } else {
                        let n = cols.len();
                        self.draw.add_swatches(&cols);
                        format!("{n} colours are in the swatches of the Draw tab.")
                    }
                }
                Purpose::Check => {
                    self.ai.check = Some(text);
                    "Done: the notes are below.".to_string()
                }
            },
        });
    }

    pub(super) fn ai_window(&mut self, ctx: &egui::Context) {
        self.ai_poll(ctx);
        if !self.ai.open {
            return;
        }
        let mut open = true;
        egui::Window::new(RichText::new("AI helper").strong()).open(&mut open).collapsible(false).default_width(470.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Uses an AI you already have: a program on this computer (Ollama) or a service with a free key. Nothing is sent anywhere until you press a button, and what the AI writes is checked before it is used.").size(11.5).color(TEXT_DIM));
            self.ai_helper_setup_ui(ui);
            let busy = self.ai.job.is_some();
            let have = self.ai_provider().is_some();
            ui.add_space(6.0);
            section_title(ui, "A brush from words");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.ai.prompt_brush).hint_text("a soft chalky pencil with grain").desired_width(300.0));
                if ui.add_enabled(have && !busy && !self.ai.prompt_brush.trim().is_empty(), egui::Button::new("Make the brush")).clicked() {
                    let user = format!("Design this brush: {}", self.ai.prompt_brush.trim());
                    self.ai_ask(Purpose::Brush, aibridge::brush_system_prompt(), user, Vec::new());
                }
            });
            section_title(ui, "Colours from words");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.ai.prompt_colors).hint_text("autumn dusk by the sea").desired_width(300.0));
                if ui.add_enabled(have && !busy && !self.ai.prompt_colors.trim().is_empty(), egui::Button::new("Make the colours")).clicked() {
                    let user = format!("Colours for: {}", self.ai.prompt_colors.trim());
                    self.ai_ask(Purpose::Colors, aibridge::COLORS_SYSTEM.to_string(), user, Vec::new());
                }
            });
            section_title(ui, "A look at my drawing");
            ui.label(RichText::new("Sends the whole picture, made smaller, to the AI. It needs an AI that can look at pictures (for Ollama a vision model such as llava).").size(11.5).color(TEXT_DIM));
            if ui.add_enabled(have && !busy, egui::Button::new("Look at my drawing")).clicked() {
                let img = crate::files::flatten(&self.draw.doc);
                let k = (768.0 / img.width().max(img.height()) as f32).min(1.0);
                let small = image::imageops::resize(&img, ((img.width() as f32 * k) as u32).max(1), ((img.height() as f32 * k) as u32).max(1), image::imageops::FilterType::Triangle);
                let mut png = std::io::Cursor::new(Vec::new());
                let _ = small.write_to(&mut png, image::ImageFormat::Png);
                self.ai_ask(Purpose::Check, aibridge::CHECK_SYSTEM.to_string(), "Look at this drawing.".to_string(), vec![png.into_inner()]);
            }
            self.comfy_ui(ui);
            if let Some(m) = &self.ai.message {
                ui.label(RichText::new(m).size(11.5).color(GOLD));
            }
            if let Some(t) = &self.ai.check {
                egui::ScrollArea::vertical().max_height(220.0).id_salt("ai_check").show(ui, |ui| {
                    ui.label(RichText::new(t).size(12.0));
                });
            }
        });
        self.ai.open = open;
    }

    /// The AIs that are set up, and how to add one.
    fn ai_helper_setup_ui(&mut self, ui: &mut egui::Ui) {
        let title = format!("Set up ({})", self.settings.ai_providers.len());
        egui::CollapsingHeader::new(RichText::new(title).size(13.0)).id_salt("ai_setup").open(if self.ai.show_setup { Some(true) } else { None }).show(ui, |ui| {
            self.ai.show_setup = false;
            ui.label(RichText::new("The key is kept in the settings file on this computer. A free key for Gemini: aistudio.google.com, Get API key.").size(11.0).color(TEXT_DIM));
            let mut remove: Option<usize> = None;
            let n = self.settings.ai_providers.len();
            for i in 0..n {
                let active = self.settings.ai_active == i;
                ui.horizontal(|ui| {
                    if widgets::chip(ui, if active { "In use" } else { "Use" }, active).clicked() {
                        self.settings.ai_active = i;
                    }
                });
                let p = &mut self.settings.ai_providers[i];
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(120.0));
                    egui::ComboBox::from_id_salt(("ai_kind", i)).selected_text(match p.kind { Kind::Ollama => "Ollama", Kind::OpenAi => "OpenAI style" }).width(100.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.kind, Kind::Ollama, "Ollama");
                        ui.selectable_value(&mut p.kind, Kind::OpenAi, "OpenAI style");
                    });
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Address").size(11.5).color(TEXT_DIM));
                    ui.add(egui::TextEdit::singleline(&mut p.url).desired_width(300.0));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Model").size(11.5).color(TEXT_DIM));
                    ui.add(egui::TextEdit::singleline(&mut p.model).desired_width(130.0));
                    ui.label(RichText::new("Key").size(11.5).color(TEXT_DIM));
                    ui.add(egui::TextEdit::singleline(&mut p.key).password(true).desired_width(110.0));
                });
                ui.separator();
            }
            if let Some(i) = remove {
                self.settings.ai_providers.remove(i);
                self.settings.ai_active = self.settings.ai_active.min(self.settings.ai_providers.len().saturating_sub(1));
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("Add Ollama").clicked() {
                    self.settings.ai_providers.push(Provider::ollama());
                }
                if ui.button("Add Gemini").clicked() {
                    self.settings.ai_providers.push(Provider::gemini());
                }
                if ui.button("Add another server").clicked() {
                    self.settings.ai_providers.push(Provider::openai_compatible());
                }
                if ui.add_enabled(self.ai.job.is_none() && !self.settings.ai_providers.is_empty(), egui::Button::new("Test the one in use")).clicked() {
                    self.ai_ask(Purpose::Test, "Answer with one short word.".into(), "Say ok.".into(), Vec::new());
                }
            });
        });
    }

    #[cfg(test)]
    pub fn test_ai(&mut self) -> &mut AiUi {
        &mut self.ai
    }

    #[cfg(test)]
    pub fn test_ai_ask(&mut self, brush: &str) {
        self.ai_ask(Purpose::Brush, aibridge::brush_system_prompt(), format!("Design this brush: {brush}"), Vec::new());
    }

    #[cfg(test)]
    pub fn test_ai_colors(&mut self, words: &str) {
        self.ai_ask(Purpose::Colors, aibridge::COLORS_SYSTEM.to_string(), format!("Colours for: {words}"), Vec::new());
    }
}

#[cfg(test)]
impl SeviathApp {
    pub fn test_set_ai(&mut self, p: Provider) {
        self.settings.ai_providers = vec![p];
        self.settings.ai_active = 0;
    }
}
