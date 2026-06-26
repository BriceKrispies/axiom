"""Gallery smoke test: every non-multiplayer demo loads — twice.

For each demo, and for each backend in {default, ?backend=canvas2d}, this:
  1. navigates to the demo,
  2. waits for the demo's positive ready signal (a stall times out → fail),
  3. asserts no uncaught page error and no FATAL console error
     (the engine logs `axiom: FATAL — no render backend …` on a hard failure;
      benign WebGPU warnings and retro_fps's hot-reload /event 404 are not fatal),
  4. screenshots the canvas and asserts it actually painted (not a flat color).

Skips netplay (the only multiplayer demo). Run with `make e2e`.
"""

from __future__ import annotations

import io
from pathlib import Path

import pytest
from PIL import Image
from playwright.sync_api import Page, expect

from demos import DEMOS, with_backend

SHOT_DIR = Path(__file__).resolve().parent / "screenshots"
ACTIVE = [d for d in DEMOS if not d.get("skip")]
READY_TIMEOUT_MS = 25_000


def _wait_for_log(page: Page, messages: list[str], needle: str, timeout_ms: int = READY_TIMEOUT_MS) -> None:
    """Poll captured console messages for one containing `needle`, else fail."""
    elapsed = 0
    while elapsed < timeout_ms:
        if any(needle in text for text in messages):
            return
        page.wait_for_timeout(200)
        elapsed += 200
    raise AssertionError(f"timed out waiting for console log containing {needle!r}; last logs: {messages[-20:]}")


@pytest.mark.parametrize("backend", ["default", "canvas2d"])
@pytest.mark.parametrize("demo", ACTIVE, ids=[d["id"] for d in ACTIVE])
def test_demo_loads(demo: dict, backend: str, gallery_base_url: str, page: Page) -> None:
    messages: list[str] = []
    errors: list[str] = []
    page.on("console", lambda m: messages.append(m.text))
    page.on("pageerror", lambda e: errors.append(str(e)))

    page.goto(f"{gallery_base_url}/{with_backend(demo['path'], backend)}", wait_until="load")

    # 1. Positive ready signal — the proof the game actually started.
    kind = demo["kind"]
    if kind == "windowing3d":
        _wait_for_log(page, messages, "axiom: render backend = ")
        if backend == "canvas2d":
            _wait_for_log(page, messages, "render backend = Canvas2d")
    elif kind == "canvas2d_app":
        _wait_for_log(page, messages, demo["ready_log"])
    elif kind == "growth":
        expect(page.locator("#status")).to_contain_text("Ready", timeout=READY_TIMEOUT_MS)
    elif kind == "harness":
        expect(page.locator("#boot")).to_contain_text("Overlay mounted", timeout=READY_TIMEOUT_MS)

    page.wait_for_timeout(600)  # let a few frames present before sampling

    # 2. No hard failure. Benign noise (WebGPU "Device failed at creation" warnings,
    #    retro_fps's hot-reload /event 404) is not fatal and is intentionally ignored.
    assert not errors, f"{demo['id']} [{backend}] uncaught page error(s): {errors}"
    fatal = [t for t in messages if "axiom: FATAL" in t or "Startup failed:" in t]
    assert not fatal, f"{demo['id']} [{backend}] fatal console error(s): {fatal}"

    # 3. Render proof: the canvas exists, has size, and is not a single flat color.
    if demo.get("check_canvas", True):
        SHOT_DIR.mkdir(exist_ok=True)
        canvas = page.locator(demo["canvas"]).first
        box = canvas.bounding_box()
        assert box and box["width"] > 0 and box["height"] > 0, f"{demo['id']} [{backend}] canvas missing/zero-size"
        png = canvas.screenshot(path=str(SHOT_DIR / f"{demo['id']}-{backend}.png"))
        img = Image.open(io.BytesIO(png)).convert("RGB")
        colors = img.getcolors(maxcolors=1 << 20)
        assert colors is None or len(colors) > 1, (
            f"{demo['id']} [{backend}] canvas is a single flat color — it did not render"
        )
