# SeviathStudio (Rust)

Native Rust rewrite of SeviathStudio using [egui](https://github.com/emilk/egui). The web version (`../seviathstudio`) stays the working app until this one reaches parity.

## Run

```
cargo run --release
```

## Screenshot checks

Renders the app offscreen and writes PNGs to `target/screens/`:

```
cargo test screens -- --nocapture
```

## Layout

- `src/app.rs` — shell: top bar, workspace switch (Character / Design), tabs, status bar, settings
- `src/tabs/` — one module per tab (`draw.rs` is the Draw tab)
- `src/theme.rs` — colors and widget styling
- `src/snake.rs`, `src/icons.rs` — logo, snake divider, scale pattern and icons, all drawn in code
- `src/widgets.rs`, `src/colorpick.rs` — shared widgets
