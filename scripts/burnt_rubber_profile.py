#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright"]
# ///
"""Profile Burnt Rubber's frame cost **as a function of position on the course**.

    uv run scripts/burnt_rubber_profile.py --url http://localhost:8085/

# Why a course sweep and not a benchmark

"Steady 60 with no drops" is not a claim about an average, it is a claim about
the *worst place on the road*. A mean frame time over a lap can sit comfortably
under budget while one corner drops frames every single time, and a benchmark
that reports one number cannot tell those apart. So this walks the car down the
course, samples frames at each station, and reports the profile against
distance — the shape that can actually answer "where does it break".

# Why the CPU is throttled

A desktop renders this game with so much headroom that vsync flattens
everything: every station reports an identical 16.7 ms and the profile is a
straight line that proves nothing. Throttling restores the headroom pressure a
phone has, so relative cost becomes visible. The numbers are then *relative* —
"station A costs 2.3x station B" — and must never be quoted as absolute
milliseconds for any real device.

# The trap this script is built around

An `Emulation.setCPUThrottlingRate` override belongs to the CDP session that set
it. If that session is collected or detached, the page silently returns to full
speed while every subsequent reading still *looks* throttled — and the failure
produces exactly the fast frame you were hoping for, so nothing about the output
looks wrong. The session is therefore held for the page's lifetime, and the
throttle is verified live both **before and after** the sweep. If the after
check fails, the run is reported as void rather than published.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright

STATE = Path(__file__).parent / ".burnt-rubber-profile"

# One in-page driver, run once. Chatty per-station round-trips would put Python
# process latency inside the measurement window; this keeps the whole sweep in
# the page and returns a single JSON blob.
SWEEP_JS = r"""
async ({stations, settle, measure, speed}) => {
  // The probe hooks live inside the page's module script, not on `window`. A
  // dynamic import of the same URL returns the *cached* module instance -- the
  // one already initialized by the page -- so this steers the running game
  // rather than booting a second copy of it.
  const mod = await import('./pkg/axiom_burnt_rubber.js');

  const nextFrame = () => new Promise(r => requestAnimationFrame(r));
  async function frames(n) {
    const out = [];
    let last = await nextFrame();
    for (let i = 0; i < n; i++) {
      const now = await nextFrame();
      out.push(now - last);
      last = now;
    }
    return out;
  }

  // A deterministic racing line. Without it the car is driven by whatever the
  // last input was, and two runs sample different geometry at the same station.
  mod.burnt_rubber_probe_autopilot(true);

  const hud = () => (document.getElementById('burnt-rubber-hud') || {}).innerText || '';
  const num = (text, re) => { const m = text.match(re); return m ? Number(m[1]) : null; };

  const rows = [];
  for (const d of stations) {
    mod.burnt_rubber_probe_place(d, speed);
    // Discarded: the chunk window has to slide, scenery has to regenerate and
    // the LOD pools have to refill. Measuring those frames would report the
    // cost of teleporting, which is not a thing the game ever does.
    await frames(settle);
    const t = await frames(measure);
    const panel = hud();
    rows.push({
      distance: d,
      frames: t,
      road_tris: num(panel, /road\s+(\d+)\s+tris/),
      scenery: num(panel, /scenery\s+(\d+)\s+props/),
      traffic: num(panel, /traffic\s+(\d+)\s+cars/),
      state: JSON.parse(mod.burnt_rubber_probe_state()),
    });
  }
  mod.burnt_rubber_probe_autopilot(false);
  return rows;
}
"""


def summarize(frames: list[float]) -> dict:
    ordered = sorted(frames)
    n = len(ordered)
    return {
        "median": statistics.median(ordered),
        "p95": ordered[min(n - 1, int(n * 0.95))],
        "worst": ordered[-1],
        # The question is dropped frames, so count them directly rather than
        # inferring from an average. 20 ms is a 60 Hz frame plus enough slack
        # that ordinary vsync jitter is not miscounted as stutter.
        "dropped": sum(1 for f in ordered if f > 20.0),
        "n": n,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://localhost:8085/")
    ap.add_argument("--throttle", type=float, default=4.0)
    ap.add_argument("--length", type=float, default=9284.0, help="course length (m)")
    ap.add_argument("--step", type=float, default=200.0, help="station spacing (m)")
    ap.add_argument("--speed", type=float, default=60.0, help="m/s at each station")
    ap.add_argument("--settle", type=int, default=25)
    ap.add_argument("--measure", type=int, default=45)
    ap.add_argument("--profile-ms", type=int, default=6000)
    ap.add_argument(
        "--attribute",
        default=None,
        help="substring of the function to attribute back to its callers "
        "(default: the heaviest self-time frame)",
    )
    ap.add_argument("--out", default=str(STATE / "profile.json"))
    args = ap.parse_args()

    stations = [round(d, 1) for d in frange(0.0, args.length, args.step)]
    STATE.mkdir(parents=True, exist_ok=True)

    with sync_playwright() as pw:
        browser = pw.chromium.launch(
            headless=True,
            args=["--enable-unsafe-webgpu", "--use-angle=default", "--enable-gpu"],
        )
        page = browser.new_page(viewport={"width": 1280, "height": 720})
        errors: list[str] = []
        page.on("pageerror", lambda e: errors.append(str(e)))

        # Held for the page's lifetime -- see the module docstring.
        cdp = page.context.new_cdp_session(page)
        cdp.send("Emulation.setCPUThrottlingRate", {"rate": args.throttle})

        page.goto(args.url, wait_until="load", timeout=60000)
        cdp.send("Emulation.setCPUThrottlingRate", {"rate": args.throttle})
        page.wait_for_timeout(4000)

        start_race(page)
        page.wait_for_timeout(3000)
        open_telemetry_panel(page)
        page.wait_for_timeout(800)

        rows = page.evaluate(
            SWEEP_JS,
            {
                "stations": stations,
                "settle": args.settle,
                "measure": args.measure,
                "speed": args.speed,
            },
        )

        # The after-check. If the throttle died mid-sweep every number above is
        # a lie, and a lie that looks like good news.
        try:
            cdp.send("Emulation.setCPUThrottlingRate", {"rate": args.throttle})
            throttle_live = True
        except Exception as exc:
            throttle_live = False
            print(f"!! throttle session died: {exc}", file=sys.stderr)

        report = [{**r, **summarize(r["frames"])} for r in rows]
        report.sort(key=lambda r: -r["p95"])

        print(f"throttle {args.throttle}x  live_after={throttle_live}  "
              f"stations={len(report)}  page_errors={len(errors)}")
        if not throttle_live:
            print("!! RUN VOID -- throttle not held to the end", file=sys.stderr)
        print()
        print(f"{'dist_m':>8} {'median':>7} {'p95':>7} {'worst':>7} {'drop':>5} "
              f"{'road_tris':>10} {'scenery':>8} {'traffic':>8}")
        for r in report[:25]:
            print(f"{r['distance']:>8.0f} {r['median']:>7.1f} {r['p95']:>7.1f} "
                  f"{r['worst']:>7.1f} {r['dropped']:>5} {str(r['road_tris']):>10} "
                  f"{str(r['scenery']):>8} {str(r['traffic']):>8}")

        out = Path(args.out)
        out.write_text(json.dumps(
            {
                "throttle": args.throttle,
                "throttle_live_after": throttle_live,
                "page_errors": errors,
                "stations": report,
            },
            indent=2,
        ), encoding="utf-8")
        print(f"\nwrote {out}")

        # Attribution: park at the worst station and sample the main thread.
        worst = report[0]
        print(f"\n=== V8 self-time at the worst station ({worst['distance']:.0f} m) ===")
        for row in sample_profile(page, cdp, worst["distance"], args):
            print(row)

        browser.close()
    return 0


def frange(start: float, stop: float, step: float):
    d = start
    while d <= stop:
        yield d
        d += step


def start_race(page) -> None:
    """Tap the START RACE button.

    Dispatched at the button's measured centre rather than a hard-coded point:
    the start screen is `pointer-events: none`, so the event must land on the
    canvas while the *coordinates* fall inside the button.
    """
    page.evaluate(
        """() => {
          const el = Array.from(document.querySelectorAll('#burnt-rubber-start *'))
            .find(e => (e.textContent || '').trim().startsWith('START RACE'));
          const r = (el || document.body).getBoundingClientRect();
          const c = document.getElementById('axiom-burnt-rubber-canvas');
          const o = {bubbles:true, cancelable:true, pointerType:'touch', isPrimary:true,
                     pointerId:1, clientX:Math.round(r.x+r.width/2), clientY:Math.round(r.y+r.height/2)};
          c.dispatchEvent(new PointerEvent('pointerdown', o));
          c.dispatchEvent(new PointerEvent('pointerup', o));
        }"""
    )


def open_telemetry_panel(page) -> None:
    """Three taps on the speedometer. The panel is where the per-frame scene
    counters are published, so the sweep can pair a cost with its content."""
    page.evaluate(
        """() => {
          const s = document.getElementById('burnt-rubber-speed');
          if (!s) return;
          const r = s.getBoundingClientRect();
          const o = {bubbles:true, cancelable:true, pointerType:'touch', isPrimary:true,
                     pointerId:1, clientX:Math.round(r.x+r.width/2), clientY:Math.round(r.y+r.height/2)};
          for (let i = 0; i < 3; i++) {
            s.dispatchEvent(new PointerEvent('pointerdown', o));
            s.dispatchEvent(new PointerEvent('pointerup', o));
          }
        }"""
    )


def frame_name(node: dict) -> str:
    """A node's function name, or a readable stand-in."""
    frame = node.get("callFrame", {})
    return frame.get("functionName") or "(anonymous)"


def index_profile(profile: dict) -> tuple[dict, dict]:
    """`(nodes by id, child id -> parent id)`.

    V8 hands back a *call tree* — each node lists its children — but the sample
    array only names leaves. Inverting the child links is what lets a leaf be
    walked back up to whoever called it, which is the entire difference between
    "`hashbrown` costs 10%" and "*this function* is spending 10% in `hashbrown`".
    """
    nodes = {n["id"]: n for n in profile["nodes"]}
    parents = {
        child: n["id"] for n in profile["nodes"] for child in n.get("children", [])
    }
    return nodes, parents


def stack_of(node_id: int, nodes: dict, parents: dict, limit: int = 128) -> list[str]:
    """Leaf-first call stack for a sampled node.

    Guards against a cycle in the parent links (malformed profiles exist) and
    against unbounded depth, because a profiler that hangs on its own data is
    worse than one that reports a truncated stack.
    """
    out: list[str] = []
    seen: set[int] = set()
    current = node_id
    while current is not None and current not in seen and len(out) < limit:
        seen.add(current)
        node = nodes.get(current)
        if node is None:
            break
        out.append(frame_name(node))
        current = parents.get(current)
    return out


def attribute(profile: dict, target: str, top: int = 12) -> list[str]:
    """Who is spending time in `target`, by share of `target`'s own samples.

    Self time answers "what is hot". It cannot answer "whose fault is it", and
    for a shared leaf — an allocator, a hash table, a memcpy — the second
    question is the only actionable one. Guessing the caller from a flat self
    time profile is how three plausible-looking `HashMap`s got optimised without
    moving the number.
    """
    nodes, parents = index_profile(profile)
    direct: dict[str, int] = {}
    chains: dict[str, int] = {}
    total = 0
    for sample in profile.get("samples", []):
        node = nodes.get(sample)
        if node is None or target not in frame_name(node):
            continue
        total += 1
        stack = stack_of(sample, nodes, parents)
        caller = stack[1] if len(stack) > 1 else "(root)"
        direct[caller] = direct.get(caller, 0) + 1
        # A few frames of context, so a caller that is itself generic (an
        # iterator adapter, a `collect`) still resolves to something nameable.
        chains[" <- ".join(stack[1:5])] = chains.get(" <- ".join(stack[1:5]), 0) + 1

    rows = [f"  target {target!r}: {total} samples"]
    rows.append("  -- immediate callers --")
    rows += [
        f"  {100.0 * n / max(total, 1):5.1f}%  {name[:100]}"
        for name, n in sorted(direct.items(), key=lambda kv: -kv[1])[:top]
    ]
    rows.append("  -- call chains (leaf -> up) --")
    rows += [
        f"  {100.0 * n / max(total, 1):5.1f}%  {chain[:150]}"
        for chain, n in sorted(chains.items(), key=lambda kv: -kv[1])[:top]
    ]
    return rows


def inclusive(profile: dict, top: int = 15) -> list[str]:
    """Total time per function: every sample where it appears anywhere on the
    stack, not just as the leaf.

    This is the column that says who *owns* the frame. A function that dispatches
    all its work to callees has near-zero self time and can dominate inclusive
    time, which is exactly the shape of a render submit or a snapshot refresh.
    """
    nodes, parents = index_profile(profile)
    totals: dict[str, int] = {}
    samples = profile.get("samples", [])
    for sample in samples:
        # `set`: a recursive function must not be counted once per stack frame.
        for name in set(stack_of(sample, nodes, parents)):
            totals[name] = totals.get(name, 0) + 1
    count = max(len(samples), 1)
    return [
        f"  {100.0 * n / count:5.1f}%  {name[:90]}"
        for name, n in sorted(totals.items(), key=lambda kv: -kv[1])[:top]
    ]


def sample_profile(page, cdp, distance: float, args) -> list[str]:
    """Park at one station and sample the main thread there.

    Self time, wasm included: the expensive work lives inside one opaque rAF
    callback, and no amount of JS-side wrapping can see into it. The V8 sampling
    profiler can, as long as the module keeps its name section.
    """
    page.evaluate(
        """async ({d, speed}) => {
             const mod = await import('./pkg/axiom_burnt_rubber.js');
             mod.burnt_rubber_probe_autopilot(true);
             mod.burnt_rubber_probe_place(d, speed);
           }""",
        {"d": distance, "speed": args.speed},
    )
    page.wait_for_timeout(1200)
    prof = page.context.new_cdp_session(page)
    prof.send("Profiler.enable")
    prof.send("Profiler.setSamplingInterval", {"interval": 100})
    prof.send("Profiler.start")
    page.wait_for_timeout(args.profile_ms)
    profile = prof.send("Profiler.stop")["profile"]
    cdp.send("Emulation.setCPUThrottlingRate", {"rate": args.throttle})

    nodes = {n["id"]: n for n in profile["nodes"]}
    hits: dict[int, int] = {}
    for s in profile.get("samples", []):
        hits[s] = hits.get(s, 0) + 1
    total = max(sum(hits.values()), 1)
    ranked = sorted(hits.items(), key=lambda kv: -kv[1])
    rows = ["  -- self time --"]
    for node_id, count in ranked[:15]:
        frame = nodes.get(node_id, {}).get("callFrame", {})
        name = frame.get("functionName") or "(anonymous)"
        url = (frame.get("url") or "").rsplit("/", 1)[-1]
        rows.append(f"  {100.0*count/total:5.1f}%  {name[:70]:<70} {url}")

    rows.append("")
    rows.append("  -- inclusive time (who OWNS the frame) --")
    rows += inclusive(profile)

    # Attribute the heaviest *shared* leaf back to its callers. Defaults to the
    # top self-time frame, which is precisely the one a flat profile leaves
    # unexplained.
    target = args.attribute or frame_name(nodes[ranked[0][0]])
    rows.append("")
    rows.append(f"  -- attribution --")
    rows += attribute(profile, target)
    return rows


if __name__ == "__main__":
    raise SystemExit(main())
