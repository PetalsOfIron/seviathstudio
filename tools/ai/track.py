# SPDX-License-Identifier: GPL-3.0-or-later
# Camera face tracking for SeviathStudio: reads the webcam, finds the face with MediaPipe's face landmarker (52 ARKit blend shapes and
# the turn of the head) and sends it to the app over UDP, the way VMC does: `/VMC/Ext/Blend/Val name value` for every shape and
# `/Seviath/Head yaw pitch roll` (degrees) and `/Seviath/Pos x y z` (centimetres). The app starts and stops this program.
#   python track.py --model face_landmarker.task [--camera 0] [--port 39539] [--fps 30]
#   python track.py --model face_landmarker.task --selftest      (loads the model, looks at one empty picture, prints JSON, exits)
import argparse
import json
import math
import socket
import struct
import sys
import time


def fail(message):
    print(json.dumps({"error": message}), flush=True)
    sys.exit(1)


def osc_pad(b):
    return b + b"\x00" * (4 - len(b) % 4)


def osc(addr, *args):
    tags = "," + "".join("f" if isinstance(a, float) else "s" for a in args)
    out = addr.encode() + b"\x00"
    out += b"\x00" * ((4 - len(out) % 4) % 4)
    t = tags.encode() + b"\x00"
    t += b"\x00" * ((4 - len(t) % 4) % 4)
    out += t
    for a in args:
        if isinstance(a, float):
            out += struct.pack(">f", a)
        else:
            s = str(a).encode() + b"\x00"
            s += b"\x00" * ((4 - len(s) % 4) % 4)
            out += s
    return out


def bundle(messages):
    out = b"#bundle\x00" + b"\x00" * 8
    for m in messages:
        out += struct.pack(">I", len(m)) + m
    return out


def euler_from_matrix(m):
    # the 4x4 transform of the face: yaw about the up axis, pitch about the side axis, roll about the view axis
    r = [[m[i][j] for j in range(3)] for i in range(3)]
    sy = math.sqrt(r[0][0] ** 2 + r[1][0] ** 2)
    if sy > 1e-6:
        pitch = math.atan2(r[2][1], r[2][2])
        yaw = math.atan2(-r[2][0], sy)
        roll = math.atan2(r[1][0], r[0][0])
    else:
        pitch = math.atan2(-r[1][2], r[1][1])
        yaw = math.atan2(-r[2][0], sy)
        roll = 0.0
    return math.degrees(yaw), math.degrees(pitch), math.degrees(roll)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--camera", type=int, default=0)
    ap.add_argument("--port", type=int, default=39539)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--fps", type=float, default=30.0)
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--image", help="look at one picture, print what was found as JSON and exit")
    args = ap.parse_args()
    try:
        import cv2
        import mediapipe as mp
        from mediapipe.tasks import python as mpp
        from mediapipe.tasks.python import vision
    except ImportError as e:
        fail("The tracking packages are missing: %s" % e)
    try:
        options = vision.FaceLandmarkerOptions(
            base_options=mpp.BaseOptions(model_asset_path=args.model),
            running_mode=vision.RunningMode.VIDEO,
            output_face_blendshapes=True,
            output_facial_transformation_matrixes=True,
            num_faces=1,
        )
        detector = vision.FaceLandmarker.create_from_options(options)
    except Exception as e:
        fail("The face model could not be loaded: %s" % e)
    if args.selftest:
        import numpy as np
        img = mp.Image(image_format=mp.ImageFormat.SRGB, data=np.zeros((240, 320, 3), dtype=np.uint8))
        detector.detect_for_video(img, 1)
        print(json.dumps({"ok": True}), flush=True)
        return
    if args.image:
        pic = cv2.imread(args.image)
        if pic is None:
            fail("The picture could not be read.")
        img = mp.Image(image_format=mp.ImageFormat.SRGB, data=cv2.cvtColor(pic, cv2.COLOR_BGR2RGB))
        res = detector.detect_for_video(img, 1)
        if not res.face_blendshapes:
            fail("No face was found in the picture.")
        shapes = {c.category_name: round(float(c.score), 3) for c in res.face_blendshapes[0]}
        out = {"shapes": len(shapes), "top": dict(sorted(shapes.items(), key=lambda kv: -kv[1])[:8])}
        if res.facial_transformation_matrixes:
            yaw, pitch, roll = euler_from_matrix(res.facial_transformation_matrixes[0])
            out["head"] = [round(yaw, 1), round(pitch, 1), round(roll, 1)]
        print(json.dumps(out), flush=True)
        return
    cap = cv2.VideoCapture(args.camera, cv2.CAP_DSHOW) if sys.platform == "win32" else cv2.VideoCapture(args.camera)
    if not cap.isOpened():
        fail("The camera %d could not be opened." % args.camera)
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    print(json.dumps({"started": True}), flush=True)
    t0 = time.time()
    last = 0.0
    while True:
        ok, frame = cap.read()
        if not ok:
            time.sleep(0.05)
            continue
        now = time.time()
        if now - last < 1.0 / max(args.fps, 1.0):
            continue
        last = now
        rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
        img = mp.Image(image_format=mp.ImageFormat.SRGB, data=rgb)
        res = detector.detect_for_video(img, int((now - t0) * 1000))
        if not res.face_blendshapes:
            continue
        msgs = [osc("/VMC/Ext/Blend/Val", c.category_name, float(c.score)) for c in res.face_blendshapes[0]]
        if res.facial_transformation_matrixes:
            m = res.facial_transformation_matrixes[0]
            yaw, pitch, roll = euler_from_matrix(m)
            msgs.append(osc("/Seviath/Head", float(yaw), float(pitch), float(roll)))
            msgs.append(osc("/Seviath/Pos", float(m[0][3]), float(m[1][3]), float(m[2][3])))
        try:
            sock.sendto(bundle(msgs), (args.host, args.port))
        except OSError:
            pass


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
