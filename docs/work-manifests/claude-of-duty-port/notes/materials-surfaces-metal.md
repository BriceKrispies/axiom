# Metal surfaces port

**File:** `apps/claude-of-duty/src/materials/surfaces/metal.rs`
**Source:** `C:\dev\Claude-of-Duty\src\materials\glsl\surfaces-metal.js:1-323` (the whole file)
**Tests:** `apps/claude-of-duty/tests/materials_metal_port.rs` (golden, 8 tests) + `metal.rs`'s own `#[cfg(test)]` unit tests (7 tests)
**Golden capture:** `apps/claude-of-duty/tests/materials/metal/capture.mjs` -> `golden.json`
**Architecture check:** pass

## What was ported

`RUST_HELPERS` (`owRustColour`, `:8-21`) and the four `owSurface` generators:

- `metal_rust` (`METAL_RUST`, `:23-88`) — mill-finish base, warped billow rust blooms with flaking scale plates, deep Worley pitting under old rust, scratches restoring bare metal, a grime pass.
- `metal_painted` (`METAL_PAINTED`, `:90-178`) — a real layer stack (paint -> primer band -> rust -> steel) with chipping driven by `chipField*0.6 + chipEdge*0.2 + rustField*0.32 + uParam.z*0.25`, Worley impact dings, a bright chip lip, scratches to bare metal, and rust bleed streaks.
- `metal_brushed` (`METAL_BRUSHED`, `:180-237`) — three shear-stretched fbm bands (64:1, 24:1 stretch) forming fibres, deep score lines, cross scratches, shallow dents, and fingerprint/grease smudges that subtract straight from `metal` (`metal -= smudge * 0.10`).
- `corrugated` (`CORRUGATED`, `:239-323`) — an analytic sinusoid profile with per-panel lap steps, a Worley zinc spangle, rust weighted into the valleys and the sheet bottom, rust-through perforations, hex screws with rubber washers on the crowns (each weeping a rust streak), and dirt in the valleys.

All four preserve the file's opening rule exactly: `metal` starts at `1.0` on bare steel/zinc and every contamination layer (rust, paint, grime, smudge, dirt, a washer) pulls it toward `0.0`, restored only where a scratch/chip exposes bare metal.

`pub mod metal;` in `surfaces/mod.rs` and `pub mod surfaces;` in `materials/mod.rs`: **already present at `HEAD` before this port finished** — a sibling agent's commit (`2856a5f7`, the ground surfaces) staged and committed those shared files while my own edit to add the `metal` line was already sitting in the working tree, so their commit swept it up. Verified with `git show HEAD:.../surfaces/mod.rs` and `git diff HEAD` (empty) before touching anything — no edit was needed or made to either file in this session; only `metal.rs` and the test/golden files were staged.

## Function signatures (no `SurfaceFn` trait, matching `bake.rs`/sibling surfaces)

```rust
pub fn metal_rust(uv: Vec2, seed: f64) -> SurfaceSample
pub fn metal_painted(uv: Vec2, seed: f64, tint_a: Vec3, param_z: f64) -> SurfaceSample
pub fn metal_brushed(uv: Vec2, seed: f64) -> SurfaceSample
pub fn corrugated(uv: Vec2, seed: f64) -> SurfaceSample
```

`metal_painted` is the only one of the four that reads a uniform beyond `uSeed`: `uTintA` (LIBRARY's `metal_painted.bake.tint_a = 0x4a5340`) and `uParam.z`. Callers pass those already resolved, exactly like `organic.rs`'s `fabric(uv, seed, tint_a, tint_b)` does for its own tints — no generator in this port reads raw `Option<u32>`/`[f32;4]` config directly.

## `hex_to_linear_tint` — the one piece of `index.js` plumbing this file needed

`src/materials/index.js:145`: `tintA: new THREE.Color(bake.tintA)`. Three's `ColorManagement` (enabled by default) decodes that hex color from sRGB to the *linear* working space at construction — the same decode `noise::ow_srgb` already implements. `hex_to_linear_tint(hex: u32) -> Vec3` in `metal.rs` unpacks the hex triplet to `[0,1]` and runs it through `ow_srgb`, so `metal_painted`'s `tint_a` parameter is fed the same value the real shader uniform would see. Pinned two ways: a unit test against a fresh `ow_srgb` call, and a golden test (`hex_to_linear_tint_reproduces_the_golden_tint_a`) against the capture script's own `owSRGB(v3(0x4a/255, ...))`.

## Local helpers, not added to `noise.rs`

`mix3`, `clamp3`, `gl_step`, `gl_sign` are private to `metal.rs`. `noise.rs` is shared infra other sibling surface files (`arch.rs`, `ground.rs`, `organic.rs`) were editing concurrently in this session; `ground.rs` independently arrived at the same three helpers under different names (`v3_add`/`v3_mix`/`v3_clamp`/`gl_step`) — confirms this is the right call (file-local GLSL-builtin plumbing, not a genuine noise-library primitive), not a missed shared abstraction.

## The `sign(0)` trap, actually hit

GLSL `sign(0.0) == 0.0`; Rust's `f64::signum() `returns `1.0`/`-1.0` even at (positive/negative) zero. `corrugated`'s ridge profile is `sign(wave) * pow(abs(wave), 0.72) * 0.5 + 0.5` with `wave = sin(uv.x * 12 * 2*pi)`. The capture grid's 6th point is engineered at `uv.x = 1/24` so `t` lands on exactly `pi` and `wave` is (up to libm rounding) exactly `0.0` — `gl_sign`, not `signum`, is what makes that texel's profile come out `0.5` (`0 * anything + 0.5`). Pinned by `corrugated_sign_zero_texel_matches_the_javascript_capture`, which also asserts the golden's 6th sample point really is `1/24` (guards against the grid being edited later and silently losing the coverage of this trap).

## Golden capture: hand transcription, no native oracle

`surfaces-metal.js` is GLSL in a JS template string; it never ran outside a browser GPU shader. `capture.mjs` transcribes, line-referenced against the source: the noise primitives these four generators actually call (`owHash11/12/22`, `owGrad2`, `owNoise`, `owFbm`/`owFbm01`/`owBillow`, `owWarp`, `owWorley`, `owShear`/`owShearPer`/`owScratches`, `owSRGB`) plus `RUST_HELPERS` and the four `owSurface` bodies — not the full 24-function noise library, only what this file needs. This is a second, independent, equally-fallible translation, not a ground truth; drift between it and `metal.rs` is what the golden test catches, not a guarantee either reading of the GLSL is correct. Regeneration command is in the script's header comment.

Capture grid: 5 general-purpose `uv` points (corners + two interior points, matching the noise/bake port precedent) plus the engineered `1/24` sign-zero point for `corrugated`, evaluated at each generator's real `LIBRARY` seed (`metal_rust`=37, `metal_painted`=61, `metal_brushed`=83, `corrugated`=29) and, for `metal_painted`, the real `tintA` (`0x4a5340`) with `paramZ = 0`.

Tolerance: `1e-6`, not `1e-12`. Every generator chains many `owFbm01`/`owBillow`/`owWorley` calls per texel (each itself several `sin`/`sqrt` octaves), so per-call libm drift compounds well past a single-primitive comparison. `1e-6` is the figure `bake.rs`'s own `build_detail_tile_matches_the_javascript_capture` test already established for exactly this class of compounded-call comparison; reused rather than re-derived.

## A test-writing lesson worth recording: hand-picked "clean"/"dirty" uv points are not safe

The first draft of the physical-plausibility unit tests hard-coded specific `uv` values as "obviously clean steel" or "obviously non-metallic paint." Checking against the actual golden capture showed this was wrong in both directions:

- `metal_rust(uv=(0,0), seed=37)` is **not** clean — `metal == 0` there (confirmed in `golden.json`). The rust bloom/spread noise has no reason to be low at the origin for an arbitrary seed.
- A 32x32 grid scan for `metal_painted`'s metallic chip-through (`smoothstep(0.78, 0.96, chip)`) found **none** at `seed=61, paramZ=0` — the feature is real but small; a 200x200 JS scan (run out-of-band with a throwaway probe script, not committed) found the maximum climbs to exactly `1.0`, and a 48x48 grid was the coarsest resolution that reliably found it.

Fixed by scanning a grid and asserting the *property* ("some texel reads near-1, some near-0") rather than asserting a specific hand-picked point, the same shape as `corrugated`'s washer/crown test. This is recorded here because it is exactly the kind of thing a future agent porting another shader-heavy surface will hit again: never trust "this uv looks like it should be clean" for an fbm/Worley-driven mask without checking the actual numbers first.

## Nothing left un-ported

All of `surfaces-metal.js` (RUST_HELPERS + 4 generators) is ported. No divergence from the source was found or needed; the one preserved verbatim oddity is `corrugated`'s `washer * 0.0` term in its `weep` computation (`surfaces-metal.js:306`), a literal no-op in the source, kept rather than dropped per the port recipe's "port the behaviour, don't tidy" rule.
