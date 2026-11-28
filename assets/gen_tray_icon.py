#!/usr/bin/env python3
"""Generate tray-playing.png and tray-paused.png from app-icon.png.

Places a crisp, high-contrast playback badge on the vinyl center label so the
playing/paused state stays readable down to 16px in Linux SNI / desktop trays.
"""

from pathlib import Path
from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parent
SIZE = 256
SUPER = SIZE * 4


def load_base():
    source = ROOT / "app-icon.png"
    if not source.exists():
        raise SystemExit("assets/app-icon.png is missing; run assets/gen_app_icon.py first")
    return Image.open(source).convert("RGBA").resize(
        (SUPER, SUPER), Image.Resampling.LANCZOS
    )


def state_badge(base, paused):
    out = base.copy()
    overlay = Image.new("RGBA", (SUPER, SUPER), (0, 0, 0, 0))
    d = ImageDraw.Draw(overlay)
    # Centered on the vinyl record (468, 540 in 1024-space -> 468, 540 in 1024 SUPER space).
    cx, cy = 468, 540
    outer_r = 152
    ring_w = 14
    # Deep obsidian badge disc with warm champagne-gold rim for high legibility at 16px–24px.
    d.ellipse(
        (cx - outer_r, cy - outer_r, cx + outer_r, cy + outer_r),
        fill=(22, 18, 17, 238),
        outline=(235, 198, 152, 230),
        width=ring_w,
    )
    ink = (252, 247, 241, 255)
    if paused:
        bar_w, bar_h, gap = 36, 114, 24
        left_x = cx - gap // 2 - bar_w
        right_x = cx + gap // 2
        top_y = cy - bar_h // 2
        for x in (left_x, right_x):
            d.rounded_rectangle(
                (x, top_y, x + bar_w, top_y + bar_h),
                radius=12,
                fill=ink,
            )
    else:
        # Optically centered play triangle.
        d.polygon(
            [(cx - 40, cy - 64), (cx - 40, cy + 64), (cx + 72, cy)],
            fill=ink,
        )
    out.alpha_composite(overlay)
    return out.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def main():
    base = load_base()
    state_badge(base, paused=False).save(ROOT / "tray-playing.png", optimize=True)
    state_badge(base, paused=True).save(ROOT / "tray-paused.png", optimize=True)
    print("wrote assets/tray-playing.png and assets/tray-paused.png")


if __name__ == "__main__":
    main()
