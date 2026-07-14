"""End-to-end test for the server-authoritative multiplayer demo (Phases 7-8).

Unlike the gallery smoke suite (which *skips* netplay because it needs a live
authoritative server), this stands up the real stack and proves the
server-authoritative loop end-to-end against a browser:

  * the in-process Axiom simulation worker (the `axiom-netplay-ffi` cdylib),
  * embedded in the .NET 10 authoritative server (`examples/axiom-netplay-dotnet`),
  * serving the prebuilt wasm renderer + `@axiom/client` SDK page,
  * driven by a real Chromium via Playwright.

What it asserts (the Phase 7 + 8 contract):
  1. the browser connects and the server is **ticking** (serverTick advances) —
     authoritative simulation runs server-side, not in the browser;
  2. holding a key moves the player's **authoritative** position (the browser
     sends only intents; `acked` climbs as the server accepts them);
  3. the server is the wall: the authoritative position is **clamped to the
     field bound** (±3.5) no matter how long the key is held — a client cannot
     walk out of the arena;
  4. client prediction **reconciles** to authority (predicted ≈ authoritative,
     no permanent drift past the wall);
  5. the page renders without a fatal error and the canvas actually paints.

This is verification tooling, outside the engine dependency graph. Run with
`make e2e-netplay` (or `pytest e2e/test_netplay.py`). Requires the .NET 10 SDK
and a prebuilt wasm bundle (`make netplay-build`).
"""

from __future__ import annotations

import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import pytest
from playwright.sync_api import Page

REPO_ROOT = Path(__file__).resolve().parent.parent
SERVER_DIR = REPO_ROOT / "examples" / "axiom-netplay-dotnet"
WEB_ROOT = REPO_ROOT / "dist"
PORT = int(os.environ.get("AXIOM_NETPLAY_E2E_PORT", "8090"))
BASE_URL = f"http://localhost:{PORT}"

# The authoritative field half-extent the worker enforces (ruleset::FIELD_BOUND)
# and the browser predicts (netplay-client.js LIMIT). They MUST agree, or
# prediction would drift past the wall — which is exactly what this test guards.
FIELD_BOUND = 3.5

CONNECT_TIMEOUT_MS = 20_000
HOLD_MS = 3_000


def _port_open(port: int) -> bool:
    with socket.socket() as s:
        s.settimeout(0.3)
        return s.connect_ex(("127.0.0.1", port)) == 0


def _wait_ready(url: str, timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=1) as r:  # noqa: S310 (localhost dev server)
                if r.status == 200:
                    return True
        except OSError:
            pass
        time.sleep(0.3)
    return False


@pytest.fixture(scope="session")
def netplay_base_url():
    """Build the native worker + the .NET server, serve the prebuilt client, and
    yield its URL. Set AXIOM_NETPLAY_E2E_REUSE=1 to reuse an already-running
    server on the port (skips the build+serve) for quick local re-runs."""
    if os.environ.get("AXIOM_NETPLAY_E2E_REUSE") and _port_open(PORT):
        yield BASE_URL
        return

    if shutil.which("dotnet") is None:
        pytest.skip("the .NET 10 SDK (`dotnet`) is required for the netplay e2e test")
    if not (WEB_ROOT / "netplay" / "pkg" / "axiom_netplay_bg.wasm").exists():
        pytest.skip("prebuilt wasm bundle missing — run `make netplay-build` first")

    # The native cdylib the server P/Invokes. Build it fresh so the test exercises
    # the current worker (incl. the authoritative field clamp).
    subprocess.run(
        ["cargo", "build", "-p", "axiom-netplay-ffi", "--release"],
        cwd=REPO_ROOT,
        check=True,
    )
    # Build the server, then run the built DLL directly (not `dotnet run`, whose
    # wrapper would spawn the real server as an orphanable child — terminate()
    # must reach the actual process, per e2e/conftest.py's http.server note).
    subprocess.run(
        ["dotnet", "build", "-c", "Debug", str(SERVER_DIR)],
        cwd=REPO_ROOT,
        check=True,
    )
    dll = next((SERVER_DIR / "bin" / "Debug").glob("net*/axiom-netplay-dotnet.dll"))
    env = {
        **os.environ,
        "ASPNETCORE_URLS": BASE_URL,
        "AXIOM_WEB_ROOT": str(WEB_ROOT),
        "AXIOM_FFI_LIB": str(REPO_ROOT / "target" / "release" / "axiom_netplay_ffi.dll"),
    }
    server = subprocess.Popen([shutil.which("dotnet"), str(dll)], cwd=REPO_ROOT, env=env)  # noqa: S603
    try:
        assert _wait_ready(f"{BASE_URL}/readyz", 60), f"netplay server never became ready on {BASE_URL}"
        yield BASE_URL
    finally:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()


def test_server_authoritative_movement_clamps_and_reconciles(netplay_base_url: str, page: Page) -> None:
    messages: list[str] = []
    errors: list[str] = []
    page.on("console", lambda m: messages.append(m.text))
    page.on("pageerror", lambda e: errors.append(str(e)))

    page.goto(f"{netplay_base_url}/netplay/", wait_until="load")

    # 1. The browser connects to the authoritative server.
    page.wait_for_function(
        "() => window.__net && window.__net.status === 'connected' && window.__net.myPlayer >= 0",
        timeout=CONNECT_TIMEOUT_MS,
    )
    before = page.evaluate("() => window.__net")
    mine = before["myPlayer"]
    assert mine in (0, 1), f"unexpected player slot {mine}"

    # The server is TICKING — authoritative simulation advances server-side.
    base_tick = before["serverTick"]
    page.wait_for_function(
        f"() => window.__net.serverTick > {base_tick}",
        timeout=CONNECT_TIMEOUT_MS,
    )

    my_x_before = before["authoritative"][mine * 2]

    # 2. Hold ArrowRight well past the wall: the browser only sends intents.
    page.evaluate("() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }))")
    page.wait_for_timeout(HOLD_MS)
    page.evaluate("() => window.dispatchEvent(new KeyboardEvent('keyup', { key: 'ArrowRight' }))")
    page.wait_for_timeout(1_000)  # let the last snapshots arrive and reconcile

    after = page.evaluate("() => window.__net")
    my_x_after = after["authoritative"][mine * 2]
    other = 1 - mine

    # The AUTHORITATIVE position moved right under server control...
    assert my_x_after > my_x_before + 1.0, (
        f"authoritative x did not advance: {my_x_before} -> {my_x_after}"
    )
    # ...because the server ACCEPTED our intents (we sent intents, never state).
    assert after["acked"] > before["acked"], "server accepted no intents"

    # 3. The server is the wall: authoritative position is clamped to the field
    #    bound no matter how long the key was held — it never escapes the arena.
    assert my_x_after <= FIELD_BOUND + 1e-3, f"authoritative x={my_x_after} escaped the field bound"
    assert abs(my_x_after - FIELD_BOUND) < 0.05, f"authoritative x={my_x_after} did not reach the wall"

    # 4. Prediction RECONCILES to authority — no permanent drift past the wall.
    assert abs(after["predicted"]["x"] - my_x_after) < 0.2, (
        f"prediction diverged from authority: predicted={after['predicted']['x']} auth={my_x_after}"
    )
    # The untouched other player stayed at its authoritative spawn.
    assert abs(after["authoritative"][other * 2] - before["authoritative"][other * 2]) < 0.2

    # 5. No fatal startup/render failure, and the canvas actually painted.
    assert not errors, f"uncaught page error(s): {errors}"
    fatal = [t for t in messages if "axiom: FATAL" in t or "Startup failed:" in t]
    assert not fatal, f"fatal console error(s): {fatal}"

    canvas = page.locator("#axiom-netplay-canvas")
    box = canvas.bounding_box()
    assert box and box["width"] > 0 and box["height"] > 0, "netplay canvas missing/zero-size"
