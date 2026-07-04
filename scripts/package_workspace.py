#!/usr/bin/env python3
"""Assemble the axiom-workspace dev console into ``dist-workspace/`` and optionally serve it.

Repo tooling (alongside ``package_gallery.py`` and ``package_app.py``), NOT part of
the engine dependency graph. The workspace is a developer *console*: it hosts the
apps you're working on (the gallery's showcase apps AND the ``games/`` cartridges)
by loading the ONE gallery bundle and calling each app's entry.

It does four things:

1. **Compile the shell.** Runs ``tsgo -p apps/axiom-workspace/web/tsconfig.json`` to
   compile the vanilla-TS browser shell (``web/src/*.ts``) to ``web/dist/*.js``.
2. **Lay the static site** into ``dist-workspace/``: ``index.html`` (rewritten to load
   the compiled entry ``dist/main.js``), ``styles/``, ``games-manifest.json``, and the
   compiled ``dist/``.
3. **Build the ONE gallery bundle** into ``dist-workspace/gallery/`` via
   ``package_app.build_bundle`` — the same bundle the gallery showcases (its
   ``<demo>_start`` entries + the re-exported ``retro_fps_start`` + the ``compare_start``
   backend-compare entry) — and copies the gallery's static per-app pages there too,
   so the console can inline-boot the single-canvas apps and open the multi-screen
   ones (growth / zanzoban / harness).
4. With ``--serve``, serves ``dist-workspace/`` over HTTP, resolving extensionless
   ES-module imports (``./foo`` -> ``./foo.js``) so the shell runs from a plain
   static server.

Usage::

    python scripts/package_workspace.py            # build dist-workspace/ (fast, wasm-only)
    python scripts/package_workspace.py --serve     # build + serve at http://localhost:8123
    python scripts/package_workspace.py --serve --port 9001
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import package_app  # noqa: E402  (local repo tooling, not an installed package)

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKSPACE_WEB = REPO_ROOT / "apps" / "axiom-workspace" / "web"
GALLERY_DIR = REPO_ROOT / "apps" / "axiom-gallery"
GALLERY_WEB = GALLERY_DIR / "web"
DIST = REPO_ROOT / "dist-workspace"

# The tsgo binary (TypeScript 7 native), vendored under the @axiom/game package —
# the same compiler the dev server uses.
TSGO = (
    REPO_ROOT
    / "packages"
    / "axiom-game"
    / "node_modules"
    / ".bin"
    / ("tsgo.cmd" if os.name == "nt" else "tsgo")
)


def compile_shell() -> None:
    """Compile the workspace TS shell to web/dist/*.js with tsgo."""
    if not TSGO.exists():
        sys.exit(
            f"error: tsgo not found at {TSGO}\n"
            "  run `npm --prefix packages/axiom-game install` first (it vendors tsgo)."
        )
    tsconfig = WORKSPACE_WEB / "tsconfig.json"
    print(f"Compiling the workspace shell with tsgo ({tsconfig.relative_to(REPO_ROOT)})")
    # noEmitOnError is false in the tsconfig, so a type slip still emits JS; surface
    # the compiler's own exit status but don't abort the build on a type warning.
    subprocess.run([str(TSGO), "-p", str(tsconfig)], check=False)
    if not (WORKSPACE_WEB / "dist" / "main.js").is_file():
        sys.exit("error: tsgo did not emit web/dist/main.js — the shell failed to compile.")


def assemble(fast: bool) -> None:
    if DIST.exists():
        shutil.rmtree(DIST)
    DIST.mkdir(parents=True)

    # 1. index.html — load the compiled entry instead of the raw TS source.
    index = (WORKSPACE_WEB / "index.html").read_text(encoding="utf-8")
    index = index.replace('src="src/main.ts"', 'src="dist/main.js"')
    (DIST / "index.html").write_text(index, encoding="utf-8")

    # 2. The compiled shell, styles, and the console manifest.
    shutil.copytree(WORKSPACE_WEB / "dist", DIST / "dist")
    shutil.copytree(WORKSPACE_WEB / "styles", DIST / "styles")
    shutil.copy2(WORKSPACE_WEB / "games-manifest.json", DIST / "games-manifest.json")

    # 3. The gallery site + bundle under gallery/. Copy the static pages first (so the
    #    multi-screen "page" apps resolve), then build the shared bundle into the same
    #    dir (axiom-loader.js + wasm), exactly as package_gallery lays its own dist.
    gallery_out = DIST / "gallery"
    gallery_out.mkdir(parents=True, exist_ok=True)
    for item in GALLERY_WEB.iterdir():
        if item.name == "pkg":
            continue
        dest = gallery_out / item.name
        (shutil.copytree if item.is_dir() else shutil.copy2)(item, dest)
    print(f"Building the gallery bundle into {gallery_out}{' (fast)' if fast else ''}\n")
    target_dir = (REPO_ROOT / "target") if fast else (REPO_ROOT / "target" / "package-mvp")
    package_app.build_bundle(GALLERY_DIR, gallery_out, fast=fast, target_dir=target_dir)


class _ExtResolvingHandler(SimpleHTTPRequestHandler):
    """A static handler that resolves extensionless ES-module imports (`./foo` ->
    `./foo.js`) so the vanilla shell's bare relative imports load from disk, and
    serves `.wasm` with the correct MIME type."""

    extensions_map = {
        **SimpleHTTPRequestHandler.extensions_map,
        ".js": "text/javascript",
        ".mjs": "text/javascript",
        ".wasm": "application/wasm",
    }

    def translate_path(self, path: str) -> str:
        fs = super().translate_path(path)
        _, ext = os.path.splitext(fs)
        if ext == "" and os.path.isfile(fs + ".js"):
            return fs + ".js"
        return fs


def serve(port: int) -> None:
    handler = partial(_ExtResolvingHandler, directory=str(DIST))
    server = ThreadingHTTPServer(("127.0.0.1", port), handler)
    print(f"\nServing the workspace console at http://localhost:{port}  (Ctrl-C to stop)")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nstopped.")
        server.server_close()


def main() -> int:
    parser = argparse.ArgumentParser(description="Package the axiom-workspace console into dist-workspace/.")
    parser.add_argument("--serve", action="store_true", help="serve dist-workspace/ after building")
    parser.add_argument("--port", type=int, default=8123, help="port for --serve (default 8123)")
    parser.add_argument(
        "--full",
        action="store_true",
        help="build the wasm2js fallback too (slow, rebuilds std MVP); default is a fast wasm-only bundle",
    )
    args = parser.parse_args()

    if not (WORKSPACE_WEB / "index.html").is_file():
        sys.exit(f"error: {WORKSPACE_WEB} not found — the workspace crate is missing.")

    compile_shell()
    assemble(fast=not args.full)
    total_mb = sum(f.stat().st_size for f in DIST.rglob("*") if f.is_file()) / (1024 * 1024)
    print(f"\nassembled the workspace console into {DIST}  ({total_mb:.0f} MB total)")
    if args.serve:
        serve(args.port)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
