# `material_shader/weathering` — porting notes

The weathering stack of `C:/dev/Claude-of-Duty/src/materials/shader.js`:
`owRunoff` + `owHash11` (PARS_FRAGMENT, source lines 141–168) and the whole
`#ifdef OW_WEATHER` block of MAIN_FRAGMENT (source lines 492–566), plus the
`DEFAULT_PARAMS` entries it reads (`weather`, `groundY`, `dustColor`,
`grimeColor`, `rustColor`).

Everything lands in one file: `modules/axiom-gpu-backend/src/material_shader/weathering.rs`.

## Entry points

All free functions, explicit arguments, no globals and no assumed binding index.

| WGSL | signature |
|---|---|
| `ow_weather_smoothstep` | `(e0: f32, e1: f32, x: f32) -> f32` |
| `ow_weather_mix3` | `(x: vec3<f32>, y: vec3<f32>, a: f32) -> vec3<f32>` |
| `ow_hash11` | `(x: f32) -> f32` |
| `ow_runoff` | `(s_axis: f32, y: f32, wobble: f32) -> vec3<f32>` |
| `ow_weather_vert` | `(nw_y: f32) -> f32` |
| `ow_weather_s_axis` | `(world_pos: vec3<f32>, nw: vec3<f32>) -> f32` |
| `ow_weather_streak_uv` | `(s_axis: f32, world_y: f32) -> vec4<f32>` |
| `ow_weather_dust` | `(OwWeatherState, nw_y, weather_x, mac1_b, mac2_g, dust_col, n_flat) -> OwWeatherState` |
| `ow_weather_rain` | `(OwWeatherState, vert, s_axis, world_y, s_n, s_fine, weather_y, vcolor, vcol_masks, grime_col, rust_col) -> OwWeatherState` |
| `ow_weather_splash` | `(OwWeatherState, vert, h_above, weather_z, mac1_b, mac2_g, grime_col, dust_col) -> OwWeatherState` |
| `ow_weather_wedge` | `(OwWeatherState, vert, h_above, weather_z, mac1_r, mac2_b, mac2_g, dust_col, n_flat) -> OwWeatherState` |
| `ow_weather_stack` | the four in order, `+ macro_tex: texture_2d<f32>, macro_smp: sampler` |

`struct OwWeatherState { albedo, orm, n_shade }` is exactly the set of fragment
locals the source's weathering section mutates (`alb.rgb`, `orm`, `nShade`).

Each of the three named sub-passes — rain runoff, ground splash, dust wedge —
plus the airborne-dust pass that opens the same source section is its own WGSL
function with its own CPU reference and its own parity coverage, rather than one
blob.

## Why this layer is the one that needed world space

Every read of `SurfaceIn::world_pos` / `world_normal` in this file is load-bearing
and cannot be moved to object space:

* **Rain runs down.** `owRunoff` picks its sources on a 2.85 m storey pitch in
  world `y`, and the run is `srcY - y`. In object space a rotated wall streaks
  sideways.
* **Ground splash is a difference against a world plane.** `hAbove =
  vOwWPos.y - owGroundY`.
* **The dust wedge sits at the wall/ground junction** — the same world plane,
  gated on `owVert`, which is a *world*-normal test.

An object-space version would look right in a still frame and be wrong the moment
anything moves. `tests::the_splash_band_tracks_the_world_ground_plane` is the
test that says so.

## Verification: CPU↔GPU parity on a real adapter

17 tests. Sixteen CPU-side, one GPU parity sweep (`--features offscreen`,
asserting a real adapter rather than skipping) covering **every** WGSL entry
point against its Rust reference over 24 samples × 3 output rows.

Measured worst absolute lane deltas, Vulkan, `Rgba32Float` target:

| entry | worst Δ |
|---|---|
| `ow_hash11` / `ow_weather_vert` / `ow_weather_s_axis` | `4.77e-7` |
| `ow_runoff` | `1.92e-4` |
| `ow_weather_streak_uv` | `4.77e-7` |
| `ow_weather_dust` | `1.19e-7` |
| `ow_weather_rain` | `1.22e-6` |
| `ow_weather_splash` | `5.96e-8` |
| `ow_weather_wedge` | `1.19e-7` |
| `ow_weather_stack` | `2.18e-6` |

Two budgets:

* **`TOLERANCE = 5e-6`** — the exact tier, set from the measured `2.18e-6` worst
  (2.3× headroom).
* **`RUNOFF_TOLERANCE = 3e-4`** — `ow_runoff` only, and **derived, not fitted**
  (see below).

Both sides compute in `f32` throughout, matching the GPU. The one `f64`
computation is the sRGB conversion, which is `f64` in the source too (`Math.pow`
on the CPU) and narrows to `f32` once, at upload.

### Why `ow_runoff` needs its own budget — read this before widening anything

Diagnosed by reading the two sides' bits, not guessed:

1. The GPU contracts `cell * 1.37 + 3.1` (the column hash's argument) into a
   single-rounding `fma`. That is **one ULP**: `0xbf8147b0` vs `0xbf8147af`.
2. `owHash11` then squares its way up. `p * (p + 33.33)` reaches at most `34.33`;
   `p * (p + p)` at most `2 · 34.33² ≈ 2357`.
3. The final `fract` therefore operates on a number whose own `f32`
   representation quantises at `2⁻¹² = 2.44e-4`. So **any** sub-ULP disagreement
   upstream lands as at most *one* step of `2.44e-4` — the two `p` values are
   adjacent `f32`s, not divergent ones. Observed: `1.92e-4`, which is that step.

This is the source's own sensitivity, present in the original GLSL — where the
same contraction is expressly permitted — not a transcription error. Consequence
worth knowing: `step(0.86, runoff.y)` is a hard threshold on a value this coarse,
so a column whose `r1` lands within `2.44e-4` of `0.86` may take different rust
arms on the two sides.

`ow_weather_rain` and `ow_weather_stack` consume `runoff` and inherit the same
mechanism, yet measure at the exact tier: at these samples every column whose
hash diverges happens to have a **zero** run, so the difference is multiplied
out. They are deliberately still held to `TOLERANCE`. If a future sample change
makes one fail by ~2e-4, that is the mechanism — move the sample or lift that one
entry, never widen the exact tier.

### The two `floor` seams, and why the samples dodge them

`owRunoff` floors twice: `floor(sAxis * 1.55)` (the column) and
`floor((y + jitter) / 2.85)` (the storey). Both are genuine discontinuities in
the *algorithm*, and WGSL guarantees division only to 2.5 ULP, so a fragment
sitting exactly on a seam may legally floor to different integers on the two
sides and diverge by the full height of the step. A tolerance wide enough to
swallow that would prove nothing.

So the sample set stays ≥ 0.10 of a cell clear of both seams, and
`parity::assert_samples_avoid_the_floor_seams` fails loudly (naming the seam) if
someone edits the samples onto one. The first run of the suite tripped it — that
guard is not theoretical.

## Source-fidelity decisions

* **`smoothstep` and `mix` are written out on both sides**, not delegated to the
  builtins whose factoring is unspecified. Two calls run with `e0 > e1` on
  purpose — `owVert`'s `smoothstep(0.72, 0.34, …)` and the splash spray's
  `smoothstep(0.10, max(z, 1e-3), …)` whenever the splash height is under 10 cm —
  where the sign of `e1 - e0` *is* the result. `mix` always takes its factor as
  an `f32` value so a literal factor cannot be folded at a wider precision.
* **`fract` is `x - floor(x)`.** World coordinates are negative across half of
  any street and the column index is `floor(sAxis * 1.55)`, so Rust's `%` would
  fold columns together on the negative side.
  `tests::runoff_columns_are_distinct_on_the_negative_side_of_the_origin` pins it.
* **`clamp` is `min(max(x, lo), hi)`**, written out — `f32::clamp` panics when
  `lo > hi` where GLSL returns `hi`.
* **GLSL `sign` does not arise.** The weathering section calls it nowhere; the
  `sgn` at source line 480 belongs to the repair-patch layer.
* **The multiply chains are the source's, untidied.** `wedge *= wedge * (0.7 + …)`
  is transcribed as `wedge * (wedge * (0.7 + …))`; `owHash11`'s `p *= p + 33.33`
  as `p * (p + 33.33)`. `(y + jitter) / SPACING` stays a division, never a
  reciprocal multiply.
* **`Math.hypot` does not arise**; the only length is GLSL `normalize`, ported as
  `v / sqrt(dot(v, v))`.
* **The one named constant.** The source writes `3.14159265`. In `f32` that is
  *exactly* `PI` (`0x4049_0FDB`), and `clippy::approx_constant` is a deny, so the
  Rust names `core::f32::consts::PI` while the WGSL keeps the source's digits
  verbatim. `tests::the_sources_pi_literal_is_the_f32_pi` proves the equality
  from integers, so the check is not written by the habit it is checking.

## Colours: three.js's `SRGBToLinear`, not the GLSL form

`dustColor` / `grimeColor` / `rustColor` are hex sRGB and reach the shader through
`new THREE.Color(hex)` — i.e. converted **on the CPU**, in `f64`, before they are
ever a uniform. So `srgb_hex_to_linear` uses three's constants
(`(c · 0.9478672986 + 0.0521327014)^2.4`, `c · 0.0773993808` below the `0.04045`
knee), not the algebraically-equal `((c + 0.055) / 1.055)^2.4`.

Unlike the GLSL, three.js **is** a runnable oracle, so this one is pinned to
ground truth rather than to a transcription. Captured from node with three 0.180
(`new THREE.Color(hex)` → `Float32Array` round trip) and asserted bit-for-bit in
`tests::the_weathering_colours_are_threes_srgb_to_linear_bit_for_bit`:

| param | hex | linear `f32` bits (r, g, b) |
|---|---|---|
| `dustColor` | `0x6b6154` | `3e168e51`, `3df4d090`, `3db5910f` |
| `grimeColor` | `0x2a2620` | `3cbdac21`, `3c9ec7c2`, `3c6ca5df` |
| `rustColor` | `0x6d3a1c` | `3e1c98ac`, `3d2d4ebb`, `3c3e4149` |

Measured aside: for *these three* colours the two sRGB forms differ only at
~1e-11 in `f64`, which the `f32` narrowing absorbs — the bits are the same either
way. That is luck, not licence: the forms disagree on 254 of the 256 byte values
and the port already paid for that once. Three's form is what runs, so three's
form is what is implemented.

## `DEFAULT_PARAMS`

```
weather:    [0.35, 0.3, 0.55, 0.4]   // dust, rain streaks, splash height, cavity grime
groundY:    0
dustColor:  0x6b6154
grimeColor: 0x2a2620
rustColor:  0x6d3a1c
```

`weather.w` (cavity grime) is carried in the same `vec4` and documented here, but
this layer never reads it — its consumer is the source's "cavity + vertex masks"
section (lines 569–571), i.e. the `masks` sibling. The zero-term test asserts
that moving `.w` changes nothing in this layer, so a future edit that quietly
wires it in fails.

## The `#ifdef`s, as data — and the one place the source does not manage it

The source guards the section with `OW_WEATHER` (on when any of
`weather.x/.y/.z` > 0) and the stain block inside the runoff pass with
`OW_VCOL_MASKS`. Growing one program permutation per define would fight the
content-addressed program identity for nothing, so both are **values**:
`vcol_masks` is `1.0`/`0.0` and selects with a `mix`, and a zero weather term
disables its own sub-pass arithmetically.

That is only sound if it is *bit-identical*, so
`tests::a_zero_weather_term_disables_its_sub_pass_bit_identically` checks all
four terms over all 24 samples. Three of the four are exact no-ops. The fourth is
a **source defect this port preserves**:

> `orm.g = clamp( orm.g + splash * 0.16 - band * vert * 0.10, 0.0, 1.0 );`
> (source line 548)

The `- band * vert * 0.10` term sits **outside** the `step(1e-4, owWeatherP.z)`
gate that disables the rest of the splash. So a material with the ground splash
switched off still has its roughness pulled down at the base of every vertical
face, by up to 0.10. Transcribed as written — "dead computation in the source is
still part of the source" — and pinned by the test, so that if the app ever wants
it fixed, that is a deliberate divergence with a name rather than an accident.

## Assumptions about sibling layers — please check these

Nothing outside `weathering.rs` was written or edited. These are the names and
shapes this layer assumes; each is either a *value it is handed* or a *shared
derivation it also defines*.

1. **`mac1` / `mac2` are passed in as `vec4<f32>`**, the two macro-noise fetches
   the source makes at lines 406–407. This layer never samples them itself; the
   `macro` sibling owns their UVs and fetches. It consumes `mac1.r`, `mac1.b`,
   `mac2.b`, `mac2.g`.
2. **`owVert` and `owSAxis` are shared with the `patches` layer** — the source
   comment at line 445 says so outright ("shared by the patch and runoff
   layers"), and they sit at lines 446–447, *outside* my assigned 492–568 range.
   I define them as `ow_weather_vert` / `ow_weather_s_axis` because this layer
   cannot work without them. If `patches` also defines them, the orchestrator
   should keep exactly one pair; the names are prefixed so nothing collides in
   the meantime.
3. **`ow_weather_smoothstep` / `ow_weather_mix3` are prefixed** for the same
   reason. If several layers write out the same GLSL shims, they are worth
   promoting into one shared preamble — I did not create one, because that is a
   shared file.
4. **`n_flat` is `owP2V * owNp`, already transformed** into `n_shade`'s space.
   The object-vs-world choice (`OW_OBJECT_SPACE`, source lines 258–266) is made
   above this layer, so the matrix product is the composer's, not mine.
5. **The `OW_VCOL_MASKS` stain block (lines 513–524) is implemented here**, since
   it is textually inside the runoff pass, and is gated by a `vcol_masks: f32`
   value. It overlaps the `masks` layer's subject matter; if `masks` claims it,
   delete `ow_weather_rain`'s `stained` term and pass `vcol_masks = 0.0`, which
   the test proves is bit-identical to it never having existed.
6. **`weather.w` is the `masks` layer's** (see above).

## Not blocking, but worth the orchestrator knowing

* **`dead_code` on the lib target.** Every `pub(crate)` item in this layer is
  currently used only by its own tests, so `cargo clippy --all-targets -D
  warnings` reports `constant/function/struct … is never used` for it (6 + 19 +
  2). This is shared by all twelve layers and clears the moment `mod.rs` composes
  them into `axiom_surface`. No `#[allow]` was added: silencing it here would
  outlive the reason for it.
* **A coverage trap several layers will hit.** A multi-line `assert!` whose
  *format arguments* are separate expressions creates a region that no passing
  test can reach — the arguments are only evaluated when the assert fires. Bind
  them to locals first and interpolate by name. Two of this file's lines were
  uncovered for exactly that reason, and `masks.rs` had the identical two-line
  miss when last measured. Likewise, one `&&` in a test assertion was the only
  branch region in the file; splitting it into two asserts took the branch column
  from 50% to no branch regions at all.
* `clippy::too_many_arguments` is allowed on `rain`, `splash` and `wedge` with a
  stated reason: the argument lists are the source's data flow under the brief's
  explicit-argument calling convention. The same allow already appears in
  `axiom-host`, `axiom-frame` and `axiom-windowing`.

## Status

`cargo test -p axiom-gpu-backend --lib --features offscreen material_shader::weathering`
— **17 passed**, including the real-adapter parity sweep.

Coverage (`cargo llvm-cov --branch -p axiom-gpu-backend --lib`, i.e. the gate's
feature set, MSVC nightly): `weathering.rs` **100.00% regions, 100.00%
functions, 100.00% lines, zero branch regions**.
