"""End-to-end test for horizontal scaleout (Phase D).

Stands up a real cluster — one DIRECTOR + two game NODES, three separate
processes — and proves:

  1. The director distributes rooms across BOTH nodes (matchmaker redirect): N
     tickets to POST /matchmake span both node URLs, and a shared room is pinned
     to one node.
  2. The browser path works through the cluster: the page loads from the director,
     POSTs /matchmake, is handed a node URL + room id, connects DIRECTLY to that
     node's game socket, and the authoritative loop runs there.

Verification tooling, outside the engine dependency graph. Run with
``make e2e-scaleout``; ``make netplay-cluster`` runs the same cluster for manual
play. Requires the .NET 10 SDK and a prebuilt wasm bundle (``make netplay-build``).
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
import urllib.request
from pathlib import Path

import pytest
from playwright.sync_api import Page

REPO_ROOT = Path(__file__).resolve().parent.parent
SERVER_DIR = REPO_ROOT / "examples" / "axiom-netplay-dotnet"
WEB_ROOT = REPO_ROOT / "dist"
DIRECTOR_PORT = int(os.environ.get("AXIOM_DIRECTOR_PORT", "8100"))
NODE_A_PORT = int(os.environ.get("AXIOM_NODE_A_PORT", "8101"))
NODE_B_PORT = int(os.environ.get("AXIOM_NODE_B_PORT", "8102"))
DIRECTOR_URL = f"http://localhost:{DIRECTOR_PORT}"

CONNECT_TIMEOUT_MS = 20_000


def _wait_nodes(director_url: str, want: int, timeout: float) -> bool:
    """Poll the director's readiness until at least `want` nodes have registered."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(f"{director_url}/readyz", timeout=1) as r:  # noqa: S310
                body = json.loads(r.read())
                if int(body.get("nodes", 0)) >= want:
                    return True
        except OSError:
            pass
        time.sleep(0.3)
    return False


def _matchmake(director_url: str) -> dict:
    req = urllib.request.Request(f"{director_url}/matchmake", method="POST")  # noqa: S310
    with urllib.request.urlopen(req, timeout=5) as r:  # noqa: S310
        return json.loads(r.read())


@pytest.fixture(scope="session")
def scaleout_director_url():
    """Build the worker + server, then launch a director and two game nodes."""
    if shutil.which("dotnet") is None:
        pytest.skip("the .NET 10 SDK (`dotnet`) is required for the scaleout e2e test")
    if not (WEB_ROOT / "pkg" / "axiom_netplay_browser_bg.wasm").exists():
        pytest.skip("prebuilt wasm bundle missing — run `make netplay-build` first")

    subprocess.run(["cargo", "build", "-p", "axiom-netplay-ffi", "--release"], cwd=REPO_ROOT, check=True)
    subprocess.run(["dotnet", "build", "-c", "Debug", str(SERVER_DIR)], cwd=REPO_ROOT, check=True)
    dll = next((SERVER_DIR / "bin" / "Debug").glob("net*/axiom-netplay-dotnet.dll"))
    dotnet = shutil.which("dotnet")
    ffi = str(REPO_ROOT / "target" / "release" / "axiom_netplay_ffi.dll")

    def launch(port: int, extra: dict[str, str]) -> subprocess.Popen:
        env = {
            **os.environ,
            "ASPNETCORE_URLS": f"http://localhost:{port}",
            "AXIOM_WEB_ROOT": str(WEB_ROOT),
            "AXIOM_FFI_LIB": ffi,
            **extra,
        }
        return subprocess.Popen([dotnet, str(dll)], cwd=REPO_ROOT, env=env)  # noqa: S603

    procs: list[subprocess.Popen] = []
    try:
        procs.append(launch(DIRECTOR_PORT, {"AXIOM_ROLE": "director"}))
        procs.append(launch(NODE_A_PORT, {
            "AXIOM_ROLE": "node",
            "AXIOM_DIRECTOR_URL": DIRECTOR_URL,
            "AXIOM_NODE_URL": f"ws://localhost:{NODE_A_PORT}/ws",
        }))
        procs.append(launch(NODE_B_PORT, {
            "AXIOM_ROLE": "node",
            "AXIOM_DIRECTOR_URL": DIRECTOR_URL,
            "AXIOM_NODE_URL": f"ws://localhost:{NODE_B_PORT}/ws",
        }))
        assert _wait_nodes(DIRECTOR_URL, want=2, timeout=60), "both game nodes never registered with the director"
        yield DIRECTOR_URL
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()


def test_director_distributes_rooms_across_both_nodes(scaleout_director_url: str) -> None:
    # Fire several tickets; the director must spread rooms over BOTH nodes.
    assignments = [_matchmake(scaleout_director_url) for _ in range(6)]
    node_urls = {a["nodeUrl"] for a in assignments}
    assert len(node_urls) == 2, f"rooms did not spread across both nodes: {node_urls}"

    # Every assignment carries a node URL and a room id.
    for a in assignments:
        assert a["nodeUrl"].startswith("ws://")
        assert a["roomId"].startswith("mm-")

    # A room is pinned to a single node: group room ids → node, none ambiguous.
    room_to_node: dict[str, str] = {}
    for a in assignments:
        room_to_node.setdefault(a["roomId"], a["nodeUrl"])
        assert room_to_node[a["roomId"]] == a["nodeUrl"], "a room id mapped to two different nodes"


def test_browser_is_redirected_to_a_node_and_plays(scaleout_director_url: str, page: Page) -> None:
    errors: list[str] = []
    page.on("pageerror", lambda e: errors.append(str(e)))

    # The page loads from the DIRECTOR, matchmakes, and connects to a NODE.
    page.goto(f"{scaleout_director_url}/netplay/", wait_until="load")
    page.wait_for_function(
        "() => window.__net && window.__net.status === 'connected' && window.__net.myPlayer >= 0",
        timeout=CONNECT_TIMEOUT_MS,
    )
    net = page.evaluate("() => window.__net")
    assert net["room"].startswith("mm-"), f"expected a matchmaker room, got {net['room']!r}"

    # The authoritative loop is running on the node it was redirected to.
    base_tick = net["serverTick"]
    page.wait_for_function(f"() => window.__net.serverTick > {base_tick}", timeout=CONNECT_TIMEOUT_MS)
    assert not errors, f"uncaught page error(s): {errors}"
