# `render/exposure.js` + the AgX half of `render/composite.js`

Slice: **EV100 metering and the AgX tone map** (spine, `modules/axiom-gpu-backend`).
Source: `src/render/exposure.js` (218 lines), `src/render/glsl.js:110-160`
(the `TONEMAP` chunk `composite.js:132` calls), plus the limits and key the
renderer sets at `src/render/index.js:213,342`.

Delivered:

| file | what |
|---|---|
| `modules/axiom-gpu-backend/src/agx.rs` | `AGX_WGSL` + the CPU reference + 12 unit tests + real-adapter parity |
| `modules/axiom-gpu-backend/src/exposure.rs` | `EXPOSURE_WGSL`, `EXPOSURE_PASS_WGSL` (the three passes), the CPU reference, 18 unit tests + real-adapter parity |

`post_chain.rs` was **not** widened. Neither module is bound by anything, by
design — see §5.

---

## 1. The metering shape, because it is not the usual one

It is **not** a fixed key and **not** a plain average. Five things, all ported:

1. **A four-level GPU reduction**, `64 -> 16 -> 4 -> 1`
   (`exposure.js:145-148,175-186`). Level one takes **four** bilinear taps of the
   scene at `±1` texel; levels two to four are a 4x4 box each. No readback, no
   stall — the whole history is one 1x1 float texel ping-ponged between two
   targets, which is why the Rust `adapt` takes `prev_ev` as an argument rather
   than owning state.
2. **A per-tap luminance clamp of 40** (`uMeter.z`), applied *before* the log.
   The solar disc is authored at radiance 4000; one such pixel in a 4-tap box
   drags the log average by stops.
3. **Centre weighting**, `w = exp(-dot(d,d) * 1.1)` over `d = (uv - 0.5) * 2`.
4. **Sky de-weight ramped by the sky's own luminance.** Where linear depth is
   `0` (nothing written) or `> 400 m`, the weight is scaled toward `0.15` — but
   only as luminance crosses the `(0.06, 0.3)` knee. The ramp is the whole
   point: de-weight unconditionally and a moonlit sky, the only absolute anchor
   the meter has at night, stops anchoring anything and night adapts up into an
   overcast afternoon.
5. **A key scale**, `exposure = key / (1.2 * 2^ev)` with `key = 1.06`
   (`index.js:342`). `1.2` is `78/(q·S)` at `q=0.65, S=100`.

Adaptation is **asymmetric** and in this direction: a *rising* EV (the image
getting darker) uses `3.2`, a falling one `1.4` — the eye brightens up slowly.
`dt` is clamped at `0.1` and the whole EV at `[-4.3, 20]`, the lower end being
a documented **night lock**, not a limit that ever binds in daylight.

Two sign traps that the source itself got wrong once and now states loudly, both
preserved: the EV **bias is added** (positive = darker), and `ev100 > prevEv`
selects `speedDown`.

There is a sixth multiplier the composite applies and the meter does not:
`uLook.w = ctx.config.exposure ?? 1` (`index.js:1530`), a flat app-level scale on
the exposure scalar. Noted in the module docs; it belongs to whoever wires the
composite.

## 2. AgX's constants, and their provenance

Transcribed from the GLSL text of `glsl.js:110-160`, not from any other AgX
implementation. Four matrices, one 6th-order polynomial, two EV bounds:

- `minEv = -12.47393`, `maxEv = 4.026069` — a 16.5-stop log window.
- `OW_REC2020_FROM_SRGB` / `OW_SRGB_FROM_REC2020`, then AgX's own `inset` /
  `outset`. **GLSL's `mat3(a,b,c)` takes COLUMNS**, so every matrix in the Rust
  and the WGSL is the *transpose* of the literal text. A test applies each matrix
  to the red basis vector and asserts it returns the GLSL's first column, which
  is the one assertion a copy-paste transposition cannot survive.
- `15.5x⁶ − 40.14x⁵ + 31.96x⁴ − 6.868x³ + 0.4298x² + 0.1191x − 0.00232`, with the
  source's grouping preserved verbatim (`x4 = (x*x)*(x*x)`, `x⁶ = x4*x2`,
  `x⁵ = x4*x`), because float addition is not associative.
- Look: `slope 1.0, power 1.0, sat 1.08` (`composite.js:344`).

**It is not ACES, and the existing grade is not ACES either.** `glsl.js` also
defines `owACES` with a completely different matrix pair and a rational fit; the
composite never calls it. Axiom's current `post_chain` composite ends in
`FrameBloom::tonemap`'s reciprocal rolloff plus `FramePostProcess`'s
exposure/contrast/saturation — an LDR grade, frequently *described* as ACES,
sharing no constant with either curve. Nothing here is derived from it.

Three measured properties worth knowing downstream, each of which contradicted
this slice's own first-draft test:

- The contrast polynomial **stops short of both ends**: `-0.00232` at 0 and
  `0.99858` at 1. Consequently **AgX's ceiling is 0.99698, not 1.0** — code value
  254. A test that asserts a blown highlight reaches display white is wrong.
- The pivot is **not** the middle of the log window: `0.5` in maps to `0.2915`.
  18% scene grey sits at `0.6061` of the window and comes out at `0.4968`.
- The inset **does not preserve neutrality exactly** (unequal near-unit row
  sums), so saturation moves a scene grey by ~1.4e-4. Real, not a defect.

## 3. Measured tolerances

Both modules follow `material_shader/cloth`'s pattern: a CPU reference that is
the semantic definition, held against a real adapter over 24 contexts on a
`Rgba32Float` target, scored `|got − want| / max(|want|, 1)`.

Adapter: **Vulkan**. Every number below is from a run, and `MEASURED_WORST` is
*asserted* rather than printed, so the run fails if an adapter deviates more than
the record — the justification cannot rot.

| entry point | worst scaled | |
|---|---|---|
| `agx_pow_fs` (the four transcendentals alone) | `1.23e-7` | ~2 ULP |
| `agx_inset_fs` (`rec2020` then `inset`) | `1.15e-7` | ~1 ULP |
| `agx_outset_fs` (`contrast`, `outset`, `srgb`) | `4.77e-7` | ~4 ULP |
| **`agx_fs`** (the whole curve) | **`8.34e-6`** | budget `2.0e-5` (2.4x) |
| `meter_reduce_parity_fs` | **`0`** | bit for bit |
| `meter_loglum_parity_fs` | `2.18e-7` | ~2 ULP |
| **`meter_adapt_parity_fs`** | **`2.73e-7`** | budget `1.0e-6` (3.7x) |

**The AgX finding is the interesting one, and it contradicts the obvious
account.** The four transcendentals, isolated, agree to two ULP; so do the
matrices; so does contrast-plus-outset over the raw unit inputs. The *whole
curve* is seventy times worse. The cause is the contrast polynomial:
`15.5x⁶ − 40.14x⁵ + 31.96x⁴ − …` reaches **intermediates of magnitude 40 to
produce a result near 1**, so it is catastrophically cancelling. One f32 rounding
at magnitude 40 is `3.8e-6` *absolute* and lands undiminished on a unit-scale
result; the final `pow(_, 2.2)` scales it by 2.2. A single `fma` contraction —
permitted to the GPU, unavailable to Rust — moves one of those roundings, and
`8.3e-6` is the bill.

That makes the polynomial's **grouping load-bearing in a stronger sense than
usual**: re-associating it moves the frame by parts in a million, not parts in a
billion. It is also why the budget cannot be tightened without rewriting the
source's curve. `agx_pow_fs` exists solely to make this attributable; it is
called by nothing in the tone map.

The `agx_pow_fs` entry also settles the `pow(x, 1.0)` question empirically:
Rust's `powf` returns `x` exactly for an exponent of one where a GPU may evaluate
`exp2(1.0 * log2(x))`, and the shipped `power` *is* 1.0 — but at `1.2e-7` it is
not the dominant term.

The reduce being **exactly** equal is worth its own line: sixteen additions in a
fixed order and a divide by a power of two leaves nothing for a contraction to
reorder — and it is also the proof that the harness feeds both sides the same
bytes, which matters because the first version of it did not (see §7).

`EXPOSURE_PASS_WGSL` cannot be held to a CPU reference (it is texture wiring), so
it is held to **building a real pipeline** against the real validator with the
real bind group layout — which catches a binding, a swizzle or an entry-point
signature that a module-level validate would not.

## 4. The photometric contract — checked, and it agrees

`exposure.js:107` computes `log2(lum * 100 / 12.5)` on a **framebuffer radiance
unit**, not on cd/m². `apps/shmup/src/sky/atmosphere.rs` fixes that unit at
`SCENE_LUX = 25000` cd/m², so the shader's `ev100` is a true EV100 **minus
`log2(25000) = 14.61` stops**. That is the same contract in a different unit, not
a discrepancy — and it is checkable, so `exposure.rs` checks it:

- the sky module's own worked example (sunlit stucco, albedo 0.4 at 45°, ~0.32
  radiance units) meters at EV 1.36 here, which converts to a **true EV100 of
  15.97** — the textbook "sunny 16" reading;
- the same test asserts that with the stray `π` the sky module records as its
  1.65-stop bug, the reading would be 17.6 and would miss. So the 1.65 stops are
  now pinned from *both* sides of the boundary.
- `index.js:211`'s claim that daylight frames meter between −1 and −2.1 inverts
  to a weighted mean of **0.029–0.063 radiance units**; both ends are asserted
  through the whole chain.

## 5. What I found, and what is still missing

**Confirmed, and it matters: the metering and the tone map are not yet fed
linear HDR.** `RenderCapability::HdrTargets` has landed as a *declaration*
(`hdr_target.rs`, granted at bind from the adapter's `Rgba16Float` format
features), but **nothing allocates the float intermediate it licenses**:
`surface_encode::scene_target_format` still returns the surface format with an
sRGB suffix, i.e. 8-bit, on every arm, and `post_chain.rs`'s own header still
describes itself as "the substitute". So today a fragment emitting 4.0 is clamped
to white before any post pass can see it, `TAP_CLAMP`'s 40 could never once bind,
and metering that buffer measures the clamp. Both modules are inert until the
intermediate is float; that is the single largest blocker on this half of item 4.

Also outstanding, and outside this slice:

1. **G8 / depth.** The sky de-weight needs a *linear view depth in metres*
   channel. The G-buffer slice is landing `gbuffer.rs`; the meter's `hasDepth`
   lane is the switch, and with it at 0 the de-weight is a bit-exact identity, so
   the metering is correct (just sky-biased) before that arrives.
2. **The `evBias` the sky publishes** (`sky/index.js:754`: `1.35` at low sun
   elevation, `0.55` after dark) is an app-tier input to `params.w`. Not ported
   here; the shmup app's sky facade owns it.
3. **A host-tier opt-in.** There is no authored channel for "use AgX" or "meter
   this frame". `FramePostProcess` has no tonemap discriminant. My recommendation
   is a selector on the frame's post-process data plus a `mix` weight in the
   composite, so weight 0 is bit-identical to today's output — but that is an
   `axiom-host` change and I did not make it (layer, another slice's territory).

**Opt-in proof.** Each module carries
`nothing_in_the_present_path_compiles_this_yet`, an `include_str!` scan of
`post_chain.rs`, `upscale.rs`, `surface_encode.rs` and `scene_renderer.rs`
asserting that none of them names `AGX_WGSL`, `EXPOSURE_WGSL`,
`EXPOSURE_PASS_WGSL`, `agx::` or `exposure::`. No pipeline in the crate compiles
either string, so no app in the repo changes by one bit from these modules
existing. When the wiring lands, that test fails — deliberately — and must be
replaced by one proving the OFF path is still bit-identical.

## 6. Wiring needed

`modules/axiom-gpu-backend/src/lib.rs`, unconditional (both modules are pure
arithmetic and belong in the native coverage gate):

```rust
// The AgX filmic tone map and the EV100 metering chain that feeds it, ported
// from the reference's `src/render/glsl.js` and `src/render/exposure.js`. WGSL
// text plus a CPU reference that is its semantic definition; pure arithmetic, so
// compiled everywhere and covered natively. Nothing binds them yet — see each
// module's `nothing_in_the_present_path_compiles_this_yet`.
mod agx;
mod exposure;
```

Those two lines are **already in the working tree** — without them the modules
are not compiled and none of the verification above is reproducible, and the
sibling slices landing this wave (`gbuffer`, `bloom_pyramid`, `cascade`) added
theirs the same way. Both are additive; no other line of `lib.rs` was touched.

Until something calls them, every `pub(crate)` item is `dead_code`-warned, as the
material-shader layers were before composition. No `#[allow]` was added, for the
same reason: the warning disappearing is how you know the wiring landed.

## 7. Two harness lessons, for the slices still to come

Both cost real time here and both are cheap to avoid.

**A `resize` in `uniform_bytes` hides a packing bug.** The exposure harness
packed 26 `vec4` per context and the WGSL strided by 24. `bytes.resize(SAMPLES *
LANES * 16, 0)` — copied from the `material_shader` layers, where it is
harmless — *truncated* the buffer, so the GPU read every context but the first
from the wrong offset. The parity run reported a 36% disagreement, which at least
does not look like a ULP; a smaller stride error would have looked exactly like
one. Both files now `assert_eq!` the packed length instead. **Every harness in
this port should do the same** — the `resize` is a silent-corruption primitive
with no upside.

**Assert one budget at the end, not one per lane.** The per-lane `assert!` inside
`compare` (again inherited from the layers) reports the *first* disagreement,
which is not the number a budget has to be set from. Both files now collect the
worst per entry point and assert once, with every entry point's worst in the
failure message — which is what made the contrast-polynomial attribution in §3
possible at all.

**Coverage footnote, same shape:** a value passed as a *trailing format argument*
to `assert!` is an argument expression, evaluated only when the assertion fails —
an uncoverable region. Five of them cost this slice its 100%. Bind the value with
`let` and use an inline capture (`{mid}`); the tests read better anyway.
