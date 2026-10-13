// SPDX-License-Identifier: GPL-3.0-or-later
// Visual checks: drive the app with simulated input, render offscreen, write PNGs to target/screens/.
// Run with `cargo test screens -- --nocapture`.

use eframe::egui::{Color32, Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, pos2, vec2};
use egui_kittest::Harness;

use crate::app::SeviathApp;
use crate::doc::Blend;

fn save(h: &mut Harness<'_, SeviathApp>, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
    std::fs::create_dir_all(&dir).unwrap();
    let img = h.render().expect("render");
    let path = dir.join(format!("{name}.png"));
    img.save(&path).unwrap();
    println!("wrote {}", path.display());
}

fn button(h: &Harness<'_, SeviathApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
}

/// A stroke through `pts`, one pointer sample per frame.
fn stroke(h: &mut Harness<'_, SeviathApp>, pts: &[Pos2]) {
    h.hover_at(pts[0]);
    h.step();
    button(h, pts[0], true);
    h.step();
    for p in &pts[1..] {
        h.hover_at(*p);
        h.step();
    }
    button(h, *pts.last().unwrap(), false);
    h.step();
}

fn wave(from: Pos2, to: Pos2, amp: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            from + (to - from) * t + vec2(0.0, (t * std::f32::consts::TAU * 1.5).sin() * amp)
        })
        .collect()
}

fn wheel(h: &Harness<'_, SeviathApp>, at: Pos2, lines: f32, modifiers: Modifiers) {
    h.hover_at(at);
    h.event(Event::MouseWheel { unit: MouseWheelUnit::Line, delta: Vec2::new(0.0, lines), phase: TouchPhase::Move, modifiers });
}

#[test]
fn screens() {
    let mut h = Harness::builder()
        .with_size([1440.0, 900.0])
        .wgpu()
        .build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);

    // Paint a few strokes with different sizes and colors.
    h.state_mut().draw_mut().brush_mut().size = 24.0;
    stroke(&mut h, &wave(pos2(520.0, 300.0), pos2(940.0, 300.0), 40.0, 40));
    h.state_mut().draw_mut().brush_mut().size = 60.0;
    h.state_mut().draw_mut().primary = Color32::from_rgb(46, 191, 113);
    stroke(&mut h, &wave(pos2(520.0, 480.0), pos2(940.0, 520.0), 60.0, 50));
    h.state_mut().draw_mut().brush_mut().size = 8.0;
    h.state_mut().draw_mut().primary = Color32::from_rgb(214, 168, 76);
    stroke(&mut h, &wave(pos2(560.0, 650.0), pos2(900.0, 640.0), 25.0, 60));
    // Eraser through the green stroke.
    h.key_press(Key::E);
    h.state_mut().draw_mut().brush_mut().size = 30.0;
    stroke(&mut h, &[pos2(730.0, 380.0), pos2(730.0, 490.0), pos2(730.0, 600.0)]);
    h.key_press(Key::B);
    h.run_steps(2);
    save(&mut h, "01_draw");

    // Every stroke became a named history step; Ctrl+Z / Ctrl+Y walk through them.
    let n = h.state().history().entries.len();
    assert_eq!(n, 4, "steps: {:?}", h.state().history().entries.iter().map(|e| e.label.clone()).collect::<Vec<_>>());
    assert!(h.state().history().entries[3].label.starts_with("Erase"), "{}", h.state().history().entries[3].label);
    let painted = |h: &Harness<'_, SeviathApp>| h.state().draw_mut_ref().doc.layers[0].tiles.len();
    let tiles_before = painted(&h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().history().cursor, n - 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(painted(&h) <= tiles_before);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(2);
    assert_eq!(h.state().history().cursor, n);
    assert_eq!(painted(&h), tiles_before);

    // Zoom in at the pointer with the wheel, mirror with F, rotate with Alt+wheel.
    wheel(&h, pos2(730.0, 480.0), 4.0, Modifiers::NONE);
    h.run_steps(2);
    h.key_press(Key::F);
    for _ in 0..2 {
        wheel(&h, pos2(730.0, 480.0), 4.5, Modifiers::ALT);
        h.step();
    }
    h.run_steps(3);
    save(&mut h, "02_zoom_rotate_mirror");

    // Fit back (Ctrl+0), straighten, un-mirror.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num0);
    h.state_mut().draw_mut().straighten();
    h.key_press(Key::F);
    h.run_steps(3);
    save(&mut h, "03_fit");

    // Layers: a purple Multiply layer clipped to the strokes below it.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    h.step();
    {
        let d = h.state_mut().draw_mut();
        d.brush_mut().size = 170.0;
        d.primary = Color32::from_rgb(150, 110, 230);
    }
    stroke(&mut h, &wave(pos2(470.0, 360.0), pos2(990.0, 600.0), 20.0, 30));
    {
        let d = h.state_mut().draw_mut();
        d.doc.layers[0].blend = Blend::Multiply;
        d.doc.layers[0].clip = true;
        d.doc.mark_all_dirty();
    }
    h.run_steps(3);
    save(&mut h, "05_clip_multiply");

    // Put the strokes layer into a folder: the clip now uses the folder and the picture stays the same.
    h.state_mut().draw_mut().select(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.run_steps(4);
    save(&mut h, "06_folder");

    // Organize: hide the color card, right-click a layer for its menu.
    h.state_mut().draw_mut().set_color_open(false);
    h.run_steps(3);
    let row = h.state().draw_mut_ref().layer_row(0).expect("row of the top layer");
    h.hover_at(row.center());
    h.step();
    h.event(Event::PointerButton { pos: row.center(), button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerButton { pos: row.center(), button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    save(&mut h, "07_layer_menu");
    h.key_press(Key::Escape);
    h.run_steps(2);

    // References: a colorful picture in the Sub view, laid over the canvas, color taken by a click.
    {
        let pic = image::RgbaImage::from_fn(320, 240, |x, y| {
            let h = x as f32 / 320.0;
            let v = 1.0 - y as f32 / 300.0;
            let c = eframe::egui::Color32::from(eframe::egui::ecolor::Hsva::new(h, 0.85, v, 1.0));
            image::Rgba([c.r(), c.g(), c.b(), 255])
        });
        let mut png = Vec::new();
        pic.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let d = h.state_mut().draw_mut();
        d.refs.add_bytes("rainbow", &png).unwrap();
        d.refs.overlay = true;
        d.refs.grid = true;
        d.show_references();
        d.set_color_open(false);
    }
    h.run_steps(4);
    save(&mut h, "11_references");
    let before = h.state().draw_mut_ref().primary;
    let target = h.state().draw_mut_ref().reference_view_rect().expect("reference view").center();
    h.hover_at(target);
    h.step();
    button(&h, target, true);
    h.step();
    button(&h, target, false);
    h.run_steps(2);
    assert_ne!(h.state().draw_mut_ref().primary, before, "clicking the reference should take its color");

    // History window and settings pages
    h.state_mut().open_window(true, None);
    h.run_steps(3);
    save(&mut h, "08_history");
    h.state_mut().open_window(false, Some(crate::settings_ui::Page::Pen));
    h.run_steps(3);
    save(&mut h, "09_settings_pen");
    h.state_mut().open_window(false, Some(crate::settings_ui::Page::Canvas));
    h.run_steps(3);
    save(&mut h, "10_settings_canvas");

    h.key_press_modifiers(Modifiers::COMMAND, Key::Num3);
    h.run_steps(3);
    save(&mut h, "04_rig");
}
