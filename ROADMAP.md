# SeviathStudio (Rust) — Roadmap

How every part of the project plan (`SeviathStudio_Project_Plan_EN.md`, Parts A–S) gets built in this Rust app: in which order, with which technique, and what each step needs from outside (accounts, servers, hardware). Libraries named here are open source and GPL-3.0-compatible.

**Rules for every milestone:** it ships working (no placeholder buttons), it has tests and screenshot checks, settings it adds actually do something, and it never needs a paid service.

## Done

| # | Milestone | Plan parts |
|---|---|---|
| 1 | Snake design, app shell, 5 tabs + Parts, Character/Design workspaces | §2, D2 "snake symbols" |
| 2 | Canvas: tiles, GPU display, zoom/pan/rotate/mirror, navigator | §9, C2 |
| 3 | Layers: 17 blend modes, clipping, folders, drag & drop, labels, search | §13, D2 layer suite |
| 4 | Brush engine: pressure, stabilizer, 103 brushes, 24 textures, dual/triple tips, texture colors, particles | §14, D2, E2 |
| 5 | Undo per tab / everything, history panel, undo-only-this-step; settings (10 pages); pen & tablet page | §2, F3 |
| 6 | References: add/drop, pick, gray, grid, palette, overlay, pop-out; app icon; GPL-3.0-or-later | §18, J3 (part), F1 |

## Drawing — finish the Draw tab (port the first app + Part D2)

| # | Milestone | How |
|---|---|---|
| 7 | **Files & projects** — `.seviath` project (ZIP: layer PNGs + JSON, §28), autosave + recovery copies, project manager with thumbnails, New project dialog (bust/half/full/custom, §9), PNG/JPEG/WebP export, **PSD import/export** with folders, blend modes, clipping, bake-effects (§15), ORA import/export, KRA import | `zip` crate; own PSD writer/reader (format is documented: layer records + RLE); ORA = ZIP+XML; KRA = ZIP+XML+LZF tiles. Project folder in Documents\SeviathStudio |
| 8 | **Selection & tools** — rectangle/ellipse/lasso with add/subtract/invert/feather, magic wand, quick mask, move/transform (scale/rotate/skew/perspective), fill + lasso fill with gap closing (§20), straight lines (Shift+click), live symmetry incl. multi-axis | Selection = one more mask tile layer; transforms resample tiles; fill = port of `fill.ts` |
| 9 | **Gradients** — linear/radial/angle/diamond/mirrored/freeform in OKLab, on-canvas handles, rotate, **blend-gradient** that mixes with existing paint, gradient brush, gradient map (§23, D2) | Gradient as an editable layer type that renders into tiles |
| 10 | **Vector layers** — vector brush with pressure profile, pressure editor graph, smoothing slider, vector eraser, shape recognition, node edit, **auto-trace** (4 modes, center-line) (B8, E4) | Port `vector.ts`/`trace.ts`; tracing on rayon threads |
| 11 | **Filters & effects** — the first app's 10 filters + C1 basics (blurs, sharpen, noise, levels/curves/HSL), non-destructive layer effects (outline, shadow, glow, color/gradient overlay), adjustment layers, check views (gray, invert, color-blind), "draw from image", lineart separate/add (§16, C1, D2) | Effects stored per layer, rendered at composite time; filters run on rayon with live preview |
| 12 | **Brush library tools** — My brushes (save/rename), brush randomizer with kinds (port), remix, **brush from lasso selection** (keep / custom / generated colors), object→seamless texture, brush cards (PNG with brush inside), import .abr/.kpp/.gbr/PNG, export .abr v2/.kpp, use terms + license stored in every brush (§14, D2, D6, E3) | Port `brushimport.ts`/`brushexport.ts`; brush settings as JSON (serde) |
| 13 | **Text & fonts** — text layers, text on curve, own fonts (.ttf/.otf), font from handwriting → .otf (§24) | `ab_glyph`/`ttf-parser` for reading; own glyph builder + OTF writer |
| 14 | **Recorder** — timelapse + real-time to MP4 (§E9), "Drawn in SeviathStudio" work history | `openh264` (BSD) encoder + MP4 muxer, encoding on a background thread |
| 15 | **Panels & workspaces** — every panel dockable/floating, second-monitor windows, saved workspaces, command palette (Ctrl+K), remappable shortcuts with conflict check (C2, B1, F3) | egui viewports for extra OS windows; one action table drives menu, palette and keys |

## Platform (Part D6, F3)

| # | Milestone | How |
|---|---|---|
| 16 | **Accounts** — email/Google sign-in, **TOTP two-factor**, sessions, data export/delete, local-only mode; points & bonuses | Supabase (open source, free tier — the first app already has `supabase-setup.sql`); OAuth via system browser + loopback redirect (PKCE); tokens kept in the OS keychain (`keyring`) — never in plain files |
| 17 | **Online library** — browse/search/filter by type, install, share via pull request to the public GitHub repo, ratings, moderation; shop tab separate from free content (§25, D6) | GitHub REST API (read without login); sharing through the user's own GitHub sign-in |

## Rigging, physics, animation (Parts §4–§12, B2–B5, G, H)

| # | Milestone | How |
|---|---|---|
| 18 | **Parts tab** — tags, auto-suggest by name/shape/position, L/R pairs, checklist, pre-rig improver (§6) | Layer metadata + image analysis on tiles |
| 19 | **Rig core** — auto mesh from alpha, Mesh/Rotation/Line changers + our own "soft" changer, parameters & keyforms incl. 2-D grids, glue, live link from Draw (§5, §13, D3) | Delaunay meshing (`spade`), deformation on the GPU through an egui-wgpu paint callback |
| 20 | **Auto-rig** — "This is a face/body/hair/arms", Rig face template, A/I/U/E/O, auto blink, XYZ generation for every part, IK, mirror/asymmetry, quality checker, beginner/pro mode (§5, G1, B2) | Rules + templates first; camera-assisted rigging later (§7) |
| 21 | **Physics** — types (hair, cloth, chain, elastic…), materials, wind themes, collisions, test panel (§10, B3) | Verlet pendulum chains driving Line changers, fixed time step |
| 22 | **Multi-view & styles** — view tabs (front/¾/side/back) with per-view layers, spine chain, body turn; shading-aware deformation, depth layer, sliding shine (G, H1) | Views = keyforms on view parameters; depth shading in the GPU renderer |
| 23 | **Graphics quality ladder** — Potato → MAX presets, render scale, filtering, AA, shading tiers, dynamic resolution (Part I) | Settings drive the wgpu rig renderer |
| 24 | **Animate tab** — timeline, keyframes, Bézier easing graphs, onion skin, loops, rig + physics inside Animate, auto-generation, lip sync from audio; export MP4/WebM-alpha/GIF/PNG sequence/sprite sheet/Lottie (§11, B5, D4) | `openh264`/`gif` encoders; audio via `cpal` |

## Streaming (Parts §3, B4, F2, J, R, S)

| # | Milestone | How |
|---|---|---|
| 25 | **Tracking v1** — connect OpenSeeFace, VMC, iFacialMocap/ARKit apps; One-Euro smoothing, deadzones, health meter, auto blink/breathing, look-at-mouse | UDP/OSC receivers (`rosc`); no camera code needed for v1 |
| 26 | **Built-in camera tracking ("Seviath Track")** — face + hands + full body, 52 ARKit-style blendshapes, iris, quality presets down to "potato", auto-degrade | Camera via `nokhwa`; ONNX models (MediaPipe/RTMPose, Apache-2.0) run with ONNX Runtime (`ort`) on CPU or GPU |
| 27 | **Outputs** — transparent window, OBS browser source, **virtual camera**, Spout2, NDI later | eframe transparent viewport; local HTTP+WebSocket server for the browser source; Windows 11 virtual camera API (`MFCreateVirtualCamera`) via the `windows` crate; v4l2loopback on Linux |
| 28 | **Hotkeys & input binding** — keyboard, mouse buttons, gamepad sticks as analog, MIDI CC, chains, profiles, conflict detector; **OBS WebSocket** scenes/sources/hotkeys by name (R, B4) | `global-hotkey`, `gilrs`, `midir`, `obws` |
| 29 | **Stream extras** — props pinned to parts, scenes, expressions on hotkeys, lighting adapt, Stream Test window (compression sim, device frames), Readiness notifier, audio-only lip sync (J2, J4, B4) | Real encode→decode through `openh264` for the compression preview |
| 30 | **Seviath Link** — local WebSocket/OSC event server, rules "WHEN event THEN actions", game connectors (CS2 GSI, League client), Stream Deck/Touch Portal bridge (F2, J1) | `tokio-tungstenite`, `rosc`; read-only connectors |
| 31 | **VRChat** — OSC in/out, web preview link for video players, standee avatar export (VRM) (Part S) | `rosc`; glTF/VRM writer |
| 32 | **Publish tab** — model package wizard (PSD for VTube Studio path, Inochi2D export, compressed own format), Twitch emotes/panels/banners, clip maker, web preview, batch export (J5, D5) | Reuses exporters from 7, 14, 24 |

## AI — bring your own (Parts K, L, M, N)

| # | Milestone | How |
|---|---|---|
| 33 | **Seviath AI Bridge** — auto-detect Ollama (port 11434) and ComfyUI (port 8188), any OpenAI-compatible endpoint, up to 3 providers with roles (primary/fallback/specialist), "0 € mode" indicator; keys in the OS keychain | `reqwest`; the app never ships or hosts a model |
| 34 | **AI in drawing** — generate brush/texture/tip, colorize lasso area, draw a marked part, AI view drafts (side/back) with consistency check, AI line cleanup | ComfyUI workflows sent as JSON; results land as layers, never overwrite art |
| 35 | **AI brushes** — LLM-guided random brushes, generated tips/grains, curated chaos (every proposal render-tested), evolution, style-aware packs (Part L) | LLM returns brush JSON → validated → test-rendered with the brush sheet code |
| 36 | **Generators** — model generator (multi-view sheet → auto-trace → tagged layers → rig suggestion), animation from text, stream assets with app-rendered text, top-3 multi-provider scoring (K2–K4) | Combines 10, 18, 20, 24, 33 |

## Community — shop, chats, moderation (Parts §26, D5, O, P, Q)

| # | Milestone | How |
|---|---|---|
| 37 | **Shop** — section tree (Models: full/half/chibi/PNGTuber/furry, Emotes, Twitch assets, Animations, PSD & bases, Brushes, Fonts, Outfits, Find a rigger), structured listing forms, links to Payhip/Ko-fi/Gumroad/Etsy (payments stay with those platforms), license checkboxes, fee/tax display, refund text, points & point outfits | Supabase tables + row-level security; the app shows and links, it never holds money |
| 38 | **Section chats** — per-section chats, threads, whispers, Twitch-style limits (slow, followers-only, unique, emote-only, raid mode), 500-char limit, link whitelist | Supabase Realtime channels; limits enforced server-side |
| 39 | **Autobot** — rules layer 24/7 (word lists, floods, scam links), AI layer on the room owner's connected AI (Claude preferred): warnings, graduated mutes, never bans, explained actions, appeals, transparency log | Deterministic layer in Supabase Edge Functions or Cloudflare Workers (free tier); AI layer runs in the owner's app |

## What needs you (can't be done from code alone)

- **Supabase project** for accounts/library/shop/chats (free tier) — the first app's setup script can be reused; keys go in a local `.env`, never in the repo.
- **GitHub repo** for the public brush library (and for this code).
- **A tablet / camera / iPhone** for real-device checks of pressure and tracking — the app has test pages for both.
- **Your own AI** (Ollama/ComfyUI installed, or free-tier keys) for the AI features — everything else works without it.
- **Platform accounts** (Twitch, OBS on your PC, payment platforms) when their connectors are built.
