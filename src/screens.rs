// SPDX-License-Identifier: GPL-3.0-or-later
// Visual checks: drive the app with simulated input, render offscreen, write PNGs to target/screens/.
// Run with `cargo test screens -- --nocapture`.

use eframe::egui::{Color32, Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, pos2, vec2};
use egui_kittest::Harness;

use crate::app::SeviathApp;
use crate::doc::Blend;

/// Only one test at a time uses the graphics card (each one opens its own device, and the computer
/// may have little video memory). The lock is held until the test ends.
fn lock_gpu() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn save(h: &mut Harness<'_, SeviathApp>, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
    std::fs::create_dir_all(&dir).unwrap();
    // many tests share the GPU, so a render can time out: try again a few times
    let mut tries = 0;
    let img = loop {
        match h.render() {
            Ok(img) => break img,
            Err(e) if tries < 6 => {
                tries += 1;
                eprintln!("render retry {tries}: {e}");
                std::thread::sleep(std::time::Duration::from_millis(250 * tries));
            }
            Err(e) => panic!("render: {e:?}"),
        }
    };
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
    let _gpu = lock_gpu();
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

/// Steps until the background file job is done.
fn wait_job(h: &mut Harness<'_, SeviathApp>) {
    for _ in 0..600 {
        h.step();
        if !h.state().test_busy() {
            h.step();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("file job never finished");
}

#[test]
fn files_screens() {
    use crate::files::{project, testdoc};
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-projects");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sample = testdoc::sample();
    project::save(sample.w, sample.h, sample.paper, &sample.layers, &[], &Default::default(), &dir.join("Hair study.seviath")).unwrap();
    let mut big = crate::doc::Document::new(400, 600);
    big.paper = Color32::from_rgb(40, 60, 50);
    project::save(big.w, big.h, big.paper, &big.layers, &[], &Default::default(), &dir.join("Night sky.seviath")).unwrap();

    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.state_mut().settings_mut().projects_dir = dir.display().to_string();
    h.state_mut().files_mut().manager_open = true;
    h.run_steps(8);
    save(&mut h, "projects_manager");
    h.state_mut().files_mut().manager_open = false;
    h.state_mut().files_mut().new_open = true;
    h.run_steps(4);
    save(&mut h, "new_project");
    h.state_mut().files_mut().new_open = false;
    h.run_steps(2);

    // open a project: the document is replaced and nothing is unsaved
    h.state_mut().test_open(dir.join("Hair study.seviath"));
    wait_job(&mut h);
    assert_eq!(testdoc::shape(&h.state().draw_mut_ref().doc), testdoc::shape(&sample));
    assert!(!h.state().test_dirty());
    assert_eq!(h.state().file_name(), "Hair study");

    // paint: now there are unsaved changes; Ctrl+S saves them in the background
    stroke(&mut h, &wave(pos2(500.0, 400.0), pos2(800.0, 450.0), 30.0, 12));
    assert!(h.state().test_dirty());
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    wait_job(&mut h);
    assert!(!h.state().test_dirty());
    let back = project::load(&dir.join("Hair study.seviath")).unwrap();
    assert_eq!(back.layers.len(), h.state().draw_mut_ref().doc.layers.len());

    // opening another file with unsaved changes asks first
    stroke(&mut h, &wave(pos2(500.0, 500.0), pos2(800.0, 550.0), 30.0, 8));
    h.state_mut().test_open(dir.join("Night sky.seviath"));
    h.run_steps(3);
    assert!(h.state_mut().files_mut().pending.is_some());
    save(&mut h, "unsaved_changes");
}

/// Alpha of the layer `li` at a screen position.
fn alpha_at(h: &Harness<'_, SeviathApp>, li: usize, at: Pos2) -> u8 {
    let d = h.state().draw_mut_ref();
    let p = d.screen_to_doc(at);
    let img = crate::files::layer_rgba(&d.doc.layers[li], p.x as i32, p.y as i32, 1, 1);
    img.get_pixel(0, 0).0[3]
}

#[test]
fn selection_screens() {
    use crate::symmetry::SymMode;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let labels = |h: &Harness<'_, SeviathApp>| h.state().history().entries.iter().map(|e| e.label.clone()).collect::<Vec<_>>();

    // Rectangle selection, then a brush stroke only paints inside it.
    h.key_press(Key::M);
    stroke(&mut h, &[pos2(560.0, 300.0), pos2(700.0, 450.0), pos2(820.0, 560.0)]);
    h.run_steps(2);
    assert!(h.state().draw_mut_ref().doc.selection.is_some());
    h.key_press(Key::B);
    h.state_mut().draw_mut().brush_mut().size = 40.0;
    stroke(&mut h, &wave(pos2(480.0, 430.0), pos2(900.0, 430.0), 10.0, 30));
    h.run_steps(2);
    assert!(alpha_at(&h, 0, pos2(760.0, 430.0)) > 0, "inside the selection");
    assert_eq!(alpha_at(&h, 0, pos2(500.0, 430.0)), 0, "left of the selection");
    assert_eq!(alpha_at(&h, 0, pos2(880.0, 430.0)), 0, "right of the selection");
    save(&mut h, "sel_01_rectangle_clip");

    // Move & transform the selected paint: Ctrl+T, then drag inside, then Enter.
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    h.run_steps(2);
    assert!(h.state().draw_mut_ref().transforming());
    stroke(&mut h, &[pos2(760.0, 430.0), pos2(770.0, 480.0), pos2(780.0, 530.0)]);
    h.state_mut().draw_mut().tools_mut().floating.as_mut().unwrap().rotate(0.4);
    h.run_steps(2);
    save(&mut h, "sel_02_transform");
    h.state_mut().draw_mut().tools_mut().floating.as_mut().unwrap().rotate(-0.4);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(!h.state().draw_mut_ref().transforming());
    assert!(labels(&h).last().unwrap().starts_with("Transform"), "{:?}", labels(&h));
    assert!(alpha_at(&h, 0, pos2(780.0, 530.0)) > 0, "moved paint is at its new place");

    // Quick mask: Q shows the selection as a red layer, Q again turns it back into a selection.
    h.key_press(Key::Q);
    h.run_steps(2);
    assert_eq!(h.state().draw_mut_ref().doc.layers[0].name, "Quick mask");
    assert!(h.state().draw_mut_ref().doc.selection.is_none());
    h.state_mut().draw_mut().brush_mut().size = 80.0;
    stroke(&mut h, &wave(pos2(500.0, 700.0), pos2(900.0, 700.0), 20.0, 20));
    h.run_steps(2);
    save(&mut h, "sel_03_quick_mask");
    h.key_press(Key::Q);
    h.run_steps(2);
    assert_ne!(h.state().draw_mut_ref().doc.layers[0].name, "Quick mask");
    let sel_ok = {
        let d = h.state().draw_mut_ref();
        let p = d.screen_to_doc(pos2(700.0, 680.0));
        d.doc.selection.as_ref().map(|s| s.at(p.x as i32, p.y as i32)).unwrap_or(0)
    };
    assert!(sel_ok > 200, "the painted quick mask became selection: {sel_ok}"); // brush opacity 95%

    // Deselect, then symmetry: one stroke on the left appears on the right too.
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run_steps(2);
    assert!(h.state().draw_mut_ref().doc.selection.is_none());
    let center = {
        let d = h.state().draw_mut_ref();
        (d.doc.w as f32 / 2.0, d.doc.h as f32 / 2.0)
    };
    {
        let t = h.state_mut().draw_mut().tools_mut();
        t.sym.mode = SymMode::Vertical;
        t.sym.cx = center.0;
        t.sym.cy = center.1;
        t.sym_open = true;
    }
    h.state_mut().draw_mut().brush_mut().size = 30.0;
    h.state_mut().draw_mut().primary = Color32::from_rgb(46, 150, 230);
    stroke(&mut h, &wave(pos2(520.0, 200.0), pos2(600.0, 330.0), 15.0, 20));
    h.run_steps(3);
    let (left, right) = {
        let d = h.state().draw_mut_ref();
        let p = d.screen_to_doc(pos2(547.0, 258.0));
        let mirrored = (2.0 * center.0 - p.x, p.y);
        // the most paint near each point (the stabilizer smooths the line a little off the raw path)
        let most = |x: f32, y: f32| {
            let img = crate::files::layer_rgba(&d.doc.layers[0], x as i32 - 100, y as i32 - 100, 200, 200);
            img.pixels().map(|p| p.0[3]).max().unwrap()
        };
        (most(p.x, p.y), most(mirrored.0, mirrored.1))
    };
    save(&mut h, "sel_04_symmetry");
    assert!(left > 0 && right > 0, "mirror copy painted: {left} {right}");

    // Lasso fill: draw a shape with the Fill tool in lasso mode.
    h.state_mut().draw_mut().tools_mut().sym.mode = SymMode::Off;
    h.state_mut().draw_mut().tools_mut().sym_open = false;
    h.key_press(Key::G);
    h.state_mut().draw_mut().tools_mut().fill_lasso = true;
    h.state_mut().draw_mut().primary = Color32::from_rgb(214, 168, 76);
    stroke(&mut h, &[pos2(880.0, 600.0), pos2(960.0, 620.0), pos2(950.0, 720.0), pos2(870.0, 700.0), pos2(860.0, 640.0)]);
    h.run_steps(2);
    assert!(alpha_at(&h, 0, pos2(910.0, 660.0)) > 200, "lasso fill painted");
    assert!(labels(&h).last().unwrap().starts_with("Lasso fill"), "{:?}", labels(&h));

    // Magic wand on the filled shape, then the options panel and selection bar are shown.
    h.key_press(Key::M);
    h.state_mut().draw_mut().tools_mut().sel_shape = crate::tabs::draw::SelShape::Wand;
    h.hover_at(pos2(910.0, 660.0));
    h.step();
    button(&h, pos2(910.0, 660.0), true);
    h.step();
    button(&h, pos2(910.0, 660.0), false);
    h.run_steps(3);
    let wand_ok = {
        let d = h.state().draw_mut_ref();
        let p = d.screen_to_doc(pos2(910.0, 660.0));
        let q = d.screen_to_doc(pos2(1000.0, 800.0));
        let s = d.doc.selection.as_ref().expect("wand selected something");
        (s.at(p.x as i32, p.y as i32), s.at(q.x as i32, q.y as i32))
    };
    assert_eq!(wand_ok, (255, 0));
    save(&mut h, "sel_05_wand");

    // Undo walks back through it all, selection included.
    let n = h.state().history().entries.len();
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().history().cursor, n - 1);
    assert!(h.state().draw_mut_ref().doc.selection.is_none(), "undoing the wand removes its selection");
}

#[test]
fn info_screen() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().open_window(false, Some(crate::settings_ui::Page::Info));
    h.run_steps(4);
    save(&mut h, "info_screen");
}

fn add_layer_named(d: &mut crate::tabs::draw::DrawState, name: &str, f: impl FnOnce(&mut crate::doc::Layer)) -> crate::doc::LayerId {
    let mut l = d.doc.new_layer(name, crate::doc::LayerKind::Raster);
    f(&mut l);
    let top = d.doc.layers[0].id;
    d.doc.insert_layer(l, top)
}

#[test]
fn organizer_screens() {
    use crate::doc::Blend;
    use crate::layerfilter::{ChipKind, SortBy};
    use crate::tabs::draw::organizer::SortBySafe;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let (hair_front, skin, eyes, folder);
    {
        let d = h.state_mut().draw_mut();
        hair_front = add_layer_named(d, "Hair front", |l| l.tag = 3);
        add_layer_named(d, "Hair back", |l| {
            l.tag = 3;
            l.locked = true;
        });
        skin = add_layer_named(d, "Skin", |l| {
            l.blend = Blend::Multiply;
            l.fav = true;
        });
        eyes = add_layer_named(d, "Eyes", |l| l.tag = 5);
        folder = d.doc.group(eyes, "Face").unwrap();
    }
    h.run_steps(3);
    let names = |h: &Harness<'_, SeviathApp>| {
        let mut v = h.state().draw_mut_ref().shown_layer_names();
        v.sort();
        v
    };

    // Filters stack, NOT flips one, saved presets live in the project.
    h.state_mut().draw_mut().layer_ui_mut().filter.query = "hair".into();
    assert_eq!(names(&h), ["Hair back", "Hair front"]);
    h.state_mut().draw_mut().layer_ui_mut().filter.cycle(ChipKind::Locked);
    assert_eq!(names(&h), ["Hair back"]);
    h.state_mut().draw_mut().layer_ui_mut().filter.cycle(ChipKind::Locked);
    assert_eq!(names(&h), ["Hair front"], "NOT locked");
    h.state_mut().draw_mut().layer_ui_mut().filter.clear();
    h.state_mut().draw_mut().layer_ui_mut().filter.cycle(ChipKind::Blend(Blend::Multiply));
    assert_eq!(names(&h), ["Skin"]);
    h.state_mut().draw_mut().layer_ui_mut().filter.clear();
    h.state_mut().draw_mut().layer_ui_mut().filter.cycle(ChipKind::Favorite);
    assert_eq!(names(&h), ["Skin"]);
    h.state_mut().draw_mut().layer_ui_mut().filter.clear();

    // Sorting by name gives a flat, alphabetical list.
    h.state_mut().draw_mut().layer_ui_mut().sort = SortBySafe(SortBy::Name);
    let sorted = h.state().draw_mut_ref().shown_layer_names();
    let mut alpha = sorted.clone();
    alpha.sort_by_key(|n| n.to_lowercase());
    assert_eq!(sorted, alpha);
    h.state_mut().draw_mut().layer_ui_mut().sort = SortBySafe(SortBy::Z);

    // Keyboard flow: Alt+L focuses the search box, typing jumps to the match.
    h.key_press_modifiers(Modifiers::ALT, Key::L);
    h.run_steps(3);
    h.event(Event::Text("ski".into()));
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().active_name(), "Skin");
    assert_eq!(h.state().draw_mut_ref().shown_layer_names(), ["Skin"]);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().shown_layer_names().len() > 3, "Escape clears the search");

    // make room for the layer rows
    h.state_mut().draw_mut().set_color_open(false);
    h.run_steps(3);
    // Ctrl + click on rows picks several layers; a batch edit hits all of them and is one history step.
    let rows: Vec<Pos2> = ["Hair front", "Skin"]
        .iter()
        .map(|n| {
            let d = h.state().draw_mut_ref();
            let idx = d.doc.layers.iter().position(|l| l.name == *n).unwrap();
            d.layer_row(idx).expect("row is shown").center()
        })
        .collect();
    h.hover_at(rows[0]);
    h.step();
    button(&h, rows[0], true);
    h.step();
    button(&h, rows[0], false);
    h.run_steps(2);
    h.hover_at(rows[1]);
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::COMMAND));
    h.step();
    h.event(Event::PointerButton { pos: rows[1], button: PointerButton::Primary, pressed: true, modifiers: Modifiers::COMMAND });
    h.step();
    h.event(Event::PointerButton { pos: rows[1], button: PointerButton::Primary, pressed: false, modifiers: Modifiers::COMMAND });
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().layer_ui_count(), 2);
    save(&mut h, "org_01_picked");
    h.state_mut().draw_mut().test_batch_hide();
    assert_eq!(h.state().draw_mut_ref().pending_label().as_deref(), Some("Hide · 2 layers"), "named as one step");
    h.run_steps(2);
    {
        let d = h.state().draw_mut_ref();
        let hidden = d.doc.layers.iter().filter(|l| !l.visible).map(|l| l.name.clone()).collect::<Vec<_>>();
        assert_eq!(hidden.len(), 2, "{hidden:?}");
    }
    h.state_mut().draw_mut().layer_ui_mut().multi.clear();

    // Delete a folder and put it back with "Oops".
    let before: Vec<String> = h.state().draw_mut_ref().doc.layers.iter().map(|l| l.name.clone()).collect();
    h.state_mut().draw_mut().test_delete_layer(folder);
    h.run_steps(2);
    assert_eq!(h.state().draw_mut_ref().doc.layers.len(), before.len() - 2, "the folder and the layer inside it");
    h.state_mut().draw_mut().test_restore_deleted();
    h.run_steps(2);
    let after: Vec<String> = h.state().draw_mut_ref().doc.layers.iter().map(|l| l.name.clone()).collect();
    assert_eq!(after, before);
    {
        let d = h.state().draw_mut_ref();
        let f = d.doc.layers.iter().find(|l| l.name == "Face").unwrap().id;
        assert_eq!(d.doc.layers.iter().find(|l| l.name == "Eyes").unwrap().parent, Some(f), "folder contents came back inside it");
    }

    // New layers beside one, clipped to it; select the pixels of a layer.
    h.state_mut().draw_mut().test_layer_beside(skin, true, true);
    {
        let d = h.state().draw_mut_ref();
        let i = d.doc.layers.iter().position(|l| l.name == "Skin").unwrap();
        assert!(d.doc.layers[i - 1].clip, "the new layer sits right above Skin and is clipped");
    }
    h.state_mut().draw_mut().test_layer_beside(skin, false, false);
    {
        let d = h.state().draw_mut_ref();
        let i = d.doc.layers.iter().position(|l| l.name == "Skin").unwrap();
        assert!(d.doc.layers[i + 1].name.starts_with("Layer"), "{}", d.doc.layers[i + 1].name);
    }
    {
        let d = h.state_mut().draw_mut();
        let img = image::RgbaImage::from_pixel(120, 80, image::Rgba([10, 200, 90, 255]));
        let i = d.doc.layers.iter().position(|l| l.id == hair_front).unwrap();
        d.doc.layers[i].tiles = crate::files::tiles_from_rgba(&img, 300, 400, d.doc.w, d.doc.h);
        d.doc.layers[i].rev += 1;
    }
    h.state_mut().draw_mut().test_select_pixels(hair_front);
    {
        let d = h.state().draw_mut_ref();
        assert_eq!(d.doc.selection.as_ref().and_then(|s| s.bounds()), Some((300, 400, 420, 480)));
    }

    // Save a filter, look at the drawer, and the quick bar.
    {
        let d = h.state_mut().draw_mut();
        let ui = d.layer_ui_mut();
        ui.filter.query = "hair".into();
        ui.filter.cycle(ChipKind::Locked);
        ui.filter.cycle(ChipKind::Locked);
        ui.filters_open = true;
        let preset = crate::layerfilter::Preset { name: "Hair, free".into(), filter: ui.filter.clone() };
        d.doc.filter_presets.push(preset);
    }
    h.run_steps(3);
    save(&mut h, "org_02_filters");
    h.state_mut().draw_mut().layer_ui_mut().filter.clear();
    h.state_mut().draw_mut().layer_ui_mut().filters_open = false;
    h.state_mut().draw_mut().test_delete_layer(eyes);
    h.hover_at(pos2(700.0, 400.0));
    h.step();
    h.key_press_modifiers(Modifiers::COMMAND, Key::L);
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().layer_ui_mut_ref().quick.is_some());
    save(&mut h, "org_03_quick_bar");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().draw_mut_ref().layer_ui_mut_ref().quick.is_none());
}

#[test]
fn newproject_screens() {
    use crate::files::template::{Background, NewProject};
    use crate::meta::{GuidePreset, ProjectMeta};
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().files_mut().new_open = true;
    h.run_steps(10);
    save(&mut h, "np_01_dialog");
    h.state_mut().files_mut().new_open = false;
    h.run_steps(2);

    // Create a rig-ready 300 DPI project with a gradient background, thirds guides and tags.
    let np = NewProject {
        name: "Bust sheet".into(),
        w: 2480,
        h: 3508,
        dpi: 300,
        bg: Background::Gradient { top: [250, 240, 230], bottom: [120, 170, 150] },
        guides: GuidePreset::Thirds,
        rig_starter: true,
        notes: "for the Friday stream".into(),
        tags: "vtuber, wip".into(),
        ..Default::default()
    };
    h.state_mut().test_create(np);
    h.run_steps(4);
    {
        let d = h.state().draw_mut_ref();
        let names: Vec<&str> = d.doc.layers.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"Eyes") && names.contains(&"Background gradient") && names.last() == Some(&"Paper"), "{names:?}");
        assert_eq!((d.doc.w, d.doc.h, d.doc.meta.dpi, d.doc.meta.guides.len()), (2480, 3508, 300, 4));
        assert_eq!(d.doc.meta.tags, ["vtuber", "wip"]);
        assert!(d.active_name() == "Fringe", "starts on the first pixel layer: {}", d.active_name());
    }
    assert_eq!(h.state().file_name(), "Bust sheet");
    assert!(!h.state().test_dirty(), "a new project starts clean");
    save(&mut h, "np_02_created");

    // Project info: change DPI, notes and guides as one undoable step; undo brings them back.
    h.state_mut().test_open_info();
    h.run_steps(3);
    save(&mut h, "np_03_info");
    let mut meta = h.state().draw_mut_ref().doc.meta.clone();
    meta.dpi = 600;
    meta.notes = "changed".into();
    meta.guides = GuidePreset::Center.build(2480, 3508);
    h.state_mut().test_set_info(meta);
    h.run_steps(3);
    assert!(h.state().test_dirty());
    assert_eq!(h.state().history().entries.last().unwrap().label, "Project info");
    assert_eq!(h.state().draw_mut_ref().doc.meta.dpi, 600);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.meta.dpi, 300, "undo restores the print size");
    assert_eq!(h.state().draw_mut_ref().doc.meta.guides.len(), 4);

    // Saved projects show size, DPI and tags in the manager, and can be found by tag.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-projects-np");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, tags) in [("Bust sheet", vec!["vtuber", "wip"]), ("Emote set", vec!["emotes"]), ("Poster", vec![])] {
        let meta = ProjectMeta { dpi: 150, tags: tags.iter().map(|t| t.to_string()).collect(), notes: format!("{name} notes"), ..Default::default() };
        let doc = crate::files::template::build(&NewProject { w: 400, h: 600, rig_starter: false, ..Default::default() });
        crate::files::project::save(doc.w, doc.h, doc.paper, &doc.layers, &[], &meta, &dir.join(format!("{name}.seviath"))).unwrap();
    }
    h.state_mut().settings_mut().projects_dir = dir.display().to_string();
    h.state_mut().files_mut().manager_open = true;
    h.run_steps(8);
    save(&mut h, "np_04_manager_all");
    h.state_mut().test_manager_query("emotes");
    h.run_steps(8);
    save(&mut h, "np_05_manager_search");
    let info = crate::files::project::read_info(&dir.join("Emote set.seviath")).unwrap();
    assert_eq!((info.w, info.h, info.meta.dpi, info.meta.tags), (400, 600, 150, vec!["emotes".to_string()]));
}

#[test]
fn gradient_screens() {
    use crate::gradient::{Repeat, Shape};
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // the Gradient tool: drag out a Sunset gradient top to bottom
    h.key_press_modifiers(Modifiers::SHIFT, Key::G);
    h.run_steps(2);
    {
        let g = h.state_mut().draw_mut().grad_mut();
        g.g.stops = crate::gradient::presets([0, 0, 0], [255, 255, 255]).into_iter().find(|p| p.0 == "Sunset").unwrap().1;
        g.g.dither = false;
    }
    h.run_steps(2);
    save(&mut h, "grad_00_options");
    stroke(&mut h, &[pos2(740.0, 120.0), pos2(740.0, 400.0), pos2(740.0, 800.0)]);
    h.run_steps(2);
    assert!(h.state().draw_mut_ref().gradient_editing(), "the gradient stays live after the drag");
    assert_eq!(alpha_at(&h, 0, pos2(740.0, 300.0)), 0, "nothing is painted until Apply");
    save(&mut h, "grad_01_live");
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(!h.state().draw_mut_ref().gradient_editing());
    let top = h.state().draw_mut_ref().pixel_rgba(0, pos2(740.0, 130.0));
    let bottom = h.state().draw_mut_ref().pixel_rgba(0, pos2(740.0, 790.0));
    assert!(top[3] == 255 && bottom[3] == 255 && top != bottom, "{top:?} {bottom:?}");
    assert!(top[2] > top[0] && bottom[0] > bottom[2], "dark violet at the top, gold at the bottom: {top:?} {bottom:?}");
    assert!(h.state().history().entries.last().unwrap().label.starts_with("Gradient"));
    save(&mut h, "grad_02_applied");

    // Radial inside a selection, repeated, multiplied over the first gradient
    h.key_press(Key::M);
    stroke(&mut h, &[pos2(560.0, 300.0), pos2(700.0, 400.0), pos2(820.0, 520.0)]);
    h.key_press_modifiers(Modifiers::SHIFT, Key::G);
    h.run_steps(2);
    {
        let g = h.state_mut().draw_mut().grad_mut();
        g.g.shape = Shape::Radial;
        g.g.repeat = Repeat::Repeat;
        g.g.stops = crate::gradient::presets([0, 0, 0], [255, 255, 255]).into_iter().find(|p| p.0 == "Rainbow").unwrap().1;
        g.g.blend = crate::doc::Blend::Normal;
    }
    stroke(&mut h, &[pos2(690.0, 410.0), pos2(720.0, 440.0), pos2(760.0, 470.0)]);
    h.key_press(Key::Enter);
    h.run_steps(3);
    let inside = h.state().draw_mut_ref().pixel_rgba(0, pos2(700.0, 420.0));
    let outside = h.state().draw_mut_ref().pixel_rgba(0, pos2(900.0, 700.0));
    let before_outside = bottom;
    assert_ne!(inside, top, "the selection got the rainbow");
    assert!(outside[0] != 0 && outside != [0; 4], "outside the selection the first gradient is still there: {outside:?} (was like {before_outside:?})");
    save(&mut h, "grad_03_radial_in_selection");

    // Gradient map turns the layer's colors into the gradient by lightness
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run_steps(2);
    {
        let g = h.state_mut().draw_mut().grad_mut();
        g.g.shape = Shape::Linear;
        g.g.repeat = Repeat::None;
        g.g.stops = vec![
            crate::gradient::Stop { pos: 0.0, color: [10, 30, 90], alpha: 1.0 },
            crate::gradient::Stop { pos: 1.0, color: [255, 240, 200], alpha: 1.0 },
        ];
    }
    h.state_mut().draw_mut().gradient_map();
    assert_eq!(h.state().draw_mut_ref().pending_label().as_deref(), Some("Gradient map · Layer 1"));
    h.run_steps(3);
    let mapped = h.state().draw_mut_ref().pixel_rgba(0, pos2(740.0, 790.0));
    assert!(mapped[2] > 60 && mapped[0] < 255, "{mapped:?}");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().pixel_rgba(0, pos2(740.0, 790.0)), bottom_after_rainbow_or(bottom, &h), "undo brings the colors back");
    save(&mut h, "grad_04_map_undone");

    // Painting with the gradient: the stroke changes color along its length
    h.key_press(Key::B);
    h.state_mut().draw_mut().brush_mut().size = 40.0;
    {
        let g = h.state_mut().draw_mut().grad_mut();
        g.brush_on = true;
        g.brush_len = 500.0;
        g.g.stops = vec![
            crate::gradient::Stop { pos: 0.0, color: [255, 0, 0], alpha: 1.0 },
            crate::gradient::Stop { pos: 1.0, color: [0, 0, 255], alpha: 1.0 },
        ];
    }
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    h.run_steps(2);
    let pts: Vec<Pos2> = (0..=40).map(|i| pos2(500.0 + i as f32 * 12.0, 840.0 - i as f32 * 0.0)).collect();
    stroke(&mut h, &pts);
    h.run_steps(3);
    let start = h.state().draw_mut_ref().max_paint_color(0, pos2(512.0, 840.0));
    let end = h.state().draw_mut_ref().max_paint_color(0, pos2(970.0, 840.0));
    assert!(start[0] > 200 && start[2] < 80, "the stroke starts red: {start:?}");
    assert!(end[2] > 150 && end[0] < 120, "and ends blue: {end:?}");
    save(&mut h, "grad_05_brush");
}

fn bottom_after_rainbow_or(_bottom: [u8; 4], h: &Harness<'_, SeviathApp>) -> [u8; 4] {
    // the pixel did not change under the rainbow (it is outside the selection), so it is the sunset's bottom
    h.state().draw_mut_ref().pixel_rgba(0, pos2(740.0, 790.0))
}

#[test]
fn randomizer_screens() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().test_rand_open(1, 3); // Skin, seed 3
    h.run_steps(4);
    let palette = h.state().draw_mut_ref().rand_swatches();
    assert_eq!(palette.len(), 5);
    let lights: Vec<f32> = palette.iter().map(|c| c[0] as f32 + c[1] as f32 + c[2] as f32).collect();
    assert!(lights[4] > lights[0], "dark to light: {palette:?}");
    save(&mut h, "rand_01_window");

    // speckle inside a selection
    h.key_press(Key::M);
    stroke(&mut h, &[pos2(540.0, 200.0), pos2(700.0, 300.0), pos2(860.0, 420.0)]);
    h.state_mut().draw_mut().test_rand_apply(2);
    assert!(h.state().draw_mut_ref().pending_label().unwrap_or_default().starts_with("Random color speckle"));
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().pixel_rgba(0, pos2(700.0, 300.0))[3] == 255);
    assert_eq!(h.state().draw_mut_ref().pixel_rgba(0, pos2(900.0, 700.0))[3], 0);
    save(&mut h, "rand_02_speckle");

    // recolor flats keeps the lineart
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run_steps(2);
    {
        let d = h.state_mut().draw_mut();
        let w = d.doc.w;
        let h_px = d.doc.h;
        let img = image::RgbaImage::from_fn(w, h_px, |x, y| {
            let (cx, cy) = (x as f32 - 1500.0, y as f32 - 2000.0);
            let r = (cx * cx + cy * cy).sqrt();
            if (880.0..900.0).contains(&r) {
                image::Rgba([12, 12, 12, 255])
            } else if r < 880.0 {
                image::Rgba([230, 160, 140, 255])
            } else if y > 3000 {
                image::Rgba([150, 190, 230, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        d.doc.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, h_px);
        d.doc.layers[0].rev += 1;
        d.doc.mark_all_dirty();
    }
    h.run_steps(2);
    let line_before = h.state().draw_mut_ref().pixel_rgba(0, pos2(740.0, 2000.0));
    let _ = line_before;
    h.state_mut().draw_mut().test_rand_apply(3);
    h.run_steps(3);
    {
        let d = h.state().draw_mut_ref();
        let inside = crate::files::layer_rgba(&d.doc.layers[0], 1500, 2000, 1, 1).get_pixel(0, 0).0;
        let line = crate::files::layer_rgba(&d.doc.layers[0], 1500 + 890, 2000, 1, 1).get_pixel(0, 0).0;
        assert_eq!(line, [12, 12, 12, 255], "lineart untouched");
        assert_ne!(inside, [230, 160, 140, 255], "the flat got a new color");
        assert_eq!(inside[3], 255);
    }
    save(&mut h, "rand_03_recolor");
}

#[test]
fn vector_screens() {
    use crate::doc::LayerKind;
    use crate::vector::{EraseMode, LineEnds};
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let labels = |h: &Harness<'_, SeviathApp>| h.state().history().entries.iter().map(|e| e.label.clone()).collect::<Vec<_>>();

    // The vector pen on a pixel layer starts a vector layer
    h.key_press(Key::P);
    h.run_steps(2);
    h.state_mut().draw_mut().vec_mut().style.size = 14.0;
    h.state_mut().draw_mut().vec_mut().style.size_pressure = false;
    stroke(&mut h, &wave(pos2(520.0, 220.0), pos2(940.0, 240.0), 30.0, 40));
    h.run_steps(3);
    {
        let d = h.state().draw_mut_ref();
        assert_eq!(d.doc.layers[0].kind, LayerKind::Vector, "a vector layer was made");
        assert_eq!(d.vector_strokes(0).len(), 1);
        assert!(d.vector_strokes(0)[0].pts.len() >= 2);
    }
    assert!(labels(&h).last().unwrap().starts_with("Vector line"), "{:?}", labels(&h));
    assert!(!h.state().draw_mut_ref().doc.layers[0].tiles.is_empty(), "pixels were drawn from the line");

    // A pointed, thick, colored line
    {
        let v = h.state_mut().draw_mut().vec_mut();
        v.style.size = 30.0;
        v.style.ends = LineEnds::Taper;
    }
    h.state_mut().draw_mut().primary = Color32::from_rgb(46, 150, 230);
    stroke(&mut h, &wave(pos2(520.0, 380.0), pos2(940.0, 420.0), 40.0, 50));
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().vector_strokes(0).len(), 2, "same layer, second line");
    save(&mut h, "vec_01_lines");

    // Hold still at the end of a rough circle: it becomes a clean one
    let center = pos2(730.0, 620.0);
    let circle: Vec<Pos2> = (0..=44).map(|i| {
        let a = i as f32 / 44.0 * 6.2;
        center + vec2(a.cos() * 110.0 + ((i * 7) % 5) as f32 - 2.0, a.sin() * 110.0 + ((i * 3) % 5) as f32 - 2.0)
    }).collect();
    h.state_mut().draw_mut().vec_mut().style.size = 10.0;
    h.state_mut().draw_mut().vec_mut().style.ends = LineEnds::Round;
    h.hover_at(circle[0]);
    h.step();
    button(&h, circle[0], true);
    h.step();
    for p in &circle[1..] {
        h.hover_at(*p);
        h.step();
    }
    for _ in 0..6 {
        h.step(); // the pen rests
    }
    button(&h, *circle.last().unwrap(), false);
    h.run_steps(3);
    let n = h.state().draw_mut_ref().vector_strokes(0).len();
    assert_eq!(n, 3);
    {
        let s = h.state().draw_mut_ref().vector_strokes(0).pop().unwrap();
        let d = h.state().draw_mut_ref();
        let c = d.screen_to_doc(center);
        let radii: Vec<f32> = s.pts.iter().map(|p| (p[0] - c.x).hypot(p[1] - c.y)).collect();
        let mean = radii.iter().sum::<f32>() / radii.len() as f32;
        let worst = radii.iter().map(|r| (r - mean).abs()).fold(0.0f32, f32::max);
        assert!(worst < mean * 0.012, "a clean circle: worst deviation {worst} px (radius {mean})");
        assert!(s.pts.len() > 40);
    }
    save(&mut h, "vec_02_circle");

    // The eraser cuts the first line into two pieces
    h.key_press(Key::E);
    h.run_steps(2);
    h.state_mut().draw_mut().brush_mut().size = 40.0;
    h.state_mut().draw_mut().vec_mut().erase = EraseMode::Cut;
    let before = h.state().draw_mut_ref().vector_strokes(0).len();
    // a point of the first line, right where the eraser will cross it
    let probe = {
        let d = h.state().draw_mut_ref();
        let dense = crate::vector::densify(&d.vector_strokes(0)[0].pts, 2.0);
        dense.iter().map(|p| d.test_doc_to_screen(pos2(p[0], p[1]))).min_by(|a, b| (a.x - 730.0).abs().total_cmp(&(b.x - 730.0).abs())).unwrap()
    };
    assert!(alpha_at(&h, 0, probe) > 0, "the probe is on the line");
    stroke(&mut h, &[pos2(probe.x, probe.y - 80.0), pos2(probe.x, probe.y), pos2(probe.x, probe.y + 80.0)]);
    h.run_steps(3);
    let after = h.state().draw_mut_ref().vector_strokes(0).len();
    assert_eq!(after, before + 1, "the touched line is now two lines");
    assert_eq!(alpha_at(&h, 0, probe), 0, "the gap is empty");
    assert!(labels(&h).last().unwrap().starts_with("Vector erase"), "{:?}", labels(&h));
    // Cmd+Z brings the line back
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().vector_strokes(0).len(), before, "undo restores the lines");
    assert!(alpha_at(&h, 0, probe) > 0, "the line is whole again");

    // Edit lines: drag a node
    h.key_press(Key::P);
    h.run_steps(2);
    h.state_mut().draw_mut().vec_mut().mode = crate::tabs::draw::vector_ui::VecMode::Edit;
    h.state_mut().draw_mut().test_vec_select(1); // the blue line
    h.run_steps(2);
    save(&mut h, "vec_03_edit");
    let s = h.state().draw_mut_ref().vector_strokes(0)[1].clone();
    let last = *s.pts.last().unwrap();
    let from = h.state().draw_mut_ref().test_doc_to_screen(pos2(last[0], last[1]));
    h.hover_at(from);
    h.step();
    button(&h, from, true);
    h.step();
    for k in 1..=4 {
        let p = from + vec2(0.0, -15.0 * k as f32);
        h.hover_at(p);
        h.step();
    }
    button(&h, from + vec2(0.0, -60.0), false);
    h.run_steps(3);
    let moved = h.state().draw_mut_ref().vector_strokes(0)[1].clone();
    let end = *moved.pts.last().unwrap();
    assert!((end[1] - last[1]).abs() > 100.0, "the end node moved up: {last:?} -> {end:?}");
    assert_eq!(moved.pts.len(), s.pts.len(), "no node was added or lost");
    assert!(labels(&h).last().unwrap().starts_with("Move node"), "{:?}", labels(&h));

    // Delete removes the selected node, then (no node) the line
    let n_before = h.state().draw_mut_ref().vector_strokes(0)[1].pts.len();
    h.key_press(Key::Delete);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().vector_strokes(0)[1].pts.len(), n_before - 1, "the selected node is gone");
    h.state_mut().draw_mut().test_vec_select(1);
    h.key_press(Key::Delete);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().vector_strokes(0).len(), before - 1, "then the whole line");
    save(&mut h, "vec_04_after_delete");
}

/// Step until `ok` holds (a background job finished).
fn wait_until(h: &mut Harness<'_, SeviathApp>, what: &str, ok: impl Fn(&Harness<'_, SeviathApp>) -> bool) {
    for _ in 0..400 {
        h.step();
        if ok(h) {
            h.step();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    panic!("timed out waiting for: {what}");
}

#[test]
fn trace_screens() {
    use crate::doc::LayerKind;
    use crate::trace::TraceMode;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // a pixel drawing: a thick ring, a bar and a red blob
    let id = {
        let d = h.state_mut().draw_mut();
        let (w, hh) = (d.doc.w, d.doc.h);
        let img = image::RgbaImage::from_fn(w, hh, |x, y| {
            let (cx, cy) = (x as f32 - 1200.0, y as f32 - 1300.0);
            let r = cx.hypot(cy);
            let bar = (1800..2800).contains(&x) && (1280..1330).contains(&y);
            let blob = (x as f32 - 2000.0).hypot(y as f32 - 2200.0) < 260.0;
            if (500.0..540.0).contains(&r) || bar {
                image::Rgba([20, 20, 30, 255])
            } else if blob {
                image::Rgba([220, 50, 50, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        d.doc.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, hh);
        d.doc.layers[0].rev += 1;
        d.doc.mark_all_dirty();
        d.doc.layers[0].id
    };
    h.run_steps(2);
    h.state_mut().draw_mut().test_open_trace(id);
    wait_until(&mut h, "the preview", |h| h.state().draw_mut_ref().trace_ready());
    save(&mut h, "trace_01_lines_preview");
    h.state_mut().draw_mut().trace_mut().set_mode(TraceMode::Both);
    wait_until(&mut h, "the preview of both", |h| h.state().draw_mut_ref().trace_ready());
    for _ in 0..30 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    save(&mut h, "trace_02_both_preview");

    // trace for real: a new vector layer above, the original hidden
    h.state_mut().draw_mut().trace_mut().set_mode(TraceMode::Both);
    h.state_mut().draw_mut().test_trace_now();
    wait_until(&mut h, "the trace", |h| !h.state().draw_mut_ref().trace_mut_ref().open);
    {
        let d = h.state().draw_mut_ref();
        assert_eq!(d.doc.layers[0].kind, LayerKind::Vector, "the traced layer is on top");
        assert!(d.doc.layers[0].name.ends_with("(traced)"));
        let v = d.doc.layers[0].vector.as_ref().unwrap();
        assert!(v.strokes.len() >= 2, "the ring and the bar: {}", v.strokes.len());
        assert!(!v.fills.is_empty(), "the red blob");
        assert!(!d.doc.layers[1].visible, "the original is hidden");
        // the traced drawing lies where the original was
        let on_ring = crate::files::layer_rgba(&d.doc.layers[0], 1200 + 520, 1300, 1, 1).get_pixel(0, 0).0;
        assert!(on_ring[3] > 128, "the ring is traced: {on_ring:?}");
        let in_blob = crate::files::layer_rgba(&d.doc.layers[0], 2000, 2200, 1, 1).get_pixel(0, 0).0;
        assert!(in_blob[0] > 150 && in_blob[1] < 120 && in_blob[3] > 200, "the blob is a red shape: {in_blob:?}");
        // the line keeps its thickness
        let width = v.strokes.iter().map(|s| s.style.size).fold(0.0f32, f32::max);
        assert!((width - 40.0).abs() < 8.0 || (width - 50.0).abs() < 8.0, "thickness kept: {width}");
    }
    assert!(h.state().history().entries.last().unwrap().label.starts_with("Auto-trace"), "{:?}", h.state().history().entries.last().map(|e| e.label.clone()));
    h.run_steps(3);
    save(&mut h, "trace_03_result");

    // one undo brings the original back and removes the traced layer
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    let d = h.state().draw_mut_ref();
    assert_eq!(d.doc.layers[0].kind, LayerKind::Raster);
    assert!(d.doc.layers[0].visible);
}

#[test]
fn filter_screens() {
    use crate::check::CheckView;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // a drawing: a red disc and a black bar on transparent
    let (w, hh) = (3000u32, 4000u32);
    {
        let d = h.state_mut().draw_mut();
        let img = image::RgbaImage::from_fn(w, hh, |x, y| {
            let disc = (x as f32 - 1300.0).hypot(y as f32 - 1500.0) < 500.0;
            let bar = (1900..2600).contains(&x) && (1400..1500).contains(&y);
            if disc { image::Rgba([220, 40, 40, 255]) } else if bar { image::Rgba([10, 10, 10, 255]) } else { image::Rgba([0, 0, 0, 0]) }
        });
        d.doc.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, hh);
        d.doc.layers[0].rev += 1;
        d.doc.mark_all_dirty();
    }
    h.run_steps(3);
    let at = |h: &Harness<'_, SeviathApp>, x: i32, y: i32| crate::files::layer_rgba(&h.state().draw_mut_ref().doc.layers[0], x, y, 1, 1).get_pixel(0, 0).0;
    assert_eq!(at(&h, 1300, 1000)[3], 0, "outside the disc (just above it)");

    // Gaussian blur: the paint spreads past the old edge
    h.state_mut().draw_mut().open_filter("blur");
    h.run_steps(2);
    h.state_mut().draw_mut().filter_ui_mut().set_param("radius", 40.0);
    wait_until(&mut h, "the blur preview", |h| h.state().draw_mut_ref().filter_ready());
    for _ in 0..12 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    save(&mut h, "flt_01_blur_preview");
    h.state_mut().draw_mut().test_apply_filter();
    wait_until(&mut h, "the blur", |h| !h.state().draw_mut_ref().filter_open_now());
    let edge = at(&h, 1300, 1000 - 5);
    assert!(edge[3] > 30 && edge[3] < 255, "the blur reaches past the old edge of the disc: {edge:?}");
    assert!(at(&h, 1300, 1500)[3] > 250, "the middle is still solid");
    assert!(h.state().history().entries.last().unwrap().label.starts_with("Gaussian blur"), "{:?}", h.state().history().entries.last().map(|e| e.label.clone()));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(at(&h, 1300, 995)[3], 0, "undo takes the blur away");

    // a color adjustment: hue 120 degrees turns red into green
    h.state_mut().draw_mut().open_filter("hsl");
    h.run_steps(2);
    h.state_mut().draw_mut().filter_ui_mut().set_param("hue", 120.0);
    wait_until(&mut h, "the hue preview", |h| h.state().draw_mut_ref().filter_ready());
    h.state_mut().draw_mut().test_apply_filter();
    wait_until(&mut h, "the hue shift", |h| !h.state().draw_mut_ref().filter_open_now());
    let c = at(&h, 1300, 1500);
    assert!(c[1] > 180 && c[0] < 80, "the disc is green now: {c:?}");
    assert_eq!(at(&h, 2200, 1450)[..3], [10, 10, 10], "the black bar stays black");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);

    // the filter only touches the selection
    h.key_press(Key::M);
    stroke(&mut h, &[pos2(540.0, 300.0), pos2(640.0, 400.0), pos2(740.0, 480.0)]);
    h.state_mut().draw_mut().open_filter("invert");
    h.run_steps(2);
    wait_until(&mut h, "the invert preview", |h| h.state().draw_mut_ref().filter_ready());
    h.state_mut().draw_mut().test_apply_filter();
    wait_until(&mut h, "invert", |h| !h.state().draw_mut_ref().filter_open_now());
    let sel_b = h.state().draw_mut_ref().doc.selection.as_ref().and_then(|s| s.bounds()).unwrap();
    assert!(sel_b.0 < 1300 && sel_b.2 > 1300 && sel_b.2 < 2540, "the selection holds the disc but not the end of the bar: {sel_b:?}");
    let inside = at(&h, 1300, 1500);
    assert!(inside[0] < 60 && inside[1] > 180 && inside[2] > 180, "inside the selection the red disc is inverted: {inside:?}");
    assert_eq!(at(&h, 2570, 1450), [10, 10, 10, 255], "outside the selection nothing changed");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run_steps(3);

    // curves editor, and the check views
    h.state_mut().draw_mut().open_filter("curves");
    h.run_steps(6);
    save(&mut h, "flt_02_curves");
    h.key_press(Key::Escape);
    h.state_mut().draw_mut().filter_ui_mut().open = false;
    h.run_steps(2);
    h.state_mut().draw_mut().check = CheckView::Gray;
    h.run_steps(4);
    assert_eq!(h.state().draw_mut_ref().check_view(), CheckView::Gray);
    save(&mut h, "flt_03_check_gray");
    h.state_mut().draw_mut().check = CheckView::Deuteranopia;
    h.run_steps(4);
    save(&mut h, "flt_04_check_colorblind");
    h.state_mut().draw_mut().check = CheckView::Normal;
    h.run_steps(2);
    // locked layers and pixel-less layers say why they can't be filtered
    h.state_mut().draw_mut().doc.layers[0].locked = true;
    h.state_mut().draw_mut().open_filter("blur");
    assert!(!h.state().draw_mut_ref().filter_open_now());
}

#[test]
fn shape_filter_screens() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let (w, hh) = (3000u32, 4000u32);
    {
        let d = h.state_mut().draw_mut();
        let img = image::RgbaImage::from_fn(w, hh, |x, y| {
            let disc = (x as f32 - 1500.0).hypot(y as f32 - 1800.0) < 900.0;
            if disc { image::Rgba([(x * 255 / w) as u8, 80, (y * 255 / hh) as u8, 255]) } else { image::Rgba([240, 220, 180, 255]) }
        });
        d.doc.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, hh);
        d.doc.layers[0].rev += 1;
        d.doc.mark_all_dirty();
    }
    h.run_steps(3);
    for (i, id) in ["cubes", "spheres", "triangles", "blocks", "illustrate"].iter().enumerate() {
        h.state_mut().draw_mut().open_filter(id);
        h.run_steps(2);
        wait_until(&mut h, "the shape preview", |h| h.state().draw_mut_ref().filter_ready());
        for _ in 0..8 {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        save(&mut h, &format!("flt_shape_{}_{id}", i + 1));
        h.state_mut().draw_mut().filter_ui_mut().open = false;
        h.run_steps(2);
    }
}

#[test]
fn effects_screens() {
    use crate::doc::LayerKind;
    use crate::effects::{Effect, FxKind, Place};
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let (w, hh) = (3000u32, 4000u32);
    {
        let d = h.state_mut().draw_mut();
        let img = image::RgbaImage::from_fn(w, hh, |x, y| {
            if (x as f32 - 1500.0).hypot(y as f32 - 1800.0) < 700.0 { image::Rgba([220, 60, 60, 255]) } else { image::Rgba([0, 0, 0, 0]) }
        });
        d.doc.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, hh);
        d.doc.layers[0].rev += 1;
        d.doc.mark_all_dirty();
    }
    h.run_steps(3);
    let flat = |h: &Harness<'_, SeviathApp>, x: u32, y: u32| *crate::files::flatten(&h.state().draw_mut_ref().doc).get_pixel(x, y);
    assert_eq!(flat(&h, 2300, 1800).0[..3], [255, 255, 255], "plain paper right of the disc");

    // a shadow and an outline appear around the disc, and the paint itself is untouched
    {
        let d = h.state_mut().draw_mut();
        d.doc.layers[0].fx = vec![
            Effect { on: true, kind: FxKind::Shadow { color: [0, 0, 0], opacity: 1.0, dx: 90.0, dy: 0.0, blur: 0.0, spread: 0.0 } },
            Effect { on: true, kind: FxKind::Outline { color: [20, 20, 160], opacity: 1.0, width: 30.0, place: Place::Outside } },
        ];
        d.doc.mark_all_dirty();
    }
    h.run_steps(6);
    let tiles_before = h.state().draw_mut_ref().doc.layers[0].tiles.len();
    assert_eq!(flat(&h, 2215, 1800).0[2], 160, "the blue outline just outside the disc (700 + 30)");
    assert!(flat(&h, 2260, 1800).0[0] < 30, "the shadow reaches to the right: {:?}", flat(&h, 2260, 1800));
    assert_eq!(flat(&h, 1500, 1800).0[..3], [220, 60, 60], "the paint is still the same red");
    save(&mut h, "fx_01_effects_on_canvas");
    assert_eq!(h.state().draw_mut_ref().doc.layers[0].tiles.len(), tiles_before);

    // an adjustment layer: invert turns the paper black and the red cyan, and it undoes in one step
    h.state_mut().draw_mut().apply_external("New adjustment layer", |d| d.add_adjustment("invert"));
    h.run_steps(6);
    assert_eq!(h.state().draw_mut_ref().doc.layers[0].kind, LayerKind::Adjust);
    assert_eq!(flat(&h, 100, 100).0[..3], [0, 0, 0], "the paper is inverted");
    let c = flat(&h, 1500, 1800).0;
    assert!(c[0] < 40 && c[1] > 190, "red became cyan: {c:?}");
    assert_eq!(h.state().history().entries.last().unwrap().label, "New adjustment layer");
    save(&mut h, "fx_02_adjustment_layer");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(4);
    assert_eq!(flat(&h, 100, 100).0[..3], [255, 255, 255], "undo takes the adjustment away");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(4);
    assert_eq!(flat(&h, 100, 100).0[..3], [0, 0, 0], "redo brings it back");
}

#[test]
fn effects_panel_screens() {
    use crate::effects::Effect;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1500.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().doc.layers[0].fx = vec![Effect::presets().remove(0), Effect::presets().remove(2)];
    h.run_steps(4);
    save(&mut h, "fx_03_effects_panel");
    h.state_mut().draw_mut().add_adjustment("levels");
    h.run_steps(4);
    save(&mut h, "fx_04_adjustment_panel");
}

#[test]
fn reference_window_resizes_from_every_side() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let pic = image::RgbaImage::from_fn(320, 240, |x, y| image::Rgba([(x * 255 / 320) as u8, (y * 255 / 240) as u8, 120, 255]));
        let mut png = Vec::new();
        pic.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let d = h.state_mut().draw_mut();
        d.refs.add_bytes("pic", &png).unwrap();
        d.show_references();
        d.set_popout(true);
    }
    h.run_steps(6);
    let r0 = h.state().draw_mut_ref().ref_window_rect();
    assert!(r0.width() > 300.0 && r0.width() < 450.0, "small by default: {r0:?}");
    save(&mut h, "ref_popout_small");
    // drag the left edge to the left, the top edge up, the bottom edge down and the right edge right
    let c = r0.center();
    stroke(&mut h, &[pos2(r0.left() + 1.0, c.y), pos2(r0.left() - 60.0, c.y), pos2(r0.left() - 120.0, c.y)]);
    h.run_steps(3);
    let r1 = h.state().draw_mut_ref().ref_window_rect();
    assert!(r1.width() > r0.width() + 80.0, "left edge: {r0:?} -> {r1:?}");
    let c = r1.center();
    stroke(&mut h, &[pos2(c.x, r1.top() + 1.0), pos2(c.x, r1.top() - 40.0), pos2(c.x, r1.top() - 80.0)]);
    h.run_steps(3);
    let r2 = h.state().draw_mut_ref().ref_window_rect();
    assert!(r2.height() > r1.height() + 50.0, "top edge: {r1:?} -> {r2:?}");
    save(&mut h, "ref_popout_bigger");
}

#[test]
fn color_profile_screens() {
    use crate::colorprofiles::Proof;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let d = h.state_mut().draw_mut();
        let (w, hh) = (d.doc.w, d.doc.h);
        let img = image::RgbaImage::from_fn(w, hh, |x, _| if x < w / 2 { image::Rgba([0, 255, 60, 255]) } else { image::Rgba([225, 170, 150, 255]) });
        d.doc.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, hh);
        d.doc.layers[0].rev += 1;
        d.doc.mark_all_dirty();
    }
    h.run_steps(4);
    let shown = |h: &mut Harness<'_, SeviathApp>, x: u32| h.render().expect("render").get_pixel(x, 450).0;
    let (left, right) = (620, 880);
    let plain = [shown(&mut h, left), shown(&mut h, right)];
    assert!(plain[0][1] > 240 && plain[0][0] < 15, "neon green as painted: {plain:?}");
    save(&mut h, "cp_01_plain");
    h.state_mut().draw_mut().proof = Proof::Print;
    h.run_steps(4);
    let printed = [shown(&mut h, left), shown(&mut h, right)];
    assert!(printed[0][1] < plain[0][1] - 10 || printed[0][0] > 20, "neon green is dulled in the print preview: {printed:?}");
    assert!((printed[1][0] as i32 - plain[1][0] as i32).abs() < 30, "a skin tone prints about as it looks: {printed:?}");
    h.state_mut().draw_mut().proof = Proof::Warn;
    h.state_mut().draw_mut().filter_ui_mut().check_open = true;
    h.run_steps(4);
    let warn = shown(&mut h, left);
    assert!((warn[0] as i32 - 128).abs() < 6 && (warn[1] as i32 - 128).abs() < 6, "unprintable colors turn gray: {warn:?}");
    save(&mut h, "cp_02_print_proof_warning");
    h.state_mut().draw_mut().proof = Proof::Off;
    h.state_mut().draw_mut().filter_ui_mut().check_open = false;
    h.state_mut().draw_mut().doc.meta.rgb_profile = "p3".into();
    h.run_steps(4);
    let p3 = shown(&mut h, left);
    assert_ne!(p3, plain[0], "the same numbers read as P3 look different on an sRGB screen");
    save(&mut h, "cp_03_display_p3");
}

#[test]
fn light_and_depth_screens() {
    use crate::doc::LayerKind;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1500.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let d = h.state_mut().draw_mut();
        // a flat colored picture, so the shading shows on top of something
        let (w, hh) = (d.doc.w, d.doc.h);
        let img = image::RgbaImage::from_pixel(w, hh, image::Rgba([210, 140, 120, 255]));
        d.doc.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, hh);
        d.doc.layers[0].rev += 1;
        d.add_depth_layer();
        let i = d.doc.index_of(d.active_id()).unwrap();
        assert!(d.doc.layers[i].depth);
        let dome = image::RgbaImage::from_fn(w, hh, |x, y| {
            let r = (x as f32 - 1500.0).hypot(y as f32 - 2000.0) / 900.0;
            if r < 1.0 {
                let v = ((1.0 - r * r).sqrt() * 255.0) as u8;
                image::Rgba([v, v, v, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        d.doc.layers[i].tiles = crate::files::tiles_from_rgba(&dome, 0, 0, w, hh);
        d.doc.layers[i].rev += 1;
        d.add_light_layer();
        d.doc.mark_all_dirty();
    }
    h.run_steps(6);
    let li = h.state().draw_mut_ref().doc.layers.iter().position(|l| l.kind == LayerKind::Light).unwrap();
    assert_eq!(li, 0, "the light layer is on top");
    let shown = |h: &mut Harness<'_, SeviathApp>, x: u32| h.render().expect("render").get_pixel(x, 760).0;
    // the dome fills the middle of the page on screen; sample its left and right sides
    let (l1, r1) = (shown(&mut h, 575), shown(&mut h, 925));
    save(&mut h, "light_01_from_the_left");
    assert!(l1[0] as i32 - l1[2] as i32 > r1[0] as i32 - r1[2] as i32 + 20, "lit warm on the left, cool shade on the right: {l1:?} {r1:?}");
    // turn the sun around
    h.state_mut().draw_mut().doc.layers[li].light.as_mut().unwrap().dir = [0.6, 0.0];
    h.state_mut().draw_mut().doc.mark_all_dirty();
    h.run_steps(6);
    let (l2, r2) = (shown(&mut h, 575), shown(&mut h, 925));
    save(&mut h, "light_02_from_the_right");
    assert!(r2[0] as i32 - r2[2] as i32 > l2[0] as i32 - l2[2] as i32 + 20, "now the right side is lit: {l2:?} {r2:?}");
    // selecting the depth layer shows it as a gray picture
    let di = h.state().draw_mut_ref().doc.layers.iter().position(|l| l.depth).unwrap();
    let did = h.state().draw_mut_ref().doc.layers[di].id;
    h.state_mut().draw_mut().set_active(did);
    h.run_steps(6);
    save(&mut h, "light_03_depth_layer_selected");
    let lid = h.state().draw_mut_ref().doc.layers[li].id;
    h.state_mut().draw_mut().set_active(lid);
    h.run_steps(6);
    save(&mut h, "light_04_light_panel");
}

#[test]
fn mixing_chart_screens() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let cols = [
        Color32::from_rgb(250, 224, 25),
        Color32::from_rgb(25, 50, 217),
        Color32::from_rgb(230, 30, 30),
        Color32::from_rgb(255, 255, 255),
        Color32::from_rgb(30, 160, 80),
    ];
    h.state_mut().draw_mut().open_mix_chart(&cols);
    h.run_steps(6);
    save(&mut h, "mix_01_chart");
    let before = h.state().draw_mut_ref().doc.layers.len();
    h.state_mut().draw_mut().apply_external("Mixing chart", |d| d.import_picture_layer(&crate::tabs::draw::DrawState::mix_chart_image(&cols, 60), "Mixing chart"));
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.layers.len(), before + 1, "the chart becomes a layer");
}

#[test]
fn material_screens() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1500.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let d = h.state_mut().draw_mut();
        d.add_material_layer();
        let i = d.doc.index_of(d.active_id()).unwrap();
        let (w, hh) = (d.doc.w, d.doc.h);
        let disc = image::RgbaImage::from_fn(w, hh, |x, y| {
            if (x as f32 - 1500.0).hypot(y as f32 - 2000.0) < 900.0 { image::Rgba([200, 200, 200, 255]) } else { image::Rgba([0, 0, 0, 0]) }
        });
        d.doc.layers[i].tiles = crate::files::tiles_from_rgba(&disc, 0, 0, w, hh);
        d.doc.layers[i].rev += 1;
        d.doc.mark_all_dirty();
    }
    h.run_steps(8);
    save(&mut h, "mat_01_chrome_ball_and_panel");
    let shown = |h: &mut Harness<'_, SeviathApp>, x: u32, y: u32| h.render().expect("render").get_pixel(x, y).0;
    let top = shown(&mut h, 747, 600);
    let bottom = shown(&mut h, 747, 930);
    assert!(top[2] as i32 > bottom[2] as i32 + 20, "chrome: sky above, ground below: {top:?} {bottom:?}");
    assert_eq!(h.state().history().entries.len(), 0);
}


#[test]
fn my_brushes_save_import_and_stamp() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let dir = std::env::temp_dir().join(format!("seviath_mybrush_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    {
        let d = h.state_mut().draw_mut();
        // save the brush in hand under a new name, with its credit
        d.lib_meta_mut().name = format!("Test brush {}", std::process::id());
        d.lib_meta_mut().author = "Seviath".into();
        d.lib_meta_mut().license = "CC0".into();
        d.save_current_brush();
        assert!(d.brush_list_has(&format!("Test brush {}", std::process::id())), "it shows up in the brush list");
        // a picture becomes a stamp brush
        let pic = dir.join("Star dust.png");
        image::RgbaImage::from_fn(40, 40, |x, y| if (x as i32 - 20).abs() + (y as i32 - 20).abs() < 14 { image::Rgba([255, 200, 0, 255]) } else { image::Rgba([0, 0, 0, 0]) }).save(&pic).unwrap();
        d.import_brushes(&[pic]);
        assert!(d.brush_list_has("Star dust"));
        assert_eq!(d.brush().tip, crate::brush::Tip::Image, "and it is the brush in hand now");
        // a stamp from what is selected
        let (w, hh) = (d.doc.w, d.doc.h);
        let blob = image::RgbaImage::from_fn(w, hh, |x, y| if (x as f32 - 400.0).hypot(y as f32 - 400.0) < 120.0 { image::Rgba([30, 120, 60, 255]) } else { image::Rgba([0, 0, 0, 0]) });
        d.doc.layers[0].tiles = crate::files::tiles_from_rgba(&blob, 0, 0, w, hh);
        let pts: Vec<_> = [(250.0, 250.0), (560.0, 250.0), (560.0, 560.0), (250.0, 560.0)].iter().map(|p| pos2(p.0, p.1)).collect();
        d.doc.selection = crate::select::Selection::polygon(&pts, w, hh, false);
        d.brush_mut().tip = crate::brush::Tip::Round;
        d.tip_from_selection();
        assert_eq!(d.brush().tip, crate::brush::Tip::Image, "the selection is the stamp");
        // clean up what the test saved
        for n in [format!("Test brush {}", std::process::id()), "Star dust".to_string()] {
            crate::brush::userlib::delete_in(&crate::brush::userlib::dir(), &n);
        }
        d.reload_user_brushes();
        assert!(!d.brush_list_has("Star dust"));
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn my_brush_panel_screens() {
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.get_by_label("My brush").click();
    h.run_steps(6);
    save(&mut h, "mb_01_my_brush_panel");
}

#[test]
fn text_screens() {
    use crate::doc::LayerKind;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.key_press(Key::T);
    h.run_steps(2);
    let before = h.state().draw_mut_ref().doc.layers.len();
    // a click on the canvas starts a text
    stroke(&mut h, &[pos2(560.0, 520.0)]);
    h.run_steps(4);
    let (i, n) = {
        let d = h.state().draw_mut_ref();
        (d.doc.index_of(d.active_id()).unwrap(), d.doc.layers.len())
    };
    assert_eq!(n, before + 1, "a text layer was made");
    assert!(h.state().draw_mut_ref().doc.layers[i].text.is_some() && h.state().draw_mut_ref().doc.layers[i].kind == LayerKind::Vector);
    assert!(h.state().history().entries.last().unwrap().label.contains("text"), "{:?}", h.state().history().entries.last().map(|e| e.label.clone()));
    // the words, in Latvian too, bent along an arch
    {
        let d = h.state_mut().draw_mut();
        let t = d.doc.layers[i].text.as_mut().unwrap();
        t.text = "Sveiki, Latvija!\nāčēģīķļņšūž".into();
        t.size = 150.0;
        t.color = [200, 60, 90];
        t.bend = 0.35;
        d.rebuild_text(i);
    }
    h.run_steps(4);
    let painted = |h: &Harness<'_, SeviathApp>| h.state().draw_mut_ref().doc.layers[i].tiles.values().flat_map(|t| t.iter()).filter(|c| c.a() > 200).count();
    assert!(painted(&h) > 5000, "the letters are painted: {}", painted(&h));
    let sample = h.state().draw_mut_ref().doc.layers[i].tiles.values().flat_map(|t| t.iter()).find(|c| c.a() == 255).copied().unwrap();
    assert!(sample.r() > 150 && sample.g() < 100, "in the chosen color: {sample:?}");
    save(&mut h, "text_01_bent_latvian");
    // dragging the text with the Text tool moves it
    let x0 = h.state().draw_mut_ref().doc.layers[i].text.as_ref().unwrap().x;
    stroke(&mut h, &[pos2(600.0, 570.0), pos2(640.0, 590.0), pos2(680.0, 610.0)]);
    h.run_steps(3);
    let x1 = h.state().draw_mut_ref().doc.layers[i].text.as_ref().unwrap().x;
    assert!(x1 > x0 + 50.0, "dragged to the right: {x0} -> {x1}");
    // a font kept in the project survives saving and opening
    {
        let d = h.state_mut().draw_mut();
        let bytes = crate::fonts::load(crate::fonts::MONO, &[]).unwrap();
        d.doc.meta.fonts.push(crate::fonts::ProjectFont { name: "My mono".into(), data: crate::brush::tipimage::base64_encode(&bytes) });
        d.doc.layers[i].text.as_mut().unwrap().font = "My mono".into();
        d.rebuild_text(i);
        let path = std::env::temp_dir().join(format!("seviath_text_{}.seviath", std::process::id()));
        crate::files::project::save(d.doc.w, d.doc.h, d.doc.paper, &d.doc.layers, &[], &d.doc.meta, &path).unwrap();
        let back = crate::files::project::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let bi = back.layers.iter().position(|l| l.text.is_some()).unwrap();
        assert_eq!(back.layers[bi].text, d.doc.layers[i].text);
        assert_eq!(back.meta.fonts.len(), 1, "the font file is inside the project");
        assert!(back.layers[bi].tiles.values().flat_map(|t| t.iter()).any(|c| c.a() == 255), "and the text is painted after opening");
    }
    // undo takes the last change away
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
}

#[test]
fn signature_screens() {
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().test_open_signatures();
    h.run_steps(4);
    let (rect, _) = h.state().test_sign_pad();
    assert!(rect.width() > 400.0, "the pad is there: {rect:?}");
    // two lines: a wavy name and an underline
    let a = rect.min + vec2(40.0, 100.0);
    let name: Vec<Pos2> = (0..24).map(|i| a + vec2(i as f32 * 14.0, (i as f32 * 0.9).sin() * 40.0)).collect();
    stroke(&mut h, &name);
    stroke(&mut h, &[a + vec2(0.0, 80.0), a + vec2(200.0, 70.0), a + vec2(380.0, 82.0)]);
    h.run_steps(2);
    let (_, sig) = h.state().test_sign_pad();
    assert_eq!(sig.strokes.len(), 2, "two lines");
    assert!(sig.strokes[0].pts.len() >= 15, "{} points", sig.strokes[0].pts.len());
    assert!(sig.strokes[1].pts[0][3] >= sig.strokes[0].pts.last().unwrap()[3], "times go forward");
    // keep it in the Full slot
    h.get_by_label("Save in this slot").click();
    h.run_steps(3);
    let kept = crate::signature::load_in(&crate::signature::dir(), crate::signature::Slot::Full).expect("saved");
    assert_eq!(kept.strokes.len(), 2);
    // stamp it
    let n = h.state().draw_mut_ref().doc.layers.len();
    h.get_by_label("Stamp it").click();
    h.run_steps(4);
    {
        let d = h.state().draw_mut_ref();
        assert_eq!(d.doc.layers.len(), n + 1);
        assert_eq!(d.doc.layers[0].name, "Signature");
        assert!(!d.doc.layers[0].tiles.is_empty(), "the stamp is painted");
    }
    assert!(h.state().history().entries.last().unwrap().label.contains("Stamp"));
    // the export rule
    h.get_by_label_contains("Add my signature to exports").click();
    h.run_steps(3);
    let r = h.state().draw_mut_ref().doc.meta.auto_sign;
    assert!(r.on && r.slot == crate::signature::Slot::Full);
    // write it live
    let n = h.state().draw_mut_ref().doc.layers.len();
    h.get_by_label("Write it live").click();
    h.run_steps(2);
    assert_eq!(h.state().draw_mut_ref().doc.layers.len(), n + 1);
    assert!(h.state().draw_mut_ref().signing_live());
    wait_until(&mut h, "the signature is written", |h| !h.state().draw_mut_ref().signing_live());
    {
        let d = h.state().draw_mut_ref();
        assert!(!d.doc.layers[0].tiles.is_empty(), "the live signature is on the layer");
        assert_eq!(d.doc.layers[0].vector.as_ref().unwrap().strokes.len(), 2, "all lines written");
    }
    h.run_steps(4);
    save(&mut h, "sign_01_window");
    crate::signature::delete_in(&crate::signature::dir(), crate::signature::Slot::Full);
}

#[test]
fn font_maker_screens() {
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().test_open_font_maker();
    h.run_steps(4);
    assert_eq!(h.state().test_font_maker().order[0], 'A', "starts with the first letter");
    let rect = h.state().test_font_maker().pad_rect;
    assert!(rect.width() > 300.0);
    // a gothic-ish A: two slanted strokes and a bar, then B as a ring
    let at = |x: f32, y: f32| rect.min + vec2(x, y);
    stroke(&mut h, &[at(60.0, 288.0), at(120.0, 120.0), at(160.0, 80.0)]);
    stroke(&mut h, &[at(160.0, 80.0), at(220.0, 200.0), at(260.0, 288.0)]);
    stroke(&mut h, &[at(100.0, 220.0), at(160.0, 205.0), at(225.0, 220.0)]);
    h.run_steps(2);
    assert_eq!(h.state().test_font_maker().font.glyphs[&'A'].len(), 3, "three lines on A");
    h.get_by_label("Next letter").click();
    h.run_steps(3);
    assert_eq!(h.state().test_font_maker().current_char(), Some('B'), "moved to B without picking a key");
    let ring: Vec<Pos2> = (0..=30).map(|i| {
        let a = i as f32 / 30.0 * std::f32::consts::TAU;
        at(160.0 + a.cos() * 80.0, 190.0 + a.sin() * 95.0)
    }).collect();
    stroke(&mut h, &ring);
    h.run_steps(2);
    assert!(h.state().test_font_maker().font.glyphs.contains_key(&'B'));
    // in the project, and used as text
    h.get_by_label("Use in this project").click();
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.meta.fonts.len(), 1);
    assert_eq!(h.state().draw_mut_ref().doc.meta.fonts[0].name, "My handwriting");
    {
        let d = h.state_mut().draw_mut();
        d.add_text_layer(pos2(200.0, 600.0));
        let i = d.doc.index_of(d.active_id()).unwrap();
        let t = d.doc.layers[i].text.as_mut().unwrap();
        t.font = "My handwriting".into();
        t.text = "AB BA".into();
        t.size = 500.0;
        d.rebuild_text(i);
        assert!(!d.doc.layers[i].tiles.is_empty(), "the text is painted with my letters");
    }
    h.run_steps(4);
    save(&mut h, "font_01_maker");
    crate::handfont::save_draft(&crate::handfont::HandFont::default());
}

#[test]
fn recorder_screens() {
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().test_open_recorder();
    h.run_steps(3);
    h.get_by_label("Start recording").click();
    h.run_steps(3);
    assert!(h.state().test_recording().is_some(), "recording started");
    // draw on the canvas behind the window: every finished stroke is a frame
    stroke(&mut h, &[pos2(560.0, 300.0), pos2(700.0, 330.0), pos2(820.0, 300.0)]);
    h.run_steps(3);
    stroke(&mut h, &[pos2(560.0, 900.0), pos2(700.0, 930.0), pos2(820.0, 900.0)]);
    wait_until(&mut h, "frames are encoded", |h| h.state().test_recording().is_some_and(|r| r.0 >= 3));
    save(&mut h, "rec_01_recording");
    // pause: new strokes are not filmed
    h.get_by_label("Pause").click();
    h.run_steps(3);
    let frozen = h.state().test_recording().unwrap().0;
    stroke(&mut h, &[pos2(560.0, 500.0), pos2(820.0, 520.0)]);
    h.run_steps(4);
    std::thread::sleep(std::time::Duration::from_millis(300));
    h.run_steps(2);
    assert_eq!(h.state().test_recording().unwrap().0, frozen, "nothing filmed while paused");
    let out = std::env::temp_dir().join(format!("seviath_rec_screen_{}.mp4", std::process::id()));
    h.state_mut().test_stop_recording(out.clone());
    h.run_steps(3);
    let msg = h.state().test_recorder_message().unwrap();
    assert!(msg.starts_with("Saved"), "{msg}");
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[4..8], b"ftyp");
    assert!(bytes.windows(4).any(|w| w == b"avc1"));
    // 3000 x 4000 picture, 1080p: portrait 810 x 1080
    let a = bytes.windows(4).position(|w| w == b"avc1").unwrap();
    assert_eq!((u16::from_be_bytes([bytes[a + 28], bytes[a + 29]]), u16::from_be_bytes([bytes[a + 30], bytes[a + 31]])), (810, 1080), "the video is the picture");
    assert!(h.state().test_recording().is_none());
    std::fs::remove_file(out).ok();
}

#[test]
fn work_history_screens() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    stroke(&mut h, &[pos2(560.0, 300.0), pos2(700.0, 330.0), pos2(820.0, 300.0)]);
    h.run_steps(3);
    stroke(&mut h, &[pos2(560.0, 500.0), pos2(700.0, 530.0), pos2(820.0, 500.0)]);
    h.run_steps(3);
    // an imported picture counts too
    h.state_mut().draw_mut().import_picture_layer(&image::RgbaImage::from_pixel(40, 40, image::Rgba([200, 30, 30, 255])), "my photo");
    h.run_steps(4);
    {
        let log = &h.state().draw_mut_ref().doc.log;
        assert_eq!(log.entries.len(), 3, "{:?}", log.entries.iter().map(|e| e.label.clone()).collect::<Vec<_>>());
        assert_eq!(log.entries.iter().filter(|e| e.kind == "stroke").count(), 2);
        assert_eq!(log.entries[2].kind, "import");
        assert_eq!(log.verify(), Ok(()));
    }
    h.state_mut().test_open_work_history();
    h.run_steps(4);
    save(&mut h, "log_01_work_history");
    // a reference that stays on show is counted, and written down when it is hidden
    {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(32, 32, image::Rgba([10, 120, 200, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let d = h.state_mut().draw_mut();
        d.refs.add_bytes("pose sheet", &png).unwrap();
        d.show_references();
    }
    let t0 = std::time::Instant::now();
    while t0.elapsed().as_secs_f32() < 2.6 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    h.state_mut().test_open_work_history(); // writes the time so far into the log
    h.run_steps(3);
    let log = &h.state().draw_mut_ref().doc.log;
    let r = log.entries.iter().find(|e| e.kind == "reference").expect("the reference time is in the log");
    assert_eq!(r.layer, "pose sheet");
    assert!(r.secs >= 2.0, "{}", r.secs); // the test clock runs faster than the wall clock
    assert_eq!(log.verify(), Ok(()));
}


#[test]
fn palette_and_shortcut_screens() {
    use crate::doc::LayerKind;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // Ctrl+K opens the palette, typing narrows it, Enter runs the first match
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    let before = h.state().draw_mut_ref().doc.layers.len();
    h.event(Event::Text("new lay".into()));
    h.run_steps(3);
    save(&mut h, "keys_01_palette");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert_eq!(h.state().draw_mut_ref().doc.layers.len(), before + 1, "the palette made a layer");
    // keys still work: B / E pick tools, Ctrl+Shift+N makes a layer
    h.key_press(Key::E);
    h.run_steps(2);
    assert_eq!(h.state().draw_mut_ref().tool_name(), "Eraser");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.layers.len(), before + 2);
    assert_eq!(h.state().draw_mut_ref().doc.layers[0].kind, LayerKind::Raster);
    // change the Brush key to Shift+B: the old key stops, the new one works
    h.state_mut().test_set_shortcut("tool.brush", "Shift+B");
    h.run_steps(2);
    h.key_press(Key::B);
    h.run_steps(2);
    assert_eq!(h.state().draw_mut_ref().tool_name(), "Eraser", "plain B no longer picks the brush");
    h.key_press_modifiers(Modifiers::SHIFT, Key::B);
    h.run_steps(2);
    assert_eq!(h.state().draw_mut_ref().tool_name(), "Brush");
    // moving a key to an action that has it takes it away from the other one
    h.state_mut().test_set_shortcut("tool.eraser", "Shift+B");
    assert_eq!(h.state().test_shortcut("tool.brush"), "");
    assert_eq!(h.state().test_shortcut("tool.eraser"), "Shift+B");
    // Alt+L is the layer search, not the Lasso tool (it takes the keyboard, so it goes last)
    h.key_press_modifiers(Modifiers::ALT, Key::L);
    h.run_steps(2);
    assert_ne!(h.state().draw_mut_ref().tool_name(), "Lasso");
    // the Shortcuts page
    h.state_mut().test_open_shortcuts_page();
    h.run_steps(4);
    save(&mut h, "keys_02_shortcuts_page");
}
