# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow"]
# ///
"""
parity_crop.py — stack the same region of two frames so it can be LOOKED at.

    uv run scripts/parity_crop.py <orig.png> <port.png> X0,Y0,X1,Y1 <out.png> [--scale N]

Every other instrument in this campaign reduces a region to numbers. Numbers
are how a difference gets ranked, but they are not how a difference gets
FOUND — a mean over a region containing two different things averages the
difference away, and no amount of care in reading the number recovers it.

This does the one thing the numeric tools cannot: it puts the original above
the port at the same crop and the same scale, so the eye can adjudicate. The
original is on top, the port below, with a one-pixel divider.
"""

from __future__ import annotations

import sys

from PIL import Image


def main() -> int:
    if len(sys.argv) < 5:
        print(__doc__)
        return 2
    orig, port, rect, out = sys.argv[1:5]
    scale = 1
    if "--scale" in sys.argv:
        scale = int(sys.argv[sys.argv.index("--scale") + 1])
    x0, y0, x1, y1 = (int(t) for t in rect.split(","))
    box = (x0, y0, x1, y1)
    a = Image.open(orig).convert("RGB").crop(box)
    b = Image.open(port).convert("RGB").crop(box)
    if scale != 1:
        a = a.resize((a.width * scale, a.height * scale), Image.NEAREST)
        b = b.resize((b.width * scale, b.height * scale), Image.NEAREST)
    canvas = Image.new("RGB", (a.width, a.height * 2 + 3), (255, 0, 255))
    canvas.paste(a, (0, 0))
    canvas.paste(b, (0, a.height + 3))
    canvas.save(out)
    print(f"wrote {out}  ({a.width}x{a.height} each; ORIGINAL on top, PORT below)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
