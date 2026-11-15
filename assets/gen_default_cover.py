#!/usr/bin/env python3
"""Rasterize the approved abstract cover with a local Chromium browser.

Requires Pillow and Chrome, Chromium, or Edge. The application embeds the PNG;
neither Python nor a browser is needed when building or running the client.
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from PIL import Image


ROOT = Path(__file__).resolve().parent
SIZE = 1024
RASTER_SIZE = SIZE * 2
MIP_SIZES = (64, 128, 256, 512)


def save_mipmaps(cover, output):
    if cover.size != (SIZE, SIZE) or cover.convert("RGBA").getextrema()[3] != (255, 255):
        raise ValueError("Expected an opaque 1024 x 1024 default cover.")
    for side in MIP_SIZES:
        path = output.with_name(f"{output.stem}-{side}{output.suffix}")
        cover.resize((side, side), Image.Resampling.LANCZOS).save(path, optimize=True)
        print(f"Saved {path} ({side} x {side})")


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
    raise FileNotFoundError("Install Chrome, Chromium, or Edge, or pass --browser PATH.")


def generate(browser, output):
    svg = (ROOT / "default-cover.svg").read_text(encoding="utf-8")
    # An isolated temporary profile leaves personal browser windows untouched.
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
            # Discard the extra viewport height, then prefilter curved edges at
            # twice the final size. The square stays opaque for circular crops.
            cover = image.crop((0, 0, RASTER_SIZE, RASTER_SIZE)).convert("RGBA")
            cover = cover.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
            if cover.getextrema()[3] != (255, 255):
                raise ValueError("Default cover must have an opaque background.")
            cover.save(output, optimize=True)
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
        generate(find_browser(arguments.browser), output)
