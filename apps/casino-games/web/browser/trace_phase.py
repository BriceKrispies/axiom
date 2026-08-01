#!/usr/bin/env -S uv run --with playwright python
# /// script
# requires-python = ">=3.10"
# dependencies = ["playwright>=1.48"]
# ///
"""
trace_phase.py — where the browser spends a frame, per GAME PHASE.

The JS sampling profiler cannot answer this one. On the DOM backend `renderScene()`
measures 0.095ms and ~73% of CPU lands in `(program)` — the engine has already
finished by the time the expensive part starts, because the expensive part is the
browser's own pipeline: recalculating style, updating the layer tree, painting and
compositing a `preserve-3d` scene made of hundreds of elements. None of that is a
JS stack, so none of it shows up in a CPU profile.

So this records a real Chrome timeline trace (the same data the DevTools
Performance panel draws) and aggregates it by event, which names the actual stage:
`UpdateLayerTree` dominating means the compositor is re-sorting the 3D tree,
`RecalcStyles` means the style writes, `Paint`/`Rasterize` means fill.

It traces TWO windows and diffs them, because an absolute number is hard to read
and the interesting question is comparative: the board is idle and fast, the reveal
moves the camera and is slow — what is different?

    # what changes between the idle board and the reveal
    uv run apps/casino-games/web/browser/trace_phase.py --backend css

    # any two phases
    uv run apps/casino-games/web/browser/trace_phase.py --backend css \
        --baseline ready --subject celebrating
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from collections import defaultdict
from pathlib import Path

from playwright.sync_api import sync_playwright

BROWSER_ARGS = [
    "--enable-unsafe-webgpu",
    "--enable-features=Vulkan",
    "--use-gl=angle",
    "--autoplay-policy=no-user-gesture-required",
]

# The timeline categories DevTools itself records for the Performance panel.
TRACE_CATEGORIES = ",".join(
    [
        "disabled-by-default-devtools.timeline",
        "disabled-by-default-devtools.timeline.frame",
        "devtools.timeline",
        "blink.user_timing",
    ]
)

# Renderer-pipeline events worth naming, in pipeline order.
INTERESTING = [
    "ParseHTML",
    "EvaluateScript",
    "FunctionCall",
    "TimerFire",
    "UpdateLayoutTree",  # style recalculation
    "RecalcStyles",
    "Layout",
    "UpdateLayerTree",
    "Paint",
    "PaintImage",
    "Rasterize",
    "CompositeLayers",
    "Commit",
    "DrawFrame",
    "MajorGC",
    "MinorGC",
]

PROBE = """
(() => {
  window.__phase = () => (window.__casino && window.__casino.hud() ? window.__casino.hud().phase : "none");
  let frames = 0;
  const tick = () => { frames += 1; requestAnimationFrame(tick); };
  requestAnimationFrame(tick);
  window.__frames = { read: () => frames };
})();
"""


def wait_phase(page, wanted: set[str], timeout_s: float = 30.0) -> str:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        phase = page.evaluate("window.__phase()")
        if phase in wanted:
            return phase
        time.sleep(0.03)
    return page.evaluate("window.__phase()")


def trace_window(page, cdp, seconds: float) -> tuple[dict[str, float], int, float]:
    """Record a timeline trace for `seconds`; return (ms by event, frames, wall ms)."""
    events: list[dict] = []
    cdp.on("Tracing.dataCollected", lambda ev: events.extend(ev.get("value") or []))
    done: list[bool] = []
    cdp.on("Tracing.tracingComplete", lambda _ev: done.append(True))

    before = page.evaluate("window.__frames.read()")
    started = time.time()
    cdp.send("Tracing.start", {"categories": TRACE_CATEGORIES, "transferMode": "ReportEvents"})
    time.sleep(seconds)
    cdp.send("Tracing.end")
    wall_ms = (time.time() - started) * 1000.0
    frames = page.evaluate("window.__frames.read()") - before

    deadline = time.time() + 20.0
    while not done and time.time() < deadline:
        page.wait_for_timeout(100)

    totals: dict[str, float] = defaultdict(float)
    for ev in events:
        # Complete events carry their own duration, in microseconds.
        if ev.get("ph") != "X":
            continue
        name = ev.get("name") or ""
        totals[name] += float(ev.get("dur") or 0) / 1000.0
    return totals, frames, wall_ms


def trace_round(page, cdp, seconds: float) -> list[dict]:
    """Trace across a whole round — the press, the flight, the lid, the celebration —
    and keep every individual event, so the SPIKES survive instead of being averaged
    away into a per-frame mean."""
    events: list[dict] = []
    cdp.on("Tracing.dataCollected", lambda ev: events.extend(ev.get("value") or []))
    done: list[bool] = []
    cdp.on("Tracing.tracingComplete", lambda _ev: done.append(True))
    cdp.send("Tracing.start", {"categories": TRACE_CATEGORIES, "transferMode": "ReportEvents"})
    page.evaluate("window.__casino.press('Synthetic:Primary')")
    time.sleep(seconds)
    cdp.send("Tracing.end")
    deadline = time.time() + 20.0
    while not done and time.time() < deadline:
        page.wait_for_timeout(100)
    return [ev for ev in events if ev.get("ph") == "X"]


def report_worst(events: list[dict], top: int = 18) -> int:
    """The longest single events in the round — the hitch, named."""
    tasks = [ev for ev in events if ev.get("name") == "RunTask"]
    tasks.sort(key=lambda ev: -float(ev.get("dur") or 0))
    print("\n=== longest main-thread TASKS in the round (a hitch is one long task) ===")
    for ev in tasks[:8]:
        print(f"    {float(ev['dur']) / 1000.0:>8.2f} ms  RunTask")

    ranked = sorted(events, key=lambda ev: -float(ev.get("dur") or 0))
    print("\n=== longest individual events (excluding the enclosing RunTask) ===")
    shown = 0
    for ev in ranked:
        if ev.get("name") == "RunTask":
            continue
        ms = float(ev.get("dur") or 0) / 1000.0
        if ms < 3.0 or shown >= top:
            break
        detail = ev.get("args", {}).get("data", {}) or {}
        note = detail.get("styleRecalcCount") or detail.get("nodeCount") or ""
        print(f"    {ms:>8.2f} ms  {ev.get('name'):<24} {note}")
        shown += 1

    by_name: dict[str, float] = defaultdict(float)
    for ev in events:
        by_name[ev.get("name") or ""] += float(ev.get("dur") or 0) / 1000.0
    print("\n=== total time in the round, by event ===")
    for name, ms in sorted(by_name.items(), key=lambda kv: -kv[1])[:14]:
        print(f"    {ms:>9.1f} ms  {name}")
    return 0


def report(label: str, totals: dict[str, float], frames: int, wall_ms: float) -> dict:
    per_frame = {name: ms / max(1, frames) for name, ms in totals.items()}
    fps = 1000.0 * frames / max(1e-6, wall_ms)
    print(f"\n[{label}] {frames} frames in {wall_ms:.0f}ms  ->  {fps:.1f} fps")
    rows = [(name, per_frame.get(name, 0.0)) for name in INTERESTING if per_frame.get(name, 0.0) > 0.01]
    rows.sort(key=lambda row: -row[1])
    for name, ms in rows:
        print(f"    {name:<20} {ms:>8.3f} ms/frame")
    return {"fps": fps, "frames": frames, "perFrame": per_frame}


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://localhost:8085/")
    ap.add_argument("--game", default="treasure-chest-pick")
    ap.add_argument("--backend", default="css")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--baseline", default="ready", help="the phase that is FAST")
    ap.add_argument("--subject", default="revealing", help="the phase that is SLOW")
    ap.add_argument("--seconds", type=float, default=3.0)
    ap.add_argument(
        "--round",
        action="store_true",
        help="trace one WHOLE round across the phase changes and report the longest "
        "individual events — averages hide a hitch, and a hitch is what a player sees",
    )
    ap.add_argument("--out", type=Path, default=None)
    args = ap.parse_args(argv)

    boot = f"{args.url.rstrip('/')}/?game={args.game}&backend={args.backend}&seed={args.seed}"

    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True, args=BROWSER_ARGS)
        page = browser.new_context(viewport={"width": 1280, "height": 900}).new_page()
        page.goto(boot, wait_until="load")
        page.evaluate(PROBE)
        time.sleep(5.0)
        cdp = page.context.new_cdp_session(page)

        if args.round:
            wait_phase(page, {"ready"})
            worst = trace_round(page, cdp, args.seconds)
            browser.close()
            return report_worst(worst)

        wait_phase(page, {args.baseline})
        base_totals, base_frames, base_wall = trace_window(page, cdp, args.seconds)

        # Drive into the subject phase, then trace it. Re-press until it arrives so
        # a missed edge cannot silently trace the wrong phase.
        for _ in range(6):
            if page.evaluate("window.__phase()") == args.subject:
                break
            page.evaluate("window.__casino.press('Synthetic:Primary')")
            wait_phase(page, {args.subject}, timeout_s=6.0)
        reached = page.evaluate("window.__phase()")
        subj_totals, subj_frames, subj_wall = trace_window(page, cdp, args.seconds)
        browser.close()

    base = report(f"{args.baseline}", base_totals, base_frames, base_wall)
    subj = report(f"{reached}", subj_totals, subj_frames, subj_wall)
    if reached != args.subject:
        print(f"\n  ! wanted phase '{args.subject}' but traced '{reached}'")

    print(f"\n=== per-frame change, {reached} vs {args.baseline} ===")
    names = set(base["perFrame"]) | set(subj["perFrame"])
    rows = []
    for name in names:
        before = base["perFrame"].get(name, 0.0)
        after = subj["perFrame"].get(name, 0.0)
        rows.append((after - before, name, before, after))
    rows.sort(key=lambda row: -abs(row[0]))
    for delta, name, before, after in rows[:14]:
        if abs(delta) < 0.02:
            continue
        print(f"    {delta:>+9.3f} ms/frame  {name:<22} {before:>7.3f} -> {after:>7.3f}")

    if args.out:
        args.out.write_text(json.dumps({"baseline": base, "subject": subj}, indent=1), encoding="utf-8")
        print(f"\n[trace] wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
