# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
"""
frame_sanity.py — is this capture a real rendered frame at all?

    uv run scripts/frame_sanity.py <shot.png> [<shot.png> ...]

WHY THIS EXISTS — a metric that cannot see a defect launders it

While auditing the shmup parity campaign I captured the original twice and my
own noise-floor script reported `maxdiff=0.0, mad=0.000` — a perfect match. The
correct reading of that number was not "the capture is deterministic". Both
frames were **dead black pages**: the WebGL context had been lost, the HUD DOM
kept updating (the match clock read 4:12 and the score 43-38, so the page looked
alive), and the 3D viewport rendered nothing. Two identical failures are
identical. The comparison metric was structurally incapable of telling me that,
because it only ever looks at the *difference* between two frames and never at
whether either frame contains a picture.

So every capture must pass this gate BEFORE any comparison quotes it. It looks
at one frame on its own and asks whether it could plausibly be a render:

  ink        fraction of the frame that is not near-black. A lost context leaves
             the 3D viewport at 0 and only the HUD lit, which is a few percent.
  uniq       distinct quantised colours. A solid fill has almost none.
  edges      mean absolute Sobel-ish gradient. A real 3D frame has structure;
             a flat fill or a single huge untextured surface has very little.
  centre     the same three, restricted to the middle half of the frame, where
             the HUD is not. This is the one that catches "dead world, live
             HUD": `ink` over the whole frame can pass on HUD pixels alone,
             `centre.ink` cannot.

The thresholds are deliberately loose. This is not a quality check and it will
not tell you a frame is *correct* — a frame can pass every line here and still
be the wrong camera, the wrong level or the wrong exposure. It only refuses to
let a black rectangle be quoted as evidence.
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image

# A frame that fails any of these is not usable as evidence.
MIN_INK = 0.10  # >=10% of pixels carry light
MIN_CENTRE_INK = 0.20  # >=20% of the HUD-free middle carries light
MIN_UNIQ = 500  # >=500 distinct quantised colours
MIN_EDGES = 1.0  # mean gradient magnitude, 0-255 units


def stats(a: np.ndarray) -> dict:
    g = a.mean(axis=2)
    ink = float((g > 8).mean())
    q = (a // 8).astype(np.int32)
    uniq = int(len(np.unique(q[..., 0] * 4096 + q[..., 1] * 64 + q[..., 2])))
    dx = np.abs(np.diff(g, axis=1)).mean() if g.shape[1] > 1 else 0.0
    dy = np.abs(np.diff(g, axis=0)).mean() if g.shape[0] > 1 else 0.0
    return {"ink": ink, "uniq": uniq, "edges": float(dx + dy)}


def check(p: Path) -> bool:
    a = np.asarray(Image.open(p).convert("RGB"), dtype=np.float64)
    h, w = a.shape[:2]
    whole = stats(a)
    centre = stats(a[h // 4 : 3 * h // 4, w // 4 : 3 * w // 4])
    fails = []
    if whole["ink"] < MIN_INK:
        fails.append(f"ink {whole['ink']:.3f} < {MIN_INK}")
    if centre["ink"] < MIN_CENTRE_INK:
        fails.append(f"centre.ink {centre['ink']:.3f} < {MIN_CENTRE_INK} (dead world, live HUD?)")
    if whole["uniq"] < MIN_UNIQ:
        fails.append(f"uniq {whole['uniq']} < {MIN_UNIQ}")
    if centre["edges"] < MIN_EDGES:
        fails.append(f"centre.edges {centre['edges']:.2f} < {MIN_EDGES} (flat fill?)")
    ok = not fails
    print(
        f"{'PASS' if ok else 'FAIL'}  {p.name:38s} "
        f"ink={whole['ink']:.3f} uniq={whole['uniq']:6d} edges={whole['edges']:6.2f} | "
        f"centre ink={centre['ink']:.3f} uniq={centre['uniq']:6d} edges={centre['edges']:6.2f}"
    )
    for f in fails:
        print(f"      ! {f}")
    return ok


def main() -> int:
    paths = [Path(p) for p in sys.argv[1:]]
    if not paths:
        print(__doc__)
        return 2
    return 0 if all([check(p) for p in paths]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
