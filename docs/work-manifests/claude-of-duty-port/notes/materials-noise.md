# Noise library port

**File:** `apps/shmup/src/materials/noise.rs`
**Source:** `C:\dev\Claude-of-Duty\src\materials\glsl\noise.js:1-218` (the whole embedded `NOISE_GLSL` body)
**Tests:** `apps/shmup/tests/materials_noise_port.rs`, 24 passed
**Architecture check:** pass

## What was ported

The tileable procedural noise library every one of the 19 surface generators (`crates/claude-of-duty/src/materials/mod.rs::LIBRARY`) is built on. It is ported as **CPU-evaluable `f64` maths**, not as a WGSL string — this is a reference implementation, produced so a later WGSL emitter can be pinned against it, mirroring how `rng.rs` is the reference `rng.js` is pinned against.

Every function from the source:

- Hashes: `owHash11`, `owHash12`, `owHash22`, `owHash32`, `owHash42` — sin-free (Dave Hoskins style).
- Gradient/value noise: `owGrad2`, `owNoise`, `owNoise01`, `owValue`.
- fbm family: `owFbm`, `owFbm01`, `owRidged`, `owBillow` (each capped at the GLSL loop's compile-time bound of 10 octaves).
- Domain warp: `owWarp`.
- Worley/Voronoi: `owWorley` (F1/F2 + cell hash), `owVoronoiEdge` (Quilez two-pass edge distance).
- Composite: `owCracks` (warped Voronoi edges, thinned + broken by an fbm mask).
- Utilities: `owSat`, `owSat3`, `owRemap`, `owRot`, `owSRGB`, `owShear`, `owShearPer`, `owScratches`.

`mod noise;` was added to `apps/shmup/src/materials/mod.rs` (re-read immediately before editing per the concurrency warning; the file was unchanged from commit `e271b214` at edit time, so the edit applied cleanly on the first try).

## Golden-capture method

A Node script (`.mjs`, deleted after use per the recipe) transcribed the GLSL body to plain JS doubles function-for-function and evaluated it over a fixed grid of points, printing `JSON.stringify`. Those values are pinned in `tests/materials_noise_port.rs` as `expected` constants:

- **Exact equality** for the hashes (`owHash11/12/22/32/42`), `owSat`/`owSat3`/`owRemap`, and `owShear`/`owShearPer` — built only from `+ - * fract`/`clamp`/`min`/`max`, no transcendentals.
- **`1e-12` tolerance** for everything touching `sin`/`cos`/`sqrt`/`pow`: `owGrad2`, `owNoise`/`owNoise01`/`owValue`, the fbm family, `owWarp`, `owWorley`, `owVoronoiEdge`, `owCracks`, `owRot`, `owSRGB`, `owScratches`.

The capture script itself had one bug worth recording (not a source defect — my transcription error): the first draft of `owHash12` in JS used the `vec2` helpers (`scale`/`fract2`) on a 3-component object instead of the `vec3` helpers (`scale3`/`fract3`), silently dropping the `z` component and producing `NaN` downstream through every caller of `owHash12` (`owGrad2`, `owValue`, and transitively `owNoise`/fbm/warp/cracks/scratches). Caught by the capture script printing `null` (JSON's `NaN`) everywhere; fixed before any value was pinned.

## The periodicity property, precisely

The recipe called periodicity "the critical property." Capturing values against JS made the exact mathematical condition precise rather than just "textures don't visibly seam":

**`f(p) == f(p + per)` holds bit-exactly only when `per`'s components are integers.** `floor(p + per) == floor(p) + per` requires `per ∈ ℤ`; this matches the source's own description of `per` as "period, in lattice cells" — every real call site passes an integer cell count. Verified for `owNoise`, `owValue`, `owFbm`/`owFbm01`/`owRidged`/`owBillow` (every octave row, including `oct = 12` which exercises the 10-cap), `owWorley`, `owVoronoiEdge` with `per = (8, 6)`.

`owWarp` is periodic in the **affine** sense appropriate to a displacement field: `warp(p + per) == warp(p) + per`, not `warp(p + per) == warp(p)`. Pinned and tested that way.

Two functions need a stronger condition than "integer per," discovered by a periodicity test that initially failed and had to be root-caused rather than loosened:

- **`owCracks`** rescales `per` by `1.7` for its break-up mask (`owFbm01(p * 1.7 + 11.3, per * 1.7, 4, 0.55)`). Its exact periodicity additionally needs `per * 1.7` to be integer. Tested with `per = (10, 10)` (`10 * 1.7 = 17`). With the more "natural" `per = (8, 6)` used for the other pins, `per * 1.7 = (13.6, 10.2)` is not integer and the mask term is measurably non-periodic (differences of several tenths, not rounding noise) — this is real, not a bug in the test.
- **`owScratches`** (via `owShear`) needs `per` **square** (`per.x == per.y`) as well as the source-documented "`k` and `stretch` must be integers." The shear mixes `per.y * k` into the x-shift; landing back on an exact lattice point of the sheared coordinate system requires `per.y * k` to be an integer multiple of `per.x`, which is automatic when `per.x == per.y` and both `k`/`stretch` are integers, and not otherwise. `per = (8, 6)` (used for the plain value pin) does **not** tile exactly under a bare `p -> p + per` shift; `per = (8, 8)` does — proven in `scratches_is_periodic_under_a_square_per`.

Neither is treated as a source defect per the recipe's rule 7 ("if fixing is clearly right, fix it; otherwise pin the behaviour"): both trade a very slightly non-seamless internal term for visual variety (an incommensurate mask frequency; an anisotropic streak direction), the effect is imperceptible at the texel scale these generators run at, and the source's own comment on `owShear` already flags half of the constraint. The port makes the *full* condition explicit in the module doc and pins it exactly rather than loosening the test to "close enough."

## One genuine source quirk preserved: `owRot`'s rotation direction

`owRot(p, a)` builds `mat2(c, -s, s, c) * p`. GLSL's `mat2(x0, y0, x1, y1)` constructor is **column-major** — column 0 is `(x0, y0) = (c, -s)`, column 1 is `(x1, y1) = (s, c)` — and `m * v = v.x * column0 + v.y * column1`, giving:

```
(c*p.x + s*p.y, c*p.y - s*p.x)
```

which is a **clockwise** rotation for positive `a`, not the counter-clockwise `(c*p.x - s*p.y, s*p.x + c*p.y)` the same-looking arguments would give under a row-major reading (the mistake is an easy one — `mat2(c, -s, s, c)` visually resembles the textbook row-major rotation matrix). `owRot` has no callers within `noise.js` itself, so this is dormant until a future surface generator uses it; ported and tested as-is (`rot_matches_the_column_major_glsl_matrix`), not "corrected," since it's the behaviour any future caller will actually see.

## Design notes

- **No shared vector type exists in this crate** (see `weapons/ballistics.rs`'s local `Vec3` for the established precedent), so `noise.rs` defines its own minimal `Copy` `Vec2`/`Vec3`/`Vec4` with exactly the operations these 24 functions use — no generic swizzle system. Every GLSL swizzle (`p.xyx`, `p3.yzx`, `p4.wzxy`, …) is expanded inline at its one call site instead, so each function stays diffable against the source line-for-line.
- **`gl_fract`/`gl_mod`** are explicit helpers, not Rust's `f64::fract`/`%` — both keep the sign of `x` for negative inputs, which would silently break periodicity for any negative lattice coordinate. This is the same class of care the recipe's `mathx.js` port took with its `b - a || 1e-6` guard.
- **`f64` throughout**, not `f32`. Unlike the rest of this port (matching a JS `number`, itself an `f64`), there is no JS-precision precedent to match here — the source is *shader* code, and GLSL `float` is 32-bit. Kept in `f64` deliberately so the golden tolerance (`1e-12`) is fighting only libm cross-implementation drift, not `f32` rounding on top of it; the WGSL emission workstream is where `f32` truncation becomes its own explicit, separately-tested concern.
- **`owSRGB`'s `mix(a, b, step(edge, x))` ported as `if`/`else`.** `step` is boolean-valued (exactly `0.0` or `1.0`), so `mix` selects one side exactly — the two forms are bit-identical, and apps are outside the Branchless Law, so nothing forces the arithmetic form.

## Nothing left un-ported

All 24 functions in `noise.js` (5 hashes + `owGrad2`/`owNoise`/`owNoise01`/`owValue` + 4 fbm-family + `owWarp` + `owWorley`/`owVoronoiEdge`/`owCracks` + 8 utilities) are ported. WGSL emission is out of scope per the task brief — this is the CPU reference implementation that emission work will be pinned against.
