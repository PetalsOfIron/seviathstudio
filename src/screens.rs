// Visual checks: drive the app with simulated input, render offscreen, write PNGs to target/screens/.
// Run with `cargo test screens -- --nocapture`.

use eframe::egui::{Color32, Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, pos2, vec2};
use egui_kittest::Harness;

use crate::app::SeviathApp;

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
    h.state_mut().draw_mut().size = 24.0;
    stroke(&mut h, &wave(pos2(520.0, 300.0), pos2(940.0, 300.0), 40.0, 40));
    h.state_mut().draw_mut().size = 60.0;
    h.state_mut().draw_mut().primary = Color32::from_rgb(46, 191, 113);
    stroke(&mut h, &wave(pos2(520.0, 480.0), pos2(940.0, 520.0), 60.0, 50));
    h.state_mut().draw_mut().size = 8.0;
    h.state_mut().draw_mut().primary = Color32::from_rgb(214, 168, 76);
    stroke(&mut h, &wave(pos2(560.0, 650.0), pos2(900.0, 640.0), 25.0, 60));
    // Eraser through the green stroke.
    h.key_press(Key::E);
    h.state_mut().draw_mut().size = 30.0;
    stroke(&mut h, &[pos2(730.0, 380.0), pos2(730.0, 490.0), pos2(730.0, 600.0)]);
    h.key_press(Key::B);
    h.run_steps(2);
    save(&mut h, "01_draw");

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

    h.key_press_modifiers(Modifiers::COMMAND, Key::Num3);
    h.run_steps(3);
    save(&mut h, "04_rig");
}
