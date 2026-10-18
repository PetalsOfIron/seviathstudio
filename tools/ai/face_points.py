# SPDX-License-Identifier: GPL-3.0-or-later
# Finds the eyes and the mouth in a picture of a face, for the face blend shapes of SeviathStudio. It is run by the app as
# a helper program, one picture at a time, and prints one line of JSON: the points in pixels of the full picture, or
# {"error": "..."}. Two finders are used:
#   mediapipe  Google's face landmarks (pip install mediapipe), good for realistic faces
#   dino       Grounding DINO with the words "eye" and "mouth" (pip install transformers torch pillow), for drawings
#   auto       mediapipe first, then dino if no face was found or mediapipe is not installed
import argparse
import json
import sys


def fail(message):
    print(json.dumps({"error": message}))
    sys.exit(1)


def load(path, crop):
    try:
        from PIL import Image
    except ImportError:
        fail("Pillow is not installed (pip install pillow).")
    img = Image.open(path).convert("RGB")
    ox, oy = 0, 0
    if crop:
        x, y, w, h = [int(v) for v in crop]
        x, y = max(0, x), max(0, y)
        img = img.crop((x, y, min(img.width, x + w), min(img.height, y + h)))
        ox, oy = x, y
    return img, ox, oy


def with_mediapipe(img):
    try:
        import mediapipe as mp
        import numpy as np
    except ImportError:
        return None, "mediapipe is not installed (pip install mediapipe)."
    arr = np.array(img)
    with mp.solutions.face_mesh.FaceMesh(static_image_mode=True, max_num_faces=1, refine_landmarks=True) as mesh:
        result = mesh.process(arr)
    if not result.multi_face_landmarks:
        return None, "mediapipe found no face (it is made for real faces)."
    lm = result.multi_face_landmarks[0].landmark
    w, h = img.width, img.height

    def pt(i):
        return (lm[i].x * w, lm[i].y * h)

    def mean(ids):
        ps = [pt(i) for i in ids]
        return (sum(p[0] for p in ps) / len(ps), sum(p[1] for p in ps) / len(ps))

    # the corners and the lids of each eye, and the corners and inner lips of the mouth
    a, b = mean([33, 133, 159, 145]), mean([362, 263, 386, 374])
    eyes = sorted([a, b], key=lambda p: -p[0])  # the character's left is the larger x
    width = (abs(pt(133)[0] - pt(33)[0]) + abs(pt(263)[0] - pt(362)[0])) / 2
    mouth = mean([61, 291, 13, 14])
    return {"eye_l": eyes[0], "eye_r": eyes[1], "mouth": mouth, "eye_w": width, "mouth_w": abs(pt(291)[0] - pt(61)[0])}, None


def with_dino(img):
    try:
        import torch
        from transformers import AutoModelForZeroShotObjectDetection, AutoProcessor
    except ImportError:
        return None, "Grounding DINO needs transformers and torch (pip install transformers torch pillow)."
    name = "IDEA-Research/grounding-dino-tiny"
    device = "cuda" if torch.cuda.is_available() else "cpu"
    processor = AutoProcessor.from_pretrained(name)
    model = AutoModelForZeroShotObjectDetection.from_pretrained(name).to(device)
    inputs = processor(images=img, text="eye. mouth. lips.", return_tensors="pt").to(device)
    with torch.no_grad():
        outputs = model(**inputs)
    sizes = [img.size[::-1]]
    try:
        res = processor.post_process_grounded_object_detection(outputs, inputs.input_ids, threshold=0.2, text_threshold=0.15, target_sizes=sizes)[0]
    except TypeError:
        res = processor.post_process_grounded_object_detection(outputs, inputs.input_ids, box_threshold=0.2, text_threshold=0.15, target_sizes=sizes)[0]
    labels = res.get("text_labels") or res.get("labels")
    found = [(str(l), float(s), [float(v) for v in b]) for l, s, b in zip(labels, res["scores"], res["boxes"])]
    eyes = sorted([f for f in found if "eye" in f[0]], key=lambda f: -f[1])[:6]
    mouths = sorted([f for f in found if "mouth" in f[0] or "lip" in f[0]], key=lambda f: -f[1])
    if len(eyes) < 2:
        return None, "Grounding DINO found %d eyes, two are needed." % len(eyes)

    def centre(f):
        x0, y0, x1, y1 = f[2]
        return ((x0 + x1) / 2, (y0 + y1) / 2)

    def width(f):
        return f[2][2] - f[2][0]

    # the two eyes: a pair side by side at the same height, apart by more than an eye's width, the best scores together
    best = None
    for i in range(len(eyes)):
        for j in range(i + 1, len(eyes)):
            a, b = eyes[i], eyes[j]
            ca, cb = centre(a), centre(b)
            w = (width(a) + width(b)) / 2
            if abs(ca[0] - cb[0]) < 0.9 * w or abs(ca[1] - cb[1]) > 0.5 * w:
                continue
            score = a[1] + b[1]
            if best is None or score > best[0]:
                best = (score, a, b)
    if best is None:
        return None, "Grounding DINO found eyes, but no two that make a pair."
    pair = sorted([best[1], best[2]], key=lambda f: -centre(f)[0])
    out = {"eye_l": centre(pair[0]), "eye_r": centre(pair[1]), "eye_w": (width(pair[0]) + width(pair[1])) / 2}
    mid_x = (out["eye_l"][0] + out["eye_r"][0]) / 2
    dist = abs(out["eye_l"][0] - out["eye_r"][0])
    # a mouth below the eyes and near the middle line
    ok = [m for m in mouths if centre(m)[1] > out["eye_l"][1] + 0.3 * dist and abs(centre(m)[0] - mid_x) < 0.8 * dist]
    if ok:
        out["mouth"] = centre(ok[0])
        out["mouth_w"] = width(ok[0])
    else:
        # no mouth found (a beast's snout, a mask): put it under the eyes the way faces usually are
        out["mouth"] = (mid_x, (out["eye_l"][1] + out["eye_r"][1]) / 2 + 0.9 * dist)
        out["mouth_w"] = 0.8 * dist
        out["note"] = "the mouth was placed from the eyes"
    return out, None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True)
    ap.add_argument("--backend", default="auto", choices=["auto", "mediapipe", "dino"])
    ap.add_argument("--crop", nargs=4, type=float)
    args = ap.parse_args()
    img, ox, oy = load(args.image, args.crop)
    order = {"auto": ["mediapipe", "dino"], "mediapipe": ["mediapipe"], "dino": ["dino"]}[args.backend]
    problems = []
    for b in order:
        out, why = (with_mediapipe if b == "mediapipe" else with_dino)(img)
        if out:
            for k in ("eye_l", "eye_r", "mouth"):
                out[k] = [out[k][0] + ox, out[k][1] + oy]
            out["backend"] = b
            print(json.dumps(out))
            return
        problems.append(why)
    fail(" ".join(problems))


if __name__ == "__main__":
    main()
