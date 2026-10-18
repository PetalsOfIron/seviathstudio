# SPDX-License-Identifier: GPL-3.0-or-later
# Makes the shape of a 3D character from one picture with Hunyuan3D 2 mini (the shape model only: the colours come from the
# drawing itself, which SeviathStudio lays on the shape afterwards). Run by the app as a helper program; it prints one line
# of JSON: {"mesh": "<path>", "vertices": n, "faces": n} or {"error": "..."}.
#   python shape3d.py --image picture.png --out shape.glb --repo C:\path\to\Hunyuan3D-2 [--turbo]
# The model is downloaded from Hugging Face the first time (about 2 GB). It needs a graphics card with about 6 GB.
import argparse
import json
import sys


def fail(message):
    print(json.dumps({"error": message}))
    sys.exit(1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--repo", required=True, help="the folder of the Hunyuan3D-2 code")
    ap.add_argument("--turbo", action="store_true", help="the faster model (fewer steps)")
    ap.add_argument("--steps", type=int, default=0)
    args = ap.parse_args()
    sys.path.insert(0, args.repo)
    try:
        import torch
        from PIL import Image
        from hy3dgen.shapegen import Hunyuan3DDiTFlowMatchingPipeline
    except ImportError as e:
        fail("Hunyuan3D is not installed here: %s" % e)
    if not torch.cuda.is_available():
        fail("No graphics card was found for torch (a card with about 6 GB is needed).")
    img = Image.open(args.image).convert("RGBA")
    # the drawing is already cut out; the model wants a square picture with the character in the middle
    side = max(img.size)
    sq = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    sq.paste(img, ((side - img.width) // 2, (side - img.height) // 2))
    sub = "hunyuan3d-dit-v2-mini-turbo" if args.turbo else "hunyuan3d-dit-v2-mini"
    try:
        pipe = Hunyuan3DDiTFlowMatchingPipeline.from_pretrained("tencent/Hunyuan3D-2mini", subfolder=sub, use_safetensors=True, device="cuda")
        if args.turbo:
            pipe.enable_flashvdm(mc_algo="mc")
        kw = {"image": sq, "generator": torch.manual_seed(12345)}
        if args.steps:
            kw["num_inference_steps"] = args.steps
        mesh = pipe(**kw)[0]
    except Exception as e:  # out of memory, a missing download, ...
        fail("%s: %s" % (type(e).__name__, str(e)[:300]))
    mesh.export(args.out)
    print(json.dumps({"mesh": args.out, "vertices": int(len(mesh.vertices)), "faces": int(len(mesh.faces))}))


if __name__ == "__main__":
    main()
