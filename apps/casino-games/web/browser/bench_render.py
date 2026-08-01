#!/usr/bin/env -S uv run --with playwright python
# /// script
# requires-python = ">=3.10"
# dependencies = ["playwright>=1.48"]
# ///
"""
bench_render.py — time the RENDER PATH alone, on one frozen scene.

End-to-end fps is the number that matters to a player, but it is a poor tool for
judging a rendering change: it mixes the game phase, the fixed-step catch-up, the
compositor and whatever else the machine is doing, and it moved +-20% run to run
here even with nothing changed. A change worth a few percent is invisible in it.

So this benchmark removes everything that is not the renderer. It boots the app on
`?shot=N`, which freezes the simulation at a fixed tick and pins the view clock, so
the retained scene stops changing. Then it reaches the engine module THROUGH THE
PAGE'S OWN IMPORT MAP -- the same module instance the app is running, so the store
singleton it renders is the real, fully-populated scene -- and calls `renderScene()`
in a tight loop, timing it.

Same scene, same store, no animation: the only variable left is the cost of drawing
it. Report is median-of-batches, which is robust to a stray scheduler hiccup in a
way a mean is not.

    uv run apps/casino-games/web/browser/bench_render.py --backend canvas2d
    uv run apps/casino-games/web/browser/bench_render.py --backend canvas2d --shot 260
"""

from __future__ import annotations

import argparse
import statistics
import sys
import time
from collections import defaultdict

from playwright.sync_api import sync_playwright

BROWSER_ARGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-gl=angle"]

BENCH = """
async ({ batches, perBatch }) => {
  const script = document.querySelector('script[type="importmap"]');
  const url = JSON.parse(script.textContent).imports["@axiom/web-engine"];
  const engine = await import(url);
  const timings = [];
  // One warm-up batch so the measured batches see already-optimized code.
  for (let b = 0; b < batches + 1; b += 1) {
    const t0 = performance.now();
    for (let i = 0; i < perBatch; i += 1) { engine.renderScene(); }
    timings.push((performance.now() - t0) / perBatch);
  }
  return { backend: engine.rendererBackendName(), nodes: engine.rendererNodeCount(), timings: timings.slice(1) };
}
"""


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://localhost:8085/")
    ap.add_argument("--game", default="treasure-chest-pick")
    ap.add_argument("--backend", default="canvas2d")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--shot", type=int, default=120, help="freeze tick (pins the scene)")
    ap.add_argument("--batches", type=int, default=9)
    ap.add_argument("--per-batch", type=int, default=30)
    ap.add_argument("--settle", type=float, default=6.0)
    ap.add_argument("--extra", default="", help="extra query string, e.g. press=Space@30 to freeze mid-reveal")
    ap.add_argument("--label", default="")
    ap.add_argument(
        "--profile",
        action="store_true",
        help="CPU-profile the render loop itself, so the breakdown has no game logic in it",
    )
    args = ap.parse_args(argv)

    boot = f"{args.url.rstrip('/')}/?game={args.game}&backend={args.backend}&seed={args.seed}&shot={args.shot}"
    boot += f"&{args.extra}" if args.extra else ""

    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True, args=BROWSER_ARGS)
        page = browser.new_context(viewport={"width": 1280, "height": 900}).new_page()
        errors: list[str] = []
        page.on("pageerror", lambda exc: errors.append(str(exc)))
        page.goto(boot, wait_until="load")
        time.sleep(args.settle)
        cdp = None
        if args.profile:
            cdp = page.context.new_cdp_session(page)
            cdp.send("Profiler.enable")
            cdp.send("Profiler.setSamplingInterval", {"interval": 100})
            cdp.send("Profiler.start")
        got = page.evaluate(BENCH, {"batches": args.batches, "perBatch": args.per_batch})
        rows: list[dict] = []
        if cdp is not None:
            result = cdp.send("Profiler.stop")["profile"]
            nodes = {n["id"]: n for n in result["nodes"]}
            self_us: dict[int, float] = defaultdict(float)
            deltas = result.get("timeDeltas") or []
            for i, node_id in enumerate(result.get("samples") or []):
                self_us[node_id] += deltas[i] if i < len(deltas) else 0.0
            total = sum(self_us.values()) or 1.0
            for node_id, micros in self_us.items():
                frame = nodes[node_id]["callFrame"]
                url = (frame.get("url") or "").rsplit("/", 1)[-1]
                rows.append(
                    {
                        "fn": f"{frame.get('functionName') or '(anonymous)'} @ {url}:{frame.get('lineNumber', -1) + 1}",
                        "pct": round(100.0 * micros / total, 2),
                        "ms": round(micros / 1000.0, 1),
                    }
                )
            rows.sort(key=lambda row: -row["pct"])
        browser.close()

    timings = got["timings"]
    median = statistics.median(timings)
    label = f"{args.label} " if args.label else ""
    print(f"[bench] {label}{got['backend']}  shot={args.shot}  nodes={got['nodes']}")
    print(f"[bench] renderScene(): median {median:.3f} ms   best {min(timings):.3f}   worst {max(timings):.3f}")
    print(f"[bench] implied ceiling {1000.0 / median:.1f} fps if render were the only cost")
    if rows:
        print("\n[bench] render-only CPU self time:")
        for row in rows[:15]:
            print(f"  {row['pct']:>6.2f}%  {row['ms']:>8.1f}ms  {row['fn']}")
    if errors:
        print("[bench] page errors:", errors[:3])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
