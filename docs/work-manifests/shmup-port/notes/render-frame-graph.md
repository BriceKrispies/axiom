# `render/index.js` + `render/pass.js` → `frame_graph/` — slice notes

**Slice:** `C:/dev/Claude-of-Duty/src/render/index.js` (1,696) and
`src/render/pass.js` (80) — the frame graph itself.
**Target:** `modules/axiom-gpu-backend/src/frame_graph/` (12 files, ~4,900 lines
including docs and tests).
**Built / tested / committed:** no, per `12-final-wave-brief.md`.

---

## 1. What was written

| file | source | what it is |
|---|---|---|
| `mod.rs` | `index.js` header | the frame-order contract, the deferral, the storage-width note |
| `quality.rs` | `core/config.js` + `QUALITY_LEVEL` | the four tiers, the CSM clamp, `boot_line` |
| `fullscreen.rs` | `pass.js` + `glsl.js` `FS_VERT` | the shared triangle, `Pass` state, `blit`, the WGSL vertex stage |
| `targets.rs` | `resize()` | the two sizes, the five frame-graph targets, formats + byte cost |
| `pipeline.rs` | `init()`'s construction block | which passes exist, tier × capability |
| `schedule.rs` | `render(ctx)` | **`plan()`** — the ordered step list, ping-pong, history |
| `lighting.rs` | `_syncSun` / `_cullLights` / `_updateBounceFill` / `_updateViewRig` | step 1's arithmetic |
| `rooms.rs` | `_updateRooms` | the interior-volume gate, the yaw recovery, V8 `hypot` |
| `frame_inputs.rs` | the per-frame scalars | `dt`, shutter, ADS, the projection jitter |
| `settings.rs` | `this.settings` | 40 live values + `setLimits(-4.3, 20)` |
| `debug_view.rs` | `?rview=` + `_renderDebug` | the eleven views and their shader mode numbers |
| `prewarm.rs` | `prewarmMaterials` + `_ensureProbe` | the 20/13-program list, the patch predicate |

The CPU-testable reference the brief asked for is **`schedule::plan(pipeline,
sizing, state) -> FramePlan`**: pass, attachment, width, height, layers, in
frame order, for any tier × capability set × frame state. Everything else feeds
it.

---

## 2. The tier system, which is the file's main structure

`QUALITY_LEVEL = { low: 0, medium: 1, high: 2, ultra: 3 }` is an **enum used as
a table index** and its order is load-bearing in five places: `qLevel >= 1`
gates contact shadows and the ADS depth of field, `qLevel >= 2` gates the bloom
depth and the 4x viewmodel MSAA, and the level is handed to
`cascade::quality_tier` (blocker/PCF tap counts) and to `MaterialPatcher`.
Pinned by `quality::tests::the_tier_order_is_the_source_table_order`.

`boot_line(tier)` reproduces the original's banner exactly:

```
low     [render] WebGL2 · low · 3x1024 CSM · taa:false gtao:false ssr:false mb:false
medium  [render] WebGL2 · medium · 3x2048 CSM · taa:true gtao:true ssr:false mb:true
high    [render] WebGL2 · high · 4x2048 CSM · taa:true gtao:true ssr:true mb:true
ultra   [render] WebGL2 · ultra · 4x2048 CSM · taa:true gtao:true ssr:true mb:true
```

**The `ultra` preset asks for `shadowMapSize: 4096` and never gets one.**
`CascadedShadowMaps`'s constructor is `Math.min(opts.mapSize ?? 2048, 2048)`, so
the request is clamped and `high` and `ultra` print an identical CSM fragment.
That is *why* the brief's boot log reads `4x2048`. Ported as a clamp, not as a
corrected preset value.

Two passes are on from `medium` up and have **no preset flag of their own** —
contact shadows and the ADS depth of field. A reader of `QUALITY_PRESETS` alone
would conclude neither ever runs.

`cascade::MAX_DISTANCE = 140.0` is the CSM constructor's `?? 140` **default**,
not what any tier runs with: the presets supply 60 / 90 / 140 / 200. Anything
calling `cascade::splits` must pass `QualityTier::csm().max_distance`.

---

## 3. The pass-ordering contract

Three kinds of constraint, all encoded in `plan()` and each with a test:

- **Producer/consumer** — 5/6/7 read the G-buffer so 4 precedes them; 8 reads
  all three so it follows; 15/16 read the *composited* colour so 14 precedes
  them (which is why a muzzle flash meters and blooms).
- **History** — GTAO, TAA and the exposure adaptation each hold a **ping-pong
  pair across frames**. SSR is a pseudo-history: it colours its hits from the
  previous frame's resolved image, which with TAA off is `hdr` *itself* — still
  holding last frame at step 7 because step 8 has not overwritten it. **That is
  the entire reason SSR is scheduled before the forward pass**, and why it is
  skipped on frame one.
- **Convention** — the viewmodel renders at 9 into its own MSAA target and
  composites at 14: `viewScene` moves in view space, a camera-matrix velocity
  buffer describes none of it, and TAA was reprojecting those pixels onto stale
  background at ~85% (measured: the optic tube and glove went semi-transparent).
  14 sits *after* 13 so a depth-driven fog pass does not bury the weapon in 40 m
  of aerial perspective, and *before* 15/16 so the flash still meters and blooms.

**The ping-pong index is not reset per frame.** Three step kinds consume a ping
(DOF, each registered pass, the viewmodel composite), and a frame consuming an
odd number hands the next frame the opposite buffer. `plan` takes
`FrameState::ping_index` and returns `FramePlan::next_ping_index` so that is
assertable; `a_step_never_writes_the_target_the_previous_step_reads` pins the
invariant the alternation exists to hold.

**There is no upscale pass.** Steps 1–16 run at `screen`; the composite (or, on
the FXAA path, FXAA) writes the canvas at `display`, and the magnification is a
side effect of that one blit. `crate::upscale` is a different thing — the live
binding's own reduced-resolution present.

### Resolutions

| pass | size | format |
|---|---|---|
| cascades | `map_size²` × `cascades` layers | R32Float |
| prepass | screen | RGBA16F / RG16F / R32F + Depth32F |
| GTAO, contact | screen | RG16F |
| SSR, DOF gather | `max(1, w >> 1)` | RGBA16F |
| motion blur tile | `max(1, ceil(w/16))` | RG16F; output full-res RGBA16F |
| TAA history ×2 | screen | RGBA16F |
| bloom mips | `floor(w/2)` iterated, break after the level that hits ≤2 | RGBA16F |
| exposure | 64 → 16 → 4 → 1, adapt 1×1 ×2 | **RGBA32Float** (`FloatType`) |
| hdr / viewmodel / ping0 / ping1 | screen | RGBA16F (+ Depth32F on the first two) |
| ldr | screen, **FXAA path only** | RGBA8UnormSrgb |

---

## 4. Traps hit, by name

- **Enum-as-table-index** — three of them: `QualityTier` (five numeric
  comparisons), `FramePass` (the discriminant *is* the frame order), and the
  `?rview=` mode integers, where **eight of eleven views share a mode number**
  with another view because the integer selects an unpacking arm in the debug
  shader, not a view. Renumbering them sequentially would change how eight views
  decode.
- **Storage widths** — `hdrTarget` is `HalfFloatType` (RGBA16F) for every
  frame-graph target except `ldr` (`UnsignedByteType`); the exposure chain alone
  is `FloatType` (RGBA32F), because a half-float 1×1 would quantize an EV; the
  full-screen triangle's positions and UVs are `Float32Array`; `probeHdr`
  allocates `Uint16Array` or `Float32Array` depending on the target's own type.
  All the CPU arithmetic is `f64` (JavaScript numbers), narrowed once at a
  uniform.
- **Float grouping is the specification** — five sites transcribed verbatim:
  `shutter * (1/60/dt)` (not `shutter / (60*dt)`), `(j.x * 2) / width` (not
  `j.x * (2/width)`), `vignette + (adsVignette - vignette) * t` (**not**
  `MathUtils.lerp`'s `(1-t)a + tb`), `hue.divideScalar(m)` as a genuine division
  three times over rather than a reciprocal-multiply, and
  `-(ox*cs + oz*sni)` / `-(-ox*sni + oz*cs)` in the room transform.
- **`MathUtils.smoothstep(x, min, max)` reverses GLSL's argument order** — and
  its two tests are *ordered*, so a table index that sums the two predicates
  answers differently from the source when `min > max`. Fixed to nest.
- **`Math.hypot` is V8's max-scaled Kahan sum**, not `sqrt(x²+y²)` — one use, in
  the room-transform yaw recovery.
- **A deferral needs an expiry check** — §7.

---

## 5. Source defects found, pinned not fixed

1. **`viewKeyMax: 2.6` is dead.** `_updateViewRig` computes
   `shaped = REF_DAYLIGHT * min(ref / REF_DAYLIGHT, 1) ** gamma`, so
   `shaped ≤ 4.6` and `keyI ≤ 4.6 × viewKeyScale = 2.53` for **every** input.
   The `Math.min(_, viewKeyMax)` can never bind. One edit to either number away
   from being live, so it is ported and pinned by
   `lighting::tests::the_viewmodel_key_is_shaped_and_capped`.
2. **Motion blur is the one velocity consumer the source forgot to gate.**
   `index.js:1441` is `if (this.motionBlur)` alone, while `MotionBlur.render`
   reads `gbuffer.velocityTexture`. Every other screen-space pass carries
   `&& this.needsPrepass`. Harmless in the source (`needsPrepass` is always
   true) and **not** harmless here, where a device without
   `RenderCapability::GBuffer` drops the prepass. Pinned by
   `pipeline::tests::motion_blur_is_the_one_velocity_consumer_the_source_forgot_to_gate`.
   TAA has the same shape. **I did not "fix" it** — deciding whether a
   G-buffer-less device runs motion blur against a stale/absent velocity buffer
   is a real design call and belongs with whoever ports `motionblur.js`.
3. **`pass.js`'s `uv` attribute is dead.** `Pass` binds `FS_VERT` as every
   pass's vertex shader and `FS_VERT` computes `vUv = position.xy * 0.5 + 0.5`,
   never reading `uv`. The uploaded array is bit-for-bit what the vertex stage
   recomputes — which is the *evidence* it is dead, and is asserted.
4. **`addLight`'s `opts.priority` is recorded and never read.** Ported as a
   field with the name and no consumer.
5. **`Bloom`'s constructor defaults (`threshold 1.0`, `knee 0.6`) are dead** —
   already found and pinned by the `bloom.js` slice
   (`bloom_pyramid::CONSTRUCTOR_DEFAULTS`); `settings.rs` cross-asserts against
   the live values so the two copies cannot fork.

---

## 6. Deliberate divergences from the source

- **`v` is flipped in `FULLSCREEN_WGSL`.** WebGL's framebuffer origin is
  bottom-left, WebGPU's is top-left, so a `vUv` derived identically from clip
  space addresses the mirrored texel. Renderer convention, not algorithm — the
  same class of decision `cascade.rs` records for clip `z` and
  `gbuffer::VELOCITY_TEXTURE_V_SIGN` for velocity. A sibling that transcribes
  `position.xy * 0.5 + 0.5` verbatim gets an upside-down frame.
- **One capability gate the source could not have.** `needsPrepass` is
  unconditionally `true` in the source; here it is
  `gbuffer_attachments_available(profile)`, which is exactly the declared
  `CapabilityDegradation::Drop` for `RenderCapability::GBuffer` (an 8-bit
  velocity target quantizes every useful magnitude to zero, so there is no
  substitute). No *other* capability gates a pass: `Bloom`, `PostProcess` and
  `Shadows` describe what this crate's frame graph offers a frame, and gating
  the frame graph on them would be circular. `HdrTargets` is consulted, but in
  `targets.rs`, where it picks a format via `ldr_substitute` rather than
  dropping a pass.
- **`Math.hypot` is re-implemented locally** rather than reused. `crate::jsmath`
  lives in `apps/shmup` and a module may not depend on an app. See §7.

---

## 7. What I need from the orchestrator or a sibling

### 7.1 The wiring line

```
modules/axiom-gpu-backend/src/lib.rs: mod frame_graph;
```

Unconditional — nothing in `frame_graph` touches `wgpu`, and it compiles on
every arm. Place it after `mod cascade;` (it depends on `cascade`,
`bloom_pyramid::schedule`, `exposure`, `gbuffer` and `hdr_target`, all of which
are already unconditional).

Expect `dead_code` warnings until a binder exists — the same state
`material_shader/` was in before composition, and for the same reason. No
`#[allow]` was added.

### 7.2 Sibling entry points I assumed — **every one is a guess**

Nothing in `frame_graph` `use`s a sibling that does not exist yet. The
assumptions are carried as **data**, in `FramePass::module_path()` /
`module_exists_today()`, and enumerated by
`schedule::tests::ten_pass_slots_name_seven_modules_this_crate_does_not_have_yet`.
Fix that table (one string each) if a sibling chose a different name; nothing
else changes.

| step | assumed module | assumed to own |
|---|---|---|
| 5 GTAO | `crate::gtao` | `gtao.js`: `core` / `temporal` / `blur` passes, RG16F full-res, five targets, `frame % 64` rotation |
| 6 contact | `crate::contact` | `contact.js`: `pass` / `blur`, RG16F full-res |
| 7 SSR | `crate::ssr` | `ssr.js`: `pass` / `blur`, **half-res** RGBA16F |
| 10 TAA | `crate::taa` | `taa.js`: one `pass`, two full-res history targets, `nextJitter()` (Halton) |
| 11 motion blur | `crate::motionblur` | `motionblur.js`: `tilePass` (RG16F, `ceil(w/16)`) + `blurPass` (full-res) |
| 12 DOF | `crate::dof` | `dof.js`: `pre` / `gather` / `combine`, half-res A/B, full-res output |
| 14/17/18/debug | `crate::composite` | `composite.js`: `createComposite(lut)`, `createViewComposite()`, `createFxaa()`, `createDebug()` |

Also referenced by name but **not** assumed to be mine: `crate::lut`
(`createGradeLut('default')` — `composite.rs` will want its `size` for
`uGrade.w`), `crate::env` (`buildFallbackEnvironment`), `crate::probe`
(`RenderProbeScene`), `crate::materialpatch` (`MAX_ROOMS = 10`, which I
duplicated as `rooms::MAX_ROOMS` — collapse onto theirs at integration).

### 7.3 `render/composite.js` (353 lines) looks unowned

My brief lists the concurrent siblings as `gtao`, `ssr`, `taa`, `dof`,
`motionblur`, `contact`, `lut`, `env`, `probe`, `materialpatch`.
`composite.js` is not among them, and it is the second-largest unported file in
`src/render/`. It owns steps 14, 17, 18 and the debug blit — **four of my
twenty plan slots point at it**, and the frame cannot be bound without it.
`post_chain.rs` is *not* a substitute: it is this engine's own bright-pass +
blur + grade chain, not `composite.js`'s AgX + LUT + vignette + CA + grain.
Please confirm whether it was assigned.

### 7.4 `jsmath` belongs in a layer, not an app

`apps/shmup/src/jsmath.rs` consolidated V8's `hypot`/`round`/`sign` after the
port found six divergent copies. The spine now needs one of them too
(`rooms::hypot2`), and a module may not depend on an app, so I transcribed it
again — **a seventh copy, which is exactly the shape the consolidation existed
to stop.**

The structural fix is the one the Layer Law already names for a
broadly-shared primitive: put the JS builtin semantics in the **kernel**, beside
`Meters`/`Radians`/`Ratio`, and have both `apps/shmup` and the spine name it.
That is a layer edit and outside this slice. Until then, `rooms::hypot2` is a
knowing duplicate and says so in its doc comment.

### 7.5 A behavioural constant that depends on boot order

`VIEW_RIG_CHILDREN = 9` (four directional lights added with their targets, plus
one hemisphere) is what `this._viewVisible` compares against — so if any
subsystem added a child to `ctx.viewScene` *before* `RenderSystem.init` ran, the
count would be wrong and the viewmodel would never draw. `RenderSystem.deps =
[]` makes it true in the shipped boot order. Recorded because it is the kind of
assumption a later app-tier slice can silently break.

---

## 8. Tolerances — **all unverified, nothing has been run**

The brief asks for a tolerance flagged unverified. This slice has two kinds of
output and they want different answers:

1. **The sequencing, sizing and tier logic — exact.** Every assertion in
   `schedule.rs`, `targets.rs`, `pipeline.rs`, `quality.rs`, `prewarm.rs` and
   `debug_view.rs` compares integers, enums, booleans or literal-derived `f64`
   with `assert_eq!`. **Expected tolerance: zero.** If any of them misses, the
   port is wrong, not the tolerance.
2. **The lighting arithmetic — `f64`, expected exact against a JS capture.**
   `lighting.rs` and `rooms.rs` transcribe `f64` expressions from `f64` source
   with the source's grouping, so a golden captured by running `index.js` under
   Node should be **bit-equal**, not merely close. Two places would be where a
   miss shows up first, and I would look at them in this order:
   - `view_rig`'s `Math.pow` — V8's `pow` and Rust's `f64::powf` both call the
     platform libm and are not required to agree in the last bit. If a golden
     misses, expect ≤ **1 ULP** (~2.2e-16 relative) on `key.intensity` and on
     the four ratios derived from it.
   - `bounce_fill`'s two normalisations. Grouped as the source groups them, so I
     expect exact; a miss here means a re-associated chain, not rounding.
   The four `assert!(… < 1e-12)` tolerances in `lighting::tests` and the
   `< 1e-215` in `rooms::tests` are **self-consistency slack against
   hand-computed expectations in the test itself**, not measured hardware
   budgets. There is no GPU in this slice, so there is no adapter tolerance to
   derive.

**No golden was captured.** This slice has no shader and no GPU pass; its
oracle would be a Node harness driving `RenderSystem` with a stub `THREE`
renderer, which is a larger harness than the arithmetic it would check. If the
orchestrator wants one, the highest-value target is `_updateBounceFill` +
`_updateViewRig` over a sweep of sun elevations — those two produce every
indirect uniform in the frame.

## 9. What I could not port

- **The binder** (§7, and `mod.rs`'s "Deferral, and what makes it live"). Expiry
  is explicit: when `gtao`, `contact`, `ssr`, `taa`, `motionblur`, `dof` and
  `composite` are all declared in `modules/axiom-gpu-backend/src/lib.rs`, add
  `frame_graph/bind.rs` and declare it there.
- **`probeHdr` / `probeHdrGrid`** (index.js:781-842). Diagnostic GPU readbacks
  that stall the pipeline and are never called from a frame. Their pure parts
  are a rect clamp and a box-bin index; porting them without the readback would
  be a shim with nothing behind it. The half-float decode they need already
  exists as `bloom_pyramid::half_storage`. Revisit when something in this crate
  can read a target back — `draw2d_offscreen` already can, so this is a genuine
  candidate rather than a permanent gap.
- **`dispose()`** — a list of `.dispose()` calls with no arithmetic. It becomes
  `Drop` on whatever owns the real targets, i.e. the binder.
