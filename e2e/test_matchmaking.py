"""End-to-end test for HTTP matchmaking (Phase B).

Stands up the all-in-one .NET server and proves two things:

  1. The matchmaker endpoint itself — POST /matchmake hands out rooms with free
     slots, filling a 2-player room before opening a new one (quickplay).
  2. The browser uses it — the page POSTs /matchmake on load, joins the
     matchmaker-assigned room (id like ``mm-3``, not the hardcoded ``lobby``),
     and the authoritative loop runs (connected, server ticking).

Verification tooling, outside the engine dependency graph. Run with
``make e2e-matchmaking``. Requires the .NET 10 SDK and a prebuilt wasm bundle
(``make netplay-build``).
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import time
import urllib.request
from pathlib import Path

import pytest
from playwright.sync_api import Page

REPO_ROOT = Path(__file__).resolve().parent.parent
SERVER_DIR = REPO_ROOT / "examples" / "axiom-netplay-dotnet"
WEB_ROOT = REPO_ROOT / "dist"
PORT = int(os.environ.get("AXIOM_MATCHMAKING_E2E_PORT", "8092"))
BASE_URL = f"http://localhost:{PORT}"

CONNECT_TIMEOUT_MS = 20_000


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


def _matchmake(base_url: str) -> str:
    req = urllib.request.Request(f"{base_url}/matchmake", method="POST")  # noqa: S310
    with urllib.request.urlopen(req, timeout=5) as r:  # noqa: S310
        return json.loads(r.read())["roomId"]


@pytest.fixture(scope="session")
def matchmaking_base_url():
    """Build the worker + the all-in-one .NET server and serve the client."""
    if os.environ.get("AXIOM_MATCHMAKING_E2E_REUSE") and _port_open(PORT):
        yield BASE_URL
        return
    if shutil.which("dotnet") is None:
        pytest.skip("the .NET 10 SDK (`dotnet`) is required for the matchmaking e2e test")
    if not (WEB_ROOT / "axiom_gallery_bg.wasm").exists():
        pytest.skip("prebuilt wasm bundle missing — run `make netplay-build` first")

    subprocess.run(["cargo", "build", "-p", "axiom-netplay-ffi", "--release"], cwd=REPO_ROOT, check=True)
    subprocess.run(["dotnet", "build", "-c", "Debug", str(SERVER_DIR)], cwd=REPO_ROOT, check=True)
    dll = next((SERVER_DIR / "bin" / "Debug").glob("net*/axiom-netplay-dotnet.dll"))
    env = {
        **os.environ,
        "ASPNETCORE_URLS": BASE_URL,
        "AXIOM_WEB_ROOT": str(WEB_ROOT),
        "AXIOM_FFI_LIB": str(REPO_ROOT / "target" / "release" / "axiom_netplay_ffi.dll"),
    }
    server = subprocess.Popen([shutil.which("dotnet"), str(dll)], cwd=REPO_ROOT, env=env)  # noqa: S603
    try:
        assert _wait_ready(f"{BASE_URL}/readyz", 60), f"server never became ready on {BASE_URL}"
        yield BASE_URL
    finally:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()


def test_matchmake_endpoint_fills_a_room_then_opens_a_new_one(matchmaking_base_url: str) -> None:
    # Three tickets into 2-player rooms: the first two share a room, the third
    # opens a new one. Proves the HTTP matchmaker directly.
    a = _matchmake(matchmaking_base_url)
    b = _matchmake(matchmaking_base_url)
    c = _matchmake(matchmaking_base_url)
    assert a == b, f"first two tickets should share a room, got {a!r} and {b!r}"
    assert c != a, f"third ticket should open a new room, got {c!r} again"


def test_browser_joins_a_matchmade_room_and_plays(matchmaking_base_url: str, page: Page) -> None:
    errors: list[str] = []
    page.on("pageerror", lambda e: errors.append(str(e)))

    page.goto(f"{matchmaking_base_url}/netplay/", wait_until="load")
    page.wait_for_function(
        "() => window.__net && window.__net.status === 'connected' && window.__net.myPlayer >= 0",
        timeout=CONNECT_TIMEOUT_MS,
    )
    net = page.evaluate("() => window.__net")

    # The page joined a MATCHMAKER-assigned room, not the hardcoded 'lobby'.
    assert isinstance(net["room"], str) and net["room"].startswith("mm-"), (
        f"expected a matchmaker room id (mm-*), got {net['room']!r}"
    )
    # And the authoritative loop is running in that room.
    base_tick = net["serverTick"]
    page.wait_for_function(f"() => window.__net.serverTick > {base_tick}", timeout=CONNECT_TIMEOUT_MS)
    assert not errors, f"uncaught page error(s): {errors}"
