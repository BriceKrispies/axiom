# `weapons/mathx.js` port notes

Source: `C:/dev/Claude-of-Duty/src/weapons/mathx.js:1-230` (whole file).
Target: `apps/claude-of-duty/src/weapons/mathx.rs`.
Test: `apps/claude-of-duty/tests/weapons_mathx_port.rs` (16 tests).

## What was ported

Everything in the file: `TAU`, `DEG`, `clamp`, `clamp01`, `lerp`, `smoothstep`,
`smootherstep`, `easeOutBack` → `ease_out_back`, `easeOutCubic` →
`ease_out_cubic`, `easeInCubic` → `ease_in_cubic`, `easeInOutSine` →
`ease_in_out_sine`, `damp`, `Spring`, `Spring3`, `Noise1`, `wrapPi` →
`wrap_pi`. Function names snake_cased; everything else (constants, formulas,
call order) kept as close to the source as Rust allows.

## Golden capture

Captured by copying a temporary `capture_mathx_tmp.mjs` script into
`C:/dev/Claude-of-Duty/` (so its relative imports of `./src/weapons/mathx.js`
and `./src/core/rng.js` resolved — Node's ESM loader rejects Windows absolute
`C:/...` import specifiers), running it under Node v24, and pasting the
JSON output into the test file as literal expected arrays. The script was
deleted after use; nothing under `C:/dev/Claude-of-Duty` was left behind.

Covered: every function at 5-9 check points, `Spring`/`Spring3` step traces
over a fixed `dt` sequence (including a `0.1` stall step and a mid-sequence
`kick`), `Noise1.at`/`fbm` at 9 fixed inputs (including negative and
out-of-table-range `x`), and the smoothed table's head/tail (recovered via
`at()` at integer points, where the Catmull-Rom spline collapses to the raw
table entry).

Tolerance: exact `f64` equality for anything built only from `+ - * /` and
comparisons (`clamp`, `lerp`, `smoothstep`, the cubic eases, `Spring`'s
semi-implicit integrator, `Noise1.at`'s Catmull-Rom, table construction).
`1e-12` absolute tolerance (the `tests/core_port.rs` figure) wherever
`sin`/`cos`/`exp` appear: `easeInOutSine`, `damp`, `wrapPi`,
`Noise1.fbm`'s frequency arithmetic isn't transcendental but its inputs are —
kept close-compared throughout for consistency with `at()`.

## Source defect ported, not fixed: `Spring3.z`

`Spring3`'s class body declares `get z()` twice: once at `mathx.js:120-122`
returning the damping ratio (`this.a.z`), and again at `mathx.js:153-155`
returning the z-position component (`this.c.x`). In a JS class body the later
declaration wins on the prototype, so the first getter is dead code — reading
`.z` always returns the position. The single `set z(v)` still writes the
damping ratio, so **the getter and setter read/write two entirely different
things under one name.**

This is not a harmless dead branch: `viewmodel.js` (not yet ported) exploits
it directly —

```js
this.recPos.z = r.damping;   // write: damping ratio
...
pz += this.recPos.z;         // read: z-position
```

So the port keeps the split rather than "fixing" it: `Spring3::set_z(&mut
self, v: f64)` sets the damping ratio on all three springs (mirrors the one
JS setter); `Spring3::z(&self) -> f64` returns `self.c.x` (mirrors the
winning getter), grouped with `x()`/`y()` rather than paired with `set_z` so
the split is visible at the call site instead of hidden behind a symmetric-
looking accessor pair. The damping ratio, once set, is still reachable
directly: `spring3.a.z`. Pinned by
`spring3_z_source_quirk_the_getter_and_setter_disagree`.

## Other divergences (all Rust-forced, not behavioral)

- **Default arguments.** JS defaults (`Spring(f=12,z=1,value=0)`,
  `Spring3(f=12,z=1)`, `Noise1(rng,size=512)`, `easeOutBack(t,k=1.6)`,
  `fbm(x,oct=3,gain=0.5)`, `Spring.step(dt,target=this.target)`,
  `Spring3.step(dt,tx=0,ty=0,tz=0)`) become explicit parameters plus a named
  constant (`EASE_OUT_BACK_DEFAULT_K`, `NOISE1_DEFAULT_SIZE`,
  `NOISE1_DEFAULT_OCTAVES`, `NOISE1_DEFAULT_GAIN`) or a `Default` impl
  (`Spring`, `Spring3`). `Spring::step`'s self-referential default
  (`target = this.target`) has no Rust equivalent parameter syntax, so it got
  its own method, `step_to_target(&mut self, dt: f64)`.
- **`Spring3.writeTo`.** The source writes into a caller-supplied
  `{x,y,z}`-shaped object (a THREE.Vector3 in practice). Nothing in this port
  has a shared vector type yet — `viewmodel.js`, the only consumer, is not
  ported — so `write_to` takes three `&mut f64` out-parameters instead of
  inventing a vector type ahead of the code that would need it.
- **`Noise1`'s `Float32Array` table.** Stored as `Vec<f32>` (kept `pub`, like
  the source's plain instance fields `size`/`t`). Every read widens to `f64`
  explicitly (matching a JS `Float32Array` read auto-widening), and the two
  write sites (initial fill, smoothing pass) narrow back to `f32` explicitly
  (matching the `Float32Array` assignment coercion) rather than letting the
  whole table upgrade to `f64` precision, which would have been a silent
  behavior change.

## Not portable from this file alone

Nothing — the file is self-contained pure math with no Three.js or DOM
contact. `viewmodel.js` (the consumer) is out of scope for this port.
