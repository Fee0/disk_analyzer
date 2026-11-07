import colorsys
import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent
WINDOW_ICON_SIDE = 64

CANVAS = 1024
CENTER = CANVAS / 2
BG_TOP = (0x30, 0x33, 0x3a)
BG_BOTTOM = (0x1f, 0x21, 0x25)
HUB = (0x4f, 0x8c, 0xf7)


def hsl(h, s, l):
    r, g, b = colorsys.hls_to_rgb(h / 360, l, s)
    return (round(r * 255), round(g * 255), round(b * 255))


VIDEO = (142, 0.62, 0.62)
DOCUMENT = (212, 0.82, 0.70)
ARCHIVE = (44, 0.88, 0.66)
CODE = (264, 0.70, 0.74)
APP = (2, 0.70, 0.68)
IMAGE = (188, 0.72, 0.62)


def ring_color(kind, ring):
    h, s, l = kind
    return hsl(h, s, l - 0.07 * ring)


DETAILED = {
    "hub": 118,
    "rings": [
        (138, 258, [(0, 128, VIDEO), (128, 206, DOCUMENT), (206, 262, ARCHIVE), (262, 312, CODE), (312, 360, APP)]),
        (270, 372, [(0, 74, VIDEO), (74, 112, IMAGE), (128, 184, DOCUMENT), (206, 244, ARCHIVE), (262, 298, CODE), (312, 340, APP)]),
    ],
    "gap": 12,
}

SIMPLE = {
    "hub": 130,
    "rings": [
        (170, 390, [(0, 140, VIDEO), (140, 230, DOCUMENT), (230, 300, ARCHIVE), (300, 360, CODE)]),
    ],
    "gap": 34,
}


def sector_points(r_in, r_out, a0, a1, gap, steps_per_degree=4):
    def edge_angles(r):
        half = math.degrees(math.asin(min(1.0, (gap / 2) / r)))
        return a0 + half, a1 - half

    def point(r, deg):
        rad = math.radians(deg - 90)
        return (CENTER + r * math.cos(rad), CENTER + r * math.sin(rad))

    o0, o1 = edge_angles(r_out)
    i0, i1 = edge_angles(r_in)
    n_out = max(2, int((o1 - o0) * steps_per_degree))
    n_in = max(2, int((i1 - i0) * steps_per_degree))
    outer = [point(r_out, o0 + (o1 - o0) * k / n_out) for k in range(n_out + 1)]
    inner = [point(r_in, i1 - (i1 - i0) * k / n_in) for k in range(n_in + 1)]
    return outer + inner, (o0, o1, i0, i1)


def tile_box(margin):
    return (margin, margin, CANVAS - margin, CANVAS - margin)


def corner_radius(margin):
    return round((CANVAS - 2 * margin) * 0.225)


def render(art, margin, size, supersample=4):
    big = CANVAS * supersample

    def scaled(points):
        return [(x * supersample, y * supersample) for x, y in points]

    gradient = Image.new("RGB", (1, big))
    for y in range(big):
        t = y / (big - 1)
        gradient.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOTTOM)))
    gradient = gradient.resize((big, big))

    mask = Image.new("L", (big, big), 0)
    x0, y0, x1, y1 = tile_box(margin)
    ImageDraw.Draw(mask).rounded_rectangle(
        (x0 * supersample, y0 * supersample, x1 * supersample, y1 * supersample),
        radius=corner_radius(margin) * supersample,
        fill=255,
    )
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    img.paste(gradient, (0, 0), mask)

    draw = ImageDraw.Draw(img)
    for ring_index, (r_in, r_out, segments) in enumerate(art["rings"]):
        for a0, a1, kind in segments:
            points, _ = sector_points(r_in, r_out, a0, a1, art["gap"])
            draw.polygon(scaled(points), fill=ring_color(kind, ring_index))
    hub = art["hub"] * supersample
    c = CENTER * supersample
    draw.ellipse((c - hub, c - hub, c + hub, c + hub), fill=HUB)

    return img.resize((size, size), Image.LANCZOS)


def svg(art, margin):
    x0, y0, x1, _ = tile_box(margin)
    side = x1 - x0
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {CANVAS} {CANVAS}">',
        '<defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" stop-color="#{"%02x%02x%02x" % BG_TOP}"/>'
        f'<stop offset="1" stop-color="#{"%02x%02x%02x" % BG_BOTTOM}"/>'
        "</linearGradient></defs>",
        f'<rect x="{x0}" y="{y0}" width="{side}" height="{side}" rx="{corner_radius(margin)}" fill="url(#bg)"/>',
    ]

    def point(r, deg):
        rad = math.radians(deg - 90)
        return f"{CENTER + r * math.cos(rad):.2f} {CENTER + r * math.sin(rad):.2f}"

    for ring_index, (r_in, r_out, segments) in enumerate(art["rings"]):
        for a0, a1, kind in segments:
            _, (o0, o1, i0, i1) = sector_points(r_in, r_out, a0, a1, art["gap"])
            large_out = 1 if o1 - o0 > 180 else 0
            large_in = 1 if i1 - i0 > 180 else 0
            d = (
                f"M{point(r_out, o0)} A{r_out} {r_out} 0 {large_out} 1 {point(r_out, o1)} "
                f"L{point(r_in, i1)} A{r_in} {r_in} 0 {large_in} 0 {point(r_in, i0)} Z"
            )
            parts.append(f'<path d="{d}" fill="#{"%02x%02x%02x" % ring_color(kind, ring_index)}"/>')
    parts.append(f'<circle cx="{CENTER}" cy="{CENTER}" r="{art["hub"]}" fill="#{"%02x%02x%02x" % HUB}"/>')
    parts.append("</svg>")
    return "\n".join(parts) + "\n"


MAC_MARGIN = 100
FLAT_MARGIN = 40
SMALL_SIZES = {16, 24, 32}


def art_for(size):
    return SIMPLE if size in SMALL_SIZES else DETAILED


(OUT / "icon.svg").write_text(svg(DETAILED, FLAT_MARGIN), encoding="utf-8")
(OUT / "icon-macos.svg").write_text(svg(DETAILED, MAC_MARGIN), encoding="utf-8")

flat = {s: render(art_for(s), FLAT_MARGIN, s) for s in [16, 24, 32, 48, 64, 128, 256, 512, 1024]}
for s, im in flat.items():
    im.save(OUT / f"icon-{s}.png", optimize=True)

ico_sizes = [16, 24, 32, 48, 64, 128, 256]
flat[256].save(
    OUT / "icon.ico",
    sizes=[(s, s) for s in ico_sizes],
    append_images=[flat[s] for s in ico_sizes if s != 256],
)

mac = {s: render(art_for(s), MAC_MARGIN, s) for s in [16, 32, 64, 128, 256, 512, 1024]}
mac[1024].save(OUT / "icon.icns", append_images=[mac[s] for s in [16, 32, 64, 128, 256, 512]])

(OUT / f"window-icon-{WINDOW_ICON_SIDE}x{WINDOW_ICON_SIDE}.rgba").write_bytes(flat[WINDOW_ICON_SIDE].tobytes())

if len(sys.argv) < 2:
    sys.exit()

preview = Image.new("RGBA", (1024 + 512 + 256 + 128 + 64 + 32 + 16 + 7 * 24, 1024), (0xe8, 0xea, 0xee, 255))
x = 0
for s in [1024, 512, 256, 128, 64, 32, 16]:
    preview.alpha_composite(flat[s], (x, (1024 - s) // 2))
    x += s + 24
dark = Image.new("RGBA", preview.size, (0x24, 0x26, 0x2b, 255))
x = 0
for s in [1024, 512, 256, 128, 64, 32, 16]:
    dark.alpha_composite(mac[s], (x, (1024 - s) // 2))
    x += s + 24
sheet = Image.new("RGBA", (preview.width, 2048))
sheet.paste(preview, (0, 0))
sheet.paste(dark, (0, 1024))
sheet.convert("RGB").resize((sheet.width // 2, sheet.height // 2), Image.LANCZOS).save(Path(sys.argv[1]))
print("ok")
