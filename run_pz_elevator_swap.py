#!/usr/bin/env python3
"""
PZ face-swapping workflow for 'Elevator Mirror Check' scene.
1. Generate base image with Gemini 2.5 Flash
2. Run ComfyUI face-swap (CLIPSeg face mask + PZ FLUX LoRA fill)
3. Save final image and log status.
"""
import json
import os
import random
import sys
import time
from pathlib import Path

# Project paths
PROJECT_ROOT = Path(__file__).resolve().parent
WORKSPACE_SHARED = PROJECT_ROOT / "workspace" / "shared"
IMAGES_DIR = PROJECT_ROOT / "images" / "2026-03-18"
ENV_PATH = PROJECT_ROOT / "workspace" / "skills" / "nano-banana-2" / ".env"
TEMPLATE_PATH = PROJECT_ROOT / "workspace" / "shared" / "workspace" / "skills" / "comfyui" / "templates" / "image_flux.1_fill_dev_OneReward.json"
STATUS_LOG = WORKSPACE_SHARED / "cursor_agent_status.log"

COMFYUI_BASE = "http://10.0.1.217:8188"
BASE_IMAGE_NAME = "pz_elevator_base.png"
FINAL_IMAGE_NAME = "pz_elevator_final.png"

GEMINI_PROMPT = (
    "A high-end cinematic shot of a young Chinese woman, PZ, with a soft oval face, full cheeks, "
    "and large horizontally almond-shaped dark brown eyes. She's wearing large thin gold metal-frame glasses "
    "and has long straight dark hair with see-through bangs. She is in a luxurious elevator with mirrored walls. "
    "She's doing a 'mirror check', slightly lifting the hem of her short black leather skirt, revealing a subtle, "
    "accidental peek of her thong. Her expression is a mix of concentration and confidence. "
    "Warm lighting, realistic photography, 4k resolution."
)


def log_status(msg: str) -> None:
    """Append a timestamped line to cursor_agent_status.log."""
    STATUS_LOG.parent.mkdir(parents=True, exist_ok=True)
    ts = time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime())
    line = f"[{ts}] {msg}\n"
    with open(STATUS_LOG, "a") as f:
        f.write(line)
    print(msg)


def load_dotenv(path: Path) -> None:
    """Load KEY=VALUE from path into os.environ."""
    if not path.exists():
        return
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                os.environ[k.strip()] = v.strip().strip('"')


def generate_base_image() -> bool:
    """Generate base image with Gemini 2.5 Flash; save to IMAGES_DIR/pz_elevator_base.png."""
    load_dotenv(ENV_PATH)
    api_key = os.environ.get("GEMINI_API_KEY")
    if not api_key:
        log_status("ERROR: GEMINI_API_KEY not found in .env")
        return False

    try:
        from google import genai
        from google.genai import types
    except ImportError:
        log_status("ERROR: google-genai not installed (pip install google-genai)")
        return False

    IMAGES_DIR.mkdir(parents=True, exist_ok=True)
    out_path = IMAGES_DIR / BASE_IMAGE_NAME

    log_status("Step 1: Generating base image with Gemini 2.5 Flash (gemini-2.5-flash-image)...")
    client = genai.Client(api_key=api_key)
    response = client.models.generate_content(
        model="gemini-2.5-flash-image",
        contents=[GEMINI_PROMPT],
        config=types.GenerateContentConfig(
            image_config=types.ImageConfig(
                aspect_ratio="9:16",
                image_size="4K",
            )
        ),
    )

    for part in response.parts:
        if part.inline_data:
            img = part.as_image()
            img.save(out_path)
            log_status(f"Base image saved: {out_path}")
            return True

    log_status("ERROR: No image data in Gemini response")
    return False


def upload_image_to_comfy(image_path: Path) -> str | None:
    """Upload image to ComfyUI; return filename for use in workflow (e.g. 'input/pz_elevator_base.png')."""
    try:
        import urllib.request
        from urllib.request import Request, urlopen
    except ImportError:
        log_status("ERROR: urllib not available")
        return None

    url = f"{COMFYUI_BASE}/upload/image"
    with open(image_path, "rb") as f:
        data = f.read()

    # ComfyUI: multipart/form-data with 'image' file and optional 'subfolder'
    boundary = "----ComfyUploadBoundary"
    b = boundary.encode()
    crlf = b"\r\n"
    parts = [
        b"--" + b + crlf,
        b'Content-Disposition: form-data; name="image"; filename="' + image_path.name.encode() + b'"' + crlf,
        b"Content-Type: image/png" + crlf + crlf,
        data,
        crlf + b"--" + b + crlf,
        b'Content-Disposition: form-data; name="subfolder"' + crlf + crlf,
        b"input" + crlf,
        b"--" + b + b"--" + crlf,
    ]
    body = b"".join(parts)

    req = Request(url, data=body, method="POST")
    req.add_header("Content-Type", "multipart/form-data; boundary=" + boundary)
    try:
        with urlopen(req, timeout=60) as resp:
            r = json.loads(resp.read().decode())
            name = r.get("name", image_path.name)
            sub = r.get("subfolder", "input")
            # LoadImage often expects "subfolder/filename" or "filename [input]"
            return f"{sub}/{name}" if sub else name
    except Exception as e:
        log_status(f"ERROR: ComfyUI upload failed: {e}")
        return None


def build_workflow_with_clipseg_mask(
    workflow: dict,
    image_ref: str,
    seed: int,
    clipseg_prompt: str = "face",
) -> dict:
    """
    Modify workflow to:
    - Load image by reference (image_ref, e.g. 'input/pz_elevator_base.png')
    - Add CLIPSeg node to mask 'face' and use that mask for inpainting
    - Set KSampler seed
    """
    w = json.loads(json.dumps(workflow))

    # Node 17: LoadImage - use uploaded image
    w["17"]["inputs"]["image"] = image_ref

    # KSampler: random seed
    w["3"]["inputs"]["seed"] = seed

    # Add CLIPSeg node (node id "clipseg_face") to produce mask from image + text
    # ComfyUI_essentials / Impact Pack style: CLIPSeg with image + text -> mask
    # If the server uses a different node, this may need to be adjusted.
    w["clipseg_face"] = {
        "inputs": {
            "image": ["17", 0],
            "text": clipseg_prompt,
        },
        "class_type": "CLIPSeg",
        "_meta": {"title": "CLIPSeg"},
    }

    # InpaintModelConditioning (38): use CLIPSeg mask instead of LoadImage mask
    w["38"]["inputs"]["mask"] = ["clipseg_face", 0]
    # GrowMask (48:199): same
    w["48:199"]["inputs"]["mask"] = ["clipseg_face", 0]

    # Save the composited image (node 50), not just the decoded patch (node 8)
    w["107"]["inputs"]["images"] = ["50", 0]

    return w


def queue_prompt(workflow: dict):
    """Post workflow to ComfyUI /prompt and return prompt_id."""
    import urllib.request

    url = f"{COMFYUI_BASE}/prompt"
    payload = {"prompt": workflow, "client_id": "pz_elevator_swap"}
    data = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=data, method="POST")
    req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=30) as resp:
        out = json.loads(resp.read().decode())
    return out.get("prompt_id"), out.get("error")


def get_history(prompt_id: str) -> dict | None:
    """Fetch /history/{prompt_id} and return output images info."""
    import urllib.request

    url = f"{COMFYUI_BASE}/history/{prompt_id}"
    req = urllib.request.Request(url, method="GET")
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode())
    return data.get(prompt_id)


def download_output_image(node_id: str, filename: str, subfolder: str, save_path: Path) -> bool:
    """Download image from ComfyUI view?filename=...&subfolder=...&type=output."""
    import urllib.parse
    import urllib.request

    # ComfyUI serves output files at /view?filename=...&subfolder=...&type=output
    q = urllib.parse.urlencode({"filename": filename, "subfolder": subfolder or "", "type": "output"})
    url = f"{COMFYUI_BASE}/view?{q}"
    try:
        req = urllib.request.Request(url, method="GET")
        with urllib.request.urlopen(req, timeout=60) as resp:
            with open(save_path, "wb") as f:
                f.write(resp.read())
        return True
    except Exception as e:
        log_status(f"ERROR: Download failed: {e}")
        return False


def run_face_swap(base_image_path: Path) -> bool:
    """Upload base image, run ComfyUI workflow with CLIPSeg face mask and PZ LoRA, save result."""
    if not TEMPLATE_PATH.exists():
        log_status(f"ERROR: Template not found: {TEMPLATE_PATH}")
        return False

    with open(TEMPLATE_PATH) as f:
        template = json.load(f)

    log_status("Uploading base image to ComfyUI...")
    image_ref = upload_image_to_comfy(base_image_path)
    if not image_ref:
        return False
    log_status(f"Uploaded as: {image_ref}")

    seed = random.randint(0, 2**53 - 1)
    workflow = build_workflow_with_clipseg_mask(template, image_ref, seed, clipseg_prompt="face")

    # Ensure output node saves to a known path so we can find filename from history
    # Template has node 107 Image Save with output_path "[time(%Y-%m-%d)]" - we'll get from history

    log_status("Queueing ComfyUI prompt (CLIPSeg face mask + PZ LoRA fill)...")
    prompt_id, err = queue_prompt(workflow)
    if err:
        log_status(f"ERROR: ComfyUI queue error: {err}")
        return False
    if not prompt_id:
        log_status("ERROR: No prompt_id from ComfyUI")
        return False

    log_status(f"Waiting for ComfyUI run (prompt_id={prompt_id})...")
    for _ in range(120):
        time.sleep(1)
        hist = get_history(prompt_id)
        if not hist:
            continue
        outputs = hist.get("outputs", {})
        for nid, out in outputs.items():
            imgs = out.get("images", [])
            if not imgs:
                continue
            img_info = imgs[0]
            filename = img_info.get("filename")
            subfolder = img_info.get("subfolder", "")
            if filename:
                save_path = IMAGES_DIR / FINAL_IMAGE_NAME
                if download_output_image(nid, filename, subfolder, save_path):
                    log_status(f"Final image saved: {save_path}")
                    return True
                break
        if outputs:
            break
    else:
        log_status("ERROR: ComfyUI run did not produce output in time")
        return False
    return False


def main() -> int:
    log_status("--- PZ face-swapping workflow: Elevator Mirror Check ---")

    base_path = IMAGES_DIR / BASE_IMAGE_NAME
    if not base_path.exists():
        if not generate_base_image():
            return 1
    else:
        log_status(f"Using existing base image: {base_path}")

    log_status("Step 2: Running face-swap on ComfyUI (CLIPSeg face + PZ FLUX LoRA 0.5)...")
    if not run_face_swap(base_path):
        return 1

    log_status("--- PZ Elevator Mirror Check workflow completed successfully ---")
    return 0


if __name__ == "__main__":
    sys.exit(main())
