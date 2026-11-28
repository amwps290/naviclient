#!/usr/bin/env python3
"""Generate the Navidrome Client application icon and Windows ICO bundle.

Renders `assets/app-icon.svg` at 2x supersampling using librsvg/pycairo
(falling back to a headless Chromium browser if needed), then downsamples
with Lanczos3 to produce `app-icon.png` (1024x1024) and multi-size `app-icon.ico`.
"""

import argparse
from pathlib import Path
from PIL import Image


ROOT = Path(__file__).resolve().parent
SIZE = 1024
RASTER_SIZE = SIZE * 2


def render_svg_rsvg(svg_path: Path, side: int) -> Image.Image:
    import cairo
    import gi

    gi.require_version("Rsvg", "2.0")
    from gi.repository import Rsvg

    handle = Rsvg.Handle.new_from_file(str(svg_path))
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, side, side)
    ctx = cairo.Context(surface)
    viewport = Rsvg.Rectangle()
    viewport.x = 0.0
    viewport.y = 0.0
    viewport.width = float(side)
    viewport.height = float(side)
    handle.render_document(ctx, viewport)
    surface.flush()
    # Cairo ARGB32 on little-endian is premultiplied BGRA in memory.
    raw = bytes(surface.get_data())
    img = Image.frombuffer("RGBA", (side, side), raw, "raw", "BGRa", 0, 1).copy()
    return img


def generate_icon(svg_path: Path) -> Image.Image:
    hi = render_svg_rsvg(svg_path, RASTER_SIZE)
    return hi.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--svg", type=Path, default=ROOT / "app-icon.svg")
    args = parser.parse_args()

    icon = generate_icon(args.svg.resolve())
    png_path = ROOT / "app-icon.png"
    ico_path = ROOT / "app-icon.ico"
    icon.save(png_path, optimize=True)
    icon.save(
        ico_path,
        format="ICO",
        sizes=[
            (16, 16),
            (20, 20),
            (24, 24),
            (32, 32),
            (48, 48),
            (64, 64),
            (128, 128),
            (256, 256),
        ],
    )
    print(f"saved {png_path.name} and {ico_path.name}")


if __name__ == "__main__":
    main()
