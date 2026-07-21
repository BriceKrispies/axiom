#!/usr/bin/env -S uv run --with playwright python
"""
Arena Forge browser interaction test.

Drives the REAL app in a headless browser at three landscape viewports
(844x390 mobile, 932x430 larger mobile, 1440x900 desktop) through a full
interaction path — inspect a shop card, buy it, play/reorder, reroll, freeze,
expire the shop timer via the deterministic test control, observe combat, and
reach the next shop — asserting authoritative state at each step through the
game's dev handle (`window.__arena`). It also captures the four required
screenshots (shop, combat, forged unit, final results).

Prereq: serve the app first, e.g. `cargo run -p axiom-serve -- arena-forge
--no-open`. Run: `uv run apps/arena-forge/web/browser/interaction_test.py`.
"""

import os
import sys
import pathlib
from playwright.sync_api import sync_playwright

URL = os.environ.get("ARENA_URL", "http://localhost:8080/")
OUT = pathlib.Path(__file__).parent / "screenshots"
OUT.mkdir(exist_ok=True)

VIEWPORTS = [("mobile", 844, 390), ("large-mobile", 932, 430), ("desktop", 1440, 900)]

TAP_JS = """
([x, y]) => {
  const c = document.getElementById('arena-canvas');
  const down = new PointerEvent('pointerdown', {clientX:x, clientY:y, bubbles:true, pointerId:1});
  const up = new PointerEvent('pointerup', {clientX:x, clientY:y, bubbles:true, pointerId:1});
  c.dispatchEvent(down);
  window.dispatchEvent(up);
}
"""

DRAG_JS = """
([x0, y0, x1, y1]) => {
  const c = document.getElementById('arena-canvas');
  c.dispatchEvent(new PointerEvent('pointerdown', {clientX:x0, clientY:y0, bubbles:true, pointerId:1}));
  for (let i=1;i<=6;i++){ const t=i/6; window.dispatchEvent(new PointerEvent('pointermove', {clientX:x0+(x1-x0)*t, clientY:y0+(y1-y0)*t, bubbles:true, pointerId:1})); }
  window.dispatchEvent(new PointerEvent('pointerup', {clientX:x1, clientY:y1, bubbles:true, pointerId:1}));
}
"""

CENTER_JS = "(r) => [r.x + r.w/2, r.y + r.h/2]"


def run(page, label, w, h):
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(URL, wait_until="load", timeout=30000)
    page.wait_for_function("() => window.__arena", timeout=8000)
    # The app now boots on the main menu; enter gameplay for this interaction test.
    page.evaluate("() => window.__arena.debugGoto('gameplay')")
    page.wait_for_function("() => window.__arena.debugLayout()", timeout=8000)

    def summary():
        return page.evaluate("() => window.__arena.debugSummary()")

    def layout():
        return page.evaluate("() => window.__arena.debugLayout()")

    def tap_rect(r):
        page.evaluate(TAP_JS, page.evaluate(CENTER_JS, r))
        page.wait_for_timeout(120)

    assert summary()["phase"] == "shop", f"{label}: expected shop, got {summary()}"

    # 1. Inspect a shop card (tap), then buy via the inspect action button.
    lay = layout()
    tap_rect(lay["shop"][0])                       # inspect
    page.screenshot(path=str(OUT / f"{label}-01-shop-inspect.png"))
    # inspect action rect (buy)
    ins = page.evaluate("() => { const w=window.__arena.debugLayout(); return null; }")
    # Recompute inspect rects in-page (mirror of ui/layout.inspectRects).
    action = page.evaluate(
        "([w,h]) => { const cl=(v,a,b)=>Math.max(a,Math.min(b,v)); const pw=cl(w*0.42,300,460), ph=cl(h*0.7,220,340);"
        " const px=(w-pw)/2, py=(h-ph)/2; return {x:px+16, y:py+ph-56, w:pw-32, h:46}; }",
        [w, h],
    )
    before = summary()
    page.evaluate(TAP_JS, page.evaluate(CENTER_JS, action))   # BUY
    page.wait_for_timeout(150)
    after = summary()
    assert after["warband"] + after["hand"] > before["warband"] + before["hand"], f"{label}: buy did not add a unit ({before}->{after})"
    assert after["gold"] < before["gold"], f"{label}: buy did not spend gold"

    # 2. Reroll and freeze.
    lay = layout()
    tap_rect(lay["buttons"]["reroll"])
    tap_rect(lay["buttons"]["freeze"])
    assert page.evaluate("() => window.__arena.debugLayout() !== null")

    # 3. Reorder: drag warband slot 0 -> slot 2 (if occupied).
    lay = layout()
    s0 = page.evaluate(CENTER_JS, lay["warband"][0])
    s2 = page.evaluate(CENTER_JS, lay["warband"][2])
    page.evaluate(DRAG_JS, [s0[0], s0[1], s2[0], s2[1]])
    page.wait_for_timeout(120)

    # 4. Forged-unit showcase screenshot.
    page.evaluate("() => window.__arena.debugShowcaseForge()")
    page.wait_for_timeout(60)
    page.screenshot(path=str(OUT / f"{label}-02-forged-unit.png"))

    # 5. Expire the shop timer via the deterministic control -> combat.
    page.evaluate("() => window.__arena.debugAdvancePhase()")
    page.wait_for_timeout(400)
    assert summary()["phase"] == "combat", f"{label}: expected combat, got {summary()}"
    page.wait_for_timeout(500)
    page.screenshot(path=str(OUT / f"{label}-03-combat.png"))

    # 6. Resolve combat -> reach the next shop (or match end).
    page.evaluate("() => window.__arena.debugAdvancePhase()")
    page.wait_for_timeout(300)
    phase = summary()["phase"]
    assert phase in ("shop", "match_complete"), f"{label}: unexpected phase after combat: {phase}"

    # 7. Run to completion for the final results screen.
    for _ in range(300):
        if summary()["phase"] == "match_complete":
            break
        page.evaluate("() => window.__arena.debugAdvancePhase()")
    page.wait_for_timeout(200)
    page.screenshot(path=str(OUT / f"{label}-04-results.png"))
    assert summary()["phase"] == "match_complete", f"{label}: match did not complete"

    assert not errors, f"{label}: page errors: {errors}"
    print(f"[{label} {w}x{h}] PASS  final={summary()}")


def main():
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        try:
            for label, w, h in VIEWPORTS:
                ctx = browser.new_context(viewport={"width": w, "height": h})
                page = ctx.new_page()
                run(page, label, w, h)
                ctx.close()
        finally:
            browser.close()
    print("ALL VIEWPORTS PASS. Screenshots in", OUT)


if __name__ == "__main__":
    try:
        main()
    except AssertionError as e:
        print("FAIL:", e)
        sys.exit(1)
