# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright"]
# ///
"""
frame_profile.py - what a frame actually costs, with vsync out of the way.

    uv run scripts/frame_profile.py <url> [--frames 400] [--warmup 120]
                                   [--cam "x y z yaw pitch fov"] [--label NAME]
                                   [--json out.json] [--vsync] [--headed]

WHY THIS EXISTS

`requestAnimationFrame` on a vsync-locked page reports 16.7 ms whether the frame
did 4 ms of work or 16. Measuring FPS that way cannot tell you whether you have
ten times the headroom you need or none at all, and it surfaces a regression only
once it has already cost a frame. Every "we're at 60" reading taken that way is a
ceiling report, not a cost report.

So this owns its own Chromium, launched with `--disable-gpu-vsync` and
`--disable-frame-rate-limit`. With the clock removed the frame interval IS the
work: 4 ms means 250 fps of headroom, 20 ms means 60 is already being missed.

WHAT IT SEPARATES, AND WHY THAT IS THE POINT

One frame-time number cannot tell you what to fix. Three are reported:

  interval     wall time between presents. The ceiling.
  main thread  the APP's own rAF callback duration - simulation, scene walk,
               draw submission, wasm. This is CPU.
  gap          interval minus main thread: time the CPU was NOT busy, waiting on
               the GPU or the compositor.

If main thread dominates, cutting GPU work buys nothing. If the gap dominates,
cutting CPU work buys nothing. Getting that backwards is the most expensive
mistake available here, and a single number cannot distinguish the two.

HOW THE CPU NUMBER IS TAKEN, AND THE TRAP IT AVOIDS

`window.requestAnimationFrame` is monkey-patched so the APP's callback is wrapped
and timed. The obvious alternative - timing from the profiler's own callback -
measures nearly the whole frame and reports a CPU cost identical to the interval
with a zero gap. That is wrong and it looks entirely plausible: this file was
written that way first and confidently reported "CPU-BOUND, gap 0.00 ms" from a
measurement that could not have produced any other answer.

HONEST LIMITS

  * `longtask` only fires above 50 ms, so on a healthy frame it reports nothing.
    It catches hitches; the wrapped callback is the per-frame signal.
  * Uncapped rendering runs hot and can thermally throttle. Compare runs from one
    session, and prefer medians.
  * MAIN THREAD only. Worker bakes and GPU-internal cost land in the gap.
  * Numbers from another machine, browser build or power state are not
    comparable. `env` records what it can so a stale comparison is detectable.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright

# The full Chromium build, not the headless shell: the shell has no real GPU path
# and cannot draw skinned geometry, which would make every number a lie.
CHANNEL = "chromium"

BASE_ARGS = [
    "--enable-unsafe-webgpu",
    "--enable-features=Vulkan",
    "--use-gl=angle",
    "--ignore-gpu-blocklist",
    "--force-color-profile=srgb",
    "--hide-scrollbars",
    "--mute-audio",
]

# Both are required. With either missing the interval is the display's clock.
UNCAPPED_ARGS = ["--disable-gpu-vsync", "--disable-frame-rate-limit"]

PROBE = r"""
(async ({ frames, warmup }) => {
  const longtasks = [];
  try {
    new PerformanceObserver(l => {
      for (const e of l.getEntries()) longtasks.push(e.duration);
    }).observe({ entryTypes: ['longtask'] });
  } catch (_) { /* not every build exposes it */ }

  // Wrap the APP's own callback. It re-registers every frame, so the next
  // registration goes through this wrapper and we time the real work.
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
      iv.push(t0 - last);
      last = t0;
      if (++n < frames) nativeRaf(tick); else resolve();
    };
    nativeRaf(tick);
  });
  window.requestAnimationFrame = nativeRaf;

  const cut = a => a.slice(warmup).sort((x, y) => x - y);
  const gl = document.createElement('canvas').getContext('webgl2');
  const dbg = gl && gl.getExtension('WEBGL_debug_renderer_info');

  return {
    interval: cut(iv),
    cpu: cut(appCpu),
    longtasks: longtasks,
    ua: navigator.userAgent,
    renderer: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : null,
    webgpu: !!navigator.gpu,
  };
})
"""


def stats(xs):
    if not xs:
        return {}
    xs = sorted(xs)

    def q(p):
        return xs[min(len(xs) - 1, int(len(xs) * p))]

    return {
        "n": len(xs),
        "median": round(statistics.median(xs), 3),
        "mean": round(statistics.fmean(xs), 3),
        "p95": round(q(0.95), 3),
        "p99": round(q(0.99), 3),
        "worst": round(xs[-1], 3),
        "best": round(xs[0], 3),
    }


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("url")
    ap.add_argument("--frames", type=int, default=400)
    ap.add_argument("--warmup", type=int, default=120)
    ap.add_argument("--settle", type=int, default=14000)
    ap.add_argument("--cam")
    ap.add_argument("--label", default="")
    ap.add_argument("--json")
    ap.add_argument("--vsync", action="store_true")
    ap.add_argument("--headed", action="store_true")
    ap.add_argument("--width", type=int, default=1280)
    ap.add_argument("--height", type=int, default=720)
    a = ap.parse_args()

    args = BASE_ARGS + ([] if a.vsync else UNCAPPED_ARGS)

    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=not a.headed, args=args, channel=CHANNEL)
        page = browser.new_page(viewport={"width": a.width, "height": a.height})
        page.goto(a.url, wait_until="load", timeout=60000)
        page.wait_for_timeout(a.settle)

        if a.cam:
            try:
                page.evaluate("window.__ax_console('cam " + a.cam + "')")
                page.wait_for_timeout(1500)
            except Exception as exc:  # noqa: BLE001
                print("  camera pin FAILED (" + str(exc) + ") - view is unpinned",
                      file=sys.stderr)

        raw = page.evaluate(PROBE, {"frames": a.frames, "warmup": a.warmup})
        browser.close()

    iv, cpu, longtasks = raw["interval"], raw["cpu"], raw["longtasks"]
    if not iv:
        print("REFUSING TO REPORT: no frames captured. The page probably never "
              "started rendering.", file=sys.stderr)
        return 2

    s_iv, s_cpu = stats(iv), stats(cpu)
    med_iv = s_iv["median"]
    med_cpu = s_cpu.get("median", 0.0)
    gap = max(0.0, med_iv - med_cpu)

    report = {
        "label": a.label or a.url,
        "url": a.url,
        "vsync": a.vsync,
        "viewport": [a.width, a.height],
        "interval_ms": s_iv,
        "main_thread_ms": s_cpu,
        "gap_ms": round(gap, 3),
        "fps": {
            "median": round(1000 / med_iv, 1) if med_iv else None,
            "p95": round(1000 / s_iv["p95"], 1) if s_iv["p95"] else None,
            "worst": round(1000 / s_iv["worst"], 1) if s_iv["worst"] else None,
        },
        "frames_over_16_67ms_pct": round(
            100.0 * sum(1 for x in iv if x > 16.667) / len(iv), 1),
        "longtasks_over_50ms": len(longtasks),
        "env": {"ua": raw["ua"], "renderer": raw["renderer"], "webgpu": raw["webgpu"]},
    }

    tag = ("VSYNC ON - display ceiling, not frame cost" if a.vsync
           else "vsync OFF - the interval IS the work")
    print("\n=== " + report["label"] + "  [" + tag + "] ===\n")
    print("  frame interval   median %7.2f ms   p95 %7.2f   p99 %7.2f   worst %7.2f"
          % (s_iv["median"], s_iv["p95"], s_iv["p99"], s_iv["worst"]))
    print("  main thread      median %7.2f ms   p95 %7.2f   worst %7.2f"
          % (med_cpu, s_cpu.get("p95", 0.0), s_cpu.get("worst", 0.0)))
    print("  gap (GPU/wait)   median %7.2f ms" % gap)
    print()
    print("  FPS              median %s   p95 %s   worst %s"
          % (report["fps"]["median"], report["fps"]["p95"], report["fps"]["worst"]))
    print("  frames over 16.67 ms  %s%%" % report["frames_over_16_67ms_pct"])
    print("  long tasks (>50 ms)   %d" % len(longtasks))

    # If the app captured its rAF reference before we patched it, the wrapper
    # never fires and the CPU column is empty. That is NOT "the main thread is
    # idle" - it is "not measured", and the two must never be confused.
    wrapper_fired = med_cpu > 0.01
    if not wrapper_fired:
        print("")
        print("  MAIN-THREAD NOT MEASURED: the rAF wrapper never fired, so the")
        print("  app holds a reference taken before injection. The gap below is")
        print("  everything, not GPU time. Do NOT read a CPU/GPU verdict from it.")
        report["main_thread_measured"] = False

    if not a.vsync and med_iv and wrapper_fired:
        share = med_cpu / med_iv
        if share > 0.7:
            verdict = "CPU-BOUND - the app's callback is most of the frame."
        elif share < 0.3:
            verdict = "GPU/PRESENT-BOUND - the main thread is idle most of the frame."
        else:
            verdict = "MIXED - neither side dominates; re-measure after any change."
        print("\n  " + verdict)
        head = 16.667 / med_iv
        print("  headroom to 60 fps: %.2fx (%s the 60 fps floor)"
              % (head, "meeting" if head >= 1 else "MISSING"))

    if not a.vsync and med_iv and not wrapper_fired:
        head = 16.667 / med_iv
        print("  headroom to 60 fps: %.2fx (%s the 60 fps floor)"
              % (head, "meeting" if head >= 1 else "MISSING"))

    if a.json:
        Path(a.json).write_text(json.dumps(report, indent=2))
        print("\n  json: " + a.json)
    print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
