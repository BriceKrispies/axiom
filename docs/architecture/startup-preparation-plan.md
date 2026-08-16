# Startup Procedural Preparation — Work Manifest

> **CORRECTION (superseded in part): `crates/axiom-proc` no longer exists.**
> The legacy v1 recipe stack was retired by
> `docs/work-manifests/shader-material-field-system/P1-retire-legacy-proc-stack.md`,
> which migrated `proc-validate`, `placement` and four leaf consumers onto
> `axiom-recipe` + `axiom-proc-core` and deleted the crate. **`ProcTrace` and the
> resumable `Evaluation::{is_done, step(budget), into_output}` were dropped with
> it** — nothing in the repo drove `step` with a real budget.
>
> Every reference below to `axiom_proc::Evaluation` as "the engine already has a
> resumable primitive" or "the future answer" (§9.3, R4, and the capability table)
> is therefore **false as written**. Incremental, budgeted preparation is now an
> unsolved problem with no existing primitive behind it; it would have to be built.
> Nothing else in this plan is affected — the barrier itself never depended on it.

> **Status: design complete, implementation NOT started.** The regression
> baseline this plan is measured against **is** implemented and green (§4).
>
> This document lives beside `engine-core-spine-plan.md` because that is the
> repository's existing convention for an architecture work plan
> (`docs/architecture/<topic>-plan.md`). No `docs/work-manifests/` directory was
> invented.
>
> It is written to be executed by an agent that has not read the conversation
> that produced it. Every path, symbol and number in it was read from the
> repository. Where an earlier draft asserted something that turned out to be
> false, the correction is kept visible rather than quietly deleted — those are
> the places a future agent is most likely to make the same mistake.

---

## 1. Objective

Give Axiom an explicit, structurally-enforced **startup preparation phase**: a
point in an application's lifecycle where expensive, startup-only work runs to
completion, produces runtime-ready in-memory data, and only then permits the
application to enter its playable, frame-stepping state.

```text
game / config / seed
      ↓
startup preparation phase          ← the new lifecycle phase
      ↓
expensive procedural generation    ← app- and module-owned, never engine-owned
      ↓
runtime-ready in-memory data
      ↓
GPU / resource finalization
      ↓
════ PREPARATION BARRIER ════      ← Runtime::start() is unreachable until here
      ↓
game becomes playable
      ↓
normal frame loop
```

**What it is not:**

| Not this | Why the distinction matters |
|---|---|
| **Offline / prebuilt asset baking** | Nothing is generated ahead of the process. Every launch generates from the seed. |
| **Asset packaging** | No `.axpkg`, no archive format, no packer. |
| **Persistent caching** | Nothing is written to disk, IndexedDB, or any store. Launch → generate → run → exit → discard is the whole lifetime. |
| **Runtime streaming** | `modules/axiom-streaming`'s residency ring is demand-driven work *during* play. Preparation is bounded work *before* play. They compose; neither replaces the other. |
| **Ordinary frame-loop procedural generation** | Work that must react to gameplay state (traffic activation, scenery visibility) stays in the frame loop. |

### 1.1 Exactly what is and is not guaranteed

An earlier draft of this document claimed the barrier makes it *"impossible to
reach the running state with mandatory preparation unfinished"*. That
overclaims in two separate ways, and a future agent who inherits the overclaim
will build on sand. The honest statement:

**Guaranteed.** `Runtime::step` cannot execute until a preparation phase has
completed successfully. Since `RunningApp::step` drives `Runtime::step` through
`HostStepDriver::drive` (`modules/axiom/src/app/frame.rs:60-62`), and
`RunningApp::tick` is `step` then `render` (`frame.rs:46-47`), an app that
presents through `tick` — which Burnt Rubber does
(`apps/burnt-rubber/src/app.rs:532`) — cannot advance or present a frame before
preparation completes.

**Not guaranteed, and must not be claimed.**

1. **Presentation is not universally gated.** `RunningApp::render(tick)`
   (`modules/axiom/src/app/frame.rs:102`) is public, documented as *"safe to
   call standalone"*, and contains **zero** references to `self.runtime`
   (verified). A host that owns its own fixed-step loop — the `@axiom/game` TS
   SDK path, per `frame.rs:50-57` — can call `render` on an unprepared app.
   Likewise `HostStepDriver::drive` returns `Ok` without calling `Runtime::step`
   when `HostStepPlan::steps() == 0` (hidden, suspended, or under-budget), so on
   those frames the gate does not fire.
2. **"Mandatory" is a composition-root concept, not a runtime one.** A
   zero-task schedule satisfies the barrier, and §7.4 mandates exactly that at
   every un-migrated call site. `RuntimeState::Prepared` means precisely *"a
   preparation phase ran to completion"* — **not** *"the right work was
   declared"*. What is mandatory for an `App`-based app is
   `modules/axiom`'s own `AuthorTask` at the reserved lowest order (§11.1); that
   contract lives in the composition root, and the plan must say so rather than
   sell the runtime as the guarantor.

The load-bearing property this work actually delivers is therefore:

> The deterministic simulation cannot advance until a preparation phase has
> completed successfully; the composition root decides what that phase contains,
> and for every `App`-based app it always contains scene authoring.

That is a real, testable, useful invariant. It is not the same as "the game
cannot start", and the difference must survive into the implementation.

---

## 2. Current-state findings

Read from the repository. Every claim was verified by reading the named file.

### 2.1 The layer DAG

21 layers. `kernel` is the only root. The spine every app rides is
`kernel → runtime → math → host → frame` (`→ ecs → introspect`).

```text
kernel  (crates/axiom-kernel/layer.toml, depends_on = [])
 ├─ runtime  (["kernel"])
 │   └─ math (["kernel","runtime"])
 │       └─ host (["kernel","runtime","math"])
 │           ├─ layout (["kernel","host"])
 │           └─ frame (["kernel","runtime","host"])
 │               └─ ecs (["kernel","frame"])
 │                   └─ introspect (["kernel","frame","ecs"])
 ├─ space, recipe, crypto, interface  (root-adjacent, ["kernel"])
 └─ proc / proc-core / proc-mesh / proc-texture / noise / geosphere / hydrology
```

**Critical for §11:** only **3 of 21 layers** (`math`, `host`, `frame`) and
**9 of 44 modules** (`scene`, `render`, `resources`, `webgpu`, `physics`,
`agent`, `agent-harness`, `physical-animation`, `axiom`) declare `runtime` in
their manifests. This bounds who can implement a `runtime`-layer trait (§6.2).

### 2.2 The lifecycle today

**`crates/axiom-runtime/src/runtime_state.rs:9-22`** — the engine's only
construction→running state machine:

```rust
#[repr(u8)]
pub enum RuntimeState {
    Created = 0, Initialized = 1, Running = 2, Paused = 3, Stopped = 4, Failed = 5,
}
```

Transitions in `crates/axiom-runtime/src/runtime.rs`: `initialize` `:79`
(`Created → Initialized`), `start` `:92` (`Initialized | Paused → Running`),
`pause` `:105`, `stop` `:116`, `step` `:144` (requires `Running`, else
`StepWhileNotRunning`). The gate is already the right shape:

```rust
(self.state == RuntimeState::Running)
    .then_some(())
    .ok_or_else(|| RuntimeError::new(
        RuntimeErrorCode::StepWhileNotRunning,
        "step() requires the runtime to be in Running"))
    .and_then(|()| self.run_one_step())
```

`RuntimeErrorCode` (`runtime_error_code.rs:10-26`) has seven codes,
`InvalidLifecycleTransition = 1` … `KernelFailure = 7`.

### 2.3 The execution vocabulary that already exists

`RuntimeSystem` (`runtime_system.rs:12`) is
`fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()>`.
`RuntimeScheduler::register` (`runtime_scheduler.rs:70`) takes
`(HandleId, &'static str, i32, Box<dyn RuntimeSystem>)`, rejects duplicate id
(`DuplicateSystemId`) and duplicate order (`DuplicateSystemOrder`, *"no implicit
tie-breaker"*), and sorts by `order` on insert (`:102`). `execute(ctx,
stop_on_error)` (`:133`) runs them in order.

**This is the shape the preparation schedule must mirror.** It is proven,
branchless, and 100%-covered, and it is the template for every line of §8.

Note also `RuntimeScheduler`'s hand-written `Debug` impl (`:36-49`) — required
because it holds `Box<dyn RuntimeSystem>` and
`crates/axiom-runtime/Cargo.toml:18` sets `missing_debug_implementations` to
`warn`. `PreparationSchedule` needs the same, and that impl is a region needing
its own coverage.

### 2.4 Where the transition to "running" happens today — the root defect

`modules/axiom/src/app.rs:324` — `RunningApp::realize(app: App)`:

```rust
runtime.initialize().expect("runtime initialize cannot fail");   // :330
runtime.start().expect("runtime start cannot fail");             // :334
…
let authored = Self::author(app.setup, aspect);                  // :353  ← AFTER start()
```

The runtime reports `Running` for an application whose scene has not been
authored and whose meshes do not exist. The three `.expect("… cannot fail")`
calls show the transition is treated as ceremony. Nothing between `realize` and
the first `request_animation_frame` reads `Runtime::state()`.

Moving `runtime.start()` below `Self::author(...)` patches the symptom. The
structural fix is that **authoring is preparation** and must be expressed as
such, so the ordering is guaranteed by the lifecycle rather than by line order.

Note `realize` returns `Self`, not a `Result`, and `modules/axiom` is inside the
branchless spine — it contains **zero** `?;` today and uses `.expect(…)`. Any
sketch of the corrected `realize` must respect both facts (§11.1).

### 2.5 Where the frame loop begins

Not in `host`, not in `frame`. `modules/axiom-windowing/src/windowing_api/web.rs`:
`drive_web_multi` `:534` starts it; `:590` `spawn_local`; `:599`
`LivePresenter::bind(…).await` — the only existing "wait before frames" seam,
which returns `None` silently on failure (`:612`); `:725`
`request_animation_frame(cb)`.

### 2.6 What does not exist today

- **No lifecycle contract in the kernel.** `crates/axiom-kernel/src/lib.rs:87-143`
  is the complete curated surface; no `Lifecycle`/`Phase`/`Stage`/`Ready`
  symbol. `CLAUDE.md:87` permits "lifecycle contracts" there; the allowance has
  never been used.
- **No `warmup` / `prewarm` / `preload` symbol** anywhere in `crates/`,
  `modules/`, or `apps/`.
- **No readiness gate on engine-internal work.** `HostSkipReason`
  (`crates/axiom-host/src/host_skip_reason.rs:7-16`) has exactly three variants,
  all external: `LifecycleHidden`, `LifecycleSuspended`, `ShutdownRequested`.
- **`SchedulePhase::Startup` is not a barrier.** `crates/axiom-ecs/src/world.rs:133-155`
  runs it on the world's first *active advance* — inside the frame loop, after
  the app is playable. It is a first-frame hook.

### 2.7 The units of work that already exist

| Symbol | File | Shape |
|---|---|---|
| `ProcMeshApi::bake(&RecipeGraph, seed)` | `crates/axiom-proc-mesh/src/proc_mesh_api.rs:24` | Synchronous, all-or-nothing |
| `ProcTextureApi::bake(&RecipeGraph, seed)` | `crates/axiom-proc-texture/src/proc_texture_api.rs:24` | Same |
| `Evaluation::{is_done, step(budget), into_output}` | `crates/axiom-proc/src/evaluation.rs:47,53,72` | **Resumable, budget-independent** |

`apps/axiom-proc-player/src/room.rs:135-141` already runs a hand-rolled bake
barrier at construction and records the microseconds it cost (`room.rs:34`) —
the existing proof that the use case is real and unsupported.

**Note for §11:** `axiom-proc`'s `layer.toml` is `["kernel","space","entropy"]`
— it does **not** declare `runtime`. The layer that owns the resumable bake
primitive cannot implement a `runtime`-layer trait without adding a ceremonial
dependency, which the Layer Law forbids. §11.2 is written around this.

---

## 3. Burnt Rubber current-state generation map

Read from `apps/burnt-rubber/src/`. The app depends on **no engine proc crate**;
every generator is app-local, seeded from one constant.

- **Root seed:** `src/lib.rs:109` — `DEFAULT_SEED: u64 = 0x0B17_4E7A_5C09_1D33`
- **RNG:** `src/draw.rs:17` — `Draw` over `axiom_kernel::DeterministicRng`, with
  `Draw::fork(salt)` at `:52`
- **Seed partitioning:** `src/course/compiler/seeds.rs:31-100` — six
  `SeedDomain`s, `section_seed()` keyed by *stable section name* (`:86`)
- **Course length:** 9 270 m (sum of `PACING[].length_m`,
  `src/course/procedural.rs:99..216`), ~4 635 samples at 2 m spacing

### 3.1 Three different counts that must not be conflated

`src/render/road_mesh.rs` carries three distinct chunkings, and an earlier draft
of this document conflated them in the very section written to keep them apart.
The corrected numbers:

| Constant | Line | Value | Count over 9 270 m |
|---|---|---|---|
| `CHUNK_LENGTH` | `:42` | 100 m | **93** scenery/authoring cells |
| `MESHES_PER_DRAW` | `:74` | 4 | — |
| `DRAW_SPAN` | `:77` | 400 m | **24** `build_draw_mesh` calls → **96** entities (4 material parts each: surface, paint, rail, verge) |
| `PAINT_CHUNK_LENGTH` | `:114` | 10 m | **927** fine paint meshes |

A third unrelated 92 lives at `src/render/effects.rs:26,28` —
`STREAK_COUNT + SPARK_COUNT = 64 + 28 = 92` entities. **Do not** write "~92
meshes" anywhere; say which of the four numbers is meant.

### 3.2 Construction-time generation (CPU, one-shot)

Entry: `procedural::plan_for` (`src/course/procedural.rs:391`) →
`compiler::compile` (`src/course/compiler/mod.rs:184`), from
`RaceSim::with_profile` (`src/sim/mod.rs:191`).

| Generator | File:line | Output | Mutated in play? |
|---|---|---|---|
| `shipping_spec` | `course/procedural.rs:248` | `CourseSpec` | no |
| `motifs::expand` | `course/motifs/mod.rs:32` | `Vec<SectionSpec>` | no |
| **`geometry::compile`** | `course/geometry/mod.rs:173` | `CompiledGeometry` — ~4 635 `TrackSample`s | no |
| `Track::from_samples` | `track/mod.rs:99` | `Track` (documented immutable) | no |
| `traffic::flow::compile` | `course/traffic/flow.rs:41` | `Vec<TrafficPlan>` | no — the pool holds copies |
| `traffic::encounters::compile` | `course/traffic/encounters.rs:40` | `EncounterOutput` | no |
| `pickups::expand_row` | `course/pickups.rs:47` | `Vec<BoostPickup>` (no RNG) | no |
| near-miss windows | `course/compiler/mod.rs:327-388` | `Vec<NearMissWindow>` | no |
| `validation::traversal::analyse` | `course/validation/traversal.rs:182` | `TraversalGrid` — **a local inside `validate()`**, consumed and dropped before it returns; `CoursePlan` stores only the `ValidationReport` (`course/runtime/mod.rs:48`) | n/a |
| `validation::validate` | `course/validation/mod.rs:67` | `ValidationReport` | no |
| `DistanceIndex::build` | `course/runtime/activation.rs:38` | spatial index, `BUCKET_M = 100` | no |
| **`CoursePlan::assemble`** | `course/runtime/mod.rs:57` | `CoursePlan` — no `&mut self` method exists; held only behind `Arc`; no interior mutability | **no** |

> **Correction.** An earlier draft attributed a 10 800-step ghost-validation sim
> to `validate()` and counted it in the per-compile cost. That is **false**.
> `validate()` (`course/validation/mod.rs:67-118`) never touches
> `validation::ghost`, and `validation::ghost::run` (`ghost.rs:105`) has **zero
> non-test callers in `src/`** — its only caller is
> `tests/course_pipeline.rs:227`. It is never paid on the shipping path. The real
> per-compile cost is geometry + flow + encounters + traversal analysis, which is
> a much smaller number, and §18's instrumentation will show it. Do not let this
> claim reappear.

Scene resources, from `RaceScene::install` (`src/render/mod.rs:85`):

| Generator | File:line | Notes |
|---|---|---|
| `RoadChunks::install` | `render/chunks.rs:168` | 24 `build_draw_mesh` → 96 entities, plus 927 fine paint meshes |
| `build_over_samples` | `render/road_mesh.rs:198` | Adjacent chunks **share** their boundary sample (`:126`) so seams are bit-identical |
| `asphalt_albedo` | `render/asphalt_texture.rs:300` | 128×128 RGBA8 = 64 KB, **argument-free constant**, `f()==f()` tested |
| `verge_albedo` | `render/verge_texture.rs:145` | 64×64 = 16 KB, argument-free |
| `foliage_albedo` | `render/foliage_texture.rs:211` | 64×64 = 16 KB, argument-free |
| `install_cone` / `install_palm_crown` / `install_shrub` | `render/prop_meshes.rs:36,89,159` | Argument-free |
| `PlayerCar::install` | `render/car_model.rs:332` | ~35 parts; **installed twice** (player + ghost) |
| `TrafficVisuals::install` | `render/car_model.rs:869` | 3 entities per pooled car |
| `PickupVisuals::install` | `render/pickups.rs:135` | 36 entities |
| `Effects::install` | `render/effects.rs:50` | 92 entities; per-slot seeds drawn once, never again (`:39-40`) |
| `distant_hills` | `render/scenery.rs:633` | Install-time only |
| `install_finish_arch` | `render/mod.rs:1253` | 3 entities + a point light |
| `install_lights` | `render/mod.rs:649` | The light rig |
| `ScenePalette::install` | `render/palette.rs:726` | Materials; generates `foliage_albedo` at `:779` |
| `palette::road_materials` | `render/palette.rs:534` | Generates asphalt at `:536`, verge at `:539` |
| **`DebugView::install`** | `app.rs:200` → `debug_view.rs:104-128` | A mesh, one `Material` per `MarkerKind`, and pooled entities per kind. Construction-time and startup-suitable |

### 3.3 Regeneration and copy waste — the measurable targets

- **Four course compiles**, all verified non-test: `app.rs:184`
  (`RaceSim::with_profile`), `app.rs:208` (`GhostRun::new` → `ghost.rs:55`),
  `app.rs:285` (`start_race`), `app.rs:319` (`restart_ghost`). So: **twice at
  construction, twice more on every restart.**
- **Two gratuitous 371 KB deep copies of the sample table**, which the migration
  should also remove because they are on the same path:
  - `src/sim/mod.rs:207` — `let track = plan.track().clone();` is used only at
    `:209`, `:211`, `:219` and then **discarded**; `RaceSim` has no `track`
    field (it reads `self.plan.track()` at `:264`). NLL permits dropping the
    `.clone()`.
  - `src/course/compiler/mod.rs:197` — `geometry.samples.clone()` into
    `Track::from_samples`; `geometry.samples` is never read again, so this can
    be a move.
  - `TrackSample` ≈ 80 B × 4 635 ≈ **371 KB per copy**; today's construction
    path pays it four times.

### 3.4 Genuinely runtime work (must stay)

| Work | File:line | Why |
|---|---|---|
| `RoadChunks::update` | `render/chunks.rs:342` | Writes only `Visible(bool)`; early-outs at `:352` on an unchanged range. **No mesh-creating path exists after install** — every `add_mesh*`/`add_texture_data`/`spawn` site in `src/` is inside an `install`/`new` |
| `SceneryField::refresh` | `render/scenery_pool.rs:138` | Retains what stayed (`:150`) and generates only chunks that **entered** (`:151-157`) — one chunk per range advance, ~1/s |
| `SceneryField::pose` | `render/scenery_pool.rs:164` | Camera-dependent; `O(cached props)` every frame |
| `Traffic::activate` | `sim/traffic.rs:267` | Keyed on `player_distance ± traffic_ahead/behind` and on which pool slot is free |
| `RaceSim::collect_pickups` | `sim/mod.rs:632` | `PickupField.taken` is per-run mutable state |
| Collision / contact | `sim/collision.rs`, `sim/contact.rs` | Derived per step from car state |
| `Effects::step` / `pose` | `render/effects.rs:102,111` | Camera-relative, from install-time seeds |
| `GhostRun` stepping | `ghost.rs`, `agent.rs` | Live agent simulation |

> **Correction.** An earlier draft justified keeping traffic activation runtime
> partly because `Draw::seeded(plan.variation_seed)` is drawn per activation.
> That half of the reason is **wrong**. `sim/traffic.rs:339-345` is documented as
> *"A pure function of the plan and nothing else — not of when it activated, not
> of which pool entry it landed in"*: `variation_seed` is compiled data
> (`course/traffic/flow.rs:243`), and the two draws (`wander_phase`,
> `wander_amount`) are a fixed two-value sequence off a fresh `Draw`. They are as
> pre-computable as anything in P1 — see P7. The **only** correct reason
> activation stays runtime is the distance-keyed scheduling and slot assignment.
> Notably `traffic.rs:345` is the app's *only* runtime `Draw::seeded` site.

---

## 4. Golden regression fixture — IMPLEMENTED AND GREEN

The only part of this manifest already built. It is the baseline every later
section is measured against.

### 4.1 Location and ownership

| Artifact | Path |
|---|---|
| Run definition (constants + drive loop) | `apps/burnt-rubber/src/golden.rs` |
| Determinism test + canonical encoders | `apps/burnt-rubber/tests/agent_golden.rs` |
| Committed golden bytes (**15 files**) | `apps/burnt-rubber/tests/golden/agent_<name>_{state,render,resources}.bin` |
| SHA-256 pins + slice contract | `apps/burnt-rubber/slice.toml` |
| Pixel harness registrations (5 rows) | `tools/axiom-shot/src/registry.rs` |
| Documentation | `apps/burnt-rubber/TESTING.md` §0 |

### 4.2 Seed and configuration

All constants in `apps/burnt-rubber/src/golden.rs`, each asserted by
`the_golden_run_is_pinned_to_the_shipping_game`:

```rust
GOLDEN_SEED       = crate::DEFAULT_SEED   // 0x0B17_4E7A_5C09_1D33
GOLDEN_PROFILE    = PlayProfile::Wheel
GOLDEN_TUNING     = Tuning::DEFAULT
GOLDEN_WIDTH      = 960     // == capture::CAPTURE_WIDTH
GOLDEN_HEIGHT     = 600     // == capture::CAPTURE_HEIGHT
GOLDEN_DRIVER     = DriverTuning::FAST
GOLDEN_STEP_LIMIT = 18_000  // 5 minutes of simulated racing
```

### 4.3 How `axiom-agent` drives it

Per fixed step, `golden::driven_with_count` calls
`agent::drive_one_step(app.sim(), &GOLDEN_DRIVER, tick)`
(`src/agent.rs:654`), which runs the full `observe → decide → emit` cycle
through `axiom_agent::AgentApi::step`
(`modules/axiom-agent/src/agent_api.rs:354`) and lowers the returned `move_axis`
intents into the one `DriveCommand` the simulation is given. No hand-written
command exists anywhere in the run.

`the_agent_is_what_moves_the_car` makes this load-bearing: 700 steps of
`DriveCommand::IDLE` leaves the car >500 m behind the agent-driven run. The
app's ghost (`src/ghost.rs`) is a *second* live agent run advanced by
`advance_steps`, so every checkpoint exercises the agent twice.

### 4.4 The five checkpoints

Logical simulation steps and a game-state condition — no wall-clock anywhere.

| # | Name | Stop | axiom-shot slice | Where the agent is |
|---|---|---|---|---|
| 1 | `grid` | step 0 | `burnt-rubber-golden-grid` | Held on the grid, counting in — before meaningful driving |
| 2 | `opening` | step 700 | `burnt-rubber-golden-opening` | Off the line, at speed on the coastal sweepers |
| 3 | `esses` | step 2200 | `burnt-rubber-golden-esses` | Mid-run: ridge crests and technical bends |
| 4 | `canyon` | step 3800 | `burnt-rubber-golden-canyon` | Late, flat out, deep into the boost spend |
| 5 | `finish` | `RacePhase::Finished` | `burnt-rubber-golden-finish` | The step it crosses the line |

### 4.5 What each checkpoint stores — three independent artifacts

- **`agent_<name>_state.bin`** (114 B) — steps, sim step count, phase, elapsed
  seconds, distance, lateral, yaw, speed, world position, progress, section
  index, boost charge + active, near misses, impacts, top speed, camera
  eye/target/fov/roll, and the ghost delta as a presence byte + value.
- **`agent_<name>_render.bin`** (43–147 KB) — tick, command count, clear colour,
  camera view-proj, light view-proj; every draw in submission order with
  **mvp, world, colour, emissive, specular**, mesh id, material id and
  contact-shadow flag; every light; and the authored render **look** — ambient
  sky/ground, and depth fog, colour grade, sky and bloom each as a presence byte
  plus fields.
- **`agent_<name>_resources.bin`** (~2.6 KB) — for every uploaded mesh: id,
  vertex count, index count, and a `StableHash` (the kernel's platform-stable
  FNV-1a) of the exact IEEE-754 bit patterns; for every uploaded texture:
  material id, width, height, sampling mode, and a `StableHash` of the pixels.

**Why the third artifact is the one this app most needs.** `FrameOutcome`
carries a mesh *id*, never its vertices — geometry and texels are uploaded once
at bind. So a road chunk built from a stale track, an off-by-one sample range, a
seam that stops being bit-identical (`render/road_mesh.rs:126`), or a moved
constant inside `asphalt_albedo` all render a *visibly* different game while
leaving the draw list byte-identical. Without it the fixture would be sensitive
to mesh-id churn and blind to mesh content — the worst combination for a
baseline whose whole job is to prove a lifecycle migration changed nothing. It
is also precisely what §12's P2/P3/P4/P5 move.

`f32` is encoded as its exact IEEE-754 little-endian bit pattern; collections
are length-prefixed with a `u32` count, matching
`apps/axiom-rotating-cube/tests/render_determinism.rs`.

### 4.6 Measured baseline

```text
burnt-rubber golden run: finished in 5419 steps (90.30 s),
71 near misses, 9 impacts, top speed 113.9 m/s
```

The 15 SHA-256 pins are in `apps/burnt-rubber/slice.toml`, which is the
authoritative record; `cargo run -p xtask -- check-slices` is the check that
matters. All five `resources` goldens hash identically
(`81c4cbd7…95a48aaf`) — the expected and correct result, since the resource set
is installed once at construction and nothing is generated during a race.

### 4.7 Reproduction and verification

```sh
# Verify the whole golden run against the committed baseline (the one command):
cargo test -p axiom-burnt-rubber --test agent_golden -- --nocapture

# Verify the SHA-256 pins (catches a deleted/regenerated/hand-edited golden):
cargo run -p xtask -- check-slices

# Render the five checkpoints as real pixels through the real backend:
cargo build --release -p axiom-shot --features offscreen
for cp in grid opening esses canyon finish; do
  ./target/release/axiom-shot --app burnt-rubber-golden-$cp \
    --backend gpu --out screenshots/golden/$cp.gpu.png
done
# ...and through the deterministic CPU rasterizer:
#   --backend canvas2d --out screenshots/golden/$cp.canvas2d.png

# Re-bless after an INTENDED change, then repin the hashes in slice.toml:
AXIOM_REGOLD=1 cargo test -p axiom-burnt-rubber --test agent_golden
```

`AXIOM_REGOLD` is compared against `"1"` exactly — `AXIOM_REGOLD=0` reading as
"re-bless everything" is the kind of footgun that silently destroys a baseline.

### 4.8 Determinism verification actually performed

| Check | Method | Result |
|---|---|---|
| All three artifacts, same process | `the_golden_run_replays_byte_equal` — every checkpoint driven twice from scratch | **byte-equal** |
| All three artifacts, across processes | Bootstrapped, then re-run twice against the committed files | **byte-equal** |
| Checkpoints are distinct | `the_checkpoints_are_all_different_frames`, all 10 pairs | **state and render differ; resources identical, as required** |
| Sensitive to the course | `a_different_course_produces_different_bytes` — seed XOR'd with the golden ratio | **all three artifacts move** |
| Sensitive to the driver | `a_different_driver_produces_different_bytes` — `steer_gain_milli + 500` | **state and render move; resources correctly do not** |
| **GPU pixels, two separate processes** | 5 checkpoints × 2 passes, SHA-256 compared | **byte-identical on this adapter** |
| **Canvas 2D pixels, two separate processes** | 5 checkpoints × 2 passes, SHA-256 compared | **byte-identical** (meets `Tolerance::EXACT`) |
| Agent-driven ghost slice | `capture::tests::every_slice_renders_identically_twice`, ghost slice added | **passes** |

The repository's image-tolerance policy is
`apps/axiom-growth/src/visual_target/compare.rs:37-47` —
`Tolerance::EXACT { mean: 0.0, max: 0 }` for the deterministic Canvas 2D arm and
`Tolerance::GPU_DEFAULT { mean: 2.0, max: 40 }` for the GPU arm. No threshold
was invented.

> **The GPU result is machine-local evidence, not a portable pin.** Two passes on
> one adapter, one driver, one process configuration. Cross-vendor divergence
> arises from FMA contraction and shared-edge rasterization tie-breaking — and
> this game deliberately puts a very large number of shared triangle edges into
> every frame (`render/road_mesh.rs:126` shares boundary samples between chunks
> so seams are bit-identical on the CPU side). **0/0 should not be expected to
> hold on other hardware.** §17.3 and §23 are written accordingly.

> **This closes a debt the repository has carried for eleven convergence
> iterations.** `visual_targets/burnt-rubber/abstractions/{0001..0004,0008}.toml`
> each record: *"No determinism / repeat-capture tolerance proof exists for ANY
> champion this campaign has promoted."* One now exists, with its limits stated.

### 4.9 Two findings the fixture produced while being built

Both are facts about the game, recorded because a future agent will otherwise
rediscover them the hard way.

1. **`DriverTuning::grip_usage` does not bind on this course.** The first
   driver-sensitivity test perturbed it by 0.01 and **not one byte moved**. The
   course is flat out end to end — its sharpest corner is well inside what the
   chassis holds at top speed — so `plan_speed` saturates against the car's top
   speed everywhere and the cornering-limit term never applies. A driver
   parameter that only shapes braking is invisible here. The test now perturbs
   `steer_gain_milli`, which is emitted as a `move_axis` intent on every one of
   the 5 419 steps.
2. **The agent-driven ghost slice was registered in `axiom-shot` but omitted
   from `every_slice_renders_identically_twice`** — so the one slice framing the
   agent was the only one whose determinism nothing proved. Since the golden run
   rests on exactly that property, it was added (and passes).

### 4.10 What this fixture does *not* catch

Stated so nobody over-trusts it:

- **Nothing measures *when* work runs.** A course compiled twice instead of once,
  or a new per-frame allocation with no visual change, moves zero bytes. §13
  and §17.2 add counter-based assertions for exactly this, because it is what
  the migration claims to improve — the goldens can only confirm nothing broke,
  never that the migration *worked*.
- **The HUD.** `HudModel` (`src/hud.rs`) is rendered into a DOM overlay on
  `wasm32` and never reaches `FrameOutcome`, so a change to the speed readout or
  the countdown display is invisible. It is derived purely from pinned sim state,
  which bounds the risk; encoding it is a reasonable future addition.
- **The debug overlay**, which is off in the shipping path and contributes zero
  draws.
- **Audio cue kinds**, beyond the aggregate near-miss and impact counts.

### 4.11 Harness code changed solely to establish the baseline

| File | Change |
|---|---|
| `apps/burnt-rubber/src/golden.rs` | **New.** Run definition, `GoldenState`, five zero-arg `RunningApp` builders, 8 unit tests. |
| `apps/burnt-rubber/src/lib.rs` | **+2 lines.** `pub mod golden;` |
| `apps/burnt-rubber/tests/agent_golden.rs` | **New.** Three encoders, `assert_golden`, 6 tests. |
| `apps/burnt-rubber/tests/golden/*.bin` | **New.** 15 committed artifacts. |
| `apps/burnt-rubber/slice.toml` | **New.** Slice contract + 15 SHA-256 pins. |
| `tools/axiom-shot/src/registry.rs` | **+5 rows** (`burnt-rubber-golden-*`). |
| `apps/burnt-rubber/src/capture.rs` | **+1 row** in the `slices()` test helper (the ghost slice, §4.9). |
| `apps/burnt-rubber/TESTING.md` | **+§0**, and `check-slices` added to the command list. |

**No game behaviour, visual, or procedural-generation code was touched.** Every
pre-existing test, slice and golden still passes.

---

## 5. Architectural placement decision

> **Extend `crates/axiom-runtime` (the `runtime` layer).**
> No new layer. No kernel change. No host or frame change in v1.

### 5.1 Owners

| Question | Owner |
|---|---|
| Who owns the lifecycle phase? | `crates/axiom-runtime` — `RuntimeState::Prepared` |
| Who owns scheduling of preparation work? | `crates/axiom-runtime` — `PreparationSchedule` |
| Who declares preparation work? | The caller, as `Box<dyn PreparationTask>` |
| Who owns the resulting prepared data? | The caller. The runtime never sees a product |
| Who determines readiness? | `Runtime` — every registered task returned `Ok` |
| Who enforces the barrier? | `Runtime::start`, which requires `Prepared` |
| Who decides *what* is mandatory? | The composition root (§1.1) |
| Who transitions into the frame loop? | `modules/axiom`'s `RunningApp::realize` |

### 5.2 Why this is the lowest correct location

`runtime` is the lowest layer that can enforce the barrier, because the barrier
is a precondition on `Runtime::step`, and `Runtime::step` lives here. Any owner
above `runtime` could only *wrap* the runtime, leaving `Runtime::step` reachable
directly — a bypassable half-fix.

Note the scope limit in §1.1: this gates simulation stepping. It does not gate
`RunningApp::render` standalone. That is a real limit of gating at this layer,
and the alternative (gating presentation too) is a *second consumer* of the same
phase, not a reason to move the phase.

### 5.3 Why it belongs there semantically

`crates/axiom-runtime/ARCHITECTURE.md:8` lists the layer's **first**
responsibility as *"a strict **lifecycle** state machine"*. Preparation is a
lifecycle phase; `RuntimeState` is the lifecycle. This is the missing phase of a
machine that already exists, expressed with the mechanism that already enforces
every other phase. The layer also already owns the exact execution shape needed
(§2.3).

### 5.4 Why not lower (the kernel)

`CLAUDE.md:87` permits "lifecycle contracts" in the kernel and `CLAUDE.md:183-186`
routes broadly-shared primitives there. Both were considered:

1. **The broadly-shared-primitive rule is conditioned on no adjacent layer
   owning it.** Here one demonstrably does: `RuntimeState`, in `runtime`.
2. **Nothing below `runtime` needs the phase.** The kernel does not step,
   schedule, or run systems.
3. **A kernel phase enum would create a second lifecycle vocabulary** to keep in
   sync with `RuntimeState`.
4. **The strongest argument, and the one to cite:** the *trait* is
   runtime-typed. `PreparationTask` returns `RuntimeResult<()>`. A kernel
   version needs a kernel error type, and then the runtime must translate — the
   two-vocabularies failure of point 3, arriving by a different door.

A genuine steelman does exist — contract in the kernel, machine and barrier in
`runtime` — and it fails on point 4.

### 5.5 Why not higher

**`crates/axiom-host`** — host's remit (`ARCHITECTURE.md:102-120`) is *facts an
engine outside of it generated*. Preparation is an internal fact.
`HostLifecycleSignal`'s alphabet is explicitly closed and external
(`host_lifecycle_signal.rs:3-9`). Decisively: `host` depends on `runtime`, so a
phase owned in `host` would be invisible to `Runtime::step`.

**`crates/axiom-frame`** — frame is a pure consumer;
`FrameLifecycleState::from_host` (`frame_lifecycle_state.rs:31`) is a total
projection of `HostLifecycleState` and cannot originate a state with no host
antecedent. It also sits above host and could not gate stepping.

**`modules/axiom`** — the correct *composition* site (§11), not the owner.
Module Law #8 permits one facade, and an engine module cannot be depended on by
another module, so a barrier owned here would be unreachable by any other
module.

### 5.6 Why a new layer would be wrong

A layer with `depends_on = ["kernel", "runtime"]` exposing a `PrepareApi` would
**pass** all seven `xtask check-architecture` rules and the
`engine_genuine_dependency` dylint — `CLAUDE.md:258-266` says so itself. It
would still be wrong:

1. **`CLAUDE.md:119`** — "Do not create tiny ceremonial layers just to feel
   organized."
2. **The edge would be useless.** To gate stepping the phase must be visible to
   `runtime`; `runtime` cannot depend on a layer above it. The new layer could
   only wrap `Runtime`, leaving `Runtime::step` bypassable.
3. **The `space`/`interface` precedent does not transfer.** Those exist because
   an `Address` and a `PanelId` have *no* lower-layer home. A lifecycle phase
   has one.

A "vocabulary-only" layer fails the same way: placed above `runtime` it cannot
be named by `runtime`; placed below with `depends_on = []` it *is* the kernel by
definition, which returns to §5.4.

---

## 6. Dependency analysis

### 6.1 What the implementation actually adds: zero new edges

- `crates/axiom-runtime/layer.toml` keeps `depends_on = ["kernel"]`. The new
  code uses `axiom_kernel::HandleId` (already used by `RuntimeScheduler`) and
  the existing error types. No new crate in `Cargo.toml`.
- `crates/axiom-host`, `crates/axiom-frame`, `crates/axiom-ecs` gain no edges.
- `modules/axiom` and `apps/burnt-rubber` already depend on `axiom-runtime`.

| Rule | Status |
|---|---|
| Layer Law: DAG acyclic | Unchanged graph |
| Layer Law: imports only declared deps | `runtime` still imports only `axiom_kernel` |
| Layer Law: genuine use | `kernel` use *increases* |
| Layer Law: capabilities are public exports | Two new symbols added to `layer.toml` (T5) |
| Module Law #1/#9/#10/#11 | Unchanged |
| Branchless Law: baseline 0 | All new runtime code branchless — sketch in §9.8 |
| Coverage Law: 100% | All new runtime code ships covered — §14 |

### 6.2 What the *advertised pattern* would cost — the honest caveat

An earlier draft claimed any layer, module or app may implement
`PreparationTask`. Measured against the manifests that is **false for most of
the repo**: only 3 of 21 layers and 9 of 44 modules declare `runtime`.
Critically, `modules/axiom-assets` (`allowed_layers = ["kernel"]`) and
`crates/axiom-proc` (`["kernel","space","entropy"]`) — both named as examples in
the earlier draft — do not.

**Adding `runtime` to a layer's `depends_on` solely to implement
`PreparationTask` is a ceremonial dependency and is forbidden.** §11.2 states
the consequence: for everything that does not already depend on `runtime`, the
**app wraps the module's product-producing call in an app-owned task**. That is
the §12 pattern anyway, and it is *the* pattern rather than one of several.

Three crates reach `Running` in **test** code and will need their tests updated
at T7 (`cargo xtask check-architecture` must be re-run afterwards, not assumed):
`crates/axiom-frame` (`frame_step_summary.rs:110,184,243`), `crates/axiom-host`
(`host_api.rs:571`, `host_step_driver.rs:147,257,272`), and
`crates/axiom-introspect` (`src/fixtures.rs:32,70` — which names `axiom_runtime`
via a dev-dependency despite `layer.toml` not declaring it).

---

## 7. Lifecycle / state model

### 7.1 The states

```text
   Created ──initialize()──▶ Initialized ──prepare(schedule)──┬──all Ok──▶ Prepared
                                                              │                │
                                                       any task Err          start()
                                                              ▼                ▼
                                                           Failed           Running
                                                          (terminal)         │   ▲
                                                                      pause()│   │start()
                                                                             ▼   │
                                                                           Paused
   Initialized / Prepared / Running / Paused ──stop()──▶ Stopped (terminal)
```

**There is no `Preparing` state.** An earlier draft had one and justified it as
"a real observable state". It is not observable: `prepare(&mut self)` holds the
exclusive borrow for the whole phase, and a task is handed no `Runtime` handle,
so no task, test or host can ever see it. Shipping an unobservable variant means
shipping arms the Coverage Law forbids ("dead branches added only to be
covered"). Discriminants are appended (§7.2), so if a budgeted executor later
makes it observable, adding it then costs nothing.

### 7.2 Discriminants

Appended, never renumbered — `raw()` is a stable identity byte surfaced through
`RuntimeStepRecord::state_after()`:

```rust
Created = 0, Initialized = 1, Running = 2, Paused = 3, Stopped = 4, Failed = 5,
Prepared = 6,      // NEW, appended
```

**Drop the `PartialOrd, Ord` derive** from `runtime_state.rs:7`. The only
consumer of the ordering in the entire workspace is the assertion
`RuntimeState::Created < RuntimeState::Running` at `runtime_state.rs:45` — a
test asserting the derive it is testing. `RuntimeState` is not a `BTreeMap`/
`BTreeSet` key anywhere. Leaving the derive after appending `Prepared = 6`
creates a total order that reads as lifecycle progression and is not one.
Replace that assertion with `assert_eq!(RuntimeState::Prepared.raw(), 6)`.

### 7.3 Legal transitions

| From | Via | To |
|---|---|---|
| `Created` | `initialize()` | `Initialized` |
| `Initialized` | `prepare(schedule)` | `Prepared` (all Ok) or `Failed` (any Err) |
| `Prepared` \| `Paused` | `start()` | `Running` |
| `Running` | `pause()` | `Paused` |
| `Initialized` \| `Prepared` \| `Running` \| `Paused` | `stop()` | `Stopped` |

### 7.4 Illegal transitions — each must fail deterministically

| Attempt | Result |
|---|---|
| `start()` from `Initialized` | `InvalidLifecycleTransition` — **this is the barrier** |
| `start()` from `Created` / `Failed` / `Stopped` | `InvalidLifecycleTransition` |
| `prepare()` from `Created` | `InvalidLifecycleTransition` |
| `prepare()` from `Prepared` / `Running` / `Paused` | `InvalidLifecycleTransition` — **preparation runs exactly once per launch** |
| `prepare()` from `Failed` / `Stopped` | `InvalidLifecycleTransition` |
| `step()` from anything but `Running` | `StepWhileNotRunning` (unchanged) |

**The barrier is one line:** `start()` accepts `Prepared | Paused` instead of
`Initialized | Paused`.

**Migration consequence, deliberate:** every existing `initialize(); start()`
fails to reach `Running` and its tests break loudly. There are 23 `.initialize()`
call sites across the workspace. A zero-task `prepare()` is legal and immediate,
so each migrates to `initialize(); prepare(PreparationSchedule::new())?; start()`.
The compiler and the test suite do the finding.

### 7.5 Why not typestate

An earlier draft claimed typestate "would fight the branchless law". That is
**backwards** — typestate is compile-time only, contains no control flow, and
would *remove* the state comparison in `start()`.

The real objection is borrow shape and re-entry: `RunningApp` **stores** a
`Runtime` field, `HostStepDriver::drive(&mut Runtime, …)`
(`host_step_driver.rs:77`) takes it by mutable reference, and `Paused → Running`
is a legal re-entry. A consuming `prepare(self) -> PreparedRuntime` would force
a type change through host, frame and the umbrella for a property the existing
state check already delivers.

---

## 8. Preparation API design

**Two new public symbols.** An earlier draft proposed five; three were exercised
by nothing but tests written to cover them, and one of those had an unreachable
half. The reduction is the design improving, not scope being cut.

### 8.1 `PreparationTask` (new, `crates/axiom-runtime/src/preparation_task.rs`)

```rust
pub trait PreparationTask {
    fn prepare(&mut self) -> RuntimeResult<()>;
}
```

| | |
|---|---|
| **Owner** | `crates/axiom-runtime` |
| **Responsibility** | One unit of startup-only work, supplied from above and opaque to the runtime |
| **Inputs** | `&mut self` — the task's own recipe, config and product handles |
| **Outputs** | `RuntimeResult<()>`. **Products never flow through the runtime** (§10) |
| **Failure** | `Err` fails the phase; the runtime transitions to `Failed` |
| **Lifetime** | Dropped when `prepare()` returns |
| **Determinism** | No clock, no ambient entropy; a seeded task carries its own seed — enforced by test, not by prose (§14.4, §17.2) |
| **Why public** | Callers must implement it. This is the entire declaration mechanism |

**Zero arguments is deliberate.** An earlier draft passed a `PreparationContext`
offering `task_index()`, `log()` and `record_metric()`. Nothing outside a test
written to cover it consumed any of the three — §18 routes *all* measurement to
the host/tooling boundary and to the app that owns the task. A task that wants
to log routes through the caller's own sink, exactly as §18 already requires for
timing.

**This trait is the anti-leakage mechanism.** `fn prepare(&mut self)` and
`fn run(&mut self, ctx: &mut RuntimeContext<'_>)` are incompatible signatures, so
a `PreparationTask` cannot be passed to `RuntimeScheduler::register` and a
`RuntimeSystem` cannot be passed to `PreparationSchedule::register`. The type
system, not developer discipline, keeps startup work out of the frame loop — and
a zero-argument `prepare` is *strictly narrower* than the context version, so it
satisfies that intent more completely.

### 8.2 `PreparationSchedule` (new, `crates/axiom-runtime/src/preparation_schedule.rs`)

```rust
pub struct PreparationSchedule { /* private fields */ }

impl PreparationSchedule {
    pub fn new() -> Self;
    pub fn register(&mut self, id: HandleId, name: &'static str, order: i32,
                    task: Box<dyn PreparationTask>) -> RuntimeResult<()>;
}
impl Default for PreparationSchedule { /* … */ }
impl std::fmt::Debug for PreparationSchedule { /* hand-written, as RuntimeScheduler:36-49 */ }
```

| | |
|---|---|
| **Owner** | `crates/axiom-runtime` |
| **Responsibility** | The declared, deterministically-ordered set of startup work |
| **Inputs** | Stable `HandleId` (**`HandleId::from_raw(n)`** — there is no `HandleId::new`), static name, explicit `i32` order, boxed task |
| **Outputs** | `RuntimeResult<()>`; entries sorted by `order` on insert |
| **Failure** | `DuplicateSystemId` / `DuplicateSystemOrder` — **reusing the existing codes**, because the invariant is identical |
| **Lifetime** | Moved into `Runtime::prepare` and dropped there |
| **Determinism** | Total order by `order`, no implicit tie-breaker (a tie is an error) |
| **Why public** | Callers must declare work before handing it over |

`len()`, `is_empty()` and `task_ids()` are **not** exposed. Their only consumers
would be tests of the accessors themselves; execution order is proved through a
shared trace, exactly as `runtime_scheduler.rs:264` already does. The
hand-written `Debug` is required by `Cargo.toml:18`'s
`missing_debug_implementations` and needs its own coverage.

### 8.3 `Runtime::prepare` (new method on the existing type)

```rust
pub fn prepare(&mut self, schedule: PreparationSchedule) -> RuntimeResult<()>;
```

| | |
|---|---|
| **Owner** | `crates/axiom-runtime` |
| **Responsibility** | Run the declared work to completion and decide readiness |
| **Inputs** | The schedule, **by value** |
| **Outputs** | `Ok(())`; the runtime is left `Prepared` |
| **Failure** | `InvalidLifecycleTransition` if not `Initialized`; `PreparationFailed` if any task erred, leaving the runtime `Failed` |
| **Lifetime** | Consumes the schedule; every task is dropped before returning |
| **Determinism** | Tasks run in `order`, single-threaded, one pass |
| **Why public** | It is the barrier |

Taking the schedule **by value** is load-bearing: it is what makes "temporary
work can die" a guarantee rather than a convention (§10), and what makes
`prepare()` un-repeatable without building a fresh schedule.

**No `PreparationReport`.** An earlier draft returned one, and its failure half
was unreachable by construction: the report was only produced on the success
path, so `all_succeeded()` could never be `false` and two specified tests were
unwritable. Rather than restructure the return type to rescue an API nothing
reads, the report is deleted. Failure diagnosis is preserved where it belongs:
`prepare` constructs `RuntimeError::new(PreparationFailed, name)` using the
**failing task's `&'static str` name**, which the schedule already holds — so
the caller learns *which* task failed. (`RuntimeError` has no
runtime-error-wrapping constructor — only `new(code, &'static str)` and
`with_kernel(…)` — so the task's own error value cannot be nested without
changing error identity semantics, which is out of scope.)

### 8.4 Modified existing symbols

| Symbol | File | Change |
|---|---|---|
| `RuntimeState` | `runtime_state.rs:9` | `+ Prepared = 6`; drop `PartialOrd, Ord` |
| `RuntimeErrorCode` | `runtime_error_code.rs:10` | `+ PreparationFailed = 8` |
| `Runtime::start` | `runtime.rs:92` | Accept `Prepared \| Paused` |
| `Runtime::stop` | `runtime.rs:116` | Also accept `Prepared` |
| `crates/axiom-runtime/layer.toml` | — | `introduced_capabilities += ["PreparationTask", "PreparationSchedule"]`; one `[[proof_exports]]` for `PreparationSchedule` with `must_reference = ["HandleId"]` |

### 8.5 Deliberately NOT in the API

Each rejected as speculative; none is needed by the Burnt Rubber proof, and each
can be added later without breaking the above:

- A DAG / `depends_on` between tasks (§9.1)
- A budget / resumable `prepare_step(n)` (§9.3)
- Cancellation (§9.5)
- A progress callback or percentage
- Any typed product channel through the runtime (§10.2)
- `HostSkipReason::NotPrepared` — but see §1.1 and §21 R1: if presentation
  gating is later wanted, this is where it goes, and §5.5's argument is about
  ownership of the *phase*, not about whether host may *report* it

---

## 9. Preparation execution model

### 9.1 Ordering: an explicit total order, not a DAG

Tasks run in ascending `order`; a duplicate `order` is an error. **A DAG must
not be built.** Burnt Rubber's real chain — compile course → derive `Track` →
build road meshes → build scenery → upload textures — is **linear**. An explicit
`i32` expresses it, is what `RuntimeScheduler` already does, and is checkable by
eye. A dependency solver for a chain is the "giant generalized task framework"
the constraints forbid.

Dependencies between outputs are expressed as the repository already does: a
later task holds a handle to storage an earlier task wrote (§10), and `order`
encodes the sequence.

### 9.2 Concurrency: architecturally excluded, not merely deferred

v1 is **single-threaded, one pass, in declaration order**:

- `wasm32-unknown-unknown` — the primary target — has no threads in this build.
- Concurrency is the easiest way to make committed outputs depend on completion
  order.

An earlier draft added a reassurance that concurrency could be introduced later
by "committing results in declaration order regardless of completion order".
**That rule is false here**, because §10.2 forbids the runtime holding any
product: there is nothing for the runtime to buffer and nothing to reorder. Two
counterexamples, the second already load-bearing in this plan:

- **Read-modify-write on a shared cell.** Task A writes `out = X`; task B does
  `out = f(out)`. Concurrently B reads stale and writes `f(stale)`. The runtime
  never sees either write.
- **Append-order-determined identity.** Mesh ids are assigned in
  `RunningApp::add_mesh_data` registration order and are encoded into
  `agent_*_render.bin`. Concurrent P3/P4/P5 tasks appending to the shared mesh
  list produce ids in *completion* order, and the render goldens move.

A structural corroboration: `Rc<RefCell<T>>` is `!Send`, so the §10.1 product
channel cannot cross a thread boundary at all. **Adding concurrency requires
reopening §10.** Say that, rather than implying it is additive.

### 9.3 No budgeting or resumption in v1

`prepare()` runs to completion. ~~The engine already has a resumable primitive for
callers that need to spread generation across frames —
`axiom_proc::Evaluation::{is_done, step(budget), into_output}` — and it sits
*above* runtime in the DAG. A task that wants incremental generation drives an
`Evaluation` to completion inside its own `prepare`.~~ **No longer true**: that
crate and that primitive were deleted (see the correction at the top). There is
no resumable generation primitive in the engine today.

Known consequence: on `wasm32` a long synchronous `prepare()` blocks the main
thread and the page cannot repaint. That is **exactly what happens today**
(§2.4), so it is not a regression — but it is why a budgeted model may
eventually be wanted (R4).

### 9.4 Failure semantics

Execution **stops at the first failing task**; the rest do not run. The runtime
transitions to `Failed` and `prepare()` returns
`Err(RuntimeError::new(PreparationFailed, <failing task's name>))`.

`Failed` is terminal, so `start()` is unreachable. No partial readiness, no
"start anyway". This differs deliberately from
`RuntimeConfig::fail_on_system_error`, which lets *per-step* systems continue: a
frame can survive a bad system; an application cannot survive a world that was
never built.

### 9.5 Cancellation: none

Not needed by the vertical proof and not free. Dropping the `Runtime` is the
cancellation story.

### 9.6 Readiness detection

`self.state == RuntimeState::Prepared`, set only by a `prepare()` in which every
registered task returned `Ok`. Not a count, not a percentage.

### 9.7 The barrier

`Runtime::start()` requires `Prepared | Paused`; `Runtime::step()` requires
`Running`. Composed: **no simulation step can occur until every registered
preparation task has completed successfully.** See §1.1 for what this does and
does not extend to.

### 9.8 Branchless implementation sketch (verified feasible)

Every piece already exists in the crate; this is a transcription, not an
invention:

```rust
pub fn prepare(&mut self, schedule: PreparationSchedule) -> RuntimeResult<()> {
    (self.state == RuntimeState::Initialized)
        .then_some(schedule)
        .map_or(Err(invalid_transition("prepare requires Initialized")),
                |s| self.run_preparation(s))
}

fn run_preparation(&mut self, mut schedule: PreparationSchedule) -> RuntimeResult<()> {
    let failure = schedule.execute();          // Option<&'static str>, schedule dropped here
    self.state = [RuntimeState::Prepared, RuntimeState::Failed]
        [usize::from(failure.is_some())];
    failure.map_or(Ok(()), |name| {
        Err(RuntimeError::new(RuntimeErrorCode::PreparationFailed, name))
    })
}
```

`PreparationSchedule::execute` transcribes `runtime_scheduler.rs:139-152` — a
`try_fold` over `iter_mut()` selecting continuation with
`[ControlFlow::Continue(()), ControlFlow::Break(name)][usize::from(failed)]`.
`register` transcribes `runtime_scheduler.rs:77-107`'s
`duplicate_id.or(duplicate_order).map_or_else(…)`. Zero
`if`/`match`/`for`/`while`/`&&`/`||`/`?`.

---

## 10. Prepared-data ownership

**The runtime owns the fact; the caller owns the data.**

| Thing | Owner | Survives the barrier? |
|---|---|---|
| Recipes, specs, seeds | The caller, captured by the task at construction | Only if the caller also holds them |
| Scratch generation state | The task struct itself | **No — dropped with the schedule** |
| Final runtime-ready CPU data | Shared storage the task wrote into | **Yes** |
| GPU-resident resources | The scene/app that registered them | **Yes** |

### 10.1 How products escape — and the mandatory shape

`Runtime::prepare` takes the schedule **by value** and drops it, so a task
cannot be read afterwards. The caller constructs the task around storage it
already holds.

**The product cell must be `Rc<RefCell<Option<T>>>` — never a defaultable bare
`T`**, and a consumer that finds it empty must **fail the phase**, not panic:

```rust
// composition root
let plan: Rc<RefCell<Option<CoursePlan>>> = Rc::new(RefCell::new(None));
schedule.register(
    HandleId::from_raw(1), "burnt-rubber/course", 100,
    Box::new(CompileCourseTask { seed, tuning, out: Rc::clone(&plan) }),
)?;

// a later consumer task, order 200
fn prepare(&mut self) -> RuntimeResult<()> {
    self.plan.borrow().as_ref()
        .ok_or_else(|| RuntimeError::new(
            RuntimeErrorCode::PreparationFailed,
            "road mesh requires the compiled course"))
        .map(|plan| { /* … build from plan … */ })
}
```

This is not style. `order` alone sequences the writes, and nothing in the type
system connects a writer to a reader. With a bare `Vec<T>` cell a premature read
yields an *empty vec* — a plausible-looking value that builds an empty mesh and
renders without erring, detectable only by a golden run that covers one app.
With `Option` a premature read is `None`, and the `ok_or_else` above turns it
into `RuntimeState::Failed` through the normal protocol. A `.expect()` there
would panic straight through `Runtime::prepare`, bypassing §9.4 entirely and
aborting on `wasm32`.

`Rc<RefCell<…>>` rather than a lock: the engine is single-threaded on its
primary target and preparation is explicitly serial (§9.2).

This is what makes **"temporary work can die"** structural. Anything a task
allocated purely in order to generate is dropped at the barrier unless the
caller deliberately kept a handle. The default is discard.

### 10.2 What the runtime must never hold

No `MeshBuffer`, no `TextureBuffer`, no `CoursePlan`, no `Handle<T>`, no generic
product slot, no `Box<dyn Any>`. If the runtime can name a product type it has
learned about rendering or content, and the layer boundary is gone.

---

## 11. Higher-level integration pattern

### 11.1 The composition root does the wiring

`modules/axiom` gains one builder method:

```rust
impl App {
    pub fn prepare_with(mut self, name: &'static str, order: i32,
                        task: Box<dyn PreparationTask>) -> Self;
}
```

**Reserved order band**, so an app cannot collide with the engine's own task:
engine-owned tasks occupy `i32::MIN..0`, app tasks `>= 0`, and `AuthorTask` is
`i32::MIN`. `prepare_with` assigns `HandleId`s from a monotonic counter in call
order (deterministic), never from a hash of `name`.

`RunningApp::realize` becomes — and note the idiom, because `realize` returns
`Self`, not a `Result`, and `modules/axiom` is inside the **branchless spine**
(it contains zero `?;` today and uses `.expect(…)`):

```rust
runtime.initialize().expect("runtime initialize cannot fail");
let mut schedule = PreparationSchedule::new();
// The umbrella's OWN first task: authoring the scene is startup-only work
// producing runtime-ready data, so it is preparation, not a side effect of
// construction. This is the structural fix for the section 2.4 defect.
schedule
    .register(AUTHOR_TASK_ID, "axiom/author", i32::MIN, Box::new(AuthorTask { /* … */ }))
    .expect("the author task is the first registration");
app.preparation
    .into_iter()
    .try_fold((), |(), (id, name, order, task)| schedule.register(id, name, order, task))
    .expect("app preparation ids and orders are unique");
runtime.prepare(schedule).expect("app preparation succeeds");
runtime.start().expect("a prepared runtime starts");
```

The §2.4 ordering defect then becomes **impossible to reintroduce**: `start()`
cannot be called before `prepare()` returns, and authoring is inside `prepare()`.

### 11.2 How modules contribute — the one legal shape

`PreparationTask` is a `runtime`-layer trait, so a trait impl creates no
dependency between implementors. But two constraints bound who can implement it
at all:

1. **Only crates that already declare `runtime`** may name the trait — 3 of 21
   layers, 9 of 44 modules, and every app (§6.2). Adding `runtime` to a
   manifest solely to implement it is a ceremonial dependency and is forbidden.
2. **Module Law #8 blocks a module exporting its task type.** A module's
   `lib.rs` may expose exactly one facade plus its `ids` vocabulary — e.g.
   `modules/axiom-scene/src/lib.rs:64-65` is `pub use ids::SceneNodeId;` and
   `pub use scene_api::SceneApi;` and nothing else. A module therefore **cannot
   name its task type to an app**.

The only legal module shape is a **facade factory returning a trait object**,
which keeps the concrete type private:

```rust
impl SceneApi {
    pub fn preparation_task(&self, /* … */) -> Box<dyn PreparationTask>;
}
```

**For everything else — which is most of the repo — the app wraps the module's
product-producing call in an app-owned task.** That is the §12 pattern, and it
is the primary pattern rather than a fallback. It also means an app-tier task is
where nearly all preparation will live, which §15 must account for.

### 11.3 Engine modules stay isolated

A module implementing `PreparationTask` gains a dependency on `axiom-runtime`
only, and only if it already declares it. `allowed_modules = []` is untouched.
No module gains the ability to see another. The facade factory must not be used
to smuggle product types across the facade — it returns `Box<dyn
PreparationTask>` and nothing else.

---

## 12. Burnt Rubber migration

### 12.1 Classification

**`STARTUP_PREPARED`:**

| # | Work | Current call site | Consumer |
|---|---|---|---|
| P1 | Course compile (`procedural::plan_for` → `CoursePlan`) | `sim/mod.rs:191` | `RaceSim`, ghost, HUD, every render subsystem |
| P2 | The three textures (`asphalt_texture.rs:300`, `verge_texture.rs:145`, `foliage_texture.rs:211` — 96 KB total) | Generated *fused into* `add_texture_data` at `palette.rs:536`, `:539` (in `road_materials`) and `palette.rs:779` (in `ScenePalette::install`) | `RoadMaterials`, `ScenePalette` |
| P3 | Road meshes: 24 `build_draw_mesh` → 96 entities, plus 927 fine paint meshes | `render/chunks.rs:168` via `render/mod.rs:85` | `RaceScene`, the GPU vertex buffers |
| P4 | Prop meshes (`prop_meshes.rs:36,89,159`), `SceneryField::install` pools, `distant_hills` | `render/scenery_pool.rs:74,103` | `SceneryField` |
| P5 | `PlayerCar::install` (×2), `TrafficVisuals::install`, `PickupVisuals::install`, `Effects::install`, `install_finish_arch`, `install_lights` | `render/mod.rs:85,649,1253` | `RaceScene` |
| P6 | Per-chunk scenery props for all 93 chunks — **deferred, and see §12.3** | `render/scenery_pool.rs:155` | `SceneryField` |
| P7 | The traffic wander pair (`wander_phase`, `wander_amount`) folded onto `TrafficPlan` at compile time | `sim/traffic.rs:344-346` | `Traffic::activate` |
| P8 | `DebugView::install` (a mesh, a `Material` per `MarkerKind`, pooled entities) | `app.rs:200` → `debug_view.rs:104-128` | `DebugView` |

P7 is provably byte-identical: `activate` is documented as *"a pure function of
the plan and nothing else"* and the two draws come off a fresh
`Draw::seeded(plan.variation_seed)`, independent of activation order, time and
slot. It is the app's only runtime `Draw::seeded`, so moving it makes the
gameplay path RNG-free.

**`RUNTIME_REQUIRED`** — the §3.4 table, unchanged. Explicitly **not moved:
dynamic traffic behaviour.** The plans are static, course-derived data and are
prepared; the *activation* of a plan into a pool slot, its wander integration,
its yielding and its retirement are gameplay and stay in the frame loop.

**`ALREADY_CORRECT`:** `Track::from_samples` (immutable), `CoursePlan`
(`Arc`-shared, no `&mut self` method exists), `pickups::expand_row` (RNG-free),
`Diagnostics::observe`, the tuning tables. The `TraversalGrid` is **already
transient** — a local inside `validate()`, consumed and dropped before it
returns, stored nowhere (`course/runtime/mod.rs:48` keeps only the report). An
earlier draft listed "dropping the TraversalGrid at the barrier" as deferred
work; it costs zero resident bytes today and there is nothing to do.

### 12.2 Ordered migration sequence

Run `cargo test -p axiom-burnt-rubber --test agent_golden` after **every** step.

1. **M1 — the task type.** Add `apps/burnt-rubber/src/preparation.rs` defining
   `RacePreparation`, holding `Rc<RefCell<Option<…>>>` product handles per
   §10.1. Dead code with unit tests. *Golden: unchanged.*
2. **M2 — the course compile (P1).** `RaceSim::from_plan` (`sim/mod.rs:202`)
   already takes `(Arc<CoursePlan>, Tuning, PlayProfile)` — exactly the
   signature this needs, so it is re-plumbing rather than a rewrite. **Also
   fixes the four-compiles problem** (§3.3): the ghost (`app.rs:319`) takes an
   `Arc` clone rather than compiling its own. **And remove the two gratuitous
   371 KB `Track` clones** at `sim/mod.rs:207` and `compiler/mod.rs:197`, which
   are on the same path.
   > **Load-bearing and previously unwritten:** `plan_for(seed, &tuning)` reads
   > `tuning.course`, `tuning.race` and `tuning.vehicle` (`compiler/mod.rs:192`,
   > `:292`, `:413-414`) and does **not** read `tuning.camera` — which is the
   > only field `with_profile` rewrites (`app.rs:180`, `framed_for_aspect`). So a
   > plan prepared before the window is sized is bit-identical to today's, and
   > M5 is safe. If a future change moves an aspect- or device-derived value into
   > `tuning.course`/`race`/`vehicle`, preparation silently breaks.
   *Golden: unchanged.*
3. **M3 — the textures (P2).** All three are argument-free constants. The
   generators are currently *fused into* the `add_texture_data` calls, so
   splitting generation from upload changes the signatures of
   `palette::road_materials` and `ScenePalette::install`. *Golden: render and
   resource bytes unchanged.*
4. **M4 — the mesh installs (P3, P4, P5, P8).** `RaceScene::install`
   (`render/mod.rs:85`) splits: CPU geometry build becomes preparation,
   `RunningApp::add_mesh_data` registration stays where the scene is assembled.
   This changes the signatures of `RaceScene::install`, `RoadChunks::install`
   and `SceneryField::install`. **Registration order must be preserved** — mesh
   ids are assigned in registration order and appear in the render goldens.
   *Golden: render and resource bytes unchanged.*
5. **M5 — wire the composition root.** `apps/burnt-rubber/src/web.rs:99` and
   `BurntRubber::with_profile` (`app.rs:158`) route through `App::prepare_with`
   and `Runtime::prepare`. `BurntRubber::with_profile` **keeps its signature**,
   so all 861 lib tests and every capture slice compile untouched. *Golden:
   unchanged.*
6. **M6 — the traffic wander pair (P7).** Fold `wander_phase`/`wander_amount`
   onto `TrafficPlan` at compile time. *Golden: unchanged — assert it, since
   this is the one migration step that touches the simulation.*
7. **M7 — prove the barrier.** The app tests of §17.2.

### 12.3 Deferred, with the correct reasons

- **P6 (all-chunk scenery pre-generation).** An earlier draft deferred this "because
  it changes resident memory". **That reason does not survive measurement.**
  `PropInstance` (`scenery.rs:153-161`) is 32 B; the pool-capacity ceiling is
  2 504 props per 17-chunk window (`scenery.rs:82-101`, proven sufficient by
  `the_pool_capacities_cover_what_the_active_range_generates`), which scaled over
  93 chunks bounds the whole course at ~13 700 props ≈ **438 KB**, with a
  realistic estimate near **237 KB** — *less than one `Track`* (371 KB, two of
  which are resident today). The real reasons to defer are:
  1. **The saving is small.** `SceneryField::refresh` (`scenery_pool.rs:138-161`)
     does **not** regenerate the window; it retains what stayed (`:150`) and
     generates only chunks that *entered* (`:151-157`) — one chunk per range
     advance, roughly once a second. That is the entire cost P6 removes.
  2. **Doing it naively is a per-frame regression.** `SceneryField::pose`
     (`:164-219`) is `O(cached props)` every frame. Holding all 93 chunks in
     `cache` makes `pose` ~5.5× more expensive on *every frame* to save one
     chunk's generation per second.
  When P6 is done, it must be specified as: *prepare a chunk-indexed
  `Vec<Vec<PropInstance>>` store; `refresh` becomes a slice lookup into it;
  `pose` continues to iterate only the active window.*
- **Adopting the engine proc stack.** Burnt Rubber's hand-rolled `Draw` and seed
  partitioning re-derive what `axiom-entropy`'s `(seed, Address, version)` model
  provides, and its textures are what `ProcTextureApi::bake` produces.
  Converging them is real, separate work. **Not part of this manifest** — doing
  it here would move the goldens for reasons unrelated to the lifecycle,
  destroying the evidence this plan depends on.

---

## 13. Golden regression integration

| Point | Command | Expected |
|---|---|---|
| **B — Baseline** (done, §4) | `cargo test -p axiom-burnt-rubber --test agent_golden` | Green, 15 committed goldens |
| **1 — After the runtime primitive** (T1–T6) | The same, plus `cargo test -p axiom-runtime` | Green, **unchanged bytes** |
| **2 — After T8** (umbrella reorder) | The same | Green, **unchanged bytes**. A diff here means the reorder changed authoring |
| **3 — After each of M2…M6** | The same, after every step | Green, **unchanged bytes**, every time |
| **4 — After the full migration** | The same, plus both pixel arms compared to §4.8 | Unchanged bytes; canvas2d byte-identical; GPU per §17.3 |
| **5 — Before landing** | The full §24 list | All green |

**Re-blessing is forbidden during this work.** A state-byte move means the
simulation changed; a render-byte move means the scene changed; a resource-byte
move means the *generated geometry or textures* changed — which is precisely
what M3/M4 touch. Any of the three is a bug in the migration. `AXIOM_REGOLD`
must not be run at any point in this plan.

**The goldens cannot prove the migration succeeded, only that it broke nothing**
(§4.10). Success is proved by the counter assertions in §17.2 and the
measurements in §18.

---

## 14. Unit tests

`crates/axiom-runtime` is inside the Coverage Law's scope: every region, line,
branch and function of the new code ships covered in the same change. ★ marks
tests that would fail if the barrier were removed.

### 14.1 Lifecycle (`crates/axiom-runtime/src/runtime.rs` tests)

| Test | Asserts |
|---|---|
| `preparation_runs_before_running` ★ | After `initialize(); prepare(schedule)` the state is `Prepared` and every task ran exactly once, in `order` (proved via a shared trace, as `runtime_scheduler.rs:264`) |
| `running_cannot_begin_before_preparation_completes` ★ | `initialize(); start()` returns `InvalidLifecycleTransition`; the state stays `Initialized` |
| `successful_preparation_permits_the_transition` ★ | `initialize(); prepare(..); start()` reaches `Running` and `step()` then succeeds |
| `failed_preparation_blocks_the_transition` ★ | A failing task leaves the state `Failed`, `prepare` returns `PreparationFailed`, and `start()` then returns `InvalidLifecycleTransition` |
| `a_failing_task_stops_the_remaining_tasks` | With tasks at orders 1/2/3 and #2 failing, the trace holds two entries, not three |
| `the_error_names_the_failing_task` | The returned `RuntimeError`'s message is the failing task's registered `&'static str` |
| `preparation_runs_exactly_once_per_launch` ★ | A second `prepare()` from `Prepared` returns `InvalidLifecycleTransition`; its tasks never run |
| `an_empty_schedule_prepares_immediately` | Reaches `Prepared` — the migration path for every existing caller |
| `stepping_does_not_rerun_preparation` ★ | After `Running`, 100 `step()` calls leave a counting task's run count at exactly 1 |
| `preparation_is_rejected_before_initialize` | `prepare()` from `Created` |
| `preparation_is_rejected_from_terminal_states` | From `Stopped` and from `Failed` |
| `stop_is_legal_from_prepared` | `Prepared → Stopped` |
| `pause_and_resume_do_not_reenter_preparation` | `Running → pause() → start()` works from `Paused` with no schedule; no task re-runs |
| `a_failed_preparation_leaves_the_step_gate_closed` | `step()` from `Failed` returns `StepWhileNotRunning` |

### 14.2 Ordering and declaration (`preparation_schedule.rs` tests)

| Test | Asserts |
|---|---|
| `tasks_run_in_ascending_order_not_registration_order` ★ | Registering 30, 10, 20 runs them 10, 20, 30 (via the trace) |
| `a_duplicate_task_id_is_rejected` | `DuplicateSystemId`; after `prepare` the trace has 2 entries, not 3 |
| `a_duplicate_order_is_rejected` | `DuplicateSystemOrder` |
| `negative_and_extreme_orders_sort_correctly` | `i32::MIN`, `-1`, `0`, `i32::MAX` — covers the reserved band of §11.1 |
| `the_schedule_and_its_debug_are_constructible` | `new()`, `Default::default()`, and the hand-written `Debug` impl (a region the Coverage Law will otherwise flag) |

### 14.3 Deterministic output (`crates/axiom-runtime/tests/preparation.rs`)

| Test | Asserts |
|---|---|
| `equivalent_inputs_produce_equivalent_prepared_output` ★ | Two runtimes, two identically-seeded schedules of the same deterministic tasks, produce byte-equal products |

This proves the *runtime* is deterministic given deterministic tasks. It cannot
prove a task is one — only an app-level replay can (§17.2), because `apps/` is
outside every mechanical gate (§15.2).

### 14.4 Ownership and discard (`crates/axiom-runtime/tests/preparation.rs`)

| Test | Asserts |
|---|---|
| `temporary_preparation_data_is_discarded_at_the_barrier` ★ | A task holding a `Drop`-counting scratch value has it dropped by the time `prepare()` returns, while the product it wrote survives |
| `products_reach_the_caller_without_passing_through_the_runtime` | The caller's `Rc<RefCell<Option<_>>>` holds the product after `prepare()`; `Runtime` exposes no accessor for it |
| `a_task_that_reads_an_unwritten_product_fails_the_phase` | A consumer registered *before* its producer finds `None`, returns `Err`, and the runtime lands in `Failed` — never a panic (§10.1) |

### 14.5 Existing tests updated, not weakened

`runtime_state.rs:41` `discriminants_are_stable_and_ordered` is extended to pin
all seven discriminants, renamed to drop "ordered", and its
`Created < Running` assertion replaced with `assert_eq!(RuntimeState::Prepared.raw(), 6)`
(the derive is removed, §7.2). Every in-crate `initialize(); start()` gains
`prepare(PreparationSchedule::new())?`.

---

## 15. Architecture tests

No existing check is weakened.

### 15.1 Curated public surface (new)

`crates/axiom-runtime` has no `tests/architecture.rs` today. Add one modelled on
`crates/axiom-kernel/tests/architecture.rs`, asserting the two new symbols are
the *only* additions — so a future agent cannot quietly widen the preparation
API back to the five the first draft proposed.

### 15.2 The layer manifest (extended)

`layer.toml` gains `PreparationTask` and `PreparationSchedule` in
`introduced_capabilities`, plus a `[[proof_exports]]` for `PreparationSchedule`
with `must_reference = ["HandleId"]`. `cargo xtask check-architecture` then
enforces both. Nothing is relaxed.

### 15.3 The barrier's accepted set (new, behavioural)

An earlier draft specified a test that "the only public path to
`RuntimeState::Running` is `start()`" — a whole-crate reachability property a
`#[test]` cannot observe. Split into what is actually writable:

1. **Behavioural:** `start()` errs from `Created`, `Initialized`, `Stopped` and
   `Failed`, and succeeds from `Prepared` and `Paused`. That *is* "the accepted
   set is exactly `{Prepared, Paused}`".
2. **Structural (optional):** an xtask source scan over
   `crates/axiom-runtime/src/**` asserting `RuntimeState::Running` is assigned
   in exactly one place.

### 15.4 No anti-leakage text scan — and why

An earlier draft proposed an `xtask` hygiene scan rejecting a type that
implements both `RuntimeSystem` and `PreparationTask`. **Do not build it.**

- `xtask` scans source **as text** with `//` comments stripped. "Type X
  implements traits A and B" is a type-resolution question; the scan is defeated
  by putting the two impls in different files (legal Rust), by a generic impl,
  or by a type alias. `CLAUDE.md` states the ownership split explicitly: xtask
  owns whole-graph structure, **dylint owns per-crate genuine-use semantics**.
- `hygiene::check` iterates only layer and module source dirs
  (`crates/xtask/src/hygiene.rs:74-87`), and per §11.2 nearly every
  `PreparationTask` impl will live in `apps/` — which the scan cannot see, and
  which is outside the dylint rulebook, the coverage gate and the branchless
  gate entirely (`tools/lints/engine_lint_helpers/src/lib.rs:44` requires a
  `crates/` or `modules/` component).

Shipping a check that advertises enforcement it cannot deliver is itself a
check-shaped shortcut. **The mechanical guarantee is the §8.1 trait-signature
split, which makes the mistake a compile error.** If a semantic check is later
wanted, it belongs in `tools/lints` as a dylint alongside `engine_no_branching`,
where `DefId` resolution makes it sound — a separate, larger decision.

### 15.5 `check-slices` in CI

`cargo run -p xtask -- check-slices` is **not** currently a CI step — only
`check-architecture` is. The golden run's 15 SHA-256 pins are therefore
unenforced by automation. Add it to the `check` job. (It *is* wired into
`cargo test --workspace` via `real_repo_slices_pass`, so the hole is narrower
than it looks — but see R6: CI is `workflow_dispatch`-only regardless.)

---

## 16. Integration tests

The smallest test proving a *generic* application can construct → prepare →
obtain a runtime-ready result → run → step, with no engine-domain concept in it.

**`crates/axiom-runtime/tests/preparation_lifecycle.rs`:**

```rust
// A generic "expensive" startup product: a deterministic table from a seed.
// Stands in for a course, a mesh set or a texture atlas without the runtime
// learning any of those words. Note `Option` per section 10.1.
struct BuildTable { seed: u64, out: Rc<RefCell<Option<Vec<u64>>>> }

impl PreparationTask for BuildTable {
    fn prepare(&mut self) -> RuntimeResult<()> {
        let mut rng = DeterministicRng::seeded(self.seed);
        *self.out.borrow_mut() = Some((0..1024).map(|_| rng.next_u64()).collect());
        Ok(())
    }
}

#[test]
fn a_generic_application_prepares_then_runs() {
    let table = Rc::new(RefCell::new(None));
    let mut runtime = Runtime::new(RuntimeConfig::new(16_666_667)).unwrap();
    runtime.initialize().unwrap();

    let mut schedule = PreparationSchedule::new();
    schedule.register(HandleId::from_raw(1), "table", 0,
        Box::new(BuildTable { seed: 42, out: Rc::clone(&table) })).unwrap();

    assert!(table.borrow().is_none(), "nothing is built before prepare");
    assert!(runtime.step().is_err(), "and the frame loop is closed");

    runtime.prepare(schedule).unwrap();
    assert_eq!(runtime.state(), RuntimeState::Prepared);
    assert_eq!(table.borrow().as_ref().unwrap().len(), 1024, "runtime-ready");

    runtime.start().unwrap();
    (0..10).for_each(|_| { runtime.step().unwrap(); });
    assert_eq!(table.borrow().as_ref().unwrap().len(), 1024, "stepping did not rebuild it");
}
```

Plus, in the same file:

- `the_frame_loop_is_closed_until_prepared` — `step()` errs at `Created`,
  `Initialized` and `Prepared`; succeeds only at `Running`.
- `a_generic_application_with_a_failing_task_never_runs` — the product is
  absent, the state is `Failed`, `start()` and `step()` both err.

---

## 17. Burnt Rubber regression tests

### 17.1 The golden run, unchanged

`cargo test -p axiom-burnt-rubber --test agent_golden` is the primary regression
test and **its assertions must not be edited** by this work.

### 17.2 New app tests (`apps/burnt-rubber/tests/preparation.rs`)

| Test | Asserts |
|---|---|
| `the_race_is_playable_only_after_preparation` ★ | The course, road meshes and textures all exist at the instant the app first becomes steppable |
| `the_course_is_compiled_exactly_once_per_launch` ★ | A counting wrapper shows **one** compile, not the four of §3.3. **This test would have failed before the migration — it is the measurable win, and the goldens cannot express it** |
| `a_restart_does_not_recompile_the_course` | `start_race()` reuses the prepared `Arc<CoursePlan>` |
| `the_ghost_shares_the_prepared_course` | `Arc::ptr_eq` with the player's |
| `preparation_failure_is_surfaced_not_swallowed` | An invalid course spec leaves the runtime `Failed` and never presents a frame |
| `two_preparations_from_the_same_seed_produce_identical_products` | Run `prepare()` twice from scratch; compare the `CoursePlan` and the three texture `Vec<u8>`s. **This is the only mechanical determinism check the app tier gets** (§15.4) |

### 17.3 Determinism criteria

| Arm | Criterion | Source |
|---|---|---|
| Golden state bytes | **Byte-identical** to baseline | `tests/agent_golden.rs` |
| Golden render bytes | **Byte-identical** | same |
| Golden resource bytes | **Byte-identical** | same |
| Canvas 2D pixels | **Byte-identical** (`Tolerance::EXACT`) | `apps/axiom-growth/src/visual_target/compare.rs:37` |
| GPU pixels | `mean ≤ 2.0`, `max ≤ 40` (`Tolerance::GPU_DEFAULT`) | same, `:45` |

**On the GPU arm, be precise about what is being claimed and what tool can claim
it.** §24's `sha256sum` is an *exact* comparison and cannot express
`mean ≤ 2 / max ≤ 40`; the only tolerance comparator in the repo
(`compare::compare_rgba`) lives inside `apps/axiom-growth` behind its
`visual-target` feature and is not reachable from a Burnt Rubber check. So:

- **On the development machine**, GPU captures are compared with `sha256sum` and
  must be byte-identical, matching §4.8. This is a strictly *stronger* check
  than the policy requires and is what the migration should be held to locally.
- **On any other machine**, byte-identity is not expected (§4.8) and there is
  currently **no tool to apply `Tolerance::GPU_DEFAULT`** to these captures.
  Extracting `compare.rs` into `tools/axiom-shot` would provide one; it is
  listed as a **non-goal** for this work (§22.12) precisely so this change stays
  scoped, which means the portable GPU criterion is **deferred, not silently
  assumed**. Do not write an acceptance criterion that no command can check.

### 17.4 Failure diagnostics

1. **Which artifact moved?** State → the simulation changed. Render → the scene
   or the look changed. Resources → the *generated geometry or textures*
   changed, which implicates M3/M4 directly.
2. **Which checkpoint moved first?** `grid` implicates construction/authoring;
   `opening` onward implicates something that manifests only once driving.
3. **Bisect by migration step.** Each of M2…M6 is independently revertible and
   each was green when it landed.

---

## 18. Performance instrumentation

### 18.1 The clock constraint, stated accurately

An earlier draft claimed *"`Runtime::step` is inside a `#[sim]` zone and the
dylint rulebook bans wall-clock time in a `#[sim]` zone"*, implying the crate is
protected. **The zone is far narrower than that.**

- `crates/axiom-runtime/src/runtime.rs:143` is the **only** `axiom_zones`
  attribute in the entire crate (verified by grep).
- `engine_lint_helpers::in_zone` (`tools/lints/engine_lint_helpers/src/lib.rs:82`)
  matches only `ItemKind::Fn` and *inline* `ItemKind::Mod`. `runtime.rs` is a
  file module, so the marker's reach is **`step`'s four-line body and nothing
  else**. `Runtime::run_one_step` — which contains all the actual per-step
  work — is **not** in the zone.
- `Runtime::prepare` would likewise be entirely outside it. Nothing would stop a
  future agent adding `Instant::now()` there.

**Mitigations, both required:** (a) deleting `PreparationReport` (§8.3) removes
the place a duration would naturally live; (b) T4 adds `#[axiom_zones::sim]` to
both `Runtime::prepare` and `Runtime::run_one_step`, so the ban actually covers
them.

### 18.2 Where each measurement is taken

| Measurement | Where | Mechanism (all pre-existing) |
|---|---|---|
| **Total preparation duration** | The host/tooling boundary, wrapping `prepare()` | `Instant` natively, `performance.now()` via windowing on wasm. Precedent: `apps/axiom-proc-player/src/room.rs:34` already records "wall-clock microseconds spent baking" |
| **Individual task duration** | The app that owns the task, inside its own `prepare` | Same. The runtime never times a task |
| **First playable frame** | The composition root | The frame index at which `RunningApp` first ticks after `start()` — already observable as `BurntRubber.frame` (`app.rs:116`) |
| **Gameplay frame-time, before and after** | The existing in-game overlay | `src/telemetry.rs` — a 240-frame rolling window reporting **median** and worst. Compare medians on the same device, same course, same checkpoint |
| **Did generation leave the frames?** | The existing counters | `src/diagnostics.rs` — `Diagnostics::rows()`: `active_chunks`, `road_triangles`, `scenery_instances`, `effect_instances`, `active_traffic`, `simulation_steps` |
| **Course compile count** | `the_course_is_compiled_exactly_once_per_launch` (§17.2) | A test, not a timing — the most reliable evidence the migration worked |
| **Native frame cost, pinned frame** | `tools/axiom-shot` | `--profile-frames N` (setup-cancelling), `--profile-compare`, `--profile-sizes` |
| **Peak preparation memory** | **No infrastructure exists** | The only memory gate is End Zone's `leakcheck`, which measures per-frame growth, not peak. Do not invent one here; record the gap (R5) |

### 18.3 The measurement protocol

- **Never A/B across processes on this machine.** `tools/axiom-shot/src/main.rs:126-134`
  records two consecutive runs of the *same* slice at 3.29 ms and 13.52 ms — a
  4× swing from GPU clock drift. Use `--profile-compare` / `--profile-sizes`,
  which interleave inside one process so drift is common-mode and cancels.
- **Frame-time comparisons must be medians over a window**, which is what
  `telemetry.rs` already reports.

### 18.4 What to record when the work lands

Total preparation ms (native and browser), before/after median gameplay frame
time at the `canyon` checkpoint, the course-compile count (4 → 1), and the
`Track` copy count (4 → 2). Label everything as measured.

---

## 19. File-by-file implementation plan

### Created

| Path | Contents |
|---|---|
| `crates/axiom-runtime/src/preparation_task.rs` | The trait + doc contract + unit tests |
| `crates/axiom-runtime/src/preparation_schedule.rs` | `PreparationSchedule`, `Default`, hand-written `Debug`, private `Registered` entry, `execute`, unit tests |
| `crates/axiom-runtime/tests/preparation.rs` | Determinism + ownership/discard tests (§14.3, §14.4) |
| `crates/axiom-runtime/tests/preparation_lifecycle.rs` | The generic-application test (§16) |
| `crates/axiom-runtime/tests/architecture.rs` | Curated-surface + accepted-set tests (§15.1, §15.3) |
| `apps/burnt-rubber/src/preparation.rs` | `RacePreparation` and its product handles |
| `apps/burnt-rubber/tests/preparation.rs` | The app regression tests (§17.2) |

### Modified

| Path | Change |
|---|---|
| `crates/axiom-runtime/src/lib.rs` | Two `pub use` + two `mod`; update the module-doc lifecycle sentence |
| `crates/axiom-runtime/src/runtime_state.rs` | `+ Prepared = 6`; drop `PartialOrd, Ord`; update the discriminant test |
| `crates/axiom-runtime/src/runtime_error_code.rs` | `+ PreparationFailed = 8`; extend its test |
| `crates/axiom-runtime/src/runtime.rs` | Add `prepare()` + `run_preparation()`; `start()` accepts `{Prepared, Paused}`; `stop()` also accepts `Prepared`; add `#[axiom_zones::sim]` to `prepare` and `run_one_step` (§18.1); update in-file tests |
| `crates/axiom-runtime/layer.toml` | `introduced_capabilities += 2`; one new `[[proof_exports]]` |
| `crates/axiom-runtime/ARCHITECTURE.md` | Update the lifecycle sentence at `:8`; add the preparation section and why products never pass through the runtime |
| `modules/axiom/src/app.rs` | `App::prepare_with`; an `App.preparation` field; `realize` reordered to `initialize → prepare(author + app tasks) → start`; `AuthorTask` extracted from `Self::author`. **Branchless — no `?`; use `.expect(…)`/`try_fold`** |
| `apps/burnt-rubber/src/lib.rs` | `pub mod preparation;` |
| `apps/burnt-rubber/src/sim/mod.rs` | Construct from a prepared `Arc<CoursePlan>` via `from_plan` (`:202`); drop the discarded `Track` clone (`:207`) |
| `apps/burnt-rubber/src/course/compiler/mod.rs` | `geometry.samples` moved rather than cloned (`:197`) |
| `apps/burnt-rubber/src/course/traffic/flow.rs`, `encounters.rs` | Fold the wander pair onto `TrafficPlan` (P7) |
| `apps/burnt-rubber/src/sim/traffic.rs` | `activate` reads the pre-drawn wander pair (`:344-346`) |
| `apps/burnt-rubber/src/app.rs` | `restart_ghost` (`:317`) and `start_race` (`:284`) reuse the prepared plan |
| `apps/burnt-rubber/src/render/mod.rs` | `RaceScene::install` (`:85`) split; signature changes |
| `apps/burnt-rubber/src/render/palette.rs` | `road_materials` (`:534`) and `ScenePalette::install` (`:726`) take prepared texture bytes |
| `apps/burnt-rubber/src/render/chunks.rs`, `scenery_pool.rs`, `prop_meshes.rs`, `debug_view.rs` | Build/install split |
| `apps/burnt-rubber/src/web.rs` | The wasm entry (`:99`) routes through preparation and times it |
| `apps/burnt-rubber/TESTING.md` | Document the preparation phase and the new tests |
| `.github/workflows/ci.yml` | Add `cargo run -p xtask -- check-slices` |
| **Every other `initialize(); start()` site** | Insert `prepare(PreparationSchedule::new())?`. 23 sites; find with `rg -n '\.initialize\(\)' --glob '!target'`. Includes test code in `crates/axiom-frame`, `crates/axiom-host`, `crates/axiom-introspect` (§6.2) |

### Deleted

None.

---

## 20. Ordered task graph

Sequential. Do not start the next task until the stated criterion is met.

**T1 — `RuntimeState::Prepared`.** *Files:* `runtime_state.rs`. Append
`Prepared = 6`; drop the `PartialOrd, Ord` derive; document `raw()` as identity;
replace the `Created < Running` assertion with
`assert_eq!(RuntimeState::Prepared.raw(), 6)` and pin all seven discriminants.
*Validation:* `cargo test -p axiom-runtime`. *Done when:* all seven pinned, crate
still branchless and fully covered.

**T2 — `PreparationTask`.** *Files:* `preparation_task.rs`, `lib.rs`. The
zero-argument trait per §8.1. *Validation:* `cargo test -p axiom-runtime`;
`bash scripts/dylint-gate.sh`. *Done when:* dylint at baseline, file 100% covered.

**T3 — `PreparationSchedule`.** *Files:* `preparation_schedule.rs`, `lib.rs`.
Mirror `runtime_scheduler.rs` including the branchless duplicate-detection idiom,
sort-on-insert, the hand-written `Debug`, and `execute` returning
`Option<&'static str>`. *Tests:* §14.2. *Done when:* ordering, both duplicate
rejections, the extreme orders and the `Debug` impl are covered.

**T4 — `Runtime::prepare` and the barrier ★.** *Files:* `runtime.rs`,
`runtime_error_code.rs`. Add `PreparationFailed = 8`; implement per §9.8;
`start()` accepts `{Prepared, Paused}`; `stop()` also accepts `Prepared`; add
`#[axiom_zones::sim]` to `prepare` and `run_one_step`. Update in-crate tests.
*Tests:* all of §14.1. *Done when:*
`running_cannot_begin_before_preparation_completes` and
`failed_preparation_blocks_the_transition` pass, and no runtime test reaches
`Running` without a `prepare()`.

**T5 — Manifest, architecture doc, architecture tests.** *Files:* `layer.toml`,
`ARCHITECTURE.md`, `tests/architecture.rs`. §15.1–15.3. *Validation:*
`cargo run -p xtask -- check-architecture`. *Done when:* the checker passes with
two new capabilities declared and the curated-surface test locks exactly two.

**T6 — Integration + ownership tests.** *Files:* `tests/preparation.rs`,
`tests/preparation_lifecycle.rs`. §14.3, §14.4, §16 — including
`a_task_that_reads_an_unwritten_product_fails_the_phase`. *Done when:* all pass
and the crate is at 100%.

**T7 — Update all 23 `initialize(); start()` sites.** *Details:* insert
`prepare(PreparationSchedule::new())?`. **Do not** work around the barrier by
re-admitting `Initialized`. *Validation:* `cargo test --workspace` **and**
`cargo run -p xtask -- check-architecture` (do not assume the three test-only
crates of §6.2 are unaffected). *Done when:* the workspace is green and the
checker passes.

**T8 — Composition root: authoring becomes preparation ★.** *Files:*
`modules/axiom/src/app.rs`. Extract `AuthorTask` at order `i32::MIN`; add
`App::prepare_with` with the reserved band and monotonic `HandleId`s; reorder
`realize`. **Branchless — no `?`.** *Tests:* `realize` leaves the runtime
`Running` with a non-empty authored scene, and the author task ran before
`start()`. *Validation:* `cargo test --workspace`; **the golden run**. *Done
when:* golden bytes **unchanged** (checkpoint 2 of §13).

**T9 — BR M1: the task type.** *Files:* `apps/burnt-rubber/src/preparation.rs`,
`src/lib.rs`. *Done when:* unit-tested and the golden run is unchanged.

**T10 — BR M2: the course compile ★.** Per §12.2 step 2, including the two
`Track` clones. *Tests:* `the_course_is_compiled_exactly_once_per_launch`,
`the_ghost_shares_the_prepared_course`, `a_restart_does_not_recompile_the_course`.
*Done when:* golden bytes **unchanged** and the compile count is 1.

**T11 — BR M3: the textures.** *Done when:* render **and resource** bytes
unchanged.

**T12 — BR M4: the mesh installs.** Preserve registration order. *Done when:*
render **and resource** bytes unchanged.

**T13 — BR M5: wire the composition root.** *Done when:* the full app suite and
the golden run are green and unchanged.

**T14 — BR M6: the traffic wander pair.** *Done when:* golden bytes unchanged —
assert it explicitly; this is the one step that touches the simulation.

**T15 — BR M7: prove the barrier at app level.** §17.2. *Done when:* all six
tests pass.

**T16 — Pixel re-verification.** Capture all five checkpoints on both backends;
compare to §4.8 per §17.3. *Done when:* both arms match and results are recorded.

**T17 — Instrumentation and the landing record.** §18.4. *Done when:* the
numbers are in the document, labelled as measured.

**T18 — Full gate run and land.** Every command in §24, one at a time. *Done
when:* all green.

---

## 21. Risks

| # | Risk | Evidence | Mitigation |
|---|---|---|---|
| **R1** | **The barrier does not gate `RunningApp::render`.** An app that owns its own loop can present unprepared. | `modules/axiom/src/app/frame.rs:102` — public, "safe to call standalone", zero `runtime` references. Also `HostStepDriver::drive` returns `Ok` without stepping when `steps() == 0` | §1.1 states the guarantee honestly. Burnt Rubber presents via `tick` (`app.rs:532`) so the vertical proof is unaffected. If presentation gating is later wanted it is a **second consumer** of `RuntimeState::Prepared`, not a relocation of the phase |
| **R2** | **`Prepared` means "someone called prepare", not "the right work was declared".** A zero-task schedule satisfies it, and T7 mandates exactly that at 23 sites. | §7.4, §14.1 | Accepted and documented (§1.1). The mandatory contract is `modules/axiom`'s `AuthorTask`, in the composition root. Do not describe the runtime as the guarantor |
| **R3** | **Mesh registration order changes during M4, moving render *and* resource goldens.** | Mesh ids are assigned in registration order and encoded in both artifacts | T12 preserves order explicitly; the golden run is the detector; re-blessing is forbidden |
| **R4** | **A long synchronous `prepare()` blocks the browser main thread.** | `src/web.rs:52` is one synchronous wasm entry; every app ships a `loading…` div the browser cannot animate | **Status quo, not a regression** (§2.4). T17 quantifies it. A budgeted model would be the answer, but the primitive that was cited here (`axiom_proc::Evaluation`) has since been **deleted** — so this is now an open problem with nothing behind it, out of scope (§9.3) |
| **R5** | **No peak-memory infrastructure exists.** | Only End Zone's `leakcheck`, which measures per-frame growth | Record the gap. It becomes load-bearing only for the deferred P6 |
| **R6** | **CI is `workflow_dispatch`-only since 2026-07-14** — none of these gates run automatically. | `.github/workflows/ci.yml` | Run all gates locally before pushing. **The implementing agent must not assume CI will catch anything** |
| **R7** | **Two gates run concurrently exhaust memory on this machine**, after which dylint reports a *fake* `cargo metadata` error that masks the real finding. | `link.exe 0xc0000142` is the OOM signature | Run gates strictly one at a time |
| **R8** | **The golden run is slow** (~117 s), so an agent may skip it between migration steps. | Measured | It is the only detector for R3. Run it after *every* step of T9–T15 |
| **R9** | **`prepare()` becomes a junk drawer** — progress callbacks, budgets, priorities, a DAG. | Generic pressure on any lifecycle API | §8.5 lists the rejected surface; T5's curated-surface test makes widening it a test failure. The first draft already drifted to five symbols; that is evidence the pressure is real |
| **R10** | **`RaceScene::install` is entangled** — it both builds CPU geometry and registers it. | `render/mod.rs:85` | T12 splits along that seam. If the split cannot preserve registration order, **stop and reshape** rather than accept a golden diff |
| **R11** | **App-tier tasks are outside every mechanical gate.** An app `PreparationTask` seeded from `Instant::now()` violates no check. | `tools/lints/engine_lint_helpers/src/lib.rs:44` requires `crates/`/`modules/` | §17.2's `two_preparations_from_the_same_seed_produce_identical_products` is the only detector; it is mandatory, not optional |

---

## 22. Explicit non-goals

1. **No offline asset baking.**
2. **No persistent asset cache** — no disk, IndexedDB, localStorage.
3. **No arbitrary disk artifacts.** The only files added are source, tests, the
   committed goldens (already landed), and this document.
4. **No generic asset pipeline.** `modules/axiom-assets` is untouched.
5. **No runtime streaming system.** `modules/axiom-streaming` is untouched.
6. **No WebGPU or platform concept in a lower portable layer.**
7. **No procedural road, terrain, mesh, texture, traffic or racing concept in
   the generic lifecycle layer.** The runtime cannot name a product type.
8. **No replacement of dynamic runtime generation that must stay dynamic**
   (§3.4).
9. **No `.axpkg`, no packer, no manifest format.**
10. **No concurrency, no budgeting, no cancellation** in v1 — and note
    concurrency is *architecturally excluded* by §10, not merely deferred (§9.2).
11. **No convergence of Burnt Rubber onto the engine proc stack** (§12.3).
12. **No `axiom-shot` pixel-comparator extraction.** Consequence, stated
    explicitly rather than hidden: the portable GPU tolerance criterion cannot be
    checked by any command, so §17.3 and §23 hold the GPU arm to
    development-machine byte-identity only.
13. **No anti-leakage text scan in xtask** (§15.4) — if wanted, it is a dylint,
    separately.
14. **No unrelated refactors** — not the two parallel recipe cores, not the
    `AXIOM_REGOLD`/`AXIOM_UPDATE_GOLDEN` naming inconsistency, not P6 (§12.3).
15. **No weakening of any existing check.** No `#[allow]`, no
    `#[coverage(off)]`, no ignore-pattern widening, no tolerance widening, no
    golden re-blessing.

---

## 23. Acceptance criteria

### Behavioural

1. `RuntimeState` has `Prepared`; `start()` accepts exactly `{Prepared, Paused}`.
2. `initialize(); start()` returns `InvalidLifecycleTransition`.
3. A failing task leaves the runtime `Failed`; `start()` and `step()` both err;
   no frame is presented; the error names the failing task.
4. Preparation runs **exactly once per launch**: a second `prepare()` is
   rejected, and 100 `step()` calls do not re-run a task.
5. A generic application can construct → prepare → obtain a product → run → step
   with no engine-domain concept in the test (§16).
6. Scratch data held by a task is dropped by the time `prepare()` returns; the
   product survives; a consumer reading an unwritten product **fails the phase
   rather than panicking**.

### Regression — the load-bearing criterion

7. **All 15 Burnt Rubber golden files are byte-identical to the §4.6 baseline.**
   No `AXIOM_REGOLD` run occurred at any point.
8. **All five checkpoints render byte-identically on Canvas 2D.**
9. **All five checkpoints render byte-identically on GPU on the development
   machine**, matching §4.8. Portable GPU tolerance is explicitly out of scope
   (§22.12) — do not claim it.
10. `cargo test -p axiom-burnt-rubber` is green: 861+ lib tests, every capture
    slice, `agent_race`, `course_pipeline`, `agent_golden`.

### Structural

11. `cargo run -p xtask -- check-architecture` passes with two new capabilities
    declared.
12. `cargo run -p xtask -- check-slices` passes; all 15 pins match.
13. `bash scripts/coverage.sh` reports **100.00%** regions/lines/functions,
    including the hand-written `Debug` impl.
14. `bash scripts/dylint-gate.sh` at or under baseline, `engine_no_branching`
    still **0**.
15. `cargo test --workspace` green.
16. `crates/axiom-runtime` exposes **exactly two** new public symbols, locked by
    its curated-surface test.

### Measurable

17. The Burnt Rubber course is compiled **once** per launch (was four times per
    construction+restart cycle), proved by
    `the_course_is_compiled_exactly_once_per_launch`.
18. Total preparation duration and before/after median gameplay frame time at
    `canyon` are recorded in §18.4, labelled as measured.

---

## 24. Validation commands

**Run one at a time** — two concurrent gates exhaust memory on this machine and
dylint then reports a fake `cargo metadata` error that masks the real finding
(R7).

```sh
# --- unit + workspace tests ---------------------------------------------
cargo test -p axiom-runtime
cargo test -p axiom-burnt-rubber
cargo test -p xtask
cargo test --workspace

# --- the Burnt Rubber golden regression (the primary detector) ----------
cargo test -p axiom-burnt-rubber --test agent_golden -- --nocapture

# --- structural gates ---------------------------------------------------
cargo run -p xtask -- check-architecture     # Layer Law + Module Law
cargo run -p xtask -- check-slices           # the 15 golden SHA-256 pins
cargo run -p xtask -- check-slice-placement  # engine render logic hiding in apps

# --- coverage (100% engine spine) ---------------------------------------
scripts/coverage.ps1                         # Windows, this repo's primary shell
bash scripts/coverage.sh                     # Linux / CI

# --- static analysis ----------------------------------------------------
bash scripts/dylint-gate.sh                  # incl. engine_no_branching, baseline 0
cargo dylint --all -- --all-targets

# --- TypeScript SDK (unaffected, but one of the four gates) -------------
bash scripts/ts-gate.sh

# --- screenshot comparison ----------------------------------------------
cargo build --release -p axiom-shot --features offscreen
for cp in grid opening esses canyon finish; do
  ./target/release/axiom-shot --app burnt-rubber-golden-$cp \
    --backend gpu      --out screenshots/after/$cp.gpu.png
  ./target/release/axiom-shot --app burnt-rubber-golden-$cp \
    --backend canvas2d --out screenshots/after/$cp.canvas2d.png
done
sha256sum screenshots/after/*.png     # compare against the section 4.8 hashes

# --- native frame-cost A/B (interleaved, never across processes) --------
./target/release/axiom-shot --profile-compare \
  burnt-rubber-golden-opening,burnt-rubber-golden-canyon \
  --profile-frames 60 --profile-trials 5

# --- wasm build + browser verification ----------------------------------
cargo build --target wasm32-unknown-unknown -p axiom-kernel
uv run scripts/localhost_servers.py start-app burnt-rubber --port 8085
uv run scripts/localhost_servers.py logs burnt-rubber -n 20
uv run scripts/playwright_controller.py goto http://localhost:8085/
uv run scripts/playwright_controller.py wait 2500
uv run scripts/playwright_controller.py console        # must be error-free
uv run scripts/playwright_controller.py screenshot burnt-rubber-after
uv run scripts/localhost_servers.py stop burnt-rubber
```

**Note on CI (R6):** `.github/workflows/ci.yml` has been `workflow_dispatch`-only
since 2026-07-14. Nothing above runs automatically. Every gate must be run
locally before pushing, and the implementing agent must not assume CI will catch
a mistake.
