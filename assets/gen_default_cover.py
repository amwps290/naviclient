#!/usr/bin/env python3
"""Generate assets/default-cover.png — a stylized vinyl record on a deep
indigo-to-plum gradient. Matches the app's turntable aesthetic and yields a
warm accent + cool base palette for the player's cover-driven theming."""

import math
import random
from PIL import Image, ImageDraw, ImageFilter

SIZE = 512
CX = CY = SIZE // 2
DISC_R = 196          # vinyl disc outer radius
LABEL_R = 64           # center label radius
HOLE_R = 8             # spindle hole radius


def lerp(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


def radial_gradient(size, inner, outer, inner_radius=0.0, falloff=1.0):
    """Low-res radial gradient upscaled with bilinear filtering."""
    low = max(16, size // 8)
    img = Image.new("RGB", (low, low))
    px = img.load()
    c = (low - 1) / 2
    max_d = math.hypot(c, c)
    for y in range(low):
        for x in range(low):
            d = (math.hypot(x - c, y - c) / max_d) ** falloff
            t = min(1.0, max(0.0, (d - inner_radius) / (1.0 - inner_radius)))
            px[x, y] = lerp(inner, outer, t)
    return img.resize((size, size), Image.BILINEAR)


def diagonal_gradient(size, stops):
    """stops: list of RGB tuples evenly spread along the (0,0)->(1,1) axis."""
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


# ---------------------------------------------------------------- background
INDIGO = (61, 68, 152)      # deep indigo (top-left)
MAGENTA = (122, 52, 122)     # dusk magenta (middle)
CORAL = (196, 84, 84)        # muted coral (bottom-right)
base = diagonal_gradient(SIZE, [INDIGO, MAGENTA, CORAL]).convert("RGB")

# vignette (darken corners slightly)
vignette = radial_gradient(SIZE, (255, 255, 255), (120, 118, 150), 0.55, 1.6).convert("L")
base = Image.composite(base, Image.new("RGB", (SIZE, SIZE), (18, 16, 38)), vignette)

# subtle glow behind the disc (warm halo, very faint)
glow = Image.new("L", (SIZE, SIZE), 0)
gd = ImageDraw.Draw(glow)
gd.ellipse([CX - DISC_R - 26, CY - DISC_R - 26, CX + DISC_R + 26, CY + DISC_R + 26], fill=70)
glow = glow.filter(ImageFilter.GaussianBlur(28))
halo = Image.new("RGB", (SIZE, SIZE), (233, 116, 81))
base = Image.composite(halo, base, glow.point(lambda v: v // 3))

# fine grain so flat areas don't look sterile (chunky grain, compresses well)
noise = Image.effect_noise((SIZE // 2, SIZE // 2), 20).convert("L").resize((SIZE, SIZE), Image.BILINEAR)
base = Image.blend(base, Image.merge("RGB", (noise, noise, noise)), 0.05)

# ---------------------------------------------------------------- vinyl disc
img = base.convert("RGBA")
overlay = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
d = ImageDraw.Draw(overlay)

# soft drop shadow under the disc
shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
sd = ImageDraw.Draw(shadow)
sd.ellipse([CX - DISC_R - 2, CY - DISC_R + 4, CX + DISC_R + 2, CY + DISC_R + 10], fill=(6, 6, 14, 170))
shadow = shadow.filter(ImageFilter.GaussianBlur(10))
img = Image.alpha_composite(img, shadow)

# disc body: near-black with a cold blue tint
d.ellipse([CX - DISC_R, CY - DISC_R, CX + DISC_R, CY + DISC_R], fill=(17, 19, 28, 255))

# grooves: thin concentric rings with subtly varying brightness
random.seed(7)
r = LABEL_R + 6
while r < DISC_R - 4:
    step = random.choice((3, 3, 4, 5))
    bright = random.randint(28, 52)
    alpha = random.randint(26, 58)
    d.ellipse([CX - r, CY - r, CX + r, CY + r], outline=(bright, bright + 4, bright + 12, alpha), width=1)
    r += step

# lead-out rim: slightly brighter band near the edge
for i, (rr, alpha) in enumerate(((DISC_R - 4, 46), (DISC_R - 2, 70), (DISC_R, 95))):
    d.ellipse([CX - rr, CY - rr, CX + rr, CY + rr], outline=(70, 76, 96, alpha), width=1)

# rim edge: cold specular highlight on the upper-left, shadow on lower-right
d.arc([CX - DISC_R + 1, CY - DISC_R + 1, CX + DISC_R - 1, CY + DISC_R - 1], 150, 315,
      fill=(150, 168, 210, 130), width=2)
d.arc([CX - DISC_R + 1, CY - DISC_R + 1, CX + DISC_R - 1, CY + DISC_R - 1], 330, 135,
      fill=(8, 9, 16, 150), width=2)

img = Image.alpha_composite(img, overlay)

# light sheen sweeping across the vinyl (blurred wide arcs, screen-like)
sheen = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
sh = ImageDraw.Draw(sheen)
for angle_start, angle_end in ((200, 265), (25, 80)):
    r_in = LABEL_R + 8
    sh.arc([CX - r_in, CY - r_in, CX + r_in, CY + r_in], angle_start, angle_end, fill=(255, 255, 255, 110), width=14)
for angle_start, angle_end in ((210, 252), (36, 70)):
    rr = DISC_R - 14
    sh.arc([CX - rr, CY - rr, CX + rr, CY + rr], angle_start, angle_end, fill=(255, 255, 255, 90), width=22)
sheen = sheen.filter(ImageFilter.GaussianBlur(9))
img = Image.alpha_composite(img, sheen)

# ---------------------------------------------------------------- center label
label = radial_gradient(SIZE, (255, 179, 92), (222, 74, 66), 0.0, 1.15).convert("RGBA")
mask = Image.new("L", (SIZE, SIZE), 0)
md = ImageDraw.Draw(mask)
md.ellipse([CX - LABEL_R, CY - LABEL_R, CX + LABEL_R, CY + LABEL_R], fill=255)
label.putalpha(mask)
img = Image.alpha_composite(img, label)

overlay = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
d = ImageDraw.Draw(overlay)
# label rim: darker ring + top-left highlight
d.ellipse([CX - LABEL_R, CY - LABEL_R, CX + LABEL_R, CY + LABEL_R], outline=(150, 42, 34, 200), width=2)
d.arc([CX - LABEL_R + 2, CY - LABEL_R + 2, CX + LABEL_R - 2, CY + LABEL_R - 2], 170, 300,
      fill=(255, 214, 150, 170), width=3)
# faint concentric rings on the label (print texture)
for rr in (LABEL_R - 12, LABEL_R - 24):
    d.ellipse([CX - rr, CY - rr, CX + rr, CY + rr], outline=(255, 235, 200, 40), width=1)

# spindle hole + tiny specular dot on its rim
d.ellipse([CX - HOLE_R, CY - HOLE_R, CX + HOLE_R, CY + HOLE_R], fill=(12, 13, 20, 255))
d.ellipse([CX - HOLE_R - 2, CY - HOLE_R - 2, CX + HOLE_R + 2, CY + HOLE_R + 2],
           outline=(120, 46, 38, 160), width=1)
d.ellipse([CX - 18, CY - 30, CX - 8, CY - 20], fill=(255, 226, 180, 60))
img = Image.alpha_composite(img, overlay)

# ---------------------------------------------------------------- finish
img = img.convert("RGBA")
img.save("assets/default-cover.png", optimize=True)
print("saved", img.size)
