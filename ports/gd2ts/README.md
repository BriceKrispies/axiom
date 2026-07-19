# gd2ts — GDScript → JavaScript transpiler

Emits **pure JS (no wasm, no WebGL)** from the disciplined, typed GDScript subset
the Axiom Godot ports are written in. The point: for a locked-down browser that
may block wasm and/or WebGL (e.g. Citrix), Godot's wasm engine can't run — but the
*game* can, if we transpile the GDScript to JS and render on a Canvas2D host. The
GDScript stays the single source of truth (you keep authoring in Godot); the JS
build is derived from it.

## Status

**Phase 1 — proven.** The determinism keystone is done: the transpiled `hash01`
is **bit-identical** to the original TS `hash01` across 40,000 cases
(`node verify.mjs`). If the deterministic hash survives the transpile, a JS build
of the game replays exactly like the Godot build.

```sh
python gd2ts.py ../godot-home-run/scripts/math_util.gd > out/math_util.mjs
cp runtime.mjs out/ && node verify.mjs   # PASS: 40000 cases, 0 mismatches
```

## How it works

- **`runtime.mjs`** — the JS shim reproducing the Godot built-ins the ports use
  (`Vector2/3`, `Quaternion`, `Color`, `cos`/`clampf`/`minf`/…, and the 32-bit
  integer helper `imul32`). Transpiled modules `import * as gd` from it.
- **`gd2ts.py`** — tab-indent-aware lexer (with bracket-continuation), a Pratt
  parser for the subset, light annotation-driven type tracking, and an ESM
  emitter.

Two real semantic gaps, both handled:

1. **64-bit ints.** GDScript ints are 64-bit; JS numbers are float64. The
   transpiler lowers integer `*` to `Math.imul` and normalizes bitwise/shift
   results with `>>> 0` (and `>>` → `>>>`), so 32-bit hash math is exact.
2. **No operator overloading.** Typed `Vector`/`Quaternion` `+ - * /` lower to
   `.add/.sub/.mul/.div` using the tracked types.

## Roadmap

- **Phase 1 (done):** scalar/int/vector expression subset, static-function
  modules, `hash01` verified bit-identical.
- **Phase 2:** classes (instance state, methods, `extends`), `preload` → `import`,
  `match` → `switch`, string `%` formatting, `null` sentinels, and the full sim
  (`swing`/`pitch`/`ball`/`fielders`/`swing_outcome`/`cinematic`/`session`).
  Verify by running the whole session in Node and matching the seed-1 outcome
  the Godot build produces (SLOW BALL → STRIKE).
- **Phase 3:** the presentation/rasterizer subset — `Transform3D`/`Basis`/
  `Projection`/`Packed*` shims + a Canvas2D host (mapping
  `canvas_item_add_triangle_array` to a real 2D canvas), plus input/loop/audio —
  yielding a single self-contained `.html` that runs the game with no wasm and no
  WebGL.

`out/` is generated (gitignored).
