"""Standalone Playwright test for the Web Worker POOL streaming variant.

Loads `workers.html`, where a pool of background Web Workers streams the assets
off the main thread, and proves the worker-pool claims end-to-end against REAL
network fetches + real off-main-thread CPU decode:

  1. boot-fast — the boot status appears quickly, before the stream finishes;
  2. completion — `window.__assetDemo.done` becomes true with `ready == total`
     and `failed == 0`;
  3. genuine pool parallelism — `peakBusy >= 2`: at least two workers were
     processing assets at the same time (true CPU parallelism, not just async);
  4. dependency ordering — every dependent became ready AFTER all its deps, read
     from `readyOrder` (workers finish out of order, but the scheduler still gates
     dependents — proving the determinism boundary holds across the pool).

Prerequisite: the served `web/` dir must contain `manifest.bin` + blobs
(`make asset-stream-pack`), the wasm bundle in `web/pkg/` (`make asset-stream-build`),
and `web/worker.js` + `web/workers.html` (committed with the app).

    uv run --with playwright python apps/axiom-asset-stream-demo/test_asset_stream_workers.py
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

# The default pool size the page uses (matches DEFAULT_WORKERS in pool.rs).
EXPECTED_WORKERS = 3

# The fixture's dependency edges (dependent id -> dependency ids). Each dependent
# MUST appear in readyOrder strictly after every one of its dependencies.
DEPENDENCY_EDGES: dict[int, list[int]] = {
    3: [1],      # hero mesh depends on the atlas texture
    4: [2, 3],   # material depends on the palette texture and the hero mesh
    9: [6, 7],   # shader/material depends on the level mesh and props texture
    10: [8],     # prefab depends on the ambient sound
}

BOOT_TIMEOUT_MS = 4_000
DONE_TIMEOUT_MS = 30_000


def _serve(directory: Path) -> tuple[http.server.ThreadingHTTPServer, int]:
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
    if not (WEB_DIR / "worker.js").exists():
        missing.append("web/worker.js (committed with the app)")
    if not (WEB_DIR / "workers.html").exists():
        missing.append("web/workers.html (committed with the app)")
    if missing:
        raise SystemExit("missing prerequisites:\n  - " + "\n  - ".join(missing))


def main() -> int:
    _check_prerequisites()
    server, port = _serve(WEB_DIR)
    base_url = f"http://127.0.0.1:{port}/workers.html"
    try:
        with sync_playwright() as pw:
            browser = pw.chromium.launch(headless=True)
            page = browser.new_page()
            errors: list[str] = []
            page.on("pageerror", lambda e: errors.append(str(e)))

            page.goto(base_url, wait_until="load")

            # 1. Boot-fast.
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

            # 3. Genuine pool parallelism.
            assert state["workerCount"] == EXPECTED_WORKERS, (
                f"expected {EXPECTED_WORKERS} workers, got {state.get('workerCount')}"
            )
            assert state["peakBusy"] >= 2, (
                f"no parallelism observed: peakBusy={state.get('peakBusy')} "
                "(expected >= 2 workers busy at once)"
            )
            print(
                f"PASS: worker pool ran in parallel — {state['workerCount']} workers, "
                f"peak {state['peakBusy']} busy at once"
            )

            # 4. Dependency ordering held across out-of-order worker completions.
            ready_order: list[int] = [int(x) for x in state["readyOrder"]]
            position = {asset_id: idx for idx, asset_id in enumerate(ready_order)}
            proven = 0
            for dependent, deps in DEPENDENCY_EDGES.items():
                assert dependent in position, f"dependent {dependent} never ready: {ready_order}"
                for dep in deps:
                    assert dep in position, f"dependency {dep} never ready: {ready_order}"
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
