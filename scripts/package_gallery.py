#!/usr/bin/env python3
"""Assemble the SINGLE-bundle demo gallery into ``dist/``.

Repo tooling (alongside ``package_app.py`` and the Makefile), NOT part of the engine
dependency graph. Every browser demo is now merged into ONE crate
(``apps/axiom-gallery``), so the gallery is ONE wasm bundle, not nine: this lays the
static site (the shell + every demo's page) over a single capability-detecting loader.

It does two things:

1. **Copy the static site** ``apps/axiom-gallery/web/`` → ``dist/`` verbatim — the
   shell (``index.html`` landing grid, ``gallery.js``, ``demo.html``, ``keypad.js``,
   ``styles.css``), every demo's page under ``dist/<demo>/``, and the netplay client
   + vendored ``@axiom/client`` SDK. (The Makefile's ``gallery-build`` vendors that SDK
   into ``web/netplay/vendor/`` first, so it rides along in the copy.)

2. **Build the ONE shared bundle** at the ``dist/`` root via ``package_app.build_bundle``:
   a size-optimized wasm fast-path AND (unless ``--fast``) a Binaryen wasm2js fallback
   for browsers with no WebAssembly, behind ``dist/axiom-loader.js``. Every page —
   shell and self-hosted — imports that one loader and calls its demo's ``<demo>_start``.

The full build rebuilds std MVP (so the wasm2js fallback is possible — see
``package_app.py``), so the first ``make gallery`` is slow; re-runs are incremental.
``--fast`` skips the fallback for tight iteration.
"""

from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import package_app  # noqa: E402  (local repo tooling, not an installed package)

REPO_ROOT = Path(__file__).resolve().parent.parent
GALLERY_DIR = REPO_ROOT / "apps" / "axiom-gallery"
WEB_DIR = GALLERY_DIR / "web"


def _dir_size_mb(path: Path) -> float:
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file()) / (1024 * 1024)


def main() -> int:
    parser = argparse.ArgumentParser(description="Package the single-bundle demo gallery into dist/.")
    parser.add_argument(
        "--fast",
        action="store_true",
        help="quick wasm-only bundle (normal incremental build, no wasm2js fallback) for iteration",
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="debug wasm build (keeps debug_assertions on for the Canvas2D deep profiler; "
        "used by the render benchmark). Implies --fast (no wasm2js fallback).",
    )
    args = parser.parse_args()
    if args.debug:
        args.fast = True

    if not (GALLERY_DIR / "Cargo.toml").is_file():
        sys.exit(f"error: {GALLERY_DIR} not found — the gallery crate is missing.")

    dist = REPO_ROOT / "dist"
    if dist.exists():
        shutil.rmtree(dist)
    dist.mkdir(parents=True)

    # 1. Lay the whole static site over dist/. Skip any stray local build output
    #    (a `pkg/` left by a standalone wasm-bindgen run) — the bundle is built fresh
    #    below, straight into the dist root.
    for item in WEB_DIR.iterdir():
        if item.name == "pkg":
            continue
        dest = dist / item.name
        if item.is_dir():
            shutil.copytree(item, dest)
        else:
            shutil.copy2(item, dest)

    # 2. Build the ONE shared bundle (axiom-loader.js + <snake>_bg.*) at the dist root.
    #    fast shares the main target dir (incremental); full uses the MVP build dir so
    #    std compiles once.
    target_dir = (REPO_ROOT / "target") if args.fast else (REPO_ROOT / "target" / "package-mvp")
    print(f"Building the gallery bundle into {dist}{' (fast: wasm-only)' if args.fast else ''}\n")
    package_app.build_bundle(GALLERY_DIR, dist, fast=args.fast, target_dir=target_dir, debug=args.debug)

    print(f"\nassembled the single-bundle gallery into {dist}  ({_dir_size_mb(dist):.0f} MB total)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
