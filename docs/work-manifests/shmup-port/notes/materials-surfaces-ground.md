# Ground surfaces port (asphalt / sand / dirt / gravel)

**File:** `apps/shmup/src/materials/surfaces/ground.rs`
**Source:** `C:\dev\Claude-of-Duty\src\materials\glsl\surfaces-ground.js:1-366` (whole file)
**Wired up:** `apps/shmup/src/materials/surfaces/mod.rs` (`pub mod ground;`), `apps/shmup/src/materials/mod.rs` (`pub mod surfaces;`)
**Tests:** `apps/shmup/tests/materials_surfaces_ground_port.rs` (5 tests) + `ground.rs`'s own `#[cfg(test)]` module (3 tests)
**Golden:** `apps/shmup/tests/materials_surfaces_ground/{capture.mjs,golden.json}`
**Full `cargo test -p axiom-shmup`:** pass. **`cargo xtask check-architecture`:** pass.

## What was ported

Four `owSurface(uv) -> (alb, h, rough, metal, ao)` GLSL bodies, transcribed to
CPU `f64` Rust functions returning `materials::bake::SurfaceSample`, using the
already-ported `materials::noise` library (no new noise primitives needed):

- `asphalt` — binder (macro/mid/fine fbm bands) + three grades of
  domain-warped-Worley angular aggregate, ravel voids, tyre-polish lanes,
  patch repairs with bleeding tar seams, alligator + thermal cracking (via
  `ow_cracks`), oil stains, settled dust.
- `sand` — asymmetric wind ripples (`sin` sheared + warped, then
  `pow(ripple,1.7)*0.75 + ripple*0.25` for the gentle-windward/sharp-lee
  profile), damp hollows, quartz sparkle, pebbles/shell fragments, dark
  mineral streaks.
- `dirt` — billow clumps (`ow_billow` over a warped domain), dried mud cracks
  (`ow_cracks`) whose plates curl up at their edges, two stone grades, dead
  grass/litter, sparse moss.
- `gravel` — three grades of aggregate at 34/19/9 mm (the finest at 5.9
  texels — right at the file's own documented Nyquist floor), a
  compacted-dust bed with stones separated from it by **relief and
  roughness, not albedo** (the palette deliberately straddles the bed value),
  a drift field that buries aggregate, dried tyre-track/heel scuffs. The
  baked AO is compressed to `0.87..1.0` — the file's own most emphatic
  comment, and the one figure explicitly called out as easy to lose.

Every frequency constant (`p * 12.0`, `P * 0.5`, etc.) is preserved exactly —
none tidied, per the module's Nyquist-budget doc comment (source lines 7-16,
reproduced in `ground.rs`'s module doc).

## Local helpers added (not in `noise.rs`)

`ground.rs` needed four bare-GLSL-builtin helpers no existing function
provided, added file-local rather than widening the shared `noise` module
(matching `bake.rs`'s own local `gl_step` precedent):

- `gl_step(edge, x)` — GLSL `step`, needed directly (not just inside a
  ported noise function) by every one of the four generators.
- `v3_add`, `v3_mix`, `v3_clamp` — `vec3 + vec3`, `mix(vec3,vec3,float)`,
  `clamp(vec3,float,float)`. `noise::Vec3` never needed a plain `add` before
  this file (every prior caller only needed `mul`/`scale`/`add_scalar`).

## `owWorley`'s `vec4` swizzle convention, reconfirmed

`.x`/`.y` = F1/F2 distance, `.z`/`.w` = the F1 cell's two hash channels
(`id_x`/`id_y` in `WorleyResult`) — same convention `bake.rs`'s
`detail_surface` already established. Every `.w`/`.z` read in the source
(`big.w`, `small.w`, `grit.z`, `a.z`, `b.w`, …) maps straight to `.id_y`/
`.id_x` with no reordering needed.

## Golden-capture method — no native oracle

`surfaces-ground.js` (and the `noise.js` it's built on) is GLSL held inside
JS template-string literals; neither ever ran anywhere but a browser GPU, so
there is no JavaScript function to import and call. `capture.mjs`
hand-transcribes the full noise library (hashes, `owNoise`/`owFbm`
family/`owWarp`/`owWorley`/`owVoronoiEdge`/`owCracks`/`owSRGB`/`owShear`) and
all four `owSurface` bodies into plain JS doubles, function-for-function
against the two source files, independently of this file's Rust
transcription. **This makes the oracle weaker than a genuine JS import** —
pinning against it catches drift between the Rust port and *a* careful
reading of the GLSL, not a mistake both transcriptions happen to share. Said
explicitly in both the capture script's and the test file's module docs, per
the recipe's requirement.

Samples use the same seeds as the real library entries in
`materials::mod::LIBRARY` (asphalt=71, sand=91, dirt=13, gravel=57) over a
fixed 10-point uv grid, plus a dedicated dense 17x17 grid for gravel's AO
band. Tolerance: `1e-9` relative (same figure `tests/sky_port.rs` uses for its
own no-oracle shader bodies) — every generator chains `sin`/`pow` (crack
networks, the ripple profile), not bit-guaranteed across V8 vs. Rust libm.

All 5 golden tests + gravel's dedicated AO-band test (asserting `0.87..1.0`
against both the transcription's own values and a hard-coded band check)
pass, plus a lighter contract-range smoke test and an explicit albedo-clamp
test inside `ground.rs` itself.

## Language traps checked

- **`sign` vs `signum`**: not applicable — this file contains no `sign()`
  call anywhere in the source.
- **Euler order**: not applicable — no rotations; `ow_warp`/`ow_shear` are
  the only domain transforms and both were already ported and golden-pinned
  in `noise.rs`.
- **`f64` throughout, no `f32` truncation**: matches `bake.rs`'s existing
  convention (`SurfaceSample` is all-`f64`) — no divergence to introduce.
- **`owSRGB`'s `step(c, vec3(0.04045))` argument order**: the source passes
  `c` as `step`'s *edge* and the constant as `x`, the reverse of the more
  common `step(edge_const, x)` shape used everywhere else in this file
  (`step(0.30, small.w)`, etc.). Got this right the first time because
  `noise.rs`'s `ow_srgb` (already ported and golden-pinned) was read as the
  reference rather than re-deriving it — the capture script's `owSRGB`
  comment calls out the argument order explicitly so a future reader doesn't
  have to re-derive it either.

## Nothing left un-ported

All four generators (`ASPHALT`, `SAND`, `DIRT`, `GRAVEL`) are fully ported.
WGSL emission remains out of scope, same as `bake.rs`/`noise.rs`.
