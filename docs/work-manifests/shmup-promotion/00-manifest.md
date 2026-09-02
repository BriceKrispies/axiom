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

## Where to pick this up

Everything below the "Progress" table is the state a fresh session needs.

### Verify like this, or you will verify nothing

```sh
# The app CANNOT LINK on the default gnu toolchain. Every command touching it
# needs the MSVC one, or the port's 205 goldens simply do not run.
RUSTUP_TOOLCHAIN=nightly-x86_64-pc-windows-msvc cargo test -p axiom-shmup --lib
RUSTUP_TOOLCHAIN=nightly-x86_64-pc-windows-msvc cargo test -p axiom-shmup --test core_port
# ... and physics_port, player_port, weapons_port, weapons_geometry_port,
#     weapons_clips_port, weapons_mathx_port, materials_noise_port,
#     render_probe_port, player_feel

# Per-crate coverage, which is what a promotion must hit. The WORKSPACE gate
# cannot run — see the blockers below — so measure the crate you touched.
RUSTUP_TOOLCHAIN=nightly-x86_64-pc-windows-msvc cargo llvm-cov clean --workspace
RUSTUP_TOOLCHAIN=nightly-x86_64-pc-windows-msvc cargo llvm-cov --no-cfg-coverage \
    -p axiom-noise -p axiom-math --summary-only

cargo xtask check-architecture
cargo dylint --all -- --all-targets      # count findings; must not rise
```

**`cargo llvm-cov clean --workspace` before every coverage run.** Stale profile
data from deleted tests reports a file at 80% that is actually at 100%, and the
mistake looks exactly like a real coverage hole.

### Blockers, none of them this program's to fix

- **The workspace coverage gate cannot complete.** `scripts/coverage.sh` runs
  every test in the workspace to collect instrumentation, and two crates are red
  on a pristine tree: `axiom-gpu-backend` (~60 failures, all one cause —
  `Binding size 80 of Buffer ... is less than minimum 96`, a uniform-layout
  mismatch from another session's in-flight WGSL work) and `axiom-end-zone`
  (6 harnesses: attempt_loop, chalk, controls, determinism, runback, targeting).
  Until those are green, per-crate `cargo llvm-cov -p <crate>` is the honest
  substitute — it measures exactly the same thing for the crate being promoted.
- **`apps/axiom-shmup` does not link on gnu.** `ld.exe: error: export ordinal
  too large` — the cdylib exceeds the 65535 export-ordinal limit by ~37%.
- **One app test is red on a pristine tree.**
  `scene::wiring::weapons::tests::holding_the_trigger_drains_the_magazine_and_kicks_the_camera`.

### The next step, and what it costs

**Step 2, the triangle-mesh collider, is the flagship gap and the largest single
item in this manifest.** `axiom-physics` already offers `raycast`,
`overlap_capsule` and `capsule_cast` against spheres, boxes, capsules, planes and
heightfields — everything except a **mesh**, which is the one shape a level is
made of. Its foundation is now built: `DAabb`, `DTriangle`, `DSegment` and
`DClosestPair` are landed, and `physics/bvh.rs` needs exactly three operations
from them (`ray_entry`, `ray_hit`, `closest_to_triangle`), all present.

What remains is `apps/axiom-shmup/src/physics/bvh.rs` — 1,285 lines of
binned-SAH construction and five queries — rewritten branchless at 100%
coverage. Budget it as more work than every step landed so far, combined, and do
not start it in a session that cannot finish it: the goldens pin exact tree
shape, node indices and node bounds, so a half-converted builder is worth less
than none. Specific shapes to plan for:

- `build_nodes` drives an explicit LIFO stack (`while let Some(..) = stack.pop()`).
  A `fold` over a worklist is the branchless form. **The push order (left child,
  then right) is load-bearing** — it decides which node index a split's children
  land at, and the goldens pin that.
- The in-place Hoare partition (`while i <= j`) is the awkward one.
- Recursion is *not* banned by the Branchless Law; `cond.then(|| ...)` is a legal
  terminating guard, which is how a recursive builder stays branchless.
- `Surface` (the game's 12-entry taxonomy) must become an opaque material index
  at the engine boundary. `axiom-physics` already has `PhysicsMaterial`.
- The facade narrows to `f32`: `PhysicsApi` speaks `Vec3`/`Meters`, the BVH
  evaluates in `f64`, and `DVec3::to_single` is the named narrowing point.

### Cheaper steps, if the appetite is for breadth

- **Step 4, audio DSP** (`audio/{dsp,ir,graph,mixer,spatial}`, 2,842 lines). Pure
  signal maths, no geometry, natively testable. One design question to settle
  first: Module Law #8 gives `axiom-audio` a single facade, so the node-graph
  *vocabulary* either becomes handles on `AudioApi` (the `axiom-physics` shape)
  or moves to a layer both the app and the module can name (the precedent
  `apps/axiom-shmup/app.toml` cites for `ProceduralBakeRequest`).
- **Step 7, `crates/axiom-sky`.** A **layer**, not a module, and the argument is
  forced: `modules/axiom-gpu-backend/src/env.rs` already cites
  `apps/shmup/src/sky/atmosphere.rs` as the model it approximates, and a module
  may not depend on another module. Blocked behind a real cost, though —
  `atmosphere::Vec3` has 17 methods and 7 consumer files inside `sky/`, so the
  promotion drags a rename across all of them. `DVec3` would need `splat`,
  componentwise `div`/`max`/`exp`, `add_scalar` and `mix` first.
- **Step 6, `axiom-navigation`** (`ai/nav.rs`, 883 lines) is smaller but more
  entangled than it looks: it needs `DAabb` (landed), a physics-probe seam, and
  the game's collision-layer masks.

### Two lessons the landed steps paid for

**The gate is a design reviewer, not an obstacle.** Three separate findings in
this session were correct and led to better code: an export justified by a
hypothetical caller, naked float parameters where typed knobs already existed,
and a `&mut` generator threaded through an engine API where taking the finished
data made the type pure. Read a finding as a question about the design before
reaching for the baseline.

**A golden suite hides latent defects it does not exercise.** `world/noise.rs`
carried a `Math.round` written as `(v + 0.5).floor()`, documented as exact "for
every finite v", which `jsmath` had already shown false at `0.49999999999999994`.
Two implementations of one primitive, one correct, and the wrong one was the one
first promoted into the engine. Its goldens never moved, because the pathological
input never arose in them. When consolidating a duplicated primitive, diff the
implementations against each other, not only against the goldens.

## The datafication turn

The promotion program above is line-by-line lifting. It has a ceiling: executed
perfectly it leaves the app at ~93,000 lines, because it classifies content as
"stays in the app". A five-agent audit of the app, run with `ax shape`, says the
ceiling is wrong — most of that content is **data written in Rust**, and the app
has no vocabulary to say it any other way.

Measured, tests excluded: **77,070 code lines**, 0.56 literals/line overall,
0.062 branches/line. `world/props` runs at 1.53 literals per line.

### What five independent audits converged on

Five subsystems, five agents, three mesh gaps named by all of them. **All three
are landed** (`c3fd78f5`):

| gap | call sites found | landed as |
|---|---|---|
| Merge N meshes | 222 (weapons) + 222 (world) + 35 (ai) | `MeshOp::Merge` |
| TRS with **rotation** | 118 + 97 + 18 | `MeshOp::Trs` |
| a vertex colour stream | world/kit called it *blocking* | `MeshBuffer::colors` |

`MeshOp::Transform` had no rotation at all, and `MeshBuffer` had no channel for
the wear/grime/AO triple every kit builder writes. The subtle half of the colour
work: `from_parts` yields an uncoloured mesh, so every existing operator would
have dropped an authored stream silently — the same defect `01-engine-gaps.md`
records as G4 for authored normals. Vertex-preserving ops now rebuild through
`MeshBuffer::respecified`.

On the field side, named independently by fx, ai and world/kit: **`Floor`,
`Fract`, `Mod`** are absent from `FieldOp`'s 27, and `Noise`/`Fbm` have **no
period parameter**, so nothing in a periodic texture library can tile. Not yet
landed.

### One premise that was wrong

`materials/surfaces/` looks like the best target on every metric — 1,799 lines at
1.66 literals/line, 0.012 branches/line. It is **infeasible**: `axiom-recipe`'s
budget is 256 nodes and the inlined generator graphs are 2.1k–43.4k, and the
algebra deliberately has no loops, no division and no `floor`/`fract`/`mod`.
Those 19 generators belong in hand-written WGSL, which is the route already
being taken. The real targets are `materials/mod.rs::LIBRARY` (957 lines, vocab
of 7, reuse 26.7 — a table wearing Rust) and the `fx` spawn recipes, which need
**no new ops at all**.

### The spawn-recipe schema, derived and ready to execute

`fx/tracers.rs` is converted and pinned by a characterization test carrying all
96 raw buffer values (`dbc6f7f1`). It proves the pattern and, honestly, saves no
lines: N=3, and the Datafication Law's saving is `(N-1) x per-variant-code`.

`impacts.rs` is where it pays — eleven surface recipes (concrete, plaster,
metal, wood, ground, glass, water, flesh, foliage, soft) of the same shape. Each
is a set of **bursts**: `for _ in 0..count { ...fields...; emit }` where every
field is either a constant or an `rng.range(lo, hi)` draw.

So a burst is a table row of `Range { lo, hi }` (a constant being `lo == hi`),
plus a count and an emit kind.

**The one hard constraint, and the reason this is delicate.** The RNG draw
*order* is load-bearing — the stream is shared with every other fx subsystem, so
a reordered draw shifts every later effect in the frame, silently and
invisibly. The orders differ per burst: `wood`'s splinter loop draws
`cone, speed, size0, life, rot, spin, seed`, and its dust loop draws
`cone, speed, off, size0, size1, life, rot, spin, seed` — the same sequence with
`off` and `size1` inserted.

They reconcile: declare **one canonical draw order** with skippable slots, where
a `None` field consumes no draw. Both `wood` bursts satisfy it. Every burst must
be checked against that order individually, and each conversion pinned by a
characterization test capturing the raw buffer before the change — the method
`fx/tracers.rs` demonstrates.

Do not convert a burst without that test. A wrong draw order produces a frame
that still looks plausible.
