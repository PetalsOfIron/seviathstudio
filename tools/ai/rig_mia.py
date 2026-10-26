# SPDX-License-Identifier: GPL-3.0-or-later
# Skin weights for a 3D character shape from Make-It-Animatable (MIT licence, https://github.com/jasongzy/Make-It-Animatable): the AI looks at
# the shape and says how much every bone moves every point (Mixamo's 52 bones: the spine, the arms, the legs, every finger). SeviathStudio
# uses the weights for its own skeleton. Run by the app as a helper program; it prints one line of JSON:
#   {"names": ["Hips", "Spine", ...], "points": n, "weights": [[[bone, w], [bone, w], ...] for every point]} or {"error": "..."}
#   python rig_mia.py --mia C:\path\to\Make-It-Animatable --mesh shape.glb --out weights.json
# It needs the Python of the Make-It-Animatable folder (its own environment) and its models (output/best/new) and data/Mixamo/bones.fbx.
import argparse
import json
import os
import sys


def fail(message):
    print(json.dumps({"error": message}))
    sys.exit(1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mia", required=True)
    ap.add_argument("--mesh", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--keep", type=int, default=4, help="how many bones a point may follow")
    args = ap.parse_args()
    if not os.path.isdir(os.path.join(args.mia, "output", "best", "new")):
        fail("The models of Make-It-Animatable are not in %s (output/best/new)." % args.mia)
    sys.path.insert(0, args.mia)
    os.chdir(args.mia)
    try:
        import gradio as gr

        # the program was made for a web page: its messages are printed instead
        for n in ("Info", "Warning", "Success"):
            setattr(gr, n, lambda *a, **k: print("[mia]", *a, file=sys.stderr, flush=True))
        import numpy as np
        import torch

        import app
    except ImportError as e:
        fail("A package of Make-It-Animatable is missing: %s" % e)
    if not torch.cuda.is_available():
        fail("No graphics card was found for torch.")
    try:
        app.init_models()
        app.init_blocks()  # makes the names its steps hand their answers to
    except Exception as e:  # noqa: BLE001
        fail("Make-It-Animatable could not start: %s: %s" % (type(e).__name__, str(e)[:300]))
    try:
        db = app.DB()
        app.clear(db)
        app.prepare_input(args.mesh, False, 0.0, db, False)
        app.preprocess(db)
        app.infer(False, db)
        app.vis(True, "LeftArm", False, db)
        bw = db.bw
        if hasattr(bw, "detach"):
            bw = bw.detach().cpu().numpy()
        bw = np.asarray(bw)
        if bw.ndim == 3:
            bw = bw[0]
        names = [n for n, _ in sorted(app.bones_idx_dict_joints.items(), key=lambda kv: kv[1])]
        names = [n.replace(app.MIXAMO_PREFIX, "") for n in names]
        # where the bones are, in the coordinates of the mesh that was given (not the normalized ones the AI works in)
        def tensor(a):
            a = a.detach().cpu().numpy() if hasattr(a, "detach") else np.asarray(a)
            return torch.as_tensor(a, dtype=torch.float32)

        inv = db.global_transform.inverse()
        heads = inv.transform_points(tensor(db.joints)[..., :3].reshape(1, -1, 3)).reshape(-1, 3).cpu().numpy()
        tails = inv.transform_points(tensor(db.joints_tail).reshape(1, -1, 3)).reshape(-1, 3).cpu().numpy()
        # a check that the way back is right: the normalized points go back to the points of the mesh
        back = inv.transform_points(tensor(db.verts).reshape(1, -1, 3)).reshape(-1, 3).cpu().numpy()
        orig = np.asarray(app.trimesh.load(args.mesh, force="mesh").vertices)
        if back.shape == orig.shape and float(np.abs(back - orig).max()) > 1e-2 * max(1.0, float(np.abs(orig).max())):
            sys.stderr.write("the way back from the normalized points is not exact")
    except Exception as e:  # noqa: BLE001
        fail("%s: %s" % (type(e).__name__, str(e)[:300]))
    keep = max(1, args.keep)
    top = np.argsort(-bw, axis=1)[:, :keep]
    weights = []
    for i in range(bw.shape[0]):
        row = [(int(j), float(bw[i, j])) for j in top[i] if bw[i, j] > 0.003]
        total = sum(w for _, w in row) or 1.0
        weights.append([[j, round(w / total, 4)] for j, w in row])
    with open(args.out, "w") as f:
        json.dump({"names": names, "points": int(bw.shape[0]), "weights": weights, "heads": heads.round(5).tolist(), "tails": tails.round(5).tolist()}, f)
    print(json.dumps({"out": args.out, "points": int(bw.shape[0]), "bones": len(names)}))


if __name__ == "__main__":
    main()
