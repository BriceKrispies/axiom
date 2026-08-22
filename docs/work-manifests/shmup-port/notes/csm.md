# Cascaded shadow maps — `render/csm.js` (582 lines)

Slice notes for the render fan-out. What landed, what it is transcribed from,
what it needs from the orchestrator, and what it does **not** do.

Code, all new:

| file | lines | what |
|---|---|---|
| `modules/axiom-gpu-backend/src/cascade.rs` | 660 | the fit: splits, sphere, snap, uniform lanes, atlas |
| `modules/axiom-gpu-backend/src/cascade/shading.rs` | 585 | the fragment stage: selection, bias, PCSS, PCF, blends |
| `modules/axiom-gpu-backend/src/cascade/adapter_proof.rs` | 931 | the real-adapter proof **and the WGSL text**, `#[cfg(all(test, feature = "offscreen"))]` |

Plus one line in `lib.rs` (`mod cascade;`). Always compiled, branchless outside
tests, **100% regions / lines / functions** and **zero counted branches** under
`cargo llvm-cov --branch -p axiom-gpu-backend`. **Nothing else in the repo is
touched** —
`scene_wgsl.rs`, `scene_renderer.rs` and `shadow_view.rs` are untouched, and
`cascade::tests::nothing_in_the_shadow_path_compiles_this_yet` is the source
scan that keeps them that way until the wiring is deliberate.

## 1. The split scheme, as the source writes it

`update()`, `lambda = 0.86`, `maxDistance = 140`:

```js
s[0] = n;                                   // camera.near
for (i = 1; i < N; i++) {
  p        = i / N;
  logSplit = n * Math.pow(f / n, p);
  uniSplit = n + (f - n) * p;
  s[i]     = lambda * logSplit + (1 - lambda) * uniSplit;
}
s[N] = f;                                   // min(camera.far, 140)
```

`lambda` weights the **logarithmic** term (the opposite of the more common
convention), and 0.86 is heavily logarithmic. At `near = 0.5`, `far = 140`,
`N = 4` the boundaries are

| | s0 | s1 | s2 | s3 | s4 |
|---|---|---|---|---|---|
| practical, λ=0.86 | 0.50 | **6.71** | **17.03** | **44.17** | 140.0 |
| uniform (λ=0) | 0.50 | 35.4 | 70.3 | 105.1 | 140.0 |

Three of the four cascades sit inside the first 45 m. That is the whole reason
four cascades beats one at street scale.

The two ends are written **directly**, not through the blend: at `p = 0` and
`p = 1` the blend is algebraically `n` and `f` but not bit-for-bit, and the
shader compares view depth against these exact values.

### Texel snapping, exactly

Two complementary stabilisers, and both are required:

1. **The fit is a bounding sphere** of each sub-frustum, so the ortho *extent* is
   rotation-invariant — turning the camera cannot resize the grid. The radius is
   then quantised, `r = ceil(r * 16) / 16`, against float drift in the fov terms.
2. **The projection is snapped** so the grid's *phase* is nailed to world space:

```js
_mat.multiplyMatrices(cam.projectionMatrix, cam.matrixWorldInverse);
_origin.set(0, 0, 0, 1).applyMatrix4(_mat);      // the WORLD ORIGIN in light clip
const half = this.mapSize * 0.5;
const sx = _origin.x * half,  sy = _origin.y * half;
const dx = (Math.round(sx) - sx) / half;
const dy = (Math.round(sy) - sy) / half;
cam.projectionMatrix.elements[12] += dx;          // NDC translation x
cam.projectionMatrix.elements[13] += dy;          // NDC translation y
```

**Trap, ported:** JS `Math.round` rounds a half toward `+∞`; `f32::round` rounds
a half *away from zero*. The reference transcribes it as `floor(x + 0.5)` and
makes that one decision in `f64` — it is the only step in the fit where a
last-bit difference selects a different integer rather than perturbing a smooth
quantity. `cascade::tests::the_snap_lands_the_world_origin_on_a_whole_texel`
pins both the property and the rounding rule.

## 2. The per-cascade ortho fit, selection and blend

Sub-frustum bounding sphere, `k2 = tan(fovy/2)² + tan(fovx/2)²`, both arms
transcribed verbatim:

```js
if (k2 * k2 * (cf + cn) >= cf - cn) { cz = -cf; r = cf * Math.sqrt(k2); }
else { cz = -0.5 * (cf + cn) * (1 + k2);
       r  = 0.5 * Math.sqrt((cf-cn)² + 2*(cf²+cn²)*k2 + (cf+cn)²*k2²); }
```

At a street fov (`k2 = 1.39` for 60°/16:9) the **far-cap arm wins for every
cascade**; the general arm needs a narrow fov to reach at all. Both are covered.

Camera: `eye = centre + sunDir * (r + backDistance)`, `backDistance = 140`,
`up = |sunDir.y| > 0.98 ? +Z : +Y`, ortho `[-r, r]²`, **`near = 0.0`**,
`far = 2r + backDistance`. `sunDir` points **from the scene toward the sun** —
the negation of the travel direction `axiom-render-pipeline`'s existing
single-cascade fit takes.

Uniform lanes per cascade: `split = cf`, `splitNear = cn`,
`texel = 2r / mapSize`, `range = far - near`. Lanes past `count` get the
source's own sentinels `1e9 / 1e9 / 0.01 / 1.0`, so the shader's
`vd < split[i]` scan can never select one.

Fragment stage: first cascade whose far split exceeds the view depth; a
**cross-fade over the last 12%** of that cascade into the next
(`t = smoothstep(mix(a, b, 0.88), b, vd)`, skipped below `t = 0.001` — the gate
is part of the value, not an optimisation); then a **fade-out over the last 12%
of the whole range** (`smoothstep(last, last * 0.88, vd)`, called with
`e0 > e1` on purpose). Filtering is normal-offset + slope-scaled bias, an
optional PCSS blocker search, and a Vogel-disc PCF with a per-pixel
interleaved-gradient phase.

Measured for a 60°/16:9 camera at `4 x 2048`:

| cascade | split | radius | world texel | cull margin (32 texels) |
|---|---|---|---|---|
| 0 | 6.71 m | 7.94 m | 7.8 mm | 0.25 m |
| 1 | 17.03 m | 20.06 m | 19.6 mm | 0.63 m |
| 2 | 44.17 m | 52.06 m | 50.8 mm | 1.63 m |
| 3 | 140.0 m | 164.94 m | 161 mm | 5.16 m |

Today's single cascade over a 60 m range is one 17 cm texel everywhere. Four
cascades buy a **21x finer** texel in the first 7 m at the same atlas budget.

## 3. Atlas layout

One `texture_2d_array<f32>`, `R32Float` (linear light-space depth — an ortho
projection makes clip depth linear), `MAX_CASCADES` layers, `2048²`, nearest
filter, clamp-to-edge, no mips, plus a shared depth buffer for the caster pass.
`4 x 2048 x R32F = 67 MB`; the source clamps `mapSize` to 2048 precisely
because `4 x 4096` is 268 MB ("a quarter of a gigabyte for shadows nobody can
see"). `atlas_byte_size` is that arithmetic, tested.

**Colour, not a depth attachment, and that is load-bearing**: PCSS needs to read
the stored depth *value* for the blocker mean, which `textureSampleCompare`
cannot give you. R32Float is `unfilterable-float` in core WebGPU, which is
exactly the source's `NearestFilter` configuration — bind it as
`TextureSampleType::Float { filterable: false }` with a `NonFiltering` sampler.

## 4. What is proven, and at what tolerance

`cascade.rs` has 16 native tests covering every region of the fit, the snap, the
split scheme, both sphere arms, the sentinels, the selection, the cross-fade,
PCSS contact-hardening and every early-out.

`cascade/adapter_proof.rs` renders the real `4 x 2048` atlas on a real adapter
from the reference's own matrices, with two horizontal casters (a near roof at
10 m and a far one at 106 m), and makes two claims:

1. **The map holds the caster where the reference projects it.** Each shadowed
   probe's predicted `(layer, u, v)` holds a depth in front of the receiver's;
   each open-road probe's holds the clear value. Real rasterisation, the
   reference's projection.
2. **The WGSL means what the reference means.** `ow_sun_shadow` runs on the
   adapter over the same atlas and is compared probe for probe against
   `sun_shadow`.

**Measured: bit-exact.** The worst CPU↔GPU difference across the eight probes is
`0.0` on this adapter (Vulkan/native), and the guard is asserted at one f32 ULP
rather than at the 1e-5 per-probe tolerance so a future divergence cannot hide in
slack nobody needs. The tolerance is not arbitrary either, because the quantity
is **discrete**: the term is a sum of 20 zero-or-one steps blended by written-out
`mix`/`smoothstep`, so two implementations that agree on which texels the taps
land in differ by *zero*, and one that disagrees about a single tap differs by
`1/20 = 0.05` — four thousand times the tolerance. 1e-5 separates "identical"
from "a tap moved" with three orders of margin on both sides.

The eight probes are not all-1.0 agreement: two read `< 0.02` (fully shadowed
under a roof, in cascades 1 and 3), two read exactly `1.0` (open road), two sit
strictly between (on the shadow's edge, the PCF disc straddling it), one is at a
view depth inside the cascade-1→2 cross-fade so both cascades contribute, and one
repeats a shadowed point with a tilted normal to drive the slope-scaled bias and
the grazing normal offset.

## 5. One cascade renders byte-identical to today — by construction

Not by assertion. The shipped shadow path is `axiom-render-pipeline`'s
`shadow_view.rs` (one bounding-sphere fit over a 60 m range) plus
`scene_wgsl.rs`'s 5x5 `textureSampleCompare` PCF, and **neither is reachable
from this module**. `apps/burnt-rubber`, `apps/end-zone` and the demos see no
change at all, because no byte of their path changed.

That property has to survive the wiring, and the honest way to keep it is for the
cascade path to be a **separate shader suffix behind a capability bit**, not a
generalisation of the existing one. The two filters are not reducible to each
other: today's is a hardware-comparison 5x5 at a fixed `0.0015` bias; the
source's is a manual-compare Vogel disc at a slope-scaled, texel-derived bias.
Trying to make `count == 1` of the new path reproduce the old path's pixels
would mean porting the source's filter *wrong*. A frame that asks for one
cascade must therefore keep compiling and running today's `shadow_factor`
unchanged.

## 6. What the orchestrator has to do next (I did none of it)

### 6a. `scene_wgsl.rs` — the change I need (I did not make it)

Additive only. Keep `shadow_factor` and the `ShadowU` uniform exactly as they
are. Add, gated on a new `CAP_CSM` capability bit so the default pipeline's text
is byte-identical:

- **Prefix**, after the existing group-2 bindings:
  ```wgsl
  struct CsmU {
      matrices: array<mat4x4<f32>, 4>,
      split: vec4<f32>, split_near: vec4<f32>,
      texel: vec4<f32>, range: vec4<f32>,
      map_size: vec4<f32>,        // x = edge, y = 1/edge
      sun_world: vec4<f32>,       // xyz, from the scene TOWARD the sun
      params: vec4<f32>,          // strength, tan(sun radius), max PCF texels, phase
  };
  @group(2) @binding(3) var<uniform> csm: CsmU;
  @group(2) @binding(4) var csm_maps: texture_2d_array<f32>;
  @group(2) @binding(5) var csm_samp: sampler;          // NonFiltering
  ```
  368 bytes of uniform, one array texture, one sampler. Group 2 is the shadow
  group and already exists; nothing else moves.
- **Functions**: `ow_mix`, `ow_smoothstep`, `ow_ig_noise`, `ow_vogel`,
  `ow_csm_tap`, `ow_csm_cascade`, `ow_sun_shadow` — **verbatim from
  `cascade/adapter_proof.rs`'s `PROBE_WGSL`**, which is the text the adapter
  proof compiles and pins. Do not retype them; copy them.
- **Fragment**: the one line that selects between the two, e.g.
  `let shade = select(shadow_factor(in.world_pos), ow_sun_shadow(view_depth, in.world_pos, n, in.position.xy), (caps & CAP_CSM) != 0u);`
  — but note `select` evaluates both arms, so the CSM arm must be spliced as a
  *variant suffix* if the byte-identity of the non-CSM pipeline is to hold at the
  shader-text level rather than only at the pixel level. My recommendation is the
  variant suffix.
- `view_depth` is `-view_pos.z`; the fragment stage does not carry it today, so
  either add an interstage lane or reconstruct it from `camera_view_proj`.

Three WGSL deltas from the GLSL, all stated in the file header:
`texture(...)` → `textureSampleLevel(..., 0.0)` (explicit LOD is what makes the
source's early returns legal in WGSL); `sc.xyz/sc.w*0.5+0.5` → `ndc.z` with a
flipped `v` (wgpu clip + framebuffer convention, the same two the existing
`shadow_factor` applies); `smoothstep` written out (WGSL leaves `low >= high`
indeterminate and the fade-out calls it that way).

### 6b. The frame contract has to widen — this is the real blocker

`SceneRenderer::record` takes `light_view_proj: [f32; 16]`; the cascades need
four matrices plus four `vec4` lanes. That threads back through
`axiom-gpu-backend::gpu_backend_api`, `offscreen.rs`, `live_gpu_binding.rs`,
`axiom::FrameOutcome`, and `axiom-render-pipeline`'s `shadow_view.rs` (which is
where the *fit* would move to — it already owns the camera and the sun, which
`cascade::fit` needs and the backend does not have). Recovering the fov/aspect
out of `camera_view_proj` inside the backend would work and is exactly the kind
of shortcut this repo forbids.

That is a multi-slice contract change and I did not make it. The maths is done
and pinned; the plumbing is a decision for whoever owns the frame contract.

### 6c. The caster pass needs four draws

`_cullCascade` is ported as `CascadeSet::cull_margin` (32 texels, the source's
measured figure — at 2 texels the source measured the pass as *not*
pixel-neutral). The existing `shadow_cull::casts_into` already does the
per-frustum test; it just needs running once per cascade, and a cascade nothing
reaches must still be **cleared**, not skipped, or it shadows with last frame's
blockers.

## 7. G10 — unchanged, and now cheaper to fix in one place

G10 is "the shadow pass runs no vertex program, so displaced geometry casts an
undisplaced shadow." This work **does not change it, does not inherit it
silently, and does not make it worse**:

- The caster pass here is the source's own `depthMaterial`, which likewise runs
  a plain transform. Adding cascades multiplies the number of caster *draws* by
  four but leaves each draw's vertex stage exactly what it is today.
- It does not get worse in a hidden way either: four cascades means a displaced
  caster's undisplaced shadow is now wrong at *four* densities instead of one,
  but it is the same wrongness — no new class of artefact.
- It gets structurally *cheaper* to fix. The fix for G10 is to run
  `axiom_displace` in the shadow vertex stage (`SHADOW_WGSL` in
  `scene_renderer.rs`), which is one shader and one pipeline. With cascades that
  one fix covers all four layers, because they share the caster pipeline and
  differ only in the bound view-projection.

Worth stating plainly: **the source has the same gap in reverse.** `csm.js`'s
depth material includes `batching`/`skinning`/`morphtarget` chunks, so the
original's *skinned* and *morphed* casters do displace correctly. Axiom's
skinned pass is likewise a separate vertex program the shadow pass does not run.
So G10 is really "the shadow pass runs *the wrong* vertex program", and the
reference's answer is to run the same one the main pass runs. That is the shape
the fix should take.

## 8. What I did not port

- `_cullCascade`'s object-hiding mechanics and `_restoreCulled` — three-specific
  scene-graph mutation; Axiom's `shadow_cull` already expresses the same test
  functionally, and only the **margin** was worth carrying over.
- `snapshotFit` / `restoreFit` — they exist because
  `RenderSystem.prewarmMaterials()` calls `update()` out of frame and must not
  leave a fit behind (the source measured a single stray `update()` moving 1.3 M
  pixels by up to 26/255). Axiom has no prewarm pass that fits the cascades, so
  there is nothing to snapshot. If a prewarm ever lands, this is the trap.
- `owSunShadow`'s `dot(lightDirView, owSunDirView) < 0.999` opener — it picks the
  sun out of three's directional-light *loop*. Axiom has one shadow-casting
  directional light, so there is no loop to pick out of. Recorded so the absence
  is a decision.
- `setStrength` / `setJitter` / `dispose` — one-line setters over
  `CascadeParams`, which is a plain value type here.
