#!/usr/bin/env python3
"""Builds the JPEG corpus used by the differential harness.

Chosen to cover what actually breaks: grayscale as well as RGB, progressive as
well as baseline, a wide quality range, and -- most importantly -- widths that
are not a multiple of the Form's pixels-per-word, which is where the C plugin
reads past its scanline buffer.
"""
import os
import sys
from PIL import Image

CASES = [
    ("rgb_even_64x48", 64, 48, "RGB", {"quality": 90}),
    ("rgb_odd_33x17", 33, 17, "RGB", {"quality": 90}),
    ("rgb_odd_1x1", 1, 1, "RGB", {"quality": 90}),
    ("rgb_tall_7x64", 7, 64, "RGB", {"quality": 75}),
    ("gray_even_32x32", 32, 32, "L", {"quality": 90}),
    ("gray_odd_31x15", 31, 15, "L", {"quality": 85}),
    ("rgb_prog_40x40", 40, 40, "RGB", {"quality": 80, "progressive": True}),
    ("rgb_lowq_50x37", 50, 37, "RGB", {"quality": 15}),
    ("rgb_big_200x150", 200, 150, "RGB", {"quality": 95}),
]


def gradient(w, h, mode):
    img = Image.new(mode, (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            r = (x * 255) // max(w - 1, 1)
            g = (y * 255) // max(h - 1, 1)
            b = ((x + y) * 255) // max(w + h - 2, 1)
            px[x, y] = (r, g, b) if mode == "RGB" else (r + g) // 2
    return img


def main(out):
    os.makedirs(out, exist_ok=True)
    for name, w, h, mode, opts in CASES:
        gradient(w, h, mode).save(os.path.join(out, name + ".jpg"), "JPEG", **opts)
        print(f"{name}.jpg {w}x{h} {mode} {opts}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "jpegs")
