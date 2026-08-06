#!/usr/bin/env -S uv run --with playwright python
# /// script
# requires-python = ">=3.10"
# dependencies = ["playwright>=1.48"]
# ///
"""
profile_frame.py — where a frame's time actually goes, in exact numbers.

`bench_render.py` times `renderScene()` alone on a frozen scene, which answers
"is a rendering change faster". It cannot answer "why is the game at 40 fps",
because the frame is more than the renderer and because the CPU submitting GL
commands is not the GPU executing them. This script answers that, with measured
numbers rather than sampled guesses, in four parts:

1. GL CALL CENSUS (exact). `getContext` is wrapped before the app boots, so the
   engine gets a Proxy that counts every WebGL call by name and accumulates the
   JS-side time spent inside it. Counts are exact, not sampled: 468 draw calls a
   frame is a fact, not an estimate. `drawElements` index counts are summed, so
   the triangle count per frame is exact too.

2. FRAME JS (exact). `requestAnimationFrame` is wrapped, so the wall time of the
   engine's own callback — the whole fold + view + reconcile + render — is
   measured per frame, separately from the time the browser spends outside it.
   The gap between "JS per frame" and "frame interval" is the part that is NOT
   the main thread, which is what a CPU profile reports as idle and cannot
   attribute.

3. GPU TIME (measured, not inferred). With `EXT_disjoint_timer_query_webgl2` the
   real GPU cost of one `renderScene()` is read back from the driver. Where the
   extension is absent (it usually is in headless Chromium) the script falls back
   to a forced-sync delta: N renders timed with, then without, a trailing
   `readPixels`, which stalls until the GPU has drained. Either way the number is
   the GPU's, not a guess derived from the frame interval.

4. CPU CALL TREE (sampled, inclusive). A CDP profile aggregated by INCLUSIVE time
   per function, so a subsystem's real share is visible — self-time alone hides
   a cheap function that calls an expensive one, which is exactly how a
   reconciler disappears from a self-time table.

    uv run apps/casino-games/web/browser/profile_frame.py --url http://localhost:8087/
    uv run apps/casino-games/web/browser/profile_frame.py --backend canvas2d
    uv run apps/casino-games/web/browser/profile_frame.py --rounds 3   # paired, for a change

Wrapping every GL call costs real time (two `performance.now()` per call), and
the script reports that overhead explicitly so it is never mistaken for the
engine's own cost. Run with `--no-gl-timing` for counts only.
"""

from __future__ import annotations

import argparse
import collections
import json
import statistics
import sys

from playwright.sync_api import sync_playwright

# The same flags bench_render.py uses: without them headless Chromium falls back
# to SwiftShader and every GPU number below would describe a software rasterizer
# pretending to be a GPU.
BROWSER_ARGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-gl=angle"]

# Installed before any page script runs, so the engine's very first getContext is
# already wrapped.
INSTRUMENT = """
(glTiming) => {
  const stats = { calls: {}, ms: {}, indices: 0, draws: 0, frames: 0, jsMs: [], intervals: [] };
  globalThis.__prof = stats;

  const origGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (kind, ...rest) {
    const ctx = origGetContext.call(this, kind, ...rest);
    if (ctx === null || kind !== "webgl2" || ctx.__wrapped) return ctx;
    const cache = new Map();
    const proxy = new Proxy(ctx, {
      get(target, key) {
        if (key === "__wrapped") return true;
        const cached = cache.get(key);
        if (cached !== undefined) return cached;
        const value = target[key];
        if (typeof value !== "function") return value;
        const name = String(key);
        const wrapped = glTiming
          ? function (...args) {
              const t0 = performance.now();
              const out = value.apply(target, args);
              stats.ms[name] = (stats.ms[name] || 0) + (performance.now() - t0);
              stats.calls[name] = (stats.calls[name] || 0) + 1;
              if (name === "drawElements") { stats.draws += 1; stats.indices += args[1]; }
              return out;
            }
          : function (...args) {
              stats.calls[name] = (stats.calls[name] || 0) + 1;
              if (name === "drawElements") { stats.draws += 1; stats.indices += args[1]; }
              return value.apply(target, args);
            };
        cache.set(key, wrapped);
        return wrapped;
      },
    });
    return proxy;
  };

  // Wrap rAF so the engine callback's own wall time is measured, apart from
  // whatever the browser does between callbacks.
  let lastStart = 0;
  const origRaf = globalThis.requestAnimationFrame.bind(globalThis);
  globalThis.requestAnimationFrame = (cb) =>
    origRaf((ts) => {
      const t0 = performance.now();
      if (lastStart !== 0) stats.intervals.push(t0 - lastStart);
      lastStart = t0;
      cb(ts);
      stats.jsMs.push(performance.now() - t0);
      stats.frames += 1;
    });
}
"""

# Run in the page, against the engine module the page itself loaded, so the store
# singleton is the real populated scene.
GPU_PROBE = """
async ({ iters }) => {
  const script = document.querySelector('script[type="importmap"]');
  const engine = await import(JSON.parse(script.textContent).imports["@axiom/web-engine"]);
  const canvas = document.getElementById("axiom-canvas");
  // The RAW context (our proxy sets __wrapped but forwards everything).
  const gl = canvas.getContext("webgl2");
  if (gl === null) return { error: "no webgl2 context" };

  const ext = gl.getExtension("EXT_disjoint_timer_query_webgl2");
  const out = { backend: engine.rendererBackendName(), nodes: engine.rendererNodeCount(), ext: ext !== null };

  // Warm up so nothing below pays first-call costs.
  for (let i = 0; i < 20; i += 1) engine.renderScene();

  if (ext !== null) {
    const samples = [];
    for (let i = 0; i < iters; i += 1) {
      const q = gl.createQuery();
      gl.beginQuery(ext.TIME_ELAPSED_EXT, q);
      engine.renderScene();
      gl.endQuery(ext.TIME_ELAPSED_EXT);
      // Spin until the driver has the result (bounded).
      for (let t = 0; t < 2000; t += 1) {
        if (gl.getQueryParameter(q, gl.QUERY_RESULT_AVAILABLE)) break;
        await new Promise((r) => setTimeout(r, 1));
      }
      const disjoint = gl.getParameter(ext.GPU_DISJOINT_EXT);
      if (gl.getQueryParameter(q, gl.QUERY_RESULT_AVAILABLE) && !disjoint) {
        samples.push(gl.getQueryParameter(q, gl.QUERY_RESULT) / 1e6);
      }
      gl.deleteQuery(q);
    }
    samples.sort((a, b) => a - b);
    out.gpuMs = samples.length ? samples[Math.floor(samples.length / 2)] : null;
    out.gpuSamples = samples.length;
  }

  // Fallback / cross-check: submission-only vs submission + forced drain. A
  // 1x1 readPixels stalls the CPU until the GPU has finished the frame, so the
  // difference is the GPU work the submission loop was hiding.
  const px = new Uint8Array(4);
  const timeLoop = (drain) => {
    const t0 = performance.now();
    for (let i = 0; i < iters; i += 1) {
      engine.renderScene();
      if (drain) gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    }
    return (performance.now() - t0) / iters;
  };
  // Alternate so drift affects both halves equally.
  const subs = [], drains = [];
  for (let r = 0; r < 5; r += 1) { subs.push(timeLoop(false)); drains.push(timeLoop(true)); }
  subs.sort((a, b) => a - b); drains.sort((a, b) => a - b);
  out.submitMs = subs[2];
  out.drainedMs = drains[2];
  return out;
}
"""


def median(values: list[float]) -> float:
    return statistics.median(values) if values else 0.0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://localhost:8087/")
    ap.add_argument("--game", default="treasure-chest-pick")
    ap.add_argument("--backend", default="webgl2")
    ap.add_argument("--seed", type=int, default=470573198)
    ap.add_argument("--seconds", type=float, default=5.0)
    ap.add_argument("--iters", type=int, default=40, help="renderScene calls per GPU timing loop")
    ap.add_argument("--dpr", type=float, default=1.0)
    ap.add_argument("--no-gl-timing", action="store_true", help="count GL calls but do not time them")
    ap.add_argument("--rounds", type=int, default=1)
    ap.add_argument("--extra", default="", help="extra query string, e.g. press=Space@30 to profile the reveal")
    ap.add_argument("--settle", type=float, default=2.5, help="seconds to wait before profiling (pick the phase)")
    ap.add_argument("--label", default="", help="tag for the report header")
    args = ap.parse_args(argv)

    boot = f"{args.url.rstrip('/')}/?game={args.game}&backend={args.backend}&seed={args.seed}"
    boot += f"&{args.extra}" if args.extra else ""

    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True, args=BROWSER_ARGS)
        for rnd in range(args.rounds):
            ctx = browser.new_context(viewport={"width": 1280, "height": 900}, device_scale_factor=args.dpr)
            ctx.add_init_script(f"({INSTRUMENT})({json.dumps(not args.no_gl_timing)})")
            page = ctx.new_page()
            errors: list[str] = []
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto(boot, wait_until="load")
            page.wait_for_timeout(int(args.settle * 1000))

            # ── the live frame ────────────────────────────────────────────────
            page.evaluate("() => { const p = globalThis.__prof; p.calls = {}; p.ms = {}; p.indices = 0; p.draws = 0; p.frames = 0; p.jsMs = []; p.intervals = []; }")
            cdp = page.context.new_cdp_session(page)
            cdp.send("Profiler.enable")
            cdp.send("Profiler.setSamplingInterval", {"interval": 50})
            cdp.send("Profiler.start")
            page.wait_for_timeout(int(args.seconds * 1000))
            prof = cdp.send("Profiler.stop")["profile"]
            live = page.evaluate("() => { const p = globalThis.__prof; return { calls: p.calls, ms: p.ms, indices: p.indices, draws: p.draws, frames: p.frames, jsMs: p.jsMs, intervals: p.intervals }; }")
            env = page.evaluate("""() => { const c = document.getElementById('axiom-canvas');
              return { dpr: devicePixelRatio, backing: [c.width, c.height],
                       css: [Math.round(c.getBoundingClientRect().width), Math.round(c.getBoundingClientRect().height)] }; }""")

            # ── the GPU ───────────────────────────────────────────────────────
            gpu = page.evaluate(GPU_PROBE, {"iters": args.iters})

            frames = max(live["frames"], 1)
            js = sorted(live["jsMs"])
            iv = sorted(live["intervals"])
            js_med = median(js)
            iv_med = median(iv)
            gl_ms_total = sum(live["ms"].values())

            print(f"\n{'=' * 78}")
            tag = f" · {args.label}" if args.label else ""
            print(f"FRAME PROFILE — {args.game} · {gpu.get('backend', '?')} · dpr {env['dpr']}{tag} · round {rnd + 1}/{args.rounds}")
            print(f"{'=' * 78}")
            print(f"  css {env['css'][0]}x{env['css'][1]}   backing {env['backing'][0]}x{env['backing'][1]}"
                  f" = {env['backing'][0] * env['backing'][1] / 1000:.0f}k px   nodes {gpu.get('nodes', '?')}")
            if errors:
                print(f"  !! page errors: {errors[:2]}")

            print(f"\n  WALL CLOCK        {frames} frames over {args.seconds:.0f}s")
            print(f"    frame interval  median {iv_med:6.2f} ms   ->  {1000 / max(iv_med, 1e-9):5.1f} fps"
                  f"   p95 {iv[int(len(iv) * 0.95)] if iv else 0:6.2f} ms")
            print(f"    JS in rAF       median {js_med:6.2f} ms   ({100 * js_med / max(iv_med, 1e-9):4.1f}% of the frame)")
            print(f"    NOT main thread median {max(iv_med - js_med, 0):6.2f} ms   "
                  f"({100 * max(iv_med - js_med, 0) / max(iv_med, 1e-9):4.1f}% — vsync wait + GPU + compositor)")

            print(f"\n  GPU (measured, {'timer query' if gpu.get('ext') else 'forced-sync delta'})")
            if gpu.get("gpuMs") is not None:
                print(f"    renderScene GPU   {gpu['gpuMs']:6.2f} ms   (median of {gpu['gpuSamples']} timer queries)")
            print(f"    submit only       {gpu.get('submitMs', 0):6.2f} ms   CPU issuing the GL commands")
            print(f"    submit + drain    {gpu.get('drainedMs', 0):6.2f} ms   forced to wait for the GPU")
            print(f"    => GPU work       {max(gpu.get('drainedMs', 0) - gpu.get('submitMs', 0), 0):6.2f} ms   (drain - submit)")

            print(f"\n  GL CALLS (exact, per frame)")
            per_frame = sorted(live["calls"].items(), key=lambda kv: -kv[1])
            print(f"    {'call':24s} {'per frame':>10s} {'total':>10s} {'ms/frame':>9s}")
            for name, count in per_frame[:10]:
                ms = live["ms"].get(name, 0.0) / frames
                print(f"    {name:24s} {count / frames:10.1f} {count:10d} {ms:9.3f}")
            print(f"    {'-' * 56}")
            print(f"    {'ALL GL':24s} {sum(live['calls'].values()) / frames:10.1f} {sum(live['calls'].values()):10d} {gl_ms_total / frames:9.3f}")
            print(f"    draw calls {live['draws'] / frames:.1f}/frame · triangles {live['indices'] / frames / 3:,.0f}/frame")
            if not args.no_gl_timing:
                print(f"    (instrumentation overhead is inside 'ms/frame': 2 performance.now() per call)")

            # ── inclusive CPU tree ────────────────────────────────────────────
            nodes = {n["id"]: n for n in prof["nodes"]}
            children: dict[int, list[int]] = {i: n.get("children", []) for i, n in nodes.items()}
            self_us: collections.Counter[int] = collections.Counter()
            deltas = prof.get("timeDeltas") or []
            samples = prof.get("samples") or []
            total = 0
            for i, sid in enumerate(samples):
                d = deltas[i] if i < len(deltas) else 0
                if d > 0:
                    self_us[sid] += d
                    total += d

            memo: dict[int, int] = {}

            def inclusive(nid: int) -> int:
                if nid in memo:
                    return memo[nid]
                memo[nid] = self_us.get(nid, 0)  # guard against cycles
                memo[nid] = self_us.get(nid, 0) + sum(inclusive(c) for c in children.get(nid, []))
                return memo[nid]

            by_name_incl: collections.Counter[str] = collections.Counter()
            by_name_self: collections.Counter[str] = collections.Counter()
            for nid, n in nodes.items():
                cf = n["callFrame"]
                name = cf.get("functionName") or "(anonymous)"
                url = (cf.get("url") or "").rsplit("/", 1)[-1].split("?")[0]
                label = f"{name} [{url}]"
                by_name_incl[label] += inclusive(nid)
                by_name_self[label] += self_us.get(nid, 0)

            idle = by_name_self.get("(idle) []", 0) + by_name_self.get("(program) []", 0)
            active = max(total - by_name_self.get("(idle) []", 0), 1)
            print(f"\n  CPU CALL TREE (sampled @50us; {total / 1000:.0f} ms sampled, {active / 1000:.0f} ms active)")
            print(f"    {'inclusive':>10s} {'self':>8s}  {'%active':>8s}  function")
            skip = {"(idle) []", "(root) []", "(program) []", "(garbage collector) []"}
            for label, incl in by_name_incl.most_common(40):
                if label in skip:
                    continue
                print(f"    {incl / 1000:9.1f}ms {by_name_self[label] / 1000:7.1f}ms {100 * incl / active:7.1f}%  {label}")
                if len([x for x in by_name_incl.most_common(40) if x[0] not in skip][:14]) and incl / 1000 < 1:
                    break
            gc = by_name_self.get("(garbage collector) []", 0)
            print(f"    {'':>10s} {gc / 1000:7.1f}ms {100 * gc / active:7.1f}%  (garbage collector)")
            ctx.close()
        browser.close()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
