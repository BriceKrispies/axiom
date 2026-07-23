#!/usr/bin/env -S uv run --with playwright python
# /// script
# requires-python = ">=3.10"
# dependencies = ["playwright>=1.48"]
# ///
"""
agent_capture.py — a reusable capture agent for the Casino Games arcade.

An "agent" that drives the REAL running arcade the way a player would — open a
machine on the prize floor, wait for the ritual to reach a phase, move the
cursor, press an action — and captures the frame. It is the browser-side
analogue of an engine agent driver: this app is pure-TS and invisible to the
Rust `axiom-agent`, so control goes through the app's own affordances (the
`?game=/?seed=/?shot=/?backend=` boot URL and the `window.__casino` handle the
shell publishes) rather than a native agent binary. The shape is the same:
observe state, issue commands, capture the frame.

It is deliberately GENERIC — one verb per player-reachable action, so a new
capture needs a new scenario, not new code:

    # named preset (the treasure-chest convergence champion: canvas2d,
    # nine chests showing, frozen on a pinned seed):
    uv run apps/casino-games/web/browser/agent_capture.py --scene chests-ready

    # ad-hoc script — open the chests, hover the centre chest, shoot:
    uv run apps/casino-games/web/browser/agent_capture.py \
        --do play:treasure-chest-pick phase:ready move:480,300 shot

    # any game / seed / freeze tick / backend / output path:
    uv run apps/casino-games/web/browser/agent_capture.py \
        --game fishing-cast --seed 1234 --shot 120 --backend canvas2d \
        --out /tmp/cast.png

Verbs: play:<gameId>[@seed], back, phase:<name>, wait:<ms>, key:<code>,
       move:<x,y>, click:<x,y>, shot[:name].
Coordinates are LOGICAL canvas space (960x600), not screen pixels.

Determinism: the preferred path is the boot URL — `?shot=N` freezes the
simulation at tick N AND pins the view clock, so the frame is a pure function
of (seed, config, tick). `--do` scripts that run past the boot are as
deterministic as the actions they issue; prefer `phase:` over `wait:` so a
capture never races the fixed-step loop.

Prereq: serve the app first (preferred: the localhost manager) —
    uv run scripts/localhost_servers.py start-app casino-games --port 8087
then point this at it with --url (default http://localhost:8087/).
"""

from __future__ import annotations

import argparse
import base64
import pathlib
import sys
import time

from playwright.sync_api import sync_playwright

OUT_DIR = pathlib.Path(__file__).parent / "screenshots"

# The logical canvas the app normalizes every pointer sample into (shell.ts).
LOGICAL_W, LOGICAL_H = 960, 600


class CasinoAgent:
    """Drives one page of the running arcade through `window.__casino`."""

    def __init__(self, page):
        self.page = page

    # ── lifecycle ────────────────────────────────────────────────────────────
    def boot(self, url: str):
        self.page.goto(url, wait_until="load", timeout=30000)
        self.page.wait_for_function("() => window.__casino", timeout=10000)

    def play(self, game_id: str, seed: int | None = None):
        self.page.evaluate("([g, s]) => window.__casino.play(g, s ?? undefined)", [game_id, seed])
        self.settle()

    def back(self):
        self.page.evaluate("() => window.__casino.back()")
        self.settle()

    # ── observation ──────────────────────────────────────────────────────────
    def hud(self) -> dict | None:
        return self.page.evaluate("() => window.__casino.hud()")

    def brief(self) -> dict:
        """The few HUD fields worth printing beside a shot."""
        hud = self.hud()
        if hud is None:
            return {"phase": "(no game mounted)"}
        return {
            "phase": hud["phase"],
            "round": hud["round"],
            "seed": hud["audit"]["seedOrRoundId"],
            "locked": hud["inputLocked"],
            "result": hud["resultText"],
        }

    def await_phase(self, phase: str, timeout_ms: int = 15000):
        """Block until the session reaches `phase`. Beats sleeping: the reveal
        ritual's lengths scale with presentationSpeed and reduced motion."""
        self.page.wait_for_function(
            "(p) => (window.__casino.hud()?.phase ?? null) === p",
            arg=phase,
            timeout=timeout_ms,
        )

    # ── control (everything a player can reach) ──────────────────────────────
    def key(self, code: str):
        self.page.evaluate("(c) => window.__casino.press(c)", code)
        self.settle()

    def pointer(self, x: float, y: float, down: bool):
        self.page.evaluate("([x, y, d]) => window.__casino.pointer(x, y, d)", [x, y, down])

    def move(self, x: float, y: float):
        self.pointer(x, y, False)
        self.settle()

    def click(self, x: float, y: float):
        self.pointer(x, y, False)
        self.settle(2)
        self.pointer(x, y, True)
        self.settle(2)
        self.pointer(x, y, False)
        self.settle()

    # ── capture ──────────────────────────────────────────────────────────────
    def screenshot(self, path: pathlib.Path, clip: str, scale: float) -> pathlib.Path:
        path.parent.mkdir(parents=True, exist_ok=True)
        if clip == "native":
            # The canvas BACKING STORE: exactly what the renderer drew (960x600),
            # with no browser resampling. Reliable on canvas2d; a WebGL2 context
            # without preserveDrawingBuffer can read back blank — use --clip canvas
            # for webgl2 captures.
            data = self.page.evaluate("() => document.getElementById('axiom-canvas').toDataURL('image/png')")
            path.write_bytes(base64.b64decode(data.split(",", 1)[1]))
            return path
        kwargs = {}
        if clip in ("canvas", "stage"):
            box = self.page.locator("#axiom-canvas" if clip == "canvas" else "#stage").bounding_box()
            kwargs["clip"] = box
        self.page.screenshot(path=str(path), scale="css" if scale == 1 else "device", **kwargs)
        return path

    def settle(self, frames: int = 6):
        # Let the fixed-step loop fold the new input and render it before the
        # next observation/capture.
        self.page.wait_for_timeout(frames * 16)


# Named presets. `chests-ready` is the treasure-chest convergence champion: the
# nine chests showing on the lagoon, canvas2d (the deterministic baseline
# backend), frozen at a tick well past the intro, on the seed the reference
# frame carries in its HUD readout ("seed 470573198 - round 1").
SCENES: dict[str, dict] = {
    "chests-ready": {
        "game": "treasure-chest-pick",
        "seed": 470573198,
        "shot": 90,
        "backend": "canvas2d",
        "steps": ["phase:ready", "shot"],
    },
    "chests-hover": {
        "game": "treasure-chest-pick",
        "seed": 470573198,
        "backend": "canvas2d",
        "steps": ["phase:ready", f"move:{LOGICAL_W // 2},{LOGICAL_H // 2}", "shot"],
    },
    "chests-reveal": {
        "game": "treasure-chest-pick",
        "seed": 470573198,
        "backend": "canvas2d",
        "steps": ["phase:ready", f"click:{LOGICAL_W // 2},{LOGICAL_H // 2}", "phase:celebrating", "wait:400", "shot"],
    },
}


def run_step(agent: CasinoAgent, step: str, out: pathlib.Path, shots: list[pathlib.Path], clip: str, scale: float):
    verb, _, arg = step.partition(":")
    if verb == "play":
        game_id, _, seed = arg.partition("@")
        agent.play(game_id, int(seed) if seed else None)
    elif verb == "back":
        agent.back()
    elif verb == "phase":
        agent.await_phase(arg)
    elif verb == "wait":
        agent.page.wait_for_timeout(int(arg))
    elif verb == "key":
        agent.key(arg)
    elif verb in ("move", "click"):
        x, y = (float(v) for v in arg.split(","))
        (agent.move if verb == "move" else agent.click)(x, y)
    elif verb == "shot":
        name = arg or "capture"
        path = out if out.suffix == ".png" and not arg else (out if out.suffix == ".png" else out / f"{name}.png")
        shots.append(agent.screenshot(path, clip, scale))
        print(f"  [shot] {shots[-1]}  {agent.brief()}")
    else:
        raise SystemExit(f"unknown step verb: {verb!r} (in {step!r})")


def main() -> int:
    ap = argparse.ArgumentParser(description="Reusable Casino Games screenshot agent.")
    ap.add_argument("--url", default="http://localhost:8087/", help="running app URL")
    ap.add_argument("--scene", choices=sorted(SCENES), help="named capture preset")
    ap.add_argument("--do", nargs="+", metavar="STEP", help="ad-hoc verb:arg steps")
    ap.add_argument("--game", help="boot straight into this game id (?game=)")
    ap.add_argument("--seed", type=int, help="pin the session seed (?seed=)")
    ap.add_argument("--shot", type=int, help="freeze the simulation at this tick (?shot=)")
    ap.add_argument("--backend", default="canvas2d", choices=["auto", "canvas2d", "webgl2"])
    ap.add_argument("--size", default="1440x900", help="viewport WxH")
    ap.add_argument("--scale", type=float, default=1.0, help="device pixel ratio for clip captures")
    ap.add_argument("--clip", default="native", choices=["native", "canvas", "stage", "page"],
                    help="native = canvas backing store (960x600, exact); canvas/stage = CSS-scaled element; page = whole cabinet")
    ap.add_argument("--out", default=str(OUT_DIR / "agent-capture.png"), help="output PNG (or dir)")
    ap.add_argument("--headed", action="store_true", help="show the browser window")
    args = ap.parse_args()

    scene = SCENES[args.scene] if args.scene else {}
    if not args.scene and not args.do and not args.game:
        scene = SCENES["chests-ready"]
    steps = list(args.do) if args.do else list(scene.get("steps", ["phase:ready", "shot"]))

    # Boot through the app's own URL affordances: game, seed, freeze tick,
    # backend. Explicit flags win over the preset.
    query = {
        "game": args.game or scene.get("game"),
        "seed": args.seed if args.seed is not None else scene.get("seed"),
        "shot": args.shot if args.shot is not None else scene.get("shot"),
        "backend": None if (args.backend == "auto") else (args.backend or scene.get("backend")),
    }
    params = "&".join(f"{k}={v}" for k, v in query.items() if v is not None)
    url = f"{args.url.rstrip('/')}/{f'?{params}' if params else ''}"

    w, h = (int(v) for v in args.size.lower().split("x"))
    out = pathlib.Path(args.out)

    shots: list[pathlib.Path] = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=not args.headed)
        page = browser.new_context(
            viewport={"width": w, "height": h},
            device_scale_factor=args.scale,
            reduced_motion="no-preference",
        ).new_page()
        errors: list[str] = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        agent = CasinoAgent(page)
        print(f"agent -> {url}  {w}x{h}  scene={args.scene or '(custom)'}  clip={args.clip}")
        t0 = time.time()
        agent.boot(url)
        for step in steps:
            print(f"- {step}")
            run_step(agent, step, out, shots, args.clip, args.scale)
        browser.close()

    if errors:
        print("PAGE ERRORS:", *errors, sep="\n  ")
        return 1
    print(f"done in {time.time() - t0:.1f}s - {len(shots)} screenshot(s)")
    for s in shots:
        print(" ", s)
    return 0


if __name__ == "__main__":
    sys.exit(main())
