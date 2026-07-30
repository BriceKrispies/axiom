#!/usr/bin/env -S uv run --with playwright python
# /// script
# requires-python = ">=3.10"
# dependencies = ["playwright>=1.48"]
# ///
"""
agent_play.py — an agent that PLAYS the arcade and measures whether it holds 60fps.

The sibling of `agent_capture.py`: same lineage, same control surface (this app is
pure-TS and invisible to the Rust `axiom-agent`, so an agent drives it through the
`window.__casino` handle the shell publishes), but where the capture agent freezes
a frame to look at it, this one plays rounds continuously and reports the frame
cost it actually got.

The target is a SMOOTH 60fps — every frame inside 16.67ms — so a single average is
the wrong readout: an arcade round is not one workload but several (an idle board,
a chest flying to hero framing, a lid opening, a celebration). Averaging them hides
which one misses. So every frame is tagged with the game PHASE it was drawn in and
the report is bucketed by phase, with the share of frames that met the budget. What
comes out names the moment to optimize, not just a number.

    # play 20 rounds on the software rasterizer and report per-phase frame cost
    uv run apps/casino-games/web/browser/agent_play.py --backend canvas2d --rounds 20

    # profile the CPU during the phase that misses the budget worst
    uv run apps/casino-games/web/browser/agent_play.py --backend canvas2d \
        --rounds 12 --profile-phase revealing

Prereq: serve the app first —
    uv run scripts/localhost_servers.py start-app casino-games --port 8085
"""

from __future__ import annotations

import argparse
import json
import statistics
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

TARGET_FPS = 60.0
BUDGET_MS = 1000.0 / TARGET_FPS
# A frame delta above this means the display went a whole refresh with nothing new:
# a visibly dropped frame. 1.5 vsync intervals, so ordinary jitter around 16.67ms
# is not counted as a stutter.
DROP_MS = BUDGET_MS * 1.5

# In-page probe: record (frame delta, phase) per rendered frame. The phase read is
# the app's own HUD projection -- the same value a player sees driving the chrome --
# so a frame is attributed to the moment it actually drew.
PROBE = """
(() => {
  const frames = [];
  let last = performance.now();
  const tick = () => {
    const now = performance.now();
    const hud = window.__casino ? window.__casino.hud() : null;
    frames.push([now - last, hud ? hud.phase : "none", hud ? hud.round : -1]);
    last = now;
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  window.__play = {
    reset: () => { frames.length = 0; },
    drain: () => frames.splice(0, frames.length),
    phase: () => (window.__casino && window.__casino.hud() ? window.__casino.hud().phase : "none"),
    round: () => (window.__casino && window.__casino.hud() ? window.__casino.hud().round : -1),
  };
})();
"""


def wait_phase(page, wanted: set[str], timeout_s: float = 30.0) -> str:
    """Block until the game reaches one of `wanted` (or give up and report where it is)."""
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        phase = page.evaluate("window.__play.phase()")
        if phase in wanted:
            return phase
        time.sleep(0.05)
    return page.evaluate("window.__play.phase()")


def summarize(samples: list[tuple[float, str, int]]) -> dict:
    """Bucket frame deltas by phase; report cost and how much of it met the budget."""
    by_phase: dict[str, list[float]] = defaultdict(list)
    for delta, phase, _round in samples:
        by_phase[phase].append(delta)

    def stats(deltas: list[float]) -> dict:
        ordered = sorted(deltas)
        n = len(ordered)
        # A vsynced 60fps display hands out frame deltas that jitter either side of
        # 16.67ms, so "delta <= 16.67" scores a PERFECT app at about 50% and says
        # nothing useful near the target. What a player actually perceives is a
        # DROPPED frame: the display had a chance to show something new and didn't,
        # which means a delta of about two vsync intervals or more.
        dropped = sum(1 for d in ordered if d > DROP_MS)
        return {
            "frames": n,
            "fps": round(1000.0 * n / max(1e-9, sum(ordered)), 1),
            "meanMs": round(statistics.fmean(ordered), 2),
            "medianMs": round(ordered[n // 2], 2),
            "p95Ms": round(ordered[min(n - 1, int(0.95 * n))], 2),
            "worstMs": round(ordered[-1], 2),
            "dropped": dropped,
            "smooth": round(100.0 * (n - dropped) / n, 1),
        }

    return {
        "overall": stats([d for d, _p, _r in samples]),
        "phases": {phase: stats(deltas) for phase, deltas in sorted(by_phase.items()) if deltas},
    }


def cpu_profile(cdp, seconds: float, top: int = 20) -> list[dict]:
    cdp.send("Profiler.start")
    time.sleep(seconds)
    result = cdp.send("Profiler.stop")["profile"]
    nodes = {n["id"]: n for n in result["nodes"]}
    samples = result.get("samples") or []
    deltas = result.get("timeDeltas") or []
    self_us: dict[int, float] = defaultdict(float)
    for i, node_id in enumerate(samples):
        self_us[node_id] += deltas[i] if i < len(deltas) else 0.0
    total = sum(self_us.values()) or 1.0
    rows = []
    for node_id, micros in self_us.items():
        frame = nodes[node_id]["callFrame"]
        url = (frame.get("url") or "").rsplit("/", 1)[-1]
        rows.append(
            {
                "fn": f"{frame.get('functionName') or '(anonymous)'} @ {url}:{frame.get('lineNumber', -1) + 1}",
                "ms": round(micros / 1000.0, 1),
                "pct": round(100.0 * micros / total, 2),
            }
        )
    rows.sort(key=lambda row: -row["ms"])
    return rows[:top]


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://localhost:8085/")
    ap.add_argument("--game", default="treasure-chest-pick")
    ap.add_argument("--backend", default="canvas2d", choices=["canvas2d", "webgl2", "css", "auto"])
    ap.add_argument("--rounds", type=int, default=20)
    ap.add_argument("--seed", type=int, default=None)
    ap.add_argument("--warmup", type=float, default=6.0, help="seconds ignored before measuring")
    ap.add_argument("--profile-phase", default="", help="take a CPU profile while in this phase")
    ap.add_argument("--profile-seconds", type=float, default=6.0)
    ap.add_argument("--headed", action="store_true")
    ap.add_argument("--out", type=Path, default=None)
    args = ap.parse_args(argv)

    boot = f"{args.url.rstrip('/')}/?game={args.game}&backend={args.backend}"
    if args.seed is not None:
        boot += f"&seed={args.seed}"

    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=not args.headed, args=BROWSER_ARGS)
        page = browser.new_context(viewport={"width": 1280, "height": 900}).new_page()
        errors: list[str] = []
        page.on("pageerror", lambda exc: errors.append(str(exc)))
        page.goto(boot, wait_until="load")
        page.evaluate(PROBE)
        cdp = page.context.new_cdp_session(page)
        cdp.send("Profiler.enable")
        cdp.send("Profiler.setSamplingInterval", {"interval": 200})

        print(f"[agent] {boot}")
        print(f"[agent] warming up {args.warmup:.0f}s, then playing {args.rounds} rounds", flush=True)
        time.sleep(args.warmup)
        page.evaluate("window.__play.reset()")

        samples: list[tuple[float, str, int]] = []
        profile_rows: list[dict] = []

        for index in range(args.rounds):
            wait_phase(page, {"ready"})
            # Pick a chest the way a player does: the primary action opens the
            # focused one. (Pointer picking is the same code path in the fold.)
            page.evaluate("window.__casino.press('Synthetic:Primary')")

            if args.profile_phase and not profile_rows:
                wait_phase(page, {args.profile_phase}, timeout_s=15.0)
                if page.evaluate("window.__play.phase()") == args.profile_phase:
                    print(f"[agent] profiling during phase={args.profile_phase}", flush=True)
                    profile_rows = cpu_profile(cdp, args.profile_seconds)

            wait_phase(page, {"complete"}, timeout_s=40.0)
            samples.extend(tuple(row) for row in page.evaluate("window.__play.drain()"))
            print(f"[agent] round {index + 1}/{args.rounds} done ({len(samples)} frames)", flush=True)
            page.evaluate("window.__casino.press('Synthetic:NewRound')")

        browser.close()

    if not samples:
        print("[agent] no frames captured", file=sys.stderr)
        return 1

    report = summarize(samples)
    overall = report["overall"]
    print(f"\n=== {args.game} on {args.backend} — target {TARGET_FPS:.0f}fps ({BUDGET_MS:.2f}ms/frame) ===")
    print(f"  {'phase':<14} {'frames':>7} {'fps':>7} {'mean':>8} {'median':>8} {'p95':>8} {'worst':>9} {'dropped':>8} {'smooth':>8}")
    for phase, got in report["phases"].items():
        print(
            f"  {phase:<14} {got['frames']:>7} {got['fps']:>7.1f} {got['meanMs']:>7.2f}m "
            f"{got['medianMs']:>7.2f}m {got['p95Ms']:>7.2f}m {got['worstMs']:>8.2f}m {got['dropped']:>8} {got['smooth']:>7.1f}%"
        )
    print(
        f"  {'OVERALL':<14} {overall['frames']:>7} {overall['fps']:>7.1f} {overall['meanMs']:>7.2f}m "
        f"{overall['medianMs']:>7.2f}m {overall['p95Ms']:>7.2f}m {overall['worstMs']:>8.2f}m {overall['dropped']:>8} {overall['smooth']:>7.1f}%"
    )

    missing = [(p, g) for p, g in report["phases"].items() if g["fps"] < 58.0 or g["smooth"] < 99.0]
    print("\n  verdict:", "HOLDS A SMOOTH 60fps" if not missing else "FALLS SHORT in: " + ", ".join(
        f"{p} ({g['fps']:.0f}fps, {g['dropped']} dropped)" for p, g in sorted(missing, key=lambda kv: kv[1]["fps"])
    ))

    if profile_rows:
        print(f"\n=== CPU self time during phase={args.profile_phase} ===")
        for row in profile_rows:
            print(f"  {row['pct']:>6.2f}%  {row['ms']:>8.1f}ms  {row['fn']}")

    if args.out:
        args.out.write_text(
            json.dumps({"url": boot, "report": report, "profile": profile_rows, "errors": errors[:20]}, indent=1),
            encoding="utf-8",
        )
        print(f"\n[agent] wrote {args.out}")
    if errors:
        print("[agent] page errors:", errors[:5])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
