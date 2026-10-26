# SPDX-License-Identifier: GPL-3.0-or-later
# Makes a game asset (a prop, a weapon, a plant, a building piece...) from one picture with TripoSR (MIT licence): a 3D model with
# a real texture, ready for a game engine. The picture is cut out of its plain background, the model is made, turned to stand up,
# scaled to a real size (metres) with its pivot at the middle of the bottom, reduced to the number of triangles the game can afford,
# unwrapped (xatlas) and the colours are baked into a texture. Run by the app as a helper program; it prints one line of JSON:
# {"mesh": "<path>", "vertices": n, "faces": n, "texture": size} or {"error": "..."}.
#   python asset_triposr.py --image picture.png --out asset.glb --repo C:\path\to\TripoSR --height 1.0 --faces 8000 --texture 1024
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
        verts = np.ascontiguousarray(verts[:, ::-1])
        return torch.from_numpy(verts.astype(np.float32)), torch.from_numpy(faces.astype(np.int64))

    mod = types.ModuleType("torchmcubes")
    mod.marching_cubes = marching_cubes
    sys.modules["torchmcubes"] = mod
    try:
        import rembg  # noqa: F401
    except ImportError:
        stub = types.ModuleType("rembg")
        stub.remove = lambda image, *a, **k: image
        stub.new_session = lambda *a, **k: None
        sys.modules["rembg"] = stub


def cut_out(img):
    """Take the plain background away: what can be reached from the border through near-uniform colour becomes see-through."""
    import cv2
    import numpy as np

    rgba = np.array(img.convert("RGBA"))
    if (rgba[:, :, 3] < 250).mean() > 0.05:
        return img  # it is cut out already
    rgb = rgba[:, :, :3]
    h, w = rgb.shape[:2]
    corners = np.array([rgb[0, 0], rgb[0, w - 1], rgb[h - 1, 0], rgb[h - 1, w - 1]], dtype=np.float32)
    bg = np.median(corners, axis=0)
    diff = np.abs(rgb.astype(np.float32) - bg).max(axis=2)
    near = (diff < 28).astype(np.uint8)
    mask = np.zeros((h + 2, w + 2), np.uint8)
    filled = near.copy() * 255
    for seed in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)]:
        if near[seed[1], seed[0]]:
            cv2.floodFill(filled, mask, seed, 128, loDiff=0, upDiff=0)
    background = filled == 128
    alpha = np.where(background, 0, 255).astype(np.uint8)
    alpha = cv2.morphologyEx(alpha, cv2.MORPH_OPEN, np.ones((3, 3), np.uint8))
    alpha = cv2.GaussianBlur(alpha, (3, 3), 0)
    rgba[:, :, 3] = alpha
    from PIL import Image

    return Image.fromarray(rgba, "RGBA")


def drop_floaters(mesh):
    """The AI sometimes leaves small loose pieces (a bit of the ground shadow, specks) next to the model: only the big pieces stay."""
    import trimesh

    parts = mesh.split(only_watertight=False)
    if len(parts) < 2:
        return mesh
    biggest = max(len(p.faces) for p in parts)
    keep = [p for p in parts if len(p.faces) >= 0.04 * biggest]
    return trimesh.util.concatenate(keep) if len(keep) > 1 else keep[0]


def decimate(mesh, faces):
    import numpy as np
    import pymeshlab
    import trimesh

    ms = pymeshlab.MeshSet()
    colors = np.asarray(mesh.visual.vertex_colors, dtype=np.float64) / 255.0
    ms.add_mesh(pymeshlab.Mesh(vertex_matrix=mesh.vertices, face_matrix=mesh.faces, v_color_matrix=colors))
    ms.meshing_decimation_quadric_edge_collapse(targetfacenum=faces, preservenormal=True, preservetopology=False, qualitythr=0.5)
    m = ms.current_mesh()
    out = trimesh.Trimesh(vertices=m.vertex_matrix(), faces=m.face_matrix(), process=False)
    out.visual.vertex_colors = (np.clip(m.vertex_color_matrix(), 0, 1) * 255).astype(np.uint8)
    return out


def bake(mesh, size):
    """Unwrap the mesh and paint the colours of its points into a texture of size x size."""
    import cv2
    import numpy as np
    import trimesh
    import xatlas
    from PIL import Image

    colors = np.asarray(mesh.visual.vertex_colors)[:, :3].astype(np.float32)
    vmapping, indices, uvs = xatlas.parametrize(mesh.vertices.astype(np.float32), mesh.faces.astype(np.uint32))
    verts = mesh.vertices[vmapping]
    cols = colors[vmapping]
    tex = np.zeros((size, size, 3), np.float32)
    got = np.zeros((size, size), np.uint8)
    px = uvs * (size - 1)
    for tri in indices:
        p = px[tri]
        c = cols[tri]
        x0, y0 = np.floor(p.min(axis=0)).astype(int)
        x1, y1 = np.ceil(p.max(axis=0)).astype(int)
        x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, size - 1), min(y1, size - 1)
        if x1 < x0 or y1 < y0:
            continue
        xs, ys = np.meshgrid(np.arange(x0, x1 + 1), np.arange(y0, y1 + 1))
        d = (p[1][1] - p[2][1]) * (p[0][0] - p[2][0]) + (p[2][0] - p[1][0]) * (p[0][1] - p[2][1])
        if abs(d) < 1e-9:
            continue
        a = ((p[1][1] - p[2][1]) * (xs - p[2][0]) + (p[2][0] - p[1][0]) * (ys - p[2][1])) / d
        b = ((p[2][1] - p[0][1]) * (xs - p[2][0]) + (p[0][0] - p[2][0]) * (ys - p[2][1])) / d
        g = 1 - a - b
        inside = (a >= -0.02) & (b >= -0.02) & (g >= -0.02)
        if not inside.any():
            continue
        col = a[..., None] * c[0] + b[..., None] * c[1] + g[..., None] * c[2]
        tex[ys[inside], xs[inside]] = col[inside]
        got[ys[inside], xs[inside]] = 255
    # the edges of the pieces: the colour next to them, so the filtering of the engine does not show seams
    tex8 = np.clip(tex, 0, 255).astype(np.uint8)
    for _ in range(4):
        grown = cv2.dilate(tex8, np.ones((3, 3), np.uint8))
        tex8 = np.where(got[..., None] > 0, tex8, grown)
        got = cv2.dilate(got, np.ones((3, 3), np.uint8))
    img = Image.fromarray(tex8, "RGB")
    uv = uvs.copy()
    uv[:, 1] = 1.0 - uv[:, 1]
    out = trimesh.Trimesh(vertices=verts, faces=indices, process=False)
    out.visual = trimesh.visual.TextureVisuals(uv=uv, image=img, material=trimesh.visual.material.PBRMaterial(baseColorTexture=img, metallicFactor=0.0, roughnessFactor=0.85))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--repo", required=True, help="the folder of the TripoSR code")
    ap.add_argument("--model", default="stabilityai/TripoSR")
    ap.add_argument("--resolution", type=int, default=256)
    ap.add_argument("--height", type=float, default=1.0, help="how tall it is in metres (0 keeps the size of the model)")
    ap.add_argument("--faces", type=int, default=8000, help="the number of triangles the game can afford (0 keeps all)")
    ap.add_argument("--texture", type=int, default=1024, help="the size of the texture picture")
    ap.add_argument("--pivot", default="bottom", choices=["bottom", "centre"], help="where the model is held")
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
        import xatlas  # noqa: F401
    except ImportError as e:
        fail("A Python package is missing here: %s (the AI files of the Rig tab install TripoSR, trimesh and pymeshlab; xatlas too)." % e)
    if not torch.cuda.is_available():
        fail("No graphics card was found for torch (a card with about 6 GB is needed).")
    try:
        img = cut_out(Image.open(args.image))
        img = resize_foreground(img.convert("RGBA"), 0.85)
        arr = np.array(img).astype(np.float32) / 255.0
        arr = arr[:, :, :3] * arr[:, :, 3:4] + (1 - arr[:, :, 3:4]) * 0.5
        rgb = Image.fromarray((arr * 255.0).astype(np.uint8))
        model = TSR.from_pretrained(args.model, config_name="config.yaml", weight_name="model.ckpt")
        model.renderer.set_chunk_size(8192)
        model.to("cuda")
        with torch.no_grad():
            codes = model([rgb], device="cuda")
            mesh = model.extract_mesh(codes, True, resolution=args.resolution)[0]
        del model
        torch.cuda.empty_cache()
    except Exception as e:  # out of memory, a missing download, ...
        fail("%s: %s" % (type(e).__name__, str(e)[:300]))
    # the model's own axes: turned so that the prop stands up with its front to the viewer
    mesh.apply_transform(trimesh.transformations.rotation_matrix(-np.pi / 2, [1, 0, 0]))
    mesh.apply_transform(trimesh.transformations.rotation_matrix(np.pi / 2, [0, 1, 0]))
    if mesh.volume < 0:
        mesh.invert()
    try:
        mesh = drop_floaters(mesh)
    except Exception as e:  # keep everything when the pieces cannot be told apart
        sys.stderr.write("loose pieces kept: %s" % e)
    try:
        if args.faces and len(mesh.faces) > args.faces:
            mesh = decimate(mesh, args.faces)
    except Exception as e:  # keep the full mesh when the reduction is not possible
        sys.stderr.write("reduction skipped: %s\n" % e)
    # real size and the pivot
    lo, hi = mesh.bounds
    size = hi - lo
    if args.height > 0 and size[1] > 0:
        mesh.apply_scale(args.height / size[1])
        lo, hi = mesh.bounds
    centre = [(lo[0] + hi[0]) / 2.0, lo[1] if args.pivot == "bottom" else (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]
    mesh.apply_translation([-centre[0], -centre[1], -centre[2]])
    try:
        mesh = bake(mesh, args.texture)
    except Exception as e:
        fail("The texture could not be made: %s: %s" % (type(e).__name__, str(e)[:300]))
    mesh.export(args.out)
    print(json.dumps({"mesh": args.out, "vertices": int(len(mesh.vertices)), "faces": int(len(mesh.faces)), "texture": args.texture}))


if __name__ == "__main__":
    main()
