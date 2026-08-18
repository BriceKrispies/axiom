# Procedural texture forge: the bake pipeline and the curvature mask bake

**Files:**
`apps/shmup/src/materials/bake.rs`
`apps/shmup/src/materials/masks.rs`
(`apps/shmup/src/materials/mod.rs` gained `pub mod bake;` / `pub mod masks;`)

**Source:**
`C:\dev\Claude-of-Duty\src\materials\generator.js:1-393` (`TextureForge`)
`C:\dev\Claude-of-Duty\src\materials\masks.js:1-234` (`bakeMasks`, `setMask`)

**Tests:** 24 unit tests across the two files, all passing.
**Architecture check:** pass (`cargo xtask check-architecture`, exit 0).
**Full crate test suite:** pass, except three pre-existing failures in
`tests/weapons_models_port.rs` — untracked, concurrent work from another agent
in `src/weapons/models/` (unrelated to this slice; confirmed via `git status`
before touching anything).

## What was ported

### `bake.rs` — `generator.js`'s `TextureForge` bake pipeline, as a CPU bake

The GPU plumbing (`THREE.WebGLRenderTarget`, the orthographic full-screen
triangle, `ShaderMaterial` uniforms, the render-target save/restore dance in
`TextureForge.build`) has no CPU analogue and isn't ported — this reproduces
the *bake*, not the renderer driving it, per the task brief. What *is* ported:

- **The `owSurface` contract** (`SurfaceSample`, `SurfaceFn`): `uv -> (albedo,
  height, roughness, metal, ao)`.
- **`bake()`** (`TextureForge.build`, `generator.js:260-321`): builds the
  albedo texture (`rgb` = albedo, sRGB-encoded unless `linear_albedo`; `a` =
  height), the optional ORM texture, and the optional Sobel-derived normal —
  skipping the whole height pass when no normal is wanted, exactly as the
  source's comment says ("the height pass exists only to feed the Sobel, so
  it is skipped with it").
- **`sobel()`** (`SOBEL`, `generator.js:53-78`): the 3x3 kernel,
  per-texel-to-per-tile slope conversion (`strength = relief / worldSize`),
  and `RepeatWrapping` neighbour addressing at the tile edge.
- **The sRGB encode** the hardware performs on write to an `SRGBColorSpace`
  render target (`generator.js:276`) — there's no GLSL source for this (the
  shader only ever writes linear numbers), so `linear_to_srgb` is the standard
  IEC 61966-2-1 encode, pinned as the algebraic inverse of the already-ported
  `noise::ow_srgb` decode (a round-trip test) plus the well-known reference
  constant (linear `0.5` -> sRGB `~0.7354`).
- **`detail_surface`** (`DETAIL_SRC`, `generator.js:91-120`) and
  **`macro_surface`** (`MACRO_SRC`, `generator.js:126-138`) — the only two
  `owSurface` bodies `generator.js` itself defines (every per-material
  generator — concrete, brick, … — lives in sibling `glsl/surfaces-*.js`
  files, out of scope for this task).
- **`build_detail`/`build_macro`** (`TextureForge.buildDetail`/`buildMacro`,
  `generator.js:344-381`) with their exact default sizes/seeds/world scales
  and `orm`/`normal`/`linearAlbedo` flags.

One new GLSL builtin needed a Rust helper that doesn't belong in `noise.rs`
(which mirrors `noise.js` function-for-function): `gl_step` — GLSL's `step`,
used directly by `DETAIL_SRC`, not one of `noise.js`'s named functions. Lives
in `bake.rs`.

### `masks.rs` — `masks.js`'s curvature bake

- **`bake_masks`**: position-clustering (quantise to `round(p * 8192)`, hash,
  bucket) so hard-edged kit geometry's per-face-duplicated vertices still see
  cross-seam adjacency; per-cluster accumulation of summed normal and summed
  unit-offset-to-neighbours; `curve = dot(avgNormal, sumOffsets) / (|n| *
  hits)` (negative = convex, positive = concave) and `spread = 1 - |sum
  normal| / count`; final per-*vertex* wear/grime/AO combining the cluster's
  curve/spread with that vertex's own stored normal (the up/down bonus) and,
  optionally, an `Rng` jitter.
- **`set_mask`**: uniform fill, no curvature.

Takes `&crate::weapons::geometry::Geo` rather than a new geometry carrier —
see "Design notes" below for why that's not a shortcut.

## Golden-capture method

Two Node scripts (`.mjs`, deleted after use per the recipe):

1. **`bake.rs`**: a self-contained transcription of `noise.js` + `generator.
   js`'s `DETAIL_SRC`/`MACRO_SRC` to plain JS doubles (same discipline as
   `tests/materials_noise_port.rs`), evaluated at:
   - 5 fixed `uv` points (pins `detail_surface`/`macro_surface` directly,
     `1e-9` tolerance — these chain `owFbm01`/`owWorley`/`owScratches`/`owWarp`,
     each already carrying `sin`/`cos`/`sqrt` drift).
   - A full 6x6 `build_detail` tile end-to-end (uv-per-texel through the
     whole pipeline including the Sobel), `1e-6` tolerance — compounding 9
     upstream `owSurface` evaluations per Sobel-derived normal texel widens
     the tolerance class beyond a single point sample, still far tighter than
     any visible difference. (A first attempt used a 4x4 tile; `P = 8`
     evenly divides 4, so every texel center landed on an exact noise-lattice
     integer and `owNoise`/`owFbm` degenerated to exactly `0` almost
     everywhere — a real, faithfully-reproduced property of the noise, not a
     bug, but a poor stress test. Switched to 6x6, which isn't commensurate
     with `P = 8` and exercises the interesting range.)

2. **`masks.rs`**: runs the **actual** `src/materials/masks.js` module (not a
   transcription) against hand-built `THREE.BufferGeometry` instances, from
   inside `C:\dev\Claude-of-Duty` so `three` and the relative import resolve:
   - A **convex corner**: three flat, non-indexed triangles meeting at a
     shared cube corner `(1,1,1)`, each duplicating its own vertices (the
     exact "hard-edged kit geometry" shape the source's doc comment calls
     out), outward-facing axis-aligned normals.
   - A **concave corner**: the same positions, every normal negated (an
     interior corner, e.g. where three walls of a room meet on the inside).
     `bakeMasks` derives convex/concave purely from the *stored* normal array
     against triangle positions, so this is a legitimate second fixture, not
     a post-hoc sign flip of the first result.
   - A **flat lone triangle** control (no real adjacency): curve is exactly
     `0` by construction (a triangle's own normal is perpendicular to both its
     own in-plane edge-offset vectors), verified in a Rust-only test rather
     than a JS capture, since it follows from the algorithm's definition, not
     from a captured number.
   - The **rng jitter** branch: a JS transcription of `apps/shmup/
     src/rng.rs`'s exact xoshiro128**/SplitMix32 sequence (verified line-for-
     line against `rng.rs`'s `seed`/`u32` before use), driving `bakeMasks`
     with `rng: Rng::new(1234)` and capturing the per-**vertex** (not
     per-cluster) jittered output — this is what proved the jitter applies
     per vertex inside the final loop, not once per cluster (see below).
   - `setMask`'s uniform fill.

   Values here are built only from `+ - * / sqrt min max clamp` — no
   `sin`/`cos`/`pow` — so they're pinned at `1e-6` (matching the `f32` storage
   width of the final `color` attribute) rather than needing a
   transcendental-class tolerance.

## A transcription bug caught by re-deriving the capture, not by trusting it

The first draft of the convex-corner test's expected array was hand-copied by
eye from a single `JSON.stringify`'d flat array printed on one line, and two
values (`v4`, `v5`) were mis-grouped in the process — an off-by-nothing
counting error, not a code defect. The **Rust implementation was correct
first**; the test's expected values were wrong. Caught by re-running the
capture with an explicit per-vertex `console.log` (position + normal + color
on one line each) instead of eyeballing a flat array, and by understanding
*why* the numbers should be what they are before accepting them: `v4` and
`v5` are both vertices of the same `+Y` face triangle, so they share that
triangle's flat-shaded normal `(0, 1, 0)` — meaning **both**, not just one, of
them get the "upward face" wear bonus (`up * up * upWear * wear`) on top of
their (identical, since they're in the same *position* cluster as `v2`/`v7`
respectively — no, wait: `v4`'s position cluster is `(1,1,0)` shared with
`v2`, and `v5`'s is `(0,1,1)` shared with `v7`) cluster's base convexity,
clamping both to `1.0`. This is recorded as a worked example in `masks.rs`'s
test doc comments so a future reader doesn't have to re-derive it.

## Design notes / divergences

- **`bake.rs`'s `f32` height buffer, never quantized further.** The source
  bakes height into a *half-float* scratch render target specifically because
  an 8-bit height field stair-steps the Sobel. This port keeps that guarantee
  with `f32` (strictly more precision than half-float) and, since there's no
  display path yet, never rounds anything to 8-bit at all — every `Texture`
  channel stays `f32` in `[0,1]` end to end.
- **`masks.rs` reuses `crate::weapons::geometry::Geo`** instead of a new
  position/normal/index carrier. This is not a shortcut: `Geo::pos`/`Geo::
  normal` are already flat, non-interleaved `f32` arrays — exactly the shape
  the source's own `plainXYZ` fast-path exists to detect and use, and exactly
  what a `THREE.BufferGeometry`'s position/normal attributes conventionally
  are under the hood in the source too (a `Float32Array`, read into JS
  `Number`/`f64` for arithmetic). So there is no `plainXYZ` port at all — every
  call site in this port is unconditionally already on the fast path, and no
  precision is lost relative to the source that wasn't already lost by the
  source's own typed-array storage.
- **Clustering uses `HashMap<i32, Vec<usize>>`, not a hand-rolled chain
  array.** The source's chained-bucket scheme
  (`masks.js:83-115`) exists to avoid a `${x},${y},${z}` **string** allocation
  per vertex — measured at 50-65 ms of a 110-118 ms bake over 202k vertices.
  That finding is about the string key, not about using a hash-map-shaped
  lookup; a `HashMap` keyed on the same `i32` hash the source computes needs
  no string and produces byte-identical clusters in the same first-seen
  order, without hand-rolling the chain.
- **`js_round` vs `f64::round`.** `Math.round` rounds ties toward
  `+Infinity`; Rust's `f64::round` rounds ties away from zero
  (`(-0.5).round() == -1.0` in Rust, `Math.round(-0.5) === -0` in JS). The
  quantization step (`round(pos * 8192)`) is exactly the kind of exact-tie-
  break case the port recipe's rule 5 calls out, so `js_round(x) = (x +
  0.5).floor()` reproduces the JS behaviour exactly rather than letting a
  position that happens to land on a `k/16384` boundary silently pick a
  different cluster than the source would.
- **`bake_masks` asserts `geo.normal.len() == geo.pos.len()`** rather than
  lazily computing normals the way the source's `if (!nrm)
  geometry.computeVertexNormals()` does. `Geo` has no lazy-normals concept —
  every primitive builder in this port populates `normal` alongside `pos` —
  so the precondition is asserted explicitly instead of silently filled in.
- **`gl_step`** lives in `bake.rs`, not `noise.rs`: it's a bare GLSL builtin
  `DETAIL_SRC` calls directly, not one of `noise.js`'s named functions, and
  `noise.rs`'s whole reason for existing is to mirror `noise.js`
  function-for-function.

## Nothing left un-ported from these two files

`generator.js`'s CPU-portable surface (everything except the WebGL render
target orchestration) and all of `masks.js` (`bakeMasks`, `setMask`) are
fully ported. The 19 per-material `owSurface` generators
(`glsl/surfaces-*.js`) are out of scope for this task and are a separate,
future port — `bake()`'s `SurfaceFn` contract is what each of those will
plug into.
