# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright"]
# ///
"""
frame_sweep.py - measure many camera lenses in ONE browser session.

    uv run scripts/frame_sweep.py <url> [--frames 200] [--warmup 60]
                                 [--settle 14000] [--json out.json]

WHY A SWEEP RATHER THAN A SINGLE READING

A single frame time says how slow something is. It does not say WHAT is slow.
Varying the lens does, because each knob changes exactly one input to the cost:

  FOV        widens the frustum -> more objects survive culling -> more draws.
             If cost is flat across 20 deg and 110 deg, draw count is NOT the
             bottleneck and the work is per-frame overhead that runs regardless.
  aim        pointing at open sky removes nearly all geometry from the frame.
             If a sky-facing frame still costs the same, the cost is not
             rendering-dependent at all - it is simulation, scene walk, or
             per-frame allocation that happens before anything is culled.
  position   inside a room vs the open street changes both draw count and
             overdraw.
  quality    the app's own preset axis: render scale, shadow map size, particle
             budgets.

The sky test is the sharpest single discriminator and it is cheap, so it runs
first. A flat sweep is not a failed experiment - it is the answer, and it rules
out the entire class of fixes that a slow frame usually invites.

ONE BROWSER, MANY CONFIGS

Relaunching Chromium per config costs more than the measurement and adds
cold-start variance. This loads once, then drives `__ax_console('cam ...')`
between runs. Vsync is off throughout, so each interval is the work.

The port's camera console takes RADIANS, yaw then pitch, and `cam off` releases
the pin.
"""

from __future__ import annotations

import argparse
import json
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

# name, x, y, z, yaw, pitch, fov, note
CONFIGS = [
    # The discriminator: almost nothing in frame.
    ("sky",        12, 1.75, 18, 0.588,  1.30, 75, "straight up - near-zero geometry"),
    ("sky-narrow", 12, 1.75, 18, 0.588,  1.30, 20, "up + narrow - the least possible work"),
    # The reference pose the parity campaign scores.
    ("hero",       12, 1.75, 18, 0.588,  0.0156, 75, "down the main street"),
    # FOV sweep at the hero position: isolates frustum width.
    ("fov-20",     12, 1.75, 18, 0.588,  0.0156, 20, ""),
    ("fov-40",     12, 1.75, 18, 0.588,  0.0156, 40, ""),
    ("fov-60",     12, 1.75, 18, 0.588,  0.0156, 60, ""),
    ("fov-90",     12, 1.75, 18, 0.588,  0.0156, 90, ""),
    ("fov-110",    12, 1.75, 18, 0.588,  0.0156, 110, ""),
    # Aim variations at the same spot.
    ("ground",     12, 1.75, 18, 0.588, -1.30, 75, "straight down at the road"),
    ("reverse",    12, 1.75, 18, 3.730,  0.0156, 75, "180 deg - the other end of the street"),
]

PROBE = r"""
(async ({ frames, warmup }) => {
  const appCpu = [];
  const nativeRaf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = function (cb) {
    return nativeRaf(function (ts) {
      const t0 = performance.now();
      try { return cb(ts); } finally { appCpu.push(performance.now() - t0); }
    });
  };
  const iv = [];
  let last = performance.now();
  await new Promise(resolve => {
    let n = 0;
    const tick = () => {
      const t0 = performance.now();
      iv.push(t0 - last); last = t0;
      if (++n < frames) nativeRaf(tick); else resolve();
    };
    nativeRaf(tick);
  });
  window.requestAnimationFrame = nativeRaf;
  const cut = a => a.slice(warmup).sort((x, y) => x - y);
  return { interval: cut(iv), cpu: cut(appCpu) };
})
"""


def med(xs):
    return round(statistics.median(xs), 2) if xs else 0.0


def p95(xs):
    return round(sorted(xs)[min(len(xs) - 1, int(len(xs) * 0.95))], 2) if xs else 0.0


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("url")
    ap.add_argument("--frames", type=int, default=200)
    ap.add_argument("--warmup", type=int, default=60)
    ap.add_argument("--settle", type=int, default=14000)
    ap.add_argument("--json")
    ap.add_argument("--width", type=int, default=1280)
    ap.add_argument("--height", type=int, default=720)
    a = ap.parse_args()

    rows = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True, args=ARGS, channel=CHANNEL)
        page = browser.new_page(viewport={"width": a.width, "height": a.height})
        page.goto(a.url, wait_until="load", timeout=60000)
        page.wait_for_timeout(a.settle)

        probe_ok = page.evaluate("typeof window.__ax_console === 'function'")
        if not probe_ok:
            print("REFUSING: window.__ax_console is absent. This sweep needs the "
                  "port's camera pin; the JS original has no equivalent.")
            browser.close()
            return 2

        for (name, x, y, z, yaw, pitch, fov, note) in CONFIGS:
            page.evaluate(
                "window.__ax_console('cam %s %s %s %s %s %s')"
                % (x, y, z, yaw, pitch, fov))
            page.wait_for_timeout(900)
            r = page.evaluate(PROBE, {"frames": a.frames, "warmup": a.warmup})
            iv, cpu = r["interval"], r["cpu"]
            if not iv:
                continue
            rows.append({
                "config": name, "fov": fov, "note": note,
                "interval_ms": med(iv), "interval_p95": p95(iv),
                "cpu_ms": med(cpu), "cpu_p95": p95(cpu),
                "gap_ms": round(max(0.0, med(iv) - med(cpu)), 2),
                "fps": round(1000 / med(iv), 1) if med(iv) else None,
            })
            last = rows[-1]
            print("  %-11s fov %3d   frame %6.2f ms   cpu %6.2f   gap %5.2f   %5.1f fps   %s"
                  % (name, fov, last["interval_ms"], last["cpu_ms"],
                     last["gap_ms"], last["fps"], note))

        stats = page.evaluate("window.__ax_console('stats')")
        browser.close()

    if not rows:
        print("no rows measured")
        return 2

    ivs = [r["interval_ms"] for r in rows]
    cpus = [r["cpu_ms"] for r in rows]
    spread = (max(ivs) / min(ivs)) if min(ivs) else 0
    cpu_spread = (max(cpus) / min(cpus)) if min(cpus) else 0

    print("\n  frame time  min %.2f  max %.2f  spread %.2fx" % (min(ivs), max(ivs), spread))
    print("  cpu time    min %.2f  max %.2f  spread %.2fx" % (min(cpus), max(cpus), cpu_spread))
    if cpu_spread < 1.25:
        print("\n  VERDICT: CPU cost is FLAT across every lens. The work does not")
        print("  depend on what is in frame, so it is not draw submission, not")
        print("  culling and not overdraw. Look for per-frame work that runs")
        print("  unconditionally: simulation, a full scene walk, or per-frame")
        print("  allocation. Reducing geometry or FOV will buy nothing.")
    elif cpu_spread > 2.0:
        print("\n  VERDICT: CPU cost tracks what is in frame. Draw submission or")
        print("  per-object work dominates; culling and batching are the lever.")
    else:
        print("\n  VERDICT: MIXED - part fixed overhead, part per-object.")

    print("\n  stats at the end of the sweep:")
    for line in str(stats).splitlines():
        print("    " + line)

    if a.json:
        Path(a.json).write_text(json.dumps({"url": a.url, "rows": rows}, indent=2))
        print("\n  json: " + a.json)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
