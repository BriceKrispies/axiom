# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy", "scipy"]
# ///
"""
shadow_structure.py — is there a CAST SHADOW here, or just dark pixels?

    uv run scripts/shadow_structure.py <orig.png> <port.png>
        [--rect X0,Y0,X1,Y1] [--dump-mask PREFIX]

WHY THE EXISTING SHADOW METRICS CANNOT ANSWER THIS

Two instruments are in use in this campaign and they disagree:

  * `bimodality` (Sarle) says the port has less lit/shadow separation
    (0.656 vs 0.541 on an earlier pair).
  * a road `p90:p10` luminance ratio says the port has MORE range
    (10.5 original vs 57 port), which was read as "both frames have real
    shadow structure".

Both are the wrong shape of question, for different reasons.

`p90:p10` is a ratio whose denominator can approach zero. A handful of
near-black outlier pixels — one dark rock, a sliver of the weapon viewmodel
intruding into the region, a black decal — drags p10 toward 0 and sends the
ratio to infinity. A ratio of 57 is therefore *evidence of a few very dark
pixels*, not evidence of a shadow, and it carries no information at all about
how much of the region is shadowed or whether the dark pixels are adjacent to
each other.

`bimodality` is a better question — it asks about the SHAPE of the luminance
distribution rather than its extremes — but it is still purely a histogram
statistic, and a histogram has no idea where the pixels are. A region that is
half-lit and half-shadowed and a region with the same pixels sprinkled at
random as salt-and-pepper noise produce the IDENTICAL histogram and the
identical bimodality coefficient. Both metrics are spatially blind, and
spatial arrangement is the entire difference between a cast shadow and noise.

WHAT A CAST SHADOW ACTUALLY IS

A cast shadow is a *large contiguous region* of reduced luminance with a
*coherent boundary*. That is a statement about connected components, so this
measures connected components:

  dark_frac       fraction of the region below the shadow threshold. The
                  threshold is Otsu's, computed on the ORIGINAL and reused for
                  the port, so both are cut at the same luminance — otherwise
                  each image gets its own threshold and a uniformly-lit region
                  is scored as 50% shadowed.
  n_blobs         connected dark components larger than 64 px.
  largest_frac    the largest single dark component as a fraction of the
                  region. THIS is the number that separates a cast shadow from
                  noise: one big shadow is ~0.2-0.6, scattered speckle is
                  ~0.001.
  top3_frac       the three largest together — a shadow broken by a kerb still
                  reads high here.
  compactness     largest blob area / its bounding-box area. A real shadow is a
                  solid slab (0.5-0.9); a sprawling speckle field that happens
                  to percolate into one component has a huge bounding box and
                  scores near 0.
  edge_strength   mean luminance gradient along the dark region's boundary. A
                  cast shadow has a step at its edge; a smooth ambient
                  gradient does not.

It also prints bimodality and p90:p10 on the same pixels so the three
instruments can be read side by side against the same evidence.

BLIND SPOTS OF THIS METRIC, STATED UP FRONT

It cannot tell a cast shadow from any other large dark object — a dark awning,
a black doorway, or the weapon viewmodel would all score as one big compact
blob. So the rect must be chosen to contain only ground, and `--dump-mask`
exists so the mask can be looked at rather than trusted. It also says nothing
about whether the shadow is in the RIGHT PLACE; two frames can both score
`largest_frac=0.4` with the shadows on opposite sides of the road.
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage

MIN_BLOB = 64


def luma(a: np.ndarray) -> np.ndarray:
    return a[..., 0] * 0.2126 + a[..., 1] * 0.7152 + a[..., 2] * 0.0722


def otsu(v: np.ndarray) -> float:
    hist, edges = np.histogram(v, bins=256, range=(0.0, 1.0))
    hist = hist.astype(np.float64)
    total = hist.sum()
    w0 = np.cumsum(hist)
    w1 = total - w0
    centres = (edges[:-1] + edges[1:]) / 2
    m0 = np.cumsum(hist * centres)
    mt = m0[-1]
    with np.errstate(invalid="ignore", divide="ignore"):
        between = (mt * w0 / total - m0) ** 2 / (w0 * w1 / total**2 + 1e-12) / total**2
    between = np.nan_to_num(between)
    return float(centres[int(np.argmax(between))])


def bimodality(v: np.ndarray) -> float:
    """Sarle's coefficient. >5/9 suggests two populations."""
    n = v.size
    m = v.mean()
    s = v.std()
    g = float(((v - m) ** 3).mean() / (s**3 + 1e-12))
    k = float(((v - m) ** 4).mean() / (s**4 + 1e-12)) - 3.0
    return (g**2 + 1.0) / (k + 3.0 * (n - 1) ** 2 / ((n - 2) * (n - 3)) + 1e-12)


def analyse(name: str, v: np.ndarray, thr: float, dump: Path | None) -> dict:
    mask = v < thr
    lab, n = ndimage.label(mask)
    sizes = np.bincount(lab.ravel())[1:] if n else np.array([], dtype=int)
    big = np.sort(sizes[sizes >= MIN_BLOB])[::-1] if sizes.size else np.array([], dtype=int)
    area = float(v.size)
    largest = float(big[0]) if big.size else 0.0
    top3 = float(big[:3].sum()) if big.size else 0.0

    compact = 0.0
    if big.size:
        idx = int(np.argmax(sizes)) + 1
        ys, xs = np.nonzero(lab == idx)
        bb = (ys.max() - ys.min() + 1) * (xs.max() - xs.min() + 1)
        compact = largest / float(bb)

    # Gradient magnitude sampled on the mask boundary.
    gy, gx = np.gradient(v)
    grad = np.hypot(gx, gy)
    border = mask ^ ndimage.binary_erosion(mask)
    edge = float(grad[border].mean()) if border.any() else 0.0

    p10, p90 = np.percentile(v, [10, 90])
    res = {
        "dark_frac": float(mask.mean()),
        "n_blobs": int(big.size),
        "largest_frac": largest / area,
        "top3_frac": top3 / area,
        "compactness": compact,
        "edge_strength": edge,
        "bimodality": bimodality(v),
        "p90_p10": float(p90 / max(p10, 1e-6)),
        "p10": float(p10),
        "p90": float(p90),
        "mean": float(v.mean()),
    }
    if dump is not None:
        Image.fromarray((mask * 255).astype(np.uint8)).save(dump)
    return res


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("orig")
    ap.add_argument("port")
    ap.add_argument("--rect", default="40,400,560,660", help="X0,Y0,X1,Y1 ground-only region")
    ap.add_argument("--dump-mask", default=None)
    a = ap.parse_args()

    x0, y0, x1, y1 = (int(t) for t in a.rect.split(","))
    ims = {}
    for key, p in (("orig", a.orig), ("port", a.port)):
        arr = np.asarray(Image.open(p).convert("RGB"), dtype=np.float64) / 255.0
        ims[key] = luma(arr[y0:y1, x0:x1])

    thr = otsu(ims["orig"])
    print(f"=== shadow structure  rect=({x0},{y0})-({x1},{y1})  {ims['orig'].size:,} px ===")
    print(f"    threshold {thr:.4f} (Otsu on ORIGINAL, reused for both)\n")

    rows = {}
    for key in ("orig", "port"):
        dump = Path(f"{a.dump_mask}.{key}.png") if a.dump_mask else None
        rows[key] = analyse(key, ims[key], thr, dump)

    keys = [
        "mean", "dark_frac", "n_blobs", "largest_frac", "top3_frac",
        "compactness", "edge_strength", "bimodality", "p10", "p90", "p90_p10",
    ]
    print(f"    {'metric':16s} {'orig':>12s} {'port':>12s}   ratio")
    for k in keys:
        o, p = rows["orig"][k], rows["port"][k]
        r = f"x{p / o:.3f}" if o else "  n/a"
        print(f"    {k:16s} {o:12.4f} {p:12.4f}   {r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
