# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright"]
# ///
"""
frame_motion.py - frame cost while the camera is MOVING.

    uv run scripts/frame_motion.py [--url ...] [--frames 300] [--settle 45000]

WHY A STATIC READING IS NOT ENOUGH

A parked camera is the friendliest frame a renderer ever gets. Anything cached
between frames stays valid: culling sets, temporal history, an adaptive
resolution controller that has settled and stopped moving. Motion invalidates all
three at once, and it is also the only state a player is ever in.

Worse for this app specifically: `RenderScaleController` reacts to measured frame
time. A camera that parks lets it converge and sit still. A camera that sweeps
into and out of expensive views can push it up and down the ladder mid-run - so
"60 fps" measured statically can be 45 in play, and the resolution can visibly
breathe while it hunts.

WHAT IT RUNS

  static        camera pinned, no updates. The control.
  static+call   pinned to the SAME pose but re-issuing the console call every
                frame. This isolates the cost of the measurement harness itself,
                so motion numbers are not inflated by the thing measuring them.
                Without this control every result below would be suspect.
  yaw-spin      a full 360 turn. Worst case for culling coherence and for any
                temporal history.
  walk          translation down the street, orientation fixed.
  walk+look     both at once - the closest thing here to real play.

Reported per pattern: median, p95, p99, worst, and the share of frames over
16.67 ms. The worst-case and p99 matter more than the median here: a hitch during
a turn is what a player feels, and a median hides it completely.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
from pathlib import Path

from playwright.sync_api import sync_playwright

CHANNEL = "chromium"
ARGS = [
    "--enable-unsafe-webgpu",
    "--enable-features=Vulkan",
    "--use-gl=angle",
    "--ignore-gpu-blocklist",
    "--hide-scrollbars",
    "--mute-audio",
    "--disable-gpu-vsync",
    "--disable-frame-rate-limit",
]

# Pattern -> a JS expression of (i, n) returning [x, y, z, yaw, pitch, fov].
# Anchored on the hero pose so every number is comparable to the parity captures.
PATTERNS = {
    "static": "() => [12, 1.75, 18, 0.588003, 0.0156, 75]",
    "static+call": "() => [12, 1.75, 18, 0.588003, 0.0156, 75]",
    "yaw-spin": "(i, n) => [12, 1.75, 18, 0.588003 + (i / n) * Math.PI * 2, 0.0156, 75]",
    "walk": "(i, n) => [12 - (i / n) * 22, 1.75, 18 - (i / n) * 33, 0.588003, 0.0156, 75]",
    "walk+look": ("(i, n) => [12 - (i / n) * 22, 1.75, 18 - (i / n) * 33, "
                  "0.588003 + Math.sin(i / n * Math.PI * 2) * 0.6, "
                  "Math.sin(i / n * Math.PI * 4) * 0.15, 75]"),
    # The trailing control. `RenderScaleController` keeps descending for a long
    # time after load, so patterns measured in sequence are NOT comparable unless
    # the first pattern is repeated last. If static-end differs from static, the
    # scaler moved underneath the run and every row between them is confounded.
    "static-end": "() => [12, 1.75, 18, 0.588003, 0.0156, 75]",
}

PROBE = r"""
(async ({ frames, path, drive }) => {
  const fn = eval(path);
  const iv = [];
  let last = performance.now();
  await new Promise(resolve => {
    let i = 0;
    const tick = () => {
      if (drive) {
        const p = fn(i, frames);
        window.__ax_console('cam ' + p[0].toFixed(4) + ' ' + p[1].toFixed(4) + ' ' +
          p[2].toFixed(4) + ' ' + p[3].toFixed(6) + ' ' + p[4].toFixed(6) + ' ' + p[5]);
      }
      const t = performance.now();
      iv.push(t - last);
      last = t;
      if (++i < frames) requestAnimationFrame(tick); else resolve();
    };
    requestAnimationFrame(tick);
  });
  return iv.slice(30);
})
"""


def summarise(xs):
    xs = sorted(xs)

    def q(p):
        return xs[min(len(xs) - 1, int(len(xs) * p))]

    return {
        "median": round(statistics.median(xs), 2),
        "p95": round(q(0.95), 2),
        "p99": round(q(0.99), 2),
        "worst": round(xs[-1], 2),
        "fps_median": round(1000 / statistics.median(xs), 1),
        "fps_p99": round(1000 / q(0.99), 1),
        "over_16_67_pct": round(100.0 * sum(1 for x in xs if x > 16.667) / len(xs), 1),
    }


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default="http://localhost:8088/")
    ap.add_argument("--frames", type=int, default=300)
    ap.add_argument("--settle", type=int, default=45000)
    ap.add_argument("--json")
    a = ap.parse_args()

    out = {}
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True, args=ARGS, channel=CHANNEL)
        page = browser.new_page(viewport={"width": 1280, "height": 720})
        page.goto(a.url, wait_until="load", timeout=60000)
        page.wait_for_timeout(a.settle)

        if not page.evaluate("typeof window.__ax_console === 'function'"):
            print("REFUSING: no __ax_console; this needs the port's camera pin.")
            browser.close()
            return 2

        print("\n  %-12s %8s %8s %8s %8s | %7s %7s | %s"
              % ("pattern", "med_ms", "p95", "p99", "worst", "fps_med", "fps_p99", ">16.67ms"))
        for name, path in PATTERNS.items():
            drive = not name.startswith("static") or name == "static+call"
            # Park at the start pose and let the scaler settle before each run.
            page.evaluate("window.__ax_console('cam 12 1.75 18 0.588003 0.015600 75')")
            page.wait_for_timeout(6000)
            iv = page.evaluate(PROBE,
                               {"frames": a.frames, "path": path, "drive": drive})
            if not iv:
                continue
            s = summarise(iv)
            out[name] = s
            print("  %-12s %8.2f %8.2f %8.2f %8.2f | %7.1f %7.1f | %s%%"
                  % (name, s["median"], s["p95"], s["p99"], s["worst"],
                     s["fps_median"], s["fps_p99"], s["over_16_67_pct"]))

        stats = str(page.evaluate("window.__ax_console('stats')"))
        browser.close()

    if "static" in out and "static+call" in out:
        overhead = out["static+call"]["median"] - out["static"]["median"]
        print("\n  harness overhead (the console call itself): %+.2f ms/frame" % overhead)
        if overhead > 1.0:
            print("  That is large enough to matter - subtract it from the motion rows.")

    if "static" in out and "walk+look" in out:
        ratio = out["walk+look"]["median"] / max(out["static"]["median"], 1e-9)
        print("  motion vs static (median): x%.2f" % ratio)
        worst = max((v["worst"] for v in out.values()), default=0)
        print("  worst frame across all patterns: %.2f ms (%.1f fps)"
              % (worst, 1000 / worst if worst else 0))
        if any(v["fps_median"] < 60 for v in out.values()):
            print("\n  NOT holding 60 under motion. The median is not the problem -")
            print("  look at p99 and worst; a hitch during a turn is what is felt.")

    for line in stats.replace("\\n", "\n").splitlines():
        if line.startswith(("frame", "pins")):
            print("    " + line)

    if a.json:
        Path(a.json).write_text(json.dumps(out, indent=2))
        print("\n  json: " + a.json)
    print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
