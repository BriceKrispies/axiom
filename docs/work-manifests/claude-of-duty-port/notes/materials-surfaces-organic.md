# Port notes: `src/materials/surfaces/organic.rs`

Source: `C:/dev/Claude-of-Duty/src/materials/glsl/surfaces-organic.js` (whole
file, 416 lines). Six `owSurface` GLSL bodies ported to CPU `f64` maths:
`wood`, `fabric`, `burlap`, `foliage`, `rubber`, `glass`.

## What was ported

Every generator mirrors its GLSL source line-for-line on top of the existing
`crate::materials::noise` primitives (hashes, fbm family, Worley/Voronoi,
warp, shear, scratches, cracks, sRGB decode) and returns a
`crate::materials::bake::SurfaceSample`. No new noise primitives were needed;
`organic.rs` only adds four private, file-local GLSL-vocabulary helpers not
already exported by `noise.rs`: `gl_step` (bare `step()` builtin, same
reasoning as `bake.rs`'s private copy), `mix3`/`add3`/`clamp3` (component-wise
vec3 ops the six generators need repeatedly).

Function names follow the sibling `surfaces/ground.rs` convention (`asphalt`,
`sand`, …) rather than a `_surface` suffix: `wood`, `fabric`, `burlap`,
`foliage`, `rubber`, `glass`. `fabric` is the only one that takes the
`uTintA`/`uTintB` uniforms as explicit `tint_a`/`tint_b: Vec3` parameters — no
other generator in the file references them.

## Source quirks preserved (not silently fixed)

Two places in the GLSL compute a value and then immediately discard it
before it ever reaches an output. Per the port recipe, dead code is not
transcribed as inert arithmetic (there's nothing to test — it provably never
affects any observable value), but both are called out in comments at the
site and in the module doc, with exact source line numbers:

1. **`wood`, `surfaces-organic.js:94-96`.** The source computes an `nf`/first
   `nd` pair (`nd = length(nf * vec2(3,1) / vec2(3,1) * vec2(1,1))`,
   algebraically `length(nf)`) and then immediately reassigns `nd` to a
   different formula (`rf - 0.22`, stretch `(1.4, 1.0)`) on the very next
   line. Only the live formula is in `organic.rs`.
2. **`foliage`, `surfaces-organic.js:290-293`.** `cover` is computed once
   without edge serration and immediately overwritten by the serrated
   version. Only the serrated formula is transcribed.

A third case is *not* dead code but is genuinely never read outside its own
scope: `foliage`'s `bestH` (`surfaces-organic.js:313`) is computed every
winning depth-sort iteration but the final `h` written by the function is
`bestCover`, not `bestH` — the module doc calls this out directly ("foliage's
`h` is a cutout mask, not a height"). This one *is* still computed in the
port (`let best_h = …; let _ = best_h;`), kept for line-for-line diffability
against the source, since — unlike the two cases above — it documents a real
semantic fact about the surface (foliage never emits real height) rather
than being pure inert arithmetic.

## Foliage's `h` is a cutout mask, not a height

Called out in the module doc and pinned by
`foliage_h_is_a_binary_ish_cutout_mask_not_a_smooth_height`: a dense 25x25 uv
grid is checked against the transcription, then partitioned into
"near-extreme" (`h < 0.05` or `h > 0.95`) vs "mid-band" texels. A genuine
alpha-test cutout mask spends most of its area at the extremes (leaf
interior vs. bare background) with only a thin serrated-edge transition band
between; a real height channel would show the opposite shape. The test
asserts `near_extreme > mid_band` over the grid, which held on first run.

## Glass: near-black albedo, roughness carries the look

Pinned by `glass_albedo_stays_near_black_while_roughness_carries_the_variation`:
a dense 17x17 grid asserts every texel's albedo stays under `0.2` (well below
mid-grey) while roughness spans more than `0.05` across the same grid and
stays inside the source's own `clamp(rough, 0.02, 0.7)`.

## Golden-capture method

`tests/materials_surfaces_organic/capture.mjs` hand-transcribes the whole
GLSL body (and the `noise.js` functions it needs) into plain JS doubles,
independently of the Rust port, then writes
`tests/materials_surfaces_organic/golden.json` — committed, byte-reproducible
by re-running `node capture.mjs > golden.json` from that directory.
**The oracle is hand-written, not genuine**: there is no real JS `owSurface`
function to import and call (it only ever existed as a GLSL string), so a
match between the Rust port and the capture catches drift between the two
transcriptions of the same GLSL — it cannot catch an error both
transcriptions share. Same discipline, same caveat, as
`tests/materials_surfaces_ground/capture.mjs`.

`tests/materials_surfaces_organic_port.rs` reads that JSON and checks every
`owSurface` output field per surface at a shared 10-point uv grid (the same
grid the ground precedent uses), plus the two dense-grid assertions above.
Tolerance: `1e-9` relative, matching the ground precedent's figure — every
generator here chains `sin`/`cos`/`exp`/`atan2`, none bit-guaranteed across
V8 and Rust's libm. `metal` fields are checked with `assert_eq!` (integer- or
comparison-derived, no transcendentals).

Real-world seeds and, for fabric, real tints are used throughout (matching
`src/materials/mod.rs::LIBRARY`'s wood=19, fabric=43 (tintA=0x5a5445,
tintB=0x3a3830), burlap=67, foliage=79, rubber=97, glass=3), so the golden
doubles as a contract check against the actual material library, not just an
arbitrary probe.

### `fabric`'s tint uniforms

The GLSL reads `uTintA`/`uTintB` directly as `vec3`. In the source engine
these come from `new THREE.Color(hex)` (`src/materials/index.js:145`), and
under this project's r180 default color management that decodes the hex
literal as sRGB into the linear working color space — the exact transform
`owSRGB` performs on every other hard-coded albedo constant in this file.
Both the capture script (`hexToLinear`) and the Rust test (`hex_to_linear`)
derive this independently by calling their own `owSRGB`/`ow_srgb`, and the
test cross-checks its own re-derivation against the golden's captured tint
values before running any sample comparisons — catching a divergence in
"what does the tint uniform even mean" before it could hide inside a
coincidentally-matching sample.

## Divergences / language traps checked for

- **`sign`/`signum`**: not used by any of these six generators (no `sign()`
  call in the source file).
- **`step`**: GLSL `step(edge, x)` ported as a plain `<` comparison
  (`gl_step`), not `f64::signum` — not the sign trap, but worth noting this
  file's `step` calls are all genuine boolean gates (`hasKnot`, `nail`,
  `weep`'s `step(0.3, rnd.w)`), not zero-sensitive.
- **Euler order**: not applicable — no rotation composition here beyond
  `owRot`'s single 2D rotation, already ported and documented (with its
  column-major clockwise quirk) in `noise.rs`.
- **f64 vs f32**: computed in `f64` throughout, matching the noise module's
  existing convention (higher precision than eventual GPU evaluation, not
  fighting `f32` rounding on top of the transcendental tolerance).
- **Evaluation order for `+`**: one real case found and fixed —
  `foliage`'s `r2 = owHash42(cell * 1.7 + 9.0 + uSeed)` is two sequential
  scalar adds in the source; an early draft combined the two constants into
  one `add_scalar(9.0 + seed)` call, which is the same real value but a
  different float rounding order. Reverted to two sequential `add_scalar`
  calls to match the source's left-to-right evaluation exactly (caught before
  running the golden, so it's not visible as a test failure — noted here so a
  future reviewer knows to watch for the same pattern elsewhere).

## Verification run

- `cargo test -p axiom-claude-of-duty --test materials_surfaces_organic_port`:
  8/8 passed on first run against the golden.
- `cargo test -p axiom-claude-of-duty` (whole crate): 367 passed, 1 failed —
  the failure is `materials::surfaces::metal::tests::
  metal_painted_is_non_metallic_somewhere_and_bare_through_a_chip_elsewhere`,
  in a sibling agent's concurrently-written `metal.rs`, unrelated to this
  slice (not touched, not staged).
- `cargo xtask check-architecture`: OK, no violations.
