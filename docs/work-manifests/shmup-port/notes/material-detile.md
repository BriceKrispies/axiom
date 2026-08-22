# Material shader layer: de-tiling and the height blend

**File:** `modules/axiom-gpu-backend/src/material_shader/detile.rs`
**Source:** `C:/dev/Claude-of-Duty/src/materials/shader.js`

| source | lines |
|---|---|
| `owHeightBlend` (in `PARS_FRAGMENT`) | `shader.js:239-250` |
| the `#ifdef OW_DETILE` second-sample block (in `MAIN_FRAGMENT`) | `shader.js:359-372` |
| the second block: the detail fold on `n2`, `dtm`, the blend call | `shader.js:377-381` |
| `DEFAULT_PARAMS.detile` | `shader.js:750-751` |
| `owRoughP.z = p.detile`, and the `OW_DETILE` define | `shader.js:833-839`, `:852` |

## What the layer is for

A tiled texture repeats, and the eye finds the repeat. De-tiling takes a
**second sample of the same texture set** at a de-correlated place and blends
the two. A 50/50 lerp would only make mush, so the blend is
**height-preserving**: each sample's own height (its albedo alpha) buys it
weight, everything but the top `0.18` of weight is subtracted away, and the
survivors are renormalised. The taller sample wins its pixels outright, so the
result still reads as *one* material with grain and edges. Which of the two
dominates is chosen by a low-frequency mask read from the macro texture, so the
crossover itself wanders over metres and does not introduce a second grid.

## The WGSL

`pub(crate) const DETILE_WGSL: &str`. Free functions over explicit arguments —
textures and samplers included, which WGSL permits — so the orchestrator owns
every binding. A test asserts the constant contains no `@group`, `@binding`,
`var<uniform>` or `var<private>`.

```wgsl
struct AxiomDetileSample { albedo: vec4<f32>, orm: vec3<f32>, normal: vec3<f32> };

fn axiom_detile_warp(v: vec2<f32>) -> vec2<f32>
fn axiom_detile_uv(uv: vec2<f32>) -> vec2<f32>

fn axiom_detile_second_sample(
    base_map: texture_2d<f32>, base_sampler: sampler,
    rough_map: texture_2d<f32>, rough_sampler: sampler,
    normal_map: texture_2d<f32>, normal_sampler: sampler,
    uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>, normal_amp: f32,
) -> AxiomDetileSample

fn axiom_detile_mask_uv(object_pos: vec3<f32>, macro_scale: f32) -> vec2<f32>
fn axiom_detile_mask(
    macro_map: texture_2d<f32>, macro_sampler: sampler,
    object_pos: vec3<f32>, macro_scale: f32,
) -> f32

fn axiom_detile_fold_detail_normal(
    n2: vec3<f32>, dn: vec3<f32>, detail_normal_amt: f32, detail_fade: f32,
) -> vec3<f32>

fn axiom_detile_height_blend(
    a: ptr<function, vec4<f32>>,
    orm_a: ptr<function, vec3<f32>>,
    n_a: ptr<function, vec3<f32>>,
    b: vec4<f32>, orm_b: vec3<f32>, n_b: vec3<f32>, t: f32,
)
```

`orm` is the source's `owORM` — `x = ao`, `y = roughness`, `z = metalness`.

### The composition order the orchestrator must keep

The source interleaves this layer with the detail layer, and the interleave
matters: the detail normal is folded into the **base** sample *and* into the
second sample before they are blended, so the blend mixes like with like.

```wgsl
// after the base sample (uv_mode / pom) has produced alb / orm / nT:
let s2 = axiom_detile_second_sample(map, samp, rough, samp, nrm, samp,
                                    uv, ddx, ddy, normal_amp);
// ... the detail layer folds `dn` into nT here ...
let n2  = axiom_detile_fold_detail_normal(s2.normal, dn, detail_p.y, det_fade);
let dtm = axiom_detile_mask(macro_tex, samp, owP, macro_p.x);
axiom_detile_height_blend(&alb, &orm, &nT, s2.albedo, s2.orm, n2, dtm * rough_p.z);
```

The only reordering against the source is that `axiom_detile_second_sample`
takes its three fetches inside one call, where the source issues them a few
lines before the `dn` fetch. Nothing between them reads their results, and
texture fetch order carries no numerical consequence.

## `inout`, in order

`owHeightBlend` writes all three of its `inout` parameters. Two ordering facts
are load-bearing and are reproduced:

- `wa` and `wb` are **read to form `k`, then both overwritten using that same
  `k`** — the second write must not see the first's new value;
- `a` is **read on the right-hand side of its own assignment** (`a = (a*wa +
  b*wb) * inv`), and it is written before `ormA` and `nA`, which do not read it.

`ptr<function, T>` was chosen over returning a struct precisely because it keeps
that shape visible. `HeightBlend` on the Rust side lists its three fields in the
order the source writes them.

## `detile == 0` disables the extra fetches — structurally, not at runtime

The source gates the whole block with a preprocessor define:

```js
if (p.detile > 0 && p.uvMode !== 'triplanar') defines.OW_DETILE = '';
```

`detile_enabled(detile: f32, triplanar: bool) -> bool` is the port of that
condition, and it is a **compile-time** decision: when it is false the block is
not emitted, exactly as in the source.

**This was verified, not assumed, and the naive alternative fails.** Feeding
`t = 0` to the blend instead is *not* bit-identical to the un-detiled path. At
`t = 0` the second sample's weight collapses to zero and the first's collapses
to the `0.18` band, so the result is `a * wa * (1 / wa)` — a round trip through
two roundings. Over 200 000 random in-range operands that round trip differs
from `a` for **17.2%** of them, by up to **5.96e-8** (one ulp). The test
`a_runtime_zero_blend_is_not_bit_identical_to_the_undetiled_path` pins a
concrete case.

So: `DEFAULT_PARAMS.detile` is `0`, and for a default material this layer must
contribute *no WGSL at all*, not a no-op call.

## The de-correlating offset

It is a **rotation**, not a hash, and the constants are transcribed exactly:

```glsl
vec2 uv2 = vec2( uv.x * 0.803 - uv.y * 0.596, uv.x * 0.596 + uv.y * 0.803 ) * 0.617
         + vec2( 0.37, 0.71 );
```

Three things worth naming, because each is a way to get it silently wrong:

- `0.803² + 0.596² = 1.000025`, so the matrix is **not** exactly a rotation
  (≈36.6°, carrying a scale of ≈1.0000125). It is transcribed as written, not
  "corrected" to a unit pair.
- The `0.617` rescale is applied to the **rotated pair**, before the offset.
- The offset `(0.37, 0.71)` is applied to the **uv only**. `ddx2` and `ddy2` get
  the rotation and the rescale and nothing else — they are derivatives, and a
  constant offset has none. `axiom_detile_warp` is the shared part precisely so
  the two cannot drift apart.

## `textureGrad`, and the one place the source does not use it

The three second-sample fetches go through `OW_TEX`, which is
`textureGrad( t, uv, dx, dy )` (`shader.js:106-111`) — the whole reason this
shader cannot live in the field algebra. The warped uv has its own screen-space
footprint and inferring it picks the wrong mip. In WGSL that is
`textureSampleGrad`, and the derivatives are function parameters, so the layer
never touches `dpdx`/`dpdy` itself.

The **mask** fetch is different: the source reads `owMacroTex` with a plain
`texture2D`, not with `OW_TEX`. That is transcribed as `textureSample`. Since
the block is compile-time gated and never sits under a runtime branch, the
uniformity requirement is satisfied. A test pins the counts: exactly three
`textureSampleGrad(` and exactly one `textureSample(` in the constant.

## Storage width

Everything is `f32` on both sides. The GPU has no other width, and a CPU
reference computing in `f64` would need a tolerance to *hide* the difference
rather than measure it.

## Transcription decisions worth recording

- **`1.0 / max(wa + wb, 1e-4)` really is a reciprocal-then-multiply in the
  source.** This port's standing trap is a division rewritten as a
  reciprocal-multiply; here the source already wrote the reciprocal, so
  "fixing" it into three divisions would have been the defect. Transcribed as
  written.
- **`normalize` is called, not expanded.** The source calls GLSL `normalize`, so
  the WGSL calls WGSL `normalize`. The CPU reference is GLSL's *definition*,
  `v / length(v)` with `length = sqrt(dot(v, v))` — a division, not an
  `inversesqrt` multiply. The hardware is free to use `inversesqrt`; the
  measured tolerance below covers that, and it is the only place in this layer
  where the two sides are allowed to differ by construction.
- **`glsl_normalize` is not `Vec3::normalize`.** `axiom-math`'s is checked and
  returns a `MathError` on the zero vector; GLSL's has no error path and yields
  NaN. The layer ports GLSL's. The `1e-4` guard in the blend can still leave a
  zero-length normal (see below), and the source's own behaviour there is NaN —
  ported rather than papered over.
- **The `1e-4` guard is reachable.** With a height so large that `max - 0.18`
  rounds back to `max`, both weights collapse to exactly zero and the guard is
  what keeps the albedo and orm lanes finite (they become zero). The normal lane
  then normalises a zero vector and is NaN. Not physical for a unorm texture,
  but the function is total and the behaviour is pinned.

## How correctness is proven

Two suites in the one file.

**CPU-only** (`mod tests`, always compiled, part of the coverage gate): 15 tests
over the pure arithmetic — the warp constants against values computed from the
GLSL text in the source's grouping, the linearity and near-unit scale of the
warp, the taller sample winning regardless of which slot it sits in, equal
heights weighing equally, only the normal lane being renormalised, the `t = 0`
non-identity, the gate's truth table, the `1e-4` collapse, the mask band, the
mask coordinate, the normal decode, the detail fold, `glsl_normalize`, and the
WGSL text's constants and calling convention.

**CPU↔GPU parity** (`mod parity`, `--features offscreen`, native only): the
shape `surface_program/parity.rs` establishes. A real adapter is acquired and
asserted non-`Noop` — it fails loudly rather than skipping — the shader's
validation scope is checked, and each entry point renders one fragment per
sample into a `24 x 1` `Rgba32Float` target which is read back and compared.

### The texture fixture

Four `64x64` `Rgba8Unorm` textures filled procedurally (`texel()`), plus one
`128x128` two-level texture for the gradient proof. Sampler:
`Nearest`/`Nearest`/`Nearest`, `Repeat` in both axes.

Nearest and unorm on purpose: `byte / 255.0` is correctly rounded on either
side and no filter weights enter the comparison — some hardware evaluates
bilinear weights at reduced sub-texel precision, which would show up as a
tolerance rather than as the exactness it should be. The consequence is that the
three fetch entry points compare **bit-exactly** (measured delta `0`), which is
a much stronger statement about `uv2` than a tolerance would be: the warped uv
lands on the *same texel* the CPU names, for all 24 samples.

Two fixture guards, because a nearest comparison is only honest away from a
texel boundary:

- `clear_uv` / `clear_object_pos` search a deterministic candidate sequence for
  inputs whose warped uv sits at least 5% inside its texel, and `expect` loudly
  if none is found.
- `the_second_sample_lands_on_many_distinct_texels` asserts the 24 warped uvs
  hit at least 22 distinct texels, so the fetch parity cannot be vacuously
  comparing one texel 24 times.

### The gradients get their own proof

A single-mip texture makes the derivatives numerically inert, so they are
pinned twice, separately:

- their **arithmetic** is compared lane by lane against `detile_warp`
  (`fs_warp`, `fs_warp_ddy`), which is what fixes the constants;
- `the_warped_gradients_select_the_mip_level` proves they are actually *handed
  to* `textureSampleGrad`: the two-level texture is red at level 0 and green at
  level 1, and a `1e-4` footprint reads red while a `3.0` footprint reads green,
  through the same `axiom_detile_warp` the uv path uses.

### Measured tolerance

`TOLERANCE = 1.0e-6` absolute. Derived from this measurement on the local
adapter (`the_measured_worst_delta_justifies_the_tolerance` prints the table and
asserts the budget sits between 2x and 10x the worst delta — a budget more than
10x looser than the hardware needs is itself a failure):

| entry point | worst abs delta |
|---|---|
| `fs_warp` | 2.3841858e-7 |
| `fs_warp_ddy` | 2.3283064e-10 |
| `fs_sample_albedo` | 0 |
| `fs_sample_orm` | 0 |
| `fs_sample_normal` | 0 |
| `fs_mask` | 1.1920929e-7 |
| `fs_fold` | 1.1920929e-7 |
| `fs_blend_albedo` | 5.9604645e-8 |
| `fs_blend_orm` | 5.9604645e-8 |
| `fs_blend_normal` | 1.1920929e-7 |

The worst, `2.38e-7` at `fs_warp`, is **one ulp** at that lane's magnitude
(`uv2` reaches ≈2.5): a single-rounding `fma` contraction of
`v.x * 0.803 - v.y * 0.596`, which the hardware is free to do and the emitter
cannot prevent. `1e-6` is 4.2x that.

Every compared lane is deliberately held under `4.0` in magnitude — the sample
generator bounds world positions to ±4 m and `macro_scale` to `0.02..0.10` — so
that one absolute budget means the same thing on every lane. An earlier fixture
with ±10 m positions pushed `fs_mask`'s uv lanes to ≈8 and its delta to
`4.77e-7`, purely because an ulp is bigger up there; the fix was the fixture's
magnitude, not the budget.

## Coverage

`material_shader/detile.rs`: **381/381 regions, 23/23 functions, 195/195 lines,
0 branches** (the non-test Rust is branchless, and the four `&&` short-circuits
that were in test assertions were replaced with range `contains` so the branch
column reads clean too).

## Status and what the orchestrator still owns

- All 19 tests green (14 CPU, 5 GPU) under
  `cargo test -p axiom-gpu-backend --lib --features offscreen material_shader::detile`.
- The WGSL parses and validates under naga 25 with all capabilities.
- **Nothing consumes `DETILE_WGSL` yet**, so a lib-only build reports `dead_code`
  for it and for every CPU reference function. Every sibling layer is in the
  same state; it clears when the orchestrator splices the layers into
  `axiom_surface`.
- **From sibling layers, at the composition site**, this layer needs: `uv`,
  `ddx`, `ddy` and the base `alb`/`orm`/`nT` from `uv_mode`/`pom`; `dn`,
  `owDetailP.y` and `detFade` from `detail`; `owMacroP.x` and the macro texture
  from `macro_variation`; and `owRoughP.z` from `params`. All are ordinary
  arguments — nothing was reached into.
