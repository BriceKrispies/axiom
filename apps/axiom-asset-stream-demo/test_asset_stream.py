"""Standalone Playwright test for the Axiom runtime asset-streaming demo.

It proves the demo's claims end-to-end against REAL network fetches:
  1. boot-fast — the boot status ("engine ready") appears quickly, before the
     stream finishes (the page is interactive before any asset loads);
  2. completion — `window.__assetDemo.done` becomes true and every asset is
     ready: `ready == total` and `failed == 0`;
  3. dependency ordering — at least one asset became ready AFTER its dependency,
     read from `window.__assetDemo.readyOrder` (the order ids became ready). The
     fixture's edges (hero 3 → atlas 1; material 4 → palette 2 + hero 3) must hold.

Prerequisite: the served `web/` dir must contain `manifest.bin` + the blobs
(`make asset-stream-pack`) AND the wasm bundle in `web/pkg/`
(`make asset-stream-build`). This script serves `web/` itself on an ephemeral
port, so just point a browser-free run at it:

    uv run --with playwright python apps/axiom-asset-stream-demo/test_asset_stream.py
    # (first time also: uv run --with playwright python -m playwright install chromium)

Exit code 0 = pass, non-zero = fail (with a printed reason).
"""

from __future__ import annotations

import functools
import http.server
import socket
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB_DIR = Path(__file__).resolve().parent / "web"

# The fixture's dependency edges (dependent id -> dependency ids). Each dependent
# MUST appear in readyOrder strictly after every one of its dependencies.
DEPENDENCY_EDGES: dict[int, list[int]] = {
    3: [1],     # hero mesh depends on the atlas texture
    4: [2, 3],  # material depends on the palette texture and the hero mesh
}

BOOT_TIMEOUT_MS = 4_000     # boot status must appear this fast (boot-fast proof)
DONE_TIMEOUT_MS = 30_000    # whole stream must complete within this


def _serve(directory: Path) -> tuple[http.server.ThreadingHTTPServer, int]:
    """Start a quiet background static file server on an ephemeral port."""
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(directory))
    handler.log_message = lambda *_args, **_kwargs: None  # type: ignore[assignment]
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, port


def _check_prerequisites() -> None:
    missing = []
    if not (WEB_DIR / "manifest.bin").exists():
        missing.append("web/manifest.bin (run: make asset-stream-pack)")
    if not (WEB_DIR / "pkg" / "axiom_asset_stream_demo.js").exists():
        missing.append("web/pkg/*.js (run: make asset-stream-build)")
    if missing:
        raise SystemExit("missing prerequisites:\n  - " + "\n  - ".join(missing))


def main() -> int:
    _check_prerequisites()
    server, port = _serve(WEB_DIR)
    base_url = f"http://127.0.0.1:{port}/"
    try:
        with sync_playwright() as pw:
            browser = pw.chromium.launch(headless=True)
            page = browser.new_page()
            errors: list[str] = []
            page.on("pageerror", lambda e: errors.append(str(e)))

            page.goto(base_url, wait_until="load")

            # 1. Boot-fast: the page says it's ready quickly.
            page.wait_for_function(
                "document.getElementById('boot')?.textContent?.includes('engine ready')",
                timeout=BOOT_TIMEOUT_MS,
            )
            print("PASS: boot status appeared (boot-fast)")

            # 2. The stream completes.
            page.wait_for_function(
                "window.__assetDemo && window.__assetDemo.done === true",
                timeout=DONE_TIMEOUT_MS,
            )
            state = page.evaluate("window.__assetDemo")
            assert not errors, f"uncaught page error(s): {errors}"
            assert state["total"] >= 1, f"no assets in manifest: {state}"
            assert state["ready"] == state["total"], f"not all ready: {state}"
            assert state["failed"] == 0, f"some loads failed: {state}"
            print(f"PASS: streaming complete — {state['ready']}/{state['total']} ready, 0 failed")

            # 3. Dependency ordering: every dependent landed after its deps.
            ready_order: list[int] = [int(x) for x in state["readyOrder"]]
            position = {asset_id: idx for idx, asset_id in enumerate(ready_order)}
            proven = 0
            for dependent, deps in DEPENDENCY_EDGES.items():
                assert dependent in position, f"dependent {dependent} never became ready: {ready_order}"
                for dep in deps:
                    assert dep in position, f"dependency {dep} never became ready: {ready_order}"
                    assert position[dep] < position[dependent], (
                        f"asset {dependent} became ready before its dependency {dep}; "
                        f"readyOrder={ready_order}"
                    )
                    proven += 1
            assert proven >= 1, "no dependency edges were checked"
            print(f"PASS: dependency ordering held for {proven} edge(s); readyOrder={ready_order}")

            browser.close()
        print("\nALL CHECKS PASSED")
        return 0
    finally:
        server.shutdown()


if __name__ == "__main__":
    sys.exit(main())
