# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow", "numpy"]
# ///
"""
parity_metrics.py — many different pixel comparisons between two frames.

    uv run scripts/parity_metrics.py <original.png> <port.png> [--json out.json]
                                     [--region world|full|sky|ground|viewmodel]
                                     [--diff out_diff.png]

WHY MORE THAN ONE METRIC

A single "percent different" number is worthless across two renderers. They will
never be byte-equal (`docs/work-manifests/shmup-port/10-convergence-plan.md`),
and a raw diff conflates every way they can differ into one scalar that cannot
tell you which one moved. Worse, some metrics are actively misleading when the
two frames are not perfectly matched — a whole-frame mean carries the *dressing*
gap as well as the *exposure* gap, which is how a fit can be confidently wrong.

So this reports a battery, grouped by what each family is blind to. Read them
together: the useful signal is usually which family disagrees with the others.

    TONE       exposure, contrast, black/white point.
               Spatially blind on purpose — valid even if framing differs.
    GRADE      hue and saturation with luminance divided out.
               Blind to exposure, so it separates "wrong brightness" from
               "wrong colour", which a mean luma cannot do.
    STRUCTURE  per-pixel and structural agreement.
               ONLY meaningful at a matched camera; reported as UNSAFE otherwise.
    TEXTURE    high-frequency energy and its distribution across spatial scales.
               Answers "is there surface detail" independently of what is in
               frame — a flat tinted wall and a textured one differ here even
               when their means agree exactly.
    LIGHTING   shadow/light separation.
               A scene with real cast shadows is BIMODAL in luminance; one lit
               uniformly is not. This detects missing shadows without needing to
               know where they should fall.

Every metric prints its own SAME/CLOSE/OFF verdict against a stated threshold,
so the output is a work list rather than a wall of numbers.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

# --------------------------------------------------------------------------
# Regions. The HUD is DOM drawn over the canvas and is a different subsystem
# from the renderer, so it is excluded by default; so is the viewmodel, which
# moves in view space and has its own sway/bob phase that no camera pin fixes.
# --------------------------------------------------------------------------

REGIONS = {
    "full": None,
    # Everything but the HUD bands and the viewmodel's lower-right wedge.
    "world": [("hud_top", 0.00, 0.09), ("hud_bottom", 0.90, 1.00)],
    "sky": [("keep", 0.00, 0.35)],
    "ground": [("keep", 0.55, 0.88)],
    "viewmodel": [("keep", 0.55, 0.98)],
}


def load(path: str) -> np.ndarray:
    """Load as float RGB in 0..1, display-referred."""
    return np.asarray(Image.open(path).convert("RGB"), dtype=np.float64) / 255.0


def luma(img: np.ndarray) -> np.ndarray:
    return img @ np.array([0.2126, 0.7152, 0.0722])


def mask_for(img: np.ndarray, region: str) -> np.ndarray:
    """Boolean mask of pixels this region keeps."""
    h, w = img.shape[:2]
    m = np.ones((h, w), dtype=bool)
    if region == "full":
        return m
    if region in ("sky", "ground", "viewmodel"):
        m[:] = False
        for _, a, b in REGIONS[region]:
            m[int(h * a) : int(h * b), :] = True
        if region == "viewmodel":
            # lower-right wedge only
            m[:, : int(w * 0.42)] = False
        return m
    # "world": drop the HUD bands, the minimap corner and the ammo corner.
    for _, a, b in REGIONS["world"]:
        m[int(h * a) : int(h * b), :] = False
    m[: int(h * 0.22), : int(w * 0.14)] = False   # minimap
    m[int(h * 0.84) :, int(w * 0.90) :] = False   # ammo
    return m


def verdict(value: float, close: float, off: float, invert: bool = False) -> str:
    """SAME / CLOSE / OFF against two thresholds."""
    v = abs(value)
    if invert:
        return "SAME" if v >= off else ("CLOSE" if v >= close else "OFF")
    return "SAME" if v <= close else ("CLOSE" if v <= off else "OFF")


# --------------------------------------------------------------------------
# TONE — spatially blind. Valid even when the framing does not match.
# --------------------------------------------------------------------------


def tone(a: np.ndarray, b: np.ndarray, m: np.ndarray) -> dict:
    la, lb = luma(a)[m], luma(b)[m]
    qs = [1, 5, 10, 25, 50, 75, 90, 95, 99]
    pa, pb = np.percentile(la, qs), np.percentile(lb, qs)

    # Histogram distance: 64-bin luminance, L1 (a true earth-mover needs the
    # cumulative form, which is what `emd` below is).
    ha, _ = np.histogram(la, bins=64, range=(0, 1), density=True)
    hb, _ = np.histogram(lb, bins=64, range=(0, 1), density=True)
    ha, hb = ha / ha.sum(), hb / hb.sum()
    emd = float(np.abs(np.cumsum(ha) - np.cumsum(hb)).sum() / 64.0)

    # Exposure error expressed in stops, which is the unit the fix is authored
    # in — a ratio of 1.15 is "+0.2 stop", not "15% wrong".
    stops = float(np.log2(max(lb.mean(), 1e-6) / max(la.mean(), 1e-6)))

    return {
        "mean_luma": {"original": float(la.mean()), "port": float(lb.mean()),
                      "ratio": float(lb.mean() / max(la.mean(), 1e-9)),
                      "stops": stops, "verdict": verdict(stops, 0.15, 0.5)},
        "median_luma": {"original": float(np.median(la)), "port": float(np.median(lb))},
        "black_point_p1": {"original": float(pa[0]), "port": float(pb[0]),
                           "delta": float(pb[0] - pa[0]),
                           "verdict": verdict(pb[0] - pa[0], 0.02, 0.06)},
        "white_point_p99": {"original": float(pa[-1]), "port": float(pb[-1]),
                            "delta": float(pb[-1] - pa[-1]),
                            "verdict": verdict(pb[-1] - pa[-1], 0.03, 0.10)},
        "dynamic_range_p99_p1": {"original": float(pa[-1] - pa[0]),
                                 "port": float(pb[-1] - pb[0])},
        "global_contrast_std": {"original": float(la.std()), "port": float(lb.std()),
                                "ratio": float(lb.std() / max(la.std(), 1e-9)),
                                "verdict": verdict(lb.std() / max(la.std(), 1e-9) - 1, 0.12, 0.35)},
        "histogram_emd": {"value": emd, "verdict": verdict(emd, 0.03, 0.10)},
        "decile_curve": {"quantiles": qs,
                         "original": [float(x) for x in pa],
                         "port": [float(x) for x in pb],
                         "max_abs_delta": float(np.abs(pb - pa).max())},
    }


# --------------------------------------------------------------------------
# GRADE — luminance divided out, so this is blind to exposure entirely.
# --------------------------------------------------------------------------


def grade(a: np.ndarray, b: np.ndarray, m: np.ndarray) -> dict:
    def chroma(img):
        px = img[m]
        l = px @ np.array([0.2126, 0.7152, 0.0722])
        return px / np.maximum(l, 1e-6)[:, None]

    ca, cb = chroma(a), chroma(b)
    mca, mcb = ca.mean(axis=0), cb.mean(axis=0)

    def sat(img):
        px = img[m]
        mx, mn = px.max(axis=1), px.min(axis=1)
        return (mx - mn) / np.maximum(mx, 1e-6)

    sa, sb = sat(a), sat(b)

    # Warm/cool: R/B on the luma-normalised triple. This is the axis the two
    # frames most obviously differ on, and it survives any exposure error.
    wa = float(mca[0] / max(mca[2], 1e-6))
    wb = float(mcb[0] / max(mcb[2], 1e-6))

    # Split-tone: is the grade different in the shadows than the highlights?
    # Compared separately because a filmic grade deliberately tints them apart,
    # and a mean over the whole frame averages that signature away.
    la, lb = luma(a)[m], luma(b)[m]
    def tint(px, l, lo, hi):
        sel = (l >= lo) & (l < hi)
        if sel.sum() < 64:
            return None
        q = px[sel]
        ll = q @ np.array([0.2126, 0.7152, 0.0722])
        c = (q / np.maximum(ll, 1e-6)[:, None]).mean(axis=0)
        return float(c[0] / max(c[2], 1e-6))

    sh_a, sh_b = tint(a[m], la, 0.0, 0.25), tint(b[m], lb, 0.0, 0.25)
    hi_a, hi_b = tint(a[m], la, 0.6, 1.01), tint(b[m], lb, 0.6, 1.01)

    return {
        "mean_chromaticity": {"original": [float(x) for x in mca],
                             "port": [float(x) for x in mcb],
                             "max_abs_delta": float(np.abs(mcb - mca).max()),
                             "verdict": verdict(float(np.abs(mcb - mca).max()), 0.04, 0.12)},
        "warmth_r_over_b": {"original": wa, "port": wb, "ratio": float(wb / max(wa, 1e-9)),
                            "verdict": verdict(wb / max(wa, 1e-9) - 1, 0.08, 0.25)},
        "saturation": {"original": float(sa.mean()), "port": float(sb.mean()),
                       "ratio": float(sb.mean() / max(sa.mean(), 1e-9)),
                       "verdict": verdict(sb.mean() / max(sa.mean(), 1e-9) - 1, 0.10, 0.30)},
        "split_tone_shadow_warmth": {"original": sh_a, "port": sh_b},
        "split_tone_highlight_warmth": {"original": hi_a, "port": hi_b},
    }


# --------------------------------------------------------------------------
# STRUCTURE — only meaningful at a matched camera.
# --------------------------------------------------------------------------


def _box(x: np.ndarray, r: int) -> np.ndarray:
    """Separable box blur via cumulative sums — no scipy dependency."""
    pad = np.pad(x, r, mode="edge")
    c = np.cumsum(np.cumsum(pad, axis=0), axis=1)
    c = np.pad(c, ((1, 0), (1, 0)))
    k = 2 * r + 1
    h, w = x.shape
    tot = c[k:k + h, k:k + w] - c[0:h, k:k + w] - c[k:k + h, 0:w] + c[0:h, 0:w]
    return tot / (k * k)


def structure(a: np.ndarray, b: np.ndarray, m: np.ndarray) -> dict:
    la, lb = luma(a), luma(b)
    d = np.abs(la - lb)[m]

    # SSIM on luminance, 8-px box windows. The standard Gaussian window needs
    # scipy; a box window is the same estimator with a different kernel and is
    # adequate for a relative score.
    c1, c2 = 0.01 ** 2, 0.03 ** 2
    mu_a, mu_b = _box(la, 8), _box(lb, 8)
    sa = _box(la * la, 8) - mu_a ** 2
    sb = _box(lb * lb, 8) - mu_b ** 2
    sab = _box(la * lb, 8) - mu_a * mu_b
    ssim_map = ((2 * mu_a * mu_b + c1) * (2 * sab + c2)) / \
               ((mu_a ** 2 + mu_b ** 2 + c1) * (sa + sb + c2))

    # A perceptual diff at 1/8 scale: ignores texture noise and grain, keeps
    # layout and large-scale lighting. Two renderers can disagree everywhere at
    # full res and still agree here — which is the interesting question.
    small_a = np.asarray(Image.fromarray((la * 255).astype(np.uint8)).resize((160, 90)), dtype=np.float64) / 255
    small_b = np.asarray(Image.fromarray((lb * 255).astype(np.uint8)).resize((160, 90)), dtype=np.float64) / 255

    return {
        "mean_abs_diff": {"value": float(d.mean()), "verdict": verdict(float(d.mean()), 0.04, 0.12)},
        "p95_abs_diff": float(np.percentile(d, 95)),
        "max_abs_diff": float(d.max()),
        "changed_pct_tol_2_255": float((d > 2 / 255).mean() * 100),
        "changed_pct_tol_8_255": float((d > 8 / 255).mean() * 100),
        "ssim": {"value": float(ssim_map[m].mean()),
                 "verdict": verdict(float(ssim_map[m].mean()), 0.0, 0.0) if False
                 else ("SAME" if ssim_map[m].mean() > 0.85 else
                       "CLOSE" if ssim_map[m].mean() > 0.6 else "OFF")},
        "downsampled_mean_abs_diff": float(np.abs(small_a - small_b).mean()),
    }


# --------------------------------------------------------------------------
# TEXTURE — high-frequency energy. Answers "is there surface detail at all".
# --------------------------------------------------------------------------


def texture(a: np.ndarray, b: np.ndarray, m: np.ndarray) -> dict:
    def grad_energy(img):
        l = luma(img)
        gx = np.zeros_like(l); gy = np.zeros_like(l)
        gx[:, 1:-1] = l[:, 2:] - l[:, :-2]
        gy[1:-1, :] = l[2:, :] - l[:-2, :]
        return np.sqrt(gx ** 2 + gy ** 2)

    ga, gb = grad_energy(a)[m], grad_energy(b)[m]

    # Radial power spectrum: how much energy sits at each spatial frequency.
    # A 64-square bake upsampled over a 2 m tile loses the HIGH band while
    # keeping the low one, so this separates "wrong texture" from "no texture".
    def radial(img):
        # Mask-aware: the region argument MUST reach the spectrum, or every
        # region reports the same whole-frame number. It did not, and an agent
        # caught it quoting an identical `high_freq_fraction` for sky, ground and
        # world. Outside the mask is filled with the in-mask mean so the boundary
        # contributes no step edge of its own, and the whole field is Hann
        # windowed for the same reason.
        l = luma(img)
        fill = l[m].mean()
        l = np.where(m, l, fill)
        l = l - l[m].mean()
        hh, ww = l.shape
        l = l * np.hanning(hh)[:, None] * np.hanning(ww)[None, :]
        f = np.abs(np.fft.fftshift(np.fft.fft2(l))) ** 2
        h, w = f.shape
        cy, cx = h // 2, w // 2
        y, x = np.ogrid[:h, :w]
        r = np.sqrt((y - cy) ** 2 + (x - cx) ** 2).astype(int)
        nb = 6
        rmax = min(cy, cx)
        edges = np.linspace(0, rmax, nb + 1).astype(int)
        out = []
        for i in range(nb):
            sel = (r >= edges[i]) & (r < edges[i + 1])
            out.append(float(f[sel].mean()))
        tot = sum(out) or 1.0
        return [v / tot for v in out]

    ra, rb = radial(a), radial(b)

    return {
        "gradient_energy": {"original": float(ga.mean()), "port": float(gb.mean()),
                            "ratio": float(gb.mean() / max(ga.mean(), 1e-9)),
                            "verdict": verdict(gb.mean() / max(ga.mean(), 1e-9) - 1, 0.20, 0.50)},
        "high_freq_fraction": {
            "note": "share of spectral power in the top two of six radial bands",
            "original": float(sum(ra[-2:])), "port": float(sum(rb[-2:])),
            "ratio": float(sum(rb[-2:]) / max(sum(ra[-2:]), 1e-9)),
            "verdict": verdict(sum(rb[-2:]) / max(sum(ra[-2:]), 1e-9) - 1, 0.25, 0.60)},
        "radial_power_6band": {"original": ra, "port": rb},
    }


# --------------------------------------------------------------------------
# LIGHTING — shadow/light separation, without needing to know where.
# --------------------------------------------------------------------------


def lighting(a: np.ndarray, b: np.ndarray, m: np.ndarray) -> dict:
    la, lb = luma(a)[m], luma(b)[m]

    def bimodality(l):
        # Sarle's bimodality coefficient: (skew^2 + 1) / kurtosis.
        # > 5/9 suggests two populations — which is what "lit and shadowed"
        # looks like. A uniformly-lit scene collapses toward unimodal.
        n = l.size
        mu, sd = l.mean(), l.std()
        if sd < 1e-9:
            return 0.0
        z = (l - mu) / sd
        skew = float((z ** 3).mean())
        kurt = float((z ** 4).mean())
        return float((skew ** 2 + 1.0) / max(kurt, 1e-9))

    def shadow_fraction(l):
        return float((l < np.percentile(l, 50) * 0.55).mean())

    # Local contrast: std within 8-px windows, averaged. Cast shadows create
    # strong local edges that a global std can miss.
    def local_contrast(img):
        l = luma(img)
        mu = _box(l, 8)
        return float(np.sqrt(np.maximum(_box(l * l, 8) - mu ** 2, 0))[m].mean())

    ba, bb = bimodality(la), bimodality(lb)
    return {
        "bimodality": {
            "note": "Sarle's coefficient; >0.555 suggests distinct lit and shadowed populations",
            "original": ba, "port": bb,
            "verdict": "SAME" if abs(bb - ba) < 0.05 else ("CLOSE" if abs(bb - ba) < 0.15 else "OFF")},
        "deep_shadow_fraction": {"original": shadow_fraction(la), "port": shadow_fraction(lb),
                                 "ratio": float(shadow_fraction(lb) / max(shadow_fraction(la), 1e-9))},
        "local_contrast_8px": {"original": local_contrast(a), "port": local_contrast(b),
                               "ratio": float(local_contrast(b) / max(local_contrast(a), 1e-9)),
                               "verdict": verdict(local_contrast(b) / max(local_contrast(a), 1e-9) - 1, 0.15, 0.40)},
    }


def write_diff(a: np.ndarray, b: np.ndarray, out: str) -> None:
    """Magenta over a dimmed original, the convention `imagediff.mjs` uses."""
    d = np.abs(luma(a) - luma(b))
    base = (a * 0.25 * 255).astype(np.uint8)
    hot = d > (8 / 255)
    base[hot] = [255, 0, 255]
    Image.fromarray(base).save(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("original")
    ap.add_argument("port")
    ap.add_argument("--region", default="world", choices=list(REGIONS))
    ap.add_argument("--json")
    ap.add_argument("--diff")
    ap.add_argument("--matched-camera", action="store_true",
                    help="assert the two frames share a camera pose; STRUCTURE "
                         "metrics are reported as unsafe without it")
    args = ap.parse_args()

    a, b = load(args.original), load(args.port)
    if a.shape != b.shape:
        print(f"REFUSING TO SCORE: {a.shape[:2]} vs {b.shape[:2]}. "
              f"Resampling one to match would invent detail that is not there.",
              file=sys.stderr)
        return 2

    m = mask_for(a, args.region)
    report = {
        "original": args.original, "port": args.port,
        "region": args.region, "pixels_scored": int(m.sum()),
        "matched_camera": bool(args.matched_camera),
        "tone": tone(a, b, m),
        "grade": grade(a, b, m),
        "structure": structure(a, b, m),
        "texture": texture(a, b, m),
        "lighting": lighting(a, b, m),
    }

    def show(group: str, body: dict, safe: bool = True) -> None:
        print(f"\n  {group.upper()}" + ("" if safe else "   [UNSAFE — camera not pinned]"))
        for k, v in body.items():
            if not isinstance(v, dict):
                print(f"    {k:34s} {v}")
                continue
            ver = v.get("verdict", "")
            o, p = v.get("original"), v.get("port")
            extra = ""
            if isinstance(o, float) and isinstance(p, float):
                extra = f"orig {o:9.4f}   port {p:9.4f}"
                if "ratio" in v:
                    extra += f"   x{v['ratio']:.3f}"
                if "stops" in v:
                    extra += f"   {v['stops']:+.2f} stop"
            elif "value" in v:
                extra = f"{v['value']:.4f}"
            print(f"    {k:34s} {extra:48s} {ver}")

    print(f"\n=== parity metrics — region '{args.region}', {int(m.sum()):,} px scored ===")
    show("tone", report["tone"])
    show("grade", report["grade"])
    show("structure", report["structure"], safe=args.matched_camera)
    show("texture", report["texture"])
    show("lighting", report["lighting"])

    if not args.matched_camera:
        print("\n  NOTE: --matched-camera not set. TONE, GRADE, TEXTURE and LIGHTING")
        print("        remain valid (they are spatially blind or distributional).")
        print("        STRUCTURE is not — do not quote it.")

    if args.diff:
        write_diff(a, b, args.diff)
        print(f"\n  diff written: {args.diff}")
    if args.json:
        Path(args.json).write_text(json.dumps(report, indent=2))
        print(f"  json written: {args.json}")
    print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
