"""Drive the generated wasm/wasm2js loader's capability-detection branches.

Repo tooling, NOT part of the engine dependency graph (same status as the rest of
``e2e/`` and ``scripts/``). This proves the behaviour of the loader templates that
``scripts/package_app.py`` (``loader_source`` / ``loader_source_inline``) bakes into
every packaged bundle — the only place the wasm→wasm2js fallback decision is made.

It imports the *actual* template functions and instantiates the *actual* generated
loader JS in Node with mocked globals, so it tests the shipped code, not a copy:

  * ``no-wasm``            — ``WebAssembly`` absent              → wasm2js fallback + warn
  * ``instantiate-reject`` — API present but module rejected     → wasm2js fallback + warn
  * ``wasm-ok``            — instantiation succeeds              → wasm path, no warn
  * no-fallback bundle, ``no-wasm`` → ``init()`` throws (``--fast`` contract preserved)

The ``instantiate-reject`` case is the regression guard the suite previously lacked:
the old loader took the wasm branch whenever the API merely *existed* and would crash
on a browser that rejects this specific MVP module instead of falling back. Mocks (not
a real browser) are used deliberately — forcing ``WebAssembly`` undefined / rejecting
is the whole point, and it needs no nightly ``-Z build-std`` wasm2js build to run.

Run via ``make loader-test`` (fast, node-only) or as part of ``pytest e2e``.
"""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
SNAKE = "quintet"


def _load_package_app():
    spec = importlib.util.spec_from_file_location("package_app", REPO_ROOT / "scripts" / "package_app.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


package_app = _load_package_app()

# Mock wasm-bindgen glue: records what the loader hands __wbg_set_wasm so the driver
# can prove which arm ran. `export *` in the loader re-exports this lone symbol fine.
GLUE_JS = """export function __wbg_set_wasm(w) { globalThis.__AXIOM_SET_WASM = w; }
"""
# Mock wasm2js module: a marker the driver detects when the fallback arm imports it.
WASM2JS_JS = 'export const __axiom_wasm2js_marker = "wasm2js";\n'

# Node driver: installs the scenario's globals, runs the loader's default init(), and
# emits a JSON verdict. WebAssembly is set on globalThis so `typeof WebAssembly` and
# `WebAssembly.instantiate` resolve to the mock (or undefined) inside the loader.
DRIVER_MJS = """\
const scenario = process.env.SCENARIO;
const warnings = [];
console.warn = (...a) => { warnings.push(a.join(" ")); };
globalThis.fetch = async () => ({ arrayBuffer: async () => new ArrayBuffer(8) });

if (scenario === "no-wasm") {
  globalThis.WebAssembly = undefined;
} else if (scenario === "instantiate-reject") {
  globalThis.WebAssembly = { instantiate: async () => { throw new Error("CompileError: MVP rejected"); } };
} else if (scenario === "wasm-ok") {
  globalThis.WebAssembly = { instantiate: async () => ({ instance: { exports: { __axiom_wasm_marker: "wasm" } } }) };
} else {
  throw new Error("unknown scenario: " + scenario);
}

let threw = null;
try {
  const mod = await import("./axiom-loader.js");
  await mod.default();
} catch (e) { threw = String((e && e.message) || e); }

const w = globalThis.__AXIOM_SET_WASM;
process.stdout.write(JSON.stringify({
  warnings,
  threw,
  usedWasm2js: Boolean(w && w.__axiom_wasm2js_marker === "wasm2js"),
  usedWasm: Boolean(w && w.__axiom_wasm_marker === "wasm"),
}));
"""

WARN_LINE = "Axiom: WebAssembly unavailable — running JS fallback."


def _write_pkg_json(path: Path, name: str | None = None) -> None:
    fields = {"type": "module", "main": "index.js"}
    if name is not None:
        fields["name"] = name
    path.write_text(json.dumps(fields), encoding="utf-8")


def _build_bundle(tmp_path: Path, *, flavor: str, has_fallback: bool) -> None:
    """Materialise a packaged bundle's loader + the modules it imports into tmp_path."""
    _write_pkg_json(tmp_path / "package.json")
    (tmp_path / "driver.mjs").write_text(DRIVER_MJS, encoding="utf-8")

    if flavor == "file":
        loader = package_app.loader_source(SNAKE, has_fallback=has_fallback)
        (tmp_path / f"{SNAKE}_bg.js").write_text(GLUE_JS, encoding="utf-8")
        (tmp_path / f"{SNAKE}_bg.wasm2js.js").write_text(WASM2JS_JS, encoding="utf-8")
    else:  # inline: bare specifiers resolve via node_modules (the import-map analogue)
        loader = package_app.loader_source_inline(SNAKE, "AAA=", has_fallback=has_fallback)
        for pkg, body in (("axiom-glue", GLUE_JS), ("axiom-wasm2js", WASM2JS_JS)):
            pkg_dir = tmp_path / "node_modules" / pkg
            pkg_dir.mkdir(parents=True)
            _write_pkg_json(pkg_dir / "package.json", name=pkg)
            (pkg_dir / "index.js").write_text(body, encoding="utf-8")
    (tmp_path / "axiom-loader.js").write_text(loader, encoding="utf-8")


def _run(tmp_path: Path, scenario: str) -> dict:
    result = subprocess.run(
        ["node", "driver.mjs"],
        cwd=tmp_path,
        env={**_clean_env(), "SCENARIO": scenario},
        capture_output=True,
        text=True,
        encoding="utf-8",  # the warn line carries an em-dash; don't let Windows cp1252 mangle it
    )
    assert result.returncode == 0, f"node driver failed:\n{result.stderr}"
    return json.loads(result.stdout)


def _clean_env() -> dict:
    import os

    return dict(os.environ)


@pytest.fixture(scope="module", autouse=True)
def _require_node():
    if shutil_which("node") is None:
        pytest.skip("node not on PATH — loader fallback test needs Node to run the loader JS")


def shutil_which(name: str) -> str | None:
    import shutil

    return shutil.which(name)


@pytest.mark.parametrize("flavor", ["file", "inline"])
def test_falls_back_when_webassembly_absent(tmp_path: Path, flavor: str) -> None:
    _build_bundle(tmp_path, flavor=flavor, has_fallback=True)
    out = _run(tmp_path, "no-wasm")
    assert out["usedWasm2js"] is True, out
    assert out["usedWasm"] is False, out
    assert out["threw"] is None, out
    assert out["warnings"] == [WARN_LINE], out


@pytest.mark.parametrize("flavor", ["file", "inline"])
def test_falls_back_when_instantiation_fails(tmp_path: Path, flavor: str) -> None:
    # The hardening this suite guards: API present, but THIS module won't instantiate.
    _build_bundle(tmp_path, flavor=flavor, has_fallback=True)
    out = _run(tmp_path, "instantiate-reject")
    assert out["usedWasm2js"] is True, out
    assert out["usedWasm"] is False, out
    assert out["threw"] is None, out
    assert out["warnings"] == [WARN_LINE], out


@pytest.mark.parametrize("flavor", ["file", "inline"])
def test_takes_wasm_path_when_supported(tmp_path: Path, flavor: str) -> None:
    _build_bundle(tmp_path, flavor=flavor, has_fallback=True)
    out = _run(tmp_path, "wasm-ok")
    assert out["usedWasm"] is True, out
    assert out["usedWasm2js"] is False, out
    assert out["threw"] is None, out
    assert out["warnings"] == [], out


@pytest.mark.parametrize("flavor", ["file", "inline"])
def test_no_fallback_bundle_throws_without_webassembly(tmp_path: Path, flavor: str) -> None:
    # `--fast` / `--no-fallback`: no wasm2js shipped, so absence must throw, not warn.
    _build_bundle(tmp_path, flavor=flavor, has_fallback=False)
    out = _run(tmp_path, "no-wasm")
    assert out["threw"] is not None, out
    assert "no JS fallback" in out["threw"], out
    assert out["usedWasm2js"] is False, out
    assert out["usedWasm"] is False, out


@pytest.mark.parametrize("flavor", ["file", "inline"])
def test_no_fallback_bundle_still_runs_wasm(tmp_path: Path, flavor: str) -> None:
    _build_bundle(tmp_path, flavor=flavor, has_fallback=False)
    out = _run(tmp_path, "wasm-ok")
    assert out["usedWasm"] is True, out
    assert out["threw"] is None, out
    assert out["warnings"] == [], out


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-q"]))
