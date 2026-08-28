# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
"""
parity_noisefloor.py — how much do two frames of the SAME build differ?

    uv run scripts/parity_noisefloor.py <a.png> <b.png> [<c.png> ...]

WHY THIS EXISTS

Every parity number in this campaign is a difference between an original frame
and a port frame. None of them mean anything until you know how big a difference
the *original alone* produces when captured twice. If two captures of the
original differ by D, then any original-vs-port difference smaller than D is
indistinguishable from capture noise and must not be quoted as a defect.

`apps/shmup/src/dev/shots.js` documents, in the `lockstep` comment block, that
in free-running capture mode the frame index at the shutter "drifted 10-20
frames run to run" and that TAA jitter, GTAO/SSR/contact-shadow noise rotation
and exposure adaptation are all phase-locked to that index. So the noise floor
is expected to be LARGE in free-running mode and small in lockstep. This
measures it instead of assuming it.

It reports, for every pair:
  maxdiff   largest absolute 0-255 channel difference anywhere
  mad       mean absolute difference over all channels
  p>2       fraction of pixels where any channel moved more than 2/255
  dLuma     difference of mean luma, in stops (the TONE metric's unit)
  worst     the 64x64 tile with the largest mean absolute difference, and where

`worst` is the part a global mean cannot show you: a localised difference (one
wrong object, one flickering shadow) is invisible in `mad` and obvious here.
"""

from __future__ import annotations

import itertools
import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def load(p: Path) -> np.ndarray:
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.float64)


def luma(a: np.ndarray) -> np.ndarray:
    return a[..., 0] * 0.2126 + a[..., 1] * 0.7152 + a[..., 2] * 0.0722


def worst_tile(d: np.ndarray, tile: int = 64) -> tuple[float, int, int]:
    h, w = d.shape[:2]
    best = (-1.0, 0, 0)
    for y in range(0, h - tile + 1, tile):
        for x in range(0, w - tile + 1, tile):
            m = float(d[y : y + tile, x : x + tile].mean())
            if m > best[0]:
                best = (m, x, y)
    return best


def compare(pa: Path, pb: Path) -> None:
    a, b = load(pa), load(pb)
    if a.shape != b.shape:
        print(f"  {pa.name} vs {pb.name}: SHAPE MISMATCH {a.shape} vs {b.shape}")
        return
    d = np.abs(a - b)
    dmax = float(d.max())
    mad = float(d.mean())
    frac = float((d.max(axis=2) > 2).mean())
    la, lb = float(luma(a).mean()), float(luma(b).mean())
    stops = math.log2(max(lb, 1e-6) / max(la, 1e-6))
    wm, wx, wy = worst_tile(d.mean(axis=2))
    print(
        f"  {pa.name:34s} vs {pb.name:34s}  "
        f"maxdiff={dmax:5.1f}  mad={mad:6.3f}  p>2={frac * 100:5.2f}%  "
        f"dLuma={stops:+.4f} stop  worst64={wm:6.3f} @({wx},{wy})"
    )


def main() -> int:
    paths = [Path(p) for p in sys.argv[1:]]
    if len(paths) < 2:
        print(__doc__)
        return 2
    print(f"noise floor over {len(paths)} captures ({len(paths) * (len(paths) - 1) // 2} pairs)")
    for pa, pb in itertools.combinations(paths, 2):
        compare(pa, pb)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
