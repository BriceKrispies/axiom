# Material shader layer: cavity + vertex-colour masks

**File:** `modules/axiom-gpu-backend/src/material_shader/masks.rs`
**Source:** `C:/dev/Claude-of-Duty/src/materials/shader.js:568-596` (the
"cavity + vertex masks" section of `MAIN_FRAGMENT`), plus `shader.js:678` (the
`aomap_fragment` override, which is where `aoStrength` is actually spent), and
the `wear` / `vertexMasks` / `aoStrength` entries of `DEFAULT_PARAMS`
(`shader.js:756`, `:774`, `:776`).

## The two WGSL entry points

```wgsl
struct AxiomMasksOut { albedo: vec3<f32>, orm: vec3<f32> };

fn axiom_masks_apply(
    albedo: vec3<f32>, orm: vec3<f32>, height_s: f32, vertex_color: vec3<f32>,
    mac1: vec4<f32>, mac2: vec4<f32>, grime_color: vec3<f32>, wear_color: vec3<f32>,
    wear_material: vec4<f32>, wear_params: vec4<f32>, weather_params: vec4<f32>,
    vertex_masks: bool,
) -> AxiomMasksOut

fn axiom_masks_ambient_occlusion(occlusion: f32, ao_strength: f32) -> f32
```

Free functions over explicit arguments: no globals, no `params.slots`, no
assumed binding index (a test asserts the WGSL contains no `@group`,
`@binding` or `var<uniform>`). `orm` is the source's `owORM` — `x = ao`,
`y = roughness`, `z = metalness`. Three private helpers
(`axiom_masks_mix`, `axiom_masks_mix3`, `axiom_masks_smoothstep`) are also in
the constant.

CPU reference: `apply(&MaskInputs) -> MaskLayer` and
`ambient_occlusion(occlusion, ao_strength)`, both `pub(crate)`.

## Cavity is not a derivative — no `fwidth`-shaped parameter needed

The brief flagged this as a risk, and it turns out not to bite. The source's
cavity term is

```glsl
float cav = 1.0 - owHeightS;
```

— the plain complement of the height field, **not** `dpdx`/`dpdy` and not a
curvature estimate. There is no screen-space derivative anywhere in this
section, so this layer does **not** need the explicit-derivative parameter
`apps/shmup/src/sky/dome.rs` had to introduce for `skSunDisc`'s `fwidth`. The
one implicit input is `owHeightS` itself, a fragment-local written earlier in
`MAIN_FRAGMENT` by the POM / detail / patch layers (`shader.js:321`, `:393`,
`:487`) — and that is an ordinary value, taken here as the `height_s` argument.

The real curvature in this system is **baked per vertex** by `materials/masks.js`
(convex → wear, concave → grime + AO) and arrives as `vColor`. That is the whole
point of the layer: the masks the geometry side pins are consumed here.

## The three `wear` channels are three masks, and `wear[3]` is dead

`materials/masks.js:11` fixes the channel meanings and the shader honours them
separately, each with its own strength lane out of `owWearP`:

| vertex channel | strength | what it drives |
|---|---|---|
| `vColor.r` | `wear[0]` | albedo → `wearColor` (scaled by `wearMaterial.w`), roughness/metalness *lerped* to `wearMaterial.xy` |
| `vColor.g` | `wear[1]` | albedo → `grimeColor`, roughness `+`, metalness `×(1-…)` |
| `vColor.b` | `wear[2]` | AO `×(1 - vColor.b·wear[2])` — the only channel that touches AO |

`the_three_wear_channels_are_separate_masks_with_separate_strengths` drives each
alone and asserts the other two lanes are bit-unmoved, so a future collapse of
any pair fails.

**`wear[3]` is uploaded and never read.** `shader.js:91` declares `owWearP` as
`x wear amt, y grime amt, z vcol AO amt, w curvature`, but a grep of the whole
source finds only the declaration, the upload (`shader.js:826`) and this layer's
three reads of `.x`/`.y`/`.z`. `DEFAULT_PARAMS.wear` agrees with the code rather
than the comment: `[0.5, 0.7, 0.5, 0]`, commented "wear, grime, extra AO,
**unused**". The uniform comment is stale — the "curvature" it names is the
per-vertex bake, not a scalar.

Ported as the source has it: `MaskInputs::wear_params` is the whole `vec4`, the
lane is named and documented, and nothing reads it.
`the_fourth_wear_lane_is_unused_by_the_source_and_moving_it_changes_nothing`
pins that, on both sides of the mask flag. Dropping the lane would have been
tidier and wrong; inventing a use for it would have been worse.

## `aoStrength` lerps toward 1

`shader.js:678`:

```glsl
float ambientOcclusion = ( owORM.r - 1.0 ) * owAoAmt + 1.0;
```

That is `mix(1, ao, aoStrength)`, **not** `ao * aoStrength`. The two agree only
where `aoStrength == 1` or `ao == 1`; everywhere else the multiply is darker,
and at `aoStrength == 0` it would black the indirect diffuse out where the
source disables occlusion entirely. Transcribed in the source's grouping —
`(ao - 1) * s + 1`, not `1 + s*(ao - 1)` — because float addition is not
associative and the source's grouping is the specification.

Note where it lives: the term is applied at the `aomap_fragment` chunk, i.e. in
the *lighting* stage, not inside `MAIN_FRAGMENT`. It is ported here because it
is the sole consumer of the occlusion this layer produces, and the brief
assigned `aoStrength` to this layer. The composing orchestrator should call
`axiom_masks_ambient_occlusion` where the engine applies AO to indirect
diffuse — not inside `axiom_surface`, which returns channels rather than light.

## `vertexMasks: false` must be bit-identical, and is

`DEFAULT_PARAMS.vertexMasks` is `false`, and in the source `OW_VCOL_MASKS` is a
**compile-time** define: with masks off the block is not in the program at all.
A runtime port cannot have two programs behind one function, so the disable is a
selection, never a blend:

- WGSL: `select(cav_value, masked_value, vertex_masks)` — returns one operand
  unchanged.
- Rust: `[(cav…), (masked…)][usize::from(input.vertex_masks)]` — the Branchless
  Law's table index, same property.

A `mix(a, b, 0.0)` would have been the natural-looking alternative and is
subtly wrong (`a*1 + b*0` is only *usually* `a`). Verified at the boundary
twice:

- CPU, `disabling_the_vertex_masks_is_bit_identical_whatever_the_mask_params_say`
  — every mask input set to zero vs. set to 9, compared with `to_bits()`, and
  the result additionally shown to equal the cavity half computed alone.
- GPU, `the_disabled_path_is_bit_identical_on_the_gpu` — the same two samples
  rendered, compared with `to_bits()`, plus a third sample with the flag on to
  prove the flag is not simply inert.

## Divergences from the source, and why

1. **`mix` and `smoothstep` are written out by hand in the WGSL** from their
   GLSL spec expressions (`x*(1-a)+y*a`; `t=clamp((x-e0)/(e1-e0),0,1); t*t*(3-2t)`)
   rather than calling the WGSL builtins. Same choice, same reason, as
   `surface_program::emit`: a builtin's internal factoring is unspecified, so
   calling it would insert an unmeasurable difference between the shader and the
   CPU reference. `clamp` *is* the builtin on the GPU — GLSL and WGSL both define
   it as `min(max(e, low), high)`, no freedom. On the CPU it is written as
   `x.max(low).min(high)` rather than `f32::clamp`, which panics on `low > high`
   where GLSL returns `high`.
2. **The `#ifdef` becomes a runtime `bool`.** Unavoidable; the bit-identity
   tests above are the price paid for it.
3. Nothing else. No division became a reciprocal-multiply (the only division in
   the layer is inside `smoothstep`, written as a division on both sides), no
   multiply chain was re-associated, and no grouping was tidied. `grimeM` is
   left **unclamped**, as the source leaves it, so above 1 the two mixes that
   follow extrapolate and `1 - grimeM*0.8` goes negative — one parity sample
   sits in that region deliberately.

## Storage width

`f32` on both sides. The GPU has no choice, and an `f64` CPU reference would
make the parity tolerance measure the width difference rather than the hardware.
The one place `f64` appears is the independent Python transcription used to pin
`the_pinned_case_matches_an_independent_transcription_of_the_glsl`; that test
carries a `1e-6` tolerance and that number *is* the `f64`↔`f32` gap, nothing
else.

## How correctness is proven

Following the `sky/` lesson that a "second transcription" written by reading
one's own Rust shares its mistakes, the pinned values come from a Python
transcription written from the **GLSL text**, not from the Rust. It was written
before the Rust reference and the numbers were not adjusted afterwards.

CPU↔GPU parity follows `surface_program/parity.rs`: acquire a real adapter and
**assert** on it (`Backend::Noop` fails the test — no silent skip), render one
fragment per sample into an `Rgba32Float` target (an `Rgba8Unorm` one quantises
to 1/255, four orders of magnitude coarser than the budget), read four lanes
back. Two draws per run, `masks_albedo_fs` (rgb + the AO the frame applies, so
`axiom_masks_ambient_occlusion` is proven over *this layer's own* occlusion
rather than at an invented value) and `masks_orm_fs`.

The parity module carries its own small device harness rather than borrowing
`surface_program::parity::ParityGpu`, which is `pub(super)` inside
`surface_program` — reaching for it would have meant editing a file this layer
does not own.

**Sample table (16):** shared between the CPU tests and the parity test, so it
is covered with or without the `offscreen` feature. It reaches `cav` at both
ends, `wearM` clamped to 1 and pinned at 0, `grimeM > 1`, both smoothsteps below
`edge0` and above `edge1`, cavity grime off and at full strength, an unpainted
mesh (`vColor = 0`), the AO channel alone, a metal `wearMaterial`, and the mask
flag both ways over otherwise identical inputs.

## Measured tolerance

**Measured worst lane delta: `5.9604645e-8` on a Vulkan adapter**, over all 16
samples × 2 entry points × 4 lanes. That is exactly `2^-24` — **one `f32` ULP at
1.0**, i.e. as close as two independent `f32` evaluations can be without being
equal. The layer has no transcendentals and one division, and every `mix` /
`smoothstep` is written out on both sides, so the only difference left is the
hardware's freedom to contract `a*b + c` into an `fma`.

**Tolerance set to `4e-7`** — 6.7x the measurement, 3.4x the ULP floor, inside
the brief's 10x rule with room for a driver that contracts differently, and
three orders of magnitude tighter than the `1e-4` the field algebra's operators
need. It was derived from the measurement, not fitted: the first draft used
`1e-6`, which passed only because the "within 10x" guard floors the measurement
at `f32::EPSILON`, and it was tightened once the real number was read.

`the_tolerance_is_within_ten_times_the_measured_hardware_delta` re-measures
every run and fails if the budget drifts loose. (The `f32::EPSILON` floor in
that guard exists so an adapter that agreed bit-for-bit could not force every
positive budget to fail; it is not what carries the test here.)

## Verification run

In-repo, in the real crate, on `stable-x86_64-pc-windows-msvc`:

```
cargo test -p axiom-gpu-backend --lib --features offscreen material_shader::masks
  -> 12 passed (9 CPU + 3 GPU), Vulkan backend
cargo test -p axiom-gpu-backend --lib material_shader::masks
  -> 9 passed (the CPU reference, with the parity module cfg'd out)
```

Both configurations matter: the coverage gate does **not** pass `--features
offscreen`, so every line of the CPU reference is reached by the 9 tests that run
without it, and the sample table is shared with the parity test precisely so it
cannot become dead code in the un-featured build.

Worth recording for the next agent: for most of this work the shared crate did
not compile with `--features offscreen`, because several sibling layers were
mid-flight (`macro_variation.rs` and two others, `E0063`/`E0382`/`E0507`/`E0583`).
Rather than touch a sibling's file, the measurement above was first taken on a
byte-identical copy of this one file in a throwaway crate outside the workspace —
possible because this file has **zero** Axiom dependencies, only `wgpu`,
`pollster` and `std`, with the `#[cfg(all(test, feature = "offscreen"))]` on the
parity module rewritten to `#[cfg(test)]`. That is a cheap escape hatch for any
layer in this fan-out that needs a real adapter while the crate is churning. The
in-repo run was then repeated once the siblings landed, and agrees.

## What this layer needs from its siblings

Nothing is imported and no sibling file was touched. When the orchestrator
composes `axiom_surface`, this layer wants, in the source's order:

- `albedo` / `orm` as the **weathering** layer leaves them (`shader.js:567`).
- `height_s` = `owHeightS` **after** POM/detail (`:321`, `:393`) *and* after the
  repair-patch layer's `owHeightS = clamp(owHeightS + pm*0.07 + lip*0.05, 0, 1)`
  (`:487`). Taking the pre-patch value would silently change every cavity term.
- `mac1` / `mac2`, the two macro-noise samples (`:406-407`), unmodified.
- `grime_color`, `wear_color`, `wear_material`, `wear_params`, `weather_params`
  from the parameter block (`material_shader/params.rs`), as whole `vec4`s.
- Downstream: the **cloth** layer (`:598`) and then **tint** (`:622`) run after
  this one, and `axiom_masks_ambient_occlusion` belongs at the engine's AO
  application site, not in `axiom_surface`.

The three helper names (`axiom_masks_mix`, `axiom_masks_mix3`,
`axiom_masks_smoothstep`) are prefixed so a sibling defining its own `mix`
helper does not collide when the layers are concatenated.

The producer of `vColor` is already ported and golden-pinned on the app side:
`apps/shmup/src/materials/masks.rs` ("`r = edge wear, g = grime, b = extra AO`",
line 15). This layer is the consumer that finally spends it.
