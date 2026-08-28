# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
"""
sky_isolate.py — segment the sky out of two frames and compare the CLOUDS.

    uv run scripts/sky_isolate.py <original.png> <port.png> [--out DIR]

WHY THIS EXISTS

`parity_metrics.py --region sky` takes a horizontal band and reports its mean
colour. That is blind to the thing that actually differs: a band containing
white cloud and blue sky averages to a colour that can match perfectly while the
cloud MORPHOLOGY — how much, how big, how soft, how structured — is nothing
alike. Mean RGB over a mixed region is the wrong instrument for cloud, and
reporting it as "the sky matches" is how you miss a whole subsystem.

So this segments sky from geometry first, then measures the cloud FIELD:

  coverage        what fraction of sky is cloud rather than blue
  edge softness   mean gradient across cloud boundaries. A procedural fbm with
                  erosion has soft, fractal edges; a few summed sine products
                  have hard, rounded ones.
  size / scale    the radial power spectrum of the sky ALONE, in six bands.
                  Real cumulus carries energy across every scale (that is what
                  "fractal" means); a low-octave field carries it in one or two.
  octave decay    how fast power falls from band to band. An fbm with N octaves
                  decays smoothly; a sum of a few sines has a spiky spectrum.
  bimodality      soft clouds blend continuously into sky; hard-edged blobs give
                  two separated populations in the luminance histogram.
  gradient        the blue gradient zenith->horizon, measured with cloud pixels
                  EXCLUDED, which is the only way to compare the atmosphere
                  itself rather than the weather in front of it.

SEGMENTATION

Sky is taken as: in the upper part of the frame, and either blue-dominant
(B > R by a margin) or bright and low-saturation (cloud). Then the largest
connected run per column from the top down is kept, which drops bright building
faces that happen to pass the colour test — sky is contiguous from the top edge,
a wall is not.
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
from PIL import Image


def load(p: str) -> np.ndarray:
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.float64) / 255.0


def luma(img: np.ndarray) -> np.ndarray:
    return img @ np.array([0.2126, 0.7152, 0.0722])


def sky_mask(img: np.ndarray) -> np.ndarray:
    """Contiguous-from-the-top sky, excluding geometry."""
    h, w = img.shape[:2]
    r, g, b = img[..., 0], img[..., 1], img[..., 2]
    l = luma(img)
    mx = img.max(axis=2)
    mn = img.min(axis=2)
    sat = (mx - mn) / np.maximum(mx, 1e-6)

    blue = (b > r + 0.02) & (l > 0.25)          # open sky
    cloud = (l > 0.55) & (sat < 0.25)            # bright, near-neutral
    cand = blue | cloud
    cand[int(h * 0.65) :, :] = False             # never below the horizon band

    # Keep only the run that touches the top edge, per column. Sky is contiguous
    # from the top of the frame; a sunlit wall or a white awning is not.
    out = np.zeros((h, w), dtype=bool)
    for x in range(w):
        col = cand[:, x]
        if not col[0]:
            # allow the HUD strip at the very top
            start = 0
            while start < h and not col[start] and start < int(h * 0.08):
                start += 1
            if start >= int(h * 0.08):
                continue
        else:
            start = 0
        y = start
        while y < h and col[y]:
            out[y, x] = True
            y += 1
    return out


def cloud_mask(img: np.ndarray, sky: np.ndarray) -> np.ndarray:
    """Cloud within sky: brighter and less blue than the local sky gradient."""
    l = luma(img)
    r, b = img[..., 0], img[..., 2]
    # "cloud" = neutral-ish and above the column's own blue level, so this
    # adapts to the zenith->horizon gradient instead of using a flat threshold.
    blueness = b - r
    thr = np.percentile(blueness[sky], 35) if sky.any() else 0.0
    return sky & (blueness < thr) & (l > np.percentile(l[sky], 45))


def radial_bands(field: np.ndarray, mask: np.ndarray, nb: int = 6) -> list[float]:
    """Radial power spectrum of the masked field, mean-filled outside the mask."""
    x = field.copy()
    fill = x[mask].mean() if mask.any() else 0.0
    x[~mask] = fill
    x = x - x.mean()
    # Hann window so the mask boundary does not dominate the spectrum.
    h, w = x.shape
    wy = np.hanning(h)[:, None]
    wx = np.hanning(w)[None, :]
    f = np.abs(np.fft.fftshift(np.fft.fft2(x * wy * wx))) ** 2
    cy, cx = h // 2, w // 2
    yy, xx = np.ogrid[:h, :w]
    rr = np.sqrt((yy - cy) ** 2 + (xx - cx) ** 2)
    rmax = min(cy, cx)
    edges = np.linspace(1, rmax, nb + 1)
    out = []
    for i in range(nb):
        sel = (rr >= edges[i]) & (rr < edges[i + 1])
        out.append(float(f[sel].mean()))
    tot = sum(out) or 1.0
    return [v / tot for v in out]


def analyse(img: np.ndarray, name: str) -> dict:
    sky = sky_mask(img)
    if sky.sum() < 500:
        return {"name": name, "error": "no sky segmented"}
    cloud = cloud_mask(img, sky)
    l = luma(img)

    # Edge softness: mean gradient magnitude ON cloud boundaries.
    gx = np.zeros_like(l); gy = np.zeros_like(l)
    gx[:, 1:-1] = l[:, 2:] - l[:, :-2]
    gy[1:-1, :] = l[2:, :] - l[:-2, :]
    grad = np.sqrt(gx ** 2 + gy ** 2)
    boundary = cloud ^ (np.roll(cloud, 1, 0) & np.roll(cloud, 1, 1))
    edge = float(grad[boundary & sky].mean()) if (boundary & sky).any() else 0.0

    bands = radial_bands(l, sky)
    # Octave decay: mean ratio between adjacent bands. Smooth fbm decays
    # steadily; a few sines give a spiky, irregular profile.
    ratios = [bands[i + 1] / max(bands[i], 1e-12) for i in range(len(bands) - 1)]

    ls = l[sky]
    mu, sd = ls.mean(), ls.std()
    z = (ls - mu) / max(sd, 1e-9)
    bimod = float(((z ** 3).mean() ** 2 + 1) / max((z ** 4).mean(), 1e-9))

    # Blue gradient with cloud EXCLUDED — the atmosphere itself.
    open_sky = sky & ~cloud
    ys, xs = np.where(open_sky)
    top = open_sky.copy(); top[int(img.shape[0] * 0.25) :, :] = False
    bot = open_sky.copy(); bot[: int(img.shape[0] * 0.25), :] = False
    zen = img[top].mean(axis=0) if top.any() else np.zeros(3)
    hor = img[bot].mean(axis=0) if bot.any() else np.zeros(3)

    return {
        "name": name,
        "sky_px": int(sky.sum()),
        "cloud_coverage": float(cloud.sum() / sky.sum()),
        "cloud_edge_gradient": edge,
        "sky_luma_std": float(sd),
        "bimodality": bimod,
        "radial_bands": bands,
        "octave_ratios": ratios,
        "high_band_share": float(sum(bands[-3:])),
        "open_sky_zenith": [float(v) for v in zen],
        "open_sky_horizon": [float(v) for v in hor],
        "zenith_horizon_luma_drop": float(luma(zen[None, :])[0] - luma(hor[None, :])[0]),
        "_sky": sky, "_cloud": cloud,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("original"); ap.add_argument("port")
    ap.add_argument("--out", default="scripts/.playwright-controller/screenshots")
    a = ap.parse_args()

    ia, ib = load(a.original), load(a.port)
    ra, rb = analyse(ia, "original"), analyse(ib, "port")
    out = Path(a.out)

    for img, r, tag in ((ia, ra, "original"), (ib, rb, "port")):
        if "error" in r:
            continue
        # sky-only crop, geometry blacked out
        vis = (img * r["_sky"][..., None] * 255).astype(np.uint8)
        Image.fromarray(vis).save(out / f"sky-{tag}.png")
        # cloud mask alone, so the morphology is visible with colour removed
        Image.fromarray((r["_cloud"] * 255).astype(np.uint8)).save(out / f"cloudmask-{tag}.png")

    def row(k, fa, fb, fmt="%.4f", ratio=True):
        va, vb = ra.get(k), rb.get(k)
        if va is None or vb is None:
            return
        extra = f"   x{vb / va:.3f}" if ratio and abs(va) > 1e-9 else ""
        print(f"    {k:26s} orig {fmt % va:>10s}   port {fmt % vb:>10s}{extra}")

    print(f"\n=== sky isolation — {ra.get('sky_px', 0):,} vs {rb.get('sky_px', 0):,} sky px ===\n")
    print("  CLOUD FIELD")
    row("cloud_coverage", 0, 0)
    row("cloud_edge_gradient", 0, 0)
    row("bimodality", 0, 0)
    row("sky_luma_std", 0, 0)
    row("high_band_share", 0, 0)
    print("\n  RADIAL POWER (6 bands, coarse -> fine, share of total)")
    print(f"    original  {['%.4f' % v for v in ra['radial_bands']]}")
    print(f"    port      {['%.4f' % v for v in rb['radial_bands']]}")
    print("\n  OCTAVE DECAY (adjacent-band ratios; smooth fbm decays evenly)")
    print(f"    original  {['%.3f' % v for v in ra['octave_ratios']]}")
    print(f"    port      {['%.3f' % v for v in rb['octave_ratios']]}")
    print("\n  OPEN SKY ONLY (cloud pixels excluded — the atmosphere itself)")
    print(f"    zenith    orig {['%.3f' % v for v in ra['open_sky_zenith']]}"
          f"   port {['%.3f' % v for v in rb['open_sky_zenith']]}")
    print(f"    horizon   orig {['%.3f' % v for v in ra['open_sky_horizon']]}"
          f"   port {['%.3f' % v for v in rb['open_sky_horizon']]}")
    row("zenith_horizon_luma_drop", 0, 0)
    print(f"\n  wrote sky-*.png and cloudmask-*.png to {out}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
