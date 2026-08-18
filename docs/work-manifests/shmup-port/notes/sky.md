# Sky / atmospheric scattering — port notes

Ported `C:/dev/Claude-of-Duty/src/sky/{atmosphere,luts,noise,celestial}.js`
into `apps/shmup/src/sky/`. `apps/shmup/src/lib.rs` gained
one line, `pub mod sky;`, in alphabetical order between `rng` and `ui`.

## What's here

- `atmosphere.rs` — the media constants (`ATMO`), the photometric-scale
  constants (`SCENE_LUX`/`SUN_ILLUMINANCE_TOP`/`MOON_ILLUMINANCE_NIGHT`), the
  phase functions (Cornette-Shanks Mie, analytic Rayleigh, Henyey-Greenstein),
  `ray_sphere`, `medium`, the LUT UV parameterisation (`lut_uv`), the analytic
  single/multi-scattering segment integral (`raymarch_sky`), and the two
  genuine-JS-oracle CPU functions (`transmittance_to_space`, `luminance`). A
  local `Vec3` (`f64`) is this module's vector vocabulary — see its doc
  comment for why it isn't `axiom_math::Vec3` (that's `f32` and fallible on
  `normalize`; this is a reference port that has to match a JS `number`
  bit-for-bit-modulo-libm).
- `luts.rs` — `Lut2D` (bilinear-sampled CPU texture, matching WebGL2's
  texel-centred `texture()` with clamp-to-edge/repeat addressing) plus the
  four bakes: `bake_transmittance`, `bake_multiscatter`, `bake_sky_view`,
  `bake_ambient`, and the sky-view LUT's own lookup parameterisation
  (`sky_view_lookup`, azimuth-relative-to-sun + square-distributed altitude).
- `noise.rs` — the shared `NOISE_GLSL` hash/value-noise/fbm library
  (`hash12`/`hash13`/`hash33`/`ign`/`val2`/`val3`/`fbm2`/`ridge2`/`fbm3`).
  Ported but **not yet wired to anything** — no `dome.js`/fog pass is in this
  slice, so nothing in `luts.rs` calls it. It's a reference implementation
  waiting for that later slice, the same role `crate::materials::noise` plays
  for the surface-texture library.
- `celestial.rs` — `solar_declination`, `alt_az`, `dir_from_alt_az`, and the
  `Celestial` struct (`set_hour`, `celestial_matrix`). Includes a small local
  `Mat3` (row-major) for the equatorial→world starfield rotation, even though
  no starfield/dome consumer exists yet in this crate — it's a small, cheap,
  fully-testable piece of `celestial.js` and the task said to port the file in
  full.

## The core problem this slice had to solve: no CPU oracle for the shaders

`atmosphere.js`'s `ATMOSPHERE_GLSL`/`SCATTER_GLSL`/etc. and every `*_FRAG` in
`luts.js` are WebGL2 fragment-shader source — GLSL template strings that only
ever execute on a browser GPU. There is no JavaScript function to `node
capture.mjs`-and-import for these, unlike (say) the audio DSP port, which had
a real Node-runnable oracle for everything.

The resolution, spelled out in both `tests/sky/capture.mjs`'s module doc and
`src/sky/{atmosphere,luts,noise}.rs`'s: **hand-transcribe each named GLSL
function into plain JS in the capture script, independently of (but
line-referenced against, the same source lines as) the Rust transcription.**
Concretely:

- Genuine oracle, imported and called directly: `celestial.js` in full, and
  `atmosphere.js`'s CPU tail (`transmittanceToSpace`, `luminance`, the
  constants).
- No oracle, hand-transcribed twice (once in `capture.mjs`, once in the Rust
  `.rs` files), each transcription tagged with the exact source line range it
  translates: the phase functions, `skRaySphere`, `skMedium`, `skRaymarchSky`,
  every `*_FRAG` bake body, `skSkyView`, and the `noise.js` hash/fbm family.

This means the golden pins the Rust port against *a specific, reviewable JS
reading of the GLSL*, not against an independent ground truth — auditing
correctness means reading the GLSL, `capture.mjs`, and the `.rs` file side by
side, not trusting either transcription as a silent oracle for the other. I
worked line-by-line from the GLSL for both, cross-checked the two against each
other (they matched on first full run except one input-selection issue, not
an algorithm bug — see "Bugs found" below), and additionally sanity-checked
the whole pipeline's output order of magnitude against the physical reference
values the source's own doc comment cites (1500 cd/m² zenith sky, etc.).

## The photometric contract

Reproduced exactly: `SUN_ILLUMINANCE_TOP = 128000/25000 = 5.12` (pinned exact,
literal constant), and `raymarch_sky`/`bake_*` never multiply by `pi` on the
way out (see `atmosphere.rs`'s module doc, which restates the source's
`atmosphere.js:20-57` note in full). Also asserted, as **order-of-magnitude
sanity bounds** rather than exact pins — because the source's own comment
states them with "~" — the two other headline figures:

- noon-sun-after-extinction: computed (real sun altitude for the site at solar
  noon, 68.44°) as `luminance(transmittance_to_space(sin(68.44°), 1.35) *
  SUN_ILLUMINANCE_TOP)` = **4.43** — order-of-magnitude "~3.9-4.4", asserted
  in `[2.5, 6.0)`. (The doc's literal "~3.9" turns out to be closest to just
  the *blue* channel, 3.83-3.91 depending on exact sun altitude used, not the
  Rec.709 luminance of the three channels — the luminance is closer to 4.4,
  which is *also* what the doc says it "matches" (4.3, the renderer's
  fallback sun intensity). Either reading is within the asserted band.)
- clear-zenith-sky: `raymarch_sky` straight up under that same sun gives
  `luminance ≈ 0.084` — order-of-magnitude "~0.06" (1500 cd/m² / 25000),
  asserted in `[0.02, 0.2)`. A dropped/doubled `pi` (the exact historical bug
  the source's comment describes) would move this by a factor of ~3.14 — well
  outside that band — so the bound is a real regression guard, not a rubber
  stamp.

These bounds are wide enough to accept the model's natural output without
pretending the source's own "~" figures are exact targets, and tight enough
(order of ~3x) to catch the one-`pi` class of regression the contract note
warns about by name.

## Resolution: what's tested at production size vs. reduced size

- Transmittance (256×64, 40 steps) and multiscatter (32×32, 8×8 directions,
  20 steps) are cheap regardless, so the golden dumps the **full production
  grid** and the Rust test bakes and compares it whole.
- Sky-view (production 384×192, 40 steps — `SKYVIEW_WIDTH`/`SKYVIEW_HEIGHT`
  constants document this) is the expensive one: 384×192×40 ≈ 2.95M
  step-iterations with four bilinear LUT taps each. `bake_sky_view` takes
  `width`/`height`/`steps` as **parameters**, not hardcoded constants, so the
  golden/test instead bakes and fully compares a **64×32** grid (same 2:1
  aspect, same step count) — the identical code path at every texel, just
  fewer of them. Total `cargo test -p axiom-shmup --test sky_port`
  runtime is ~1.1s; a full 384×192 bake would very likely still be fine
  (it's roughly 6x the texels) but wasn't necessary to prove the algorithm
  and I didn't want to inflate the golden file or slow the suite without a
  reason. Any future GPU/system wiring that actually needs the LUTs at
  production resolution just calls `bake_sky_view` with the named constants.

## Bugs found while writing this (both fixed before landing, not shipped)

1. **`f64::signum` vs GLSL `sign`.** GLSL `sign(0.0) == 0.0`; Rust's
   `f64::signum()` returns `±1.0` even at `±0.0` and can never return `0`.
   `skSkyView`'s `v = 0.5 + 0.5*sign(altitude)*sqrt(...)` genuinely depends on
   the `sign(0)==0` case (exact-horizon altitude), so I wrote `gl_sign` in
   `atmosphere.rs` explicitly rather than reaching for `signum()`. Caught by
   writing the doc comment before the code and asking "does this actually
   match," not by a failing test — worth flagging because a naive port would
   have compiled, run, and been subtly wrong exactly at the horizon.
2. **Zero-vector `normalize()` panics/NaNs.** My first `skyViewLookup` golden
   test case used ray direction `[0, 1, 0]` (exact zenith). `skSkyView`'s
   `proj = normalize(rayDir - up*dot(rayDir,up))` divides a zero vector by its
   own zero length there — genuinely singular in the source too (same 0/0 in
   GLSL), not a Rust-only defect. I did not "fix" the algorithm (the source
   doesn't guard it either, at any of its real call sites — every real caller
   samples a ray direction from a grid or a jittered hemisphere, never exactly
   parallel to `up`); I just picked non-degenerate test inputs, and documented
   the singularity at the `capture.mjs` call site so a future reader doesn't
   reintroduce it as a test case and get confused by a NaN mismatch.

## What a GPU/WGSL bake would still need on top of this

This reference implementation is deliberately **not** what ships to a GPU:

- **No `f32`/fp16 storage quantization.** The source's multiscatter, sky-view
  and ambient render targets are `RGBA16F`; a real bake rounds every value
  written to those three textures to half precision before the next pass
  reads it back (transmittance is float32, so only that one LUT would be
  quantization-free even on GPU). This Rust port stays `f64` throughout,
  matching `crate::materials::noise`'s own precedent for the same reason:
  higher precision than the eventual GPU evaluation, so a golden-comparison
  tolerance is fighting the actual algorithm, not also fighting truncation.
- **No WGSL emission.** The GLSL bodies were read and hand-translated to get
  the reference; nothing here emits WGSL/GLSL text. A future GPU-backed sky
  would need an emitter (or a rewrite of `luts.rs`'s bakes as compute/fragment
  shaders against `axiom-gpu-backend`) that this reference can be pinned
  against, the same way this reference itself is pinned against the original
  GLSL.
- **No render-target/texture-upload plumbing.** `Lut2D` is a plain `Vec<Vec3>`
  buffer; nothing here creates a GPU texture, sets wrap modes, or uploads it.
  That's the same "app owns the translation" boundary the CLAUDE.md Module Law
  describes for module-to-module glue, except here it's port-reference-to-GPU.
- **`f32` vs `f64` texture read-back.** `skTransmittance`/`skMultiScatter`
  read back through GLSL's `sampler2D`, which is `f32`-precision hardware
  texture filtering; `Lut2D::sample`'s bilinear math is `f64`. Numerically
  close but not identical to what a GPU would actually interpolate.

## Verification

`cargo test -p axiom-shmup --test sky_port` — 19 tests, all
passing, ~1.1s. `cargo test -p axiom-shmup` (whole crate) — 153+ lib
tests plus every integration test file, all green. `cargo xtask
check-architecture` — OK.

Golden: `tests/sky/golden.json` (~1.2 MB — comparable to the audio port's
703 KB precedent), produced by `tests/sky/capture.mjs` (`node capture.mjs >
golden.json`, reproducible/deterministic — no randomness anywhere in this
slice). Tolerance: `REL = 1e-9` relative, looser than `core_port.rs`'s `1e-12`
for the RNG's Box-Muller draws because the LUT bakes chain dozens of
transcendentals per texel (V8 and Rust's libm are not bit-identical); still
tight enough that a real algorithmic or photometric-scale bug moves a value by
decimal *digits*, not in the ninth. The `ATMO`/`SCENE_LUX`/etc. constants are
pinned with exact `assert_eq!` (no arithmetic on the Rust side to introduce
drift).
