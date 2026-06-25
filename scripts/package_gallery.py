#!/usr/bin/env python3
"""Assemble the demo gallery into ``dist/`` with every app PACKAGED.

Repo tooling (alongside ``package_app.py`` and the Makefile), NOT part of the engine
dependency graph. This replaces the old ``--target web`` + copy assembler: instead of
copying each app's ``wasm-bindgen --target web`` ``pkg/`` output, it runs the full
packaging pipeline (``package_app.package``) for every demo, so each ``dist/<id>/``
carries the capability ladder — a size-optimized wasm fast-path AND a Binaryen
wasm2js fallback for browsers with no WebAssembly, behind the shared loader. The
gallery shell (``gallery.js``) boots every demo through that loader.

All apps share ONE persistent MVP cargo target dir (``target/package-mvp``), so std
and the dependency graph compile once and are reused across the eight apps. The first
run is still slow (it builds std MVP and every app); re-runs are incremental.

Build the netplay SDK first if you want netplay's networking (the Makefile's
``gallery-build`` vendors it into the app's ``web/`` before calling this, and the
packager then copies that ``web/vendor/`` into ``dist/netplay/``).
"""

from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import package_app  # noqa: E402  (local repo tooling, not an installed package)

REPO_ROOT = Path(__file__).resolve().parent.parent

# The static gallery shell copied verbatim to the dist root (the shared per-demo
# shell + landing grid). gallery.js boots each shared-shell demo via its loader.
GALLERY_FILES = ["index.html", "demo.html", "gallery.js", "keypad.js", "styles.css"]

# gallery demo id (== dist subdir, must match gallery.js DEMOS[].id/dir/page) ->
# the app crate directory packaged into it.
GALLERY_APPS = {
    "rotating-cube": "apps/axiom-demo-rotating-cube-browser",
    "netplay": "apps/axiom-netplay-browser",
    "retro_fps": "apps/axiom-retro-fps-browser",
    "stress-cubes": "apps/axiom-stress-cubes-browser",
    "growth": "apps/axiom-growth",
    "roomed-puzzle": "apps/axiom-roomed-puzzle",
    "quintet": "apps/axiom-quintet",
    "harness": "apps/axiom-browser-dev-harness",
}


def _dir_size_mb(path: Path) -> float:
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file()) / (1024 * 1024)


def main() -> int:
    parser = argparse.ArgumentParser(description="Package every gallery demo into dist/.")
    parser.add_argument(
        "--fast",
        action="store_true",
        help="quick wasm-only demos (normal incremental build, no wasm2js fallback) for iteration",
    )
    args = parser.parse_args()

    dist = REPO_ROOT / "dist"
    if dist.exists():
        shutil.rmtree(dist)
    dist.mkdir(parents=True)

    gallery = REPO_ROOT / "gallery"
    for name in GALLERY_FILES:
        shutil.copy2(gallery / name, dist / name)

    # fast shares the main target dir (incremental); full uses the MVP build dir.
    target_dir = (REPO_ROOT / "target") if args.fast else (REPO_ROOT / "target" / "package-mvp")
    print(f"Packaging {len(GALLERY_APPS)} demos into {dist}{' (fast)' if args.fast else ''}\n")
    for demo_id, app_rel in GALLERY_APPS.items():
        app_dir = REPO_ROOT / app_rel
        if not (app_dir / "Cargo.toml").is_file():
            sys.exit(f"error: {app_rel} not found — gallery app missing.")
        package_app.package(app_dir, out=dist / demo_id, fast=args.fast, target_dir=target_dir)

    print(f"\nassembled packaged gallery into {dist}  ({_dir_size_mb(dist):.0f} MB total)")
    sizes = sorted(
        ((demo_id, _dir_size_mb(dist / demo_id)) for demo_id in GALLERY_APPS),
        key=lambda kv: kv[1],
        reverse=True,
    )
    for demo_id, mb in sizes:
        print(f"  {demo_id:<16} {mb:6.1f} MB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
