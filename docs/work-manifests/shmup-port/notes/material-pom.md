# `material_shader/pom` — parallax occlusion mapping

Ported from `C:/dev/Claude-of-Duty/src/materials/shader.js`: `owPOM` in
`PARS_FRAGMENT` (source line 214), its call site in `MAIN_FRAGMENT` (`Vp`, `vt`,
`pFade`), and the `parallax` / `parallaxFade` / `parallaxLayers` knobs of
`DEFAULT_PARAMS`, which the source packs as
`owParallaxP = vec4(parallax, parallaxFade[0], parallaxFade[1], parallaxLayers)`
with defaults `(0, 6, 14, 22)`.

Everything lives in `modules/axiom-gpu-backend/src/material_shader/pom.rs`: the
WGSL as `pub(crate) const POM_WGSL`, the CPU reference, and the parity tests.

## The WGSL

```wgsl
fn axiom_pom_fade(view_distance: f32, fade_near: f32, fade_far: f32) -> f32

fn axiom_pom_view_tangent(
    view_dir: vec3<f32>, tangent: vec3<f32>, bitangent: vec3<f32>, normal: vec3<f32>
) -> vec3<f32>

fn axiom_pom(
    uv: vec2<f32>, vt: vec3<f32>, ddx: vec2<f32>, ddy: vec2<f32>,
    depth: f32, fade: f32, layers: f32,
    height_map: texture_2d<f32>, height_sampler: sampler,
) -> vec2<f32>
```

`owPOM` reads the `owParallaxP.w` uniform for its layer count; here that is the
explicit `layers` argument, per the brief's calling convention. The height field
is the **alpha of the albedo map** (`OW_TEX( map, … ).a`), which the orchestrator
binds; `axiom_pom` names it as a parameter so it assumes no binding index.

`depth` is `owParallaxP.x`, and `fade` is what `axiom_pom_fade` produces from
`length(vViewPosition)` and `owParallaxP.yz`. `vt` comes from
`axiom_pom_view_tangent` given the tangent frame the `frames` / `uv_mode` layer
owns. Note the source gates the whole effect at build time with
`if (p.parallax > 0 && p.uvMode !== 'triplanar') defines.OW_PARALLAX` — the
triplanar exclusion is the uv-mode layer's business, not this one's.

## What I found in the source

### 1. The linear refine is a step, not a lerp — a defect, ported verbatim

```glsl
vec2  prev   = c + dUv;
float after  = d - cur;
float before = ( 1.0 - OW_TEX( map, prev, ddx, ddy ).a ) - cur + layer;
float w      = clamp( after / max( after - before, 1e-4 ), 0.0, 1.0 );
return mix( c, prev, w );
```

At a genuine crossing the loop exits with `cur >= d`, so `after = d - cur <= 0`;
the step before it had `cur' < d'`, so `before = d' - cur' > 0`. Their difference
is therefore **always negative** — and `max( after - before, 1e-4 )` replaces it
with `+1e-4`. The weight becomes a large negative number and clamps to `0`, so
`mix( c, prev, 0 )` returns `c`. Symmetrically, when the march runs out of layers
with `after > 0`, the weight clamps to `1` and it returns `prev`.

`w` is therefore only ever `0` or `1` (barring a `0 < after < 1e-4` sliver that
no realistic input hits): **the intersection is never interpolated**. The
textbook form (LearnOpenGL and every derivative) divides by `after - before`
unguarded, or floors it *negatively* with `min(after - before, -1e-4)`, and lands
between the two samples. The source's guard has the wrong sign for the quantity
it guards.

I did **not** fix it. A POM silhouette that interpolates where the original
snapped is a different image, and the job here is that the pixels match. Whether
Axiom's own materials should keep the source's behaviour or take the textbook one
is a product decision for whoever owns the material vocabulary; the one-character
change is `max(…, 1e-4)` → `min(…, -1e-4)` in both `POM_WGSL` and `pom()`.

The visible consequence of the defect is that the displaced uv snaps to whole
layer steps, so a parallaxed surface quantises at grazing angles instead of
resolving smoothly — worst where `nl` is smallest, i.e. face-on (`nl = 8`) and
far away (the floor of `4`).

### 2. The layer clause cannot fire on its own

`cur >= d || float( i ) >= nl` reads like two independent exits, but `d = 1 - a`
is in `0..=1` for any texture, and `cur` reaches `nl * (1/nl) = 1` at `i = nl`.
So by the time `float( i ) >= nl` is true, `cur >= d` is true as well: the layer
clause is a **safety net**, not a distinct exit, and only fires alone when
accumulating `1/nl` `nl` times lands a ULP below `1.0` against a `d` of exactly
`1.0`. The exit that *is* independently reachable is the loop's own bound of 48,
whenever `nl > 48` (nothing clamps `parallaxLayers`, so an authored `100` gets
truncated to 48 steps and the effect silently loses depth). Both are covered.

### 3. Everything transcribed literally

- `( vt.xy / max( abs( vt.z ), 0.30 ) ) * depth * fade` is a real division and a
  left-to-right multiply chain — not a reciprocal-multiply, not refactored.
- `mix( owParallaxP.w, 8.0, clamp( abs( vt.z ), 0.0, 1.0 ) )` then
  `max( nl * fade, 4.0 )`, in that order: the fade scales the layer count *after*
  the grazing mix, and the floor of four applies to the product.
- `clamp(x, 0, 1)` is the spec expansion `min(max(x, 0), 1)`; the CPU reference
  uses `f32::clamp`, which agrees with it for every finite input, and every input
  here is finite (a texture sample in `0..=1`, a layer depth built from it, a
  normalised cosine).
- `smoothstep` is `t = clamp((x-e0)/(e1-e0),0,1); t*t*(3-2*t)`. Degenerate edges
  (`e0 == e1`) divide by zero on both sides, exactly as the source does.
- `normalize` is `v / length(v)`, a division per lane. `axiom_pom_view_tangent`
  normalises **twice** — the eye vector, then the projection onto the frame — and
  both are in the source; the second is what makes `vt.z` a cosine.
- Storage width: the CPU reference computes in `f32` throughout, the same width
  the GPU uses, so no `f64`-vs-`f32` allowance is folded into the tolerance.

## `textureGrad`, not `textureSample`

The source samples through `OW_TEX`, which expands to
`textureGrad( t, uv, dx, dy )` (`OW_NOGRAD` swaps in `texture2D` for platforms
without it), and its comment says why: implicit derivatives inside the march's
non-uniform control flow are undefined. `ddx`/`ddy` are computed once at the call
site (`dFdx( f.uv )` / `dFdy( f.uv )`, from the *frame's* uv, before any
displacement) and threaded in. `axiom_pom` keeps them explicit and uses
`textureSampleGrad`; the WGSL test asserts the text contains no bare
`textureSample(`.

`the_march_honours_the_gradients_it_is_given` proves they actually reach the
sampler rather than being decorative arguments: the fixture texture has two mip
levels, and a gradient wide enough to select level 1 changes every answer.

## The parity fixture, and why it is built the way it is

**The texture.** 16x16 `Rgba8Unorm`, two mip levels, sampled NEAREST with
clamp-to-edge.

- Level 0 alpha is `255 - 17 * x` — a staircase along u, **flat along v**. Full
  height (`alpha = 255`, depth `0`) at column 0, a full-depth well
  (`alpha = 0`, depth `1`) at column 15. So marching towards lower u walks out of
  the well and crosses; marching the other way runs into a wall the layer budget
  can never reach.
- Level 1 alpha is a solid `255`, so a mip switch is unmistakable.
- Flat along v means the v lane of the sweep is exercised (it moves the returned
  uv) without v ever selecting a different height — half the texel-boundary risk
  removed by construction.
- `k / 255` is the exact unorm8 decode, and the CPU reference computes the same
  `f32`, so the height values are bit-identical on both sides.

**Nearest, not linear.** Linear filtering has implementation-defined sub-texel
precision (typically 8 fractional bits), which would put the hardware's
interpolator inside the comparison. Nearest makes a lookup one exact texel — at
the cost of a cliff if the two sides ever round to *different* texels.

**So the case parameters are dyadic on purpose.** `vt.z` is `0.5` or `1.0`, so
`nl` is an exact `16` or `8` and `q = max(|vt.z|, 0.30)` is exact; `vt.x * depth`
is chosen so `dUv.x` is a whole number of texels; every start uv is a texel
centre. Every sample therefore lands mid-texel and nearest-filter rounding cannot
differ. `every_case_is_well_conditioned_enough_for_the_comparison_to_mean_something`
**asserts** that (max deviation from a texel centre `< 1e-5` texels) rather than
trusting it, and also asserts that each march's exit test is at least `0.01` from
flipping and that the uv actually moved — a parity comparison between two
unchanged uvs proves nothing.

A useful accident of the algebra: `dUv` is independent of `fade`, because the
sweep scales by `fade` and the layer by `1/fade`. That lets the `fade = 0.5` and
`fade = 0.25` cases stay dyadic.

The sixteen cases cover: one, two and three texels a layer; both ends of the
grazing mix (`nl = 16` and `nl = 8`); the `nl` floor of 4; `fade` of 1, 0.5 and
0.0625; a v-axis sweep in both directions; the hard 48-step cap (the only case
where the weight saturates *high*); an immediate exit at full height where the
loop body never runs; and both disabled forms.

## Tolerances, measured

| what | measured worst delta | budget | why |
|---|---|---|---|
| `axiom_pom` uv | **0.0** (bit-exact) | `0.0` | see below |
| `axiom_pom_view_tangent` + `axiom_pom_fade` | **1.19e-7** (`2^-23`, one ULP at 1.0) | `2.5e-7` | `normalize` is a reciprocal-square-root on hardware and a divide on the CPU; `smoothstep` is three roundings either way |

The march's budget is zero and that is a measurement, not a hope. Every quantity
it accumulates is exact: `nl` is an exact `mix` of exact values, `layer = 1/nl`,
`dUv = P * layer` is one correctly-rounded multiply, the march is repeated
subtraction, and the refine's weight saturates so `mix` returns an endpoint
untouched. There is nothing for an `fma` contraction or a reassociation to
change. The only thing that could move the answer is a **different step count**,
which is exactly what the comparison exists to catch — and it would show up as a
whole texel, not a ULP.

Measured on the development adapter via `wgpu::Instance::default()` with
`HighPerformance`; the tests print both measurements, so a looser adapter is a
visible regression rather than a silent one.

## Running it

```
CARGO_TARGET_DIR=C:/Users/Brice/AppData/Local/Temp/claude/shmup-agent-targets/pom \
  cargo test -p axiom-gpu-backend --lib --features offscreen material_shader::pom
```

Four tests need the feature (they assert a real backend rather than skipping);
ten more are pure CPU and run in the default build, which is what keeps the
coverage gate — run without `offscreen` — at 100% for this file: **356 regions,
21 functions, 192 lines, all covered; 0 branches**, the Branchless Law holding on
the Rust while the WGSL keeps the loop it is named for.

## Loose ends for the orchestrator

- `POM_WGSL` and the CPU reference are `pub(crate)` and nothing composes them
  yet, so the lib target reports `dead_code`, which `cargo clippy -- -D warnings`
  turns into a CI failure. The fix belongs in `material_shader/mod.rs` (a shared
  file this layer must not touch): reference the layer from the composed
  `axiom_surface`. Not papered over with an `#[allow]` here.
- `axiom_pom_view_tangent` needs `f.T` / `f.B` / `f.N` from the frames layer and
  the view direction (`-vViewPosition` normalised for mesh uv, `vOwViewDirP` for
  planar); `axiom_pom_fade` needs `length(vViewPosition)`. Both are call-site
  quantities this layer does not own.
- The height source is the albedo map's alpha. If the orchestrator binds the
  packed material set instead, `axiom_pom` still only needs *a* `texture_2d<f32>`
  whose `.a` is height.
