# `render/taa.js` + `render/motionblur.js` → `modules/axiom-gpu-backend/src/{taa,motionblur}.rs`

Two passes that share the G-buffer velocity channel, ported together because
they share the one divergence from the source and would otherwise have derived
it twice.

Nothing here has been built, compiled or run — the final-wave brief forbids it.
Every tolerance below is an **expectation with its reasoning**, not a
measurement.

---

## 1. What landed

| file | contents |
|---|---|
| `src/taa.rs` (~1.5k lines) | Halton jitter, the resolve's WGSL + CPU reference, 8-entry-point GPU parity |
| `src/motionblur.rs` (~1.3k lines) | tile-max + blur WGSL + CPU reference, 9-entry-point GPU parity |

No `wgpu` resources, no pass structs, no ping-pong bookkeeping: those belong to
the frame graph (`render/index.js`'s port), and §5 says exactly what it must
supply. What is here is the arithmetic, the shader text, and the proof they
agree.

WGSL entry points:

- `taa_vs` / `taa_resolve_fs` — one module, `taa_shader_source()`.
- `mb_vs` / `mb_tile_fs` — `motion_blur_tile_source()`.
- `mb_vs` / `mb_blur_fs` — `motion_blur_blur_source()`, a **separate** module.

## 2. The one divergence: `VELOCITY_TEXTURE_V_SIGN`

`gbuffer.rs` stores `(curr.xy/w − prev.xy/w) * 0.5` from unjittered
view-projections, in a clip space whose `y` runs **up**. WebGL's framebuffer `v`
also runs up, so in the source the stored delta *is* the texture-space delta.
WebGPU's `v` runs down, so it is not.

Applied in exactly four places, all the same fact, each declared as
`const taa_v_sign` / `const mb_v_sign` in the WGSL and pinned to the gbuffer
constant by a test that also counts the occurrences:

1. `taa_velocity` — the velocity-texture read.
2. `taa_background_velocity` — `uv → NDC`.
3. `taa_background_velocity` — reprojected `NDC → uv`.
4. `mb_velocity` — the velocity read, *after* the tile-vs-own selection.

Position 4 matters: the source selects by `length()`, which is sign-invariant,
so flipping after the compare leaves the tile pass and the selection
**bit-identical to the source** and confines the divergence to one line. Flipping
before would have been equally correct and would have made the diff against the
JavaScript larger for no gain.

The **jitter deliberately gets no flip.** The resolve never learns the jitter —
velocity comes from unjittered matrices — so convergence only requires that the
whole frame rasterise at one common sub-pixel offset. Mirroring the sequence in
`y` would change which sub-pixel positions are visited and in what order, for no
benefit.

`z = 1.0` in the far-plane reprojection needs **no** change: WebGL NDC depth is
`-1..1` and WebGPU's is `0..1`, and far is `1` in both.

## 3. Transcription, and what the second reading caught

Both the WGSL and the CPU reference were written from the GLSL text, hunting
specifically for the failure modes the brief names.

**Divisions kept as divisions** (no reciprocal-multiply anywhere):
`tonemapW`'s `c / (1 + lum)`, `tonemapWInv`'s `c / max(1e-4, 1 - lum)`,
`sampleCatmullRom`'s `w2 / max(w12, 1e-5)` and its three `/ texSize`,
`result / max(wsum, 1e-5)`, `m1 / 9.0`, `m2 / 9.0`, `extent / max(|dir|, 1e-5)`,
`1.0 / (1.0 + Y)`, the `/ max(sum, 1e-5)` blend, `(d - centreDepth) / max(1, centreDepth)`,
`maxPx / pixels`, `(i + jitter) / 12`, `sum / wsum`.

**Groupings preserved verbatim**, notably:

- Catmull-Rom's Horner nesting `f * (-0.5 + f * (1.0 - 0.5 * f))` — expanding it
  into powers of `f` is algebraically equal and numerically different.
- `( curY * wc * ( 1.0 - feedback ) + clipped * wh * feedback )` as
  `((curY*wc)*(1-fb) + (clipped*wh)*fb)`, left to right.
- `( vec2(x, y) - 3.5 ) * 2.0 * uTexel` as `((v - 3.5) * 2.0) * texel`.
- `shutter * (1 / 60 / dt)` as `shutter * ((1/60) / dt)`, in `f64`, narrowed once.

**`mix`, `clamp`, `step`, `smoothstep`, `length` are written out** in both the
WGSL and the Rust — GLSL's `mix(x,y,a)` is `x*(1-a) + y*a`, not `a + (b-a)t`, and
WGSL's builtins are permitted to factor differently. `length` is the plain root,
**not** `jsmath::hypot`: this is GLSL, where nothing is compensated.

**Halton's `while (i > 0)`** became a 32-digit `fold`, which is *exact*, not an
approximation: once `i` reaches zero every remaining term is `f * 0.0 == 0.0` and
`r + 0.0 == r`. A test drives 192 (index, base) pairs against a direct
transcription of the source loop and asserts bit-equality.

**Jitter precision.** Three's `Matrix4.elements` is a JS array of `f64`, narrowed
once on upload, so `taa_jitter_projection` does `f64::from(m[8]) + jx` and casts
at the end. Doing the add in `f32` rounds twice and lands on a different
sub-pixel position — which is the filter.

## 4. Storage width

- **TAA history is `Rgba16Float` and the resolve reads its own previous output.**
  That is a feedback loop, so the `f16` rounding is *inside* it and a reference
  that skips it drifts further every frame rather than staying one ULP away.
  `taa_history_store` is that rounding, borrowed from
  `bloom_pyramid::half_storage::quantize`.
- **Motion blur has no feedback loop** — nothing reads its own previous output —
  so its `Rgba16Float` output and `Rg16Float` tile target are a single
  quantisation at the end. That is why this module has no `history_store` peer,
  stated rather than omitted.

## 5. What the frame graph must supply

Assuming natural names in `render/index.js`'s port:

**TAA**
- the **jittered projection** for the world camera only —
  `taa::taa_jitter_projection(&projection, frame_index, width, height)`, with a
  counter that advances once per resolved frame (the source's `nextJitter`
  increments on every call). Never the viewmodel camera: it has its own MSAA
  target and no history, so a jitter there is a permanent wobble.
- the **unjittered `invVP` and previous-frame `prevVP`** — the same pair
  `gbuffer::pack_gbuffer_uniform` already takes. Pack with
  `taa::pack_taa_uniform`.
- **two `Rgba16Float` history targets, ping-ponged.** The pass reads one and
  writes the other; the written one is the resolved colour, the *unwritten* one
  is the source's `previousTexture` and the correct SSR source.
- `params.z = 1.0` on the first frame after a resize or a camera cut, `0.0`
  otherwise (the source's `_needsReset`, set by `setSize` and `reset`).
- a **linear-filter, clamp-to-edge sampler**. The Catmull-Rom taps land at
  fractional texel offsets and are meaningless without it.

**Motion blur**
- tile target sized by `motion_blur_tile_size(w, h)`, format `Rg16Float`.
- `params = [motion_blur_shutter(0.42, dt), MOTION_BLUR_MAX_RADIUS_PX,
  (frame % 64) as f32, MOTION_BLUR_INTENSITY]`.
- the tile pass's `texel` is the **full-resolution** texel size, not the tile
  target's.
- **two shader modules, not one**: both passes claim `@group(0) @binding(0)`, and
  a WGSL module may not declare two resources at one group/binding. The shared
  vertex stage therefore lives in the binding-free `MOTION_BLUR_WGSL_COMMON`.

Ordering in the source's frame graph: TAA resolves, then motion blur consumes
the resolved colour, then the composite.

## 6. Source characteristics carried, not fixed

- **`uParams.w` ("motionScale") is dead in `taa.js`** — declared, set to `1`,
  never read. Carried, named at its declaration, and pinned by a test asserting
  `params.w` appears zero times in the arithmetic.
- **`uTexel` is dead in `motionblur.js`'s `BLUR`** — declared and uploaded by
  `setSize`, never read by the body. Same treatment.
- **The `MotionBlur` constructor's `shutter = 0.5` is dead** — `render()`
  overwrites `uParams.x` from `index.js`'s `0.42` every frame before the pass
  runs. Carried as `MOTION_BLUR_CONSTRUCTOR_PARAMS`, pinned.
- **The tile dilation under-covers its own tile by one texel.** The 8×8 taps span
  `-7 … +7` texels — fifteen of the sixteen the tile covers. Ported as written
  and asserted, so nobody later reads it as an off-by-one to "fix".
- **`bestLen` starts at `0.0` with a strict `>`**, so an all-zero tile answers
  `(0,0)` and the earliest of equal magnitudes wins. Both are load-bearing: the
  first decides whether a static tile blurs at all, the second decides which of
  two equally-fast objects owns the tile.
- **`taa.js`'s dilation seeds `bestDepth = 1e9` while an uncovered tap reads
  `1e8`**, so the first tap always wins initially and `bestUv` is never the
  unmodified centre. Preserved, with the all-uncovered case asserted.
- **`mb_tap_inside` is written as the negation of the source's skip test**, not
  as an interval test, so a NaN `uv` is *kept* exactly as the source keeps it
  (`NaN < 0` is false, so the source does not `continue`).

## 7. A stated divergence with no fix applied: the dither's screen coordinate

`owIGN( gl_FragCoord.xy + uParams.z * 2.717 )` reads WebGL's `gl_FragCoord`,
whose `y` counts **up** from bottom-left. WGSL's `@builtin(position).xy` counts
**down** from top-left, so this pass's dither pattern is the source's mirrored in
`y`.

Not corrected. Interleaved gradient noise has no preferred origin, the value
feeds a `±0.5` sample offset rather than a colour, and correcting it would mean
threading the render height in solely to reproduce a phase. Recorded here and at
the shader so nobody later reads a mirrored dither as a bug. **The fix, if exact
browser parity is ever wanted, is `vec2(position.x, resolution.y - position.y)`
in `MOTION_BLUR_BLUR_WGSL`** — one line, no other file.

## 8. WGSL restrictions that shaped the port

Two things the GLSL does freely and WGSL does not:

1. **`textureSample` is illegal in non-uniform control flow.** Every tap in both
   passes sits inside a loop or behind a branch, so every fetch is
   `textureSampleLevel(..., 0.0)`. No target here carries mips, so level 0 is
   what `texture2D` read anyway. A test asserts `textureSample(` appears zero
   times in either shader.
2. **A value-typed `array` parameter cannot be indexed by a runtime value.**
   `taa_resolve`, `taa_dilate`, `taa_catmull_rom_combine`, `mb_tile_max` and
   `mb_accumulate` each copy the parameter into a function-scope `var` first.
   Same arithmetic, same order; the copy is what makes the loop legal. (Same
   reason `bloom_pyramid::wgsl` uses `var taps = array(...)` for its tap tables.)

## 9. Tolerances — expectations, not measurements

None has run. Each carries its reasoning at its declaration, and each module has
a `the_tolerances_are_within_ten_times_the_measured_delta` test that re-measures
every tier and fails if a budget is more than 10x looser than the hardware needs
— so the first green run both validates these and prints the numbers that should
replace them.

| tier | budget | reasoning |
|---|---|---|
| `taa` resolve | `4e-6` | ~40 multiply-adds, two reciprocals, a `sqrt`, a divide, outputs order 1 |
| `taa` Catmull-Rom weights / coverage | `5e-7` | four Horner polynomials in one variable |
| `taa` Catmull-Rom plan | `1e-6` | four divisions; the likeliest reciprocal substitution |
| `taa` velocity + reprojection | `1e-5` | two `mat4` products, two perspective divides |
| `taa` dilation | **exact** | selection only |
| `mb` tap weight / blend | `1e-6` | a divide, a `smoothstep`, a `mix` |
| `mb` accumulation | `2e-5` | 24 weighted adds of the above |
| `mb` velocity (`uv` lanes) | `1e-8` | order `1e-2`, one ULP ≈ `1e-9` |
| `mb` velocity (`pixels` lane) | `1e-5` | order 30, one ULP ≈ `2e-6` — a separate budget because one number cannot serve both magnitudes |
| `mb` tap positions | `1e-7` | a divide by `12.0`, which a driver may reciprocal |
| `mb` tile offsets / tile max | **exact** | multiplication and selection only |
| `mb` `owIGN` | `2e-5`, wraps excluded | see below |

**`owIGN` cannot be held tight and should not be.** `fract(52.98 * fract(dot(...)))`
amplifies one ULP of the inner `fract` by two orders of magnitude, and *at* a
wrap it is discontinuous, where no tolerance is meaningful — only a side. The
test therefore skips probes within `1e-3` of `0` or `1` and asserts that at least
half the probes survived the filter, so it cannot silently degrade into proving
nothing.

## 10. Wiring the orchestrator must apply

```
modules/axiom-gpu-backend/src/lib.rs: mod taa;
modules/axiom-gpu-backend/src/lib.rs: mod motionblur;
```

No cfg gate is needed on either: both files are pure Rust plus `&str` shader
text, with `wgpu` touched only inside `#[cfg(all(test, feature = "offscreen"))]`.
Expect `dead_code` warnings until the frame graph consumes them, which is correct
— they disappear when the wiring lands, and silencing them first would hide a
module that never got connected.

## 11. Raised for someone else

- **`bloom_pyramid::half_storage` now has its second consumer**, which is the
  condition its own doc sets for lifting it: *"This is a property of an
  `Rgba16Float` attachment, not of a bloom — it is `crate::hdr_target`'s topic,
  one file up… The moment a second pass needs it, lift it whole."* `taa.rs` uses
  it across a module boundary today. Not done here — it moves
  `bloom_pyramid/mod.rs` and `hdr_target.rs`, neither of which this slice owns.
  **The lift is now due**; the mechanical change is moving the file and updating
  two `use` paths.

- **A shared GPU parity harness is now ~1,800 duplicated lines across the
  crate**, and this slice adds two more copies of the ~200-line
  adapter/render/readback rig. The `material_shader` log already deferred this to
  composition for good reason (interrupting in-flight agents costs more than the
  dedup saves). Recording that the count went up by two, and that both of these
  harnesses take two uniform bindings rather than one — whatever shared rig
  lands must support that, or these two revert to bespoke.

- **Nothing in this slice reads `GBufferChannel::Depth`'s units beyond
  "positive metres"**, which is what `gbuffer.rs` states. `mb_tap_weight`'s
  rejection is *relative* (`/ max(1, centreDepth)`), so it is scale-free above
  1 m and absolute below it — worth knowing if the depth channel ever changes
  units.
