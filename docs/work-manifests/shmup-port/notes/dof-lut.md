# `render/dof.js` + `render/lut.js` → `modules/axiom-gpu-backend/src/{dof,lut}.rs`

Two post passes that sit at **opposite ends** of the frame, which is the single
most important thing in this note and the thing the file names hide.

| | source pass | space it works in | Axiom neighbour |
|---|---|---|---|
| `dof.js` | **12 of 18**, before metering | linear **HDR radiance** | reads `gbuffer`, runs before `exposure`/`bloom_pyramid`/`agx` |
| `lut.js` | inside **17/18**, the composite | **display-referred sRGB code values** | runs *after* `agx`, inside `post_chain`'s `graded()` |

## Files written

- `modules/axiom-gpu-backend/src/dof.rs` — `DOF_WGSL` (binding-free arithmetic)
  + `DOF_PASS_WGSL` (three fragment entry points) + the CPU reference + 13 pure
    tests.
- `modules/axiom-gpu-backend/src/dof/parity.rs` — four-tier CPU↔GPU parity.
- `modules/axiom-gpu-backend/src/lut.rs` — `GRADE_LUT_WGSL` + the `f64` grade,
  the table generator, and the CPU trilinear + 12 pure tests.
- `modules/axiom-gpu-backend/src/lut/parity.rs` — three-tier CPU↔GPU parity.

Nothing else was touched. No build, no test, no commit, per
`12-final-wave-brief.md`.

---

## 1. Where `lut.js` goes, and why the ordering *is* the correctness

`composite.js:131-145`, verbatim, is the whole argument:

```glsl
  vec3 col = owAgX( hdr, uLook.x, uLook.y, uLook.z );   // tone map
  col = clamp( col, 0.0, 1.0 );
  vec3 disp = owLinearToSrgb( col );                    // ENCODE
  vec3 graded = sampleLut( disp );                      // <- the LUT
  disp = mix( disp, graded, uGrade.y );
  // ... grain, ordered dither
```

The source's own comment above that block records a defect it already shipped
and fixed: *"the LUT's toe/shadowTint are additive **code value** offsets, so
feeding it linear light turned a 0.008 toe into a hard linear floor and painted
the whole frame's shadows blue-grey. Encode first, grade second."*

Every constant in the preset is calibrated against **where AgX puts things**:
`pivot = 0.50` because "AgX puts 18% scene grey near 0.50 display";
`saturation = 1.20` because "AgX's inset/outset pair is a *desaturating*
transform by construction". In front of AgX, or on linear light, all of them are
calibrated against nothing.

### The exact insertion point in `post_chain.rs`

`post_chain`'s HDR composite already runs the first four lines, and `graded()`
opens with `let d = srgb_encode(linear);` — **that `d` is the source's `disp`.**
So:

```wgsl
fn graded(linear: vec3<f32>) -> vec3<f32> {
    let d = srgb_encode(linear);
    let d = axiom_lut_apply(d, AXIOM_LUT_SIZE, AXIOM_LUT_STRENGTH);   // <-- HERE
    let f = max((d - vec3<f32>(params.grade.w)) / ...
```

**Before** the `FramePostProcess` grade terms (`f`, `e`, `k`, `s`), not after.
Three reasons in descending force:

1. The source feeds the LUT raw AgX output with **nothing** between. Any term
   inserted ahead of it hands the table code values it was never authored
   against.
2. `FramePostProcess` has no counterpart in the source chain at all. It is
   Axiom's *app-authored* whole-frame grade; the LUT is the engine's film print.
   The print goes on first and the colourist works on top.
3. `FramePostProcess` packs the identity when unauthored, so putting it second
   costs nothing in the default case. Putting the **LUT** second would make the
   frame depend on whether an app had authored a grade, which the reference's
   picture does not.

It is **not** in the bloom chain — bloom is added in *linear light* at
`composite.js:126`, eleven lines and one tone map earlier — and it is **not**
scene-referred.

## 2. The half-texel inset

`composite.js:33-34`, to the character:

```glsl
vec3 uvw = clamp( c, 0.0, 1.0 ) * ( ( n - 1.0 ) / n ) + ( 0.5 / n );
```

A 33³ texture's texel *centres* are at `(i + 0.5) / 33`. Input `0` must land on
centre 0 and input `1` on centre 32, so the map is `× 32/33, + 0.5/33`.
Transcribed with the source's grouping; both divisions kept as divisions and
`32/33` deliberately **not** folded to `0.969696` (there is a test asserting the
literal is absent).

Omitting it compresses the whole table by one texel and clamps both ends flat —
subtly wrong everywhere and plausible-looking, which is why it survives review.
`omitting_the_inset_moves_every_sample_by_half_a_texel` pins the magnitude, and
the parity tier compares the coordinate **on its own** rather than only through
a fetch that would partly absorb the error.

## 3. Format: no image, so no layout ambiguity

The brief warns about strip / tile-grid / row-major-blue layouts. **None apply**:
the LUT is *computed*, never encoded as a 2D image. `createGradeLut` builds a
`THREE.Data3DTexture(data, 33, 33, 33)` directly.

The write order still is the layout — `z` outer, `y`, `x` inner, RGBA
(`lut.js:153-163`) — so R runs along `x`, G along `y`, B along `z`, and `(r,g,b)`
is the sample coordinate. `texel_index` is that address and there are tests
pinning each axis.

## 4. Storage width found twice

- **The LUT table is `f64`.** `applyGrade` is JavaScript: every `Math.pow`,
  multiply and add is `f64`, and the *only* narrowing in the whole file is
  `Math.round(… * 255)` into a `Uint8Array`. `grade`/`shoulder_params`/`scurve`
  are `f64` throughout; `quantize` is the single narrowing. The **sampling** is
  the other side of that line and is `f32`, on the GPU, on bytes.
- **The DOF blur targets are `Rgba16Float`.** `hdrTarget` is
  `THREE.HalfFloatType` (`pass.js:67`), so the CoC that rides in alpha is
  rounded to `f16` **twice** — once storing the prefilter, once storing the
  gather. `quantized_coc` (over `bloom_pyramid::half_storage`) is the entry
  point for a chain-level reference; `gather`/`combine` deliberately do not
  apply it, because they model the arithmetic *inside* a pass, not the store
  between two.

## 5. Depth is a POINT fetch — the DOF finding that matters most

`tDepth` is the prepass's slot-2 attachment: `RedFormat` + `FloatType` with
`minFilter` **and** `magFilter` set to `THREE.NearestFilter`
(`prepass.js:151-171`). Every `texture2D(tDepth, …)` in `dof.js` is therefore a
point fetch, not a bilinear one.

This lands exactly right in Axiom: `GBufferChannel::Depth` is `R32Float`, which
is **non-filterable in wgpu**, so the only legal fetch is the one the source
already used. `DOF_PASS_WGSL` reads it with `textureLoad` (no sampler at all,
so a filtering one cannot be bound by accident) with an explicit clamp to the
edge texel. There is a test asserting the depth channel is never sampled and has
no sampler declared.

The clear value agrees too: the G-buffer pass clears colour to
`LoadOp::Clear(Color::TRANSPARENT)`, i.e. `0`, which is precisely the
`depth <= 0.0 ⇒ sky ⇒ 1e4 m` convention `dof.js:44`/`dof.js:49` encodes.

The **colour** fetches are the opposite: `tColor`/`tSrc` are `hdrTarget`s with
`LinearFilter`, so the gather's 32 spiral taps genuinely are bilinear.

## 6. There is no tile / max-CoC prepass

The brief asked for one "if the source has one". It does not. The source
substitutes two things, both ported:

- the prefilter packs `max(k0..k3)` — the 2×2 neighbourhood maximum — into alpha;
- the gather carries `max(centre.a, every tap's .a)` forward, which is a
  spiral-shaped max filter over the gather radius;

and the combine dilates the full-res CoC with `blur.a * 0.85`. The gather radius
is therefore always the frame's *global* maximum, never a per-tile one. The
source's own justification is that an in-focus tap contributes a weight of
exactly zero, so a fixed 32-iteration loop is correct as well as cheap — there
is a test pinning that (`an_in_focus_tap_contributes_no_weight`). Adding a tile
prepass later would be an optimisation, not a fidelity fix.

## 7. Bokeh pattern

32 taps (`#define OW_DOF_TAPS 32`), golden-angle spiral, `sqrt(t)` radial
distribution so it is area-uniform, per-pixel rotation from interleaved gradient
noise dithered by `frame % 64`. Weight `clamp(tapCoC * 0.5 - dist + 1, 0, 1)` —
scatter-as-gather.

Literals kept to the source's digits and **pinned against their closed forms**,
because a tidy-up is the likely defect:

- `2.39996323` ≠ `PI * (3 - sqrt 5)` (`2.3999632297…`) — different `f32`s.
- `6.2831853` ≠ `f32::consts::TAU` (`6.28318530717…`) — different `f32`s.
- `length(off)` is **not** algebraically collapsed to `sqrt(t) * radius`, and a
  test asserts the two forms differ in at least one tap.

## 8. Two settings tables, and they disagree

`DepthOfField`'s constructor defaults (`dof.js:157-158`) and the shipped
settings (`index.js:376-382`) are **not** the same numbers — `maxCoc` 5.0 vs
3.3, `nearRatio` 0.6 vs 0.38, `focusMax` 20 vs 18, `farStart` 1.2 vs 1.15,
`farRange` 20 vs 18. Both are ported (`CONSTRUCTOR_DEFAULTS`, `SOURCE_SETTINGS`)
with a test asserting they differ, because collapsing them onto one number is a
silent change to whichever frames use the other.

Same shape on the LUT side: `lutStrength` is `0.85` at
`createComposite` (`composite.js:333`) and `1.0` as shipped (`index.js:384`).

## 9. Not ported, deliberately, with the expiry check

`lut.js` also exports `srgbToLinear` / `linearToSrgb`. **Nothing imports them** —
`index.js:15` takes only `createGradeLut`. They are not ported because this
crate already has exactly one definition of that curve
(`surface_encode::SRGB_TRANSFER_WGSL`), and a second is precisely the drift that
module exists to prevent.

Checked before declining, since "algebraically equal, numerically different" has
already bitten this port once: the GLSL writes the exponent `0.41666667` and
Axiom writes `1.0 / 2.4`. They are the **same `f32`** (both are
`13981013 × 2^-25`), so the decision costs nothing — there is a test asserting
the bit equality. The only difference is the knee comparison's strictness (GLSL
`step` takes the power branch *at* `0.0031308`, the JS takes the linear one),
which is a single point of measure zero.

**Expiry:** if a future slice ever needs a JS-side `srgbToLinear`, it must use
`surface_encode`'s curve, not re-add these. The file that would have to change
is `modules/axiom-gpu-backend/src/surface_encode.rs`, and only if the knee
strictness is ever shown to matter.

## 10. Tolerances — all UNVERIFIED

This wave does not build or run, so every constant is a **derived expectation**
with its arithmetic written out at the definition, never a number fitted to an
observed miss. Both parity modules carry a
`the_tolerances_are_within_ten_times_the_measured_delta` test that re-measures
every tier on each run and **fails, naming the number to tighten to**, if any
tolerance is more than 10× the delta actually seen.

| tier | constant | derivation |
|---|---|---|
| DOF CoC | `2e-6` | 2 ULP at a CoC of order 3 (`4.8e-7`), ×4 |
| DOF prefilter/combine | `2e-6` | 2 ULP + one shared divisor rounding, ×4 |
| DOF spiral | `2e-4` | `cos`/`sin` argument reduction over 13 turns (`~1e-5`) × radius 4, ×5 |
| DOF gather | `2e-4` | same, accumulated |
| DOF IGN | `1e-2` | see below — `~2.5e-3` structural, ×4 |
| LUT inset | `1e-6` | 2 ULP at unit magnitude (`1.2e-7`), ×8 |
| LUT sampled | `6e-4` | 8-bit subtexel floor × a `~0.04` cell delta (`1.6e-4`), ×4 |
| LUT blend | `6e-4` | as above plus one `mix` |

### The IGN budget is structural, and the gather is protected from it

`owIGN` is `fract(52.9829189 * fract(dot(p, k)))` on a `gl_FragCoord`, so
`dot(p, k)` runs to ~140. One `f32` rounding at magnitude 140 is `7.6e-6`
**absolute**, `fract` lands that undiminished on a unit result, the outer
`× 52.98` scales it to `~4e-4`, and the outer `fract` keeps it whole. Times
`TAU` that is `~2.5e-3` radians. This is catastrophic cancellation *by
construction* — the same shape as `agx.rs`'s contrast-polynomial finding — and
no care in the transcription closes it.

So IGN gets its **own tier and its own budget**, and the gather tier is driven
with an **exact rotation handed in through the uniform**, never one computed
from IGN on the GPU. Mixing the two is how a chain tier ends up with a tolerance
nobody can justify.

Similarly the gather's 32 tap values are handed in from a function of the tap
index (mirrored exactly on both sides) rather than sampled, so the tier measures
the accumulation order and the weights — the transcription — not the hardware's
bilinear.

### If the LUT sampled tier comes back at `1.6e-4`

That is a real finding about **3D** texture filtering on this device (the
2D bilinear was measured far better in `bloom.md` §9) and it belongs in this
note, not in a widened budget.

---

## What I need from siblings and the orchestrator

### From the frame-graph sibling (`render/index.js`)

DOF is **pass 12**, after motion blur and before metering. It needs, per frame:

1. Two half-res `Rgba16Float` targets, `max(1, w >> 1) × max(1, h >> 1)` — a
   shift, not a rounded divide — with a **linear** clamp-to-edge sampler. Half
   float is not an economy; it is where the CoC is rounded, twice.
2. `GBufferTargets::view(GBufferChannel::Depth)`, bound as a plain
   `texture_2d<f32>` with **no sampler**.
3. `tune.x` = `dof::max_coc_pixels(setting, internal_height, ads_amount)` — the
   *internal* render height, not the canvas height (`index.js:918` calls
   `setSize(rw, rh)`).
4. `tune.y` = `dof::frame_phase(frame)`.

Run order is strictly prefilter → gather → combine, each reading the previous
target. The gather binds its source in the **same slot** the prefilter binds the
scene colour, so one bind-group layout serves all three pipelines (wgpu permits
a layout to carry entries an entry point does not statically use).

The pass is skipped when the sights are down (`_adsT > 0.01`). At `amount = 0`
the chain is an exact copy rather than a wrong frame, but it is still three
passes of bandwidth.

The frame graph also owns the **bilinear** half of the gather, which this
slice's parity deliberately does not cover.

### Wiring lines the orchestrator must add

```text
modules/axiom-gpu-backend/src/lib.rs: mod dof;
modules/axiom-gpu-backend/src/lib.rs: mod lut;
```

Both are pure arithmetic + `&str`, so they compile everywhere with no `cfg`,
exactly like `agx` and `exposure`. Their `parity` submodules are already gated
`#[cfg(all(test, feature = "offscreen"))]` inside the files.

### Composite edits (LUT), when the wave integrates

1. Concatenate `GRADE_LUT_WGSL` into the composite source **on the HDR arm
   only**, alongside `agx::AGX_WGSL`.
2. Renumber `GRADE_LUT_WGSL`'s `@group(1)` to whatever is free in the composite
   — it is a placeholder and the one thing in that string not transcribed from
   the source.
3. Add to `tone_constants`: `AXIOM_LUT_SIZE` from `lut::SIZE` and
   `AXIOM_LUT_STRENGTH` from `lut::LUT_STRENGTH` (not retyped).
4. Insert `let d = axiom_lut_apply(d, AXIOM_LUT_SIZE, AXIOM_LUT_STRENGTH);`
   immediately after `let d = srgb_encode(linear);` in `graded()`.
5. Upload `lut::grade_lut(lut::SHIPPED_PRESET)` once at startup into a
   `33³ Rgba8Unorm` **3D** texture with a linear clamp-to-edge sampler.

**Confirm WebGL2 3D-texture support on the real device before switching the
browser arm on.** `33` fits `downlevel_webgl2_defaults().max_texture_dimension_3d`
(256), but that is a limit check, not a support check.

Both modules carry a `nothing_in_the_present_path_compiles_this_yet` test. When
each is wired, that test must be **replaced** — not deleted — by one asserting
which arm carries the text, and for the LUT specifically that it still sits
after `srgb_encode` and before the `FramePostProcess` grade terms, since that is
the ordering a later edit is most likely to quietly reverse.
`agx.rs`'s `only_the_hdr_composite_arm_carries_agx` is the worked precedent.
