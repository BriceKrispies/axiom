# shmup → Axiom: the promotion manifest

`apps/axiom-shmup` is 128,460 lines of `src/`. Its entire contact with the
engine is about 120 symbol references, and almost all of them are `Mat4`,
`Quat`, `Vec3`, `Seconds` and a handful of `axiom_host` frame types:

```
 38 axiom_math::Mat      18 axiom::prelude          6 axiom_host::FrameSky
 18 axiom_math::Quat     11 axiom_kernel::Seconds   5 axiom_surface::MaterialParams
  9 axiom_math::Vec       2 axiom_windowing::WindowingApi
```

The app is not built on Axiom. It runs its own engine — its own subsystem
registry, fixed-step loop, event bus, collision world, character controller,
audio DSP, geometry kit, noise library, input model, atmosphere and particle
system — and hands Axiom a draw list at the end.

This manifest is the plan to fix that: promote every capability in the app that
is genuinely engine, at the lowest correct layer, and delete every duplicate of
something the engine already ships.

## Why this was not already done

`docs/work-manifests/shmup-port/00-manifest.md` chose it deliberately:

> **Engine capability** → `crates/` and `modules/`. Branchless Law, Coverage
> Law, Layer Law, Module Law all apply. Expensive, and correct.
> **The game itself** → `apps/`. Ports at porting speed.
> […] It means only genuine engine primitives pay the gate tax.

That was the right call *while porting*. The port is landed. What is left is a
composition leaf carrying ~35,000 lines of reusable engine, which is exactly the
"useful code in the wrong place" the placement procedure exists to prevent.

The cost of this manifest is that gate tax, paid in full: everything below lands
**branchless** (`engine_no_branching`, baseline 0) and at **100% region/line/
function coverage**. There is no reduced-rate version of it.

## The ceiling, stated honestly

| tier | what | lines |
|---|---|---|
| 1 | capabilities the engine does not have | ~21,800 |
| 2 | duplicates of capability the engine already ships | ~13,500 |
| 3 | this game's content — **stays in the app** | ~93,000 |

Tier 3 is not a backlog. Weapon models, soldier variants, the city layout, the
19 material recipes, the audio recipes, the HUD widgets, `config.rs` and all of
`scene/` are gameplay concepts, and the Module Law puts gameplay concepts in the
composition leaf. Promoting them would be the junk-drawer failure in reverse.

## Tier 1 — capabilities the engine does not have

| capability | app source | lines | placement | forcing law |
|---|---|---|---|---|
| Triangle-soup BVH + closest/any-hit ray, AABB range, capsule overlap/sweep | `physics/{bvh,math,penetration}.rs` | 2,502 | module `physics` | `axiom-physics` already owns `physics-queries`, `physics-capsule-contacts` and `physics-shape-casts`; a mesh collider is the missing shape in that same facade, not a second module |
| Capsule character controller — collide-and-slide, step-up, ground/slope, crouch | `physics/character.rs` | 1,056 | module `fp-controller` | it already introduces `first-person-walk-step`; a walk step that cannot collide is the incomplete half of one capability |
| Ragdoll / articulated constraint solve | `physics/{ragdoll,rigidbody}.rs` | 2,262 | module `physics` (joints) + module `physical-animation` (the ragdoll) | Module Law #2 — a joint is a physics primitive; pose-from-physics is what `physical-animation` is for |
| Audio DSP, IR synthesis, node graph, mixer buses, spatialisation | `audio/{dsp,ir,graph,mixer,spatial}.rs` | 2,842 | module `audio` | `axiom-audio` is scheduling bookkeeping only today — no synthesis, no biquad, no convolution. Its own doc says "not a synthesizer"; that sentence is the gap |
| Physical atmosphere, 3 LUT bakes, ephemeris, cloud decks, night sky, volumetrics | `sky/*` minus `system.rs` | 3,317 | **new module `axiom-sky`** | no engine equivalent; `axiom_host::FrameSky` is a *frame contract* with no model behind it, and `gpu-backend/src/env.rs` already cites `apps/shmup/src/sky/atmosphere.rs` as the definition it approximates |
| Walkability grid bake, A-star, string pull, cover scoring | `ai/nav.rs` | 883 | **new module `axiom-navigation`** | `modules/axiom-agent/ARCHITECTURE.md` explicitly refuses pathfinding as "a capability that belongs elsewhere" — this is that elsewhere |
| Particle pools, decal projection, tracers, shells, atlas packing, light pool | `fx/{particles,decals,tracers,shells,atlas,lights,haze,util}.rs` | 3,144 | **new module `axiom-fx`** | `host` has `KIND_PARTICLE_QUAD` (a draw command) and `decal_budget` (a quality knob); nothing simulates either |
| Layered pose blending, IK, attachment anchors | `ai/{animator,rig,clips}.rs` | 2,474 | module `animation` | it already introduces `pose-blending` and `animation-clip-sampling`; layers, IK and anchors are the rest of that capability |
| Skinned character builder (parameterised body/kit geometry + skin weights) | `ai/{geo,parts}.rs` | 3,312 | module `figure` | it already introduces `articulated-figure-definition` and `figure-box-posing`; a skinned builder is the non-box case. Soldier *variants* stay in the app |

## Tier 2 — duplicates of capability the engine already ships

| duplicate | app source | lines | existing home |
|---|---|---|---|
| Hard-surface geometry kit — earcut, extrude, lathe, rounded box, torus, sphere, merge, assembly | `weapons/geometry/**` | 3,229 | `crates/axiom-mesh-ops` has **every one**: `triangulate_profile`, `extrude`, `revolve`, `rounded_box`, `torus`, `uv_sphere`, and `axiom-mesh::combine`/`weld` |
| Building-kit geometry primitives — chamfer box, quad, `trs`, patch | `world/kit/primitives.rs` | 828 | `crates/axiom-mesh-ops` |
| Geometry accumulator | `world/{geo,accum}.rs` | 701 | `axiom-mesh::MeshStreams` |
| Four separate noise families | `{materials,fx,sky,world}/noise.rs` | 1,401 | `crates/axiom-noise` (726 lines: value noise + fbm only) |
| Input snapshot, edge queries, look curve, dead zone, action table | `input.rs`, `touch.rs` core | ~1,200 | `modules/axiom-input` (`InputState`, `ActionId`, `swipe_synth`) |
| Subsystem registry, fixed-step accumulator loop, event bus | `registry.rs`, `engine.rs`, `events.rs` | 872 | `axiom-runtime` (`RuntimeScheduler`, `RuntimeSystem`, `RuntimeEventQueue`) + `axiom-frame` (`FrameAccumulator`, `StepBudget`) + `axiom-ecs::EventBuffer` |
| Procedural texture forge — bake pipeline, curvature masks, quantized upload, WGSL emit | `materials/{bake,masks,upload,gpu_bake,wgsl}` | 3,257 | `crates/axiom-proc-texture` + `crates/axiom-surface` + `modules/axiom-gpu-backend`. Already partly planned — see `shmup-port/01-engine-gaps.md` G15 and `08-material-shader-plan.md` |
| HUD draw-list / widget vocabulary | `ui/util.rs` + each widget's draw half | ~2,000 | `crates/axiom-interface` (`draw_list`, `panel`, `layout_rect`, `ui_geometry`) |

### The golden conflict, and how it is resolved

The app's copies are not arbitrary: they are pinned by goldens captured from
real `three@0.180` running under Node. `axiom-mesh-ops::revolve` and the app's
`lathe_z` compute the same surface in a **different vertex order**;
`triangulate_profile` and `earcut` emit the same triangles in a **different
order**.

**The engine operator is extended to express the source's shape**, and the
existing goldens keep passing bit-for-bit.

The extension lands as a **named convention**, never a `three_js` flag. Seam
duplication, cap winding, ring order and UV origin are genuine parameters of a
lathe or an extrude — engines differ on them for real reasons, and
`revolve.rs`'s own doc already argues one of those choices at length. A
parameter named after another library would be the ceremonial variant the Layer
Law bans; a parameter named after the geometric decision it makes is the
operator being honest about a choice it was making implicitly.

Where an operator's difference turns out to be *more* than a convention
(different tessellation, different UV derivation), that is recorded here as a
finding rather than papered over with a compatibility mode.

## Step 0 — the precision collision, and how it is resolved

Found while starting step 1, and it gates everything else.

**The app is `f64`; the engine was `f32`.** 8,635 `f64` occurrences against 3,365
`f32`, with `f64` dominant in *every* subsystem — because this is a port of
JavaScript, where `number` **is** `f64`, and every golden was captured from Node.
`crates/axiom-math/src/scalar.rs` stated the opposite outright — *"Axiom
standardises on IEEE-754 `f32` as the engine scalar"* — and the layer held zero
`f64`. The physics BVH, the character controller, the atmosphere and spatial
audio all need a double-precision *geometry* vocabulary the engine did not have.

The resolution is not an amendment to the policy. It is the policy finally
saying what it already meant, because **the engine had already settled this and
measured it**. `crates/axiom-surface/src/material_params.rs`, over all 256 byte
inputs:

| how the curve is computed | values differing | worst gap |
|---|---|---|
| f64 throughout | 254 / 256 | 1.08e-11 |
| f64, then narrowed to the f32 uniform | **0 / 256** | **0** |
| natively in f32 | 175 / 256 | 1.79e-7 |

An `f32`-native transcription does not merely lose digits: it **introduces** a
disagreement the reference does not have, on 175 of 256 inputs. So:

> **`f32` is the *interchange* scalar** — what crosses a facade, fills a vertex
> buffer, reaches a uniform, stores a transform. **Evaluate at the precision the
> domain requires; narrow once, at the boundary.**

That rule is now written into `scalar.rs` where the misleading sentence was, and
given a vocabulary rather than left to loose `f64` triples:

- **kernel** — `BinaryWriter::write_f64` / `BinaryReader::read_f64` and
  `impl Reflect for f64`. Serialization is a kernel responsibility, and a `f64`
  that round-tripped through `f32` would lose exactly the digits these domains
  are carried for.
- **math** — `DVec3`, the double-precision sibling of `Vec3`: the same
  operations, the same never-panics discipline, plus `floor`/`fract`
  (GLSL's `fract`, not Rust's sign-keeping one) because splitting a position
  into lattice cell and offset is what the precision is *for*.
  `DVec3::to_single` / `::from_single` are the one **named** narrowing boundary,
  so "compute in f64, narrow once" is a symbol you can search for rather than an
  `as f32` scattered across call sites.
- **math** — `impl ApproxEq for f64` and `Epsilon::DEFAULT_DOUBLE` (`1e-12`).
  `Epsilon::DEFAULT` is `1e-6`, sized for `f32`; applied to an `f64` it would
  call two values equal that disagree in the sixth digit. The `Epsilon` *type* is
  shared — a tolerance is a tolerance — but not its default.

`DVec3` lands at **100% regions / functions / lines** and adds **zero** dylint
findings (the branchless count stays at its two pre-existing sites, neither in
this code).

**What was deliberately not built.** `DVec2`, `DAabb`, `DRay`, `DTriangle`,
`DSegment`, `DCapsule`, `DSphere`, `DObb` — the rest of the family the physics
kernel will need. Each lands with the promotion that consumes it. Building the
whole family now would be surface with no caller, which is the ceremonial-export
failure the Layer Law bans, in a slightly different hat.

**A pre-existing red, recorded so it is not mistaken for this work.**
`cargo test -p axiom-burnt-rubber --lib` fails 19 gameplay-tuning tests
(`course::validation`, `sim::traffic`, `tuning`, `ghost`, `script`) on a
**pristine** tree — verified by stashing this change and re-running. Because
`scripts/coverage.sh` runs the whole workspace's tests, that red blocks the
coverage gate for everyone, not just this program. It is app-tier and outside
the coverage scope line, but it has to be fixed before the gate can certify
anything.

## Step 1a — the positional value basis, and what a promotion looks like

`world/noise.rs` (258 lines) is now `axiom_noise::{hash_01, value_noise_01,
value_fbm_01}` + `UnitNoise`, at **100% regions / functions / lines** across the
layer and **zero** new dylint findings. The app keeps 107 lines: the four-scalar
call shape its ~50 call sites read best in, and — the part that is genuinely
content — **this world's constants**, the per-axis drift `(2.03, 2.01, 1.97)`
and the `0.5` gain that are the identity of its surface variation.

Three things this step establishes as the pattern for the rest:

**The goldens move with the algorithm.** The Node-captured values that pinned
the app's copy now pin the layer's, in
`crates/axiom-noise/tests/positional_basis_golden.rs`, asserted with
`assert_eq!` rather than a tolerance — the basis is exact 32-bit integer
arithmetic and a divide by a power of two, so slack would be unearned. The
app keeps a small binding test with five of the same values, which catches the
one class the layer's goldens cannot: a binding that hands the layer the wrong
constants or the axes in the wrong order.

**The layer had to say why it now holds two bases.** `axiom-noise` was one
gradient basis; it is now two, and they are not interchangeable — different
lattice key, different fade, different range, different precision, and one is
seeded while the other is deliberately not. That table is in the layer's own
docs, because a future agent choosing between them needs the *reason*: a seeded
basis reshuffles when an unrelated subsystem takes one more draw from the shared
stream, and a positional one cannot. That durability property is why both exist.

**The rulebook caught a real mistake, not a false positive.** `round_ties_up`
was briefly exported on the argument that a shader reimplementing the basis
would need to round identically. True, and not a caller —
`engine_no_unitless_float_public_api` flagged it, and it was right: "someone
might need it" is how a layer grows an API nobody calls. It is private now.
Expect this; the gate is a design reviewer, not an obstacle.

### Two findings that will bite every later step

**The app's goldens do not link on the default toolchain.**
`cargo test -p axiom-shmup` fails to link the `cdylib` with
`ld.exe: error: export ordinal too large: 90051` — the gnu linker's 65535
export-ordinal limit, exceeded by ~37%. **Nothing** in `apps/axiom-shmup/tests/`
can run there, which means the port's entire verification suite is unavailable
on the default toolchain. It runs clean on
`RUSTUP_TOOLCHAIN=nightly-x86_64-pc-windows-msvc` (all 153 goldens pass), and
that is how every step of this program must verify itself. Pre-existing —
measured at 90052 before this change and 90051 after, because the app's module
shrank.

**One app lib test is red on a pristine tree.**
`scene::wiring::weapons::tests::holding_the_trigger_drains_the_magazine_and_kicks_the_camera`
fails with and without this change, on both toolchains.

## Order of work

Dependency-ordered. Nothing here is ordered by preference.

**Wave 1 — pure math, no render coupling, natively golden-testable.**

1. `axiom-noise` absorbs the four noise families. Smallest, and it unblocks the
   materials forge, `axiom-sky`, `axiom-fx` and the world kit.
2. `axiom-physics` gains the static triangle-mesh collider (BVH + queries).
   Five app subsystems — character, mantle, nav, fx impacts, audio occlusion —
   currently share one app-local trait seam standing in for exactly this.
3. `axiom-fp-controller` gains the capsule controller, on top of (2).
4. `axiom-audio` gains DSP, IR, the node graph, the mixer and spatialisation.

**Wave 2 — geometry consolidation.**

5. `axiom-mesh-ops` gains the conventions the port's kit needs; `weapons/geometry`,
   `world/kit/primitives` and `world/{geo,accum}` collapse onto it and `axiom-mesh`.

**Wave 3 — new modules for absent domains.**

6. `axiom-navigation`.
7. `axiom-sky` — depends on (1).
8. `axiom-fx` — depends on (1) and (2).

**Wave 4 — the loop.**

9. The app stops running its own registry, frame loop and event bus and runs on
   `axiom-runtime`'s scheduler. Fewest lines removed of any step here, and the
   most important: it is what makes this an Axiom app rather than a game sharing
   an address space with one.

**Wave 5 — the rest.**

10. `axiom-animation` layers/IK/anchors; `axiom-figure` skinned builder.
11. The materials forge into `proc-texture`/`surface`/`gpu-backend`.
12. `axiom-interface` HUD vocabulary.
13. `axiom-input` absorbs the input model.

## What each step must produce

No step is finished until all of it holds:

- the capability lives at the placement named above, with the argument written
  into its `layer.toml` / `module.toml` and not only into this file;
- it is branchless — `bash scripts/dylint-gate.sh` finds no
  `engine_no_branching` finding in the new code;
- it is at 100% regions/lines/functions — `bash scripts/coverage.sh`;
- `cargo xtask check-architecture` passes;
- the app's copy is **deleted**, not left beside the promotion. A promotion that
  leaves the duplicate in place has added a second definition and fixed nothing;
- the port's goldens still pass, unchanged.

## Progress

| step | state |
|---|---|
| 0. the precision floor — kernel `f64` binary/reflect, math `DVec3`, the amended scalar policy | **done** |
| 0b. `jsmath.rs` → `axiom-math` (hypot / rounding / sign / divisor guard) | **done** |
| 1a. `world/noise.rs` → `axiom-noise` (positional value basis) | **done** |
| 1b. `fx/noise.rs` → `axiom-noise` (Perlin permutation + Worley) | **done** |
| 1c. `sky/noise.rs` → `axiom-noise` | **blocked** — `atmosphere::Vec3` ripples across 7 sky files; lands with step 7 |
| 1d. `materials/noise.rs` → `axiom-noise` | **blocked** — another session holds live WIP in `materials/wgsl/`; lands with step 11 |
| 0c. `physics/math.rs` → `axiom-math` (`DAabb`/`DTriangle`/`DSegment`/`DClosestPair`) | **done** |
| 2. mesh collider → `axiom-physics` | not started |
| 3. capsule controller → `axiom-fp-controller` | not started |
| 4. audio DSP → `axiom-audio` | not started |
| 5. geometry → `axiom-mesh-ops` | not started |
| 6. `axiom-navigation` | not started |
| 7. `axiom-sky` | not started |
| 8. `axiom-fx` | not started |
| 9. the loop → `axiom-runtime` | not started |
| 10. animation / figure | not started |
| 11. materials forge | not started |
| 12. `axiom-interface` HUD | not started |
| 13. `axiom-input` | not started |
