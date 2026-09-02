# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright>=1.48", "pillow>=10", "numpy>=1.26"]
# ///
"""**The capability bisect.** Render one pose under many capability words, and
score each frame against a reference the fault produced.

    uv run scripts/capability_sweep.py --ref shots/phone.jpg
    uv run scripts/capability_sweep.py --url http://localhost:8088/ --list

Why this exists
---------------
`?nocaps=`, `?device=` and `?debug=` already let a URL turn one thing off
(`live_gpu_binding::dropped_by_url`, `DeviceFacts::impersonating`,
`scene_renderer::debug_probe`). What was missing is the *sweep*: loading
seventeen of them by hand, eyeballing seventeen screenshots and holding the
differences in your head is exactly the job a person does badly.

The scoring is the point. A device fault reported as "it looks wrong" is
unfalsifiable, and two screenshots of a dark scene are indistinguishable by eye
when one is dark because the sun set and the other is dark because a term
collapsed to zero. So each frame is reduced to a **luminance signature** and
compared against the reference's, and the configs are ranked by distance.

The signature deliberately separates the sky from everything else. A collapsed
lighting term multiplies the *geometry* and leaves the sky alone (the sky pass
binds none of it), so "dark geometry under a correct sky" is the fingerprint,
and a frame that is uniformly dark — a real dusk, a wrong exposure — scores far
away from it rather than tying with it.

This is verification tooling in the shape the repo already uses for the browser
(`playwright_controller.py`, `parity_shot.py`, `frame_sweep.py`): outside the
engine dependency graph, not a layer, module, app or Cargo package.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image
from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parent.parent

# One token per load. A bisect wants a single variable moved at a time; a
# combination is only worth running once a single token has proved interesting.
NOCAPS = [
    "shadows", "normalmap", "specular", "sky", "bloom",
    "aerial", "textures", "hdr", "gbuffer", "sdf",
]
DEVICE = [
    "no-hdr", "no-rg16f", "no-r32f",
    "no-float-filter", "no-depth-filter", "no-mrt",
]


def configs(extra: list[str]) -> list[tuple[str, str]]:
    """`(label, query-fragment)` for every point in the sweep."""
    out = [("baseline", "")]
    out += [(f"nocaps={t}", f"&nocaps={t}") for t in NOCAPS]
    out += [(f"device={t}", f"&device={t}") for t in DEVICE]
    out += [(f"extra:{e}", f"&{e}") for e in extra]
    return out


def signature(path_or_img, hud_top: float, hud_bottom: float) -> dict:
    """Reduce a frame to the handful of numbers that separate the fault.

    The HUD is cropped away by fraction rather than by pixel because the sweep
    and the reference are different resolutions, and the HUD is the one part of
    the frame that is drawn by the DOM and therefore cannot show the fault.
    """
    img = path_or_img
    if not isinstance(img, Image.Image):
        img = Image.open(img)
    a = np.asarray(img.convert("RGB"), dtype=np.float32) / 255.0
    h = a.shape[0]
    lum = a @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    core = lum[int(h * hud_top):int(h * hud_bottom), :]
    p10, p50, p90 = (float(np.percentile(core, q)) for q in (10, 50, 90))
    return {
        "median": p50,
        "mean": float(core.mean()),
        "p10": p10,
        "p90": p90,
        "dark_frac": float((core < 0.10).mean()),
        "sky_frac": float((core > 0.55).mean()),
        "ratio": p90 / max(p50, 1e-6),
    }


def distance(sig: dict, ref: dict) -> float:
    """Distance in the three axes that actually name the fault.

    `dark_frac` and `median` say the geometry collapsed; `sky_frac` says the sky
    did not. All three matter — a frame that is dark everywhere, sky included,
    is a different fault and must not score as a match — so the sky term is
    weighted equally rather than folded in as a tiebreak.
    """
    return (
        abs(sig["dark_frac"] - ref["dark_frac"])
        + abs(sig["sky_frac"] - ref["sky_frac"])
        + abs(sig["median"] - ref["median"]) * 2.0
    )


def capture(page, url: str, cam: str, settle: int, out: Path) -> Image.Image:
    page.goto(url, wait_until="load", timeout=120_000)
    # `__ax_console` is installed before the GPU binds, so it is the readiness
    # signal the app itself documents. Poll rather than sleep a guessed amount.
    page.wait_for_function("typeof window.__ax_console === 'function'", timeout=120_000)
    page.wait_for_timeout(settle)
    # Pin AFTER settling: the first frame's rig overwrites a pose set too early.
    page.evaluate(f"window.__ax_console('cam {cam}')")
    page.wait_for_timeout(1200)
    page.screenshot(path=str(out))
    return Image.open(out)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://localhost:8088/")
    ap.add_argument("--ref", help="reference image the fault produced")
    ap.add_argument("--cam", default="9.26 1.681 13.19 -2.5527 -0.08 80")
    ap.add_argument("--backend", default="webgl2")
    ap.add_argument("--scale1", action="store_true", default=True)
    ap.add_argument("--no-scale1", dest="scale1", action="store_false")
    ap.add_argument("--width", type=int, default=411)
    ap.add_argument("--height", type=int, default=752)
    ap.add_argument("--settle", type=int, default=6000)
    ap.add_argument("--hud-top", type=float, default=0.12)
    ap.add_argument("--hud-bottom", type=float, default=0.78)
    ap.add_argument("--out", default="shots/capsweep")
    ap.add_argument("--extra", action="append", default=[],
                    help="an additional raw query fragment, e.g. 'nocaps=gbuffer,shadows'")
    ap.add_argument("--list", action="store_true", help="print the sweep and exit")
    args = ap.parse_args()

    plan = configs(args.extra)
    if args.list:
        for label, frag in plan:
            print(f"{label:28} {frag}")
        return 0

    out_dir = REPO / args.out
    out_dir.mkdir(parents=True, exist_ok=True)

    ref = None
    if args.ref:
        ref = signature(Path(args.ref), args.hud_top, args.hud_bottom)
        print("reference:", json.dumps({k: round(v, 4) for k, v in ref.items()}))
        print()

    base = args.url.rstrip("/") + "/?backend=" + args.backend
    base += "&scale=1" if args.scale1 else ""

    rows = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": args.width, "height": args.height})
        for label, frag in plan:
            safe = label.replace("=", "_").replace(",", "+").replace(":", "_")
            path = out_dir / f"{safe}.png"
            try:
                img = capture(page, base + frag, args.cam, args.settle, path)
            except Exception as exc:  # a config that will not boot is a RESULT
                print(f"{label:28} FAILED TO LOAD  {type(exc).__name__}: {exc}")
                rows.append((label, None, None, path))
                continue
            sig = signature(img, args.hud_top, args.hud_bottom)
            d = distance(sig, ref) if ref else None
            rows.append((label, sig, d, path))
            print(
                f"{label:28} median={sig['median']:.4f} dark={sig['dark_frac']:.3f} "
                f"sky={sig['sky_frac']:.3f} ratio={sig['ratio']:.2f}"
                + (f"  dist={d:.4f}" if d is not None else "")
            )
        browser.close()

    if ref:
        print("\n--- ranked by distance to the reference ---")
        scored = [r for r in rows if r[2] is not None]
        for label, sig, d, path in sorted(scored, key=lambda r: r[2])[:8]:
            print(f"  {d:.4f}  {label:28} {path.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
