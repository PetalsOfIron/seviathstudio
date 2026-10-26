# Feedback: bugs and ideas

Write what you find here while you test the app. One item per line or short paragraph; no format needed. Claude reads this file at
the start of every session, fixes the bugs first, and does the ideas when they fit (a better solution than the one written here is
fine: it is explained in the "Done" part below).

## Bugs (things that are broken)

-

## Ideas (things that could be done)

-

## Done (Claude moves handled items here with a line about what was done)

### 2026-10-26 (the first round of your testing)

- **Rigging by hand, hard to find, UI hard to understand (Rig, Physics, Animate tabs):** the tools of rigging by hand are now one row along the top of the tab and always there: Meshes, Skin and bones, every changer (Rotation, Warp, Line, Soft, Path, Pins, Move, Scale, Flip, Fade, Order, Wave), Changers, Shapes, Glue. A tool opens a small window of its own with its details (the grid of a warp, what it sits inside, the pivot, a name). The parts and **all the parameters (sliders) are on the left, next to each other**, always. The **automatic rig is on the right** (Rig tab). Parts need no tags for rigging by hand: a part with no mesh gets one when the changer is made. (`app/rigbench.rs`)
- **Physics tab:** the same top tools and left side, and a **Check the physics** window (the picture with the physics running, Reset, on and off).
- **Animate tab:** the same top tools and left side (parts and parameters), and the button **Edit the rig on the picture** that shows the rig editor in the middle (all parts, one by one, drag the points) instead of the finished picture. Rotation changers are in the tools. **3D by hand:** on the Rig tab, 3D character page, *Sliders for the bones* makes a slider for each main bone (arms, legs, spine, head: group 3D bones, on the left); they pose the 3D character, and in the Animate tab the button **3D view** shows the posed 3D character in the middle, so the bone sliders are keyed like any slider. *Not done:* saving that animation as a VRM animation file.
- **3D and 2D rigging apart:** the Rig tab has the buttons **2D rig** and **3D character** at the top. The 3D character has its own page: the big 3D picture in the middle, its controls on the right.
- **Stream, background colour wheel:** *Show the model in other programs* has **or any colour**, a colour picker, next to the four backgrounds.
- **Stream, "Sliders moved by the face" unclear:** it is the list of which slider follows which part of your face (VTube Studio has the same thing under Model parameters). It is now under **Advanced: which slider follows what** and closed; *Make the mapping* fills it for you. Under Calibrate there is **Follow the face with**: one choice (the camera with MediaPipe, VMC, iFacialMocap, OpenSeeFace).
- **Three eyes open inputs:** the one for both eyes is gone from the list; Eye open left and right stay (a rig with a single Eyes open slider is still moved).
- **Another virtual camera, not only OBS:** **Unity Capture** (free, MIT, no OBS): File, Downloads gets it, you run its Install.bat once, then *Send the model to Unity Video Capture* in Stream, Virtual camera. It takes see-through pictures too. *Not tried with the real driver* (the way the pictures are handed over is tested against a stand-in).
- **"Stream trackers are only for 3D tracking, there are no 2D trackers. MediaPipe, NVIDIA Broadcast?":** a tracker only gives numbers (the turn of the head, the eyes, the mouth, the brows), so the same ones move a 2D or a 3D character; there is no separate kind for 2D. MediaPipe is the camera tracker we use. NVIDIA Broadcast is a camera filter, it gives no tracking numbers to other programs (VTube Studio uses NVIDIA's separate AR SDK; that is not in here).
- **"You can go live without everything set in Stream":** the checklist only tells; nothing stops you.
- **Draw tab, after changing the brush size it zooms on click and cannot draw:** I could not make it happen (the size slider and keys leave the brush on, there is a test for it). The only way a click zooms is the Zoom tool (key Z). Now, when the Zoom tool zooms on a click, the page says so: "The Zoom tool is on, so a click zooms in. Press B (or pick the brush on the left) to paint again." If it still happens, write which keys you pressed (a pen's express keys can send Z).

- Grid size both ways, sharper zoom, points that move, Path/Soft/Pins, push brush, physics groups, parameters column, solid black: you can set the warp grid columns and rows separately (Changers and Shapes); the picture is drawn sharp at the zoom you are at; every point of a changer can be dragged right away when a slider is picked (it saves by itself when you let go); Path needs picked parts and makes the meshes it lacks, and the tool no longer stays in draw mode after you switch; the new Brush tool pushes the points of a warp, line, pins, paths or soft changer; the Physics tab has a "Physics settings" window (groups, inputs, outputs, pendulum points like Live2D); Parameters are their own column next to Parts in Rig, Physics and Animate; Pen is fully opaque, so black on a normal layer is fully black.
