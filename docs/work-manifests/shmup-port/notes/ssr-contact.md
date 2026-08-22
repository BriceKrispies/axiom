# `render/ssr.js` + `render/contact.js` — the two screen-space marching passes

Slice: the two bounded marches that read the same G-buffer.
Source: `C:/dev/Claude-of-Duty/src/render/ssr.js` (197) and
`src/render/contact.js` (168), plus the six lines of
`src/render/materialpatch.js` that consume them.

Targets written:

| path | what |
|---|---|
| `modules/axiom-gpu-backend/src/ssr.rs` | SSR: constants, CPU reference, 3 WGSL consts |
| `modules/axiom-gpu-backend/src/ssr/parity.rs` | 3-tier CPU↔GPU parity |
| `modules/axiom-gpu-backend/src/contact.rs` | contact shadows: same shape |
| `modules/axiom-gpu-backend/src/contact/parity.rs` | 3-tier CPU↔GPU parity |

Nothing was built, checked, tested or committed — the wave brief
(`12-final-wave-brief.md`) forbids all four. **Every tolerance below is derived
from the arithmetic and is unverified.**

---

## 1. Wiring the orchestrator must apply

```
modules/axiom-gpu-backend/src/lib.rs:  mod ssr;
modules/axiom-gpu-backend/src/lib.rs:  mod contact;
```

`contact` must come **after** `ssr` for readability only — Rust does not care
about order, but `contact` imports from `ssr` and a reader should meet them in
that order. Neither needs a `Cargo.toml` change: both use only what the crate
already depends on (`wgpu`/`bytemuck`/`pollster` behind `offscreen`, and
`crate::gbuffer` / `crate::bloom_pyramid::half_storage` intra-crate).

Suggested `lib.rs` comment, matching the house style:

```rust
// Two bounded screen-space marches over the same G-buffer: screen-space
// reflections (marched against linear depth, coloured from the PREVIOUS resolved
// frame through the velocity buffer) and contact shadows (a short march toward
// the sun that puts back the last few centimetres a cascade texel cannot
// resolve). Ported from `render/ssr.js` and `render/contact.js`. The CPU
// references and the constants are pure and compiled everywhere; the WGSL and
// the parity harnesses sit behind the GPU arms. `contact` imports `ssr`'s
// `ScreenImage` and `COMMON` reference, so read `ssr` first.
mod ssr;
mod contact;
```

## 2. What must be supplied by whoever wires the frame graph

The frame-graph sibling (`render/index.js`) has to produce, per frame:

| lane | who owns it | note |
|---|---|---|
| `uProj`, `uProjInv` | camera | column-major, the **same** pair the G-buffer prepass rasterised with |
| the G-buffer's three views | `gbuffer::GBufferTargets::view(…)` | `Normal`, `Velocity`, `Depth` |
| previous **resolved** colour | TAA history, or the HDR target before it is overwritten | `render/index.js` picks `this.taa ? this.taa.previousTexture : this.hdrRt.texture` |
| `uSunDirView` | sun direction transformed by `camera.matrixWorldInverse`, **normalised** | contact only |
| `frame` | `frame % 64` — `SsrParams::at_frame` / `ContactParams::at_frame` do the modulo |
| the target size | in pixels; **SSR's is half-resolution** (`ssr::ssr_target_size`) |

Ordering, from `render/index.js` steps 5/6/7:

1. G-buffer prepass.
2. **contact** (needs only the G-buffer). Its result feeds the *sun* term.
3. **SSR** (needs the G-buffer **and last frame's resolved colour**), and it is
   skipped on the very first frame — `!this._firstFrame` — because there is no
   previous frame to reflect. That guard is the frame graph's, not this module's,
   and it must not be dropped: without it frame 0 reflects an uninitialised
   target.
4. Forward world pass, which consumes both.

Both passes run **march → blur-X into a second target → blur-Y back into the
first**, and the texture the material samples is the first target.

## 3. What the material port (`materialpatch.js`) must call

Both consumption rules live **here**, not in the material, because both are
properties of the pass:

- `ssr::ssr_resolve(radiance, reflection, roughness)` — the roughness cutoff
  (`< 0.62`), the reversed ramp `smoothstep(0.62, 0.14, roughness)`, and the fact
  that the reflection **replaces** the IBL specular via `mix` rather than adding
  to it. The `owFeat.z` feature bit and the fetch stay in the material.
- `contact::contact_shadow_for_light(enabled, dot_light_sun, sampled)` — the
  `0.999` sun-dot test. Dropping it would darken every non-sun light by the sun's
  contact shadow.

The material's fetch UV is `gl_FragCoord.xy * owScreenTexel`, a genuine
**reciprocal-multiply in the source**. Do not tidy it to a division; this pass's
own UV is a division for the opposite reason (its `uTexel` is dead).

## 4. Adaptations to WebGPU — two, both exact, both stated in code

1. **`NDC_V_SIGN = -1.0`.** WebGL's framebuffer `v` runs up and coincides with
   NDC `y`; a WebGPU texture's `v` runs down. The two UV↔NDC crossings
   (`owViewPos` and the forward projection) negate `y`. A float negation is
   exact, so the source's grouping is untouched. The CPU↔GPU parity tier
   *cannot* prove this — both sides carry the flip — so it is proved
   algebraically by `ssr::tests::reconstruct_then_project_round_trips_the_uv`.
2. **`gl_FragCoord`.** WebGL counts `y` up from the bottom, `@builtin(position)`
   counts it down from the top. Both passes take their target size as a uniform
   lane (not in the source) and reconstruct `size.y - position.y`, so the
   interleaved-gradient dither matches the source's pattern rather than a
   vertically mirrored one.

A third difference is not an adaptation: `vUv` is computed from
`@builtin(position)` rather than taken from a varying. Same value for a
full-screen triangle, one interpolator out of the parity measurement.

A fourth, forced by the target and precedented by `gbuffer.rs`: three.js declares
every fragment output `vec4` and lets the attachment's channel count discard the
rest. `contact_fs` and `contact_blur_fs` return `vec2<f32>` because the target is
`Rg16Float`. Same numbers.

## 5. Transcription notes worth re-reading before touching an expression

SSR:

- `uTexel` is **declared and never read** by the source's fragment shader. Kept
  as a lane, named, documented as dead.
- `stepScale = pow(maxDist / 0.06, 1.0 / 28.0)` — division inside the `pow`,
  exponent not pre-multiplied. `t` starts at `0.06 + jitter * 0.06` but the scale
  is derived from `0.06` alone, so a jittered ray trips the tail
  `if (t > maxDist) break` slightly early. That is the source's arithmetic.
- The thickness window **grows** with distance (`thickness + t * 0.06`); the
  confidence fade uses the **un-grown** thickness. So a hit found late can be
  inside the test and outside the fade, and correctly fades to nothing.
- The confidence uses the hit iteration's `t`; the edge fade uses the
  **refined** UV. Different quantities, both the source's.
- `lo` is written by the refine loop and never read after it. Kept.
- The refine samples at an **unclamped** UV that can leave the screen; the
  samplers are `ClampToEdge` and `ScreenImage` clamps identically.
- The velocity's `y` is negated on read
  (`gbuffer::VELOCITY_TEXTURE_V_SIGN`) — SSR is exactly the "reprojects with
  `uv - velocity`" consumer that constant was written for.

Contact:

- `occ = max(occ, 1.0 - t*t)` is immediately followed by `break` and `occ` is
  `0.0` on every path that reaches it. The `max` is **dead** in the source. Ported
  and named as dead.
- `NdL` gates and is never used again — it does **not** scale the occlusion.
- The loop's `continue` on `cov < 0.5` is not a `break`. Collapsing them would
  stop every ray at the first sliver of sky it crosses.
- `bias` uses the **scene's** depth at the sample, not the shading point's.
- `stepV = L * (len / 14.0)` — the division is inside the parentheses.
- `1e4` on an uncovered pixel is a **sentinel**, not a placeholder: it drives the
  bilateral's exponential weight to zero so sky never averages into geometry. It
  is exactly representable in `f16` (spacing at 8192 is 8; 10000/8 is an integer),
  so it survives the store.
- The bilateral's centre weight is `0.5` (`/1.4`); SSR's blur's is `0.4`
  (`/1.3`). Different filters, not interchangeable.
- `sum += a.r*wa + b.r*wb` adds **one** term whose two products are summed first.
  Written `sum + (a*wa + b*wb)` in Rust, because `sum + a*wa + b*wb` is a
  different grouping.

Storage widths, grepped before starting: SSR marches and blurs at **half**
resolution into `Rgba16Float`; contact runs at **full** resolution into
`Rg16Float` (`THREE.HalfFloatType` + `THREE.RGFormat`); the G-buffer's three
attachments are all `NearestFilter`; every post target is `LinearFilter` +
`ClampToEdge`.

## 6. Parity structure and the tolerances I *expect*

Both passes get three tiers, following `bloom_pyramid/parity.rs`. All use the
shared `test_gpu::TestGpu::shared()` fixture — no `wgpu::Instance` is created
anywhere in this slice.

| tier | what | expected tolerance | why |
|---|---|---|---|
| arithmetic (exact) | pure functions through a uniform | `1e-6` abs | 2–3 `f32` ULP at magnitude 1; `fma` contraction and reciprocal precision are the only remaining freedom |
| arithmetic (`owIGN`) | the dither only | `1e-3` abs | **algorithmic, not a concession** — see below |
| SSR march | real pipeline, 4 textures, `Rgba16Float` | `2e-3` abs | ~2x one `f16` ULP at magnitude 1 (`9.77e-4`) |
| SSR blur | real pipeline, `Rgba16Float` | `2e-3` abs | same |
| contact march (shadow) | real pipeline, `Rg16Float` | `2e-3` abs | same |
| contact march (depth) | same | `4e-3` **relative** | the lane is metres in an `f16`; one ULP is relative, so an absolute budget would either fail at the `1e4` sentinel or be meaningless near the camera |
| contact bilateral | real pipeline, `Rg16Float` | `5e-3` abs | as above plus two `exp` evaluations, which both sides approximate differently |

Every tier also asserts `measured * 10 >= budget`, so the first real run either
confirms the reasoning or produces the number that replaces it.

**Why `owIGN` needs its own budget.** It is
`fract(52.9829189 * fract(dot(p, k)))` — a hash. `p` reaches ~1500 (pixel plus
`frame * 7.331`), so `dot(p, k)` reaches ~110, where one `f32` ULP is `7.6e-6`.
One `fma` contraction of that two-term dot moves it by about that much; `fract`
keeps the absolute error and drops the magnitude; `× 52.98` multiplies it to
`~4e-4`; the outer `fract` keeps it. `1e-3` is the **floor** for this function on
any two implementations that do not agree bit-for-bit on the inner dot. That is
fine and worth stating: the jitter perturbs the march's start by `6e-5` metres,
four orders below the thickness window it feeds. The dither is allowed to
disagree; the geometry is not.

**The discrete hazard, asserted separately.** A march is a *predicate* over a
depth buffer. A pixel whose `diff` lands within a rounding error of the thickness
edge (SSR) or the bias/thickness edge (contact) can hit on one side and miss on
the other — an O(1) disagreement no continuous tolerance should ever absorb. Both
march tiers therefore assert **exact agreement on whether each pixel hit at all**,
separately, and a non-zero count is a finding: either `pow`/jitter diverged
further than the arithmetic tier says it can, or a scene pixel sits exactly on the
boundary and the scene must move. **Not a budget to widen.**

**The synthetic scenes are geometrically consistent, deliberately.** `ssr::tests::
floor_scene` builds its depth buffer by intersecting each pixel's view ray with
two real planes (floor `y = -1.5`, wall `z = -12`) rather than painting a ramp. A
painted ramp's gradient disagrees with its own normals, and a ray reflected off it
immediately "hits" the surface it just left — the test would pass while proving
nothing. Contact's stepped wall is the peer: the sun marches *into* the near slab,
because the reverse direction walks away from the step and finds nothing.

Both march tiers additionally assert that the scene produced *some* hits, so a
comparison of two black frames cannot read as a pass.

## 7. Deferrals, each with a named expiry

1. **`glsl.js`'s `COMMON` has no shared Rust home.** `ign`, `view_pos` and
   `project_uv` live in `ssr.rs`; `contact.rs` imports them. The right home is
   `crate::gbuffer`, beside `decode_normal`, whose own docs already argue the
   case ("it is what every consumer of slot 0 will run"). **Expiry: when a third
   `COMMON` consumer lands** — `gtao.js`, `taa.js` and `motionblur.js` all
   include it. **File that must change:
   `modules/axiom-gpu-backend/src/gbuffer.rs`** (plus the two `use` lines here).
   The WGSL copies are *not* a deferral: the source inlines `${COMMON}` into every
   pass, and two independently written shader transcriptions compared against one
   Rust reference is the check this port keeps needing.
2. **`half_storage` now has a second consumer.** `bloom_pyramid/half_storage.rs`
   says "the moment a second pass needs it, lift it whole". Both parity modules
   here use it. **Expiry: now.** It is a property of an `Rgba16Float`/`Rg16Float`
   attachment, so its home is `modules/axiom-gpu-backend/src/hdr_target.rs`. Not
   done here because `hdr_target.rs` and `bloom_pyramid/mod.rs` are other slices'
   files this wave.
3. **Neither pass is bound to a frame.** `lib.rs` has no `mod ssr`/`mod contact`
   yet, and no frame graph calls either. **Expiry: when the `render/index.js`
   sibling lands its pass schedule.** The `_firstFrame` guard on SSR (§2) is the
   one behaviour that lives *only* in the frame graph and must not be lost.
4. **`materialpatch.js` is not ported.** The two consumption functions exist here
   and are tested, but nothing calls them. **Expiry: when the material-patch slice
   lands** — it must call `ssr_resolve` and `contact_shadow_for_light` rather than
   re-deriving the cutoff and the sun-dot test.
5. **The `Rgba16Float` quantisation between march and blur is the caller's.**
   `ssr_blur_pixel` / `contact_blur_pixel` take whatever precision their
   `ScreenImage` holds; the parity harnesses quantise with `half_storage`. A
   production chain that skips the quantisation will drift from the reference by
   one `f16` ULP per pass. **Expiry: same as (3).**

## 8. Things I checked and did *not* find

- No `Math.hypot`, no `MathUtils.lerp`, no `MathUtils.smoothstep`, no
  `new THREE.Color`, no `Math.round`, no `|0`, no `rng.fork()`, no seeds in
  either file — this slice is pure GLSL plus two `THREE.Vector4` literals.
- The only `sign`-shaped construct is `owOctWrap`'s ternary, which is inside
  `gbuffer::decode_normal` and already ported. Nothing in either pass calls GLSL
  `sign`, so the "returns 0.0 for zero" trap does not arise here.
- `fract` appears only inside `owIGN`, on a strictly positive argument — but it
  is written `x - floor(x)` anyway, because a `%` would be wrong the moment the
  argument is not.
- Neither pass reads `Time`, so the `Time::default()` `scale: 0.0` trap does not
  arise. The dither's clock is `frame % 64`, an integer supplied by the caller.
