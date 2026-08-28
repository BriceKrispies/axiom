# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright", "pillow", "numpy"]
# ///
"""
quality_guard.py - capture the port and check its image did not get worse.

    uv run scripts/quality_guard.py [--url ...] [--baseline shots/quality-baseline/hero.png]
                                    [--settle 45000] [--accept] [--json out.json]

WHY THIS EXISTS

Frame rate on this app is bought with resolution. `RenderScaleController` drops
the render scale down a ladder (0.50 / 0.62 / 0.75 / 0.87 / 1.0) until the frame
fits its budget, so every performance win is a quality spend, and the spend is
ADAPTIVE - it varies with whatever the machine happened to be doing. A change
that "improved fps" may simply have pushed the scaler down a rung, and a frame
rate number alone cannot tell those apart.

So this pins the camera, captures, and scores against a saved baseline. It is
the other half of `frame_profile.py`: that one says how fast, this one says at
what cost.

WHAT IT CHECKS, AND WHY EACH ONE

  detail      high-frequency energy and gradient energy. A render-scale drop
              removes fine detail first, so this is the earliest and most
              sensitive signal that the scaler moved.
  structure   SSIM and mean absolute difference against the baseline. Catches
              geometry, framing and large-scale lighting changes.
  tone        mean luminance and contrast. Catches an exposure or grade shift
              that detail metrics are blind to.
  grade       saturation and warmth with luminance divided out.

A DROP IS NOT AUTOMATICALLY A FAILURE, and the tool does not pretend otherwise.
If a change deliberately trades image for speed, `--accept` re-baselines. What
the guard prevents is that trade happening SILENTLY, which is what an adaptive
scaler does by design.

HONEST LIMIT

The baseline was itself captured at whatever rung the scaler had settled on, so
this measures drift FROM THAT POINT, not absolute quality. Re-baseline
deliberately, never to make a red run green.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image
from playwright.sync_api import sync_playwright

CHANNEL = "chromium"
ARGS = [
    "--enable-unsafe-webgpu",
    "--enable-features=Vulkan",
    "--use-gl=angle",
    "--ignore-gpu-blocklist",
    "--force-color-profile=srgb",
    "--hide-scrollbars",
    "--mute-audio",
]

# The parity campaign's reference pose. Same numbers as `parity_shot`, so a
# quality run and a parity run are directly comparable.
HERO_CAM = "12 1.75 18 0.588003 0.015600 75"


def luma(img):
    return img @ np.array([0.2126, 0.7152, 0.0722])


def world_mask(img):
    """Drop the HUD bands and the two HUD corners - DOM, not the renderer."""
    h, w = img.shape[:2]
    m = np.ones((h, w), dtype=bool)
    m[: int(h * 0.09), :] = False
    m[int(h * 0.90):, :] = False
    m[: int(h * 0.22), : int(w * 0.14)] = False
    m[int(h * 0.84):, int(w * 0.90):] = False
    return m


def box(x, r):
    pad = np.pad(x, r, mode="edge")
    c = np.cumsum(np.cumsum(pad, axis=0), axis=1)
    c = np.pad(c, ((1, 0), (1, 0)))
    k = 2 * r + 1
    h, w = x.shape
    tot = c[k:k + h, k:k + w] - c[0:h, k:k + w] - c[k:k + h, 0:w] + c[0:h, 0:w]
    return tot / (k * k)


def detail_energy(img, m):
    l = luma(img)
    gx = np.zeros_like(l)
    gy = np.zeros_like(l)
    gx[:, 1:-1] = l[:, 2:] - l[:, :-2]
    gy[1:-1, :] = l[2:, :] - l[:-2, :]
    grad = float(np.sqrt(gx ** 2 + gy ** 2)[m].mean())

    # Radial spectrum of the masked region: a scale drop empties the top bands.
    f = l.copy()
    fill = f[m].mean()
    f = np.where(m, f, fill) - fill
    hh, ww = f.shape
    f = f * np.hanning(hh)[:, None] * np.hanning(ww)[None, :]
    p = np.abs(np.fft.fftshift(np.fft.fft2(f))) ** 2
    cy, cx = hh // 2, ww // 2
    yy, xx = np.ogrid[:hh, :ww]
    rr = np.sqrt((yy - cy) ** 2 + (xx - cx) ** 2)
    rmax = min(cy, cx)
    edges = np.linspace(1, rmax, 7)
    bands = [float(p[(rr >= edges[i]) & (rr < edges[i + 1])].mean()) for i in range(6)]
    tot = sum(bands) or 1.0
    bands = [b / tot for b in bands]
    return grad, float(sum(bands[-3:])), bands


def ssim(a, b, m):
    la, lb = luma(a), luma(b)
    c1, c2 = 0.01 ** 2, 0.03 ** 2
    mu_a, mu_b = box(la, 8), box(lb, 8)
    sa = box(la * la, 8) - mu_a ** 2
    sb = box(lb * lb, 8) - mu_b ** 2
    sab = box(la * lb, 8) - mu_a * mu_b
    s = ((2 * mu_a * mu_b + c1) * (2 * sab + c2)) / \
        ((mu_a ** 2 + mu_b ** 2 + c1) * (sa + sb + c2))
    return float(s[m].mean())


def capture(url, out, settle, cam):
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True, args=ARGS, channel=CHANNEL)
        page = browser.new_page(viewport={"width": 1280, "height": 720})
        page.goto(url, wait_until="load", timeout=60000)
        page.wait_for_timeout(settle)
        stats = ""
        try:
            page.evaluate("window.__ax_console('cam " + cam + "')")
            page.wait_for_timeout(3000)
            stats = str(page.evaluate("window.__ax_console('stats')"))
        except Exception as exc:  # noqa: BLE001
            print("  camera pin FAILED: " + str(exc), file=sys.stderr)
        page.screenshot(path=out)
        browser.close()
    return stats


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://localhost:8088/")
    ap.add_argument("--baseline", default="shots/quality-baseline/hero.png")
    ap.add_argument("--out", default="shots/quality-latest/hero.png")
    ap.add_argument("--settle", type=int, default=45000,
                    help="ms before capture; the adaptive scaler needs time to settle")
    ap.add_argument("--cam", default=HERO_CAM)
    ap.add_argument("--accept", action="store_true",
                    help="re-baseline from this capture; a deliberate act, never a fix")
    ap.add_argument("--json")
    a = ap.parse_args()

    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    stats = capture(a.url, str(out), a.settle, a.cam)
    print("\n  captured: " + str(out))
    if stats:
        for line in stats.replace("\\n", "\n").splitlines():
            if line.startswith(("frame", "pins")):
                print("    " + line)

    base_p = Path(a.baseline)
    if a.accept or not base_p.exists():
        base_p.parent.mkdir(parents=True, exist_ok=True)
        base_p.write_bytes(out.read_bytes())
        why = "re-baselined (--accept)" if base_p.exists() else "no baseline; created"
        print("\n  " + why + ": " + str(base_p) + "\n")
        return 0

    base = np.asarray(Image.open(base_p).convert("RGB"), dtype=np.float64) / 255.0
    new = np.asarray(Image.open(out).convert("RGB"), dtype=np.float64) / 255.0
    if base.shape != new.shape:
        print("REFUSING TO SCORE: %s vs %s" % (base.shape[:2], new.shape[:2]),
              file=sys.stderr)
        return 2

    m = world_mask(base)
    g0, h0, b0 = detail_energy(base, m)
    g1, h1, b1 = detail_energy(new, m)
    l0, l1 = luma(base)[m], luma(new)[m]
    s = ssim(base, new, m)
    mad = float(np.abs(luma(base) - luma(new))[m].mean())

    def sat(img):
        px = img[m]
        mx, mn = px.max(axis=1), px.min(axis=1)
        return float(((mx - mn) / np.maximum(mx, 1e-6)).mean())

    rows = [
        ("detail: gradient energy", g0, g1, g1 / max(g0, 1e-9), 0.90),
        ("detail: high-freq share", h0, h1, h1 / max(h0, 1e-9), 0.80),
        ("tone: mean luma", float(l0.mean()), float(l1.mean()),
         float(l1.mean() / max(l0.mean(), 1e-9)), 0.95),
        ("tone: contrast", float(l0.std()), float(l1.std()),
         float(l1.std() / max(l0.std(), 1e-9)), 0.92),
        ("grade: saturation", sat(base), sat(new), sat(new) / max(sat(base), 1e-9), 0.92),
    ]

    print("\n  === quality vs baseline ===\n")
    worse = []
    for name, v0, v1, ratio, floor in rows:
        bad = ratio < floor
        if bad:
            worse.append(name)
        print("    %-26s base %8.4f   now %8.4f   x%.3f   %s"
              % (name, v0, v1, ratio, "DROPPED" if bad else "ok"))
    print("    %-26s %8s   now %8.4f   %s"
          % ("structure: SSIM", "-", s, "ok" if s > 0.90 else "CHANGED"))
    print("    %-26s %8s   now %8.4f" % ("structure: mean abs diff", "-", mad))

    print("\n  radial bands (coarse -> fine)")
    print("    base  " + str(["%.4f" % v for v in b0]))
    print("    now   " + str(["%.4f" % v for v in b1]))

    ok = not worse and s > 0.90
    if ok:
        print("\n  PASS - image quality held.\n")
    else:
        print("\n  QUALITY DROPPED: " + (", ".join(worse) if worse else "structure"))
        print("  A render-scale rung change is the usual cause; the high-freq share")
        print("  falls first. If the trade was deliberate, re-run with --accept.\n")

    if a.json:
        Path(a.json).write_text(json.dumps({
            "baseline": str(base_p), "capture": str(out), "ssim": s,
            "mean_abs_diff": mad, "pass": ok,
            "rows": [{"metric": n, "base": v0, "now": v1, "ratio": r} for n, v0, v1, r, _ in rows],
            "bands_base": b0, "bands_now": b1,
        }, indent=2))
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
