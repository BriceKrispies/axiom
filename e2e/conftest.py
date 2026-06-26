"""Pytest fixtures for the gallery browser smoke suite.

Repo tooling, NOT part of the engine dependency graph (same status as scripts/ and
the playwright controller). Builds + serves the FAST gallery (wasm-only, seconds)
once per session and points the tests at it; configures Chromium with the same GPU
flags scripts/playwright_controller.py uses so the WebGPU→WebGL2 path is exercised.
"""

from __future__ import annotations

import os
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
PORT = int(os.environ.get("AXIOM_E2E_PORT", "8000"))
BASE_URL = f"http://localhost:{PORT}"


def _port_open(port: int) -> bool:
    with socket.socket() as s:
        s.settimeout(0.3)
        return s.connect_ex(("127.0.0.1", port)) == 0


def _wait_http(url: str, timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            urllib.request.urlopen(url, timeout=1)  # noqa: S310 (localhost dev server)
            return True
        except OSError:
            time.sleep(0.3)
    return False


@pytest.fixture(scope="session")
def gallery_base_url():
    """Build the fast gallery into dist/ and serve it for the session.

    Set AXIOM_E2E_REUSE=1 to reuse an already-running gallery on the port (skips the
    rebuild+serve) for quick local re-runs.
    """
    if os.environ.get("AXIOM_E2E_REUSE") and _port_open(PORT):
        yield BASE_URL
        return

    # Run directly with this interpreter (no `uv run` wrapper): the build is plain
    # stdlib + subprocess, and crucially the http.server Popen must BE the real
    # process so terminate() reaches it on teardown (a `uv run python` wrapper would
    # be killed while the child server orphaned, leaving :8000 held on Windows).
    subprocess.run([sys.executable, "scripts/package_gallery.py", "--fast"], cwd=REPO_ROOT, check=True)
    server = subprocess.Popen(  # noqa: S603
        [sys.executable, "-m", "http.server", str(PORT), "--directory", "dist"], cwd=REPO_ROOT
    )
    try:
        assert _wait_http(BASE_URL, 30), f"gallery server never came up on {BASE_URL}"
        yield BASE_URL
    finally:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()


@pytest.fixture(scope="session")
def browser_type_launch_args(browser_type_launch_args, browser_name):
    """Expose a real GPU adapter (matches scripts/playwright_controller.py). Under
    headless Chromium WebGPU device creation fails, so 3D demos fall back to WebGL2 —
    which is exactly the path the default (non-canvas2d) pass asserts loads.

    The GPU flags are Chromium-only command-line switches; Firefox and WebKit reject
    unknown switches at launch, so they are applied solely to the chromium browser."""
    chromium_gpu_args = [
        "--enable-unsafe-webgpu",
        "--enable-features=Vulkan",
        "--use-gl=angle",
    ]
    extra_args = chromium_gpu_args if browser_name == "chromium" else []
    return {
        **browser_type_launch_args,
        "headless": os.environ.get("AXIOM_PW_HEADLESS", "1") != "0",
        "args": [
            *browser_type_launch_args.get("args", []),
            *extra_args,
        ],
    }
