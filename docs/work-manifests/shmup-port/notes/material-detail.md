# The micro detail layer — port notes

Layer 4d of the runtime material shader port. Source:
`C:/Claude-of-Duty/src/materials/shader.js` — the
`// ---- micro detail normal, faded by distance ----` section of `MAIN_FRAGMENT`
(lines 372-393), its triplanar twin (lines 310-321), and the `detail` /
`detailWorld` entries of `DEFAULT_PARAMS` together with the `detailTiles`
derivation in `extendMaterial`.

Everything landed in one file: `modules/axiom-gpu-backend/src/material_shader/detail.rs`.

## What the layer does

One shared, tiny, high-frequency detail set — a normal map and a height/albedo
map — tiled far denser than the base material and layered on top, so a wall
still has a tooth at half a metre. It fades out with distance, because a
sub-millimetre pattern at thirty metres is sub-pixel and turns into shimmer.

Six outputs at one shading point: the blended tangent normal, the modulated
albedo, the modulated roughness, `owMicro` (the raw -1..1 micro height), the
fade, and `owHeightS` (the surface height the cavity/wear masks read
downstream).

## WGSL entry points

All free functions; textures and sampler are parameters, so nothing here names
a binding index.

| function | signature |
|---|---|
| `axiom_detail_fade` | `(fade_metres: f32, dist: f32) -> f32` |
| `axiom_detail_uv` | `(uv: vec2<f32>, tiles: f32) -> vec2<f32>` |
| `axiom_detail_blend_normal` | `(n_tangent: vec3<f32>, dn: vec3<f32>, normal_strength: f32, fade: f32) -> vec3<f32>` |
| `axiom_detail_blend_normal_projected` | `(n_world, d_world, face_normal: vec3<f32>, normal_strength: f32, fade: f32) -> vec3<f32>` |
| `axiom_detail_micro` | `(detail_texel: vec4<f32>) -> f32` |
| `axiom_detail_albedo` | `(albedo: vec3<f32>, detail_texel: vec4<f32>, micro: f32, albedo_strength: f32, fade: f32) -> vec3<f32>` |
| `axiom_detail_roughness` | `(roughness: f32, micro: f32, albedo_strength: f32, fade: f32) -> f32` |
| `axiom_detail_height` | `(albedo_alpha: f32, micro: f32, fade: f32) -> f32` |
| `axiom_detail` | `(detail_normal_tex, detail_tex: texture_2d<f32>, detail_sampler: sampler, uv, ddx, ddy: vec2<f32>, dist: f32, detail_p: vec4<f32>, n_tangent: vec3<f32>, albedo: vec4<f32>, roughness: f32) -> AxiomDetailOut` |

`detail_p` is the source's `owDetailP`: `.x` tiles, `.y` normal strength, `.z`
albedo strength, `.w` fade metres.

**The layer is exposed both as parts and as one composition on purpose.** The
source *interleaves*: under `OW_DETILE` the de-tiling height blend
(`owHeightBlend`) runs **between** the normal blend (line 376) and the albedo
modulation (line 387), and de-tiling's second normal `n2` is blended with the
*same* `dn`, strength and fade at line 378. So `axiom_detail` is the un-de-tiled
composition, and an orchestrator that also runs de-tiling calls the parts in the
source's order with the blend spliced in.

## The normal blend is UDN

The trap named in the brief, answered from the GLSL text:

```glsl
nT = normalize( vec3( nT.xy + dn.xy * owDetailP.y * detFade, nT.z ) );
```

The two tangent `xy` are **summed** and the **base** `z` is kept unchanged, then
the whole thing is renormalised. That is **UDN**, not:

- **whiteout**, which would be `nT.z * dn.z` in the third lane;
- the **partial-derivative** blend, which would cross-multiply
  (`nT.xy * dn.z + dn.xy * nT.z`);
- a **lerp** of the two normals.

`the_normal_blend_is_udn_and_not_whiteout_or_a_lerp` computes all four on the
same inputs and asserts the other three differ, so a future "simplification"
into one of them fails rather than merely looking slightly wrong at grazing
angles.

The **triplanar** arm (line 315) is a different blend and is ported separately
as `axiom_detail_blend_normal_projected`: there the detail normal already sits
in world space, so its component along the face normal is projected out and the
remainder added —
`normalize( nP + ( dW - fd.N * dot( dW, fd.N ) ) * owDetailP.y * detFade )`.
Everything *else* in the triplanar arm (lines 316-318, 321) is character-for-
character the same as the tangent path, so it reuses the same parts.

## The two boundaries

`detFade = 1.0 - smoothstep( owDetailP.w * 0.45, owDetailP.w, owDist )`.

- **Far end.** At `owDist == owDetailP.w` the smoothstep argument is exactly
  `1.0`, so the fade is an exact `0.0` — not a small number. Checked, not
  assumed, and checked *past* the edge too.
- **Near end.** At and below `0.45 * owDetailP.w` the fade is an exact `1.0`, so
  the layer runs at its full authored strength with no attenuation creeping in.

**Zero contribution is bit-identical.** With the fade at zero, `x + y * 0.0`
returns `x` exactly and `x * 1.0` returns `x` exactly, so albedo, roughness and
height come back as the values that went in, for *any* detail texels. Pinned on
the CPU (`at_the_far_end_the_layer_is_bit_identical_to_the_undetailed_path`, over
three different texels) **and on the rendered pixels**
(`the_far_end_contributes_exactly_nothing_on_the_gpu`), because "exactly zero" is
a claim about the hardware's arithmetic too.

One honest caveat: the **normal** is not literally the input, because the source
renormalises on that line unconditionally — at zero fade the blend collapses to
`normalize(nT)`. There is no un-normalised alternative in the source; the detail
line has no `#ifdef` around it, so `normalize(nT)` *is* what the undetailed path
hands the lighting. The test asserts exactly that equality rather than pretending
the identity is stronger than it is.

**`owMicro` itself is not faded.** The source keeps the raw -1..1 height and
multiplies by `owDetFade` at each *use* site — including once more downstream at
line 429 — so the value survives the far end even though every use of it here
vanishes. Pinned, so a tidy-up cannot fold the fade into the value.

## `detail[1]` and `detail[2]` stay separate

`.y` scales the normal only. `.z` scales the albedo speckle **and** the cavity
darkening, which are the same physical signal (a trough reads dark because it is
an occluded pocket; modulating only albedo gives a washed pattern).
`the_normal_and_albedo_strengths_are_independent` triples each in turn and
asserts the other two outputs do not move.

## The `detailWorld` derivation

The measured bug the source documents at length, ported as `detail_tiles`:

```js
const dw = p.detailWorld ?? DEFAULT_PARAMS.detailWorld;   // 0.26
const detailTiles =
  p.uvMode === 'mesh' || !(dw > 0) || p.scale < 0.3
    ? p.detail[0]
    : Math.max(1.2, p.scale / dw);
```

`detail[0]` is authored as tiles **per base tile**, which silently ties the micro
layer's world scale to the macro layer's. A prop at `scale` 0.55 m with
`detail[0] = 10` mapped the 0.25 m bake into 55 mm — a 1.6 mm grain became
0.35 mm, under one pixel at 0.5 m, and the whole layer filtered away. The proof
it was dead: *cranking `detail[2]` from 0.42 to 2.5 on the market stall changed
the frame by nothing at all*.

Three transcription points that are easy to lose:

1. **`!(dw > 0)`, not `dw <= 0`.** The negation is outside the comparison so a
   NaN also takes the authored branch. Transcribed as written, and tested with
   `f32::NAN`.
2. **`Math.max(1.2, x)` propagates NaN**, where Rust's `f32::max` swallows it.
   Written as the select `[derived, 1.2][usize::from(1.2 > derived)]`, which is
   both branchless and exactly JavaScript's semantics — including for a NaN and
   for `-0`.
3. **`p.scale / dw` is a division** and stays one; it is never turned into a
   reciprocal multiply.

The property the derivation exists to hold is tested directly: a 0.55 m prop and
a 2 m wall come out with the *same* metres-per-detail-tile.

Two carve-outs survive and are tested: mesh UV (where `scale` is a repeat count,
not metres) and `scale < 0.3` (a viewmodel part is mapped at 0.02-0.12 m and
wants detail an order of magnitude finer than a wall's; forcing 0.26 m on it
would put a 2 mm aggregate tooth on a bolt carrier). Note `0.3 / 0.26` is 1.154,
*under* the 1.2 floor — so a surface mapped at exactly 0.3 m derives and then
floors to 1.2.

## Parity: the texture, and what it proves

Both detail textures are **8x8 single-mip `Rgba8Unorm`**, filled by a fixed
integer recurrence over the texel coordinates (`parity_texel`, distinct
constants per texture, spread across the whole byte range). The sampler is
`Repeat` with **nearest** min/mag/mip.

Why nearest, deliberately:

- an 8-bit unorm converts to `n / 255` exactly, so the fetched value is
  *identical* on both sides — nothing about texture *filtering* precision (only
  8-bit fixed-point on plenty of hardware) leaks into a tolerance that is meant
  to measure this layer's arithmetic;
- the CPU can name the same texel, so the reference and the shader see the same
  inputs rather than merely similar ones.

The run asserts each sample sits **more than 0.05 texels from a texel boundary**
(the measured minimum is 0.23), so a sample cannot resolve to a different texel
on the GPU than on the CPU for reasons unrelated to this layer.

**The limit, stated rather than papered over:** with one mip level the
derivative arguments cannot change the answer, so parity pins the *values* but
not that `owDetailP.x` reaches `textureSampleGrad`'s gradients. That scaling is
pinned separately, as text
(`the_wgsl_defines_the_layers_entry_points_and_scales_the_derivatives` asserts
the `det_ddx`/`det_ddy` lines and both `textureSampleGrad` call shapes) and as a
CPU function (`detail_uv`, which the parity harness itself uses to locate its
texels, so it is genuinely exercised and not a shim). A two-mip parity texture
would prove it functionally, at the cost of pinning LOD selection, which the
spec permits an implementation to approximate — that trade was judged not worth
it.

The parity run also asserts, **on the GPU's own output**, that it straddles
every boundary it claims to: a sample with fade exactly 0, one with fade exactly
1, one strictly between; `owMicro` on both sides of zero (so the trough-only
cavity darkening is exercised in both states); and `owHeightS` saturating at
both clamps.

## Tolerance

| | |
|---|---|
| measured worst absolute lane delta | `1.1920929e-7` |
| that is | exactly `2^-23` — **one f32 ULP at unit magnitude** |
| recorded ceiling (`MEASURED_WORST`) | `1.2e-7`, re-measured and asserted every run |
| tolerance (`TOLERANCE`) | `1.0e-6` — 8.4x the measurement |

The two sides differ by at most a single last-bit rounding, which is what a
faithful transcription of a short arithmetic chain should look like once the
texture fetch is made exact. The exact tier's `1e-4` next door would be eight
hundred times looser than this hardware needs, which the brief calls a failure
in its own right; the test asserts `TOLERANCE <= 10 * MEASURED_WORST` so it
cannot drift into that.

Both sides compute in `f32` throughout — the CPU reference uses `[f32; N]` and
never widens to `f64`, so a parity delta is the hardware's rounding and never a
storage-width mismatch this file introduced.

## Divergences from the source

None in the arithmetic. Every grouping is the source's:
`(dn.xy * y) * fade`, `micro * 0.95 + (r - 0.5) * 1.25` unfactored,
`smoothstep`'s `(x - e0) / (e1 - e0)` left as a division, `normalize` as a
per-component **division** by the length rather than a multiply by a
precomputed reciprocal.

Two deliberate CPU-reference choices, both to match GLSL/WGSL rather than Rust:

- `clamp` is written as `min(max(e, low), high)` — the expansion WGSL specifies
  — not `f32::clamp`, whose NaN handling differs;
- `smoothstep` is written out rather than reached for, so its clamp and its
  cubic are the spec's.

## Not ported here (belongs to a sibling)

Everything inside `#ifdef OW_DETILE` at lines 377-381 is **de-tiling's** (layer
4c), not this layer's — the `n2` blend, the `dtm` mask
(`texture2D( owMacroTex, ( owP.xz + owP.y * 0.7 ) * owMacroP.x * 5.0 + 0.21 ).g`
remapped by `owRoughP.z`), and the `owHeightBlend` call. This layer supplies the
pieces de-tiling needs to interleave with — see the composition note above — but
does not own them.

## Assumptions about siblings

Reported to the orchestrator; nothing in this file depends on them compiling.

- `frames.rs` owns the projection frame, so it produces `fd.T/B/N` and the
  world-space `dW = fd.T * dn.x + fd.B * dn.y + fd.N * dn.z` that
  `axiom_detail_blend_normal_projected` takes as `d_world`. The cut is at the
  world-space detail normal, not at the frame.
- `uv_mode.rs` owns `f.uv` / `mesh` vs `planar` vs `triplanar`, and therefore
  owns *which* uv reaches `axiom_detail`. `detail_tiles`'s `uv_mode_is_mesh`
  flag is a `bool` parameter for exactly that reason — this file does not name a
  uv-mode enum.
- `params.rs` owns the `SurfaceParams` packing; `detail_tiles` computes the
  value that goes into `owDetailP.x` but does not choose its slot.
- `pom.rs`/`detile.rs` own the `uv`, `ddx`, `ddy` handed in.
