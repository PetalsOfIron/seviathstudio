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
    h.state_mut().draw_mut().layouts.working.set_mode(crate::layout::PanelId::SubView, crate::layout::Mode::Hidden);
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
    // the words box has the keyboard now (typing is one undo step); Escape lets go of it
    h.key_press(Key::Escape);
    h.run_steps(3);
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

#[test]
fn panel_and_workspace_screens() {
    use crate::layout::{Mode, PanelId, Side};
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let width_with_panels = h.state().draw_mut_ref().test_canvas_width();
    assert_eq!(h.state().draw_mut_ref().layouts.working.column(Side::Right), [PanelId::Color, PanelId::SubView, PanelId::Layers]);
    // the little buttons above a panel: Float turns a docked panel into a window over the canvas
    let n = h.get_all_by_label("Float").count();
    assert!(n >= 3, "every docked panel has a Float button: {n}");
    h.get_all_by_label("Float").nth(2).unwrap().click();
    h.run_steps(4);
    let floating: Vec<PanelId> = PanelId::ALL.iter().copied().filter(|p| h.state().draw_mut_ref().layouts.working.get(*p).mode == Mode::Floating).collect();
    assert_eq!(floating.len(), 1, "{floating:?}");
    // "Dock" in that window puts it back
    h.get_by_label("Dock").click();
    h.run_steps(4);
    assert!(PanelId::ALL.iter().all(|p| h.state().draw_mut_ref().layouts.working.get(*p).mode == Mode::Docked));
    // its own window (shown inside this window when the computer can't make a second one)
    h.state_mut().draw_mut().layouts.working.set_mode(PanelId::SubView, Mode::Window);
    h.run_steps(4);
    assert_eq!(h.state().draw_mut_ref().layouts.working.column(Side::Right), [PanelId::Color, PanelId::Layers]);
    save(&mut h, "panels_01_window_mode");
    h.state_mut().draw_mut().layouts.working.dock(PanelId::SubView, Side::Right);
    // Layers on the left of the brush
    h.state_mut().draw_mut().layouts.working.dock(PanelId::Layers, Side::Left);
    h.run_steps(4);
    assert_eq!(h.state().draw_mut_ref().layouts.working.column(Side::Left), [PanelId::Brush, PanelId::Layers]);
    save(&mut h, "panels_02_layers_left");
    // workspaces: Focus hides everything and the canvas grows
    assert!(h.state().draw_mut_ref().layouts.changed());
    h.state_mut().draw_mut().layouts.switch("Focus");
    h.run_steps(4);
    let focus_w = h.state().draw_mut_ref().test_canvas_width();
    assert!(focus_w > width_with_panels + 300.0, "{focus_w} vs {width_with_panels}");
    save(&mut h, "panels_03_focus");
    // the palette brings a panel back
    h.state_mut().test_run_action_now("panel.layers");
    h.run_steps(4);
    assert_eq!(h.state().draw_mut_ref().layouts.working.get(PanelId::Layers).mode, Mode::Docked);
    // keep my own layout, switch away and back
    h.state_mut().draw_mut().layouts.save_as("My layout").unwrap();
    h.state_mut().test_run_action_now("ws.next");
    h.run_steps(2);
    assert_ne!(h.state().draw_mut_ref().layouts.active, "My layout");
    h.state_mut().draw_mut().layouts.switch("My layout");
    h.run_steps(2);
    assert_eq!(h.state().draw_mut_ref().layouts.working.get(PanelId::Layers).mode, Mode::Docked);
    assert_eq!(h.state().draw_mut_ref().layouts.working.get(PanelId::Color).mode, Mode::Hidden);
}

#[test]
fn parts_tab_screens() {
    use crate::doc::LayerKind;
    use crate::parts::Part;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1500.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // a small character sheet: named layers with paint in the right places (the page is 3000 x 4000)
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, x: i32, y: i32, bw: u32, bh: u32, c: [u8; 3]| {
            let img = image::RgbaImage::from_pixel(bw, bh, image::Rgba([c[0], c[1], c[2], 255]));
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            d.doc.layers.insert(0, l);
        };
        add("Face", 900, 700, 1200, 1500, [240, 210, 190]);
        add("Eye L", 1560, 1250, 300, 140, [40, 60, 120]);
        add("Eye R", 1140, 1250, 300, 140, [40, 60, 120]);
        add("Mouth", 1350, 1700, 300, 90, [190, 70, 80]);
        add("Hair front", 800, 500, 1400, 450, [120, 60, 160]);
        add("Layer 9", 100, 3000, 400, 400, [90, 90, 90]);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.2");
    h.run_steps(5);
    h.get_by_label("Suggest tags").click();
    h.run_steps(3);
    let n = h.state_mut().test_parts().suggestions.len();
    assert!(n >= 5, "names are understood: {n} suggestions");
    h.get_by_label("Accept the sure ones").click();
    h.run_steps(4);
    {
        let d = h.state().draw_mut_ref();
        let tag = |n: &str| d.doc.layers.iter().find(|l| l.name == n).and_then(|l| l.part);
        assert_eq!(tag("Eye L"), Some(Part::EyeL));
        assert_eq!(tag("Eye R"), Some(Part::EyeR));
        assert_eq!(tag("Mouth"), Some(Part::Mouth));
        assert_eq!(tag("Hair front"), Some(Part::HairFront));
        assert_eq!(tag("Face"), Some(Part::Face));
        assert_eq!(tag("Layer 9"), None, "an unnamed blob is not tagged by itself");
    }
    assert!(h.state().history().entries.last().unwrap().label.contains("suggested"));
    assert_eq!(h.state().history().entries.last().unwrap().tab, crate::tabs::Tab::Parts);
    save(&mut h, "parts_01_tagged");
    // not ready while a layer has no tag
    assert!(!h.state().draw_mut_ref().doc.meta.parts_ready);
    let blob = h.state().draw_mut_ref().doc.layers.iter().find(|l| l.name == "Layer 9").unwrap().id;
    h.state_mut().test_tag(&[blob], Some(Part::Accessory));
    h.run_steps(4);
    h.get_by_label("Mark as ready to rig").click();
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().doc.meta.parts_ready, "ready to rig");
    // undo takes the last step back (undo in this tab)
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!(!h.state().draw_mut_ref().doc.meta.parts_ready);
    save(&mut h, "parts_02_checklist");
}

#[test]
fn prerig_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1100.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // a red disc with a white fringe, a 2x2 hole, a stray speck and a few nearly invisible pixels
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut img = image::RgbaImage::new(300, 300);
        for y in 0..300i32 {
            for x in 0..300i32 {
                let r = (((x - 150).pow(2) + (y - 150).pow(2)) as f32).sqrt();
                if r <= 90.0 {
                    img.put_pixel(x as u32, y as u32, image::Rgba([200, 40, 40, 255]));
                } else if r <= 91.6 {
                    img.put_pixel(x as u32, y as u32, image::Rgba([255, 255, 255, 120]));
                }
            }
        }
        for (x, y) in [(150, 150), (151, 150), (150, 151), (151, 151)] {
            img.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
        }
        for (x, y) in [(5, 5), (6, 5), (5, 6), (6, 6)] {
            img.put_pixel(x, y, image::Rgba([200, 40, 40, 255]));
        }
        for i in 0..30 {
            img.put_pixel(20 + i, 290, image::Rgba([10, 10, 10, 3]));
        }
        let mut l = d.doc.new_layer("Eye L", LayerKind::Raster);
        l.tiles = crate::files::tiles_from_rgba(&img, 1200, 1500, w, h2);
        l.part = Some(crate::parts::Part::EyeL);
        l.rev += 1;
        d.doc.layers.insert(0, l);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.2");
    h.run_steps(3);
    h.get_by_label("Pre-rig improver").click();
    h.run_steps(5);
    {
        let n = h.state_mut().test_prerig().problems.len();
        assert!(n >= 4, "stray, hole, ghost and fringe are found: {n}");
    }
    save(&mut h, "prerig_01_found");
    // nothing has changed yet
    let steps = h.state().history().entries.len();
    let px_before = {
        let l = h.state().draw_mut_ref().doc.layers.iter().find(|l| l.name == "Eye L").unwrap().clone();
        (crate::prerig::px_get(&l, 1350, 1650).a(), crate::prerig::px_get(&l, 1205, 1790).a())
    };
    assert_eq!(px_before.0, 0, "the hole is still there");
    // Fix the first one: one undo step, and the list gets shorter
    let n0 = h.state_mut().test_prerig().problems.len();
    h.get_by_label("Fix").click();
    h.run_steps(4);
    assert_eq!(h.state().history().entries.len(), steps + 1);
    assert!(h.state().history().entries.last().unwrap().label.starts_with("Fix ·"));
    assert!(h.state_mut().test_prerig().problems.len() < n0);
    // Ignore one, fix the rest
    h.get_by_label("Ignore").click();
    h.run_steps(3);
    h.get_by_label("Fix everything").click();
    h.run_steps(4);
    assert!(h.state_mut().test_prerig().problems.is_empty(), "nothing left to fix");
    let hole_after = crate::prerig::px_get(&h.state().draw_mut_ref().doc.layers.iter().find(|l| l.name == "Eye L").unwrap().clone(), 1350, 1650).a();
    assert!(hole_after == 255 || h.state_mut().test_prerig().problems.is_empty());
    // undo takes the last fix back
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    save(&mut h, "prerig_02_after");
}

#[test]
fn rig_tab_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2400.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // a head (a disc) and an arm (a bar) on the 3000 x 4000 page
    let (head, arm) = {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut disc = image::RgbaImage::new(1000, 1000);
        for y in 0..1000i32 {
            for x in 0..1000i32 {
                if (((x - 500).pow(2) + (y - 500).pow(2)) as f32).sqrt() <= 450.0 {
                    disc.put_pixel(x as u32, y as u32, image::Rgba([240, 200, 170, 255]));
                }
            }
        }
        let bar = image::RgbaImage::from_pixel(900, 220, image::Rgba([70, 90, 200, 255]));
        let mut a = d.doc.new_layer("Head", LayerKind::Raster);
        a.tiles = crate::files::tiles_from_rgba(&disc, 1000, 800, w, h2);
        a.part = Some(crate::parts::Part::Face);
        a.rev += 1;
        let ida = a.id;
        let mut b = d.doc.new_layer("Arm", LayerKind::Raster);
        b.tiles = crate::files::tiles_from_rgba(&bar, 1100, 2100, w, h2);
        b.part = Some(crate::parts::Part::ArmL);
        b.rev += 1;
        let idb = b.id;
        d.doc.layers.insert(0, a);
        d.doc.layers.insert(0, b);
        d.doc.mark_all_dirty();
        (ida, idb)
    };
    h.state_mut().test_run_action_now("tab.3");
    h.state_mut().test_rig().pro = true;
    h.run_steps(5);
    // meshes for everything that is tagged
    h.get_by_label("Mesh all tagged parts").click();
    h.run_steps(4);
    {
        let rig = h.state().draw_mut_ref().doc.rig.clone();
        assert_eq!(rig.meshes.len(), 2, "a mesh for the head and one for the arm");
        assert!(rig.meshes.iter().all(|m| m.tris.len() >= 8), "{:?}", rig.meshes.iter().map(|m| m.tris.len()).collect::<Vec<_>>());
    }
    // a Rotation changer for the head, a slider, and the binding
    h.state_mut().test_rig().selected = vec![head];
    h.run_steps(2);
    h.get_by_label("New Rotation changer").click();
    h.run_steps(3);
    h.get_by_label("Add slider").click();
    h.run_steps(3);
    h.get_by_label("Bind: turn with the slider").click();
    h.run_steps(3);
    let (pid, cid) = {
        let r = h.state_mut().test_rig();
        (r.sel_param.expect("a slider is picked"), r.sel_changer.expect("a changer is picked"))
    };
    let _ = cid;
    assert_eq!(h.state().draw_mut_ref().doc.rig.bindings.len(), 1);
    assert_eq!(h.state_mut().test_moved(head), 0.0, "the slider is in the middle: nothing moves");
    save(&mut h, "rig_01_rest");
    h.state_mut().test_rig().values.set(pid, 30.0);
    h.run_steps(4);
    assert!(h.state_mut().test_moved(head) > 100.0, "the head turns with the slider");
    assert_eq!(h.state_mut().test_moved(arm), 0.0, "the arm has no changer and stays");
    save(&mut h, "rig_02_turned");
    // undo takes the binding away (undo in this tab)
    let steps = h.state().history().entries.len();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().doc.rig.bindings.is_empty());
    assert_eq!(h.state().history().cursor, steps - 1);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.rig.bindings.len(), 1, "redo brings it back");
    // a Warp changer for the arm with a keyform made from a dragged shape
    h.state_mut().test_rig().selected = vec![arm];
    h.run_steps(2);
    h.get_by_label("New Warp changer").click();
    h.run_steps(3);
    let warp = h.state_mut().test_rig().sel_changer.unwrap();
    h.state_mut().test_rig().values.set(pid, 30.0);
    h.get_by_label("Edit the shape").click();
    h.run_steps(3);
    {
        let w = h.state_mut().test_rig().test_warp_edit().expect("editing");
        let n = w.len();
        w[n - 1] = [0.0, 150.0]; // the last corner goes down
    }
    h.get_by_label("Save the shape at this value").click();
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().doc.rig.bindings.iter().any(|b| b.changer == warp));
    assert!(h.state_mut().test_moved(arm) > 50.0, "the arm bends at this value");
    h.state_mut().test_rig().values.set(pid, 0.0);
    h.run_steps(2);
    assert!(h.state_mut().test_moved(arm) < 1.0, "and is straight again in the middle");
    save(&mut h, "rig_03_warp");
    let _ = warp;
}

#[test]
fn parts_one_click_button() {
    use crate::doc::LayerKind;
    use crate::parts::Part;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1000.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, x: i32, y: i32, bw: u32, bh: u32| {
            let img = image::RgbaImage::from_pixel(bw, bh, image::Rgba([120, 120, 200, 255]));
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            d.doc.layers.insert(0, l);
        };
        add("Eye L", 1560, 1250, 300, 140);
        add("Mouth", 1350, 1700, 300, 90);
        add("sparkle 3", 100, 3000, 200, 200);
        add("Layer 9", 2400, 3500, 200, 200);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.2");
    h.run_steps(4);
    // a rule of my own for "sparkle"
    h.state_mut().test_parts().new_rule = ("sparkle*".into(), Some(Part::Accessory));
    h.run_steps(2);
    h.get_by_label("Add rule").click();
    h.run_steps(3);
    assert_eq!(h.state_mut().test_parts().rules.len(), 1);
    h.get_by_label("Tag everything for me").click();
    h.run_steps(4);
    {
        let d = h.state().draw_mut_ref();
        let tag = |n: &str| d.doc.layers.iter().find(|l| l.name == n).and_then(|l| l.part);
        assert_eq!(tag("Eye L"), Some(Part::EyeL));
        assert_eq!(tag("Mouth"), Some(Part::Mouth));
        assert_eq!(tag("Layer 9"), None, "an unnamed layer is left alone without guesses");
    }
    assert!(h.state().history().entries.last().unwrap().label.contains("automatically"));
    save(&mut h, "parts_03_auto");
    // undo puts it all back in one step
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().doc.layers.iter().all(|l| l.part.is_none()));
    // a rule that is typed stays in the file for next time
    assert_eq!(crate::parts::load_rules().len(), 1);
    let _ = std::fs::remove_file(std::env::temp_dir().join(format!("seviath_name_rules_{}.json", std::process::id())));
}

#[test]
fn rig_tab_more_changers_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let (hair, cheek, left, right, head) = {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, img: image::RgbaImage, x: i32, y: i32| {
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            let id = l.id;
            d.doc.layers.insert(0, l);
            id
        };
        let solid = |w: u32, h: u32, c: [u8; 3]| image::RgbaImage::from_pixel(w, h, image::Rgba([c[0], c[1], c[2], 255]));
        let disc = |r: i32| {
            let mut img = image::RgbaImage::new(2 * r as u32, 2 * r as u32);
            for y in 0..2 * r {
                for x in 0..2 * r {
                    if (((x - r).pow(2) + (y - r).pow(2)) as f32).sqrt() <= r as f32 {
                        img.put_pixel(x as u32, y as u32, image::Rgba([240, 190, 160, 255]));
                    }
                }
            }
            img
        };
        let hair = add("Hair", solid(160, 900, [120, 60, 160]), 1400, 600);
        let cheek = add("Cheek", disc(200), 1000, 1700);
        let left = add("Left", solid(300, 300, [60, 160, 120]), 1200, 2600);
        let right = add("Right", solid(300, 300, [60, 120, 200]), 1500, 2600);
        let head = add("Head", disc(300), 1800, 1700);
        d.doc.mark_all_dirty();
        (hair, cheek, left, right, head)
    };
    h.state_mut().test_run_action_now("tab.3");
    h.state_mut().test_rig().pro = true;
    h.run_steps(4);
    h.state_mut().test_rig().selected = vec![hair, cheek, left, right, head];
    h.run_steps(2);
    h.get_by_label("Make mesh for the picked parts").click();
    h.run_steps(4);
    assert_eq!(h.state().draw_mut_ref().doc.rig.meshes.len(), 5);
    // a slider for everything, set to its far end while the shapes are made
    h.get_by_label("Add slider").click();
    h.run_steps(3);
    let pid = h.state_mut().test_rig().sel_param.unwrap();
    h.state_mut().test_rig().values.set(pid, 30.0);

    // Line changer on the hair: the tip swings
    h.state_mut().test_rig().selected = vec![hair];
    h.run_steps(2);
    h.get_by_label("New Line changer").click();
    h.run_steps(3);
    h.get_by_label("Edit the shape").click();
    h.run_steps(3);
    {
        let w = h.state_mut().test_rig().test_warp_edit().expect("editing the line");
        assert_eq!(w.len(), 5);
        w[4] = [200.0, 0.0];
        w[3] = [80.0, 0.0];
    }
    h.get_by_label("Save the shape at this value").click();
    h.run_steps(3);
    assert!(h.state_mut().test_moved(hair) > 100.0, "the strand bends at the end of the slider");
    h.state_mut().test_rig().values.set(pid, 0.0);
    h.run_steps(2);
    assert!(h.state_mut().test_moved(hair) < 1.0, "and is straight in the middle");
    h.state_mut().test_rig().values.set(pid, 30.0);

    // Soft changer on the cheek: pushed and grown
    h.state_mut().test_rig().selected = vec![cheek];
    h.run_steps(2);
    h.get_by_label("New Soft changer").click();
    h.run_steps(3);
    h.get_by_label("Edit the shape").click();
    h.run_steps(3);
    {
        let w = h.state_mut().test_rig().test_warp_edit().expect("editing the soft changer");
        w[0] = [60.0, 0.0];
    }
    h.state_mut().test_rig().soft_grow = 0.3;
    h.get_by_label("Save the shape at this value").click();
    h.run_steps(3);
    assert!(h.state_mut().test_moved(cheek) > 20.0, "the cheek is pushed");

    // glue the two touching parts
    h.state_mut().test_rig().selected = vec![left, right];
    h.run_steps(2);
    h.get_by_label("Glue the two picked parts").click();
    h.run_steps(3);
    {
        let rig = h.state().draw_mut_ref().doc.rig.clone();
        assert_eq!(rig.glues.len(), 1, "one seam");
        assert!(rig.glues[0].pairs.len() >= 2, "{} pairs", rig.glues[0].pairs.len());
    }

    // a second slider and a grid shape for the head
    h.state_mut().test_rig().new_param = ("Head up".into(), -30.0, 30.0);
    h.get_by_label("Add slider").click();
    h.run_steps(3);
    let pid2 = h.state_mut().test_rig().sel_param.unwrap();
    assert_ne!(pid, pid2);
    h.state_mut().test_rig().selected = vec![head];
    h.run_steps(2);
    h.get_by_label("New Rotation changer").click();
    h.run_steps(3);
    h.state_mut().test_rig().sel_param = Some(pid);
    h.state_mut().test_rig().sel_param2 = Some(pid2);
    h.state_mut().test_rig().values.set(pid, 30.0);
    h.state_mut().test_rig().values.set(pid2, 30.0);
    h.run_steps(3);
    h.get_by_label("Set the turn at these two values").click();
    h.run_steps(3);
    {
        let rig = h.state().draw_mut_ref().doc.rig.clone();
        let b = rig.bindings.iter().find(|b| b.params.len() == 2).expect("a grid binding");
        assert_eq!(b.params, vec![pid, pid2]);
    }
    h.state_mut().test_rig_edit(|r| {
        if let Some(b) = r.bindings.iter_mut().find(|b| b.params.len() == 2) {
            b.ease = crate::rig::Ease::Smooth;
        }
    });
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().doc.rig.bindings.iter().any(|b| b.ease == crate::rig::Ease::Smooth));
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.state_mut().test_rig().values.set(pid, 30.0);
    h.run_steps(4);
    save(&mut h, "rig_04_more_changers");
}

#[test]
fn rig_tab_uncut_skin_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // one layer with the whole figure on it: a head, a body and an arm. Nothing is cut apart.
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut img = image::RgbaImage::new(w.min(3000), h2.min(4000));
        for y in 0..img.height() as i32 {
            for x in 0..img.width() as i32 {
                let in_body = (1200..1800).contains(&x) && (1500..2600).contains(&y);
                let in_arm = (1800..2400).contains(&x) && (1500..1750).contains(&y);
                let in_head = (((x - 1500).pow(2) + (y - 1200).pow(2)) as f32).sqrt() <= 220.0;
                if in_head {
                    img.put_pixel(x as u32, y as u32, image::Rgba([240, 200, 170, 255]));
                } else if in_body {
                    img.put_pixel(x as u32, y as u32, image::Rgba([70, 130, 200, 255]));
                } else if in_arm {
                    img.put_pixel(x as u32, y as u32, image::Rgba([60, 110, 180, 255]));
                }
            }
        }
        let mut l = d.doc.new_layer("Whole drawing", LayerKind::Raster);
        l.tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, h2);
        l.rev += 1;
        d.doc.layers.insert(0, l);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.state_mut().test_rig().pro = true;
    h.run_steps(4);
    h.get_by_label("Make one mesh of the drawing").click();
    h.run_steps(5);
    {
        let sk = h.state().draw_mut_ref().doc.rig.skin.clone().expect("a skin");
        assert!(sk.tris.len() > 40, "{} triangles", sk.tris.len());
        assert!(!sk.layers.is_empty());
        assert!(sk.bones.is_empty());
    }
    // a spine bone, and an arm bone inside it
    h.state_mut().test_add_bone([1500.0, 2600.0], [1500.0, 1500.0]);
    h.run_steps(3);
    let spine = h.state_mut().test_rig().sel_changer.unwrap();
    h.state_mut().test_add_bone([1800.0, 1625.0], [2400.0, 1625.0]);
    h.run_steps(3);
    let arm = h.state_mut().test_rig().sel_changer.unwrap();
    {
        let rig = h.state().draw_mut_ref().doc.rig.clone();
        assert_eq!(rig.skin.as_ref().unwrap().bones.len(), 2);
        assert_eq!(rig.changer(arm).unwrap().parent, Some(spine), "the arm bone sits inside the spine bone");
    }
    h.get_by_label("Weights from the bones").click();
    h.run_steps(4);
    {
        let sk = h.state().draw_mut_ref().doc.rig.skin.clone().unwrap();
        let w = sk.weights_at(2100.0, 1625.0);
        assert!(w[1] > 0.9, "the middle of the arm follows the arm bone: {w:?}");
        let w = sk.weights_at(1500.0, 2200.0);
        assert!(w[0] > 0.9, "the body follows the spine: {w:?}");
    }
    // a slider for each bone
    h.get_by_label("Slider for this bone").click();
    h.run_steps(3);
    let arm_slider = h.state_mut().test_rig().sel_param.unwrap();
    h.state_mut().test_rig().sel_changer = Some(spine);
    h.run_steps(2);
    h.get_by_label("Slider for this bone").click();
    h.run_steps(3);
    let spine_slider = h.state_mut().test_rig().sel_param.unwrap();
    assert_ne!(arm_slider, spine_slider);
    assert!(h.state_mut().test_moved(u64::MAX) < 0.01, "sliders in the middle: nothing moves");
    save(&mut h, "rig_05_skin_rest");
    h.state_mut().test_rig().values.set(arm_slider, 45.0);
    h.run_steps(3);
    let arm_only = h.state_mut().test_moved(u64::MAX);
    assert!(arm_only > 100.0, "the arm swings: {arm_only}");
    h.state_mut().test_rig().values.set(spine_slider, 20.0);
    h.state_mut().test_rig().heat = true;
    h.run_steps(4);
    let both = h.state_mut().test_moved(u64::MAX);
    assert!(both > arm_only, "the spine moves everything that follows it: {both} vs {arm_only}");
    save(&mut h, "rig_06_skin_posed");
    // undo walks back through the edits (the last one was the second slider)
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().doc.rig.params.len() == 1);
}

#[test]
fn rig_tab_manual_weights_screens() {
    use crate::doc::LayerKind;
    use crate::rig::skin::{Brush, BrushOpts};
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // the figure on one layer, plus a separate arm layer that is tagged as an arm
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut body = image::RgbaImage::new(w.min(3000), h2.min(4000));
        for y in 1500..2600u32 {
            for x in 1200..1800u32 {
                body.put_pixel(x, y, image::Rgba([70, 130, 200, 255]));
            }
        }
        let arm = image::RgbaImage::from_pixel(600, 250, image::Rgba([60, 110, 180, 255]));
        let mut a = d.doc.new_layer("Body", LayerKind::Raster);
        a.tiles = crate::files::tiles_from_rgba(&body, 0, 0, w, h2);
        a.rev += 1;
        let mut b = d.doc.new_layer("Arm", LayerKind::Raster);
        b.tiles = crate::files::tiles_from_rgba(&arm, 1800, 1500, w, h2);
        b.part = Some(crate::parts::Part::ArmL);
        b.rev += 1;
        d.doc.layers.insert(0, a);
        d.doc.layers.insert(0, b);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.state_mut().test_rig().pro = true;
    h.run_steps(4);
    h.get_by_label("Make one mesh of the drawing").click();
    h.run_steps(4);
    h.state_mut().test_add_bone([1500.0, 2600.0], [1500.0, 1500.0]);
    h.run_steps(2);
    let spine = h.state_mut().test_rig().sel_changer.unwrap();
    h.state_mut().test_add_bone([1800.0, 1625.0], [2400.0, 1625.0]);
    h.run_steps(2);
    let arm_bone = h.state_mut().test_rig().sel_changer.unwrap();
    let weight = |h: &Harness<'_, SeviathApp>, bone: usize, x: f32, y: f32| h.state().draw_mut_ref().doc.rig.skin.as_ref().unwrap().weight_at(bone, x, y);
    // no automatic weights: everything is loose until you paint
    assert_eq!(weight(&h, 1, 2100.0, 1625.0), 0.0);
    // paint the arm bone over the arm, by hand
    let before = h.state().history().entries.len();
    h.state_mut().test_paint([2100.0, 1625.0], Brush::Add, BrushOpts { radius: 400.0, strength: 1.0, hardness: 1.0, keep_normalized: true });
    h.run_steps(3);
    assert!(weight(&h, 1, 2100.0, 1625.0) > 0.99, "painted");
    assert_eq!(weight(&h, 1, 1500.0, 2400.0), 0.0, "and only there");
    assert_eq!(h.state().history().entries.len(), before + 1, "one step for the stroke");
    // the weights from a tag: the layer tagged as an arm gives the arm bone its shape
    h.state_mut().test_rig().sel_changer = Some(arm_bone);
    h.state_mut().test_rig().sel_changer = Some(spine);
    h.run_steps(2);
    h.get_by_label("Flood the whole bone").click();
    h.run_steps(3);
    assert!(weight(&h, 0, 1000.0, 3500.0) > 0.4, "flooded everywhere (to the strength)");
    h.get_by_label("Clear this bone").click();
    h.run_steps(3);
    assert_eq!(weight(&h, 0, 1000.0, 3500.0), 0.0);
    h.state_mut().test_rig().sel_changer = Some(arm_bone);
    h.run_steps(2);
    h.get_by_label("Clear this bone").click();
    h.run_steps(3);
    h.get_by_label("Give the bone the parts with this tag").click();
    h.run_steps(3);
    assert!(weight(&h, 1, 2100.0, 1625.0) > 0.95, "the arm layer's paint is the arm bone's weight");
    assert_eq!(weight(&h, 1, 1500.0, 2200.0), 0.0, "the body is not");
    // mirror the arm bone's weights onto the spine bone across the page's middle
    h.state_mut().test_rig().mirror_to = Some(spine);
    h.state_mut().test_rig().mirror_axis = 1500.0;
    h.run_steps(2);
    h.get_by_label("Mirror").click();
    h.run_steps(3);
    assert!(weight(&h, 0, 900.0, 1625.0) > 0.9, "the arm's weights are on the other side now: {}", weight(&h, 0, 900.0, 1625.0));
    // painting shows the picture at rest, even with a slider moved
    h.get_by_label("Slider for this bone").click();
    h.run_steps(3);
    let slider = h.state_mut().test_rig().sel_param.unwrap();
    h.state_mut().test_rig().values.set(slider, 45.0);
    h.run_steps(3);
    assert!(h.state_mut().test_moved(u64::MAX) > 50.0, "posed");
    h.state_mut().test_weight_mode(true);
    h.run_steps(3);
    assert!(h.state_mut().test_moved(u64::MAX) < 0.01, "at rest while painting");
    save(&mut h, "rig_07_weight_painting");
    h.state_mut().test_weight_mode(false);
    h.run_steps(2);
    assert!(h.state_mut().test_moved(u64::MAX) > 50.0);
    // undo takes back the last weight step
    let arm_after = weight(&h, 0, 900.0, 1625.0);
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    let _ = arm_after;
}

#[test]
fn rig_tab_auto_face_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1500.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    // a simple face with named layers and nothing tagged
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, x: i32, y: i32, bw: u32, bh: u32, c: [u8; 3]| {
            let mut img = image::RgbaImage::new(bw, bh);
            for yy in 0..bh {
                for xx in 0..bw {
                    img.put_pixel(xx, yy, image::Rgba([c[0], c[1], c[2], 255]));
                }
            }
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            d.doc.layers.insert(0, l);
        };
        add("hair back", 900, 700, 1200, 1500, [90, 60, 140]);
        add("face", 1000, 900, 1000, 1200, [240, 205, 180]);
        add("eye L", 1550, 1400, 250, 110, [40, 70, 140]);
        add("eye R", 1200, 1400, 250, 110, [40, 70, 140]);
        add("brow L", 1550, 1250, 250, 40, [70, 40, 30]);
        add("brow R", 1200, 1250, 250, 40, [70, 40, 30]);
        add("nose", 1450, 1550, 100, 200, [225, 180, 150]);
        add("mouth inside", 1350, 1900, 300, 90, [120, 30, 50]);
        add("mouth", 1350, 1900, 300, 90, [200, 80, 90]);
        add("hair front", 950, 800, 1100, 350, [110, 70, 170]);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    // beginner mode: only the automatic rig and the sliders are shown
    assert!(h.query_by_label("Mesh all tagged parts").is_none(), "pro things are hidden in beginner mode");
    h.get_by_label("This is a face").click();
    h.run_steps(5);
    {
        let d = h.state().draw_mut_ref();
        assert_eq!(d.doc.layers.iter().filter(|l| l.part.is_some()).count(), 10, "tagged by name");
        let rig = d.doc.rig.clone();
        assert_eq!(rig.meshes.len(), 10);
        for n in ["Angle X", "Angle Y", "Angle Z", "Eyes open", "Brows", "Mouth open", "Mouth A", "Mouth O"] {
            assert!(rig.params.iter().any(|p| p.name == n), "a slider {n}");
        }
        assert!(rig.auto.is_some());
    }
    assert!(h.state().history().entries.iter().any(|e| e.label.starts_with("Rig the face")));
    // the sliders move the drawing
    let (ax, eyes, a) = {
        let r = h.state().draw_mut_ref().doc.rig.clone();
        let id = |n: &str| r.params.iter().find(|p| p.name == n).unwrap().id;
        (id("Angle X"), id("Eyes open"), id("Mouth A"))
    };
    let eye_layer = h.state().draw_mut_ref().doc.layers.iter().find(|l| l.name == "eye L").unwrap().id;
    let mouth_layer = h.state().draw_mut_ref().doc.layers.iter().find(|l| l.name == "mouth").unwrap().id;
    save(&mut h, "rig_08_face_rest");
    h.state_mut().test_rig().values.set(ax, 30.0);
    h.state_mut().test_rig().values.set(eyes, 0.0);
    h.state_mut().test_rig().values.set(a, 1.0);
    h.run_steps(4);
    assert!(h.state_mut().test_moved(eye_layer) > 30.0, "the eye closes and slides");
    assert!(h.state_mut().test_moved(mouth_layer) > 30.0, "the mouth opens");
    save(&mut h, "rig_09_face_posed");
    // the test motion plays by itself
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.get_by_label("Test motion").click();
    h.run_steps(8);
    assert!(h.state_mut().test_rig().values.0.len() >= 8, "every slider is moving");
    h.get_by_label("Stop the test motion").click();
    h.run_steps(3);
    assert!(h.state_mut().test_rig().values.0.is_empty());
    // undo takes the whole rig back in one step
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().doc.rig.params.is_empty(), "one undo step for the whole face rig");
}

#[test]
fn rig_tab_body_hair_and_checker_screens() {
    use crate::doc::LayerKind;
    use crate::rig::check::IssueKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2000.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, x: i32, y: i32, bw: u32, bh: u32, c: [u8; 3]| {
            let img = image::RgbaImage::from_pixel(bw, bh, image::Rgba([c[0], c[1], c[2], 255]));
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            d.doc.layers.insert(0, l);
        };
        add("hair back", 1000, 500, 1000, 1100, [90, 60, 140]);
        add("face", 1100, 700, 800, 900, [240, 205, 180]);
        add("eye L", 1550, 1100, 200, 90, [40, 70, 140]);
        add("eye R", 1250, 1100, 200, 90, [40, 70, 140]);
        add("mouth", 1350, 1450, 300, 70, [200, 80, 90]);
        add("hair front", 1050, 600, 900, 250, [110, 70, 170]);
        add("torso", 1200, 1700, 600, 800, [70, 130, 200]);
        add("arm L", 1800, 1700, 160, 500, [60, 110, 180]);
        add("hand L", 1800, 2200, 160, 160, [240, 205, 180]);
        add("arm R", 1040, 1700, 160, 500, [60, 110, 180]);
        add("hand R", 1040, 2200, 160, 160, [240, 205, 180]);
        add("leg L", 1500, 2500, 300, 700, [50, 60, 120]);
        add("leg R", 1200, 2500, 300, 700, [50, 60, 120]);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.get_by_label("This is a face").click();
    h.run_steps(5);
    h.get_by_label("Rig the body").click();
    h.run_steps(5);
    h.get_by_label("Hair sway").click();
    h.run_steps(5);
    let rig = h.state().draw_mut_ref().doc.rig.clone();
    for n in ["Angle X", "Eyes open", "Mouth open", "Body tilt", "Breath", "Arm L", "Arm R", "Hand L", "Leg L", "Leg R", "Hair sway", "Hand L reach X", "Hand L reach Y"] {
        assert!(rig.params.iter().any(|p| p.name == n), "a slider {n}");
    }
    assert_eq!(rig.iks.len(), 2, "both hands can reach");
    assert_eq!(rig.auto.as_ref().unwrap().groups.len(), 3, "face, body and hair");
    // the sliders work together: the head leans with the body, the hair swings
    let id = |n: &str| rig.params.iter().find(|p| p.name == n).unwrap().id;
    let layer = |h: &Harness<'_, SeviathApp>, n: &str| h.state().draw_mut_ref().doc.layers.iter().find(|l| l.name == n).unwrap().id;
    let (face, hair, hand, torso) = (layer(&h, "face"), layer(&h, "hair front"), layer(&h, "hand L"), layer(&h, "torso"));
    h.state_mut().test_rig().values.set(id("Body tilt"), 10.0);
    h.run_steps(3);
    assert!(h.state_mut().test_moved(face) > 20.0, "the head leans with the body");
    assert!(h.state_mut().test_moved(torso) > 20.0);
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.state_mut().test_rig().values.set(id("Hair sway"), 1.0);
    h.run_steps(3);
    assert!(h.state_mut().test_moved(hair) > 20.0, "the hair sways");
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.state_mut().test_rig().values.set(id("Hand L reach X"), -150.0);
    h.state_mut().test_rig().values.set(id("Hand L reach Y"), -250.0);
    h.run_steps(3);
    assert!(h.state_mut().test_moved(hand) > 100.0, "the hand is brought up by the reach sliders");
    save(&mut h, "rig_10_body");
    h.state_mut().test_rig().values = crate::rig::Values::default();
    // the quality checker: gaps found, glued with one click each, until it is quiet
    h.get_by_label("Check the rig").click();
    h.run_steps(5);
    for _ in 0..80 {
        let gaps = h.state_mut().test_rig().issues.iter().filter(|i| matches!(i.kind, IssueKind::Gap { .. })).count();
        if gaps == 0 {
            break;
        }
        h.get_all_by_label("Glue them").next().unwrap().click();
        h.run_steps(5);
    }
    assert_eq!(h.state_mut().test_rig().issues.iter().filter(|i| matches!(i.kind, IssueKind::Gap { .. })).count(), 0, "no gaps are left");
    assert!(!h.state().draw_mut_ref().doc.rig.glues.is_empty() || h.state_mut().test_check() == 0);
    save(&mut h, "rig_11_checked");
    // undo goes back one fix at a time; the whole body rig is one step too
    let steps = h.state().history().entries.len();
    assert!(steps >= 4);
}

#[test]
fn rig_tab_gizmo_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2200.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, x: i32, y: i32, bw: u32, bh: u32, c: [u8; 3]| {
            let img = image::RgbaImage::from_pixel(bw, bh, image::Rgba([c[0], c[1], c[2], 255]));
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            d.doc.layers.insert(0, l);
        };
        add("hair back", 1000, 600, 1000, 1100, [90, 60, 140]);
        add("face", 1100, 800, 800, 900, [240, 205, 180]);
        add("eye L", 1550, 1200, 200, 90, [40, 70, 140]);
        add("eye R", 1250, 1200, 200, 90, [40, 70, 140]);
        add("nose", 1450, 1350, 100, 200, [225, 180, 150]);
        add("mouth", 1350, 1560, 300, 70, [200, 80, 90]);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.get_by_label("This is a face").click();
    h.run_steps(6);
    let (ax, ay, az) = {
        let r = h.state().draw_mut_ref().doc.rig.clone();
        let id = |n: &str| r.params.iter().find(|p| p.name == n).unwrap().id;
        (id("Angle X"), id("Angle Y"), id("Angle Z"))
    };
    let (centre, radius) = h.state_mut().test_rig().gizmo_geom.expect("the rings are drawn");
    // drag the left-right ring to the right: the head turns
    let start = centre + vec2(radius * 0.3, 0.0);
    h.hover_at(start);
    h.step();
    button(&h, start, true);
    h.step();
    h.hover_at(start + vec2(25.0, 0.0));
    h.step();
    h.hover_at(start + vec2(50.0, 0.0));
    h.step();
    button(&h, start + vec2(50.0, 0.0), false);
    h.run_steps(3);
    let x = h.state_mut().test_rig().values.0.get(&ax).copied().unwrap_or(0.0);
    assert!((x - 15.0).abs() < 1.0, "50 px on the ring moves it a quarter of the range: {x}");
    // the up-down ring: drag it up
    let (centre, radius) = h.state_mut().test_rig().gizmo_geom.unwrap();
    let start = centre + vec2(radius, 0.0);
    h.hover_at(start);
    h.step();
    button(&h, start, true);
    h.step();
    h.hover_at(start + vec2(0.0, -30.0));
    h.step();
    button(&h, start + vec2(0.0, -30.0), false);
    h.run_steps(3);
    let y = h.state_mut().test_rig().values.0.get(&ay).copied().unwrap_or(0.0);
    assert!(y > 5.0, "dragging up raises it: {y}");
    // the tilt ring: a quarter turn around
    let (centre, radius) = h.state_mut().test_rig().gizmo_geom.unwrap();
    let start = centre + vec2(radius * 0.707, radius * 0.707);
    h.hover_at(start);
    h.step();
    button(&h, start, true);
    h.step();
    h.hover_at(centre + vec2(0.0, radius));
    h.step();
    h.hover_at(centre + vec2(-radius * 0.707, radius * 0.707));
    h.step();
    button(&h, centre + vec2(-radius * 0.707, radius * 0.707), false);
    h.run_steps(3);
    let z = h.state_mut().test_rig().values.0.get(&az).copied().unwrap_or(0.0);
    assert!((z - 15.0).abs() < 1.5, "a quarter turn around the ring is a quarter of the range: {z}");
    save(&mut h, "rig_12_gizmo");
    // snapping and limits from the rig
    h.state_mut().test_rig().snap = 15.0;
    let before = h.state_mut().test_rig().values.clone();
    let _ = before;
    // Key this pose: kept, and K does it too in the Rig tab
    let n = h.state().draw_mut_ref().doc.rig.poses.len();
    h.get_by_label("Key this pose (K)").click();
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.rig.poses.len(), n + 1);
    h.key_press(Key::K);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.rig.poses.len(), n + 2, "K keys a pose in the Rig tab");
    let pose = h.state().draw_mut_ref().doc.rig.poses[0].clone();
    assert!(pose.values.iter().any(|(id, v)| *id == ax && (*v - 15.0).abs() < 1.0), "the pose holds the turn: {:?}", pose.values);
    // going back to rest and then to the pose
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.run_steps(2);
    h.get_all_by_label("Go to").next().unwrap().click();
    h.run_steps(3);
    let back = h.state_mut().test_rig().values.0.get(&ax).copied().unwrap_or(0.0);
    assert!((back - 15.0).abs() < 1.0, "{back}");
    // recording while posing makes a motion
    h.get_by_label("Record while posing").click();
    h.run_steps(2);
    for i in 0..12 {
        h.state_mut().test_rig().values.set(ax, i as f32 * 2.0);
        h.run_steps(3);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.get_by_label("Stop recording").click();
    h.run_steps(3);
    let clips = h.state().draw_mut_ref().doc.rig.clips.clone();
    assert_eq!(clips.len(), 1, "one motion");
    assert!(clips[0].frames.len() >= 4, "{} frames", clips[0].frames.len());
    // it plays back: the sliders follow the recording
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.get_by_label("Play").click();
    h.run_steps(6);
    assert!(h.state_mut().test_rig().values.0.contains_key(&ax) || clips[0].frames.iter().any(|f| f.is_empty()));
    // K in the Draw tab does nothing to the rig
    h.get_by_label("Stop").click();
    h.run_steps(2);
    h.state_mut().test_run_action_now("tab.1");
    h.run_steps(3);
    let poses = h.state().draw_mut_ref().doc.rig.poses.len();
    h.key_press(Key::K);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.rig.poses.len(), poses);
}

#[test]
fn rig_tab_joined_sliders_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, x: i32, y: i32, bw: u32, bh: u32, c: [u8; 3]| {
            let img = image::RgbaImage::from_pixel(bw, bh, image::Rgba([c[0], c[1], c[2], 255]));
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            d.doc.layers.insert(0, l);
        };
        add("hair back", 1000, 600, 1000, 1100, [90, 60, 140]);
        add("face", 1100, 800, 800, 900, [240, 205, 180]);
        add("eye L", 1550, 1200, 220, 100, [240, 240, 240]);
        add("eye R", 1230, 1200, 220, 100, [240, 240, 240]);
        add("iris L", 1620, 1210, 80, 80, [40, 70, 140]);
        add("iris R", 1300, 1210, 80, 80, [40, 70, 140]);
        add("mouth inside", 1350, 1560, 300, 70, [120, 30, 50]);
        add("mouth", 1350, 1560, 300, 70, [200, 80, 90]);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.get_by_label("This is a face").click();
    h.run_steps(6);
    let rig = h.state().draw_mut_ref().doc.rig.clone();
    for n in ["Head", "Eyeballs", "Mouth"] {
        assert!(rig.pads.iter().any(|p| p.name == n), "a joined square {n}");
    }
    assert!(rig.params.iter().any(|p| p.name == "Eyeball X") && rig.params.iter().any(|p| p.name == "Mouth form"));
    let areas = h.state_mut().test_rig().pad_areas.clone();
    assert_eq!(areas.len(), 3, "three squares are shown");
    // drag in the eyeball square to the top right crossing: both sliders move
    let area = areas.iter().find(|a| a.0 == "Eyeballs").unwrap().1;
    let at = area.right_top();
    h.hover_at(at - vec2(1.0, -1.0));
    h.step();
    button(&h, at - vec2(1.0, -1.0), true);
    h.step();
    button(&h, at - vec2(1.0, -1.0), false);
    h.run_steps(3);
    let id = |n: &str| rig.params.iter().find(|p| p.name == n).unwrap().id;
    let (bx, by) = (h.state_mut().test_rig().values.0.get(&id("Eyeball X")).copied(), h.state_mut().test_rig().values.0.get(&id("Eyeball Y")).copied());
    assert_eq!((bx, by), (Some(1.0), Some(1.0)), "landed on the corner key point: both sliders are at their ends");
    let iris = h.state().draw_mut_ref().doc.layers.iter().find(|l| l.name == "iris L").unwrap().id;
    assert!(h.state_mut().test_moved(iris) > 10.0, "the iris looks up and right");
    save(&mut h, "rig_13_pads");
    // the mouth square: smile and open
    let area = h.state_mut().test_rig().pad_areas.iter().find(|a| a.0 == "Mouth").unwrap().1;
    // the right end of the form line, with the mouth closed (the bottom of the open line)
    let corner = area.right_bottom() - vec2(1.0, 1.0);
    h.hover_at(corner);
    h.step();
    button(&h, corner, true);
    h.step();
    button(&h, corner, false);
    h.run_steps(3);
    let form = h.state_mut().test_rig().values.0.get(&id("Mouth form")).copied();
    assert_eq!(form, Some(1.0), "the right end of the mouth line is the smile");
    assert_eq!(h.state_mut().test_rig().values.0.get(&id("Mouth open")).copied(), Some(0.0), "and the mouth stays closed");
    // joining by hand and splitting
    let n = h.state().draw_mut_ref().doc.rig.pads.len();
    h.state_mut().test_rig_edit(|r| {
        let (a, b) = (r.params.iter().find(|p| p.name == "Mouth A").unwrap().id, r.params.iter().find(|p| p.name == "Mouth I").unwrap().id);
        r.pads.push(crate::rig::Pad { name: "A + I".into(), x: a, y: b });
    });
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.rig.pads.len(), n + 1);
    h.get_all_by_label("Split").last().unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.rig.pads.len(), n, "split brings the two sliders back");
}

#[test]
fn physics_tab_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2200.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, x: i32, y: i32, bw: u32, bh: u32, c: [u8; 3]| {
            let img = image::RgbaImage::from_pixel(bw, bh, image::Rgba([c[0], c[1], c[2], 255]));
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            d.doc.layers.insert(0, l);
        };
        add("hair back", 1000, 500, 1000, 1100, [90, 60, 140]);
        add("face", 1100, 700, 800, 900, [240, 205, 180]);
        add("eye L", 1550, 1100, 200, 90, [40, 70, 140]);
        add("eye R", 1250, 1100, 200, 90, [40, 70, 140]);
        add("mouth", 1350, 1450, 300, 70, [200, 80, 90]);
        add("hair front", 1050, 600, 900, 250, [110, 70, 170]);
        add("earring", 1010, 1200, 60, 200, [230, 200, 60]);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.get_by_label("This is a face").click();
    h.run_steps(5);
    h.state_mut().test_run_action_now("tab.4");
    h.run_steps(5);
    let layer = |h: &Harness<'_, SeviathApp>, n: &str| h.state().draw_mut_ref().doc.layers.iter().find(|l| l.name == n).unwrap().id;
    let (back, ring) = (layer(&h, "hair back"), layer(&h, "earring"));
    // before: nothing moves
    save(&mut h, "phys_1_before");
    h.state_mut().test_rig().selected = vec![back];
    h.run_steps(2);
    h.get_by_label("Make it move").click();
    h.run_steps(5);
    {
        let rig = &h.state().draw_mut_ref().doc.rig;
        assert_eq!(rig.physics.len(), 1);
        assert_eq!(rig.physics[0].kind, crate::rig::physics::Kind::Hair);
        let m = rig.mesh_of(back).unwrap();
        assert_eq!(m.parent, Some(rig.physics[0].changer), "the part hangs from its curve");
    }
    h.state_mut().test_rig().selected = vec![ring];
    h.state_mut().test_phys().kind = crate::rig::physics::Kind::Hanging;
    h.run_steps(2);
    h.get_by_label("Make it move").click();
    h.run_steps(5);
    assert_eq!(h.state().draw_mut_ref().doc.rig.physics.len(), 2);
    // at rest nothing is moved, even after some time
    for _ in 0..120 {
        h.state_mut().phys_advance(1.0 / 60.0);
    }
    assert!(h.state_mut().test_moved(back) < 2.0, "as drawn at rest: {}", h.state_mut().test_moved(back));
    // the head shakes: the hair follows with a delay
    let ax = h.state().draw_mut_ref().doc.rig.params.iter().find(|p| p.name == "Angle X").unwrap().id;
    h.state_mut().test_rig().values.set(ax, 28.0);
    let mut worst: f32 = 0.0;
    for _ in 0..20 {
        h.state_mut().phys_advance(1.0 / 60.0);
    }
    worst = worst.max(h.state_mut().test_moved(back));
    assert!(worst > 5.0, "the hair moves: {worst}");
    save(&mut h, "phys_2_moving");
    // wind pushes
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.state_mut().test_rig_edit(|r| r.world.wind = crate::rig::physics::Wind::Storm);
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.rig.world.wind, crate::rig::physics::Wind::Storm);
    for _ in 0..90 {
        h.state_mut().phys_advance(1.0 / 60.0);
    }
    assert!(h.state_mut().test_moved(ring) > 5.0, "the earring blows: {}", h.state_mut().test_moved(ring));
    // a test motion runs and ends
    h.get_by_label("Shake the head").click();
    h.run_steps(3);
    assert!(h.state_mut().test_phys().motion.is_some());
    for _ in 0..200 {
        h.state_mut().phys_advance(1.0 / 60.0);
    }
    assert!(h.state_mut().test_phys().motion.is_none(), "the motion ends");
    save(&mut h, "phys_3_storm");
    // physics off: the sliders alone
    h.state_mut().test_phys().on = false;
    h.run_steps(3);
    assert!(h.state_mut().test_moved(ring) < 1.0);
    // removing puts the part back
    h.state_mut().test_phys().on = true;
    h.state_mut().test_phys().sel = Some(1);
    h.run_steps(3);
    h.get_by_label("Remove").click();
    h.run_steps(3);
    let rig = h.state().draw_mut_ref().doc.rig.clone();
    assert_eq!(rig.physics.len(), 1);
    assert!(rig.mesh_of(ring).unwrap().parent.is_none(), "back to where it was");
    // undo brings it back
    h.state_mut().test_run_action_now("edit.undo");
    h.run_steps(3);
    assert_eq!(h.state().draw_mut_ref().doc.rig.physics.len(), 2);
}

#[test]
fn live2d_import_and_layer_eye_screens() {
    use crate::import::caff::tests::make;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let xml = crate::import::tests::model_xml();
    let png = crate::import::tests::texture_png();
    let data = make(0x7273423d, &[("tex.png", &png, false), ("main.xml", xml.as_bytes(), true)]);
    let imp = crate::import::cmo3::import_bytes(data).unwrap();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let layer = imp.doc.layers[0].id;
    h.state_mut().draw_mut().replace_document(imp.doc);
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(6);
    assert_eq!(h.state().draw_mut_ref().doc.rig.meshes.len(), 1);
    // the picture is drawn through the imported rig: turning the head moves the eye
    let z = h.state().draw_mut_ref().doc.rig.params.iter().find(|p| p.name == "Angle Z").unwrap().id;
    h.state_mut().test_rig().values.set(z, 30.0);
    h.run_steps(3);
    assert!(h.state_mut().test_moved(layer) > 5.0, "the imported eye turns with the head");
    save(&mut h, "import_1_live2d");
    // hide the layer with the eye: it is no longer drawn, and shows again
    h.state_mut().toggle_layer_visible_for_test(layer);
    h.run_steps(3);
    assert!(!h.state().draw_mut_ref().doc.layers.iter().find(|l| l.id == layer).unwrap().visible);
    assert_eq!(h.state_mut().test_moved(layer), 0.0, "a hidden layer is not in the picture");
    save(&mut h, "import_2_hidden");
    h.get_by_label("Show all").click();
    h.run_steps(3);
    assert!(h.state().draw_mut_ref().doc.layers.iter().find(|l| l.id == layer).unwrap().visible);
    assert!(h.state_mut().test_moved(layer) > 5.0);
}

#[test]
fn sharp_curves_and_smooth_warp_screens() {
    use crate::doc::LayerKind;
    use crate::rig::{Form, Kind};
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let layer;
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let img = image::RgbaImage::from_fn(600, 600, |x, y| if (x / 50 + y / 50) % 2 == 0 { image::Rgba([220, 60, 80, 255]) } else { image::Rgba([40, 90, 200, 255]) });
        let mut l = d.doc.new_layer("cloth", LayerKind::Raster);
        l.tiles = crate::files::tiles_from_rgba(&img, 700, 700, w, h2);
        l.rev += 1;
        layer = l.id;
        d.doc.layers.insert(0, l);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    // a mesh with a few big triangles, in a warp that is pushed hard in the middle
    h.state_mut().test_rig_edit(|r| {
        let id = r.fresh_id();
        r.meshes.push(crate::rig::ArtMesh { id, layer, verts: vec![[700.0, 700.0], [1300.0, 700.0], [1300.0, 1300.0], [700.0, 1300.0]], tris: vec![[0, 1, 2], [0, 2, 3]], parent: None });
        let w = r.add_changer("Bend", Kind::Warp { rect: [700.0, 700.0, 1300.0, 1300.0], cols: 4, rows: 4 }, None);
        r.smooth.push(w);
        let p = r.add_param("Bend", 0.0, 1.0, 0.0);
        let mut off = vec![[0.0, 0.0]; 25];
        off[12] = [0.0, 160.0];
        off[11] = [0.0, 60.0];
        off[13] = [0.0, 60.0];
        r.set_keyform(w, p, 1.0, Form::Warp { offsets: off });
        r.meshes.last_mut().unwrap().parent = Some(w);
    });
    h.run_steps(3);
    let p = h.state().draw_mut_ref().doc.rig.params.iter().find(|p| p.name == "Bend").unwrap().id;
    // at rest: two triangles. bent: many, so the picture follows the curve
    assert_eq!(h.state_mut().test_tris(layer), 2);
    h.state_mut().test_rig().values.set(p, 1.0);
    h.run_steps(3);
    let sharp = h.state_mut().test_tris(layer);
    assert!(sharp > 30, "triangles added where it bends: {sharp}");
    save(&mut h, "warp_1_sharp");
    h.state_mut().test_rig().hq = false;
    h.run_steps(3);
    assert_eq!(h.state_mut().test_tris(layer), 2, "off: only the mesh it has");
    save(&mut h, "warp_2_plain");
}

#[test]
fn new_changers_screens() {
    use crate::doc::LayerKind;
    use crate::rig::{Form, Kind};
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let (a, b);
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let mut add = |name: &str, x: i32, y: i32, bw: u32, bh: u32, c: [u8; 3]| {
            let img = image::RgbaImage::from_pixel(bw, bh, image::Rgba([c[0], c[1], c[2], 255]));
            let mut l = d.doc.new_layer(name, LayerKind::Raster);
            l.tiles = crate::files::tiles_from_rgba(&img, x, y, w, h2);
            l.rev += 1;
            let id = l.id;
            d.doc.layers.insert(0, l);
            id
        };
        a = add("cloth", 800, 800, 500, 600, [200, 60, 90]);
        b = add("eyelid", 1100, 900, 300, 120, [60, 90, 200]);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.state_mut().test_rig().pro = true;
    h.state_mut().test_rig().selected = vec![a, b];
    h.run_steps(2);
    {
        h.state_mut().test_rig_edit(|r| {
            for (l, rect) in [(a, [800.0, 800.0, 1300.0, 1400.0]), (b, [1100.0, 900.0, 1400.0, 1020.0])] {
                let id = r.fresh_id();
                r.meshes.push(crate::rig::ArtMesh { id, layer: l, verts: vec![[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]], tris: vec![[0, 1, 2], [0, 2, 3]], parent: None });
            }
        });
    }
    h.run_steps(3);
    // the buttons make the changers
    h.state_mut().test_rig().selected = vec![a];
    h.run_steps(2);
    for label in ["New Pin changer", "New Move changer", "New Scale changer", "New Flip changer", "New Fade changer", "New Order changer", "New Wave changer"] {
        h.get_by_label(label).click();
        h.run_steps(3);
    }
    {
        let rig = &h.state().draw_mut_ref().doc.rig;
        let kinds: Vec<&Kind> = rig.changers.iter().map(|c| &c.kind).collect();
        assert!(kinds.iter().any(|k| matches!(k, Kind::Pins { pts } if pts.len() == 4)));
        assert!(kinds.iter().any(|k| matches!(k, Kind::Move)) && kinds.iter().any(|k| matches!(k, Kind::Scale { .. })));
        assert!(kinds.iter().any(|k| matches!(k, Kind::Flip { .. })) && kinds.iter().any(|k| matches!(k, Kind::Fade)));
        assert!(kinds.iter().any(|k| matches!(k, Kind::Order)) && kinds.iter().any(|k| matches!(k, Kind::Wave { .. })));
    }
    // a path drawn on the picture makes a Path changer around the picked part; a second stroke adds a curve
    h.state_mut().test_rig().selected = vec![b];
    h.state_mut().test_rig().sel_changer = None;
    h.state_mut().test_stroke(vec![[1100.0, 960.0], [1250.0, 940.0], [1400.0, 960.0]]);
    h.run_steps(3);
    let pc = h.state().draw_mut_ref().doc.rig.changers.iter().find(|c| matches!(c.kind, Kind::Paths { .. })).unwrap().id;
    assert_eq!(h.state().draw_mut_ref().doc.rig.meshes.iter().find(|m| m.layer == b).unwrap().parent, Some(pc));
    h.state_mut().test_stroke(vec![[1100.0, 900.0], [1250.0, 900.0], [1400.0, 900.0]]);
    h.run_steps(3);
    match &h.state().draw_mut_ref().doc.rig.changer(pc).unwrap().kind {
        Kind::Paths { paths } => assert_eq!(paths.len(), 2, "two spots on one part"),
        _ => unreachable!(),
    }
    // closing the eyelid: a slider drags the first path down; fading hides the cloth; order moves it to the front
    let (fade, order, pins) = {
        let rig = &h.state().draw_mut_ref().doc.rig;
        let f = |pred: fn(&Kind) -> bool| rig.changers.iter().find(|c| pred(&c.kind)).unwrap().id;
        (f(|k| matches!(k, Kind::Fade)), f(|k| matches!(k, Kind::Order)), f(|k| matches!(k, Kind::Pins { .. })))
    };
    h.state_mut().test_rig_edit(|r| {
        // the cloth sits inside order, inside fade, inside the pins
        r.changers.iter_mut().find(|c| c.id == fade).unwrap().parent = Some(pins);
        r.changers.iter_mut().find(|c| c.id == order).unwrap().parent = Some(fade);
        r.meshes.iter_mut().find(|m| m.layer == a).unwrap().parent = Some(order);
        let p = r.add_param("Eye open", 0.0, 1.0, 1.0);
        let mut off = vec![vec![[0.0, 0.0]; 5], vec![[0.0, 0.0]; 5]];
        for o in off[0].iter_mut() {
            *o = [0.0, 40.0];
        }
        r.set_keyform(pc, p, 0.0, Form::Paths { offsets: off, turns: vec![] });
        r.set_keyform(fade, p, 0.0, Form::Vals { v: vec![0.3] });
        r.set_keyform(order, p, 0.0, Form::Vals { v: vec![5.0] });
        let mut pin_off = vec![[0.0, 0.0]; 4];
        pin_off[3] = [60.0, 40.0];
        r.set_keyform(pins, p, 0.0, Form::Pins { offsets: pin_off });
    });
    h.run_steps(3);
    let eye = h.state().draw_mut_ref().doc.rig.params.iter().find(|p| p.name == "Eye open").unwrap().id;
    assert!(h.state_mut().test_moved(b) < 0.5, "open: as drawn");
    assert_eq!(h.state_mut().test_stack(a).map(|s| s.1), Some(1.0));
    h.state_mut().test_rig().values.set(eye, 0.0);
    h.run_steps(3);
    assert!(h.state_mut().test_moved(b) > 15.0, "the path drags the eyelid down: {}", h.state_mut().test_moved(b));
    assert!(h.state_mut().test_moved(a) > 20.0, "the pins pull the cloth");
    let (pos_a, op_a) = h.state_mut().test_stack(a).unwrap();
    assert!((op_a - 0.3).abs() < 0.01, "faded: {op_a}");
    assert!(pos_a > h.state_mut().test_stack(b).unwrap().0, "moved forward past the eyelid");
    save(&mut h, "changers_1_new");
    // numbers of a numeric changer can be typed and saved at the slider value
    h.state_mut().test_rig().values = crate::rig::Values::default();
    h.run_steps(2);
}

#[test]
fn path_turns_and_turn_the_whole_shape_screens() {
    use crate::doc::LayerKind;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 2300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let layer;
    {
        let d = h.state_mut().draw_mut();
        let (w, h2) = (d.doc.w, d.doc.h);
        let img = image::RgbaImage::from_fn(400, 500, |x, y| if (x / 40 + y / 40) % 2 == 0 { image::Rgba([220, 60, 80, 255]) } else { image::Rgba([240, 200, 60, 255]) });
        let mut l = d.doc.new_layer("ribbon", LayerKind::Raster);
        l.tiles = crate::files::tiles_from_rgba(&img, 900, 800, w, h2);
        l.rev += 1;
        layer = l.id;
        d.doc.layers.insert(0, l);
        d.doc.mark_all_dirty();
    }
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.state_mut().test_rig().pro = true;
    h.state_mut().test_rig_edit(|r| {
        let id = r.fresh_id();
        r.meshes.push(crate::rig::ArtMesh { id, layer, verts: vec![[900.0, 800.0], [1300.0, 800.0], [1300.0, 1300.0], [900.0, 1300.0]], tris: vec![[0, 1, 2], [0, 2, 3]], parent: None });
        r.add_param("Curl", 0.0, 1.0, 0.0);
    });
    h.state_mut().test_rig().selected = vec![layer];
    h.state_mut().test_stroke(vec![[1100.0, 800.0], [1100.0, 1050.0], [1100.0, 1300.0]]);
    h.run_steps(3);
    let (pc, curl) = {
        let rig = &h.state().draw_mut_ref().doc.rig;
        (rig.changers.iter().find(|c| matches!(c.kind, crate::rig::Kind::Paths { .. })).unwrap().id, rig.params[0].id)
    };
    h.state_mut().test_rig().sel_changer = Some(pc);
    h.state_mut().test_rig().sel_param = Some(curl);
    h.state_mut().test_rig().values.set(curl, 1.0);
    h.run_steps(3);
    h.get_by_label("Edit the shape").click();
    h.run_steps(3);
    assert!(h.state_mut().test_rig().test_warp_edit().is_some());
    // turn the whole shape: the points go round
    let before = h.state_mut().test_rig().test_warp_edit().unwrap().clone();
    h.get_by_label("the whole shape").click();
    h.run_steps(3);
    let after = h.state_mut().test_rig().test_warp_edit().unwrap().clone();
    assert_ne!(before, after, "the points moved round");
    save(&mut h, "path_turn_1_edit");
    // saved: the shape has turns, and the picture shows them
    h.get_by_label("Save the shape at this value").click();
    h.run_steps(3);
    let rig = h.state().draw_mut_ref().doc.rig.clone();
    assert!(rig.bindings.iter().any(|b| b.forms.iter().any(|f| matches!(f, crate::rig::Form::Paths { turns, .. } if !turns.is_empty() && turns[0].iter().any(|t| t.abs() > 1.0)))), "turns saved with the shape");
    assert!(h.state_mut().test_moved(layer) > 20.0);
}

#[test]
fn rig_for_games_screens() {
    use crate::vrm::export::Format;
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let doc = crate::vrm::tests_build::character();
    let mut h = Harness::builder().with_size([1440.0, 2300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.state_mut().test_game().author = "Me".into();
    h.get_by_label("Make the 3D character").click();
    h.run_steps(4);
    assert!(h.state_mut().test_game().model.is_some());
    assert!(h.state_mut().test_game().message.as_deref().unwrap().contains("expressions"));
    // turn it round and show the blink in the preview
    h.state_mut().test_game().yaw = 30.0;
    h.state_mut().test_game().expr = Some("blink".into());
    h.run_steps(4);
    save(&mut h, "game_1_preview");
    // both formats come out checked
    let (one, s1) = h.state_mut().game_bytes(Format::Vrm1).unwrap();
    let (zero, s0) = h.state_mut().game_bytes(Format::Vrm0).unwrap();
    assert_eq!((s1.version, s0.version), (1, 0));
    assert!(one.len() > 1000 && zero.len() > 1000);
    // with no meshes there is nothing to build
    h.state_mut().test_game().model = None;
    h.state_mut().test_rig_edit(|r| r.meshes.clear());
    h.get_by_label("Make the 3D character").click();
    h.run_steps(3);
    assert!(h.state_mut().test_game().model.is_none());
    assert!(h.state_mut().test_game().message.as_deref().unwrap().contains("Nothing to build"));
}

#[test]
fn uncut_face_points_screens() {
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let img = crate::rig::uncut::tests::figure_image();
    let mut doc = crate::vrm::tests_build::uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = doc.layers.iter().map(|l| l.id).collect();
    let mut rig = crate::rig::Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &doc, &ids, 0.6).unwrap();
    doc.rig = std::sync::Arc::new(rig);
    let mut h = Harness::builder().with_size([1440.0, 2300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.get_by_label("Make the 3D character").click();
    h.run_steps(4);
    assert!(h.state_mut().test_game().model.is_some());
    let msg = h.state_mut().test_game().message.clone().unwrap();
    assert!(msg.contains("guessed"), "{msg}");
    save(&mut h, "game_2_uncut_face_points");
    // the face points can be set by clicking: the instruction shows on the picture
    h.state_mut().test_face_click(1);
    h.run_steps(3);
    save(&mut h, "game_3_click_the_face");
    // the AI finder is there, and without Python packages it says so plainly instead of failing
    h.state_mut().test_game().ai_python = "definitely-not-python-xyz".into();
    h.get_by_label("Find the face with AI").click();
    h.run_steps(30);
    std::thread::sleep(std::time::Duration::from_millis(500));
    h.run_steps(10);
    let msg = h.state_mut().test_game().message.clone().unwrap();
    assert!(msg.contains("Install Python"), "{msg}");
}

#[test]
fn uncut_joints_screens() {
    let _gpu = lock_gpu();
    let img = crate::rig::uncut::tests::figure_image();
    let mut doc = crate::vrm::tests_build::uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = doc.layers.iter().map(|l| l.id).collect();
    let mut rig = crate::rig::Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &doc, &ids, 0.6).unwrap();
    doc.rig = std::sync::Arc::new(rig);
    let mut h = Harness::builder().with_size([1440.0, 1900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    h.state_mut().test_run_action_now("tab.3");
    h.state_mut().test_joint_edit(true);
    h.run_steps(5);
    save(&mut h, "rig_joints");
}

/// The hair editor and the texture parts on the real Midlander (when the AI files are on this computer).
#[test]
fn hair_and_texture_parts_screens() {
    use egui_kittest::kittest::Queryable;
    let dir = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("SeviathAI/jobs/midlander");
    let (Ok(png), Ok(glb)) = (std::fs::read(dir.join("original.png")), std::fs::read(dir.join("shape.glb"))) else { return };
    let _gpu = lock_gpu();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    let mut doc = crate::vrm::tests_build::uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = doc.layers.iter().map(|l| l.id).collect();
    let mut rig = crate::rig::Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &doc, &ids, 0.5).unwrap();
    doc.rig = std::sync::Arc::new(rig);
    let mut h = Harness::builder().with_size([1440.0, 2600.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    h.state_mut().test_game().shape = Some(std::sync::Arc::new(crate::vrm::shape::Shape::from_glb(&glb).unwrap()));
    h.state_mut().test_game_make();
    h.run_steps(6);
    let msg = h.state_mut().test_game().message.clone();
    assert!(h.state_mut().test_game().model.is_some(), "{msg:?}");
    h.state_mut().test_game().guides = true;
    h.state_mut().test_hair_edit(true);
    h.run_steps(6);
    save(&mut h, "game_4_hair_and_texture_parts");
    let strands = h.state().draw_mut_ref().doc.rig.skin.as_ref().unwrap().hair.chains.len();
    assert!(strands >= 6, "the strands of the app are the start: {strands}");
}

/// With the AI files missing the panel says so and offers the download; the AI buttons wait.
#[test]
fn missing_ai_files_show_the_download_button_screens() {
    use egui_kittest::kittest::Queryable;
    let _gpu = lock_gpu();
    let empty = std::env::temp_dir().join(format!("seviath_noai_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&empty);
    crate::aisetup::TEST_ROOT.with(|r| *r.borrow_mut() = Some(empty.clone()));
    let img = crate::rig::uncut::tests::figure_image();
    let mut doc = crate::vrm::tests_build::uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = doc.layers.iter().map(|l| l.id).collect();
    let mut rig = crate::rig::Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &doc, &ids, 0.6).unwrap();
    doc.rig = std::sync::Arc::new(rig);
    let mut h = Harness::builder().with_size([1440.0, 2300.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    h.state_mut().test_run_action_now("tab.3");
    h.run_steps(5);
    // the card is there and the download button waits for the licences
    h.get_by_label("The AI files are missing");
    h.get_by_label("Download the AI files");
    assert!(!h.state_mut().test_game().setup_agree);
    save(&mut h, "game_5_ai_files_missing");
    crate::aisetup::TEST_ROOT.with(|r| *r.borrow_mut() = None);
    let _ = std::fs::remove_dir_all(&empty);
}

/// The Live2D files section of the Physics tab, and the swinging of a Live2D model moving sliders.
#[test]
fn live2d_runtime_physics_screens() {
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1400.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    let phys = br#"{"Version":3,"Meta":{"Fps":60,"EffectiveForces":{"Gravity":{"X":0,"Y":-1},"Wind":{"X":0,"Y":0}},"PhysicsDictionary":[{"Id":"PhysicsSetting1","Name":"Hair"}]},"PhysicsSettings":[{"Id":"PhysicsSetting1","Input":[{"Source":{"Target":"Parameter","Id":"ParamAngleX"},"Weight":100,"Type":"X","Reflect":false}],"Output":[{"Destination":{"Target":"Parameter","Id":"ParamHair"},"VertexIndex":2,"Scale":20,"Weight":100,"Type":"Angle","Reflect":false}],"Vertices":[{"Position":{"X":0,"Y":0},"Mobility":1,"Delay":0.9,"Acceleration":1,"Radius":0},{"Position":{"X":0,"Y":10},"Mobility":0.95,"Delay":0.9,"Acceleration":1,"Radius":10},{"Position":{"X":0,"Y":20},"Mobility":0.95,"Delay":0.9,"Acceleration":1,"Radius":10},{"Position":{"X":0,"Y":30},"Mobility":0.95,"Delay":0.9,"Acceleration":1,"Radius":10}],"Normalization":{"Position":{"Minimum":-10,"Default":0,"Maximum":10},"Angle":{"Minimum":-10,"Default":0,"Maximum":10}}}]}"#;
    let (mut x, mut hair) = (0, 0);
    h.state_mut().test_rig_edit(|r| {
        x = r.add_param("Angle X", -30.0, 30.0, 0.0);
        hair = r.add_param("Hair", -1.0, 1.0, 0.0);
        r.params.iter_mut().find(|p| p.id == x).unwrap().cid = "ParamAngleX".into();
        r.params.iter_mut().find(|p| p.id == hair).unwrap().cid = "ParamHair".into();
        r.param_physics = crate::import::runtime::parse_physics3(phys).unwrap();
        r.expressions = vec![crate::import::runtime::parse_exp3("tilt", br#"{"Parameters":[{"Id":"ParamAngleX","Value":10,"Blend":"Add"}]}"#).unwrap()];
    });
    h.state_mut().test_run_action_now("tab.4");
    h.run_steps(4);
    save(&mut h, "physics_live2d_files");
    // the head turns: the hair swings and gives values to its slider; the values the user set are not changed
    h.state_mut().test_rig().values.set(x, 25.0);
    let mut peak = 0.0f32;
    for _ in 0..40 {
        h.state_mut().phys_advance(1.0 / 60.0);
        peak = peak.max(h.state_mut().test_phys().param_out.get(&hair).copied().unwrap_or(0.0).abs());
    }
    assert!(peak > 0.02, "the hair slider was driven: {peak}");
    assert_eq!(h.state_mut().test_rig().values.0.get(&hair), None, "what the user set is untouched");
    assert!(h.state_mut().test_rig().effective_values().0.contains_key(&hair), "what is shown has the swing");
    // switched off: no swing
    h.state_mut().test_rig_edit(|r| r.param_physics.on = false);
    h.state_mut().phys_advance(1.0 / 60.0);
    assert!(h.state_mut().test_phys().param_out.is_empty());
}

/// The Animate tab: tracks with keys move the sliders, the playhead applies them, and the animation is saved as pictures.
#[test]
fn animate_tab_screens() {
    use crate::anim::{Ease, Target};
    let _gpu = lock_gpu();
    let doc = crate::vrm::tests_build::character();
    let mut h = Harness::builder().with_size([1440.0, 1000.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    // the sliders of the character: the first two that move something
    let params: Vec<(u32, String, f32, f32)> = h.state().draw_mut_ref().doc.rig.params.iter().map(|p| (p.id, p.name.clone(), p.min, p.max)).collect();
    assert!(params.len() >= 2, "the test character has sliders: {}", params.len());
    let mut layer = 0;
    h.state_mut().test_rig_edit(|r| {
        r.anim.length = 24;
        r.anim.fps = 12.0;
        r.anim.set_marker(0, "Idle");
        r.anim.set_marker(12, "Walk");
        for (id, name, lo, hi) in params.iter().take(2) {
            let t = r.anim.track_for(Target::Param(*id), name);
            r.anim.tracks[t].set_key(0.0, *lo, Ease::InOut);
            r.anim.tracks[t].set_key(12.0, *hi, Ease::Linear);
            r.anim.tracks[t].set_key(23.0, (*lo + *hi) / 2.0, Ease::Linear);
        }
        layer = r.meshes.first().map(|m| m.layer).unwrap_or(0);
    });
    let _ = layer;
    h.state_mut().test_run_action_now("tab.5");
    h.run_steps(5);
    // at frame 0 and at frame 12 the character looks different
    h.state_mut().test_anim().frame = 0.0;
    h.run_steps(3);
    let a = h.state_mut().test_still(300).expect("a picture");
    h.state_mut().test_anim().frame = 12.0;
    h.run_steps(3);
    let b = h.state_mut().test_still(300).expect("a picture");
    assert_ne!(a.as_raw(), b.as_raw(), "the sliders were moved by the tracks");
    save(&mut h, "animate_1_timeline");
    // playing moves the playhead and stops at the end when it does not loop
    h.state_mut().test_rig_edit(|r| r.anim.looping = false);
    h.state_mut().test_anim().frame = 20.0;
    h.state_mut().test_anim().playing = true;
    for _ in 0..40 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(!h.state_mut().test_anim().playing && h.state_mut().test_anim().frame >= 22.9, "played to the end: {}", h.state_mut().test_anim().frame);
    // saved as pictures: one for every frame, and the first and the middle differ
    let dir = std::env::temp_dir().join(format!("seviath_anim_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    h.state_mut().test_anim_export_png(dir.clone(), 240);
    for _ in 0..200 {
        h.run_steps(1);
        if !h.state_mut().test_anim_busy() {
            break;
        }
    }
    assert!(!h.state_mut().test_anim_busy());
    let count = std::fs::read_dir(&dir).unwrap().flatten().count();
    assert_eq!(count, 24, "{:?}", h.state_mut().test_anim().message);
    let f0 = image::open(dir.join("frame_0001.png")).unwrap().to_rgba8();
    let f12 = image::open(dir.join("frame_0013.png")).unwrap().to_rgba8();
    assert_ne!(f0.as_raw(), f12.as_raw());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Keying by keyboard, auto key from a slider, deleting, and undo of timeline edits.
#[test]
fn animate_tab_keys_autokey_and_undo() {
    use crate::anim::Target;
    let _gpu = lock_gpu();
    let doc = crate::vrm::tests_build::character();
    let mut h = Harness::builder().with_size([1440.0, 1000.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    let (id, name) = { let p = h.state().draw_mut_ref().doc.rig.params[0].clone(); (p.id, p.name) };
    h.state_mut().test_rig_edit(|r| {
        r.anim.track_for(Target::Param(id), &name);
    });
    h.state_mut().test_run_action_now("tab.5");
    h.run_steps(4);
    // K puts a key on the picked track at the playhead, with the value of the slider
    h.state_mut().test_anim().sel_track = Some(0);
    h.state_mut().test_anim().frame = 6.0;
    h.state_mut().test_rig().values.set(id, 5.0);
    h.run_steps(2);
    h.key_press(eframe::egui::Key::K);
    h.run_steps(3);
    let keys = |h: &mut Harness<'_, SeviathApp>| h.state().draw_mut_ref().doc.rig.anim.tracks[0].keys.clone();
    let k = keys(&mut h);
    assert_eq!(k.len(), 1, "{k:?}");
    assert_eq!((k[0].frame, k[0].value), (6.0, 5.0));
    // that was one undo step: undo takes it away
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::Z);
    h.run_steps(3);
    assert!(keys(&mut h).is_empty(), "undone");
    // Delete takes the picked key away
    h.state_mut().test_anim().frame = 8.0;
    h.state_mut().test_rig().values.set(id, 2.0);
    h.state_mut().test_rig_edit(|r| { r.anim.tracks[0].set_key(8.0, 2.0, crate::anim::Ease::Linear); });
    h.state_mut().test_anim().sel = Some((0, 0));
    h.run_steps(2);
    h.key_press(eframe::egui::Key::Delete);
    h.run_steps(3);
    assert!(keys(&mut h).is_empty());
    // space plays and pauses
    h.key_press(eframe::egui::Key::Space);
    h.run_steps(2);
    assert!(h.state_mut().test_anim().playing);
    h.key_press(eframe::egui::Key::Space);
    h.run_steps(2);
    assert!(!h.state_mut().test_anim().playing);
}

/// The animation saved as a GIF and as an MP4 video.
#[test]
fn animation_is_saved_as_gif_and_mp4() {
    use crate::anim::{Ease, Target};
    use image::AnimationDecoder;
    let _gpu = lock_gpu();
    let doc = crate::vrm::tests_build::character();
    let mut h = Harness::builder().with_size([1200, 800].map(|v| v as f32)).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    let (id, name, lo, hi) = { let p = h.state().draw_mut_ref().doc.rig.params[0].clone(); (p.id, p.name, p.min, p.max) };
    h.state_mut().test_rig_edit(|r| {
        r.anim.length = 10;
        r.anim.fps = 10.0;
        let t = r.anim.track_for(Target::Param(id), &name);
        r.anim.tracks[t].set_key(0.0, lo, Ease::Linear);
        r.anim.tracks[t].set_key(9.0, hi, Ease::Linear);
    });
    h.state_mut().test_run_action_now("tab.5");
    h.run_steps(4);
    let dir = std::env::temp_dir().join(format!("seviath_anim_vid_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wait = |h: &mut Harness<'_, SeviathApp>| {
        for _ in 0..300 {
            h.run_steps(1);
            if !h.state_mut().test_anim_busy() {
                break;
            }
        }
        assert!(!h.state_mut().test_anim_busy());
    };
    let gif = dir.join("a.gif");
    h.state_mut().test_anim_export_file(1, gif.clone(), 200);
    wait(&mut h);
    let frames = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(std::fs::File::open(&gif).unwrap())).unwrap().into_frames().collect_frames().unwrap();
    assert_eq!(frames.len(), 10, "{:?}", h.state_mut().test_anim().message);
    assert_ne!(frames[0].buffer().as_raw(), frames[9].buffer().as_raw());
    let mp4 = dir.join("a.mp4");
    h.state_mut().test_anim_export_file(2, mp4.clone(), 200);
    wait(&mut h);
    let bytes = std::fs::read(&mp4).unwrap();
    assert!(bytes.len() > 2000 && &bytes[4..8] == b"ftyp", "{} bytes {:?}", bytes.len(), h.state_mut().test_anim().message);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Stream tab: a face sent over the network moves a slider, the browser source serves the picture, and the screen is saved.
#[test]
fn stream_tab_follows_a_sent_face_and_serves_the_picture() {
    use crate::track::net::{Arg, tests::{osc_bundle, osc_message}};
    use std::io::{Read, Write};
    let _gpu = lock_gpu();
    let doc = crate::vrm::tests_build::character();
    let mut h = Harness::builder().with_size([1440.0, 1000.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    let (id, lo, hi) = { let p = h.state().draw_mut_ref().doc.rig.params[0].clone(); (p.id, p.min, p.max) };
    h.state_mut().test_rig_edit(|r| {
        r.tracking.maps.push(crate::track::Map { param: id, input: "MouthOpen".into(), in_lo: 0.0, in_hi: 1.0, out_lo: lo, out_hi: hi, clamp: true, smoothing: 0.0, invert: false });
        r.tracking.auto_blink = false;
    });
    h.state_mut().test_run_action_now("tab.6");
    h.run_steps(3);
    h.state_mut().test_stream().sources[1].port = 48_555;
    h.state_mut().test_stream_source(1, true);
    assert!(h.state_mut().test_stream().sources[1].error.is_none(), "{:?}", h.state_mut().test_stream().sources[1].error);
    let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let pkt = osc_bundle(&[osc_message("/VMC/Ext/Blend/Val", &[Arg::Str("jawOpen".into()), Arg::Float(1.0)])]);
    for _ in 0..40 {
        sock.send_to(&pkt, "127.0.0.1:48555").unwrap();
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(h.state_mut().test_stream().tracked, "the face is heard");
    let v = h.state_mut().test_rig().values.0.get(&id).copied().unwrap_or(f32::NAN);
    assert!((v - hi).abs() < (hi - lo) * 0.1, "the open mouth moved the slider to its top: {v} of {lo}..{hi}");
    // the browser source serves the newest picture
    h.state_mut().test_stream().server = crate::outputs::BrowserSource::start(48_734).ok();
    if h.state_mut().test_stream().server.is_some() {
        for _ in 0..8 {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let mut s = std::net::TcpStream::connect(("127.0.0.1", 48_734)).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        s.write_all(b"GET /frame.png HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut all = Vec::new();
        let _ = s.read_to_end(&mut all);
        assert!(all.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(&all[..all.len().min(80)]));
        assert!(all.windows(4).any(|w| w == b"\x89PNG"));
    }
    // a pad button switches an expression on and off, on top of the tracked face
    let (e_id, e_lo, e_hi, e_def) = { let p = h.state().draw_mut_ref().doc.rig.params[1].clone(); (p.id, p.min, p.max, p.default) };
    h.state_mut().test_rig_edit(|r| {
        r.own_exprs.push(crate::expressions::OwnExpr { name: "Happy".into(), values: vec![(e_id, e_hi)], fade_in: 0.0, fade_out: 0.0, ..Default::default() });
        r.hotkeys.push(crate::hotkeys::Binding { input: crate::hotkeys::Input::Pad { button: "South".into() }, action: crate::hotkeys::Action::Toggle("Happy".into()) });
    });
    let press = crate::hotkeys::InputEvent { input: crate::hotkeys::Input::Pad { button: "South".into() }, pressed: true };
    let release = crate::hotkeys::InputEvent { input: crate::hotkeys::Input::Pad { button: "South".into() }, pressed: false };
    let val = |h: &mut Harness<'_, SeviathApp>| h.state_mut().test_rig().values.0.get(&e_id).copied().unwrap_or(e_def);
    h.state_mut().test_stream().hub.as_ref().unwrap().push(press.clone());
    h.state_mut().test_stream().hub.as_ref().unwrap().push(release.clone());
    h.run_steps(4);
    assert!((val(&mut h) - e_hi).abs() < 1e-3, "the expression is on: {} of {e_lo}..{e_hi}", val(&mut h));
    assert!(h.state_mut().test_stream().tracked, "the face is still followed");
    h.state_mut().test_stream().hub.as_ref().unwrap().push(press);
    h.state_mut().test_stream().hub.as_ref().unwrap().push(release);
    h.run_steps(4);
    assert!((val(&mut h) - e_def).abs() < 1e-3, "off again: {}", val(&mut h));
    h.state_mut().test_stream().hub.as_ref().unwrap().push(crate::hotkeys::InputEvent { input: crate::hotkeys::Input::Pad { button: "South".into() }, pressed: true });
    h.run_steps(3);
    // an expression that hides a layer (a prop or accessory): the picture changes while it is on and comes back when it is off
    let hidden_layer = h.state().draw_mut_ref().doc.rig.meshes.last().unwrap().layer;
    h.state_mut().test_rig_edit(|r| r.own_exprs.push(crate::expressions::OwnExpr { name: "NoLayer".into(), fade_in: 0.0, fade_out: 0.0, hide: vec![hidden_layer], ..Default::default() }));
    h.run_steps(3);
    let with_layer = h.state_mut().test_still(300).unwrap();
    h.state_mut().test_stream().expr.set("NoLayer", true);
    h.run_steps(4);
    let without = h.state_mut().test_still(300).unwrap();
    assert_ne!(with_layer.as_raw(), without.as_raw(), "the layer is hidden by the expression");
    h.state_mut().test_stream().expr.set("NoLayer", false);
    h.run_steps(4);
    assert_eq!(h.state_mut().test_still(300).unwrap().as_raw(), with_layer.as_raw(), "and shown again");
    // emotes: the plain face and the expression, at three sizes each, cut to the face
    let dir = std::env::temp_dir().join(format!("seviath_emotes_{}", std::process::id()));
    let n = h.state_mut().test_stream_emotes(&dir).unwrap();
    assert_eq!(n, 9, "three looks (plain, Happy, NoLayer) at three sizes");
    let neutral = image::open(dir.join("Neutral_112.png")).unwrap().to_rgba8();
    let happy = image::open(dir.join("Happy_112.png")).unwrap().to_rgba8();
    assert_eq!(neutral.dimensions(), (112, 112));
    assert!(neutral.pixels().any(|p| p.0[3] > 200), "the face is drawn");
    assert!(neutral.pixels().any(|p| p.0[3] == 0), "the background is see-through");
    assert_ne!(neutral.as_raw(), happy.as_raw(), "the expression changes the picture");
    assert_eq!(image::open(dir.join("Happy_28.png")).unwrap().to_rgba8().dimensions(), (28, 28));
    assert!((h.state_mut().test_rig().values.0.get(&e_id).copied().unwrap_or(e_def) - e_hi).abs() < 1e-3, "the sliders are as they were");
    let _ = std::fs::remove_dir_all(&dir);
    save(&mut h, "stream_2_expressions");
    save(&mut h, "stream_1");
    // switched off again
    h.state_mut().test_stream_source(1, false);
    h.run_steps(2);
    assert!(h.state_mut().test_stream().sources[1].rx.is_none());
}

/// A sound on the timeline: it is read, drawn above the tracks, and the mouth is made from how loud it is.
#[test]
fn animate_tab_sound_waveform_and_mouth() {
    let _gpu = lock_gpu();
    let doc = crate::vrm::tests_build::character();
    let mut h = Harness::builder().with_size([1440.0, 1000.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(doc);
    let path = std::env::temp_dir().join(format!("seviath_sound_{}.wav", std::process::id()));
    std::fs::write(&path, crate::audio::tests::wav(2.0, 8000)).unwrap();
    let (id, lo, hi) = {
        let p = h.state().draw_mut_ref().doc.rig.params.iter().find(|p| p.name == "Mouth open").cloned().expect("a mouth slider");
        (p.id, p.min, p.max)
    };
    let p2 = path.to_string_lossy().to_string();
    h.state_mut().test_rig_edit(|r| {
        r.anim.length = 48;
        r.anim.fps = 24.0;
        r.anim.audio = Some(crate::anim::AudioRef { path: p2, offset: 0.0 });
    });
    h.state_mut().test_run_action_now("tab.5");
    h.run_steps(6);
    assert!(h.state_mut().test_anim().audio.is_some(), "the sound was read: {:?}", h.state_mut().test_anim().message);
    // the mouth from the sound: open in the loud first second (24 frames), shut in the quiet second
    let au = h.state_mut().test_anim().audio.clone().unwrap().1;
    let curve = au.mouth_open(24.0, 0.0, 48);
    h.state_mut().test_rig_edit(|r| {
        r.anim.set_from_curve(id, "Mouth open", &curve, lo, hi);
    });
    let a = h.state().draw_mut_ref().doc.rig.anim.clone();
    let t = &a.tracks[a.track(crate::anim::Target::Param(id)).unwrap()];
    assert!(t.value_at(10.0).unwrap() > hi * 0.7, "open while loud: {:?}", t.value_at(10.0));
    assert!(t.value_at(40.0).unwrap() < lo + (hi - lo) * 0.1, "shut while quiet");
    assert!(t.keys.len() < 40, "only the keys that matter: {}", t.keys.len());
    h.run_steps(4);
    save(&mut h, "animate_2_sound");
    // playing starts the sound player (silent when there is no sound card) and stopping cuts it
    h.state_mut().test_anim().playing = true;
    for _ in 0..5 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(h.state_mut().test_anim().test_audio_playing());
    h.state_mut().test_anim().playing = false;
    h.run_steps(3);
    assert!(!h.state_mut().test_anim().test_audio_playing());
    let _ = std::fs::remove_file(&path);
}

/// Frame by frame: a cel track with two drawings, the drawings show at their frames, the preview and the saved pictures follow.
#[test]
fn animate_tab_cels_show_by_frame_and_save() {
    use crate::doc::Document;
    let _gpu = lock_gpu();
    let mut h = Harness::builder().with_size([1440.0, 1000.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc));
    h.run_steps(4);
    h.state_mut().draw_mut().replace_document(Document::new(400, 300));
    h.state_mut().test_run_action_now("tab.5");
    h.run_steps(3);
    h.state_mut().test_rig_edit(|r| {
        r.anim.length = 6;
        r.anim.fps = 6.0;
    });
    // a red square on the first drawing, a blue one on the second (from frame 3)
    let square = |col: [u8; 4], x: i32, y: i32| {
        let mut img = image::RgbaImage::new(60, 60);
        for p in img.pixels_mut() {
            *p = image::Rgba(col);
        }
        crate::files::tiles_from_rgba(&img, x, y, 400, 300)
    };
    h.state_mut().test_cel_new_track();
    let cel1 = h.state().draw_mut_ref().doc.rig.anim.cel_tracks[0].cels[0];
    h.state_mut().test_anim().frame = 3.0;
    h.state_mut().test_cel_new_cel(0);
    let cel2 = h.state().draw_mut_ref().doc.rig.anim.cel_tracks[0].cels[1];
    for (id, tiles) in [(cel1, square([220, 30, 30, 255], 50, 50)), (cel2, square([30, 30, 220, 255], 250, 150))] {
        let d = &mut h.state_mut().draw_mut().doc;
        let l = d.layers.iter_mut().find(|l| l.id == id).unwrap();
        l.tiles = tiles;
        l.rev += 1;
    }
    let a = h.state().draw_mut_ref().doc.rig.anim.clone();
    assert_eq!((a.cel_tracks[0].shown_at(0), a.cel_tracks[0].shown_at(2), a.cel_tracks[0].shown_at(3), a.cel_tracks[0].shown_at(5)), (Some(cel1), Some(cel1), Some(cel2), Some(cel2)));
    // the layers follow the playhead
    let vis = |h: &mut Harness<'_, SeviathApp>, id: u64| h.state().draw_mut_ref().doc.layers.iter().find(|l| l.id == id).unwrap().visible;
    h.state_mut().test_anim().frame = 1.0;
    h.run_steps(3);
    assert!(vis(&mut h, cel1) && !vis(&mut h, cel2), "the first drawing shows at frame 1");
    save(&mut h, "animate_3_cels");
    h.state_mut().test_anim().frame = 4.0;
    h.run_steps(3);
    assert!(!vis(&mut h, cel1) && vis(&mut h, cel2), "the second shows at frame 4");
    // in the Draw tab the frame before shows faintly (the red drawing), and the held blue drawings are not put again
    h.state_mut().test_run_action_now("tab.1");
    h.run_steps(4);
    assert_eq!(h.state().draw_mut_ref().onion.len(), 1, "one ghost: the red drawing of frame 3");
    save(&mut h, "animate_4_onion_in_draw");
    h.state_mut().test_run_action_now("tab.5");
    h.run_steps(3);
    // the pictures put together: red at frame 1, blue at frame 4, see-through around
    let ctx = h.ctx.clone();
    let red = h.state_mut().test_cel_preview_picture(&ctx, 1).unwrap();
    let blue = h.state_mut().test_cel_preview_picture(&ctx, 4).unwrap();
    assert_ne!(red.id(), blue.id(), "two different pictures");
    // saved as pictures: six frames, red in the first three and blue in the last three, on white paper
    let dir = std::env::temp_dir().join(format!("seviath_cels_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    h.state_mut().test_anim_export_png(dir.clone(), 400);
    for _ in 0..200 {
        h.run_steps(1);
        if !h.state_mut().test_anim_busy() {
            break;
        }
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().flatten().count(), 6, "{:?}", h.state_mut().test_anim().message);
    let f1 = image::open(dir.join("frame_0002.png")).unwrap().to_rgba8();
    let f5 = image::open(dir.join("frame_0005.png")).unwrap().to_rgba8();
    assert_eq!(f1.get_pixel(80, 80).0, [220, 30, 30, 255], "the red square at frame 2");
    assert_eq!(f1.get_pixel(280, 180).0, [255, 255, 255, 255], "the blue one is not there yet: white paper");
    assert_eq!(f5.get_pixel(280, 180).0, [30, 30, 220, 255], "the blue square at frame 5");
    assert_eq!(f5.get_pixel(80, 80).0, [255, 255, 255, 255]);
    let _ = std::fs::remove_dir_all(&dir);
}
