# The virtual camera (show the model in Discord, Zoom, a browser)

The model can be a camera that other programs pick. The camera is made by OBS Studio (free); SeviathStudio sets OBS up.
You do it once, then it is one click.

## Once

1. **Open SeviathStudio**, go to the **Stream** tab, and make sure the character moves (a rig made in the Rig tab).
2. **Close OBS** if it is open.
3. In the Stream tab find **Virtual camera**. It shows where OBS is (for example `C:\Program Files\obs-studio\...`).
4. Press **Let this program control OBS**.
   It makes a password and switches on the remote control of OBS. The old OBS settings file is kept next to it as
   `config.json.before-seviath`.
5. Press **Start OBS**. Wait until the OBS window is there.
   SeviathStudio notices that OBS is open and connects by itself (the OBS Studio box says Connected).

## Every time

1. Open SeviathStudio, Stream tab. Open OBS (or press **Start OBS**). It connects by itself.
2. Press **Make the virtual camera**.
   This makes a scene called `SeviathStudio` in OBS with the model on a background colour and starts the camera.
   The background colour is the one you picked under *Show the model in other programs* (Dark, Green, Light, Black).
   A camera cannot be see-through, so choose the colour you want behind the model.
3. In Discord, Zoom, Teams or the browser: Settings, Video, Camera, pick **OBS Virtual Camera**.
4. To stop: **Stop the virtual camera** (or the hotkey *OBS virtual camera on or off*, made in Hotkeys).

## If something does not work

- **"OBS is open, but its remote control is off"**: close OBS, press *Let this program control OBS*, start OBS again.
  Or in OBS: Tools, WebSocket Server Settings, switch on *Enable WebSocket server*.
- **It does not connect**: look at the line under the OBS Studio box. A wrong password is said in words. Press *Disconnect*,
  then *Connect*. The port is 4455 unless you changed it in OBS.
- **The other program does not list OBS Virtual Camera**: the camera must be running first (press Make the virtual camera),
  and the other program often has to be restarted after the first time the camera was ever started.
- **The model is cut or too small**: *Size* under *Show the model in other programs* sets the size the camera gets.
- **OBS is not installed**: *Get OBS Studio (free)* opens obsproject.com/download, *Install it with winget* installs it in a window.

## Why OBS and not a camera of its own

Windows only lets a program become a camera through a driver. OBS Studio already has one that is signed and that every
program knows, so SeviathStudio uses it instead of shipping its own driver.
