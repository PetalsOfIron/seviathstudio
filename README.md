# SeviathStudio

Open-source app to **draw, rig, animate and stream a 2D character from one file**, with reliable PSD export — made for VTuber artists, riggers and VTubers. Native Rust, built with [egui](https://github.com/emilk/egui).

This is the Rust version. It is being built tab by tab: **Draw** works today (brush engine with 103 brushes and pen pressure, layers with blend modes, clipping and folders, undo history); Parts, Rig, Physics, Animate and Stream follow.

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
- `brush_sheet.png` — every brush painted on one image (`cargo test --release brush_sheet`)
- `texture_sheet.png` — every texture in its natural colors (`cargo test --release texture_sheet`)

Performance check: `cargo test --release bench_full_redraw -- --ignored --nocapture`

## Code map

- `src/app.rs` — shell: top bar, workspaces, tabs, status bar, history and settings windows
- `src/settings.rs`, `src/settings_ui.rs` — settings (saved as JSON in your user config folder) and their pages
- `src/history.rs` — undo: copy-on-write snapshots, undo per tab or everything, undo a single step from the middle
- `src/tabs/` — one module per tab (`draw.rs` + `draw/layers.rs` + `draw/brushes.rs` is the Draw tab)
- `src/doc.rs` — document, layers (folders, clipping, labels), tiles and compositing
- `src/blend.rs` — the 17 blend modes (W3C compositing math)
- `src/view.rs`, `src/canvas.rs` — zoom/pan/rotate/mirror and GPU tile display
- `src/brush/` — brush engine: settings + presets, tips, textures, stroke engine (stabilizer, pressure, taper, mixing, whole-stroke edges)
- `src/stroke.rs` — the live stroke buffer shown on top of its layer until the pen lifts
- `src/theme.rs`, `src/snake.rs`, `src/icons.rs`, `src/widgets.rs`, `src/colorpick.rs` — the look: colors, snake art, icons and widgets, all drawn in code

## Contributing

Issues and pull requests are welcome. Please run `cargo test --release` before sending a change, and keep new UI text short and plain.

## License

To be decided by the author (MIT or MPL-2.0). Third-party code and fonts keep their own licenses (listed in *Settings → Info*).

Made by [@SnakeSeviath](https://x.com/SnakeSeviath).
