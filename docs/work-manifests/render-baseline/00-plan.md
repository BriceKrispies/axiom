# The render baseline — shmup's frame becomes the engine's frame

**Decision.** `apps/shmup` (Claude-of-Duty, Three.js r180, landed verbatim at
`102852b7`) is the quality bar every future Axiom app is measured against. The
engine does not adopt Three.js to get there: the passes that produce that frame
are already ported into `modules/axiom-gpu-backend` as Rust + WGSL, verified
against the JS source function by function and pinned against a real adapter
(`deda8e62`). This document is the dependency-ordered work that takes them the
last mile, from "ported and tested" to "on screen in every app".

The premise is not aspirational. `modules/axiom-gpu-backend/src/lib.rs:182` says
it plainly:

> `// The render frame graph — src/render/ of Claude-of-Duty, 18 passes.`

---

## 1. Where this actually stands

`apps/shmup`'s visual stack is four directories, and its own `ARCHITECTURE.md`
hard rule 2 ("never import another subsystem's module") held: `src/render/`
imports exactly three files outside itself, none of them game code.

| shmup | lines | engine counterpart | state |
|---|---|---|---|
| `src/render/` | 6,186 | `modules/axiom-gpu-backend` (18 passes) | ported; **partly wired** |
| `src/materials/` | 5,098 | `material_shader/` (16,888) + `surface_program/` (9,323) | ported |
| `src/sky/` | 3,463 | `crates/axiom-host/src/frame_sky.rs` (a gradient + body + cloud) | **not ported** |
| `src/fx/` | 6,799 | — | **not ported** |

`modules/axiom-gpu-backend` is 79,607 lines. That number answers the question
this program might otherwise stall on: *can a modern renderer live inside the
Branchless Law and the Coverage Law?* It already does. The cost is roughly 4x
the line count of the JS, and almost all of the excess is CPU reference
evaluators and adapter-parity harnesses — the things that make the port
checkable rather than merely written.

### Which passes reach a pixel today

`live_gpu_binding` → `SceneRenderer` is the real browser path.

| pass | module | reaches a pixel |
|---|---|---|
| 2 cascades | `cascade` | yes |
| 4 prepass | `gbuffer` | yes (`ac4d9294`) |
| 5 GTAO | `gtao::pass::GtaoPass` | yes (`ac4d9294`) |
| 6 contact shadows | `contact::pass::ContactPass` | yes (`ac4d9294`) |
| 16 bloom | `bloom_pyramid` | yes (`post_chain`) |
| 17 composite (AgX + LUT) | `agx`, `lut`, `post_chain` | partly — see §2 |
| 7 SSR | `ssr` | **no** |
| 10 TAA | `taa` | **no** |
| 11 motion blur | `motionblur` | **no** |
| 12 ADS depth of field | `dof` | **no** |
| 15 metering | `exposure` | **no** |
| 3 TAA jitter | (frame graph) | **no** |
| 9 / 14 viewmodel + resolve | — | **no contract exists** |
| 13 registered passes | — | **no contract exists** |
| 18 FXAA | — | **not ported** |

Verified mechanically: `crate::taa`, `crate::ssr`, `crate::dof` and
`crate::motionblur` appear nowhere in `scene_renderer`, `live_gpu_binding`,
`offscreen` or `gpu_backend_api`.

---

## 2. The blocker, named by the code that is blocked

`modules/axiom-gpu-backend/src/frame_graph/mod.rs:114`:

> **Deferral, and what makes it live.** The binder — `frame_graph/bind.rs`,
> mapping each `PlannedStep` onto a real pass call — is not written because
> seven of the modules it would call do not exist yet in this crate: `gtao`,
> `contact`, `ssr`, `taa`, `motionblur`, `dof` and `composite`. The moment all
> seven are declared in `src/lib.rs`, this deferral has expired: add
> `frame_graph/bind.rs`, and change `src/lib.rs` to declare it. Nothing else in
> this crate needs to move.

Six of the seven are now declared. **`composite` is the one that is not**, and
it is load-bearing beyond its name: `FramePass::module_path()` routes *four*
passes to it — `ViewmodelComposite` (14), `Composite` (17), `Fxaa` (18) and
`Debug` (19). `vignette`, `chromatic`, `grain` and `fxaa` appear in the crate
**only inside `frame_graph`'s own doc comments**. Nothing implements them.

The deferral's stated precondition is also weaker than the real one. Declaring a
module is not the same as being able to *call* it:

| module | WGSL + CPU reference | adapter parity | runtime pass struct |
|---|---|---|---|
| `gtao` | yes | `gtao/parity.rs` | `gtao/pass.rs` (533) |
| `contact` | yes | `contact/parity.rs` | `contact/pass.rs` (526) |
| `ssr` | yes | `ssr/parity.rs` (1,046) | **none** |
| `dof` | yes | `dof/parity.rs` (925) | **none** |
| `taa` | yes | in-file, `#[cfg(test)]` | **none** |
| `motionblur` | yes | in-file, `#[cfg(test)]` | **none** |

`taa.rs` and `motionblur.rs` do create pipelines — at lines 1747 and 1322, both
below their `#[cfg(test)]` at 880 and 669. Test-only.

So the real precondition for `bind.rs` is: one new module (`composite`) and four
new `pass.rs` files, each modelled on the two that already exist.

---

## 3. The waves

Ordered by what unblocks what. Each wave names what it touches.

### Wave A — `composite`, the seventh module

**Touches:** a new `modules/axiom-gpu-backend/src/composite/` (`wgsl.rs`,
`reference.rs`, `parity.rs`, `pass.rs`), one `mod composite;` in `lib.rs`.

Ports `apps/shmup/src/render/composite.js` (353 lines), which is four GLSL
chunks and four factories:

- `COMPOSITE` (:15) — exposure, bloom add, linear-light chromatic aberration and
  cos⁴ vignette *before* the tone map; AgX; sRGB encode; then the LUT, grain and
  dither in **display** space. `agx.rs` and `lut.rs` already own the middle;
  this pass owns the lens tail on either side of them, and the ordering is the
  substance — the source's own note is that a vignette applied to code values
  makes display white unreachable anywhere but frame centre.
- `FXAA` (:168) — pass 18, live only when TAA is off.
- `VIEW_COMPOSITE` (:237) — premultiplied resolve of the viewmodel over the
  world. **Deferred to Wave E**; it has nothing to resolve until a viewmodel
  contract exists.
- `DEBUG` (:298) — the `?rview=` arm. `frame_graph/debug_view.rs` (352) already
  plans it; this is its execution half.

The existing `post_chain.rs` is not replaced. It is the LDR/no-tonemap arm and
stays byte-identical; `composite` is the HDR arm's tail.

### Wave B — the four missing pass structs

**Touches:** `ssr/pass.rs`, `dof/pass.rs`, `taa/pass.rs`, `motionblur/pass.rs`,
and the `mod pass;` line in each parent.

Mechanical against two exemplars (`gtao/pass.rs`, `contact/pass.rs`): pipelines,
bind-group layouts, target allocation, resize, and the ping-pong history where
the pass has one. The pass semantics are already fixed by the WGSL and its CPU
reference, so this wave adds no new judgment — the traps are the ones
`ac4d9294` already paid for once:

- a render pass may not read and write one texture, so a history is a distinct
  target, never the accumulator;
- the history must stay un-blurred, or every frame re-blurs an already-blurred
  image and the effect creeps outward into a grey wash;
- WebGPU's default `maxBindGroups` is 4, so 0..3 is the entire budget and a
  fifth group refuses to create a pipeline on a conforming device.

SSR carries one ordering constraint the others do not: it reads the *previous*
frame's resolved colour, so it must sit before the forward pass, not after, and
it is skipped on `FrameState::first_frame`.

### Wave C — per-object motion vectors

**Touches:** the frame packet's instance stream in `crates/axiom-host`, whatever
produces it, and `scene_renderer.rs:1922`.

This is a prerequisite for Wave D, not a polish item. `gbuffer`'s shader already
reads `world` and `prev_world` correctly. The *feed* passes the same matrix
twice, and `scene_renderer.rs:1924` says why:

> `prev_world` is the SAME matrix: per-instance motion history does not exist
> yet, so an object's own movement contributes no velocity and only the camera's
> does. Stated rather than hidden — a temporal pass fed a zero object velocity
> smears moving geometry, and that is the shape of the defect to look for.

TAA and motion blur are both fed by that lane. Landing either on camera-only
velocity ships the defect the comment describes. The fix is a previous-transform
lane in the frame contract — the lowest layer where "where was this instance
last frame" is knowable — not a cache inside the backend.

TAA jitter (pass 3) lands here too: `pack_gbuffer_uniform` already keeps the
rasterised and unjittered transforms as separate lanes precisely so the jitter
can be applied to one and not the other.

### Wave D — `frame_graph/bind.rs`

**Touches:** one new file, one `mod bind;`, and the hand-wired pass sequence in
`scene_renderer.rs` that it replaces.

The frame graph is already a pure, CPU-testable sequencer: `schedule::plan`
takes a `FramePipeline`, a `ScreenSizing` and a `FrameState` and returns the
ordered `PlannedStep` list — pass, attachment, resolution — for all four quality
tiers, and it reproduces the original's boot banner exactly
(`[render] WebGL2 · ultra · 4x2048 CSM · taa:true gtao:true ssr:true mb:true`).
`bind.rs` is the half that turns a `PlannedStep` into a wgpu call.

New `RenderCapability` bits are needed for the tier resolve to be honest about
what a device can do (bit 14 `GBuffer` is the last one taken; 15 is free).
Whether SSR/TAA/MB/DOF get one bit or four is a Wave D decision, and it should
follow how `hdr_target` grants `HdrTargets` at bind from what the adapter
actually reported, with a declared degradation rather than a silent one.

After this wave, `scene_renderer` stops owning pass order. That is the point:
the ordering is data, testable without a GPU, and "does SSR run on medium?" is
answered by a function instead of by a browser.

### Wave E — the viewmodel arm

**Touches:** a new frame-contract concept in `crates/axiom-host`, `bind.rs`,
`composite`'s `VIEW_COMPOSITE`.

`viewmodel`, `view_scene` and `viewScene` currently appear **zero times** in
`axiom-host`, `scene_renderer` and `live_gpu_binding`. Passes 9 and 14 have
nothing to bind to.

This is a real contract addition, not a wiring job: a second scene with its own
camera, its own MSAA colour+depth target, and a premultiplied resolve after the
registered passes. The source's reason for the separation is worth carrying
across verbatim, because it is a bug report rather than a preference —
everything in `viewScene` moves in *view* space, a velocity buffer built from
camera view-projection matrices describes none of it, so those pixels emitted
zero motion and TAA blended them ~85% onto stale history. The optic tube and the
glove went semi-transparent with balcony rails legible straight through them.

Pass 13 (`Registered`, "whatever fx/ui/sky registered") has no contract either,
and belongs in this wave for the same reason.

Deferrable: an engine that never draws a first-person weapon loses nothing by
planning passes 9/14 and binding them to nothing. It should be deferred
*explicitly*, with the plan still naming them, rather than removed from the
graph.

### Wave F — sky

**Touches:** a new module, and `frame_sky.rs`'s relationship to it.

`apps/shmup/src/sky/` is 3,463 lines: a physical atmosphere with four LUT bakes,
a dome, clouds, stars, celestial bodies and a volumetric march. The engine's
`FrameSky` is a vertical gradient with an optional body and a cloud layer,
evaluated backend-neutrally — a different thing, well argued for what it is.

The two must not be conflated. `FrameSky`'s existing rationale (a sky that
degrades *with* its capability, by declaration, so a backend that dropped
`RenderCapability::Sky` cannot silently keep drawing cards) is the constraint
any port has to satisfy. And shmup's sky is nine GPU programs — which its own
`fidelity.js` names as the single largest item `lean` cuts.

### Wave G — fx

**Touches:** a new module.

`apps/shmup/src/fx/` is 6,799 lines: GPU particles, muzzle flash, tracers,
impacts, decals, shells, explosions, haze, ambience. Nothing in the engine
corresponds. Largest wave, least blocked by anything above it, and the one most
likely to want a contract of its own rather than to hang off the frame graph's
`Registered` slot.

---

## 4. What "baseline" has to mean, precisely

**It is not shmup's default frame.** `apps/shmup/src/core/fidelity.js` makes
`lean` the default, and lean drops the post chain (SSR, GTAO, contact, TAA,
motion blur, DOF, bloom, FXAA), the sky, the whole fx system, fifteen of
nineteen library surfaces, and the per-pixel material ornament. The frame this
program targets is `?fidelity=full`.

That axis exists for a measured reason, and the engine inherits the same
arithmetic the moment it has the same number of programs:

> cold boot ≈ (number of lit programs) × (~100 KB of translated HLSL each)

Measured on shmup, cold, settled: **lean 14,835 ms at 43 programs; full ~26,000
ms at ~101**. Three separate attempts to shrink the second factor — fewer
lights, fewer cascades, swapping the material class to Lambert — bought 4%, 13%
and "renders nothing".

So the baseline is two numbers, not one, and the second is not optional:

1. **the frame** — the `?fidelity=full` pass list at a stated quality tier;
2. **a program budget** — the count of programs the frame actually draws with,
   because that count *is* the cold boot.

The engine is better placed to hold both than shmup is. `Surface::digest()` is
structural and excludes parameter values, so retuning cannot force a recompile
and two independently-authored identical materials collapse to one pipeline; and
everything compiles inside a `PreparationTask`, so a cache miss renders a
fallback and *reports* rather than compiling mid-frame. That is
`core/prewarm.js`'s entire problem solved by construction. Neither property
survives if the baseline is stated as a look and not as a budget.

---

## 5. How the baseline is enforced, not just written down

A quality bar that lives in prose decays. Two mechanisms, both of which the repo
already has the parts for:

- **A pixel gate.** shmup's `tools/capture.mjs` + `tools/imagediff.mjs` is the
  existing shape; `axiom-shot` and the Playwright controller are the engine's. A
  reference frame per tier, diffed on every push that touches the frame graph.
- **A program-count assertion.** `frame_graph::quality::boot_line` already
  reproduces the source's boot banner exactly for all four tiers. Extending that
  to the drawn-program count makes the second half of the baseline mechanical.

Neither belongs to a wave above; both should land with Wave D, the first point
at which there is a full frame to gate.

---

## 6. What this program deliberately does not do

- **It does not adopt Three.js.** The alternative — lifting
  `src/{render,sky,materials,fx}` into a `packages/` tier governed like `apps/`
  and `tools/` — gets a good frame sooner and costs the ~80k lines already
  ported and verified, plus the Rust spine's claim to be the rendering story.
  Rejected on that trade, not on principle.
- **It does not touch `apps/shmup`.** shmup stays the running reference and the
  source of truth for every pixel comparison. It is not an Axiom app (no
  `app.toml`, no workspace membership) and does not need to become one.
- **It does not resume the app port.** `cf56a515`'s 125,600-line Rust
  transcription of the *game* was deleted at `78403267` and stays deleted. Only
  the engine-side half — `modules/axiom-gpu-backend` — is live work.

---

## 7. Provenance

Every claim above is from the tree at `4d72e596`:

| claim | where |
|---|---|
| the 18 passes are `src/render/` | `modules/axiom-gpu-backend/src/lib.rs:182` |
| the binder deferral and its precondition | `modules/axiom-gpu-backend/src/frame_graph/mod.rs:114-123` |
| four passes route to a `composite` that does not exist | `frame_graph/schedule.rs`, `FramePass::module_path()` |
| SSR/TAA/MB/DOF have no runtime pass struct | `ssr/`, `dof/` contain only `parity.rs`; `taa.rs:1747` and `motionblur.rs:1322` are below their `#[cfg(test)]` |
| GTAO/contact/prepass/LUT reach pixels | `ac4d9294`, `scene_renderer.rs:839-1490` |
| `prev_world` is the same matrix | `scene_renderer.rs:1922-1934` |
| no viewmodel concept exists | zero hits for `viewmodel`/`view_scene` in `axiom-host` + the two renderers |
| lean is the default and what it drops | `apps/shmup/src/core/fidelity.js` |
| the cold-boot arithmetic and the three failed attempts | same file, header |
| render imports three files outside itself | `apps/shmup/src/render/*.js` |
