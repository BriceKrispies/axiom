#!/usr/bin/env python3
"""Package a single Axiom browser app into a self-contained, droppable bundle.

Repo tooling (alongside ``package_gallery.py`` and the Makefile), NOT part of the
engine dependency graph. It turns one ``apps/<app>`` crate into a directory you can
serve from any static host, with a built-in capability ladder:

  * **wasm fast path** — real WebAssembly where the browser supports it, run through
    Binaryen ``wasm-opt -Oz`` for size.
  * **wasm2js fallback** — for a browser with no WebAssembly at all, Binaryen
    ``wasm2js`` compiles the same module to plain JS. The loader prints exactly one
    ``console.warn`` line and runs it. (The engine's own WebGPU -> WebGL2 -> Canvas2D
    backend fallback is orthogonal and already handled at runtime in
    ``axiom-windowing``; together they let even a no-wasm, no-WebGPU browser run the
    game: logic in JS, rendered via the canvas2d backend.)

Both paths are driven by ONE wasm-bindgen *bundler*-target glue file, so there is a
single ``__wbg_set_wasm`` seam and a single loader. The loader is made API-compatible
with the old ``--target web`` glue (``export default init`` + ``export *``), so an
app's existing ``web/index.html`` only needs its import path swapped to the loader.

Why MVP / build-std: modern wasm-bindgen emits reference-types (externref) glue,
which ``wasm2js`` cannot consume ("multiple tables not supported"). Reference-types
comes from Rust's default wasm target features, and the precompiled ``std`` is built
with them on — so the app must be rebuilt as MVP *including* ``std`` via nightly
``-Z build-std`` with the features disabled. That is the lowest correct layer to fix
it: the artifact is genuinely MVP, not a reference-types build with the symptom
papered over.

Two app shapes package through the same pipeline:

  * **Native apps** wire the wasm glue directly in ``web/index.html`` (importing
    ``pkg/<snake>.js``). The packager rewrites that to the bundle-root loader.
  * **SDK-hosted TypeScript apps** (``axiom-game-runtime``, authored over the
    ``@axiom/game`` SDK) load the glue from a compiled harness (``web/dist/harness.js``)
    at the conventional ``/pkg/<snake>.js`` path, the SDK from a ``/vendor/<name>/`` URL
    in the page's import map, and the author module from ``/dist/game.js``. For these
    the packager builds the SDK, compiles ``web/src`` with tsgo, materializes the vendor
    dir, and drops the loader in AT ``pkg/<snake>.js`` (no page rewrite needed, since the
    loader is a drop-in for the ``--target web`` glue). Such bundles use absolute
    (/pkg, /vendor, /dist) URLs — serve them from a domain root; ``--inline`` is not
    supported for them.

Usage:
    uv run --no-project python scripts/package_app.py <app> [--out DIR] [--inline]

``<app>`` is an app crate dir (``apps/axiom-gallery``) or a short name (``gallery``).
"""

from __future__ import annotations

import argparse
import base64
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BINARYEN_BIN = REPO_ROOT / "scripts" / "packaging" / "node_modules" / "binaryen" / "bin"
WASM_TARGET = "wasm32-unknown-unknown"

# The fast/preview cargo profile for browser-demo bundles (defined in the root
# Cargo.toml). It inherits `release` but turns fat LTO OFF, so the engine's object code
# is codegen'd ONCE (cached in the shared target dir) and merely LINKED into each demo
# app, instead of re-running whole-program LTO over the whole engine per app — the fix
# for the ~10-min 9-app Pages build. wasm-opt -Oz (below) recovers most of the size.
# `make package` still ships via `[profile.release]` (fat LTO).
PREVIEW_PROFILE = "release-preview"

# An "SDK-hosted" browser app (axiom-game-runtime, axiom-retro-fps-ts-browser) is authored
# in TypeScript over the `@axiom/game` SDK. Its index.html does NOT wire the wasm glue
# itself: a compiled harness (web/dist/harness.js) imports the glue from the
# conventional `/pkg/<snake>.js` path, the SDK from a `/vendor/<name>/` URL declared in
# the page's import map, and the author module from `/dist/game.js`. The dev server
# synthesizes /vendor and /dist live (see scripts/axiom_dev_server.mjs); packaging
# bakes them in — it compiles web/src -> web/dist with the same tsgo, materializes the
# vendor dir from the SDK's built dist/, and drops the capability-detecting loader in
# AT `pkg/<snake>.js` (the exact path the harness imports), since the loader is a
# drop-in for the `--target web` glue. No harness edit, no compiled-JS rewriting.
AXIOM_GAME_SDK = REPO_ROOT / "packages" / "axiom-game"
VENDORED_SDKS = {"/vendor/axiom-game/": AXIOM_GAME_SDK / "dist"}

# Disable the post-MVP wasm features that make wasm-bindgen emit externref glue and
# that wasm2js cannot consume. Rebuilt into std too (-Z build-std), so the whole
# module is consistently MVP. Encoded with \x1f separators (CARGO_ENCODED_RUSTFLAGS)
# so a --remap-path-prefix containing spaces survives.
MVP_TARGET_FEATURES = "-Ctarget-feature=-reference-types,-multivalue"

# wasm-opt passes that lower the remaining post-MVP ops (sign-ext, nontrapping
# float->int, bulk-memory copy/fill) down to MVP so wasm2js validates its output.
WASM2JS_LOWERING = [
    "--signext-lowering",
    "--llvm-nontrapping-fptoint-lowering",
    "--llvm-memory-copy-fill-lowering",
    "-O2",
]

# When the module exposes i64 across the JS boundary, Binaryen legalizes it and emits
# an `import * as env from 'env'` for the i64 high-word scratch (set/getTempRet0). A
# bare specifier has no resolver in a static bundle, so we replace that import with the
# standard inline scratch — making the wasm2js fallback self-contained for ANY app, not
# just i64-free ones. (Single quotes: exactly what Binaryen emits.)
WASM2JS_ENV_IMPORT = "import * as env from 'env';"
WASM2JS_ENV_SHIM = (
    "// scripts/package_app.py: Binaryen's wasm2js imports its i64 high-word scratch "
    "from a bare\n// `env` module the embedder must supply; a static bundle has no "
    "bare-specifier resolver,\n// so the standard tempRet0 scratch is inlined here to "
    "keep the fallback self-contained.\n"
    "const env = (() => { let tempRet0 = 0; "
    "return { setTempRet0(x) { tempRet0 = x | 0; }, getTempRet0() { return tempRet0; } }; })();"
)


def run(cmd: list[str], *, env: dict[str, str] | None = None, cwd: Path | None = None) -> None:
    """Run a subprocess, streaming its output, and abort the script on failure."""
    printable = " ".join(str(c) for c in cmd)
    print(f"  $ {printable}", flush=True)
    result = subprocess.run(cmd, env=env, cwd=cwd)
    if result.returncode != 0:
        sys.exit(f"error: command failed ({result.returncode}): {printable}")


def binaryen(tool: str, *args: str) -> list[str]:
    """A Binaryen tool invocation. The npm `bin/*` are node scripts, so run via node."""
    script = BINARYEN_BIN / tool
    if not script.is_file():
        sys.exit(
            f"error: {script} not found — install the pinned Binaryen toolchain first:\n"
            f"       npm --prefix scripts/packaging install"
        )
    return ["node", str(script), *args]


def resolve_app(arg: str) -> Path:
    """Resolve an app identifier to its crate directory (must hold Cargo.toml + web/)."""
    candidates = [Path(arg), REPO_ROOT / arg, REPO_ROOT / "apps" / arg, REPO_ROOT / "apps" / f"axiom-{arg}"]
    for cand in candidates:
        if (cand / "Cargo.toml").is_file():
            return cand.resolve()
    searched = "\n       ".join(str(c) for c in candidates)
    sys.exit(f"error: could not find an app crate for '{arg}'. Looked at:\n       {searched}")


def run_shell(cmd: str, *, cwd: Path | None = None, check: bool = True) -> int:
    """Run a shell command string (for the npm/tsgo wrappers, which are .cmd on
    Windows and need a shell). Aborts on failure unless ``check=False``."""
    print(f"  $ {cmd}", flush=True)
    result = subprocess.run(cmd, cwd=str(cwd) if cwd else None, shell=True)
    if check and result.returncode != 0:
        sys.exit(f"error: command failed ({result.returncode}): {cmd}")
    return result.returncode


def crate_name(app_dir: Path) -> str:
    with (app_dir / "Cargo.toml").open("rb") as f:
        return tomllib.load(f)["package"]["name"]


def is_sdk_hosted(app_dir: Path, snake: str) -> bool:
    """True for a TypeScript SDK-hosted app: a compiled harness imports the wasm glue
    from `/pkg/<snake>.js`, so index.html itself never references `pkg/<snake>.js`
    (a native app wires that glue path directly in its page). This is the exact
    condition under which the native index.html glue-rewrite would be a no-op."""
    html = (app_dir / "web" / "index.html").read_text(encoding="utf-8")
    return f"pkg/{snake}.js" not in html


def prepare_sdk_hosted(app_dir: Path) -> None:
    """Build the @axiom/game SDK and compile the app's `web/src` to `web/dist`, the
    same toolchain the dev server runs — so the baked-in bundle matches the live loop.

    The SDK build is required (it produces the vendored dist/ and the tsgo binary);
    the app compile uses tsgo with the app's own tsconfig (`noEmitOnError: false`, so
    it emits JS even past type errors — hence ``check=False`` plus an output check)."""
    if not AXIOM_GAME_SDK.is_dir():
        sys.exit(f"error: {AXIOM_GAME_SDK} not found — cannot package an SDK-hosted TS app.")
    print("  (SDK-hosted TS app: building @axiom/game SDK + compiling web/src with tsgo)")
    run_shell(f'npm --prefix "{AXIOM_GAME_SDK}" install --no-audit --no-fund')
    run_shell(f'npm --prefix "{AXIOM_GAME_SDK}" run build')
    tsgo = AXIOM_GAME_SDK / "node_modules" / ".bin" / ("tsgo.cmd" if os.name == "nt" else "tsgo")
    run_shell(f'"{tsgo}" -p "{app_dir / "web" / "tsconfig.json"}"', cwd=app_dir / "web", check=False)
    missing = [n for n in ("harness.js", "game.js") if not (app_dir / "web" / "dist" / n).is_file()]
    if missing:
        sys.exit(f"error: tsgo did not emit web/dist/{', '.join(missing)} — cannot package.")


def vendor_sdks(app_dir: Path, out: Path) -> None:
    """Materialize every `/vendor/<name>/` URL the app's page declares (in its import
    map) from the corresponding SDK's built dist/, the way the dev server serves them
    live. Skipped for any vendor dir the app already ships under its own web/."""
    html = (app_dir / "web" / "index.html").read_text(encoding="utf-8")
    for url_prefix, src in VENDORED_SDKS.items():
        rel = url_prefix.strip("/")
        if (url_prefix in html) and not (app_dir / "web" / rel).exists():
            if not src.is_dir():
                sys.exit(f"error: vendored SDK build missing at {src} — run its `npm run build` first.")
            shutil.copytree(src, out / rel, dirs_exist_ok=True)


def _loader_body(
    *,
    glue_specifier: str,
    glue_import_key: str,
    acquire_bytes: str,
    wasm2js_specifier: str,
    has_fallback: bool,
) -> str:
    """The shared init() body for both loader flavours.

    Detection is capability-AND-instantiation aware. A browser can expose the
    WebAssembly API yet still *reject this specific (MVP) module* — a real failure
    mode, not just total API absence — so when a JS fallback is shipped we attempt
    real instantiation and treat EITHER the API being absent OR the instantiate call
    throwing/rejecting as "wasm unavailable", taking the wasm2js arm and emitting the
    single console.warn on whichever path leads there.

    When no fallback is shipped (`--fast`/`--no-fallback`) the original contract is
    preserved: an absent API throws the documented "no JS fallback" error, and an
    instantiation failure propagates its own (more informative) error naturally —
    there is no second arm to fall through to.

    `acquire_bytes` is the flavour-specific snippet that leaves a `bytes` ArrayBuffer
    in scope (a fetched sibling `.wasm` file vs. an embedded `data:` URL)."""
    instantiate = f"""    {acquire_bytes}
    const {{ instance }} = await WebAssembly.instantiate(bytes, {{ "{glue_import_key}": bg }});
    bg.__wbg_set_wasm(instance.exports);"""
    head = f"""import * as bg from "{glue_specifier}";
export * from "{glue_specifier}";

let booted = false;
"""
    if has_fallback:
        return f"""{head}
async function instantiateWasm() {{
{instantiate}
}}

export default async function init() {{
  if (booted) return bg;
  const hasWasm =
    typeof WebAssembly === "object" && typeof WebAssembly.instantiate === "function";
  // Try real wasm first; fall back on EITHER the API being absent OR this MVP module
  // failing to compile/instantiate in this engine (instantiateWasm() rejecting).
  const ranWasm = hasWasm && (await instantiateWasm().then(() => true, () => false));
  if (!ranWasm) {{
    console.warn("Axiom: WebAssembly unavailable — running JS fallback.");
    const wasm2js = await import("{wasm2js_specifier}");
    bg.__wbg_set_wasm(wasm2js);
  }}
  booted = true;
  return bg;
}}
"""
    return f"""{head}
export default async function init() {{
  if (booted) return bg;
  const hasWasm =
    typeof WebAssembly === "object" && typeof WebAssembly.instantiate === "function";
  if (!hasWasm) {{
    throw new Error("Axiom: WebAssembly unavailable and this bundle ships no JS fallback.");
  }}
{instantiate}
  booted = true;
  return bg;
}}
"""


def loader_source(snake: str, *, has_fallback: bool) -> str:
    """The capability-detecting loader: a drop-in for the old `--target web` glue."""
    header = """// Generated by scripts/package_app.py — do not edit.
// Capability-detecting loader: real wasm where supported, wasm2js fallback otherwise.
// API-compatible with the old wasm-bindgen `--target web` glue: `await init()` then
// call the app's exports (re-exported below). Pass nothing — any argument is ignored.
"""
    return header + _loader_body(
        glue_specifier=f"./{snake}_bg.js",
        glue_import_key=f"./{snake}_bg.js",
        acquire_bytes=(
            f'const url = new URL("./{snake}_bg.wasm", import.meta.url);\n'
            "    const bytes = await (await fetch(url)).arrayBuffer();"
        ),
        wasm2js_specifier=f"./{snake}_bg.wasm2js.js",
        has_fallback=has_fallback,
    )


def loader_source_inline(snake: str, wasm_b64: str, *, has_fallback: bool) -> str:
    """A single-file loader: wasm bytes embedded; cross-module specifiers are the bare
    names the inline import map (data: URLs) resolves. Bare specifiers always consult
    the realm's import map regardless of the importing module's base URL — which is
    why this works from inside a data: URL module."""
    return "// Generated by scripts/package_app.py --inline — do not edit.\n" + _loader_body(
        glue_specifier="axiom-glue",
        glue_import_key=f"./{snake}_bg.js",
        acquire_bytes=f'const bytes = await (await fetch("data:application/wasm;base64,{wasm_b64}")).arrayBuffer();',
        wasm2js_specifier="axiom-wasm2js",
        has_fallback=has_fallback,
    )


def _copy_extra_web_assets(app_dir: Path, out: Path) -> None:
    """Copy everything in the app's web/ except pkg/ and index.html."""
    for item in (app_dir / "web").iterdir():
        if item.name in {"pkg", "index.html"}:
            continue
        dest = out / item.name
        if item.is_dir():
            shutil.copytree(item, dest, dirs_exist_ok=True)
        else:
            shutil.copy2(item, dest)


def emit_index_html(app_dir: Path, out: Path, snake: str, *, sdk_hosted: bool) -> None:
    """Copy the app's web/ shell (minus pkg/), vendor any declared SDKs, and — for a
    native app — rewire its index.html glue import to the bundle-root loader.

    An SDK-hosted app needs NO page rewrite: its harness imports the glue from
    `/pkg/<snake>.js`, and the packager drops the loader in at exactly that path (see
    ``package``), so the page is served verbatim."""
    _copy_extra_web_assets(app_dir, out)
    vendor_sdks(app_dir, out)
    html = (app_dir / "web" / "index.html").read_text(encoding="utf-8")
    # A native app's page needs the loader swapped in for the web-target glue, with the
    # (now loader-ignored) wasm URL pointed at the bundle root. An SDK-hosted app's page
    # references neither (the harness owns the glue path) — these replaces are no-ops
    # there, but we skip them to keep the contract explicit.
    if not sdk_hosted:
        html = html.replace(f"pkg/{snake}.js", "axiom-loader.js")
        html = html.replace(f"pkg/{snake}_bg.wasm", f"{snake}_bg.wasm")
    (out / "index.html").write_text(html, encoding="utf-8")


def _data_url_js(text: str) -> str:
    return "data:text/javascript;base64," + base64.b64encode(text.encode("utf-8")).decode("ascii")


def emit_inline_html(
    app_dir: Path, out: Path, snake: str, glue_js: str, wasm2js_js: str | None, wasm_b64: str
) -> None:
    """Fold the whole bundle into a single self-contained index.html via an import map
    of data: URLs (glue / wasm2js / loader) plus the wasm embedded in the loader."""
    _copy_extra_web_assets(app_dir, out)
    loader_js = loader_source_inline(snake, wasm_b64, has_fallback=wasm2js_js is not None)
    # Hyphenated, colon-free bare specifiers: unambiguously "bare" (not URL-like), so
    # they always resolve through the document import map — including from inside the
    # data: URL modules, whose own base URL would otherwise defeat a relative path.
    imports = {"axiom-glue": _data_url_js(glue_js), "axiom-loader": _data_url_js(loader_js)}
    if wasm2js_js is not None:
        # The wasm2js module's ESM import of the glue (single-quoted) -> the map key.
        # Its internal asmFunc import key (double-quoted) is left untouched.
        wasm2js_js = wasm2js_js.replace(f"'./{snake}_bg.js'", '"axiom-glue"')
        imports["axiom-wasm2js"] = _data_url_js(wasm2js_js)
    import_map = (
        '<script type="importmap">\n{"imports":'
        + "{" + ",".join(f'"{k}":"{v}"' for k, v in imports.items()) + "}}"
        + "\n</script>\n"
    )
    html = (app_dir / "web" / "index.html").read_text(encoding="utf-8")
    # Point the app's loader import at the bare specifier (drop any ./ prefix), and
    # strip the cache-bust query (?v=${...}) — meaningless in a single file and it
    # would defeat the exact-string import-map match.
    html = html.replace(f"./pkg/{snake}.js", "axiom-loader").replace(f"pkg/{snake}.js", "axiom-loader")
    html = re.sub(r"axiom-loader\?v=\$\{[^}]*\}", "axiom-loader", html)
    # The import map must precede the first module script; inject it right after <head>.
    head = html.find("<head>")
    if head == -1:
        sys.exit("error: --inline needs a <head> in the app's index.html to place the import map.")
    insert = head + len("<head>")
    html = html[:insert] + "\n" + import_map + html[insert:]
    (out / "index.html").write_text(html, encoding="utf-8")


def _compile_wasm_bundle(
    app_dir: Path,
    tmp: Path,
    *,
    fast: bool,
    has_fallback: bool,
    target_dir: Path | None,
    debug: bool = False,
    prebuilt: bool = False,
) -> tuple[str, str, Path, Path | None]:
    """Build the crate's wasm and produce the bundle artifacts in ``tmp``.

    Steps 1-4 of the pipeline, shared by ``package`` (single app) and
    ``build_bundle`` (the multi-page gallery): cargo build (fast incremental or MVP
    ``-Z build-std``), wasm-bindgen *bundler* glue, ``wasm-opt -Oz`` fast wasm, and
    the optional wasm2js fallback. Returns ``(snake, glue_js, fast_wasm_path,
    wasm2js_path_or_None)``.

    ``prebuilt=True`` means this crate's wasm has already been produced in
    ``target_dir`` (e.g. by ``prebuild_wasm_crates`` in one shared cargo invocation),
    so step 1's cargo build is skipped and the existing artifact is read."""
    name = crate_name(app_dir)
    snake = name.replace("-", "_")
    # 1. Build the wasm (unless already prebuilt into target_dir). fast = normal
    # incremental build in the no-LTO PREVIEW profile (shares the main target dir, so
    # the engine compiles once and links into each app). Otherwise an MVP build with
    # std rebuilt and reference-types off (what wasm2js requires), paths anonymized,
    # into the shared package-mvp dir.
    env = os.environ.copy()
    if fast:
        target_dir = target_dir or (REPO_ROOT / "target")
        env["CARGO_TARGET_DIR"] = str(target_dir)
        # A --debug bundle keeps `debug_assertions` on (for the Canvas2D deep
        # profiler); the render benchmark uses it. Otherwise the no-LTO preview profile.
        profile = [] if debug else ["--profile", PREVIEW_PROFILE]
        if not prebuilt:
            run(["cargo", "build", "-p", name, "--target", WASM_TARGET, *profile], env=env)
    else:
        target_dir = target_dir or (REPO_ROOT / "target" / "package-mvp")
        env["CARGO_TARGET_DIR"] = str(target_dir)
        home = str(Path.home())
        env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join([MVP_TARGET_FEATURES, f"--remap-path-prefix={home}=~"])
        if not prebuilt:
            run(
                ["cargo", "+nightly", "build", "-p", name, "--target", WASM_TARGET, "--release",
                 "-Z", "build-std=std,panic_abort"],
                env=env,
            )
    profile_subdir = "debug" if (fast and debug) else (PREVIEW_PROFILE if fast else "release")
    built = target_dir / WASM_TARGET / profile_subdir / f"{snake}.wasm"
    if not built.is_file():
        sys.exit(f"error: expected wasm not produced at {built}")

    # 2. wasm-bindgen bundler glue (the shared __wbg_set_wasm seam).
    pkg = tmp / "pkg"
    run(["wasm-bindgen", "--target", "bundler", "--out-dir", str(pkg), str(built)])
    bg_wasm = pkg / f"{snake}_bg.wasm"

    # 3. Fast-path wasm: size-optimized.
    fast_wasm = tmp / f"{snake}_bg.opt.wasm"
    run(binaryen("wasm-opt", "-Oz", str(bg_wasm), "-o", str(fast_wasm)))

    # 4. wasm2js fallback: lower remaining post-MVP ops, then compile to JS, then
    # inline the bare-`env` i64 scratch so the module resolves in a static bundle.
    wasm2js_path: Path | None = tmp / f"{snake}_bg.wasm2js.js"
    if has_fallback:
        lowered = tmp / "mvp_lowered.wasm"
        run(binaryen("wasm-opt", str(bg_wasm), *WASM2JS_LOWERING, "-o", str(lowered)))
        run(binaryen("wasm2js", str(lowered), "-o", str(wasm2js_path)))
        text = wasm2js_path.read_text(encoding="utf-8")
        wasm2js_path.write_text(text.replace(WASM2JS_ENV_IMPORT, WASM2JS_ENV_SHIM, 1), encoding="utf-8")
    else:
        wasm2js_path = None

    glue_js = (pkg / f"{snake}_bg.js").read_text(encoding="utf-8")
    return snake, glue_js, fast_wasm, wasm2js_path


def prebuild_wasm_crates(app_dirs: list[Path], *, target_dir: Path, debug: bool = False) -> None:
    """Compile several apps' wasm cdylibs in ONE ``cargo build`` invocation (the
    fast/preview path only), so the shared engine dependency graph builds once and the
    per-app leaf codegens run together across cores — instead of N serial cargo
    processes that each finish before the next starts. Artifacts land in ``target_dir``;
    each app's bundle is then finished per-app via ``build_bundle(..., prebuilt=True)``.
    The slow MVP ``build-std`` path is not batched here (it stays single-crate)."""
    names = [crate_name(d) for d in app_dirs]
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)
    profile = [] if debug else ["--profile", PREVIEW_PROFILE]
    pkg_flags = [flag for name in names for flag in ("-p", name)]
    print(
        f"  pre-building {len(names)} demo wasm crate(s) in one cargo invocation "
        f"({'debug' if debug else PREVIEW_PROFILE})",
        flush=True,
    )
    run(["cargo", "build", *pkg_flags, "--target", WASM_TARGET, *profile], env=env)


def build_bundle(
    app_dir: Path,
    out: Path,
    *,
    fast: bool = False,
    has_fallback: bool = True,
    target_dir: Path | None = None,
    keep_temp: bool = False,
    debug: bool = False,
    prebuilt: bool = False,
) -> str:
    """Build ONE crate's wasm bundle and drop the loader + companions into ``out/``
    (native root layout: ``axiom-loader.js`` + ``<snake>_bg.{wasm,js[,wasm2js.js]}``).

    Unlike ``package``, this writes NO index.html and copies NO web assets — the
    caller assembles the static site itself. This is the seam the multi-page demo
    gallery (``package_gallery.py``) builds on: one shared bundle, many pages that
    each ``import "./axiom-loader.js"`` and call their own demo's export. ``out`` is
    created if absent and NOT wiped (the caller may have staged the site there
    first). Returns the snake-cased crate name."""
    has_fallback = has_fallback and not fast
    out.mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix=f"axiom-bundle-{crate_name(app_dir).removeprefix('axiom-')}-"))
    try:
        snake, glue_js, fast_wasm, wasm2js_path = _compile_wasm_bundle(
            app_dir, tmp, fast=fast, has_fallback=has_fallback, target_dir=target_dir, debug=debug, prebuilt=prebuilt
        )
        shutil.copy2(fast_wasm, out / f"{snake}_bg.wasm")
        if wasm2js_path is not None:
            shutil.copy2(wasm2js_path, out / f"{snake}_bg.wasm2js.js")
        (out / f"{snake}_bg.js").write_text(glue_js, encoding="utf-8")
        (out / "axiom-loader.js").write_text(loader_source(snake, has_fallback=has_fallback), encoding="utf-8")
        return snake
    finally:
        if keep_temp:
            print(f"  (kept temp build dir: {tmp})")
        else:
            shutil.rmtree(tmp, ignore_errors=True)


def package(
    app_dir: Path,
    *,
    out: Path,
    inline: bool = False,
    has_fallback: bool = True,
    fast: bool = False,
    target_dir: Path | None = None,
    keep_temp: bool = False,
) -> Path:
    """Package one app crate into ``out``. Reusable from package_gallery.py too.

    ``fast=True`` skips the wasm2js fallback and the slow MVP/``build-std`` rebuild:
    it does a no-LTO ``release-preview`` build (reference-types and all), so the engine
    compiles once and links into the app — wasm-only but quick, for tight gallery
    iteration. The output still goes through the same loader, so the page boot path is
    identical.

    The default (``fast=False``) path needs the genuinely-MVP module for wasm2js, so
    it rebuilds std MVP via nightly ``-Z build-std`` into ``target_dir`` (default
    ``target/package-mvp``), shared across apps so std compiles once. Returns ``out``.
    """
    name = crate_name(app_dir)
    snake = name.replace("-", "_")
    if not (app_dir / "web" / "index.html").is_file():
        sys.exit(f"error: {app_dir}/web/index.html not found — not a browser app.")
    has_fallback = has_fallback and not fast
    sdk_hosted = is_sdk_hosted(app_dir, snake)
    if inline and sdk_hosted:
        sys.exit(
            "error: --inline is not supported for SDK-hosted TS apps (their multi-module "
            "harness + vendored SDK + dynamic author import are not foldable into one file). "
            "Use the default directory bundle."
        )
    tmp = Path(tempfile.mkdtemp(prefix=f"axiom-pkg-{name.removeprefix('axiom-')}-"))

    print(f"Packaging {name} -> {out}{'  (fast: wasm-only)' if fast else ''}")
    try:
        # 0. An SDK-hosted app's TypeScript host edge (harness + author module) and its
        # vendored SDK are compiled/built here so the baked bundle matches the dev loop.
        if sdk_hosted:
            prepare_sdk_hosted(app_dir)
        # 1-4. Build the wasm + bundle artifacts (shared with the gallery's
        # build_bundle): cargo build, wasm-bindgen bundler glue, wasm-opt -Oz, and the
        # optional wasm2js fallback.
        snake, glue_js, fast_wasm, wasm2js_path = _compile_wasm_bundle(
            app_dir, tmp, fast=fast, has_fallback=has_fallback, target_dir=target_dir
        )
        wasm2js_js = wasm2js_path.read_text(encoding="utf-8") if wasm2js_path is not None else None

        if out.exists():
            shutil.rmtree(out)
        out.mkdir(parents=True)

        # 5. Emit the bundle in the chosen shape.
        if inline:
            wasm_b64 = base64.b64encode(fast_wasm.read_bytes()).decode("ascii")
            emit_inline_html(app_dir, out, snake, glue_js, wasm2js_js, wasm_b64)
        else:
            # The loader is a drop-in for the wasm-bindgen `--target web` glue. A native
            # app's page is rewritten to import it as the bundle-root `axiom-loader.js`;
            # an SDK-hosted app's compiled harness imports `/pkg/<snake>.js`, so the
            # loader is dropped in at exactly that path (its `_bg.*` companions beside
            # it). Either way the loader + companions live together in one dir.
            loader_dir, loader_name = ((out / "pkg"), f"{snake}.js") if sdk_hosted else (out, "axiom-loader.js")
            loader_dir.mkdir(parents=True, exist_ok=True)
            shutil.copy2(fast_wasm, loader_dir / f"{snake}_bg.wasm")
            if has_fallback:
                shutil.copy2(wasm2js_path, loader_dir / f"{snake}_bg.wasm2js.js")
            (loader_dir / f"{snake}_bg.js").write_text(glue_js, encoding="utf-8")
            (loader_dir / loader_name).write_text(loader_source(snake, has_fallback=has_fallback), encoding="utf-8")
            emit_index_html(app_dir, out, snake, sdk_hosted=sdk_hosted)
        return out
    finally:
        if keep_temp:
            print(f"  (kept temp build dir: {tmp})")
        else:
            shutil.rmtree(tmp, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description="Package one Axiom browser app into a self-contained bundle.")
    parser.add_argument("app", help="app crate dir (apps/axiom-gallery) or short name (gallery)")
    parser.add_argument("--out", help="output dir (default: dist-app/<name>)")
    parser.add_argument("--inline", action="store_true", help="emit a single self-contained index.html")
    parser.add_argument("--no-fallback", action="store_true", help="skip the wasm2js fallback (wasm-only bundle)")
    parser.add_argument(
        "--fast",
        action="store_true",
        help="quick wasm-only build: no-LTO release-preview profile, no MVP/build-std, no wasm2js",
    )
    parser.add_argument(
        "--target-dir",
        help="cargo target dir for the MVP build (default: target/package-mvp, persistent + "
        "shared so std/deps are compiled once across repeated runs and across apps)",
    )
    parser.add_argument("--keep-temp", action="store_true", help="keep the intermediate build dir for inspection")
    args = parser.parse_args()

    app_dir = resolve_app(args.app)
    bundle_id = crate_name(app_dir).removeprefix("axiom-")
    out = Path(args.out).resolve() if args.out else (REPO_ROOT / "dist-app" / bundle_id)
    print("  (MVP build via nightly -Z build-std; slow the first time — it compiles std)")
    package(
        app_dir,
        out=out,
        inline=args.inline,
        has_fallback=not args.no_fallback,
        fast=args.fast,
        target_dir=Path(args.target_dir).resolve() if args.target_dir else None,
        keep_temp=args.keep_temp,
    )

    snake = crate_name(app_dir).replace("-", "_")
    has_fallback = not args.no_fallback and not args.fast
    # SDK-hosted apps keep the loader + wasm in pkg/ (the harness's glue path).
    loc = (out / "pkg") if is_sdk_hosted(app_dir, snake) else out

    def kb(p: Path) -> str:
        return f"{p.stat().st_size / 1024:.0f} KB" if p.is_file() else "—"

    print(f"\npackaged -> {out}")
    if args.inline:
        print(f"  index.html  {kb(out / 'index.html')}  (single self-contained file)")
    else:
        print(f"  index.html            {kb(out / 'index.html')}")
        print(f"  {snake}_bg.wasm       {kb(loc / f'{snake}_bg.wasm')}  (fast path)")
        if has_fallback:
            print(f"  {snake}_bg.wasm2js.js {kb(loc / f'{snake}_bg.wasm2js.js')}  (fallback)")
    print(f"\n  serve with:  uv run --no-project python -m http.server 8000 --directory {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
