# Port notes: `world::noise` and `world::masks`

Source: `C:/dev/Claude-of-Duty/src/world/util.js` (~1119 lines).
Targets: `apps/shmup/src/world/noise.rs`, `apps/shmup/src/world/masks.rs`.
Commit: `25dcf85c`.

## What was ported

### `noise.rs` — the positional noise basis (`util.js:28-77`)

- `hash3(x, y, z)` — deterministic 3D value hash in `[0,1)`, built from
  `Math.round` + `Math.imul` + 32-bit XOR/shift, ported to `u32` wrapping
  arithmetic.
- `fade(t)` — the private smoothstep polynomial `noise3` interpolates with.
  Kept private, matching the source (not exported by `util.js`).
- `noise3(x, y, z)` — trilinear-interpolated value noise over the `hash3`
  lattice.
- `fbm3(x, y, z, octaves)` — fractal Brownian summation of `noise3` octaves.
  The source defaults `octaves = 3`; Rust has no default arguments, so the
  port adds `FBM3_DEFAULT_OCTAVES: u32 = 3` and callers wanting the source's
  default pass it explicitly (same pattern as `Spring`/`EASE_OUT_BACK_DEFAULT_K`
  in `weapons/mathx.rs`).

All three are **position-deterministic** — pure functions of `(x, y, z)`, no
RNG stream — which is exactly why editing one builder's geometry cannot
reshuffle another's wear pattern: neither ever drew from `Rng`.

### `masks.rs` — the vertex-mask convention + operations (`util.js:9-16, 203-240`)

- Ported the `[r = edge wear, g = grime, b = extra AO]` convention as a module
  doc comment (source `util.js:9-16`).
- `paint_masks(geo, paint)` — ported from `paintMasks` (`util.js:205-227`):
  rewrites every vertex's mask from a callback `(x, y, z, nx, ny, nz, out, i)`
  that reads the current mask via `out` and mutates it in place.
- `fill_masks(geo, w, g, a)` — ported from `fillMasks` (`util.js:230-240`):
  writes one uniform triple into every vertex's mask. Source defaults
  `w = g = a = 0`; ported as `FILL_MASKS_DEFAULT: [f32; 3]` for the same
  no-default-args reason as `fbm3`.
- `MaskGeometry` — **new type, not in the source.** A minimal position/normal/
  mask vertex carrier (three index-aligned `Vec<[f32; 3]>`s) so `paint_masks`
  and `fill_masks` have something to operate on. See "What was deliberately
  left out" below for why it's this narrow.

## Golden-capture method used

Per the port recipe: wrote a temporary Node script (copied into
`C:/dev/Claude-of-Duty/` so its relative `import` resolved, then deleted after
running — `node --version` was v24.15.0, deps already installed) that called
`hash3`/`noise3`/`fbm3` from the real `util.js` over 10 points spanning zero,
unit, fractional, large-magnitude, negative and irrational inputs, and printed
`toPrecision(17)`. Those exact strings are the `expected` arrays in
`noise.rs`'s test module (`hash3_matches_the_javascript_exactly`,
`noise3_matches_the_javascript_exactly`,
`fbm3_default_octaves_matches_the_javascript_exactly`, plus `octaves = 1` and
`octaves = 5` cases exercising the loop bounds).

**Tolerance: exact equality, not `1e-12`.** Every operation in `hash3` is
32-bit integer arithmetic (`Math.round`, `Math.imul`, XOR, shift) — no
transcendental. `noise3`'s `fade` is `+ - *` only. `fbm3` adds `+ - * /` only
(no `sin`/`cos`/`ln`/`exp`/`pow`/`sqrt` anywhere in this call chain). The port
recipe's own rule is "exact equality for anything integer-derived or built
only from `+ - * /`", so all four test functions assert `==`, not
`assert_close`. (Contrast with `rng.rs`'s `gauss()`, which does need the
`1e-12` tolerance because Box–Muller uses `ln`/`sqrt`/`sin`/`cos` — this code
has no such path.)

`masks.rs` has no numeric constants to pin against the JavaScript (its two
functions are structural: iterate, read, call back, write) — its tests instead
pin the *shape* of the ported behavior (order of arguments to the callback,
read-before-write semantics, panic on mismatched lengths) with hand-written
assertions rather than a JS capture.

## Divergences from the source, and why

1. **`Math.round` half-up vs Rust `f64::round` half-away-from-zero.**
   `Math.round(-1.5) === -1` but `(-1.5_f64).round() == -2.0` in Rust. Added a
   private `round_half_up(v) = (v + 0.5).floor()` helper that reproduces the
   JS rule exactly at every `.5` boundary; pinned directly in
   `round_half_up_matches_javascripts_math_round_at_the_half_boundary`, not
   just indirectly through `hash3`.
2. **`f64 as i32` saturates; JS `ToInt32` wraps mod 2^32.** Only diverges once
   `|v| >= 2^31`, which `hash3`'s `x * 1013`/`y * 1619`/`z * 31337` would need
   a world coordinate with `|coord| >~ 68,000` to reach — never happens in this
   level. Documented at the `round_half_up_bits` call site rather than adding
   unreachable wraparound-handling code.
3. **`fbm3`'s and `fillMasks`'s defaulted arguments.** Rust has no default
   arguments (same divergence class as `rng.rs`'s `Rng::new`/`Rng::default`
   and `weapons/mathx.rs`'s `Spring`). Added `FBM3_DEFAULT_OCTAVES` and
   `FILL_MASKS_DEFAULT` constants naming the source's default explicitly.
4. **`MaskGeometry` is new.** The source's `paintMasks`/`fillMasks` operate
   directly on a `THREE.BufferGeometry`'s `position`/`normal`/`color`
   attributes, lazily computing normals and lazily allocating the color
   attribute if absent. There is no Rust geometry type yet for them to operate
   on (see below), so `MaskGeometry` is a narrow carrier — exactly the three
   columns these two functions touch, always fully populated (no lazy
   normal-computation or lazy color-attribute allocation, since a `Vec` has no
   "attribute absent" state to lazily fill). `MaskGeometry::new` asserts
   `positions.len() == normals.len()` rather than silently trusting it, which
   the source can't do (typed-array `.count` fields are just read, never
   cross-checked).

## What was deliberately NOT ported (and why)

Per the task's explicit scope line: **everything that builds `THREE.Shape` /
`THREE.ExtrudeGeometry`**, plus every geometry *builder* that merely *consumes*
the noise basis or the mask convention — all of it belongs with the geometry
back end, a separate workstream (the Assembler port), not this one:

- `wallPanel` (`util.js:451-514`) and `holePath` (`util.js:392-439`) — the
  real-holes wall system, `THREE.Shape`/`ExtrudeGeometry`.
- `polyPrism` (`util.js:622-639`) — also `ExtrudeGeometry`.
- `Accum` (`util.js:103-201`) — the geometry-merging accumulator; needs a real
  indexed vertex/index buffer type, not `MaskGeometry`.
- `chamferBox`, `plainBox`, `quad` (`util.js:267-384`) — box/plane geometry
  builders.
- `weatherProp` (`util.js:246-260`) — the `paintMasks` *caller* that bakes
  wear/grime/AO from `fbm3` + bounding-box height; left out because it needs a
  real geometry's bounding box, which `MaskGeometry` deliberately doesn't
  carry.
- `runoffStreak`, `solidSlabs`, `patchGeometry`, `driftBerm`, `rockGeometry`,
  `clothGeometry`, `catenaryTube`, `sackGeometry`, `tubeY`, `warpGeometry`,
  `disposeAll` (`util.js:517-1119`) — every other geometry builder in the
  file.
- `trs`/`newTrs` (`util.js:79-96`) — matrix composition helpers, unrelated to
  noise or masks; not part of this task's two named targets.

None of these were touched, edited, or stubbed. `MaskGeometry` is explicitly
documented (in `masks.rs`'s module doc comment) as a stand-in to be re-pointed
at the real geometry type once the Assembler port lands it — this port does
not attempt to guess that type's final shape.

## Verification

- `cargo test -p axiom-shmup --lib world::` — 28 passed (10 new noise
  tests, 8 new mask tests, plus the pre-existing 10 in `layout`/`palette`).
- `cargo test -p axiom-shmup --lib --test core_port` — 53 passed
  (unaffected, confirming no regression to the deterministic core).
- `cargo test -p axiom-shmup --test audio_port` has 11 pre-existing
  failures — untracked (`git status` shows `apps/shmup/src/audio/`
  and `tests/audio_port.rs` as `??`), owned by a concurrent agent per this
  task's concurrency warning, and touching nothing this port changed. Left
  alone.
- `cargo xtask check-architecture` — exit 0, `OK: all layers satisfy the Axiom
  Layer Law.` (This is an app-tier addition; the checker's output is about the
  crate/layer graph, and confirms adding two private `apps/` submodules
  disturbed nothing there.)
