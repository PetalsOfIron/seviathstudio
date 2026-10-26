# Testing guide: what to try, tab by tab

Write what is wrong in `FEEDBACK.md` (Bugs) and what you wish was there (Ideas). One line each is enough: what you did, what you
expected, what happened. Things marked **needs hardware** were only tested with stand-ins, so they are the most likely to have bugs.

## Start

- Open the app (`cargo run --release` in `seviathstudio-rs`, or the built `.exe`). The tabs are Draw, Parts, Rig, Physics, Animate, Stream.
- File, Open takes `.seviath`, `.psd`, `.clip` (Clip Studio), `.ora`, `.kra`, `.cmo3`, `.moc3` (or the `.model3.json` next to it), pictures.
  Try a Live2D model from your VTube Studio folder: it should open as a rig with sliders, and the physics, expressions and
  motions of the folder should come with it (Physics tab, Live2D files).

## Draw

- Brushes, layers, selections, filters, vector layers, text: the earlier milestones. Try your normal work and say what feels wrong.
- File, **AI helper**: set up Ollama or a Gemini key, then try a brush from words, colours from words, a look at the drawing
  (**needs a real AI**). *A picture from ComfyUI*: start your ComfyUI, press *Find its checkpoints*, write what you want, *Make the picture* (**needs ComfyUI**, only tried with a fake one).
- File, New: the new stream sizes (Twitch banner, offline screen, profile picture, YouTube and Discord banners).

## Parts (before rigging a finished drawing)

- Open one of your finished PSDs (many layers in folders). Press **Tidy for rigging**. Every layer gets a tag, your layer order is
  not changed. Check the Parts checklist, fix the tags that are wrong by hand, then go to the Rig tab and press the rig buttons.
- At the end of the Parts side panel: **How to layer** (the order that rigs well) and **Clip the selected layers** (cut layers to the shape of a part).
  Your own clipping from the PSD is kept; the face patch, teeth and inner mouth are cut to the Faceline.
- **Also put each part together** makes a smaller model (the layers of one part become one layer, only where nothing else is
  between them).

## Rig

- The tools of rigging by hand are along the top: Meshes, Skin and bones, the changers (Rotation, Warp, Line, Soft, Path, Pins, Move, Scale, Flip, Fade, Order, Wave), Changers, Shapes, Glue. Click a tool: its window opens with its details. Pick parts on the left (no tags needed), press Make the changer, drag its points on the picture, then Shapes says which slider moves it.
- The parameters (sliders) are on the left under the parts, the automatic rig is on the right.
- **2D rig / 3D character** at the top: the 3D character has its own page. Make the 3D character, turn it, try the expressions, save as VRM 1.0 / 0.x / GLB. The notes in gold are things a game maker should know. Press **Sliders for the bones** to get a slider for each main bone (3D bones, on the left): move them to pose the character, and key them in the Animate tab (button 3D view). Tick **Follow the tracking** and turn a tracking source on in the Stream tab: the 3D head turns and blinks with your face (**needs hardware**). For Unreal Engine: **Get VRM4U** opens the free plugin; copy it into the Plugins folder of your project and drag the saved `.vrm` into the Content Browser (**needs Unreal**, not tried).
- Make a rig (Parts tab first to tag layers for the automatic rig), move the sliders. **Views**: add views, put layers in them, Build the views, move the View slider.
- The AI files (the shape AI, the face finder) are downloaded from the card on the 3D page or from File, Downloads (**needs the internet, several GB**).

## Physics

- The same tools along the top and the same left side as the Rig tab (rigging by hand), and **Check the physics**: a window with the picture and the physics running. Hair and cloth swing, Live2D physics settings from a model folder, expressions and motions of a Live2D model.

## Animate

- Add tracks for sliders and layers, key them (K), play (Space), scrub, change the easing of a key (right click).
- **Markers**: Add marker, rename, Play this part. **Sprite sheet**: Save, Sprite sheet: check the picture and the `.json`.
- **Sound**: Add a sound, see the waveform, play, Make the mouth from the sound.
- **Camera**: Add a camera track (zoom, move, turn), Shake. Save a PNG sequence and look at the pictures.
- **Frame by frame**: New cel track, Draw it, draw in the Draw tab with the Frame before / Frame after bar and the onion skin,
  back in Animate set which drawing shows when (Show drawing, New drawing here, Blank here, timing), the **X-sheet** button
  (click a cell, type a number). Pictures as drawings: a GIF or pictures in a folder (a video needs ffmpeg).
- A rigged character together with drawings: the drawings are over the character.
- Save: PNG pictures, GIF, MP4 (no sound in the video), sprite sheet.

## Stream

Start here: make a rig, open the Stream tab.

- **Tracking** (**needs hardware**): turn on a source and send your face to it: VMC (Waidayo and similar), iFacialMocap,
  OpenSeeFace, or the camera of this computer (needs the AI files, from the Rig tab card). Watch the bars under
  *What the face does now* (with the body and hand models the arms, the lean and the fingers show there too: raise an arm, open and close a hand, **needs hardware**). Press **Make the mapping**, then **Calibrate** looking at the camera. Say how the movement feels
  (too slow, too jumpy, wrong direction: *Turn it round*).
- **Expressions**: pose the sliders, give the look a name, Save. Click it to switch it on and off. Under *Layers it shows or hides*
  pick a prop layer.
- **Hotkeys** (**needs hardware** for MIDI and pad): Learn an input (press a key, a MIDI note or a pad button), pick the action.
  Keys work while another program has the focus.
- **OBS Studio** and **Virtual camera**: follow `docs/virtual-camera.md`. This is the first thing to try with your real OBS.
- **Show the model in other programs**: the model window (see-through, on top) and the OBS browser source address.
- **VRChat**: Send to VRChat with VRChat's OSC on (**needs VRChat**). **Link**: switch it on and open
  `http://127.0.0.1:39560/trigger/hello` in a browser, then Learn an input to see it.
- **Emotes**, **Clip** (records the model to MP4), **Ready to go live?**, **Test the look**.
- Sticks of a pad and knobs of a MIDI controller appear as inputs of the mapping after they have moved (**needs hardware**).

## Settings (the gear)

- Interface, Drawing the character: Quality (Potato to Max) and the pictures a second of the OBS browser source. If the movement is
  slow, pick a lower step.

## What is not done

See `ROADMAP.md`: every row says what is in and what is not. Big missing things: a virtual camera that does not need OBS, Inochi2D
export, Unity / Godot / Unreal runtimes, the one-click AI picture to rigged 3D model.
