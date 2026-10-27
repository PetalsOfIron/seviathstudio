# SPDX-License-Identifier: GPL-3.0-or-later
# Makes the shape of a 3D character from one picture with CharacterGen (Apache 2.0, https://github.com/zjp-shadow/CharacterGen): it was made for
# anime characters. It first draws the character from the back, the front and both sides in an A-pose (arms away from the body), and then builds
# the mesh from those four pictures, so it knows where the arms and the hair are. The colours come from the drawing itself, which SeviathStudio
# lays on the shape afterwards. Run by the app as a helper program; it prints one line of JSON: {"mesh": "<path>", ...} or {"error": "..."}.
#   python shape_charactergen.py --image picture.png --out shape.glb --repo C:\path\to\CharacterGen-code [--seed 2333] [--steps 40]
# The picture should have a see-through (or plain) background. It needs the Python of its own environment (torch 2.1, diffusers 0.24, nvdiffrast).
# The first run downloads Stable Diffusion 2.1 (for its parts) and a small background remover from Hugging Face.
import argparse
import json
import os
import sys
import types


def fail(message):
    print(json.dumps({"error": message}))
    sys.exit(1)


def install_stubs():
    """webui.py was made for a web page and for a package that is not on PyPI: the page is not needed, and the background remover is written here."""
    import cv2
    import numpy as np

    gradio = types.ModuleType("gradio")
    gradio.Blocks = gradio.Markdown = gradio.Row = gradio.Column = gradio.Image = gradio.Number = gradio.Checkbox = gradio.Slider = gradio.Button = gradio.Textbox = gradio.Tab = gradio.Model3D = gradio.Examples = lambda *a, **k: None
    sys.modules["gradio"] = gradio
    pkg = types.ModuleType("rm_anime_bg")
    cli = types.ModuleType("rm_anime_bg.cli")
    cli.SCALE = 255

    def get_mask(session, img, s=1024):
        img = (img / 255).astype(np.float32)
        h, w = h0, w0 = img.shape[:-1]
        h, w = (s, int(s * w / h)) if h > w else (int(s * h / w), s)
        ph, pw = s - h, s - w
        img_input = np.zeros([s, s, 3], dtype=np.float32)
        img_input[ph // 2: ph // 2 + h, pw // 2: pw // 2 + w] = cv2.resize(img, (w, h))
        img_input = np.transpose(img_input, (2, 0, 1))[np.newaxis, :]
        mask = session.run(None, {"img": img_input})[0][0]
        mask = np.transpose(mask, (1, 2, 0))
        mask = mask[ph // 2: ph // 2 + h, pw // 2: pw // 2 + w]
        return cv2.resize(mask, (w0, h0))[:, :, np.newaxis]

    cli.get_mask = get_mask
    pkg.cli = cli
    sys.modules["rm_anime_bg"] = pkg
    sys.modules["rm_anime_bg.cli"] = cli
    # webui.py asks the Hugging Face hub which files exist at the start; they are all here already
    import huggingface_hub

    huggingface_hub.list_repo_files = lambda *a, **k: []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--repo", required=True, help="the folder of the CharacterGen code (with its models)")
    ap.add_argument("--seed", type=int, default=2333)
    ap.add_argument("--steps", type=int, default=40)
    ap.add_argument("--smooth", type=int, default=3, help="how many times the surface is smoothed")
    ap.add_argument("--sd21", default="sd2-community/stable-diffusion-2-1", help="where Stable Diffusion 2.1 is (the first-party repository was taken down; this mirror is the same model)")
    ap.add_argument("--offload", default="model", choices=["model", "sequential", "none"], help="keep the parts of the 2D stage in the main memory until they are needed")
    ap.add_argument("--views-in", default="", help="a folder with right.png, back.png, left.png and front.png from an earlier run: the drawing stage is skipped")
    ap.add_argument("--views-out", default="", help="a folder to keep the four pictures it drew (for looking at)")
    args = ap.parse_args()
    if not os.path.isdir(os.path.join(args.repo, "2D_Stage", "models")) or not os.path.isdir(os.path.join(args.repo, "3D_Stage", "models")):
        fail("The models of CharacterGen are not in %s." % args.repo)
    os.chdir(args.repo)
    sys.path.insert(0, args.repo)
    try:
        install_stubs()
        import numpy as np
        import torch
        from omegaconf import OmegaConf
        from PIL import Image

        if not torch.cuda.is_available():
            fail("No graphics card was found for torch.")
        import webui as W
    except ImportError as e:
        fail("A package of CharacterGen is missing: %s" % e)
    except SystemExit:
        raise
    except Exception as e:  # noqa: BLE001
        fail("CharacterGen could not start: %s: %s" % (type(e).__name__, str(e)[:300]))
    try:
        if args.views_in:
            # the four pictures are there already (from an earlier run): only the 3D stage is done
            views = [Image.open(os.path.join(args.views_in, n + ".png")).convert("RGBA") for n in ("right", "back", "left", "front")]
        else:
            image = Image.open(args.image).convert("RGBA")
            remover = W.rm_bg_api()
            cfg2d = OmegaConf.load("./2D_Stage/configs/infer.yaml")
            cfg2d.pretrained_model_path = args.sd21
            api2d = W.Inference2D_API(**cfg2d)
            # 8 GB of video memory (the desktop uses some) is not enough for the two networks and four views at once: the parts wait in the main
            # memory until they are needed. Without this the card swaps and a step takes minutes.
            if args.offload != "none":
                pipe = api2d.validation_pipeline
                try:
                    if args.offload == "sequential":
                        pipe.enable_sequential_cpu_offload()
                    else:
                        pipe.enable_model_cpu_offload()
                except Exception as e:  # noqa: BLE001
                    sys.stderr.write("offloading skipped: %s" % e)
            views = api2d.inference(image, 512, 768, crop=True, seed=args.seed, timestep=args.steps)
            views = remover.remove_background(imgs=views, alpha_min=0.2, alpha_max=0.9)
            del api2d
            torch.cuda.empty_cache()
        # the order the pictures come in is right, back, left, front (as the web page of CharacterGen hands them on)
        right, back, left, front = views[0], views[1], views[2], views[3]
        if args.views_out:
            os.makedirs(args.views_out, exist_ok=True)
            for name, v in (("right", right), ("back", back), ("left", left), ("front", front)):
                v.save(os.path.join(args.views_out, name + ".png"))
        api3d = W.Inference3D_API()
        save_dir, _obj, glb = api3d.process_images(back, front, right, left, True, args.smooth)
    except torch.cuda.OutOfMemoryError:
        fail("The graphics card ran out of memory. Close other programs that use it.")
    except Exception as e:  # noqa: BLE001
        import traceback

        sys.stderr.write(traceback.format_exc())
        fail("%s: %s" % (type(e).__name__, str(e)[:300]))
    import trimesh

    mesh = trimesh.load(glb, force="mesh", process=False)
    # CharacterGen faces the other way (its front is -z): turned half a round about the up axis, so the front looks to +z as for the other shape AI
    mesh.apply_transform(trimesh.transformations.rotation_matrix(3.141592653589793, [0, 1, 0]))
    mesh.export(args.out)
    print(json.dumps({"mesh": args.out, "vertices": int(len(mesh.vertices)), "faces": int(len(mesh.faces)), "folder": save_dir}))


if __name__ == "__main__":
    main()
