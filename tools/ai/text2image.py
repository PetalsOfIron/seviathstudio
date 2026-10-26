# SPDX-License-Identifier: GPL-3.0-or-later
# Makes pictures of a character from a text with Stable Diffusion XL (CreativeML Open RAIL++-M licence: free to use, also for work).
# Run by the app as a helper program; it prints one line of JSON: {"images": ["<path>", ...], "seed": n} or {"error": "..."}.
#   python text2image.py --prompt "..." --negative "..." --out-dir C:\folder --count 4 --seed 123 --steps 30 --guidance 6.5 --width 832 --height 1216
# The model (stabilityai/stable-diffusion-xl-base-1.0, about 7 GB in half precision) comes from the Hugging Face cache, or is downloaded
# the first time. It runs in 8 GB of video memory by keeping the parts of the model on the processor until they are needed.
# The pose is exact: a stick figure in the standard rig pose (A-pose or T-pose) is drawn here and given to a pose ControlNet
# (xinsir/controlnet-openpose-sdxl-1.0, Apache 2.0), so every character stands the way a skeleton wants it. --pose none skips that.
import argparse
import json
import os
import sys


def fail(message):
    print(json.dumps({"error": message}))
    sys.exit(1)


# the colours of the limbs of an OpenPose picture, in the order of LIMBS
COLORS = [(255, 0, 0), (255, 85, 0), (255, 170, 0), (255, 255, 0), (170, 255, 0), (85, 255, 0), (0, 255, 0), (0, 255, 85), (0, 255, 170), (0, 255, 255), (0, 170, 255), (0, 85, 255), (0, 0, 255), (85, 0, 255), (170, 0, 255), (255, 0, 255), (255, 0, 170), (255, 0, 85)]
# the points of the 18 point body, as (across, down) in units of a head height from the middle of the top of the head
STAND = {
    0: (0.0, 0.55), 1: (0.0, 1.15), 2: (-0.85, 1.4), 3: (-1.25, 2.6), 4: (-1.65, 3.6), 5: (0.85, 1.4), 6: (1.25, 2.6), 7: (1.65, 3.6),
    8: (-0.5, 3.7), 9: (-0.55, 5.4), 10: (-0.6, 7.2), 11: (0.5, 3.7), 12: (0.55, 5.4), 13: (0.6, 7.2),
    14: (-0.17, 0.45), 15: (0.17, 0.45), 16: (-0.38, 0.5), 17: (0.38, 0.5),
}
T_ARMS = {3: (-2.3, 1.45), 4: (-3.6, 1.5), 6: (2.3, 1.45), 7: (3.6, 1.5)}
LIMBS = [(1, 2), (1, 5), (2, 3), (3, 4), (5, 6), (6, 7), (1, 8), (8, 9), (9, 10), (1, 11), (11, 12), (12, 13), (1, 0), (0, 14), (14, 16), (0, 15), (15, 17)]


def figure_frame(width, height):
    """The size of a head (the unit of the figure) and where the middle of the top of the head is: the whole body, feet included, fits with a
    margin all round."""
    unit = 0.84 * height / 7.2
    return unit, width / 2.0, 0.04 * height


def refine_face(pipe, image, width, height, prompt, negative, seed, steps, guidance):
    """The face is small in a picture of the whole body, so the eyes come out soft. The head is where the pose says it is: it is cut out,
    drawn again in a bigger picture (a light touch, so it stays the same face) and put back with soft edges."""
    import numpy as np
    import torch
    from diffusers import StableDiffusionXLImg2ImgPipeline
    from PIL import Image, ImageDraw, ImageFilter

    unit, ox, oy = figure_frame(width, height)
    side = int(2.3 * unit)
    cx, cy = int(ox), int(oy + 0.5 * unit)
    box = (max(cx - side // 2, 0), max(cy - side // 2, 0))
    box = (min(box[0], width - side), min(box[1], height - side), )
    box = (box[0], box[1], box[0] + side, box[1] + side)
    crop = image.crop(box).resize((1024, 1024), Image.LANCZOS)
    refiner = StableDiffusionXLImg2ImgPipeline(
        vae=pipe.vae, text_encoder=pipe.text_encoder, text_encoder_2=pipe.text_encoder_2, tokenizer=pipe.tokenizer,
        tokenizer_2=pipe.tokenizer_2, unet=pipe.unet, scheduler=pipe.scheduler,
    )
    gen = torch.Generator("cpu").manual_seed(seed)
    out = refiner(
        prompt=prompt + ", close-up of the face, sharp detailed eyes, detailed eyelashes and irises, clean lineart",
        negative_prompt=negative + ", blurry eyes, deformed eyes, asymmetrical eyes",
        image=crop, strength=0.38, num_inference_steps=max(steps, 24), guidance_scale=guidance, generator=gen,
    ).images[0].resize((side, side), Image.LANCZOS)
    # the edge of the patch fades, so there is no seam
    mask = Image.new("L", (side, side), 0)
    ImageDraw.Draw(mask).ellipse((side * 0.08, side * 0.04, side * 0.92, side * 0.96), fill=255)
    mask = mask.filter(ImageFilter.GaussianBlur(side * 0.07))
    result = image.copy()
    result.paste(out, box[:2], mask)
    return result


def pose_picture(kind, width, height):
    """The stick figure of a standing body (OpenPose style) that the picture is made around."""
    import cv2
    import numpy as np

    pts = dict(STAND)
    if kind == "t-pose":
        pts.update(T_ARMS)
    unit, ox, oy = figure_frame(width, height)
    xy = {k: (int(ox + v[0] * unit), int(oy + v[1] * unit)) for k, v in pts.items()}
    img = np.zeros((height, width, 3), dtype=np.uint8)
    for i, (a, b) in enumerate(LIMBS):
        (x1, y1), (x2, y2) = xy[a], xy[b]
        mid = ((x1 + x2) / 2.0, (y1 + y2) / 2.0)
        length = ((x1 - x2) ** 2 + (y1 - y2) ** 2) ** 0.5
        angle = np.degrees(np.arctan2(y1 - y2, x1 - x2))
        poly = cv2.ellipse2Poly((int(mid[0]), int(mid[1])), (int(length / 2), 4), int(angle), 0, 360, 1)
        cv2.fillConvexPoly(img, poly, tuple(int(c * 0.6) for c in COLORS[i % len(COLORS)][::-1]))
    for i, (x, y) in xy.items():
        cv2.circle(img, (x, y), 4, COLORS[i % len(COLORS)][::-1], thickness=-1)
    return img


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--prompt", required=True)
    ap.add_argument("--negative", default="")
    ap.add_argument("--out-dir", required=True)
    ap.add_argument("--count", type=int, default=4)
    ap.add_argument("--seed", type=int, default=-1, help="-1 picks one")
    ap.add_argument("--steps", type=int, default=30)
    ap.add_argument("--guidance", type=float, default=6.5)
    ap.add_argument("--width", type=int, default=832)
    ap.add_argument("--height", type=int, default=1216)
    ap.add_argument("--pose", default="a-pose", choices=["a-pose", "t-pose", "none"], help="the standard pose of the body (none: free)")
    ap.add_argument("--refine-face", type=int, default=1, help="draw the head again in a bigger picture, so the eyes are sharp (1 or 0)")
    ap.add_argument("--pose-strength", type=float, default=1.0)
    ap.add_argument("--controlnet", default="xinsir/controlnet-openpose-sdxl-1.0")
    ap.add_argument("--model", default="stabilityai/stable-diffusion-xl-base-1.0")
    ap.add_argument("--lora", default="", help="a .safetensors file with a style (optional)")
    ap.add_argument("--lora-weight", type=float, default=0.8)
    args = ap.parse_args()
    try:
        import random

        import torch
        from diffusers import ControlNetModel, StableDiffusionXLControlNetPipeline, StableDiffusionXLPipeline
        from PIL import Image
    except ImportError as e:
        fail(f"A Python package is missing ({e}). Press Download the AI files in the Rig tab, or install torch and diffusers.")
    if not torch.cuda.is_available():
        fail("No NVIDIA graphics card was found for PyTorch (CUDA).")
    width, height = args.width // 8 * 8, args.height // 8 * 8
    pose = None
    try:
        if args.pose != "none":
            net = ControlNetModel.from_pretrained(args.controlnet, torch_dtype=torch.float16)
            pipe = StableDiffusionXLControlNetPipeline.from_pretrained(args.model, controlnet=net, torch_dtype=torch.float16, variant="fp16", use_safetensors=True)
            pose = Image.fromarray(pose_picture(args.pose, width, height)[:, :, ::-1])
        elif os.path.isfile(args.model):
            pipe = StableDiffusionXLPipeline.from_single_file(args.model, torch_dtype=torch.float16)
        else:
            pipe = StableDiffusionXLPipeline.from_pretrained(args.model, torch_dtype=torch.float16, variant="fp16", use_safetensors=True)
    except Exception as e:  # noqa: BLE001
        fail(f"The picture model could not be loaded ({e}). The first run needs an internet connection (about 10 GB).")
    if args.lora:
        try:
            pipe.load_lora_weights(args.lora)
            pipe.fuse_lora(lora_scale=args.lora_weight)
        except Exception as e:  # noqa: BLE001
            fail(f"The style file could not be loaded ({e}).")
    # 8 GB of video memory (some of it is used by the desktop): the big parts are kept in 8 bit numbers and worked on in 16 bit, so the
    # picture model and the pose model fit together and nothing has to be swapped in and out at every step
    for part in (getattr(pipe, "unet", None), getattr(pipe, "controlnet", None)):
        if part is not None and hasattr(part, "enable_layerwise_casting"):
            try:
                part.enable_layerwise_casting(storage_dtype=torch.float8_e4m3fn, compute_dtype=torch.float16)
            except Exception as e:  # noqa: BLE001
                sys.stderr.write("8 bit storage skipped: %s" % e)
    pipe.enable_model_cpu_offload()
    pipe.enable_vae_tiling()
    seed = args.seed if args.seed >= 0 else random.randint(0, 2**31 - 1)
    os.makedirs(args.out_dir, exist_ok=True)
    paths = []
    for i in range(max(1, min(args.count, 8))):
        gen = torch.Generator("cpu").manual_seed(seed + i)
        try:
            extra = {"image": pose, "controlnet_conditioning_scale": args.pose_strength} if pose is not None else {}
            img = pipe(
                prompt=args.prompt,
                negative_prompt=args.negative or None,
                num_inference_steps=args.steps,
                guidance_scale=args.guidance,
                width=width,
                height=height,
                generator=gen,
                **extra,
            ).images[0]
        except torch.cuda.OutOfMemoryError:
            fail("The graphics card ran out of memory. Close other programs that use it, or make the picture smaller.")
        if pose is not None and args.refine_face:
            try:
                img = refine_face(pipe, img, width, height, args.prompt, args.negative, seed + i, args.steps, args.guidance)
            except Exception as e:  # noqa: BLE001
                sys.stderr.write("face refinement skipped: %s" % e)
        path = os.path.join(args.out_dir, f"gen_{seed + i}.png")
        img.save(path)
        paths.append(path)
    print(json.dumps({"images": paths, "seed": seed}))


if __name__ == "__main__":
    main()
