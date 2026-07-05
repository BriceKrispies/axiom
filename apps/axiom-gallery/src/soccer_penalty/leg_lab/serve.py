#!/usr/bin/env python3
"""Build + serve the STANDALONE TypeScript leg lab, with hot reload.

The leg lab is authored entirely in TypeScript on the `@axiom/game` SDK (there is
no Rust in this folder). This script is the folder-local runner: it compiles the
leg lab's `src/*.ts` to `dist/` with tsgo, makes sure the shared `@axiom/game` SDK
build and the `axiom-game-runtime` wasm exist (building them if missing), then
serves this folder over HTTP with the same route layout the repo dev server uses:

    /                     -> index.html
    /events               -> the hot-reload stream (Server-Sent Events)
    /dist/<x>             -> this folder's compiled TS
    /vendor/axiom-game/<x>-> packages/axiom-game/dist   (the SDK, as ES modules)
    /pkg/<x>              -> apps/axiom-game-runtime/web/pkg  (the shared wasm engine)

## Hot reload
It watches `src/*.ts`; on save it recompiles with tsgo and pushes a `reload` event
to the browser, which re-imports the game module and re-runs from tick 0 (see the
harness's EventSource). Because the lab is split across several modules, every
relative `/dist` import is cache-busted with the reload version at serve time, so
editing ANY module (not just `game.ts`) takes effect — the whole graph re-fetches.

Usage (from anywhere)::

    python apps/axiom-gallery/src/soccer_penalty/leg_lab/serve.py
    # then open http://localhost:8010/   (?backend=canvas2d forces software render)

Flags: --port N, --build-only, --no-build.

The compiled `dist/` and the SDK/wasm build outputs are git-ignored.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

# Print UTF-8 regardless of the console/redirect encoding (Windows defaults to
# cp1252, which can't encode the arrows/ellipses in the log lines below).
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

HERE = Path(__file__).resolve().parent
# leg_lab -> soccer_penalty -> src -> axiom-gallery -> apps -> repo root
REPO_ROOT = HERE.parents[4]
SDK_DIR = REPO_ROOT / "packages" / "axiom-game"
SDK_DIST = SDK_DIR / "dist"
PKG_DIR = REPO_ROOT / "apps" / "axiom-game-runtime" / "web" / "pkg"
DIST = HERE / "dist"
WIN = sys.platform == "win32"
TSGO = SDK_DIR / "node_modules" / ".bin" / ("tsgo.cmd" if WIN else "tsgo")

MIME = {
    ".html": "text/html",
    ".js": "text/javascript",
    ".mjs": "text/javascript",
    ".wasm": "application/wasm",
    ".json": "application/json",
    ".map": "application/json",
    ".css": "text/css",
    ".ts": "text/plain",
}

# A quoted RELATIVE `.js` module specifier (`"./x.js"`, `"../a/b.js"`) — an import
# in a compiled dist module. Absolute (`/dist`, `/vendor`, `/pkg`) and bare
# (`@axiom/game`) specifiers are left alone.
RELATIVE_IMPORT = re.compile(r'(["\'])(\.\.?/[^"\']+?\.js)\1')


class ReloadState:
    """The hot-reload version + a condition SSE handlers wait on for a bump."""

    def __init__(self) -> None:
        self.version = 1
        self.cond = threading.Condition()

    def bump(self) -> int:
        with self.cond:
            self.version += 1
            self.cond.notify_all()
            return self.version


RELOAD = ReloadState()


# ── building ────────────────────────────────────────────────────────────────

def run(cmd: list[str], cwd: Path) -> int:
    print(f"  $ {' '.join(cmd)}  (in {cwd})")
    return subprocess.run(cmd, cwd=str(cwd), shell=WIN).returncode


def ensure_sdk() -> None:
    """(Re)build the @axiom/game SDK to dist/ — always, since it is cheap (tsgo) and
    a dist/ that is stale against the current SDK/runtime source causes an ABI
    mismatch (e.g. the createMaterial arg count) that only shows up at runtime."""
    print("Building the @axiom/game SDK (npm run build)…")
    if run(["npm", "run", "build"], SDK_DIR) != 0:
        sys.exit("error: the @axiom/game SDK build failed.")


def ensure_wasm() -> None:
    """Build the shared axiom-game-runtime wasm (cargo + wasm-bindgen) if missing."""
    if (PKG_DIR / "axiom_game_runtime.js").is_file():
        return
    print("The axiom-game-runtime wasm is missing — building it…")
    if run(["cargo", "build", "-p", "axiom-game-runtime", "--target", "wasm32-unknown-unknown", "--release"], REPO_ROOT) != 0:
        sys.exit("error: the axiom-game-runtime wasm build failed.")
    wasm = REPO_ROOT / "target" / "wasm32-unknown-unknown" / "release" / "axiom_game_runtime.wasm"
    if run(["wasm-bindgen", "--target", "web", "--out-dir", str(PKG_DIR), str(wasm)], REPO_ROOT) != 0:
        sys.exit("error: wasm-bindgen failed.")


def compile_ts() -> int:
    """Compile this folder's src/*.ts to dist/*.js with tsgo. Returns the exit code
    (tsgo emits even on type errors, so a non-zero code is logged, not fatal, in
    the watch loop)."""
    if not TSGO.exists():
        sys.exit(f"error: tsgo not found at {TSGO} — run `npm install` in {SDK_DIR} first.")
    return run([str(TSGO), "-p", "tsconfig.json"], HERE)


# ── hot-reload watcher ──────────────────────────────────────────────────────

def watch_and_recompile() -> None:
    """Poll `src/*.ts` (+ index.html) and, on any change, recompile and broadcast a
    reload. Runs on a daemon thread for the life of the server."""
    def snapshot() -> dict[str, float]:
        files = list((HERE / "src").glob("*.ts")) + [HERE / "index.html"]
        return {str(p): p.stat().st_mtime for p in files if p.exists()}

    last = snapshot()
    while True:
        time.sleep(0.4)
        now = snapshot()
        if now == last:
            continue
        last = now
        print("change detected → recompiling…")
        if compile_ts() == 0:
            version = RELOAD.bump()
            print(f"  reloaded (v{version})\n")
        else:
            print("  not reloaded (compile failed)\n")


# ── serving ─────────────────────────────────────────────────────────────────

def resolve(clean_path: str) -> Path | None:
    """Map a URL path to a file on disk, routing /vendor + /pkg to the shared builds."""
    if clean_path == "/":
        return HERE / "index.html"
    if clean_path.startswith("/vendor/axiom-game/"):
        return SDK_DIST / clean_path[len("/vendor/axiom-game/"):]
    if clean_path.startswith("/pkg/"):
        return PKG_DIR / clean_path[len("/pkg/"):]
    return HERE / clean_path.lstrip("/")


class Handler(BaseHTTPRequestHandler):
    """Serve this folder + the shared SDK/wasm, with an SSE reload stream and
    version-stamped `/dist` imports so the whole module graph hot-reloads."""

    protocol_version = "HTTP/1.1"

    def log_message(self, *_args: object) -> None:  # noqa: D102 (quiet access log)
        pass

    def do_GET(self) -> None:  # noqa: N802 (stdlib override name)
        clean = self.path.split("?", 1)[0].split("#", 1)[0]
        if clean == "/events":
            self.stream_events()
            return
        self.serve_file(clean)

    def serve_file(self, clean: str) -> None:
        target = resolve(clean)
        if target is None or not target.is_file():
            self.send_error(404, f"not found: {clean}")
            return
        body = target.read_bytes()
        # Cache-bust the relative imports of a compiled dist module with the current
        # reload version, so re-importing `game.js?v=N` pulls a fresh whole graph.
        if target.suffix == ".js" and str(target).startswith(str(DIST)):
            version = RELOAD.version
            body = RELATIVE_IMPORT.sub(
                lambda m: f'{m.group(1)}{m.group(2)}?v={version}{m.group(1)}', body.decode("utf-8")
            ).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", MIME.get(target.suffix, "application/octet-stream"))
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def stream_events(self) -> None:
        """The Server-Sent Events reload stream: send `reload` when the watcher bumps
        the version, and a keepalive comment otherwise, until the client disconnects."""
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "keep-alive")
        self.end_headers()
        seen = RELOAD.version
        try:
            self.wfile.write(b"retry: 1000\n: connected\n\n")
            self.wfile.flush()
            while True:
                with RELOAD.cond:
                    RELOAD.cond.wait(timeout=15)
                    current = RELOAD.version
                if current != seen:
                    seen = current
                    self.wfile.write(f"event: reload\ndata: {current}\n\n".encode("utf-8"))
                else:
                    self.wfile.write(b": ping\n\n")
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError, OSError):
            pass  # client went away


def serve(port: int) -> None:
    threading.Thread(target=watch_and_recompile, daemon=True).start()
    server = ThreadingHTTPServer(("", port), Handler)
    server.daemon_threads = True
    print(f"\nServing the TypeScript leg lab at http://localhost:{port}/  (Ctrl-C to stop)")
    print(f"  hot reload ON — edit src/*.ts and save; watching {HERE / 'src'}")
    print(f"  force software render if needed: http://localhost:{port}/?backend=canvas2d")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


def main() -> int:
    parser = argparse.ArgumentParser(description="Build + serve the standalone TypeScript leg lab (with hot reload).")
    parser.add_argument("--port", type=int, default=8010, help="HTTP port (default 8010)")
    parser.add_argument("--build-only", action="store_true", help="build, do not serve")
    parser.add_argument("--no-build", action="store_true", help="serve without the initial rebuild (still hot-reloads)")
    args = parser.parse_args()

    if not args.no_build:
        ensure_sdk()
        ensure_wasm()
        if compile_ts() != 0:
            print("warning: initial TypeScript compile reported errors (serving anyway).")
    if args.build_only:
        return 0
    if not (DIST / "harness.js").is_file():
        sys.exit("error: no compiled dist/ — run without --no-build first.")
    serve(args.port)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
