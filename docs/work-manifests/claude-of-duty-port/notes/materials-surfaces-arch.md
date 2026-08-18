# Architectural surface generators (concrete, brick, plaster, tile)

**File:** `apps/claude-of-duty/src/materials/surfaces/arch.rs`
**Source:** `C:\dev\Claude-of-Duty\src\materials\glsl\surfaces-arch.js:1-563`
**Tests:** `apps/claude-of-duty/tests/materials_surfaces_arch_port.rs`, 7 passed
**Architecture check:** pass (`cargo xtask check-architecture`, exit 0)

## What was ported

Four `owSurface(uv) -> (albedo, height, roughness, metal, ao)` GLSL bodies,
ported line-for-line as CPU `f64` maths, each returning a
`materials::bake::SurfaceSample`:

- `concrete_surface(uv, seed, param)` — pour/wash banding, exposed-aggregate
  Worley, a 5-8 mm sand fraction, bug holes, formwork board lines + tie-rod
  holes (`param.x`), saw-cut control joints + power-float swirl (`param.y`),
  two crack fields, spalling with a bright rim, 2-5 cm chips, rebar rust
  bleed. Also backs `mod.rs::LIBRARY`'s `concrete_floor` entry — same
  generator, different `param`/`seed` (see the module doc for why this is
  one function, not two).
- `brick_surface(uv, seed)` — running-bond lattice (6 x 18, per-brick hash
  jitter), a raked joint with a hard arris (`smoothstep(J*shoulder, J*1.02,
  …)`, deliberately not a full-width ramp), 5 kiln colour families, broken
  arrises, efflorescence, mortar smear, weathering, hairline cracks.
- `plaster_surface(uv, seed)` — sheared trowel field (`ow_shear`/
  `ow_shear_per`), skim-coat laps (~40 cm passes with a per-lap value shift +
  arris), the documented "0.1-1 m band" (damp bloom / hand-height soiling /
  dirt wash, each contrast-expanded `(n - 0.5) * K + 0.5` before use — a
  4-octave fbm01 only spans ~0.3-0.7), sand tooth, pinholes, hairline
  crazing, structural cracks, blown plaster showing substrate, 6-9 cm chips
  showing browncoat, tide marks, black mould.
- `tile_surface(uv, seed)` — 6x6 grid, flat grout bed with a hard arris,
  per-tile glaze + batch shade, broken/cracked tiles, traffic wear.

Every generator preserves its frequency constants exactly (the "Nyquist
budget" the module doc and the recipe both call out — `p = uv * 8`, so a term
at `p * K` lays `8K` cells across a bake) and clamps albedo/roughness/ao to
the source's own physical-plausibility bounds in its last lines.

## Local helpers, and why they don't live in `noise.rs`/`bake.rs`

`noise.rs` mirrors `noise.js` function-for-function and has no generic
`vec3` algebra (see its own module doc). This file needed a few GLSL
primitives no existing module defines, so they're local, private functions
in `arch.rs` rather than additions to a shared file another concurrent
surfaces-*.rs port might also need to touch:

- `gl_step` — same definition `bake.rs`'s own private `gl_step` uses
  (duplicated per file is the established pattern, not an oversight).
- `v2_abs` — componentwise `vec2 abs`.
- `v3_mix`/`v3_add`/`v3_clamp` — componentwise `vec3` `mix`/`+`/`clamp`
  (`clamp` always to a uniform per-channel bound in this file, so `v3_clamp`
  takes scalar `lo`/`hi` rather than `Vec3`).

Deliberately **not** added: inherent methods on the shared `Vec3` type
(`impl Vec3 { … }` from `arch.rs`). Rust allows multiple `impl` blocks for
one type across files in the same crate, but three other agents are
concurrently porting sibling `surfaces-*.js` files into the same directory
and each would independently reach for the same missing `vec3` ops — a
same-named method added from two files would be a duplicate-definition
compile error neither side could see coming. Free functions scoped to this
module avoid that collision risk entirely.

## `concrete_floor` is `concrete_surface` with different `uParam`

`mod.rs::LIBRARY`'s `concrete` entry (`generator: "concrete"`, `param: [1, 0,
0, 0]`) and `concrete_floor` entry (`generator: "concrete"`, `param: [0, 1,
0, 0]`) both point at the same source generator — confirmed by reading
`mod.rs` before writing any Rust. `concrete_surface` takes the full `uParam`
as a `Vec4` and reads `.x` (`formAmt`)/`.y` (`jointAmt`) exactly as the GLSL
does; `.z`/`.w` are carried but never read, matching the source. `brick`/
`plaster`/`tile` never reference `uParam` in their GLSL bodies at all, so
their Rust signatures only take `uv` and `seed` — no unused parameter
plumbing.

## Golden-capture method, and the caveat this recipe calls out explicitly

`surfaces-arch.js` embeds all four bodies as GLSL inside a JS template
literal (the same shape `noise.js`/`generator.js` use) — **there is no
importable JS function to call as ground truth.**
`tests/materials_arch/capture.mjs` is a from-scratch transcription of the
noise library and all four `owSurface` bodies into plain JS doubles, written
directly against the GLSL source (not against `arch.rs`), then evaluated at
a fixed 6-point uv grid and written to `tests/materials_arch/golden.json`
(committed, reproducible: `node capture.mjs > golden.json`).

**This oracle is weaker than the rest of the port.** A bug made once in the
GLSL -> Rust transcription and made again, identically, in the GLSL -> JS
transcription would agree with itself and pass. The two transcriptions were
written independently (JS written fresh from the GLSL source file, not
copy-adapted from the already-written Rust), which is the best mitigation
available without a real oracle, but it is not a substitute for one. Anyone
extending this file should re-derive suspicious values from the algorithm
(per the recipe's rule: "when a golden disagrees with the port, work out
what the value *should* be before changing either side") rather than trust
either side by default.

`concrete_surface` is pinned three ways: the real `concrete` library params
(seed 11, `param = [1,0,0,0]`), the real `concrete_floor` params (seed 47,
`param = [0,1,0,0]`), and a third seed-11 `param = [0,0,0,0]` variant that
isn't a `LIBRARY` entry but is the only one of the three where `formAmt` and
`jointAmt` are *both* zero — the one combination the real library never
exercises, and the one that actually stress-tests every `* formAmt`/`*
jointAmt` term collapsing to exactly zero rather than multiplying by 1.

Tolerance is `1e-7`, wider than the `1e-12` single-transcendental-call figure
`tests/core_port.rs` established: every sample here chains well over a dozen
`owFbm01`/`owWorley`/`owWarp`/`owCracks`/`owSRGB` calls (each already
carrying `sin`/`cos`/`sqrt`/`pow` libm drift), so the compounded tolerance
needs to be wider than one call's worth — the same reasoning
`materials/bake.rs`'s `1e-6` 9-sample Sobel-stencil test gives, widened
slightly further for chains several times longer. `1e-7` is still many
orders tighter than any texel-visible difference.

A fourth test (`every_generator_stays_within_its_documented_clamp_bounds`)
sweeps a 9x9 uv grid through all five variants (concrete wall, concrete
floor, brick, plaster, tile) and asserts every output stays inside its
generator's own documented clamp bounds — a Rust-only property test (no JS
capture needed: it follows from `gl_clamp`'s definition, not a captured
value), catching any accidental omission of a clamp at the end of a
generator.

## Nothing left un-ported, no source defects found

All four generators are fully ported; no `sign()`, rotation, or other
previously-costly trap (see the port recipe's "Language traps" section)
appears in this file — the only two `mod()` calls (`brick`'s `mod(row, 2.0)`,
`mod(col, COLS)`) go through the existing `gl_mod`, not Rust's `%`. No
behavioral defect was found worth flagging as a preserved source quirk.

## Full crate test suite

`cargo test -p axiom-claude-of-duty` passes 366/368; the 2 failures are in
`materials::surfaces::metal::tests` (`metal.rs`), a sibling agent's
concurrent, uncommitted work on `surfaces-metal.js` in the same directory —
confirmed via `git status` before touching anything, and this file makes no
changes to `metal.rs`. All 7 tests added by this slice pass, and
`cargo xtask check-architecture` exits 0.
