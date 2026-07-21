#!/usr/bin/env python3
"""Assemble the app gallery into ``dist/`` from per-app ``app.json`` manifests.

Repo tooling (alongside ``package_app.py`` and the Makefile), NOT part of the engine
dependency graph.

**Apps register themselves.** An app joins the gallery by having an ``app.json`` in
its own directory — nothing central lists it. There is no hand-maintained manifest
here, no per-app Makefile target, and no committed build artifact: add the file and
the app appears; delete the app directory and it leaves. (The previous design kept
the list in ``gallery.js`` plus a Makefile target plus a committed single-file page
per app — three shared files per app, which is exactly why five of the seven
TypeScript targets had rotted into pointing at apps that no longer existed.)

``app.json`` carries only what cannot be derived — the editorial copy and the app's
shape::

    {
      "title": "Axiom Arcade",
      "blurb": "One line for the card.",
      "description": "The long-form paragraph.",
      "kind": "ts-web-engine",          // or "rust-wasm"
      "tags": ["game", "arcade"]
    }

Everything else is derived here: the id (directory name, minus any ``axiom-``
prefix), the entry page, the engine version, and the build timestamp. Those land in
``dist/manifest.json``, which the landing grid fetches at runtime.

**One engine, many apps.** The pure-TypeScript engine is built ONCE into
``dist/engine/web-engine/<version>/`` and every TypeScript app resolves the bare
``@axiom/web-engine`` specifier to it through an injected import map — the same
mechanism ``axiom-serve`` already injects in dev, just pointing at a versioned,
deployable path. Apps ship only their own compiled code, the browser caches the
engine once across the whole gallery, and an engine fix is one rebuild rather than
re-packaging every app. The path is relative, so the gallery can be served from any
sub-path. Rust apps still statically link the engine into their own wasm — that is
inherent to wasm, not a choice this script makes.

Layout produced::

    dist/
      index.html, gallery.js, styles.css   the landing grid (static shell)
      manifest.json                        every registered app, derived + authored
      engine/web-engine/<version>/         the shared pure-TS engine, built once
      <id>/                                one directory per app

``--fast`` skips the Binaryen wasm2js fallback for the Rust apps (tight iteration),
and ``--only <id> [...]`` restricts which apps are rebuilt.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import package_app  # noqa: E402  (local repo tooling, not an installed package)

REPO_ROOT = Path(__file__).resolve().parent.parent
GALLERY_WEB = REPO_ROOT / "apps" / "axiom-gallery" / "web"
APPS_DIR = REPO_ROOT / "apps"
WEB_ENGINE_DIR = REPO_ROOT / "packages" / "axiom-web-engine"
# tsgo lives in the @axiom/game package's node_modules; every TS build in this repo
# reaches it the same way (see the Makefile).
TSGO_PREFIX = REPO_ROOT / "packages" / "axiom-game"

KIND_RUST = "rust-wasm"
KIND_TS = "ts-web-engine"
KINDS = (KIND_RUST, KIND_TS)


@dataclass
class AppSpec:
    """One registered app: its authored ``app.json`` plus everything derived."""

    id: str
    dir: Path
    title: str
    blurb: str
    description: str
    kind: str
    tags: list[str] = field(default_factory=list)
    # Static files a TS app loads at RUNTIME (so the page never references them and
    # they cannot be discovered from it). Paths are relative to the app's `web/`.
    assets: list[str] = field(default_factory=list)

    @property
    def page(self) -> str:
        return f"{self.id}/index.html"

    def manifest_entry(self) -> dict:
        return {
            "id": self.id,
            "title": self.title,
            "blurb": self.blurb,
            "description": self.description,
            "kind": self.kind,
            "tags": self.tags,
            "page": self.page,
        }


# ---------------------------------------------------------------------------
# Discovery
# ---------------------------------------------------------------------------


def _derive_id(app_dir: Path) -> str:
    """The gallery id for an app directory: its name without the ``axiom-`` prefix
    (``apps/axiom-home-run`` -> ``home-run``, ``apps/casino-games`` -> ``casino-games``).
    Keeps every existing demo URL stable."""
    name = app_dir.name
    return name[len("axiom-") :] if name.startswith("axiom-") else name


def discover_apps() -> list[AppSpec]:
    """Every app under ``apps/`` carrying an ``app.json``, sorted by title.

    Presence of the file IS the registration. Absence means "not published" — which
    is what keeps the internal crates (the FFI shims, the netcode sims, the shared
    game runtime) out of the gallery without a denylist anywhere.
    """
    specs: list[AppSpec] = []
    for manifest in sorted(APPS_DIR.glob("*/app.json")):
        app_dir = manifest.parent
        try:
            data = json.loads(manifest.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            sys.exit(f"error: {manifest} is not valid JSON — {exc}")

        missing = [k for k in ("title", "blurb", "kind") if not data.get(k)]
        if missing:
            sys.exit(f"error: {manifest} is missing required field(s): {', '.join(missing)}")
        kind = data["kind"]
        if kind not in KINDS:
            sys.exit(f"error: {manifest} has kind {kind!r} — expected one of {', '.join(KINDS)}")
        if not (app_dir / "web" / "index.html").is_file():
            sys.exit(f"error: {app_dir}/web/index.html not found — {manifest} registers an app with no page.")

        specs.append(
            AppSpec(
                id=data.get("id") or _derive_id(app_dir),
                dir=app_dir,
                title=data["title"],
                blurb=data["blurb"],
                description=data.get("description", ""),
                kind=kind,
                tags=list(data.get("tags", [])),
                assets=list(data.get("assets", [])),
            )
        )

    duplicates = {s.id for s in specs if [t.id for t in specs].count(s.id) > 1}
    if duplicates:
        sys.exit(f"error: duplicate gallery id(s): {', '.join(sorted(duplicates))}")
    return sorted(specs, key=lambda s: s.title.lower())


# ---------------------------------------------------------------------------
# The shared pure-TypeScript engine
# ---------------------------------------------------------------------------


def _run(cmd: list[str], *, cwd: Path | None = None) -> None:
    printable = " ".join(cmd)
    result = subprocess.run(cmd, cwd=cwd, shell=(os.name == "nt"))
    if result.returncode != 0:
        sys.exit(f"error: command failed ({result.returncode}): {printable}")


def engine_version() -> str:
    return json.loads((WEB_ENGINE_DIR / "package.json").read_text(encoding="utf-8"))["version"]


def _ensure_node_deps(prefix: Path, what: str) -> None:
    """Install a package's node_modules if they are missing.

    Keeps this script runnable from a bare checkout — notably the Pages deploy,
    which invokes it directly rather than through `make` and would otherwise need
    to mirror every npm install here in the workflow. It installs on demand instead,
    so there is one place that knows what the build needs.
    """
    if (prefix / "node_modules").is_dir():
        return
    print(f"[deps] installing {what} (one-off)", flush=True)
    _run(["npm", "--prefix", str(prefix), "install", "--no-audit", "--no-fund"])


def build_shared_engine(dist: Path) -> str:
    """Build ``@axiom/web-engine`` once and lay it under
    ``dist/engine/web-engine/<version>/``. Returns the version, which every TS app's
    import map then points at."""
    version = engine_version()
    _ensure_node_deps(WEB_ENGINE_DIR, "@axiom/web-engine dependencies")
    # tsgo and esbuild both live in the @axiom/game package's toolchain.
    _ensure_node_deps(TSGO_PREFIX, "the TypeScript build toolchain (tsgo, esbuild)")
    print(f"[engine] building @axiom/web-engine {version} (once, shared by every TS app)", flush=True)
    _run(["npm", "--prefix", str(WEB_ENGINE_DIR), "run", "build"])

    built = WEB_ENGINE_DIR / "dist"
    if not (built / "index.js").is_file():
        sys.exit(f"error: {built}/index.js missing — the @axiom/web-engine build produced no entry point.")
    out = dist / "engine" / "web-engine" / version
    out.mkdir(parents=True, exist_ok=True)
    # Ship the runnable JS only; the .d.ts files are a build-time concern.
    for item in built.rglob("*.js"):
        dest = out / item.relative_to(built)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(item, dest)
    size_kb = sum(f.stat().st_size for f in out.rglob("*.js")) / 1000
    print(f"[engine] {out.relative_to(dist)}  ({size_kb:.0f} KB, fetched once for the whole gallery)", flush=True)
    return version


# ---------------------------------------------------------------------------
# TypeScript apps
# ---------------------------------------------------------------------------

# The bundled entry every packaged TS app page loads.
BUNDLE_NAME = "app.js"

_ASSET_REF_RE = re.compile(r'(?:href|src)="(?!https?:|//|data:)\.?/?([^"#?]+)"')
_CSS_URL_RE = re.compile(r"url\(\s*['\"]?(?!https?:|//|data:)\.?/?([^)'\"]+)")


def _referenced_assets(page_html: str, web: Path) -> set[Path]:
    """The static files a page actually asks for, as paths relative to ``web/``.

    Derived from the page rather than filtered by a denylist. A denylist is the same
    trap as a central app list: it silently ships whatever nobody remembered to
    exclude — an app's Python interaction tests and screenshot corpus, say — and
    nothing ever tells you. Reading the references instead means the packaged app
    contains exactly what it loads.

    Stylesheets are followed one level for ``url(...)`` so fonts and images ride
    along. Anything a JS module fetches at RUNTIME is invisible here; an app that
    does that declares the paths in its ``app.json`` ``assets`` list.
    """
    found: set[Path] = set()
    for match in _ASSET_REF_RE.finditer(page_html):
        rel = Path(match.group(1))
        if (web / rel).is_file():
            found.add(rel)
    for sheet in [f for f in found if f.suffix == ".css"]:
        for match in _CSS_URL_RE.finditer((web / sheet).read_text(encoding="utf-8", errors="ignore")):
            rel = (sheet.parent / match.group(1)).as_posix()
            if (web / rel).is_file():
                found.add(Path(rel))
    return found

_IMPORT_MAP = '<script type="importmap">{{"imports":{{"@axiom/web-engine":"{path}"}}}}</script>'


def _rewrite_page(html: str, *, import_map: str) -> str:
    """Make one app page self-sufficient under ``dist/<id>/``.

    Two edits. First, app-absolute asset paths (``/dist/main.js``, ``/styles/x.css``)
    become relative — under a per-app directory a leading slash would escape to the
    gallery root. Second, an import map is injected so the bare ``@axiom/web-engine``
    specifier resolves to the shared engine, mirroring what ``axiom-serve`` injects
    in dev.
    """
    html = re.sub(r'(\b(?:href|src)=")/(?!/)', r"\1./", html)
    if "importmap" in html:
        return html
    if "<head>" not in html:
        sys.exit(
            "error: page has no <head> to inject the engine import map into.\n"
            "       Without it the bare @axiom/web-engine specifier cannot resolve and the app "
            "will not boot."
        )
    return html.replace("<head>", f"<head>\n    {import_map}", 1)


def _neutralize_dev_couplings(js: str) -> str:
    """Strip the dev-server couplings from a compiled harness.

    The TS harnesses hot-reload in dev by dynamically importing a cache-busted
    ``/dist/game.js?v=N`` and listening on an ``/events`` SSE channel. Neither
    survives a static deploy: the import is app-absolute (wrong under ``dist/<id>/``)
    and the SSE endpoint does not exist. Both are rewritten by PATTERN rather than
    by exact string, so a harness that is formatted differently — or a fourth app
    that grows one later — is still handled; ``_verify_static`` then fails the build
    if anything of the sort survives.

    The import becomes a plain SIBLING specifier (``./game.js``), because this runs
    over the staged copy of ``web/dist`` whose files sit at the staging root. That
    also makes the import statically analyzable, which is what lets esbuild pull the
    game module INTO the bundle: left as a template literal with an interpolation it
    is opaque, and esbuild would quietly emit a bundle that still fetches a path
    which does not exist in the deployed tree.
    """
    js = re.sub(
        r"import\(\s*(?:__rewriteRelativeImportExtension\(\s*)?`/dist/([^`?]+)(?:\?[^`]*)?`\s*\)?\s*\)",
        r'import("./\1")',
        js,
    )
    js = re.sub(r'new EventSource\(\s*"[^"]*"\s*\)', "({ addEventListener() {} })", js)
    return js


# What a surviving dev-server coupling actually looks like in code. These match
# CODE SHAPES, not substrings: a loose search for "/events" or "/dist/" also hits
# prose in a doc comment and the perfectly ordinary relative import
# `from "./events.js"`, so it cries wolf on correct output.
_DEV_COUPLINGS = (
    (re.compile(r"""new\s+EventSource\(\s*['"`]/"""), "an absolute EventSource URL (dev SSE channel)"),
    (re.compile(r"""import\(\s*(?:__rewriteRelativeImportExtension\(\s*)?['"`]/"""), "a dynamic import of an absolute path"),
    (re.compile(r"""\bfrom\s*['"`]/"""), "a static import from an absolute path"),
)


def _verify_static(out: Path, app_id: str) -> None:
    """Fail loudly if anything in the packaged app still reaches for the dev server.

    A silent miss here is the worst failure mode available: the gallery builds green
    and the app 404s in the browser. Cheap to check, so it is checked every time.
    """
    offenders: list[str] = []
    for item in sorted(out.rglob("*.js")):
        text = item.read_text(encoding="utf-8", errors="ignore")
        for pattern, what in _DEV_COUPLINGS:
            if pattern.search(text):
                offenders.append(f"{item.relative_to(out)} ({what})")
    page = out / "index.html"
    if page.is_file() and re.search(r'\b(?:href|src)="/(?!/)', page.read_text(encoding="utf-8")):
        offenders.append("index.html (an app-absolute href/src)")
    if offenders:
        sys.exit(
            f"error: [{app_id}] packaged output still reaches for the dev server:\n"
            + "".join(f"       - {o}\n" for o in offenders)
            + "       Extend _neutralize_dev_couplings() / _rewrite_page() to cover the new shape."
        )


_ENTRY_RE = re.compile(r'<script\s+type="module"\s+src="/dist/([^"]+)"\s*>\s*</script>')


def build_ts_app(spec: AppSpec, dist: Path, version: str) -> None:
    """Compile and bundle a TypeScript app into ``dist/<id>/``, resolved against the
    shared engine rather than carrying a copy of it.

    The app is bundled into a single ``app.js`` with ``@axiom/web-engine`` marked
    EXTERNAL — so the bare specifier survives into the output and the browser
    resolves it, once, through the injected import map. Raw compiler output would
    also run, but it is ~100 files of heavily-commented source per app; bundling
    turns that into one request and a third of the bytes, while still sharing the
    engine. Identifiers are never mangled (the repo-wide convention for its
    packagers), so the deployed code stays readable.
    """
    out = dist / spec.id
    web = spec.dir / "web"
    print(f"[{spec.id}] compiling TypeScript", flush=True)
    _run(["npm", "--prefix", str(TSGO_PREFIX), "exec", "--", "tsgo", "-p", str(web / "tsconfig.json")])

    page_src = (web / "index.html").read_text(encoding="utf-8")
    entry = _ENTRY_RE.search(page_src)
    if entry is None:
        sys.exit(
            f'error: [{spec.id}] {web}/index.html has no <script type="module" src="/dist/....js"> entry point '
            "— the packager needs one to know what to bundle."
        )

    # Stage the compiled output and neutralize the dev-server couplings THERE, so the
    # app's own web/dist (what axiom-serve serves in dev) is never mutated.
    stage = dist / f".stage-{spec.id}"
    if stage.exists():
        shutil.rmtree(stage)
    shutil.copytree(web / "dist", stage)
    for item in stage.rglob("*.js"):
        text = item.read_text(encoding="utf-8")
        rewritten = _neutralize_dev_couplings(text)
        if rewritten != text:
            item.write_text(rewritten, encoding="utf-8")

    out.mkdir(parents=True, exist_ok=True)
    _run(
        [
            "npm", "--prefix", str(TSGO_PREFIX), "exec", "--", "esbuild",
            str(stage / entry.group(1)),
            "--bundle",
            "--format=esm",
            "--platform=browser",
            "--target=es2022",
            "--legal-comments=none",
            "--minify-whitespace",
            "--minify-syntax",
            # The whole point: the engine is NOT bundled in. It stays a bare
            # specifier and resolves through the import map to the one shared copy.
            "--external:@axiom/web-engine",
            f"--outfile={out / BUNDLE_NAME}",
        ]
    )
    shutil.rmtree(stage)

    # The deployed page: entry swapped to the bundle, app-absolute paths made
    # relative, import map injected.
    import_map = _IMPORT_MAP.format(path=f"../engine/web-engine/{version}/index.js")
    page_out = _rewrite_page(
        _ENTRY_RE.sub(f'<script type="module" src="./{BUNDLE_NAME}"></script>', page_src, count=1),
        import_map=import_map,
    )
    (out / "index.html").write_text(page_out, encoding="utf-8")

    # Exactly the static files the DEPLOYED page references, plus anything the app
    # declares it fetches at runtime. Derived from the rewritten page so the compiled
    # entry (now bundled into app.js) is not dragged along beside it.
    declared = {Path(a) for a in spec.assets}
    for rel in sorted(_referenced_assets(page_out, web) | declared):
        source = web / rel
        if not source.exists():
            sys.exit(f"error: [{spec.id}] app.json declares asset {rel.as_posix()!r}, which does not exist in {web}")
        dest = out / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(source, dest, dirs_exist_ok=True) if source.is_dir() else shutil.copy2(source, dest)

    _verify_static(out, spec.id)
    size_kb = sum(f.stat().st_size for f in out.rglob("*") if f.is_file()) / 1000
    print(f"[{spec.id}] packaged into {out.name}/  ({size_kb:.0f} KB, engine shared)", flush=True)


# ---------------------------------------------------------------------------
# Static shell + Rust apps
# ---------------------------------------------------------------------------


def copy_gallery_static(dist: Path) -> None:
    """Lay the gallery's static shell (landing grid + styles) over ``dist/``, skipping
    stray local build output. The shell owns no per-app directories: every app page
    is produced by its own build below."""
    if not (GALLERY_WEB / "index.html").is_file():
        sys.exit(f"error: {GALLERY_WEB} not found — the gallery static site is missing.")
    for item in GALLERY_WEB.iterdir():
        if item.name == "pkg":
            continue
        dest = dist / item.name
        if item.is_dir():
            shutil.copytree(item, dest, dirs_exist_ok=True)
        else:
            shutil.copy2(item, dest)


def build_rust_apps(
    specs: list[AppSpec],
    dist: Path,
    *,
    fast: bool,
    target_dir: Path,
    debug: bool = False,
) -> None:
    """Build every Rust app's wasm bundle into ``dist/<id>/``. Unchanged in substance
    from before: a Rust app statically links the engine into its own wasm, so there is
    no bundle to share."""
    if not specs:
        return
    app_dirs = [(spec.id, spec.dir) for spec in specs]
    prebuilt = fast

    def finish(app_id: str, app_dir: Path) -> None:
        out = dist / app_id
        print(f"[{app_id}] finishing wasm bundle into {out}{' (fast: wasm-only)' if fast else ''}", flush=True)
        snake = package_app.build_bundle(
            app_dir, out, fast=fast, target_dir=target_dir, debug=debug, prebuilt=prebuilt
        )
        package_app.emit_index_html(app_dir, out, snake, sdk_hosted=False)

    # fast: compile every wasm in ONE cargo invocation up front (engine deps build
    # once, no fat LTO so they LINK into each app), THEN finish each bundle
    # CONCURRENTLY — the per-app `wasm-opt -Oz` dominates and the runs are independent.
    # The slow MVP build-std path stays serial: each app runs its own cargo build,
    # which would contend on the shared target dir's build lock if parallelized.
    if prebuilt:
        package_app.prebuild_wasm_crates([d for _, d in app_dirs], target_dir=target_dir, debug=debug)
        workers = min(len(app_dirs), os.cpu_count() or 4)
        with ThreadPoolExecutor(max_workers=workers) as pool:
            list(pool.map(lambda pair: finish(*pair), app_dirs))
    else:
        for app_id, app_dir in app_dirs:
            finish(app_id, app_dir)


def build_apps(
    dist: Path,
    *,
    fast: bool,
    target_dir: Path,
    debug: bool = False,
    only: list[str] | None = None,
) -> list[AppSpec]:
    """Discover every registered app, build the shared engine, and lay each app under
    ``dist/<id>/``. Returns the specs that were built."""
    specs = discover_apps()
    if only is not None:
        known = {s.id for s in specs}
        unknown = [i for i in only if i not in known]
        if unknown:
            sys.exit(f"error: unknown app id(s) {', '.join(unknown)} — registered: {', '.join(sorted(known))}")
        specs = [s for s in specs if s.id in only]

    ts_specs = [s for s in specs if s.kind == KIND_TS]
    rust_specs = [s for s in specs if s.kind == KIND_RUST]

    if ts_specs:
        version = build_shared_engine(dist)
        for spec in ts_specs:
            build_ts_app(spec, dist, version)

    build_rust_apps(rust_specs, dist, fast=fast, target_dir=target_dir, debug=debug)
    return specs


def emit_manifest(dist: Path, specs: list[AppSpec]) -> None:
    """Write ``dist/manifest.json`` — what the landing grid fetches to build its cards."""
    manifest = {
        "generated": datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
        "engine": {"webEngine": engine_version()},
        "apps": [spec.manifest_entry() for spec in specs],
    }
    (dist / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"[manifest] {len(specs)} app(s) registered", flush=True)


def _dir_size_mb(path: Path) -> float:
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file()) / (1024 * 1024)


def main() -> int:
    parser = argparse.ArgumentParser(description="Package the app gallery into dist/.")
    parser.add_argument(
        "--fast",
        action="store_true",
        help="quick wasm-only bundles (no-LTO release-preview profile, no wasm2js fallback) for iteration",
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="debug wasm builds (keeps debug_assertions on for the Canvas2D deep profiler; "
        "used by the render benchmark). Implies --fast (no wasm2js fallback).",
    )
    parser.add_argument(
        "--only",
        nargs="+",
        metavar="ID",
        help="build only these app ids (still lays the full static shell and manifest)",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="list every registered app (id, kind, title) and exit — builds nothing",
    )
    args = parser.parse_args()
    if args.debug:
        args.fast = True

    if args.list:
        for spec in discover_apps():
            print(f"{spec.id:<24} {spec.kind:<16} {spec.title}")
        return 0

    if not (GALLERY_WEB / "index.html").is_file():
        sys.exit(f"error: {GALLERY_WEB} not found — the gallery static site is missing.")

    dist = REPO_ROOT / "dist"
    if dist.exists():
        shutil.rmtree(dist)
    dist.mkdir(parents=True)

    # 1. The static shell (landing grid + styles) over dist/.
    copy_gallery_static(dist)

    # 2. The shared engine + one directory per registered app.
    target_dir = (REPO_ROOT / "target") if args.fast else (REPO_ROOT / "target" / "package-mvp")
    specs = build_apps(dist, fast=args.fast, target_dir=target_dir, debug=args.debug, only=args.only)

    # 3. Re-lay the static shell (it is cheap and idempotent, and the landing grid may
    #    have changed while the slow wasm builds ran). App directories are untouched.
    copy_gallery_static(dist)

    # 4. The manifest the landing grid reads. It describes what was actually BUILT,
    #    not merely what is registered: under --only, dist/ holds a subset, and a
    #    manifest listing the rest would render cards that 404.
    emit_manifest(dist, specs)

    print(f"\nassembled the gallery into {dist}  ({_dir_size_mb(dist):.0f} MB total)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
