#!/usr/bin/env -S uv run --with playwright python
"""
agent_capture.py — a reusable capture agent for Arena Forge.

An "agent" that drives the REAL running game the way a player would — through the
game's own control surface (`window.__arena`, the same dev handle the interaction
test uses) — runs a scripted sequence of high-level game actions, and captures an
in-game screenshot. It is the browser-side analogue of an engine agent driver:
the app is pure-TS and invisible to the Rust `axiom-agent`, so control goes
through the game's public debug surface rather than a native agent binary, but the
shape is the same — observe state, issue commands, capture the frame.

It is deliberately GENERIC. `ArenaAgent` exposes one method per game action; a
scenario is just a list of `verb:arg` steps, so new captures need no new code:

    # named preset (reproduces the resized-Ironbound gallery reference):
    uv run apps/arena-forge/web/browser/agent_capture.py --scene ironbound

    # ad-hoc script — go to the gallery, filter Emberkin, big icons, front-on, shoot:
    uv run apps/arena-forge/web/browser/agent_capture.py \
        --do goto:figure_lab spin:off group:emberkin icon:180 shot

    # any viewport / backend / output path:
    uv run apps/arena-forge/web/browser/agent_capture.py --scene ironbound \
        --size 1280x720 --backend webgl2 --out /tmp/iron.png

Verbs: goto:<screen>, group:<lab-group>, sort:<mode>, search:<text|->,
       icon:<px>, cols:<n>, select:<cardId|index>, spin:<on|off>,
       forged:<on|off>, wait:<ms>, shot[:name].

Prereq: serve the app first (preferred: the localhost manager) —
    uv run scripts/localhost_servers.py start-app arena-forge --port 8085
then point this at it with --url (default http://localhost:8085/).
"""

from __future__ import annotations

import argparse
import pathlib
import sys
import time

from playwright.sync_api import sync_playwright

OUT_DIR = pathlib.Path(__file__).parent / "screenshots"


class ArenaAgent:
    """Drives one page of the running game through `window.__arena`."""

    def __init__(self, page):
        self.page = page

    # ── lifecycle ────────────────────────────────────────────────────────────
    def boot(self, url: str):
        self.page.goto(url, wait_until="load", timeout=30000)
        self.page.wait_for_function("() => window.__arena", timeout=10000)

    def goto(self, screen: str):
        self.page.evaluate("(s) => window.__arena.debugGoto(s)", screen)
        if screen == "figure_lab":
            self.page.wait_for_function("() => window.__arena.debugLabInfo() !== null", timeout=8000)
        self.settle()

    # ── observation ──────────────────────────────────────────────────────────
    def info(self) -> dict:
        return self.page.evaluate("() => window.__arena.debugLabInfo()")

    def card_ids(self) -> list[str]:
        return self.page.evaluate("() => window.__arena.debugLabCardIds()")

    # ── gallery control ──────────────────────────────────────────────────────
    def group(self, name: str):
        # Selecting a card also selects its group; use the first card of the group
        # so the filter changes even when nothing is selected yet.
        self.page.evaluate("(g) => window.__arena.debugLabSelect(g, '')", name)
        self.settle()

    def sort(self, mode: str):
        self.page.evaluate("(m) => window.__arena.debugLabSort(m)", mode)
        self.settle()

    def search(self, term: str):
        self.page.evaluate("(t) => window.__arena.debugLabSearch(t)", "" if term == "-" else term)
        self.settle()

    def icon(self, px: float):
        self.page.evaluate("(p) => window.__arena.debugLabSetIcon(p)", px)
        self.settle()

    def columns(self, target: int):
        """Resize the icons until the grid lays out exactly `target` columns
        (bigger icons => fewer columns). Robust to the icon clamp: stops at the
        edge of the resize range if the target is unreachable."""
        for _ in range(48):
            cols = self.info()["columns"]
            if cols == target:
                return
            self.page.evaluate("(f) => window.__arena.debugLabZoom(f)", 1.06 if cols > target else 0.95)
            self.settle()
        # Fell through: report where we ended up rather than silently missing.
        print(f"  [cols] wanted {target}, settled at {self.info()['columns']}")

    def select(self, card: str):
        ids = self.card_ids()
        card_id = ids[int(card)] if card.isdigit() else card
        group = self.info()["group"]
        self.page.evaluate("([g, c]) => window.__arena.debugLabSelect(g, c)", [group, card_id])
        self.settle()
        got = self.info()["card"]
        if got != card_id:
            raise SystemExit(f"select failed: no card '{card_id}' in {ids}")

    def spin(self, on: bool):
        self.page.evaluate("(o) => window.__arena.debugLabSpin(o)", on)
        self.settle()

    def forged(self, on: bool):
        self.page.evaluate("(o) => window.__arena.debugLabForged(o)", on)
        self.settle()

    # ── capture ──────────────────────────────────────────────────────────────
    def screenshot(self, path: pathlib.Path, clip: str = "none") -> pathlib.Path:
        path.parent.mkdir(parents=True, exist_ok=True)
        kwargs = {}
        if clip == "gallery":
            r = self.page.evaluate("() => window.__arena.debugLabGalleryRect()")
            if r:
                kwargs["clip"] = {"x": r["x"], "y": r["y"], "width": r["w"], "height": r["h"]}
        self.page.screenshot(path=str(path), **kwargs)
        return path

    def settle(self, frames: int = 6):
        # Let the fixed-step loop render the new state (and the spin reach a stable
        # pose) before the next observation/capture.
        self.page.wait_for_timeout(frames * 16)


# The one named preset asked for: the resized Ironbound gallery, Iron Vanguard
# selected, figures front-on — matching the supplied reference frame.
SCENES: dict[str, list[str]] = {
    "ironbound": ["goto:figure_lab", "spin:off", "group:ironbound", "cols:4", "select:iron_vanguard", "shot"],
    "emberkin": ["goto:figure_lab", "spin:off", "group:emberkin", "cols:4", "shot"],
    "all-tribes": ["goto:figure_lab", "spin:off", "sort:tribe", "icon:96", "shot"],
}

BOOLS = {"on": True, "off": False, "true": True, "false": False, "1": True, "0": False}


def run_step(agent: ArenaAgent, step: str, out: pathlib.Path, shots: list[pathlib.Path], clip: str = "none"):
    verb, _, arg = step.partition(":")
    if verb == "goto":
        agent.goto(arg)
    elif verb == "group":
        agent.group(arg)
    elif verb == "sort":
        agent.sort(arg)
    elif verb == "search":
        agent.search(arg or "-")
    elif verb == "icon":
        agent.icon(float(arg))
    elif verb == "cols":
        agent.columns(int(arg))
    elif verb == "select":
        agent.select(arg)
    elif verb == "spin":
        agent.spin(BOOLS[arg])
    elif verb == "forged":
        agent.forged(BOOLS[arg])
    elif verb == "wait":
        agent.page.wait_for_timeout(int(arg))
    elif verb == "shot":
        name = arg or "capture"
        path = out if out.suffix == ".png" and not arg else (out / f"{name}.png" if out.is_dir() or out.suffix != ".png" else out)
        shots.append(agent.screenshot(path, clip))
        print(f"  [shot] {shots[-1]}  {agent.info()}")
    else:
        raise SystemExit(f"unknown step verb: {verb!r} (in {step!r})")


def main() -> int:
    ap = argparse.ArgumentParser(description="Reusable Arena Forge screenshot agent.")
    ap.add_argument("--url", default="http://localhost:8085/", help="running app URL")
    ap.add_argument("--scene", choices=sorted(SCENES), help="named capture preset")
    ap.add_argument("--do", nargs="+", metavar="STEP", help="ad-hoc verb:arg steps")
    ap.add_argument("--size", default="1280x720", help="viewport WxH")
    ap.add_argument("--backend", default="auto", choices=["auto", "webgl2", "canvas2d"])
    ap.add_argument("--out", default=str(OUT_DIR / "agent-capture.png"), help="output PNG (or dir)")
    ap.add_argument("--clip", default="none", choices=["none", "gallery"], help="clip shots to the gallery grid (chrome-free)")
    ap.add_argument("--headed", action="store_true", help="show the browser window")
    args = ap.parse_args()

    if not args.scene and not args.do:
        args.scene = "ironbound"
    steps = list(SCENES[args.scene]) if args.scene else list(args.do)
    w, h = (int(v) for v in args.size.lower().split("x"))
    url = args.url if args.backend == "auto" else f"{args.url.rstrip('/')}/?backend={args.backend}"
    out = pathlib.Path(args.out)

    shots: list[pathlib.Path] = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=not args.headed)
        page = browser.new_context(viewport={"width": w, "height": h}).new_page()
        errors: list[str] = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        agent = ArenaAgent(page)
        print(f"agent -> {url}  {w}x{h}  scene={args.scene or '(custom)'}")
        t0 = time.time()
        agent.boot(url)
        for step in steps:
            print(f"- {step}")
            run_step(agent, step, out, shots, args.clip)
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
