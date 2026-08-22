# `material_shader/macro_variation` — the macro-variation layer

Ported from `C:/dev/Claude-of-Duty/src/materials/shader.js`, `MAIN_FRAGMENT`
lines 402-443, plus `DEFAULT_PARAMS.macro` / `.macroBig` / `.macroRelief`
(lines 729-749) and the `OW_MACRO_RELIEF` define (line 856).

File: `modules/axiom-gpu-backend/src/material_shader/macro_variation.rs`.
Nothing else was touched.

---

## 1. What the layer is

Large-scale break-up. A tiled surface reads as one flat colour across a 12 m
wall; this layer samples a shared macro noise map in **world space** (never uv
space — a break-up that rode the tiling would repeat with it) and modulates
albedo, roughness and hue from it, then adds a second, much coarser band and an
optional normal tilt.

Data flow, in the source's own order:

```
owUpFace  = step(0.62, |owNw.y|)
macroUv   = mix(wall projection, floor projection, owUpFace)
mac1      = tex(macroUv * macro.x)
mac2      = tex(macroUv * macro.x * 0.211 + 0.37)
macro     = clamp((mac1.r*0.55 + mac2.b*0.45 - 0.5) * macroBig.x + 0.5, 0, 1)
alb      *= mix(1, 0.55 + 0.92*macro, macro.y)          <- ALBEDO strength
[macroBig.y > 0]  big band -> alb *= 1 + big*amp ; orm.g -= big*amp*0.55
alb      *= mix(vec3(1), vec3(1.05,1,0.93), (mac2.r-0.5)*macro.w)   <- HUE strength
orm.g     = clamp(orm.g + (mac1.g-0.5)*macro.z + (mac1.a-0.5)*0.16
                        - owMicro*0.07*owDetFade, 0, 1) <- ROUGHNESS strength
[macroRelief > 0] gradient tilt of nShade + a mac1.b albedo darkening
```

### The four strengths are four knobs

`macro = [worldScale, albedoAmt, roughAmt, hueAmt]`. They are **not**
interchangeable and none of them was collapsed:

- `macro[1]` is **achromatic**: one scalar multiplier `0.55 + 0.92*macro`
  applied to all three channels, so it changes value and leaves the channel
  ratios alone.
- `macro[3]` is **chromatic**: a per-channel multiplier lerped between
  `vec3(1)` and `vec3(1.05, 1.0, 0.93)`, with a **signed** `t`
  (`(mac2.r - 0.5) * hueAmt`), so it extrapolates past white the other way for
  `mac2.r < 0.5`. It warms/cools; it does not darken.
- `macro[2]` touches `orm.g` only.
- `macro[0]` scales the sample coordinates, so it moves everything downstream.

`the_four_macro_strengths_are_independent_and_the_hue_term_is_chromatic` pins
all four separately, including the ratio test that separates the value term from
the hue term.

---

## 2. Traps, and what was done about each

### 2.1 The contrast expansion is centred on `0.5`

```glsl
clamp( ( mac1.r * 0.55 + mac2.b * 0.45 - 0.5 ) * owMacroBig.x + 0.5, 0.0, 1.0 )
```

Subtract the midpoint, scale, add it back — that order. Two plausible tidyings
are both wrong and both change *brightness* rather than contrast: scaling first
and re-centring afterwards, and re-centring around the mean of the two bands
instead of the constant `0.5`. Pinned by
`the_contrast_expansion_is_centred_on_the_midpoint`, which recovers `macro` from
the albedo multiplier and asserts that contrast `0` lands **exactly** on the
midpoint, that raising the contrast moves the sample further from it, and that
it never crosses it.

Note the two band weights `0.55 / 0.45` sum to 1 but the two bands are `mac1.r`
and `mac2.**b**` — different channels of different fetches. The big band uses a
different pair again (`0.62 / 0.38`, `.r` and `.b`).

### 2.2 The `0.62` horizon is inclusive

GLSL `step(edge, x)` is `1.0` when `x >= edge`. So a face at exactly
`|n.y| == 0.62` **is** up-facing: it takes the floor projection for `macroUv`
*and* it gets ruts. `>` instead of `>=` would flip both, for that face and
(through the `mix`) for the whole projection choice.

`the_up_face_horizon_is_inclusive_at_exactly_0_62` asserts `up_face == 1.0` at
exactly `0.62` and `0.0` one ULP below, and that the two took *different*
projections so the horizon is load-bearing rather than cosmetic. WGSL's `step`
has the same `>=` definition, so the shader side needed no adjustment; the CPU
reference spells it out as `[0.0, 1.0][usize::from(x >= edge)]`.

### 2.3 A default of `0` disables, bit-identically — verified, not assumed

Both `macroBig[1]` (big-band amplitude) and `macroRelief` default to `0`, and
**neither is a multiply-by-zero that quietly vanishes**:

- The relief block ends in `normalize( nShade + ... )`. Renormalizing an
  already-unit `f32` vector is **not** the identity, so an amplitude-zero relief
  block would still perturb the shading normal.
- The relief block's albedo term is `1.0 - (mac1.b - 0.5) * 0.16 * owUpFace`.
  It contains no `macroRelief` factor at all — a zero amplitude would not
  silence it. Only the `#ifdef` does.

So both stay real gates. In the WGSL they are runtime `if`s on exactly the
source's predicates (`owMacroBig.y > 0.0`, and `macroRelief > 0` which is
precisely the condition `applyOwMaterial` sets `OW_MACRO_RELIEF` on). In the
Rust — where the Branchless Law forbids an `if` — they are value *selections*
via `[untouched, modified][usize::from(gate)]`, which is bit-identical to
skipping the block.

`the_disabled_defaults_are_bit_identical` proves it **on bits, not a tolerance**:

- with `macroBig[1] == 0`, changing `macroBig[2]` (the big band's world scale,
  which nothing else reads) from `0.03` to `5.75` moves not one bit of albedo or
  roughness — and raising the amplitude *does* move them, so the check is not
  passing because the band is inert;
- with `macroRelief == 0` and a deliberately **non-unit** input shading normal
  `(0, 0, 2)`, the output is exactly `(0, 0, 2)`. A stray `normalize` cannot
  hide behind that;
- and the relief albedo factor is re-derived independently in the test
  (`1 - (mac1.b - 0.5) * 0.16 * upFace`) and asserted bit-equal against the
  enabled path.

The GPU side gets the same proof: parity case 15 carries the non-unit normal
with relief off, and the parity test asserts the rendered lane is **bit-equal**
to `(0, 0, 2, 1)`.

### 2.4 `macroBig[3]` is unused, and stays unused

The source's own comment marks the fourth word `unused`, and nothing in
`shader.js` reads `owMacroBig.w`. It is carried in the port's parameter struct
(`MacroVariationIn::macro_big` is a `[f32; 4]`, documented as such) and read
nowhere. `the_wgsl_declares_the_layers_entry_point_and_result_struct` asserts
the WGSL contains no `macro_big.w`, so a later hand cannot quietly invent a
meaning for it. Dropping the word from the layout would have been the other
error — it is part of the uniform's shape whether or not anything consumes it.

### 2.5 Grouping and associativity

Everything is transcribed left-to-right from the GLSL text, not from the Rust:

- `macroUv * owMacroP.x * 0.211 + 0.37` is `((uv * scale) * 0.211) + 0.37`. The
  `0.211 * scale` fold is **not** taken.
- `big * owMacroBig.y * 0.55` is `(big * amp) * 0.55`.
- `orm.g + a + b - c` is `((orm.g + a) + b) - c`.
- `( vec2(mhx, mhy) - mac1.b ) * owMacroRelief * owUpFace` is
  `((v - b) * relief) * upFace`.
- No division in this layer became a reciprocal-multiply, because the layer
  contains exactly one division: the `normalize` at the end of the relief block,
  which is written as a division on the CPU side.
- **`mix(1.0, 1.0, t)` is left alone.** The hue term's green lane is
  `mix(vec3(1)..., vec3(...,1.0,...), t)`, i.e. `1.0*(1-t) + 1.0*t`, which is
  *not* identically `1.0` in `f32`. Collapsing it to `1.0` would be exactly the
  tidying the brief warns about. Both sides write `mix` as
  `x*(1-a) + y*a`, which is how GLSL and WGSL both define it (never
  `x + a*(y-x)`).

`clamp` is written as `min(max(x, lo), hi)` on the CPU rather than
`f32::clamp`, whose bound assertion and NaN rule are Rust's, not the shading
language's.

---

## 3. The deliberate divergence: four hoisted texture fetches

The source takes its gated samples *inside* the gates:

```glsl
if ( owMacroBig.y > 0.0 ) { ... texture2D(...) ... }
#ifdef OW_MACRO_RELIEF     ... texture2D(...) ...
```

WGSL forbids an implicit-LOD `textureSample` under control flow it cannot prove
uniform, and whether a parameter is uniform is a property of the **call site** —
a contract this layer cannot impose on the orchestrator that composes it. The
two alternatives were:

1. keep the fetches inside the gates and rely on WGSL's uniformity analysis
   propagating through function parameters (works, but silently binds the
   orchestrator to pass a uniform-derived argument);
2. switch to `textureSampleLevel(..., 0.0)` (kills mipping — a real visual
   change at distance, which is precisely where the macro band matters);
3. hoist the four fetches above their gates.

**Option 3 was taken.** Texture sampling is pure, so every value the gates
consume is bit-identical and the gates still decide whether it is *used*.
The cost is four fetches a disabled permutation would not have paid. If that
cost matters, the right fix is a program permutation at the orchestrator level
(the content-addressed program identity already gives one pipeline per distinct
program), not a change here.

Nothing else diverges.

---

## 4. What the parity test actually pins

`the_macro_layer_agrees_with_its_cpu_reference_on_a_real_adapter`, gated
`#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]`.

- **A real adapter, or a loud failure.** It `expect`s an adapter and asserts the
  backend is not `Noop`. It never skips.
- **16 cases, 4 output vectors each**, rendered into one `64 x 1`
  `Rgba32Float` target — column `i*4 + k` emits case `i`'s `k`-th result vector,
  so all sixteen output lanes per case come back from one pass.
- The cases cover: both sides of the `0.62` horizon **and exactly on it**; a
  floor, a ceiling and two walls; each gate off, each on, both on; a skewed view
  basis; negative world coordinates (the repeat wrap sign-extends); a zero
  albedo strength; a negative hue strength; both ends of the roughness clamp;
  a contrast that saturates `macro` at both ends; and the non-unit-normal
  bit-identity case.
- **It is checked for vacuity three ways**: the rendered lanes must vary; case 5
  must differ from case 0 (the big band did something); case 6's normal lane
  must differ from case 0's (the relief tilt did something).

### The macro texture, and what it is

The source's `owMacroTex` is `shared.macro`, an authored fbm map. There is no
such artifact to compare against here, and this layer's content is the
*arithmetic*, not the map. So `MacroNoise::procedural` fills a `64 x 64` RGBA
texture from a 32-bit integer avalanche — four independent channels per texel,
each `n / 2^24` for `n < 2^24`, so every value is exact in an `f32`.

The **same texels** are uploaded to the GPU (as `Rgba32Float`, so the round trip
is lossless — an 8-bit format would have put the CPU's `n/255` against the GPU's
decoder and measured two decoders rather than this layer) and read by the CPU
reference. Sampling is **nearest** with **repeat** addressing on both sides, so
a texel fetch is exact and no filter-weight quantisation enters the comparison.

Two consequences worth stating plainly:

- What is pinned is every multiply, lerp, clamp, gate and normalize *between*
  the fetch and the result. **Filtering and mip selection are not pinned** —
  a bilinear or mip-chain defect in the real material would not be caught here.
- A uniform-random texel drives the clamps into saturation at both ends far
  more often than an authored fbm map would, which is a coverage advantage.

### Tolerance: `1.0e-6`, from a measured `1.79e-7`

Measured, not fitted. `MEASURED_WORST = 1.79e-7` is the worst absolute lane
delta the sweep showed on the recording adapter (Windows, `wgpu` default
backend; the failure message names the live one). Output lanes here live in
`0..=1.6`, so that is about `1.5e-7` **relative** — the last mantissa bit or
two, which is what `mix`, a three-column `mat3 * vec3` and a `normalize`
reciprocal are each free to cost on hardware that may contract to `fma` or
evaluate `rsqrt` at its own precision.

`TOLERANCE = 1.0e-6` is 5.6x that. Following
`surface_program::parity_transcendental`'s shape, the test holds **three**
relations every run, so neither the record nor the budget can quietly stop
describing the hardware:

1. every lane is inside `TOLERANCE`;
2. the live worst delta is within `DRIFT_LIMIT` (2x) of `MEASURED_WORST` — the
   committed record is still true;
3. `TOLERANCE` is no more than `SLACK_LIMIT` (10x) above the **live** delta —
   being too generous fails here.

`the_tolerance_is_not_loose_against_the_recorded_measurement` holds the same
relation against the committed record without touching a GPU.

Both sides compute in **`f32`** throughout. The GPU has no choice, and a `f64`
reference would have been measuring its own extra precision rather than this
layer's transcription.

---

## 5. Coverage

`macro_variation.rs`: **regions 100% (579/579), functions 100% (37/37), lines
100% (476/476), branches 0** — measured with
`cargo llvm-cov --branch -p axiom-gpu-backend --lib` on
`nightly-x86_64-pc-windows-msvc`, *without* `--features offscreen`, i.e. exactly
the configuration the coverage gate runs. Every non-test item — the WGSL
constant, `MacroNoise`, the sampler, the seven scalar/vector helpers and
`macro_variation` itself — is reached by a native test; nothing depends on the
GPU-gated arm for its coverage.

"Branches 0" is not a gap: the file's non-test Rust contains **zero** control
flow (no `if`/`match`/`for`/`while`/`&&`/`||`/`?`), which is the Branchless Law
holding. The only two branch arms llvm-cov originally found were `||`
short-circuits in one *test* predicate; that test now uses
`[a, b, c].contains(&x)` instead, so the column reads clean rather than
misleading.

No `#[allow]`, no `#[coverage(off)]`, no ignore pattern.

---

## 6. What the orchestrator needs to know

### 6.1 The entry point

```wgsl
fn axiom_macro_variation(
    world_pos: vec3<f32>,        // vOwWPos
    world_normal: vec3<f32>,     // owNw — normalized, already * owFaceDir
    albedo_in: vec3<f32>,        // alb.rgb
    roughness_in: f32,           // orm.g
    shade_normal_in: vec3<f32>,  // nShade, VIEW space
    view_from_world: mat3x3<f32>,// mat3( viewMatrix )
    micro: f32,                  // owMicro   (from the detail layer)
    det_fade: f32,               // owDetFade (from the detail layer)
    macro_p: vec4<f32>,          // owMacroP
    macro_big: vec4<f32>,        // owMacroBig
    macro_relief: f32,           // owMacroRelief
    macro_tex: texture_2d<f32>,
    macro_smp: sampler,
) -> AxiomMacroVariation
```

`AxiomMacroVariation { albedo, roughness, shade_normal, up_face, mac1, mac2 }`.

It reads no globals, no `params.slots`, and assumes no binding index. The
texture and sampler are parameters, so the layer is self-contained.

### 6.2 Cross-layer contracts

**Inbound** — `micro` and `det_fade` are the detail layer's `owMicro` /
`owDetFade`. On a fragment where the detail layer did not run they are `0.0` in
the source (declared `float owMicro = 0.0; float owDetFade = 0.0;` at line
273-274), and passing `0.0` here reproduces that exactly.

**Outbound** — this is the important one. `mac1`, `mac2` and `owUpFace` do
**not** stay inside the macro section; five later sections of `shader.js` read
them directly, so they are returned rather than discarded:

| consumer | reads | source line |
|---|---|---|
| repair patches — lattice wander | `mac2.r`, `mac2.g` | 460 |
| weathering — dust wedge | `mac1.b`, `mac2.g` | 495 |
| weathering — ground splash | `mac1.b`, `mac2.g` | 544 |
| weathering — wedge height/shape | `mac1.r`, `mac2.b`, `mac2.g` | 557-559 |
| masks — wear / grime | `mac1.b`, `mac2.a`, `mac2.g` | 584, 591 |

Whoever composes `axiom_surface` must thread these through; recomputing them in
a later layer would double four texture fetches and, worse, would be a second
definition of the macro uv able to drift from this one.

`up_face` is `step(0.62, abs(owNw.y))`. Note the *later* sections compute their
own, different facing terms (`owVert = smoothstep(0.72, 0.34, |owNw.y|)` at line
446, `owDown = smoothstep(0.10, -0.70, owNw.y)` at 603). Those are not this one
and must not be folded into it.

### 6.3 Parameter packing

Not this layer's business — `macro_p`, `macro_big` and `macro_relief` arrive as
explicit arguments. The slot assignment belongs in `material_shader/params.rs`.
For that file's author: `macro` and `macroBig` are one `vec4` each and
`macroRelief` is a lone scalar sharing a `vec4` with whatever else the layout
puts there. `macroBig[3]` is dead and must still be reserved.

### 6.4 Sibling assumptions

**None.** The layer takes world position and world normal directly, so it does
not name anything from `frames.rs` or `uv_mode.rs`, and neither file was read or
written. In particular it does **not** consume the `uv` the uv-mode layer
produces: `macroUv` is derived from world space on purpose, which is what makes
the break-up survive the tiling.

### 6.5 Two things the orchestrator will hit

1. **Dead-code warnings until the layer is spliced.** `cargo build -p
   axiom-gpu-backend` warns that `MACRO_VARIATION_WGSL`, `MACRO_NOISE_SIZE`,
   `MacroNoise` and `macro_variation` are never used — nothing in the crate
   composes them yet. They are silent under `cargo test` (the tests use all of
   them) and will go away the moment `axiom_surface` splices the layer in. No
   `#[allow(dead_code)]` was added, deliberately: a suppression added now would
   outlive the reason for it.
2. **Twelve copies of a GPU harness.** Because `surface_program::parity`'s
   `ParityGpu` is `pub(super)` and `surface_program/` is off-limits to a layer
   author, this file carries its own adapter acquisition, texture upload,
   pipeline and readback (~250 lines of test code). Every layer that needs a
   real adapter will duplicate it. The right home for it is a shared
   `material_shader/parity_gpu.rs` — but creating one is an orchestrator
   decision about a shared file, not a layer author's. **Recommended:** lift it
   once the fan-out lands; this file's copy is a reasonable donor, since it
   already handles the texture + sampler bindings the field-algebra harness
   never needed.

---

## 7. Source defects found

None. The macro section transcribes cleanly; the `macroBig[3]` word is
documented-unused rather than accidentally dead, and the double evaluation of
`step(0.62, abs(owNw.y))` (once inline in the `mix`, once as `owUpFace`) is
redundant but exact.

One observation, not a defect: `macroBig`'s doc comment says
"`1/bigWorldScale` is the period of the macro texture in metres, and its
coarsest band is a third of that — so 0.028 gives ~12 m features", while
`DEFAULT_PARAMS` ships `0.03` (~11 m) with the amplitude at `0`, i.e. the band
is off by default and the documented `0.028` is the value an authored material is
expected to opt into. Both numbers are carried through faithfully; parity case 5
uses `0.028` so the documented configuration is the one under test.
