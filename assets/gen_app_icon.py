#!/usr/bin/env python3
"""Generate the Navidrome Client application icon and Windows ICO bundle."""

from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parent
SIZE = 1024
INDIGO = (52, 58, 118)
CORAL = (218, 91, 91)
ORANGE = (255, 159, 86)
INK = (15, 17, 28)
WHITE = (248, 247, 244)


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def gradient(c1, c2):
    low = 128
    image = Image.new("RGB", (low, low))
    pixels = image.load()
    for y in range(low):
        for x in range(low):
            t = (x + y) / (2 * (low - 1))
            pixels[x, y] = lerp(c1, c2, t)
    return image.resize((SIZE, SIZE), Image.Resampling.BICUBIC).convert("RGBA")


def rounded_icon(base, radius=220):
    mask = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (16, 16, SIZE - 16, SIZE - 16), radius=radius, fill=255
    )
    out = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    out.paste(base, (0, 0), mask)
    return out


def glow_layer(center, radius, color, alpha=88, blur=92):
    layer = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    x, y = center
    d.ellipse(
        (x - radius, y - radius, x + radius, y + radius),
        fill=(*color, alpha),
    )
    return layer.filter(ImageFilter.GaussianBlur(blur))


def draw_disc(layer):
    x, y, radius = 455, 575, 300
    shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    sd.ellipse(
        (x - radius, y - radius + 18, x + radius, y + radius + 18),
        fill=(0, 0, 0, 130),
    )
    layer.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(24)))

    d = ImageDraw.Draw(layer)
    d.ellipse(
        (x - radius, y - radius, x + radius, y + radius), fill=(*INK, 255)
    )
    for factor, width, alpha in ((0.82, 8, 110), (0.63, 7, 90), (0.45, 6, 74)):
        r = int(radius * factor)
        d.arc(
            (x - r, y - r, x + r, y + r),
            18,
            325,
            fill=(142, 150, 206, alpha),
            width=width,
        )
    label_radius = int(radius * 0.22)
    d.ellipse(
        (
            x - label_radius,
            y - label_radius,
            x + label_radius,
            y + label_radius,
        ),
        fill=(*ORANGE, 255),
    )
    d.ellipse((x - 17, y - 17, x + 17, y + 17), fill=(*INK, 255))


def generate_icon():
    icon = rounded_icon(gradient((20, 23, 43), (84, 43, 88)))
    icon.alpha_composite(glow_layer((455, 575), 355, CORAL))
    draw_disc(icon)

    d = ImageDraw.Draw(icon)
    outer = (118, 238, 792, 912)
    d.arc(outer, 192, 324, fill=(*CORAL, 255), width=42)
    d.arc(outer, 20, 142, fill=(126, 142, 239, 245), width=30)

    arm = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ad = ImageDraw.Draw(arm)
    points = [(786, 238), (748, 365), (650, 548)]
    ad.line(points, fill=(7, 8, 18, 145), width=42, joint="curve")
    ad.line(points, fill=(*WHITE, 255), width=25, joint="curve")
    ad.ellipse(
        (746, 198, 826, 278),
        fill=(27, 28, 43, 255),
        outline=(*WHITE, 255),
        width=11,
    )
    ad.polygon(
        [(622, 534), (661, 525), (674, 568), (634, 583)], fill=(*CORAL, 255)
    )
    icon.alpha_composite(arm)
    return icon


def main():
    icon = generate_icon()
    png_path = ROOT / "app-icon.png"
    ico_path = ROOT / "app-icon.ico"
    icon.save(png_path, optimize=True)
    icon.save(
        ico_path,
        format="ICO",
        sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print(f"saved {png_path.name} and {ico_path.name}")


if __name__ == "__main__":
    main()
