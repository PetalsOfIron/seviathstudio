# SPDX-License-Identifier: GPL-3.0-or-later
# Camera face tracking for SeviathStudio: reads the webcam, finds the face with MediaPipe's face landmarker (52 ARKit blend shapes and
# the turn of the head) and sends it to the app over UDP, the way VMC does: `/VMC/Ext/Blend/Val name value` for every shape and
# `/Seviath/Head yaw pitch roll` (degrees) and `/Seviath/Pos x y z` (centimetres). The app starts and stops this program.
#   python track.py --model face_landmarker.task [--camera 0] [--port 39539] [--fps 30] [--pose-model pose_landmarker.task] [--hand-model hand_landmarker.task]
# With the pose and hand models it also sends the body: `/Seviath/Body name value` (ArmLeftAngle, ForearmLeftAngle, BodyTilt, BodyLeanX,
# HandLeftOpen, FingerLeftIndex... see body_values()). Angles are in degrees in the picture: 0 hanging down, 90 straight out, 180 up.
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


def angle_out(a, b, outward):
    # the direction from point a to point b in the picture: 0 hanging down, 90 sideways (outward is +1 or -1 on x), 180 straight up
    dx, dy = (b.x - a.x) * outward, b.y - a.y
    return math.degrees(math.atan2(dx, dy))


def clamp01(v):
    return max(0.0, min(1.0, v))


def dist(a, b):
    return math.hypot(a.x - b.x, a.y - b.y)


def body_values(pose, hands):
    """The numbers of the body: `pose` is the 33 landmarks of one person (or None), `hands` is a list of (side, 21 landmarks) where side
    is the side of the person ("Left" or "Right")."""
    out = {}
    if pose:
        ls, rs, le, re, lw, rw = pose[11], pose[12], pose[13], pose[14], pose[15], pose[16]
        seen = lambda *p: all(getattr(q, "visibility", 1.0) > 0.5 for q in p)
        # the person's left is on the right of the picture, so outward for the left arm is +x
        if seen(ls, le):
            out["ArmLeftAngle"] = angle_out(ls, le, 1)
        if seen(rs, re):
            out["ArmRightAngle"] = angle_out(rs, re, -1)
        if seen(le, lw):
            out["ForearmLeftAngle"] = angle_out(le, lw, 1)
        if seen(re, rw):
            out["ForearmRightAngle"] = angle_out(re, rw, -1)
        if seen(ls, rs):
            out["BodyTilt"] = math.degrees(math.atan2(ls.y - rs.y, ls.x - rs.x))
            out["BodyLeanX"] = max(-1.0, min(1.0, ((ls.x + rs.x) / 2 - 0.5) * 2))
    for side, h in hands:
        palm = max(dist(h[0], h[9]), 1e-4)
        names = ["Index", "Middle", "Ring", "Pinky"]
        opens = []
        for k, (mcp, tip) in enumerate([(5, 8), (9, 12), (13, 16), (17, 20)]):
            ratio = dist(h[tip], h[0]) / max(dist(h[mcp], h[0]), 1e-4)
            v = clamp01((ratio - 1.0) / 1.0)
            out["Finger%s%s" % (side, names[k])] = v
            opens.append(v)
        out["Finger%sThumb" % side] = clamp01((dist(h[4], h[5]) / palm - 0.2) / 0.6)
        out["Hand%sOpen" % side] = sum(opens) / 4.0
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--pose-model", help="the pose landmarker model: sends the arms and the lean of the body")
    ap.add_argument("--hand-model", help="the hand landmarker model: sends the fingers")
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
    pose_det = hand_det = None
    try:
        if args.pose_model:
            pose_det = vision.PoseLandmarker.create_from_options(
                vision.PoseLandmarkerOptions(base_options=mpp.BaseOptions(model_asset_path=args.pose_model), running_mode=vision.RunningMode.VIDEO, num_poses=1)
            )
        if args.hand_model:
            hand_det = vision.HandLandmarker.create_from_options(
                vision.HandLandmarkerOptions(base_options=mpp.BaseOptions(model_asset_path=args.hand_model), running_mode=vision.RunningMode.VIDEO, num_hands=2)
            )
    except Exception as e:
        fail("The body or hand model could not be loaded: %s" % e)
    if args.selftest:
        import numpy as np
        img = mp.Image(image_format=mp.ImageFormat.SRGB, data=np.zeros((240, 320, 3), dtype=np.uint8))
        detector.detect_for_video(img, 1)
        if pose_det:
            pose_det.detect_for_video(img, 1)
        if hand_det:
            hand_det.detect_for_video(img, 1)
        print(json.dumps({"ok": True, "pose": bool(pose_det), "hands": bool(hand_det)}), flush=True)
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
        msgs = []
        if res.face_blendshapes:
            msgs = [osc("/VMC/Ext/Blend/Val", c.category_name, float(c.score)) for c in res.face_blendshapes[0]]
        if pose_det or hand_det:
            stamp = int((now - t0) * 1000)
            pose = None
            hands = []
            if pose_det:
                pr = pose_det.detect_for_video(img, stamp)
                if pr.pose_landmarks:
                    pose = pr.pose_landmarks[0]
            if hand_det:
                hr = hand_det.detect_for_video(img, stamp)
                for lm, hd in zip(hr.hand_landmarks, hr.handedness):
                    # the label is for a mirrored picture, so the camera picture has the other side
                    label = hd[0].category_name
                    hands.append(("Right" if label == "Left" else "Left", lm))
            for name, value in body_values(pose, hands).items():
                msgs.append(osc("/Seviath/Body", name, float(value)))
        if res.face_blendshapes and res.facial_transformation_matrixes:
            m = res.facial_transformation_matrixes[0]
            yaw, pitch, roll = euler_from_matrix(m)
            msgs.append(osc("/Seviath/Head", float(yaw), float(pitch), float(roll)))
            msgs.append(osc("/Seviath/Pos", float(m[0][3]), float(m[1][3]), float(m[2][3])))
        if not msgs:
            continue
        try:
            sock.sendto(bundle(msgs), (args.host, args.port))
        except OSError:
            pass


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
