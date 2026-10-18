# Working on SeviathStudio (notes for Claude)

## The project
- Native Rust desktop app: **draw, rig, animate and stream a 2D (VTuber) character from one file**. egui/eframe **0.36**, wgpu, edition 2024. GPL-3.0-or-later (every `.rs` starts with `// SPDX-License-Identifier: GPL-3.0-or-later`).
- This folder: `C:\Users\SaphiresSilver\Projects\seviathstudio-rs` (git, branch `main`, remote https://github.com/santamadaraneilande-ctrl/seviathstudio).
- The **plan**: `C:\Users\SaphiresSilver\Downloads\SeviathStudio_Project_Plan_EN.md` (Parts A–AV, ~3400 lines; the user keeps adding parts — reread the new ones when asked). Part AU records what the Rust build already is.
- **`ROADMAP.md`** = build order of the whole plan (milestones 1–56, each with the technique). When the user says "continue" / "next task": build the next roadmap milestone.
- The old Svelte web app is at `C:\Users\SaphiresSilver\Projects\seviathstudio` (reference only, e.g. `src/lib/*.ts` to port). **Never commit or push there** (its push URL is disabled on purpose). Claude Code sessions may start with that folder as cwd — always work with absolute paths into `seviathstudio-rs`.

## The user
- An artist (Seviath, @SnakeSeviath), not a programmer. Has a **Huion tablet** (pressure via Windows Ink → `Event::Touch { force }`).
- Writes English or **Latvian** — answer in the language of their message. Keep replies plain and short; explain what they can now do in the app.
- Wants *everything in the plan*, working. "Decide settings yourself" — add every setting that's useful, but **every setting must actually work**.
- Desktop shortcut runs `target\release\seviathstudio.exe` → always `cargo build --release` at the end of a milestone.

## Rules for every milestone
- Ships working: **no placeholder buttons**, no fake features. Tests + screenshot checks. Never needs a paid service. Keep secrets/personal paths out of the repo.
- Be honest in the final message: what works, what was not tested, what differs from the plan.
- Finish with: `cargo test --release` (all green) → `cargo build --release` → update `README.md` + `ROADMAP.md` (move the milestone to "Done") → commit → `git push` (pushing from this folder is allowed and wanted).
- Commit messages end with `Co-Authored-By: <model name> <noreply@anthropic.com>`.

## Toolchain & commands
- Cargo is at `%USERPROFILE%\.cargo\bin` — in Bash prepend: `export PATH="$USERPROFILE/.cargo/bin:$PATH"`.
- `cargo test --release` (~25 s for tests, longer when compiling). Screenshot tests write `target/screens/*.png` — **look at them** with the Read tool after UI changes.
- UI tests use `egui_kittest` (`src/screens.rs`): `Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| SeviathApp::new(cc))`, helpers `stroke`, `wave`, `button`, `save`, `alpha_at`. Probe painted pixels with a tolerance window (stabilizer moves lines off the raw path).
- Editing on Windows: Bash heredocs with quotes are fragile — for multi-line edits write a small Python script (`python - <<'EOF' … EOF` with `s.replace(old, new)`) or use the Edit tool.

## Code style
- Not rustfmt-formatted: long lines (~140–160), match the surrounding code. Comments short and plain, like the existing ones.
- UI text: short, plain words; **no arrow glyphs (→ ↔ ↕)** in UI strings (the font lacks them, they render as boxes).
- egui 0.36 API: `App::ui(&mut self, ui, frame)`, `Panel::left/right/top/bottom(id).show(ui, ..)`, `CentralPanel::show(ui, ..)`, `egui::Modal`, `ui.close()`, `DroppedFile` path via trait, `ctx.content_rect()`, `request_repaint_after_secs`.

## Architecture (where things are)
- `src/app/mod.rs` shell (top bar, tabs, status bar, history/settings/account windows); `src/app/files.rs` File menu, background save/open/export jobs, autosave, close guard, New project dialog, project manager.
- `src/doc.rs` document: layers (folders, clipping, blend), **256×256 tiles of premultiplied `Color32` in `Arc` (copy-on-write)**, compositing (rayon), `selection: Option<Selection>`.
- `src/history.rs` undo (`DocState` snapshots incl. selection; per-tab undo, undo-only-this-step; `generation` counter = dirty tracking).
- `src/brush/` engine (settings, 103 presets, tips, textures, stroke engine with stabilizer, mirrors for symmetry); `src/stroke.rs` live stroke buffer (masked by selection on apply).
- `src/select.rs` selection masks (polygon raster, combine, grow/shrink via distance transform, feather, outline, flood, wand); `src/fill.rs` bucket + gap closing; `src/transform.rs` floating pixels + homography; `src/symmetry.rs`.
- `src/tabs/draw.rs` Draw tab (+ `draw/layers.rs`, `draw/brushes.rs`, `draw/references.rs`, `draw/tools.rs` for selection/fill/move/symmetry UI). History steps come from `track_history` (a change finishes when the pointer is up; `label_hint` names it).
- `src/files/` formats: `project.rs` (`.seviath` ZIP: `project.json` + tile PNGs + thumbnail), `psd.rs`, `ora.rs`, `kra.rs` (import), flat export in `mod.rs` (PNG carries an optional credit text chunk).
- `src/settings.rs` (serde JSON in `%APPDATA%\SeviathStudio\settings.json`, `#[serde(default)]`, `persist=false` in tests) + `src/settings_ui.rs` pages.
- Look: `theme.rs` (colors EMERALD, VENOM, GOLD, PANEL, TEXT, TEXT_DIM…), `snake.rs` (logo, scales, divider), `icons.rs` (line icons drawn in code), `widgets.rs` (chip, segmented, icon_button, section_title).

- Effects/adjustments: `Layer.fx` + `Layer.adjust` (LayerKind::Adjust); drawn in `Document::composite_into`; effect reach is capped at 200 px so a tile only needs its 8 neighbours (`take_dirty` widens by one ring when effects exist); `files::bake` for PSD/ORA.

## Where the work stands
- Milestones **1–20b done** (see ROADMAP "Done"). 508 tests pass.
- Plan Part AT core is done (Info screen, PNG credit, README credit). ROADMAP now includes Parts AS (milestone **9a**) and AV (**37a**).
- Everything up to milestone 30f is done (see ROADMAP "Done"). **Next: better face guess and shapes from real models (user VRMs, CC4 FBX), then the rest of 30c (optional AI, 3D painting)**, then 31. (old note: 30c — rig an uncut character into 3D for games (AI and own rig mixable, VRM engine)** (21 to 24 are dropped, the app is offline), then rigging, animation, streaming, games and AI (26–51).
- Gotcha: egui `consume_key(Modifiers::NONE, key)` also matches when Alt is held, so check keys with Alt/Ctrl **before** plain tool keys (see the order in `DrawState::ui`). In kittest, hold modifiers for clicks with `Event::ModifiersChanged`.
- Gotcha: inside `DrawState::ui` a change made by code is picked up by `track_history` by itself: set `label_hint` instead of using `apply_external` (that is for code that runs outside the draw frame, e.g. menus), or the step is recorded twice.
- Gotcha: very long bash heredocs sometimes fail to parse — write the Python patch script with the Write tool into the scratchpad and run it.
- Decisions already made (don't re-ask): no Mixbox (license) → own Kubelka–Munk mixing; project-file verification can only be tamper-evident (open-source app); game runtimes in separate MIT repos; payments only via hand-off (PayPal), the app never touches money.

## Online scope (user decision 2026-10-17, updated)
The app is **offline for now**: no accounts, no online library, no shop, no chat, no servers. The library is only the user's own brushes. Sharing is done by people themselves on other sites. Drop or skip any roadmap item that needs the internet (AI features use the user's own local AI).

## Computer limits
The user's PC has 8 GB VRAM and can't do several heavy things at once. Screenshot tests take one GPU lock (`lock_gpu()` in `src/screens.rs`): never run GPU jobs in parallel. The whole test run takes about 3 to 4 minutes for that reason.

- **FEEDBACK.md**: the user tests the app and writes bugs and ideas there. Read it at the start of every session: fix the bugs first, do the ideas that fit (a better solution than the one written is welcome, say what was done), and move handled items to its Done part.
