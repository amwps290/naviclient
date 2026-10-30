#!/usr/bin/env python3
"""Generate tray-playing.png and tray-paused.png from app-icon.png."""

from pathlib import Path
from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parent
SIZE = 256


def load_base():
    source = ROOT / "app-icon.png"
    if not source.exists():
        raise SystemExit("assets/app-icon.png is missing; run assets/gen_app_icon.py first")
    return Image.open(source).convert("RGBA").resize(
        (SIZE, SIZE), Image.Resampling.LANCZOS
    )


def state_badge(base, paused):
    out = base.copy()
    d = ImageDraw.Draw(out)
    # The badge sits on the record label and remains readable down to 16px.
    cx, cy, radius = 114, 144, 18
    d.ellipse(
        (cx - radius, cy - radius, cx + radius, cy + radius),
        fill=(15, 17, 28, 220),
    )
    if paused:
        for x in (cx - 7, cx + 3):
            d.rounded_rectangle(
                (x, cy - 9, x + 5, cy + 9),
                radius=2,
                fill=(248, 247, 244, 255),
            )
    else:
        d.polygon(
            [(cx - 5, cy - 10), (cx - 5, cy + 10), (cx + 11, cy)],
            fill=(248, 247, 244, 255),
        )
    return out


def main():
    base = load_base()
    state_badge(base, paused=False).save(ROOT / "tray-playing.png", optimize=True)
    state_badge(base, paused=True).save(ROOT / "tray-paused.png", optimize=True)
    print("wrote assets/tray-playing.png and assets/tray-paused.png")


if __name__ == "__main__":
    main()
