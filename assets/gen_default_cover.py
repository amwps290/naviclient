#!/usr/bin/env python3
"""Rasterize `assets/default-cover.svg` into `default-cover.png` and its mipmap pyramid.

Prefers librsvg/pycairo when available and falls back to a local headless
Chromium browser. Adds a subtle fine-art cotton-paper grain so gradients stay
silky and organic. The client embeds the generated PNGs; neither Python nor
librsvg/Chromium is needed when building or running the client.
"""

import argparse
import os
import random
from pathlib import Path
import shutil
import subprocess
import tempfile

from PIL import Image


ROOT = Path(__file__).resolve().parent
SIZE = 1024
RASTER_SIZE = SIZE * 2
MIP_SIZES = (64, 128, 256, 512)


def add_subtle_paper_grain(image: Image.Image, amplitude: int = 3) -> Image.Image:
    """Add deterministic ultra-subtle fine-art paper texture (keeps alpha = 255)."""
    rgb = image.convert("RGB")
    rng = random.Random(20261128)
    w, h = rgb.size
    src = rgb.tobytes()
    out = bytearray(len(src))
    for i in range(0, len(src), 3):
        delta = rng.randint(-amplitude, amplitude)
        r = src[i] + delta
        g = src[i + 1] + delta
        b = src[i + 2] + delta
        out[i] = 0 if r < 0 else (255 if r > 255 else r)
        out[i + 1] = 0 if g < 0 else (255 if g > 255 else g)
        out[i + 2] = 0 if b < 0 else (255 if b > 255 else b)
    grained = Image.frombytes("RGB", (w, h), bytes(out)).convert("RGBA")
    return grained


def save_mipmaps(cover, output):
    if cover.size != (SIZE, SIZE) or cover.convert("RGBA").getextrema()[3] != (255, 255):
        raise ValueError("Expected an opaque 1024 x 1024 default cover.")
    for side in MIP_SIZES:
        path = output.with_name(f"{output.stem}-{side}{output.suffix}")
        cover.resize((side, side), Image.Resampling.LANCZOS).convert("RGB").save(path, optimize=True)
        print(f"Saved {path} ({side} x {side})")


def render_with_rsvg(svg_path: Path):
    import cairo
    import gi

    gi.require_version("Rsvg", "2.0")
    from gi.repository import Rsvg

    handle = Rsvg.Handle.new_from_file(str(svg_path))
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, RASTER_SIZE, RASTER_SIZE)
    ctx = cairo.Context(surface)
    viewport = Rsvg.Rectangle()
    viewport.x = 0.0
    viewport.y = 0.0
    viewport.width = float(RASTER_SIZE)
    viewport.height = float(RASTER_SIZE)
    handle.render_document(ctx, viewport)
    surface.flush()
    raw = bytes(surface.get_data())
    img = Image.frombuffer("RGBA", (RASTER_SIZE, RASTER_SIZE), raw, "raw", "BGRa", 0, 1).copy()
    return img.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def find_browser(override):
    if override:
        candidate = shutil.which(override) or override
        if Path(candidate).is_file():
            return str(candidate)
        raise FileNotFoundError(f"Browser not found: {override}")
    for name in ("msedge", "chromium", "chromium-browser", "google-chrome", "chrome"):
        if candidate := shutil.which(name):
            return candidate
    candidates = [
        Path(os.environ.get("PROGRAMFILES(X86)", "C:/Program Files (x86)"))
        / "Microsoft/Edge/Application/msedge.exe",
        Path(os.environ.get("PROGRAMFILES", "C:/Program Files"))
        / "Google/Chrome/Application/chrome.exe",
        Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        Path("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
    ]
    for candidate in candidates:
        if candidate.is_file():
            return str(candidate)
    raise FileNotFoundError("Install librsvg/pycairo or Chrome/Chromium/Edge, or pass --browser PATH.")


def render_with_browser(browser, svg_path: Path):
    svg = svg_path.read_text(encoding="utf-8")
    with tempfile.TemporaryDirectory(prefix="naviclient-cover-", ignore_cleanup_errors=True) as folder:
        temporary = Path(folder)
        document = temporary / "cover.html"
        screenshot = temporary / "cover.png"
        document.write_text(
            '<!doctype html><meta charset="utf-8"><style>'
            f'html,body{{margin:0;width:{RASTER_SIZE}px;height:{RASTER_SIZE}px;overflow:hidden;}}'
            f'svg{{display:block;width:{RASTER_SIZE}px;height:{RASTER_SIZE}px;}}'
            '</style>' + svg,
            encoding="utf-8",
        )
        command = [
            browser, "--headless=new", "--no-first-run", "--no-default-browser-check",
            "--disable-background-networking", "--force-device-scale-factor=1",
            "--hide-scrollbars", "--run-all-compositor-stages-before-draw",
            "--virtual-time-budget=1000",
            f"--window-size={RASTER_SIZE},{RASTER_SIZE + 200}",
            f"--user-data-dir={temporary / 'profile'}",
            f"--screenshot={screenshot}", document.as_uri(),
        ]
        result = subprocess.run(
            command, capture_output=True, timeout=60,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        if result.returncode != 0 or not screenshot.is_file():
            diagnostic = result.stderr.decode("utf-8", errors="replace")[-1000:]
            raise RuntimeError(f"Cover rasterization failed: {diagnostic}")
        with Image.open(screenshot) as image:
            if image.width != RASTER_SIZE or image.height < RASTER_SIZE:
                raise ValueError(f"Unexpected browser raster size: {image.size}")
            cover = image.crop((0, 0, RASTER_SIZE, RASTER_SIZE)).convert("RGBA")
            return cover.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def generate(browser_override, output):
    svg_path = ROOT / "default-cover.svg"
    cover = None
    if not browser_override:
        try:
            cover = render_with_rsvg(svg_path)
        except Exception:
            cover = None
    if cover is None:
        cover = render_with_browser(find_browser(browser_override), svg_path)
    cover = add_subtle_paper_grain(cover, amplitude=1)
    if cover.getextrema()[3] != (255, 255):
        raise ValueError("Default cover must have an opaque background.")
    # Save opaque RGB PNGs (smaller embedded binary; Rust decoder loads to RGBA with alpha=255).
    cover.convert("RGB").save(output, optimize=True)
    save_mipmaps(cover, output)
    print(f"Saved {output} ({SIZE} x {SIZE})")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--browser", help="Path to Chrome, Chromium, or Edge")
    parser.add_argument("--output", type=Path, default=ROOT / "default-cover.png")
    parser.add_argument("--mips-only", action="store_true", help="Generate smaller levels from the existing PNG")
    arguments = parser.parse_args()
    output = arguments.output.resolve()
    if arguments.mips_only:
        with Image.open(output) as cover:
            save_mipmaps(cover.convert("RGBA"), output)
    else:
        generate(arguments.browser, output)
