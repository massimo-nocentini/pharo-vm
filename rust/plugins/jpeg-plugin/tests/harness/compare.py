#!/usr/bin/env python3
"""Compares two runs of jpegdiff.st, visible pixels only.

Why visible pixels only: a Form whose width is not a multiple of its
pixels-per-word carries padding pixels in the last word of each row. That is
exactly where the C plugin reads past libjpeg's scanline buffer, so those words
differ wildly between implementations while nothing a user can see differs at
all. Comparing whole bitmaps reports a failure that is really a C bug in
invisible data.

Usage: compare.py <dump_c> <dump_rs>

Exits non-zero if any *visible* channel differs by more than --tolerance
(default 4, comfortably above the 3 observed between libjpeg-6b and
jpeg-decoder, and far below anything a user would notice).
"""
import argparse
import os
import re
import struct
import sys

# width/height per corpus image, keyed by file stem (see make_corpus.py).
DIMS = {
    "rgb_even_64x48": (64, 48), "rgb_odd_33x17": (33, 17), "rgb_odd_1x1": (1, 1),
    "rgb_tall_7x64": (7, 64), "gray_even_32x32": (32, 32), "gray_odd_31x15": (31, 15),
    "rgb_prog_40x40": (40, 40), "rgb_lowq_50x37": (50, 37), "rgb_big_200x150": (200, 150),
}


def words(path):
    b = open(path, "rb").read()
    return struct.unpack(f"<{len(b) // 4}I", b[: len(b) // 4 * 4])


def pixels_of_word(w, depth):
    """Channels of each pixel packed in this word, left to right."""
    d = abs(depth)
    if d == 32:
        return [((w >> 16) & 255, (w >> 8) & 255, w & 255)]
    if d == 16:
        hi = ((w >> 26) & 31, (w >> 21) & 31, (w >> 16) & 31)
        lo = ((w >> 10) & 31, (w >> 5) & 31, w & 31)
        return [hi, lo] if depth > 0 else [lo, hi]
    if d == 8:
        b = [(w >> 24) & 255, (w >> 16) & 255, (w >> 8) & 255, w & 255]
        return [(x,) for x in (b if depth > 0 else b[::-1])]
    return []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump_a")
    ap.add_argument("dump_b")
    ap.add_argument("--tolerance", type=int, default=4)
    args = ap.parse_args()

    failures, worst_visible = [], 0
    names = sorted(os.listdir(args.dump_a))
    for name in names:
        pa, pb = os.path.join(args.dump_a, name), os.path.join(args.dump_b, name)
        if not os.path.exists(pb):
            failures.append(f"{name}: missing from {args.dump_b}")
            continue
        stem = name.split(".jpg")[0]
        depth = int(re.search(r"_d(-?\d+)_", name).group(1))
        W, H = DIMS[stem]
        wa, wb = words(pa), words(pb)
        if len(wa) != len(wb):
            failures.append(f"{name}: bitmap size {len(wa)} vs {len(wb)} words")
            continue

        ppw = {32: 1, 16: 2, 8: 4}[abs(depth)]
        wpr = (W + ppw - 1) // ppw
        vis_max = 0
        for row in range(H):
            for j in range(wpr):
                idx = row * wpr + j
                if idx >= len(wa):
                    break
                for k, (x, y) in enumerate(
                    zip(pixels_of_word(wa[idx], depth), pixels_of_word(wb[idx], depth))
                ):
                    if j * ppw + k >= W:
                        continue  # padding: not visible, and where the C reads OOB
                    vis_max = max(vis_max, max(abs(p - q) for p, q in zip(x, y)))
        worst_visible = max(worst_visible, vis_max)
        if vis_max > args.tolerance:
            failures.append(f"{name}: visible delta {vis_max} > {args.tolerance}")

    print(f"compared {len(names)} cases; worst visible channel delta: {worst_visible}")
    if failures:
        print(f"FAILED ({len(failures)}):", file=sys.stderr)
        for f in failures:
            print("  " + f, file=sys.stderr)
        return 1
    print("OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
