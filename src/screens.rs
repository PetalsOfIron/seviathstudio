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
    project::save(sample.w, sample.h, sample.paper, &sample.layers, &[], &dir.join("Hair study.seviath")).unwrap();
    let mut big = crate::doc::Document::new(400, 600);
    big.paper = Color32::from_rgb(40, 60, 50);
    project::save(big.w, big.h, big.paper, &big.layers, &[], &dir.join("Night sky.seviath")).unwrap();

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
