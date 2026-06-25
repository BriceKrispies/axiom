#!/usr/bin/env python3
"""Package a single Axiom browser app into a self-contained, droppable bundle.

Repo tooling (alongside ``assemble_gallery.py`` and the Makefile), NOT part of the
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

Usage:
    uv run --no-project python scripts/package_app.py <app> [--out DIR] [--inline]

``<app>`` is an app crate dir (``apps/axiom-quintet``) or a short name (``quintet``).
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


def crate_name(app_dir: Path) -> str:
    with (app_dir / "Cargo.toml").open("rb") as f:
        return tomllib.load(f)["package"]["name"]


def loader_source(snake: str, *, has_fallback: bool) -> str:
    """The capability-detecting loader: a drop-in for the old `--target web` glue."""
    fallback = (
        f"""    console.warn("Axiom: WebAssembly unavailable — running JS fallback.");
    const wasm2js = await import("./{snake}_bg.wasm2js.js");
    bg.__wbg_set_wasm(wasm2js);"""
        if has_fallback
        else """    throw new Error("Axiom: WebAssembly unavailable and this bundle ships no JS fallback.");"""
    )
    return f"""// Generated by scripts/package_app.py — do not edit.
// Capability-detecting loader: real wasm where supported, wasm2js fallback otherwise.
// API-compatible with the old wasm-bindgen `--target web` glue: `await init()` then
// call the app's exports (re-exported below). Pass nothing — any argument is ignored.
import * as bg from "./{snake}_bg.js";
export * from "./{snake}_bg.js";

let booted = false;

export default async function init() {{
  if (booted) return bg;
  const hasWasm =
    typeof WebAssembly === "object" && typeof WebAssembly.instantiate === "function";
  if (hasWasm) {{
    const url = new URL("./{snake}_bg.wasm", import.meta.url);
    const bytes = await (await fetch(url)).arrayBuffer();
    const {{ instance }} = await WebAssembly.instantiate(bytes, {{ "./{snake}_bg.js": bg }});
    bg.__wbg_set_wasm(instance.exports);
  }} else {{
{fallback}
  }}
  booted = true;
  return bg;
}}
"""


def loader_source_inline(snake: str, wasm_b64: str, *, has_fallback: bool) -> str:
    """A single-file loader: wasm bytes embedded; cross-module specifiers are the bare
    names the inline import map (data: URLs) resolves. Bare specifiers always consult
    the realm's import map regardless of the importing module's base URL — which is
    why this works from inside a data: URL module."""
    fallback = (
        """    console.warn("Axiom: WebAssembly unavailable — running JS fallback.");
    const wasm2js = await import("axiom-wasm2js");
    bg.__wbg_set_wasm(wasm2js);"""
        if has_fallback
        else """    throw new Error("Axiom: WebAssembly unavailable and this bundle ships no JS fallback.");"""
    )
    return f"""// Generated by scripts/package_app.py --inline — do not edit.
import * as bg from "axiom-glue";
export * from "axiom-glue";

let booted = false;

export default async function init() {{
  if (booted) return bg;
  const hasWasm =
    typeof WebAssembly === "object" && typeof WebAssembly.instantiate === "function";
  if (hasWasm) {{
    const bytes = await (await fetch("data:application/wasm;base64,{wasm_b64}")).arrayBuffer();
    const {{ instance }} = await WebAssembly.instantiate(bytes, {{ "./{snake}_bg.js": bg }});
    bg.__wbg_set_wasm(instance.exports);
  }} else {{
{fallback}
  }}
  booted = true;
  return bg;
}}
"""


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


def emit_index_html(app_dir: Path, out: Path, snake: str) -> None:
    """Copy the app's web/ shell (minus pkg/) and rewire index.html to the loader."""
    _copy_extra_web_assets(app_dir, out)
    html = (app_dir / "web" / "index.html").read_text(encoding="utf-8")
    # The only change an app's page needs: import the loader instead of the web-target
    # glue, and point the (now loader-ignored) wasm URL at the bundle root.
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


def main() -> int:
    parser = argparse.ArgumentParser(description="Package one Axiom browser app into a self-contained bundle.")
    parser.add_argument("app", help="app crate dir (apps/axiom-quintet) or short name (quintet)")
    parser.add_argument("--out", help="output dir (default: dist-app/<name>)")
    parser.add_argument("--inline", action="store_true", help="emit a single self-contained index.html")
    parser.add_argument("--no-fallback", action="store_true", help="skip the wasm2js fallback (wasm-only bundle)")
    parser.add_argument("--keep-temp", action="store_true", help="keep the intermediate build dir for inspection")
    args = parser.parse_args()

    app_dir = resolve_app(args.app)
    name = crate_name(app_dir)
    snake = name.replace("-", "_")
    bundle_id = name.removeprefix("axiom-")
    if not (app_dir / "web" / "index.html").is_file():
        sys.exit(f"error: {app_dir}/web/index.html not found — not a browser app.")

    out = Path(args.out).resolve() if args.out else (REPO_ROOT / "dist-app" / bundle_id)
    has_fallback = not args.no_fallback
    tmp = Path(tempfile.mkdtemp(prefix=f"axiom-pkg-{bundle_id}-"))

    print(f"Packaging {name} -> {out}")
    print(f"  (MVP build via nightly -Z build-std; this is a full rebuild and is slow the first time)")

    try:
        # 1. MVP build, std included, reference-types off, build paths anonymized.
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(tmp / "target")
        home = str(Path.home())
        env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(
            [MVP_TARGET_FEATURES, f"--remap-path-prefix={home}=~"]
        )
        run(
            [
                "cargo", "+nightly", "build", "-p", name,
                "--target", WASM_TARGET, "--release",
                "-Z", "build-std=std,panic_abort",
            ],
            env=env,
        )
        built = tmp / "target" / WASM_TARGET / "release" / f"{snake}.wasm"
        if not built.is_file():
            sys.exit(f"error: expected wasm not produced at {built}")

        # 2. wasm-bindgen bundler glue (the shared __wbg_set_wasm seam).
        pkg = tmp / "pkg"
        run(["wasm-bindgen", "--target", "bundler", "--out-dir", str(pkg), str(built)])
        bg_wasm = pkg / f"{snake}_bg.wasm"

        # 3. Fast-path wasm: size-optimized (into tmp; placed below by the chosen mode).
        fast_wasm = tmp / f"{snake}_bg.opt.wasm"
        run(binaryen("wasm-opt", "-Oz", str(bg_wasm), "-o", str(fast_wasm)))

        # 4. wasm2js fallback: lower remaining post-MVP ops, then compile to JS.
        wasm2js_path = tmp / f"{snake}_bg.wasm2js.js"
        if has_fallback:
            lowered = tmp / "mvp_lowered.wasm"
            run(binaryen("wasm-opt", str(bg_wasm), *WASM2JS_LOWERING, "-o", str(lowered)))
            run(binaryen("wasm2js", str(lowered), "-o", str(wasm2js_path)))

        # Fresh output dir.
        if out.exists():
            shutil.rmtree(out)
        out.mkdir(parents=True)

        glue_js = (pkg / f"{snake}_bg.js").read_text(encoding="utf-8")
        wasm2js_js = wasm2js_path.read_text(encoding="utf-8") if has_fallback else None

        # 5. Emit the bundle in the chosen shape.
        if args.inline:
            wasm_b64 = base64.b64encode(fast_wasm.read_bytes()).decode("ascii")
            emit_inline_html(app_dir, out, snake, glue_js, wasm2js_js, wasm_b64)
        else:
            shutil.copy2(fast_wasm, out / f"{snake}_bg.wasm")
            if has_fallback:
                shutil.copy2(wasm2js_path, out / f"{snake}_bg.wasm2js.js")
            (out / f"{snake}_bg.js").write_text(glue_js, encoding="utf-8")
            (out / "axiom-loader.js").write_text(loader_source(snake, has_fallback=has_fallback), encoding="utf-8")
            emit_index_html(app_dir, out, snake)

        def kb(p: Path) -> str:
            return f"{p.stat().st_size / 1024:.0f} KB" if p.is_file() else "—"

        print(f"\npackaged {name} -> {out}")
        if args.inline:
            print(f"  index.html  {kb(out / 'index.html')}  (single self-contained file)")
        else:
            print(f"  index.html            {kb(out / 'index.html')}")
            print(f"  {snake}_bg.wasm       {kb(out / f'{snake}_bg.wasm')}  (fast path)")
            if has_fallback:
                print(f"  {snake}_bg.wasm2js.js {kb(out / f'{snake}_bg.wasm2js.js')}  (fallback)")
        print(f"\n  serve with:  uv run --no-project python -m http.server 8000 --directory {out}")
        return 0
    finally:
        if args.keep_temp:
            print(f"  (kept temp build dir: {tmp})")
        else:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
