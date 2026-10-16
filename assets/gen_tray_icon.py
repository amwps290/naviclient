#!/usr/bin/python3
"""Generate tray icons: assets/tray-playing.png & assets/tray-paused.png.

A circular badge on the app's dusk gradient (indigo -> magenta -> coral,
same palette as default-cover.png) with a top inner highlight for depth, a
subtle white rim so the icon reads on both light and dark panels, and a
white glyph with soft drop shadow:

- playing: an eighth note
- paused: two rounded bars

Rendered at 256px; the Rust side downscales to the sizes SNI hosts ask for.
"""

from PIL import Image, ImageChops, ImageDraw, ImageFilter

SIZE = 256
CX = CY = SIZE // 2
R = CX - 6  # badge radius (leave room for rim + antialias)

INDIGO = (61, 68, 152)
MAGENTA = (122, 52, 122)
CORAL = (196, 84, 84)


def lerp(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


def diagonal_gradient(size, stops):
    low = max(16, size // 8)
    img = Image.new("RGB", (low, low))
    px = img.load()
    m = low - 1
    for y in range(low):
        for x in range(low):
            t = (x + y) / (2 * m)
            f = t * (len(stops) - 1)
            i = min(int(f), len(stops) - 2)
            px[x, y] = lerp(stops[i], stops[i + 1], f - i)
    return img.resize((size, size), Image.BILINEAR)


def mask_circle(size, radius, blur=0):
    m = Image.new("L", (size, size), 0)
    ImageDraw.Draw(m).ellipse(
        [CX - radius, CY - radius, CX + radius, CY + radius], fill=255
    )
    if blur:
        m = m.filter(ImageFilter.GaussianBlur(blur))
    return m


def badge_base():
    """Gradient badge + top inner highlight + bottom shading + soft rim."""
    badge = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    grad = diagonal_gradient(SIZE, [INDIGO, MAGENTA, CORAL]).convert("RGBA")
    badge.paste(grad, (0, 0), mask_circle(SIZE, R))

    # top inner highlight: crescent along the upper rim only -> reads as a
    # convex disc without washing out the gradient (outer circle minus a
    # smaller circle nudged down)
    outer = Image.new("L", (SIZE, SIZE), 0)
    inner = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(outer).ellipse(
        [CX - R * 0.9, CY - R * 0.85, CX + R * 0.9, CY + R * 0.95], fill=255
    )
    ImageDraw.Draw(inner).ellipse(
        [CX - R * 0.9, CY - R * 0.55, CX + R * 0.9, CY + R * 1.25], fill=255
    )
    dome = ImageChops.subtract(outer, inner)
    dome = dome.filter(ImageFilter.GaussianBlur(8))
    hi = Image.new("RGBA", (SIZE, SIZE), (255, 255, 255, 0))
    hi.putalpha(dome.point(lambda v: v * 25 // 100))
    badge.alpha_composite(hi)

    # bottom shading: darker floor for the glyph's shadow to land on
    floor = Image.new("L", (SIZE, SIZE), 0)
    fd = ImageDraw.Draw(floor)
    fd.ellipse([CX - R, CY + R * 0.05, CX + R, CY + R * 1.7], fill=70)
    floor = floor.filter(ImageFilter.GaussianBlur(16))
    dark = Image.new("RGBA", (SIZE, SIZE), (12, 8, 32, 0))
    dark.putalpha(floor.point(lambda v: v * 55 // 100))
    badge.alpha_composite(dark)

    # rim: thin light stroke inside the edge for definition on any panel
    rim = ImageDraw.Draw(badge)
    rim.ellipse(
        [CX - R + 3, CY - R + 3, CX + R - 3, CY + R - 3],
        outline=(255, 255, 255, 46),
        width=4,
    )
    return badge


def with_glyph(base, glyph_rgba):
    """Composite glyph with a soft drop shadow onto the badge."""
    out = base.copy()

    shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    shadow.alpha_composite(glyph_rgba, (0, 5))
    alpha = shadow.split()[3].point(lambda v: v * 38 // 100)
    shadow = Image.new("RGBA", (SIZE, SIZE), (10, 6, 28, 0))
    shadow.putalpha(alpha)
    shadow = shadow.filter(ImageFilter.GaussianBlur(5))
    out.alpha_composite(shadow)

    out.alpha_composite(glyph_rgba)
    return out


def glyph_note():
    """White eighth note: slanted head, stem, small flag."""
    g = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    d = ImageDraw.Draw(g)

    # head: rotated ellipse (draw upright then rotate the layer)
    head = Image.new("RGBA", (64, 52), (0, 0, 0, 0))
    hd = ImageDraw.Draw(head)
    hd.ellipse([4, 8, 58, 44], fill=(255, 255, 255, 255))
    head = head.rotate(18, expand=True, resample=Image.BILINEAR)
    hx, hy = 74, 158  # head centre-ish anchor
    g.alpha_composite(head, (hx - head.width // 2, hy - head.height // 2))

    # stem from the head's right edge up
    stem_x = hx + 26
    d.rounded_rectangle(
        [stem_x, 74, stem_x + 11, hy + 18], radius=5, fill=(255, 255, 255, 255)
    )

    # flag: small curved banner off the stem top
    flag = Image.new("RGBA", (56, 64), (0, 0, 0, 0))
    fd = ImageDraw.Draw(flag)
    fd.pieslice([-30, -24, 62, 60], 300, 30, fill=(255, 255, 255, 255))
    flag = flag.filter(ImageFilter.GaussianBlur(0.6))
    g.alpha_composite(flag, (stem_x + 6, 66))
    return g


def glyph_pause():
    """Two rounded white bars."""
    g = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    d = ImageDraw.Draw(g)
    bar_w, bar_h, gap = 30, 92, 26
    total = bar_w * 2 + gap
    x0 = CX - total // 2
    for i in range(2):
        x = x0 + i * (bar_w + gap)
        d.rounded_rectangle(
            [x, CY - bar_h // 2, x + bar_w, CY + bar_h // 2],
            radius=12,
            fill=(255, 255, 255, 255),
        )
    return g


def main():
    base = badge_base()
    playing = with_glyph(base, glyph_note())
    paused = with_glyph(base, glyph_pause())
    playing.save("assets/tray-playing.png")
    paused.save("assets/tray-paused.png")
    print("wrote assets/tray-playing.png and assets/tray-paused.png")


if __name__ == "__main__":
    main()
