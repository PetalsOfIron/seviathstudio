# SPDX-License-Identifier: GPL-3.0-or-later
# Makes the shape of a 3D character from one picture with TripoSR (MIT licence, about 6 GB of video memory), usable everywhere. The shape only: the colours come from the drawing itself, which SeviathStudio lays on the
# shape afterwards. Run by the app as a helper program; it prints one line of JSON: {"mesh": "<path>", "vertices": n, "faces": n} or
# {"error": "..."}.
#   python shape_triposr.py --image picture.png --out shape.glb --repo C:\path\to\TripoSR [--resolution 256]
# The model (stabilityai/TripoSR, 1.7 GB) comes from the Hugging Face cache, or is downloaded the first time.
import argparse
import json
import sys
import types


def fail(message):
    print(json.dumps({"error": message}))
    sys.exit(1)


def install_marching_cubes_shim():
    """TripoSR wants `torchmcubes`, a package that has to be compiled. scikit-image does the same job (a little slower)."""
    import numpy as np
    import torch

    def marching_cubes(vol, thresh):
        from skimage import measure

        v = vol.detach().float().cpu().numpy()
        verts, faces, _, _ = measure.marching_cubes(v, level=float(thresh))
        # torchmcubes gives the points as (z, y, x); TripoSR turns them round again
        verts = np.ascontiguousarray(verts[:, ::-1])
        return torch.from_numpy(verts.astype(np.float32)), torch.from_numpy(faces.astype(np.int64))

    mod = types.ModuleType("torchmcubes")
    mod.marching_cubes = marching_cubes
    sys.modules["torchmcubes"] = mod
    # the background remover of TripoSR is not used (the drawing is already cut out), so a package that is not installed is not needed
    try:
        import rembg  # noqa: F401
    except ImportError:
        stub = types.ModuleType("rembg")
        stub.remove = lambda image, *a, **k: image
        stub.new_session = lambda *a, **k: None
        sys.modules["rembg"] = stub


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--repo", required=True, help="the folder of the TripoSR code")
    ap.add_argument("--model", default="stabilityai/TripoSR", help="a folder with config.yaml and model.ckpt, or the name on Hugging Face")
    ap.add_argument("--resolution", type=int, default=256, help="the grid of the surface (256 is good, 192 is lighter)")
    ap.add_argument("--faces", type=int, default=60000, help="fewer triangles than this are kept (0 keeps all)")
    args = ap.parse_args()
    sys.path.insert(0, args.repo)
    try:
        import numpy as np
        import torch
        import trimesh
        from PIL import Image

        install_marching_cubes_shim()
        from tsr.system import TSR
        from tsr.utils import resize_foreground
    except ImportError as e:
        fail("TripoSR is not installed here: %s" % e)
    if not torch.cuda.is_available():
        fail("No graphics card was found for torch (a card with about 6 GB is needed).")
    img = Image.open(args.image).convert("RGBA")
    try:
        # the drawing is already cut out: the character is put in the middle of a grey square, as the model was taught
        img = resize_foreground(img, 0.85)
        arr = np.array(img).astype(np.float32) / 255.0
        arr = arr[:, :, :3] * arr[:, :, 3:4] + (1 - arr[:, :, 3:4]) * 0.5
        rgb = Image.fromarray((arr * 255.0).astype(np.uint8))
        model = TSR.from_pretrained(args.model, config_name="config.yaml", weight_name="model.ckpt")
        model.renderer.set_chunk_size(8192)
        model.to("cuda")
        with torch.no_grad():
            codes = model([rgb], device="cuda")
            mesh = model.extract_mesh(codes, False, resolution=args.resolution)[0]
        del model
        torch.cuda.empty_cache()
    except Exception as e:  # out of memory, a missing download, ...
        fail("%s: %s" % (type(e).__name__, str(e)[:300]))
    # the model's own axes: turned so that the character stands up and looks at the viewer
    mesh.apply_transform(trimesh.transformations.rotation_matrix(-np.pi / 2, [1, 0, 0]))
    mesh.apply_transform(trimesh.transformations.rotation_matrix(np.pi / 2, [0, 1, 0]))
    if mesh.volume < 0:
        mesh.invert()
    if args.faces and len(mesh.faces) > args.faces:
        try:
            import pymeshlab

            ms = pymeshlab.MeshSet()
            ms.add_mesh(pymeshlab.Mesh(vertex_matrix=mesh.vertices, face_matrix=mesh.faces))
            ms.meshing_decimation_quadric_edge_collapse(targetfacenum=args.faces, preservenormal=True)
            m = ms.current_mesh()
            mesh = trimesh.Trimesh(vertices=m.vertex_matrix(), faces=m.face_matrix(), process=False)
        except Exception as e:  # keep the full mesh when the decimation is not possible
            sys.stderr.write("decimation skipped: %s\n" % e)
    mesh.export(args.out)
    print(json.dumps({"mesh": args.out, "vertices": int(len(mesh.vertices)), "faces": int(len(mesh.faces))}))


if __name__ == "__main__":
    main()
