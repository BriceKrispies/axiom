#!/usr/bin/env python3
"""Assemble the static demo-gallery deploy bundle into ``dist/``.

Repo tooling (alongside the Makefile and the other scripts/), NOT part of the
engine dependency graph. It is the single source of truth for the gallery's
``dist/`` layout, called by both ``make gallery-build`` and the
``deploy-pages`` GitHub Actions workflow so local previews and the published
site are byte-for-byte the same shape.

It copies the static gallery shell (``gallery/``) plus each browser app's
wasm-bindgen output (``apps/<app>/web/pkg``) into ``dist/``. Build the wasm
bundles first (``make gallery-build`` does this; CI runs the same cargo +
wasm-bindgen commands), or this fails with a pointer to do so.
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# The static gallery shell files copied verbatim to the dist root.
GALLERY_FILES = ["index.html", "demo.html", "gallery.js", "keypad.js", "styles.css"]

# demo id (== dist subdir) -> the app's wasm-bindgen output directory. These
# demos run in the shared per-demo shell (demo.html), so only their pkg is copied.
DEMO_PKGS = {
    "rotating-cube": "apps/axiom-demo-rotating-cube-browser/web/pkg",
    "netplay": "apps/axiom-netplay-browser/web/pkg",
    "doom": "apps/axiom-doom-browser/web/pkg",
    "stress-cubes": "apps/axiom-stress-cubes-browser/web/pkg",
}

# Self-hosted demos own their page (a multi-screen flow that does not fit the
# shared shell), so their whole web/ dir (index.html + pkg) is copied verbatim.
# demo id (== dist subdir) -> the app's web/ directory.
DEMO_PAGES = {
    "growth": "apps/axiom-growth/web",
}


def main() -> int:
    dist = REPO_ROOT / "dist"
    if dist.exists():
        shutil.rmtree(dist)
    dist.mkdir(parents=True)

    gallery = REPO_ROOT / "gallery"
    for name in GALLERY_FILES:
        shutil.copy2(gallery / name, dist / name)

    for demo_id, pkg_rel in DEMO_PKGS.items():
        pkg = REPO_ROOT / pkg_rel
        if not pkg.is_dir():
            print(
                f"error: {pkg_rel} not found — build the wasm bundles first "
                f"(`make gallery-build`, or the cargo + wasm-bindgen steps in "
                f"deploy-pages.yml).",
                file=sys.stderr,
            )
            return 1
        shutil.copytree(pkg, dist / demo_id / "pkg")

    for demo_id, web_rel in DEMO_PAGES.items():
        web = REPO_ROOT / web_rel
        if not (web / "pkg").is_dir():
            print(
                f"error: {web_rel}/pkg not found — build the wasm bundles first "
                f"(`make gallery-build`, or the cargo + wasm-bindgen steps in "
                f"deploy-pages.yml).",
                file=sys.stderr,
            )
            return 1
        # Copy the whole self-hosted page (index.html + pkg + any assets).
        shutil.copytree(web, dist / demo_id)

    print(f"assembled gallery into {dist}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
