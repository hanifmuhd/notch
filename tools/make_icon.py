"""Draws the ring app icon (spec 5.7) as a 1024 px PNG; `cargo tauri icon` makes the rest.

Usage: python3 tools/make_icon.py src-tauri/icons/source-ring.png [src-tauri/icons/source-ring-small.png]
"""
import sys
from PIL import Image, ImageDraw

S = 1024
SS = 4  # draw 4x and downscale for smooth edges


def ring(d, r, width, track, colour, pct):
    c = S * SS // 2
    R = r + width / 2  # Pillow strokes inward from the box: centre the band on r
    box = [c - R * SS, c - R * SS, c + R * SS, c + R * SS]
    d.ellipse(box, outline=track, width=width * SS)
    if pct > 0:
        end = -90 + 360 * pct
        d.arc(box, -90, end, fill=colour, width=width * SS)
        # round caps at both ends of the arc
        import math
        for a in (-90, end):
            rad = math.radians(a)
            x, y = c + r * SS * math.cos(rad), c + r * SS * math.sin(rad)
            h = width * SS / 2
            d.ellipse([x - h, y - h, x + h, y + h], fill=colour)


def draw(small):
    """Full mark, or for 32 px and below: a thicker outer arc and a solid inner dot."""
    img = Image.new("RGBA", (S * SS, S * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, S * SS - 1, S * SS - 1], radius=224 * SS, fill=(0x12, 0x12, 0x14, 255))
    track = (0x36, 0x36, 0x38, 255)  # #ffffff26 over #121214, pre-blended (Pillow does not composite)
    if small:
        ring(d, 300, 150, track, (0xF3, 0xF1, 0xEE, 255), 0.68)
        c, r = S * SS // 2, 120 * SS
        d.ellipse([c - r, c - r, c + r, c + r], fill=(0x9F, 0xB8, 0xFF, 255))
    else:
        ring(d, 330, 104, track, (0xF3, 0xF1, 0xEE, 255), 0.68)
        ring(d, 180, 104, track, (0x9F, 0xB8, 0xFF, 255), 0.37)
    return img.resize((S, S), Image.LANCZOS)


if __name__ == "__main__":
    draw(False).save(sys.argv[1])
    if len(sys.argv) > 2:
        draw(True).save(sys.argv[2])
