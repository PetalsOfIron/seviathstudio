# SeviathStudio

Open-source app to **draw, rig, animate and stream a 2D character from one file**, with reliable PSD export — made for VTuber artists, riggers and VTubers. Native Rust, built with [egui](https://github.com/emilk/egui).

This is the Rust version. It is being built tab by tab: **Draw** works today (brush engine with 103 brushes and pen pressure, layers with blend modes, clipping and folders, undo history, selections, quick mask, move & transform, paint bucket with gap closing, live symmetry, layer filters and batch edits, print size (DPI), guides, project templates, gradients and a color randomizer, vector layers (pen, eraser, node editing, pressure graph, auto-trace), filters and color adjustments, check views, projects with autosave, PSD / OpenRaster / Krita import and PSD / ORA / PNG / JPEG / WebP export); Parts, Rig, Physics, Animate and Stream follow.

## Build and run

1. Install Rust: <https://rustup.rs> (Windows also needs the Visual Studio C++ Build Tools).
2. In this folder:

```
cargo run --release
```

The app is then at `target/release/seviathstudio(.exe)`.

**Pen tablets:** pressure comes through Windows Ink. If pressure doesn't work, turn on "Windows Ink" in your tablet driver (Huion, Wacom, XP-Pen…) and check *Settings → Pen & tablet*, which has a live pressure meter and a test area.

## Checks

```
cargo test --release
```

Runs the unit tests and a full UI run that paints, undoes and redoes with simulated input. Pictures land in `target/screens/`:

- `01_draw.png` … `10_settings_canvas.png` — the app at each step of the UI run
- `designer_sheet.png` — the new brushes and rolled ones (`cargo test --release designer_sheet`)
- `wet_sheet.png` — wet watercolor, ink and lift / scrape strokes (`cargo test --release wet_sheet`)
- `mix_01_chart.png` — the mixing chart (`cargo test --release mixing_chart_screens`)
- `light_01_from_the_left.png` … `light_04_light_panel.png` — light and depth layers (`cargo test --release light_and_depth_screens`)
- `cp_01_plain.png` … `cp_03_display_p3.png` — color profile and print proof (`cargo test --release color_profile_screens`)
- `fx_01_effects_on_canvas.png` … `fx_04_adjustment_panel.png` — effects and adjustment layers (`cargo test --release effects_screens effects_panel_screens`)
- `flt_shape_1_cubes.png` … `flt_shape_5_illustrate.png` — shape filters (`cargo test --release shape_filter_screens`)
- `flt_01_blur_preview.png` … `flt_04_check_colorblind.png` — filters and check views (`cargo test --release filter_screens`)
- `trace_01_lines_preview.png` … `trace_03_result.png` — Auto-trace (`cargo test --release trace_screens`)
- `vec_01_lines.png` … `vec_04_after_delete.png` — vector layers (`cargo test --release vector_screens`)
- `grad_00_options.png` … `grad_05_brush.png`, `rand_01_window.png` … `rand_03_recolor.png` — gradients and the color randomizer (`cargo test --release gradient_screens randomizer_screens`)
- `np_01_dialog.png` … `np_05_manager_search.png` — New project, Project info and the manager (`cargo test --release newproject_screens`)
- `org_01_picked.png`, `org_02_filters.png`, `org_03_quick_bar.png` — layer organizer (`cargo test --release organizer_screens`)
- `sel_01_rectangle_clip.png` … `sel_05_wand.png` — selection, transform, quick mask, symmetry and wand (`cargo test --release selection_screens`)
- `projects_manager.png`, `new_project.png`, `unsaved_changes.png` — the file windows (from `cargo test --release files_screens`)
- `brush_sheet.png` — every brush painted on one image (`cargo test --release brush_sheet`)
- `texture_sheet.png` — every texture in its natural colors (`cargo test --release texture_sheet`)

Performance check: `cargo test --release bench_full_redraw -- --ignored --nocapture`

## Code map

- `src/app/` — shell: top bar, workspaces, tabs, status bar, history and settings windows; `app/files.rs` is the File menu, saving/opening in the background, autosave, the New project dialog and the project manager
- `src/files/` — file formats: `project.rs` (`.seviath`, a ZIP of layer tiles + JSON), `psd.rs`, `ora.rs`, `kra.rs`, flat picture export
- `src/settings.rs`, `src/settings_ui.rs` — settings (saved as JSON in your user config folder) and their pages
- `src/history.rs` — undo: copy-on-write snapshots, undo per tab or everything, undo a single step from the middle
- `src/tabs/` — one module per tab (`draw.rs` + `draw/layers.rs` + `draw/brushes.rs` is the Draw tab)
- `src/doc.rs` — document, layers (folders, clipping, labels), tiles and compositing
- `src/blend.rs` — the 17 blend modes (W3C compositing math)
- `src/view.rs`, `src/canvas.rs` — zoom/pan/rotate/mirror and GPU tile display
- `src/brush/` — brush engine: settings + presets, tips, textures, stroke engine (stabilizer, pressure, taper, mixing, whole-stroke edges)
- `src/stroke.rs` — the live stroke buffer shown on top of its layer until the pen lifts
- `src/brush/random.rs` — the brush randomizer (groups, locks)
- `src/wet.rs` — wet paint: bleed, edge pooling, the wet memory
- `src/pigment.rs` + `tabs/draw/mixchart_ui.rs` — pigment (Kubelka-Munk) mixing, and the mixing chart
- `src/light.rs` — depth layers and light layers (the shading)
- `src/colorprofiles.rs` — RGB profiles (display, ICC files, baking) and the print press models
- `src/effects.rs` + `tabs/draw/effects_ui.rs` — layer effects (shadow, glow, outline, overlays) and adjustment layers
- `src/filters.rs`, `src/check.rs` + `tabs/draw/filters_ui.rs` — filters (blurs, stylize, line art, levels, curves, HSL…) with live preview, and the check views
- `src/trace.rs` + `tabs/draw/trace_ui.rs` — Auto-trace: pixel drawing to vector lines (center line, width as pressure) and color shapes
- `src/vector.rs`, `src/shapes.rs` + `tabs/draw/vector_ui.rs` — vector layers: lines with pressure, rasterizer, eraser, node editing, hold-to-straighten
- `src/gradient.rs`, `src/oklab.rs`, `src/randomizer.rs` + `tabs/draw/gradient_ui.rs`, `random_ui.rs` — gradients (shapes, stops, OKLab, gradient map, colors along a stroke) and the color randomizer
- `src/layerfilter.rs` + `src/tabs/draw/organizer.rs` — layer filters (chips with NOT, sorting, saved filters), picking many layers, favorites, Oops restore, quick bar
- `src/select.rs`, `src/fill.rs`, `src/transform.rs`, `src/symmetry.rs` — selections (masks, magic wand, grow/shrink/feather, outlines), paint bucket, move & transform, live symmetry; their panels are in `src/tabs/draw/tools.rs`
- `src/theme.rs`, `src/snake.rs`, `src/icons.rs`, `src/widgets.rs`, `src/colorpick.rs` — the look: colors, snake art, icons and widgets, all drawn in code

## Contributing

Issues and pull requests are welcome. Please run `cargo test --release` before sending a change, and keep new UI text short and plain.

## License

SeviathStudio is free software: you can redistribute it and/or modify it under the terms of the **GNU General Public License, version 3 or (at your option) any later version** — see [COPYING](COPYING). Every source file carries `SPDX-License-Identifier: GPL-3.0-or-later`.

- Modified versions you share must publish their source under the same license.
- Programs that only talk to SeviathStudio over its planned WebSocket/OSC API are not covered and may use any license.
- Brushes, textures and other art in the library keep their own licenses (CC0 / CC BY); the GPL covers the program code.
- Third-party libraries and fonts keep their own (GPL-compatible) licenses, listed in *Settings → Info*.

**Created by Seviath** — <https://x.com/SnakeSeviath>
