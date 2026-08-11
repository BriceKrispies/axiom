# Startup Procedural Preparation — Coordinator Manifest

> **This directory is an execution plan for a swarm of implementation agents.**
> The architecture is already decided. A subagent handed one manifest from this
> directory should implement exactly that manifest and **must not redesign
> anything**. If a manifest appears to require a decision, that is a defect in
> this document — report it to the orchestrator rather than deciding locally.
>
> **Status:** planning complete, **implementation not started**. No production
> code has been modified by the session that wrote this.
>
> **Companion document:** `docs/architecture/startup-preparation-plan.md` is the
> architecture *reference* (why this shape, what was rejected, the full
> current-state survey). **This README is authoritative for execution**: where
> the two disagree on task ordering or file ownership, this one wins, because it
> re-cuts that document's sequential T1–T18 graph into parallel streams.

---

## 1. Goal

Give Axiom an explicit, structurally-enforced **startup preparation phase**:
expensive, startup-only procedural work runs to completion, produces
runtime-ready in-memory data, and only then may the application begin stepping
its simulation.

```text
launch
  → construct application from configuration + seed
  → startup preparation phase
  → expensive procedural generation
  → runtime-ready in-memory representations
  → resource/GPU finalization at its existing higher-layer owner
  ══ PREPARATION BARRIER ══
  → application becomes playable
  → normal frame loop
```

On exit the generated data simply disappears. On the next launch it is generated
again. "Baked" here means only: *expensive procedural descriptions are resolved
into runtime-ready representations before normal gameplay begins.*

### What this is NOT

| Not this | Distinction |
|---|---|
| **Offline asset baking** | Nothing is generated ahead of the process. |
| **Persistent caching** | Nothing is written to disk, IndexedDB or any store. No cache file is created, ever. |
| **Build-time generation** | No codegen, no `build.rs`, no pre-pass. |
| **Asset packaging** | No `.axpkg`, no archive, no packer. `modules/axiom-assets` is untouched. |
| **Runtime streaming** | `modules/axiom-streaming`'s residency ring is demand-driven work *during* play and remains the correct owner of that. |
| **Ordinary dynamic procedural gameplay** | Traffic activation, scenery visibility, effects and collision stay in the frame loop. Preparation is only for work that needs no gameplay state at all. |

---

## 2. Architectural finding

### Decision record

**Decision:** `crates/axiom-runtime` — the existing `runtime` layer, **extended**.
No new layer. No kernel change. No new crate.

**Invariant owned:**
> The deterministic simulation cannot advance until a preparation phase has
> completed successfully. `Runtime::step` is reachable only from
> `RuntimeState::Running`; `Running` is reachable only from
> `RuntimeState::Prepared`; `Prepared` is reachable only from a `Runtime::prepare`
> in which every registered preparation task returned `Ok`.

**Why here:** the barrier is a *precondition on `Runtime::step`*, and
`Runtime::step` lives in this crate (`crates/axiom-runtime/src/runtime.rs:144`).
The layer's own `ARCHITECTURE.md:8` already lists *"a strict **lifecycle** state
machine"* as its first responsibility, and `RuntimeState`
(`crates/axiom-runtime/src/runtime_state.rs:9`) is the engine's only
construction→running state machine. This is the missing phase of a machine that
already exists. The layer also already owns the exact execution shape required:
`RuntimeScheduler::register` (`runtime_scheduler.rs:70`) is a stable-id'd,
explicitly-ordered, duplicate-rejecting registry of opaque work supplied from
above — proven, branchless and fully covered.

**Why not one layer lower (`crates/axiom-kernel`):** `CLAUDE.md:87` does permit
"lifecycle contracts" in the kernel, the kernel has never used that allowance,
and it already ships public traits (`LogSink`, `TelemetrySink`). So the question
is real and deserves a real answer.

The answer is **not** "the error type would be wrong". An earlier draft argued
that, and it is refutable in thirty seconds: `RuntimeError::with_kernel`
(`crates/axiom-runtime/src/runtime_error.rs:31`) and
`RuntimeErrorCode::KernelFailure = 7` exist precisely to translate a kernel error,
and `Runtime::new` already does it (`runtime.rs:60`). A kernel task returning
`KernelResult<()>` would cost one `.map_err`.

The real reason is **concept-splitting**. The barrier *state* and the *gate* must
live in `runtime` — `RuntimeState` (`runtime_state.rs:9`) and `start`/`step`
(`runtime.rs:92,144`) are there and cannot move, because `runtime` is the lowest
layer that can name them. A kernel-owned trait plus schedule would therefore put
half of one concept in the kernel and half in `runtime`, for no gain, while
paying the kernel's curated-export amendment cost
(`crates/axiom-kernel/ARCHITECTURE.md:58-76`). Nothing below `runtime` steps,
schedules or runs systems, so nothing below `runtime` needs to name the phase.

**Why not one layer higher (`crates/axiom-host`, `crates/axiom-frame`, a new
layer above runtime):** mechanically impossible, not merely unwise. For
`crates/axiom-runtime` to *name* a capability, that capability's layer must be in
runtime's `depends_on`; a layer that itself depends on `runtime` would then form
`runtime → X → runtime`, which `check.rs:161-183` rejects as `DependencyCycle`.
So a phase owned above runtime could never gate `Runtime::step`. Separately,
`crates/axiom-host`'s remit (`ARCHITECTURE.md:102-120`) is *facts an engine
outside of it generated* — its `HostLifecycleSignal` alphabet
(`host_lifecycle_signal.rs:3-9`) is deliberately closed and external — while
preparation is an internal fact.

**Why not a new root-adjacent layer (`depends_on = ["kernel"]`):** this is
*mechanically legal* (verified against the checker) and was seriously evaluated.
It is rejected because the barrier itself must still live in `runtime` (only
`runtime` holds the state and gates `step`), so the new layer would own only the
`PreparationTask` trait and the schedule while `runtime` owned `Prepared` and
`prepare()` — splitting one concept across two layers for no gain, at the cost of
a new crate, a new workspace member, its own `proof_exports`, its own 100%
coverage obligation and its own architecture test. `CLAUDE.md:119`: *"Do not
create tiny ceremonial layers just to feel organized."*

**Higher-level participants:** only a crate that **already** declares `runtime`
may name `PreparationTask` — **3 of 21 layers** (`math`, `host`, `frame`),
**9 of 44 modules**, and **9 of 31 apps** (Burnt Rubber is one, via
`apps/burnt-rubber/Cargo.toml:41`). **Adding `runtime` to a manifest solely in
order to implement the trait is a ceremonial dependency and is forbidden.**

Those two sentences together would make `App::prepare_with` unusable by 22 of 31
apps, so manifest `06` additionally re-exports the trait through the umbrella's
single facade — `pub use axiom_runtime::PreparationTask;` in
`modules/axiom/src/prelude.rs`, which already re-exports five layers and is the
module's one legal public surface under Module Law #8. An app then names it via
`axiom::prelude` without any manifest change.

For any producer that does **not** depend on `runtime`, the *app* wraps that
module's product-producing call in an app-owned task. That is the primary
pattern, not a fallback.

**App responsibility:** the composition root decides *what* preparation
contains. `modules/axiom`'s `RunningApp::realize` registers scene authoring as
the engine's own first preparation task; each app adds its domain tasks via
`App::prepare_with`.

**Platform-specific responsibility:** none in this work. See §6.

### Scope limit — state this honestly, do not overclaim

The barrier gates **simulation stepping**, not all presentation.
`RunningApp::render(tick)` (`modules/axiom/src/app/frame.rs:102`) is public,
documented *"safe to call standalone"*, and references `self.runtime` **zero**
times; a host that owns its own loop (the `@axiom/game` TS SDK path) can call it
on an unprepared app. `HostStepDriver::drive` also returns `Ok` without calling
`Runtime::step` when `HostStepPlan::steps() == 0`.

Burnt Rubber presents via `RunningApp::tick` (`apps/burnt-rubber/src/app.rs:532`),
which is `step` then `render` (`app/frame.rs:46-47`), so the vertical proof is
unaffected. If presentation gating is ever wanted it is a **second consumer** of
`RuntimeState::Prepared`, not a relocation of the phase.

Likewise `Prepared` means *"a preparation phase ran to completion"*, **not**
*"the right work was declared"* — a zero-task schedule satisfies it, and §9
mandates exactly that at every un-migrated call site. What is mandatory for an
`App`-based app is the engine's own `AuthorTask`, contributed by the composition
root.

### Generation moves; registration does not — and why that is still the win

A `PreparationTask` has a zero-argument `prepare(&mut self)`, so it can never
touch `&mut RunningApp`. It follows that a task **cannot create scene entities**:
`add_mesh_data` / `add_material` / `spawn` all need the app. And Burnt Rubber
calls `RaceScene::install(&mut running, …)` and `DebugView::install(&mut running)`
from `BurntRubber::with_profile` at `apps/burnt-rubber/src/app.rs:198-200` —
i.e. **after** `App::build()` → `realize` → `start()`.

So after this programme lands, the honest description is:

> The **expensive generation** — course compilation, texture synthesis, road and
> prop geometry — happens inside the barrier. The **cheap registration** of that
> data into the scene (`add_mesh_data`, `add_material`, `spawn`) still happens
> after `Running`, before the first `tick()`.

That is still the win the programme is for: what was slow moves; what was already
trivial stays. But **do not** describe preparation tasks as producing entities.
§13's "Output" column is written to that rule; the tasks produce `MeshData`,
`Vec<u8>` pixels and an `Arc<CoursePlan>`, never entities.

Two consequences a future agent must not rediscover the hard way:

1. **Moving registration inside the barrier is a separate, larger design** — it
   would need the phase to own `&mut RunningApp` (e.g. an
   `App::install_with(Box<dyn FnOnce(&mut RunningApp)>)` executed inside
   `realize` between `prepare` and `start`). It is **out of scope** and must not
   be invented by manifest `11`.
2. **`AuthorTask` wraps zero work in the proof app.** Burnt Rubber's setup
   closure is empty — `.setup(|_world, _meshes, _materials| {})`
   (`apps/burnt-rubber/src/app.rs:189`). `AuthorTask` is still the right
   structural fix for the `realize` ordering defect and it is exercised by
   `modules/axiom`'s own tests, but it is not what proves the barrier in Burnt
   Rubber. The domain tasks are.

---

## 3. Current lifecycle (real symbols)

```text
RuntimeState::Created                      crates/axiom-runtime/src/runtime_state.rs:11
  │ Runtime::initialize()                  crates/axiom-runtime/src/runtime.rs:79
  ▼
RuntimeState::Initialized
  │ Runtime::start()                       crates/axiom-runtime/src/runtime.rs:92
  ▼                                        (accepts Initialized | Paused)
RuntimeState::Running ⇄ Paused             pause():105
  │ Runtime::step()                        crates/axiom-runtime/src/runtime.rs:144
  ▼                                        (requires Running, else StepWhileNotRunning)
  RuntimeStepRecord
```

Driven from the composition root, `modules/axiom/src/app.rs:324`
`RunningApp::realize`:

```rust
runtime.initialize().expect("runtime initialize cannot fail");   // :330
runtime.start().expect("runtime start cannot fail");             // :334
…
let authored = Self::author(app.setup, aspect);                  // :353  ← AFTER start()
```

**This ordering is the root defect this work fixes.** The runtime reports
`Running` for an application whose scene is not authored and whose meshes do not
exist. Nothing between `realize` and the first `request_animation_frame` reads
`Runtime::state()`.

Browser path, `apps/burnt-rubber/src/web.rs`:

```text
configure_surface_from_canvas (:81)   canvas measured, no GPU device
  → BurntRubber::with_profile (:99)   ALL expensive generation, synchronous, still no device
  → mesh_set()/material_textures() (:127-128)
  → run_web_multi (:328) → drive_web_multi → spawn_local → LivePresenter::bind().await
                                              ← GPU device first exists HERE, then rAF
```

---

## 4. Proposed lifecycle

```text
Created ──initialize()──▶ Initialized ──prepare(schedule)──┬─ all Ok ─▶ Prepared
                                                           │                │
                                                    any task Err          start()
                                                           ▼                ▼
                                                        Failed           Running ⇄ Paused
                                                       (terminal)           │
                                                                         step()
Initialized / Prepared / Running / Paused ──stop()──▶ Stopped (terminal)
```

**There is no `Preparing` state.** `prepare(&mut self)` holds the exclusive
borrow for the whole phase and a task is handed no `Runtime`, so no task, test or
host could ever observe it; shipping it would mean shipping arms the Coverage
Law forbids. Discriminants are appended, so a future budgeted executor can add it
then at zero cost.

### Legal transitions

| From | Via | To |
|---|---|---|
| `Created` | `initialize()` | `Initialized` |
| `Initialized` | `prepare(schedule)` | `Prepared` (all Ok) or `Failed` (any Err) |
| `Prepared` \| `Paused` | `start()` | `Running` |
| `Running` | `pause()` | `Paused` |
| `Initialized` \| `Prepared` \| `Running` \| `Paused` | `stop()` | `Stopped` |

### Illegal transitions — each must fail deterministically

| Attempt | Result |
|---|---|
| `start()` from `Initialized` | `InvalidLifecycleTransition` — **this is the barrier** |
| `start()` from `Created` / `Failed` / `Stopped` | `InvalidLifecycleTransition` |
| `prepare()` from `Created` | `InvalidLifecycleTransition` |
| `prepare()` from `Prepared` / `Running` / `Paused` | `InvalidLifecycleTransition` — **preparation runs exactly once per launch** |
| `prepare()` from `Failed` / `Stopped` | `InvalidLifecycleTransition` |
| `step()` from anything but `Running` | `StepWhileNotRunning` (unchanged) |

---

## 5. Architectural boundaries

| Tier | Owns |
|---|---|
| **`crates/axiom-runtime`** (lowest) | The phase (`RuntimeState::Prepared`), the declaration mechanism (`PreparationTask`, `PreparationSchedule`), the executor (`Runtime::prepare`), deterministic ordering, the failure code, and the gate on `start()`. Knows **nothing** about what is being prepared. |
| **Higher engine modules** | Domain generation. A module that already declares `runtime` may implement `PreparationTask` and hand it out from its facade as `Box<dyn PreparationTask>` (Module Law #8 forbids exporting the concrete type). |
| **`modules/axiom`** (composition root) | `App::prepare_with`, the `PreparationTask` prelude re-export, and the engine's own `AuthorTask` — scene authoring expressed as preparation, which is what makes the §3 ordering defect unreinstatable. |
| **Host/platform** | Nothing new. The GPU device does not exist during preparation (§6). Timing of `prepare()` is measured here because the runtime has no clock. |
| **Apps** | Which participants compose into their startup, and all domain generation logic. |
| **Tooling/tests** | The golden regression (already landed), the architecture surface lock, gate execution. |

### The lowest primitive may know

preparation · readiness · deterministic work ordering · completion · failure ·
lifecycle transition

### The lowest primitive may NOT know

road generation · terrain · meshes · textures · render pipelines · WebGPU ·
physics · Burnt Rubber · traffic · vegetation · game recipes

This is mechanically backed: four sibling layers already assert that
`crates/axiom-runtime/src` contains no reference to them
(`crates/axiom-frame/tests/architecture.rs:308`,
`crates/axiom-host/tests/architecture.rs:431`,
`crates/axiom-math/tests/architecture.rs:306`,
`crates/axiom-ecs/tests/architecture.rs:251`), and `runtime` is **not** in
`PLATFORM_FACING_LAYERS` (`crates/xtask/src/hygiene.rs:52`, which is `["host"]`
only), so the browser-API scan bans `web_sys|js_sys|wasm_bindgen|WebGPU|WebGL|
requestAnimationFrame|window.|document.|canvas` in every new runtime file.

---

## 6. CPU / GPU boundary — why there is no GPU manifest

Investigated per Phase 10 and answered on evidence, not preference.

**The GPU device does not exist while preparation runs.** In
`apps/burnt-rubber/src/web.rs` the app — and therefore all generation — is fully
constructed at `:99`, and the first `wgpu::Device` is created much later inside
`LiveGpuBinding::initialize` (`modules/axiom-gpu-backend/src/live_gpu_binding.rs:134`),
reached only after `run_web_multi` (`:328`) crosses `spawn_local`
(`modules/axiom-windowing/src/windowing_api/web.rs:590`). So the low-level
primitive's ignorance of WebGPU is a *fact of the current ordering*, not an
aspiration.

**The CPU→GPU seam already sits where it belongs.** `add_mesh_data` /
`add_texture_data` (`modules/axiom/src/app/authoring.rs:73,132`) are **CPU-side
registration**, not upload; `mesh_set()` / `material_textures()`
(`modules/axiom/src/app/resources.rs:35,71`) produce plain data by value; the
only crate that touches a GPU buffer is `modules/axiom-gpu-backend`, whose
`module.toml` allows exactly `["host","math"]` and `allowed_modules = []`, and
which only `modules/axiom-windowing` may name.

**Conclusion: no GPU-side preparation work is independently implementable
today, and none is scheduled.** Inventing a "GPU preparation" abstraction now
would be the premature abstraction Phase 10 warns against.

**Recorded for a future pass, not scoped here:** `mesh_set()` and
`material_textures()` regenerate and deep-copy everything on every call with no
caching, and `material_textures()` re-runs each procedural texture generator.
That is a real startup cost at a well-defined moment and a natural future
preparation product. It is **out of scope** — it changes no lifecycle and would
move golden bytes.

---

## 7. Public contract — FIXED. Do not redesign.

Two new public symbols. This is the entire foundational surface.

### `crates/axiom-runtime/src/preparation_task.rs`

```rust
/// A unit of startup-only work, supplied from above and opaque to the runtime.
pub trait PreparationTask {
    fn prepare(&mut self) -> RuntimeResult<()>;
}
```

**Zero arguments is deliberate and load-bearing.** It is what makes a
`PreparationTask` structurally un-registerable as a `RuntimeSystem` (whose `run`
takes `&mut RuntimeContext<'_>`) and vice versa — the type system, not developer
discipline, keeps startup work out of the frame loop. It also denies a task any
tick, command queue, event queue or clock. Products never flow through the
runtime; a task writes into storage its constructor captured (§8).

### `crates/axiom-runtime/src/preparation_schedule.rs`

```rust
pub struct PreparationSchedule { /* private: Vec<(&'static str, Box<dyn PreparationTask>)> */ }

impl PreparationSchedule {
    pub fn new() -> Self;
    pub fn push(&mut self, name: &'static str, task: Box<dyn PreparationTask>);
}

impl Default for PreparationSchedule { … }
impl std::fmt::Debug for PreparationSchedule { … }   // hand-written; required by
                                                     // crates/axiom-runtime/Cargo.toml:18
                                                     // missing_debug_implementations = "warn"
```

**`push`, not `register` — and deliberately no `HandleId`, no `order: i32`, no
`RuntimeResult`.** An earlier draft mirrored `RuntimeScheduler::register`
verbatim. That was cargo-culting, and the difference matters:

`RuntimeScheduler` is a **long-lived, multi-writer registry**. Systems arrive
through `Runtime::scheduler_mut()` (`runtime.rs:258`) from independent layers at
arbitrary times across the runtime's whole life, nobody controls the
interleaving, and ids are read back via `system_ids()` and `SystemOutcome`.
Duplicate `id`/`order` rejection is what makes execution order a function of
*configuration* rather than of who called first. That is real, and it is why the
scheduler has that shape.

**None of it holds here.** The schedule is built at exactly one site
(`RunningApp::realize`), populated in a straight line, moved into `prepare` by
value, and dropped. Registration order **is** a deterministic total order, for
free, with no duplicate possible and no id ever read.

Registration order is in fact *stronger* than a reserved-order band: with a band,
an app could legally register at the engine's order and correctness would rest on
everyone honouring a convention. With plain push order, `realize` pushes
`AuthorTask` first and an app simply **cannot** get in front of it.

Deleting the two parameters also deletes: both duplicate-rejection error paths,
the reserved-order band, three id constants in manifest `07`, a
`negative_and_extreme_orders_sort_correctly` test, and a `try_fold` in `06`.
The named struct is still kept rather than a bare `Vec` — it carries the
`&'static str` names the failure protocol needs, makes the by-value move
meaningful, and hosts the required hand-written `Debug`.

For manifest `02`'s exclusive use, the schedule also exposes a crate-private
executor:

```rust
impl PreparationSchedule {
    /// Run every task in push order, stopping at the first failure.
    /// Returns the failing task's name **and its own error**, or `None`.
    pub(crate) fn execute(&mut self) -> Option<(&'static str, RuntimeError)>;
}
```

Returning the task's own `RuntimeError` — not just a name — is what makes the
failure protocol in §8 actually deliver its diagnostic. `RuntimeError` is
`#[derive(Debug, Clone, Copy)]` (`runtime_error.rs:13`), so this costs nothing.

### `crates/axiom-runtime/src/runtime.rs` — new method

```rust
pub fn prepare(&mut self, schedule: PreparationSchedule) -> RuntimeResult<()>;
```

Taking the schedule **by value** is load-bearing: it is what makes "temporary
work can die" a guarantee rather than a convention, and what makes `prepare()`
un-repeatable without constructing a fresh schedule.

**There is no `PreparationReport`.** An earlier design returned one and its
failure half was unreachable by construction (a report was only produced on the
success path, so `all_succeeded()` could never be `false`).

Failure diagnosis is instead carried by the error itself. `execute` returns
`Option<(&'static str, RuntimeError)>` and `prepare` rebuilds the error keeping
**both** facts:

```rust
failure.map_or(Ok(()), |(name, cause)| Err(RuntimeError::new(cause.code(), name)))
```

so the caller learns *which* task failed (the name) and *why* (the task's own
code). An earlier draft discarded the cause and hard-coded `PreparationFailed`,
which silently threw away the message §8 spends a paragraph teaching task authors
to construct. `RuntimeError` is `Copy`, so preserving it costs nothing.

### Modified existing symbols

| Symbol | File | Change |
|---|---|---|
| `RuntimeState` | `runtime_state.rs:9` | append `Prepared = 6`; **drop the `PartialOrd, Ord` derive** |
| `RuntimeErrorCode` | `runtime_error_code.rs:10` | append `PreparationFailed = 8` |
| `Runtime::start` | `runtime.rs:92` | accept `Prepared \| Paused` (was `Initialized \| Paused`) |
| `Runtime::stop` | `runtime.rs:116` | additionally accept `Prepared` |

The `PartialOrd`/`Ord` derive must go: the only consumer of the ordering in the
whole workspace is the assertion `RuntimeState::Created < RuntimeState::Running`
at `runtime_state.rs:45` — a test asserting the derive it is testing —
and `RuntimeState` is not a `BTreeMap`/`BTreeSet` key anywhere. Appending
`Prepared = 6` after `Failed = 5` would otherwise leave a total order that reads
as lifecycle progression and is not one. Discriminants are **appended, never
renumbered**: `raw()` is a stable identity byte surfaced through
`RuntimeStepRecord::state_after()`.

---

## 8. Determinism contract

**Ordering.** Tasks run in **push order**. That is a total order by
construction, with no duplicate possible and no tie-breaker to depend on.
**No DAG, and no explicit order key.** The real dependency chain (compile course
→ derive track → build meshes from that track → synthesize textures) is *linear*;
push order expresses it exactly, is checkable by eye at the single site that
builds the schedule, and cannot be violated by a caller. Building a dependency
solver — or even an `i32` ordering key — for a straight line is the "giant
generalized task framework" Phase 7 forbids.

**Concurrency: none — and this is settled by evidence, not taste.**
`crates/axiom-runtime` and `crates/axiom-host` contain **zero** `async fn`,
`.await`, `Future` or `spawn_local`. The engine's only async lives in two modules
and four files (`modules/axiom-windowing/src/windowing_api/web.rs`,
`modules/axiom-gpu-backend/src/{gpu_backend_api,live_gpu_binding}.rs`), has no
executor and no join handle. `wasm32-unknown-unknown` — the primary target — has
no threads in this build. Sequential deterministic preparation is not merely the
smallest sufficient model; it is the only one consistent with the spine as it
exists.

**Adding concurrency later would require reopening the ownership model, and the
contract is designed so its *meaning* would not change:** `prepare(schedule)`
returning `RuntimeResult<()>` after all work completes is equally true of a
concurrent implementation. What would have to change is §9's product channel —
`Rc<RefCell<…>>` is `!Send`. Do not write a comment claiming concurrency is a
drop-in addition.

**Failure.** Execution stops at the first failing task; the remainder do not run.
The runtime becomes `Failed` (terminal), so `start()` is unreachable. No partial
readiness. This differs deliberately from `RuntimeConfig::fail_on_system_error`,
which lets *per-step* systems continue: a frame can survive a bad system; an
application cannot survive a world that was never built.

**Replay.** Equivalent seeds and configuration must produce byte-equal prepared
products. The runtime cannot enforce this for app-tier tasks — `apps/` is outside
the dylint rulebook, the coverage gate and the branchless gate
(`tools/lints/engine_lint_helpers/src/lib.rs:43-63` requires a `crates/` or
`modules/` component) — so manifest `13` mandates an app-level replay test as the
only mechanical detector.

**Product ownership.** The runtime owns the fact; the caller owns the data.
`Runtime::prepare` drops the schedule before returning, so scratch state dies at
the barrier. A product cell **must** be `Rc<RefCell<Option<T>>>` — never a
defaultable bare `T` — and a consumer that finds it empty must return `Err`, not
panic:

```rust
fn prepare(&mut self) -> RuntimeResult<()> {
    self.plan.borrow().as_ref()
        .ok_or_else(|| RuntimeError::new(
            RuntimeErrorCode::PreparationFailed,
            "road mesh requires the compiled course"))
        .map(|plan| { /* … */ })
}
```

With a bare `Vec<T>` cell a premature read yields an *empty vec* — a
plausible-looking value that builds empty geometry and renders without erring.
With `Option` it is `None` and the phase fails through the normal protocol. A
`.expect()` there would panic through `Runtime::prepare`, bypassing the failure
protocol entirely and aborting on `wasm32`.

---

## 9. Structural enforcement (Phase 12)

Three mechanisms, all repository-native, none invented:

1. **Lifecycle state** — `start()` requires `Prepared`; `step()` requires
   `Running`. Same branchless `then_some(...).map_or(...)` idiom that already
   enforces every other transition.
2. **Trait-signature split** — `PreparationTask::prepare(&mut self)` and
   `RuntimeSystem::run(&mut self, ctx: &mut RuntimeContext<'_>)` are
   incompatible, so `PreparationSchedule::register` and
   `RuntimeScheduler::register` cannot accept each other's work. Startup work
   becoming frame work is a **compile error**.
3. **Ownership transfer** — the schedule moves into `prepare` by value and is
   dropped there, so a task cannot be re-run or read afterwards.

**Deliberately NOT used: an xtask hygiene text scan.** It was designed and
rejected. `xtask` scans source as text; "type X implements traits A and B" is a
type-resolution question, defeated by putting the two impls in different files
(legal Rust). Worse, `hygiene::check` iterates only layer and module source dirs,
while nearly every `PreparationTask` impl will live in `apps/`. Shipping a check
that advertises enforcement it cannot deliver is itself a check-shaped shortcut.
It would also require a new `ViolationKind`, and `ViolationKind::TOKENS`
(`crates/xtask/src/violation.rs:150-201`) is a fixed `[&str; 50]` indexed by
discriminant — adding a variant without its token breaks the array. If a semantic
check is ever wanted it belongs in `tools/lints` as a dylint, separately.

**Typestate was evaluated and rejected** — not for the reason an earlier draft
gave (it would *reduce* branch-shaped code, not fight the branchless law) but
because `RunningApp` stores a `Runtime` field, `HostStepDriver::drive(&mut
Runtime, …)` takes it by mutable reference, and `Paused → Running` is a legal
re-entry. A consuming `prepare(self) -> PreparedRuntime` would force a type
change through host, frame and the umbrella for a property the state check
already delivers.

---

## 10. Dependency graph

```text
01-runtime-preparation-primitive          (FOUNDATION — everything waits)
   │
   ├───────────────────────────────┬──────────────────────────┐
   ▼                               ▼                          ▼
┌─────────────────────────────────────────────┐   03-runtime-manifest-and-docs
│  ATOMIC LANDING GROUP — one merge           │   (independent; lands any time
│                                             │    after 01)
│   02-runtime-preparation-barrier  (head)    │
│      │  05 and 06 branch FROM 02's branch   │
│      ├── 05-runtime-callsite-sweep          │
│      └── 06-composition-root-preparation    │
└─────────────────────────────────────────────┘
   │                    │
   ▼                    ▼
04-runtime-        07-burnt-rubber-preparation-scaffold
preparation-tests            │
(after the group     ┌───────┼───────┐
 lands)              ▼       ▼       ▼
                    08      09      10
                 course  textures meshes
                     └───────┼───────┘
                             ▼
                     11-burnt-rubber-wiring
                             │
                             ▼
             12-burnt-rubber-traffic-preparation   (OPTIONAL)
                             │
                             ▼
                     13-integration                (FINAL — sole owner)
```

### Launch schedule

| Wave | Manifests | Width | Landing |
|---|---|---|---|
| 1 | `01` | 1 | own commit |
| 2 | `02`, `05`, `06` + `03` | 4 | **`02`+`05`+`06` merge together**; `03` separately |
| 3 | `04` | 1 | own commit |
| 4 | `07` | 1 | own commit |
| 5 | `08`, `09`, `10` | 3 | own commits, any order |
| 6 | `11` | 1 | own commit |
| 7 | `12` (optional) | 1 | own commit |
| 8 | `13` | 1 | own commit |

**Maximum useful parallel width is 4 (wave 2) and otherwise 3.** Do not launch
more agents than that and expect throughput; the serialisation points below are
real, not conservative.

### The one atomic landing group, and why it exists

**Read this carefully — the obvious mental model is wrong.**
`Runtime::start`'s *signature does not change*. It stays
`pub fn start(&mut self) -> RuntimeResult<()>`; only the accepted-state
predicate inside the body moves from `{Initialized, Paused}` to
`{Prepared, Paused}` (`runtime.rs:92-102`). **Therefore nothing fails to
compile.** `cargo build --workspace` succeeds throughout.

What breaks is **behaviour**: all 23 sites are `.unwrap()`/`.expect()` on a
`start()` that now returns `Err`, so they *panic at test time*. `main` would be
red on `cargo test`, not on `cargo build`.

That still justifies the group — `main` must be green — but it changes every
detection command. **Never enumerate the work with `cargo build`; it will report
nothing and an agent will wrongly conclude it is done.** Use:

```sh
cargo test --workspace 2>&1 | grep -E 'panicked at|test result: FAILED'
```

and derive scope from the pre-computed census (`rg -n '\.initialize\(\)'`),
which is exact: 8 sites in `runtime.rs` (`02`), 1 in `modules/axiom/src/app.rs`
(`06`), 14 across nine files (`05`).

Resolution: `05` and `06` branch from `02`'s branch rather than `main`, write
against the frozen contract in `02`, and the orchestrator merges the three
together. Each agent owns a disjoint file set, so there is no write conflict —
only a shared landing.

### Why each serialisation exists

| Edge | Cause |
|---|---|
| `01 → 02` | `runtime.rs` needs `RuntimeState::Prepared` and `PreparationSchedule` to exist. |
| `02 → everything` | `start()`'s accepted-state change is the API break; nothing downstream compiles until it lands with its in-file call sites fixed. |
| `06 → 07` | Burnt Rubber's scaffold needs `App::prepare_with`. |
| `07 → 08,09,10` | `preparation/mod.rs` must declare all three submodules first, so the three domain agents never touch it. |
| `08,09,10 → 11` | `render/mod.rs` and `apps/burnt-rubber/src/app.rs` are contested by more than one domain stream and are reserved for `11`. |

---

## 11. File ownership matrix

**Every production file expected to change has exactly one primary owner.**
"Others may edit?" is **No** unless stated.

| Path | Owning manifest | Others may edit? |
|---|---|---|
| `crates/axiom-runtime/src/preparation_task.rs` *(new)* | `01` | No |
| `crates/axiom-runtime/src/preparation_schedule.rs` *(new)* | `01` | No |
| `crates/axiom-runtime/src/runtime_state.rs` | `01` | No |
| `crates/axiom-runtime/src/lib.rs` | `01` | **No — reserved.** 65 lines; two `mod`+`pub use` pairs land in the same two contiguous blocks |
| `crates/axiom-runtime/src/runtime.rs` | `02` | **No — reserved.** 656 lines; contains 8 of the 23 breaking call sites *inside its own test module* |
| `crates/axiom-runtime/src/runtime_error_code.rs` | `02` | No |
| `crates/axiom-runtime/layer.toml` | `03` | No |
| `crates/axiom-runtime/ARCHITECTURE.md` | `03` | No |
| `crates/axiom-runtime/tests/architecture.rs` *(new)* | `04` | No |
| `crates/axiom-runtime/tests/preparation.rs` *(new)* | `04` | No |
| `crates/axiom-runtime/tests/preparation_lifecycle.rs` *(new)* | `04` | No |
| `crates/axiom-runtime/src/runtime_scheduler.rs` | `05` | No |
| `crates/axiom-runtime/tests/integration.rs` | `05` | No |
| `crates/axiom-frame/src/frame_step_summary.rs` | `05` | No |
| `crates/axiom-host/src/host_api.rs` | `05` | No |
| `crates/axiom-host/src/host_step_driver.rs` | `05` | No |
| `crates/axiom-introspect/src/fixtures.rs` | `05` | No |
| `apps/axiom-demo-rotating-cube/src/demo_api.rs` | `05` | No |
| `apps/axiom-demo-rotating-cube/examples/introspection_evidence.rs` | `05` | No |
| `tools/axiom-profile-runner/src/scenario.rs` | `05` | No |
| `modules/axiom/src/app.rs` | `06` | **No — reserved.** Holds the 23rd call site *and* all composition work |
| `modules/axiom/src/app_tests.rs` | `06` | No |
| `modules/axiom/src/app/preparation.rs` *(new)* | `06` | No |
| `modules/axiom/src/prelude.rs` | `06` | No — one line, `pub use axiom_runtime::PreparationTask;` |
| `apps/burnt-rubber/src/preparation/mod.rs` *(new)* | `07` | **No — frozen after `07`.** Declares all three submodules up front |
| `apps/burnt-rubber/src/lib.rs` | `07` | No (one line) |
| `apps/burnt-rubber/src/preparation/course.rs` | **created by `07`**, filled by `08` | `07` creates the stub; `08` is the only manifest that edits its body |
| `apps/burnt-rubber/src/sim/mod.rs` | `08` | No |
| `apps/burnt-rubber/src/course/compiler/mod.rs` | `08` | No |
| `apps/burnt-rubber/src/preparation/textures.rs` | **created by `07`**, filled by `09` | as above |
| `apps/burnt-rubber/src/render/palette.rs` | `09` | No |
| `apps/burnt-rubber/src/render/{asphalt,verge,foliage}_texture.rs` | `09` | No |
| `apps/burnt-rubber/src/preparation/meshes.rs` | **created by `07`**, filled by `10` | as above |
| `apps/burnt-rubber/src/render/scenery.rs` | `10` | No — `distant_hills` (`:633`) is part of P4 |
| `apps/burnt-rubber/src/render/chunks.rs` | `10` | No |
| `apps/burnt-rubber/src/render/scenery_pool.rs` | `10` | No |
| `apps/burnt-rubber/src/render/prop_meshes.rs` | `10` | No |

| **`apps/burnt-rubber/src/render/mod.rs`** | `11` | **No — deliberately reserved.** 1872 lines; `09` needs `:86` and `10` needs `:375-376`. (They are ~290 lines apart, so git *would* auto-merge — the reservation is because both need the file at all, and because `11` must switch both call sites in one coherent pass, not because the hunks are adjacent.) |
| **`apps/burnt-rubber/src/app.rs`** | `11` | **No — deliberately reserved.** 1060 lines; `08` and the wiring both want it |
| `apps/burnt-rubber/src/web.rs` | `11` | No |
| `apps/burnt-rubber/src/course/traffic/flow.rs` | `12` | No |
| `apps/burnt-rubber/src/course/traffic/encounters.rs` | `12` | No |
| `apps/burnt-rubber/src/sim/traffic.rs` | `12` | No |
| `apps/burnt-rubber/src/course/traffic/mod.rs` | `12` | No — `TrafficPlan` is declared at `:44` |
| `apps/burnt-rubber/src/course/validation/mod.rs` | `12` | No — 4 struct-literal sites |
| `apps/burnt-rubber/src/course/validation/traversal.rs` | `12` | No — 2 struct-literal sites |
| `apps/burnt-rubber/tests/preparation.rs` *(new)* | `13` | No |
| `apps/burnt-rubber/TESTING.md` | `13` | No |
| `docs/architecture/startup-preparation-plan.md` | `13` | No |
| `docs/work-manifests/startup-preparation/README.md` | `13` | No — records landed outcomes |
| `.github/workflows/ci.yml` | `13` | No |

### Files NO manifest may modify

| Path | Why |
|---|---|
| **Workspace `Cargo.toml`** | No new crate is created. Zero new Cargo edges anywhere. If a manifest appears to need it, **stop and report**. |
| `crates/axiom-runtime/Cargo.toml` | `axiom-kernel` + `axiom-zones` are already present and sufficient. |
| `apps/burnt-rubber/tests/golden/**` (15 files) | The committed baseline. **Re-blessing is forbidden.** |
| `apps/burnt-rubber/slice.toml` | The 15 SHA-256 pins. A golden diff is a bug, not a new baseline. |
| `apps/burnt-rubber/tests/agent_golden.rs` | The regression's assertions must not be edited to accommodate the migration. |
| `tools/axiom-shot/src/registry.rs` | Zero-edit — but a build tripwire if any stream changes a `capture::build_*` signature. |
| `crates/xtask/src/**` | No architecture-checker change is required or permitted (§9). |
| `tools/lints/**`, `tools/lints/dylint-baseline.txt` | No lint change. The baseline is not to be raised. |
| `CLAUDE.md` | Out of scope. (Noted: it is **stale** — it names `windowing` as the sole platform-facing module, but `crates/xtask/src/hygiene.rs:65-70` allows five. Report, do not fix.) |

---

## 12. Contract ownership matrix

| Capability | Owning manifest |
|---|---|
| `PreparationTask` trait definition | `01` |
| `PreparationSchedule` type + `register` semantics | `01` |
| `RuntimeState::Prepared` | `01` |
| `Runtime::prepare` + execution + failure protocol | `02` |
| `RuntimeErrorCode::PreparationFailed` | `02` |
| The barrier (`start()` accepted-state set) | `02` |
| `layer.toml` capability declarations | `03` |
| Public-surface lock for the new API | `04` |
| `App::prepare_with` + prelude re-export + `AuthorTask` | `06` |
| Burnt Rubber schedule assembly | `07` |
| Burnt Rubber course/texture/mesh task bodies | `08` / `09` / `10` |
| Burnt Rubber composition wiring | `11` |
| End-to-end proof, gates, landing | `13` |

---

## 13. Burnt Rubber classification (Phase 9)

Root seed: `apps/burnt-rubber/src/lib.rs:109`
`DEFAULT_SEED = 0x0B17_4E7A_5C09_1D33`. Course length 9 270 m, ~4 635 samples.

**Do not conflate the three chunk counts.** `CHUNK_LENGTH = 100 m` → **93**
scenery cells; `DRAW_SPAN = 400 m` → **24** `build_draw_mesh` calls producing
**96** entities (4 material parts each); `PAINT_CHUNK_LENGTH = 10 m` → **927**
fine paint meshes. A fourth unrelated 92 is `Effects::install`'s entity count
(`render/effects.rs:26,28`).

### STARTUP_PREPARABLE

| # | Symbol | Current invocation | Proposed phase | Output | Runtime consumer | Lifetime | Depends on | Expected frame impact |
|---|---|---|---|---|---|---|---|---|
| P1 | `course::procedural::plan_for` → `CoursePlan::assemble` (`src/course/procedural.rs:391`, `src/course/runtime/mod.rs:57`) | `RaceSim::with_profile` → `src/sim/mod.rs:191` | preparation, order 100 | `Arc<CoursePlan>` | `RaceSim`, ghost, HUD, all render subsystems | whole run | — | none per frame; removes **3 of 4** compiles |
| P2 | `asphalt_albedo` / `verge_albedo` / `foliage_albedo` (`render/asphalt_texture.rs:300`, `verge_texture.rs:145`, `foliage_texture.rs:211`) | fused into `add_texture_data` at `render/palette.rs:536,539,779` | preparation, order 200 | 3 × `Vec<u8>` (96 KB) | `RoadMaterials`, `ScenePalette` | whole run | — | none per frame |
| P3 | `build_draw_mesh` / `build_paint_chunk` (`render/road_mesh.rs`, driven from `render/chunks.rs:179,201`) | inline inside `RoadChunks::install` | preparation | 24 `ChunkMeshes` + 927 `MeshData` (**geometry, not entities**) | `RaceScene`, GPU vertex buffers | whole run | P1 (`Track`) | none per frame |
| P4 | `palm_crown_surface()` / `shrub_surface()` (`render/prop_meshes.rs:96,166`, already pure) + the cone builder split out of `install_cone` (`:36`) | inline inside the `install_*` wrappers | preparation | 3 `MeshData` (**geometry, not entities**) | `SceneryField` | whole run | P1 | none per frame |
| ~~P5~~ | **RECLASSIFIED — not in scope.** `PlayerCar::install` ×2, `TrafficVisuals::install`, `PickupVisuals::install`, `Effects::install`, `install_finish_arch`, `install_lights` | `render/mod.rs:85,649,1253` | **unchanged** | These build from engine primitives (`Mesh::cube()`, `Mesh::cylinder()`), not generated geometry — there is nothing expensive to prepare. Moving them would be churn with golden risk and no benefit. Manifest `10` is explicitly forbidden from touching them | — | — | — | none |
| P6 | `DebugView::install` (`src/debug_view.rs:104-128`) | `app.rs:200` | preparation, order 330 | mesh + material per `MarkerKind` + pooled entities | `DebugView` | whole run | — | none per frame |
| P7 | traffic wander pair `wander_phase`/`wander_amount` (`sim/traffic.rs:344-346`) | per activation, runtime | fold onto `TrafficPlan` at compile time | 2 × `f32` per plan | `Traffic::activate` | whole run | P1 | removes the app's **only** runtime `Draw::seeded` |

### RUNTIME_REQUIRED — must NOT be moved

| Work | File:line | Why |
|---|---|---|
| `RoadChunks::update` | `render/chunks.rs:342` | Writes only `Visible(bool)`; early-outs on unchanged range. No mesh-creating path exists after install |
| `SceneryField::refresh` | `render/scenery_pool.rs:138` | Retains what stayed (`:150`), generates only chunks that **entered** (`:151-157`) — one chunk per range advance |
| `SceneryField::pose` | `render/scenery_pool.rs:164` | Camera-dependent; `O(cached props)` per frame |
| `Traffic::activate` scheduling | `sim/traffic.rs:267` | Keyed on `player_distance ± traffic_ahead/behind` and free-slot choice |
| `RaceSim::collect_pickups` | `sim/mod.rs:632` | `PickupField.taken` is per-run mutable state |
| Collision / contact | `sim/collision.rs`, `sim/contact.rs` | Derived per step from car state |
| `Effects::step` / `pose` | `render/effects.rs:102,111` | Camera-relative, from install-time seeds |
| `GhostRun` stepping | `ghost.rs`, `agent.rs` | Live agent simulation |

> **Dynamic traffic behaviour is explicitly not frozen.** The plans are static,
> course-derived data and are prepared (P1). The activation of a plan into a pool
> slot, its wander integration, its yielding and its retirement are gameplay and
> stay in the frame loop. P7 pre-draws only the two *variation constants*, which
> `sim/traffic.rs:339` documents as *"a pure function of the plan and nothing
> else"*.

### ALREADY_CORRECT — no work

`Track::from_samples` (immutable), `CoursePlan` (`Arc`-shared, no `&mut self`
method exists), `pickups::expand_row` (RNG-free), `Diagnostics::observe`, the
tuning tables. **`TraversalGrid` is already transient** — a local inside
`validate()` (`course/validation/mod.rs:76`), consumed and dropped before it
returns; `CoursePlan` stores only the report. There is nothing to drop at the
barrier.

> **Correction to guard against.** The ghost-validation sim
> (`course/validation/ghost.rs:105`, `VALIDATION_STEP_LIMIT = 10 800`) is
> **test-only** — zero non-test callers in `src/`; its only caller is
> `tests/course_pipeline.rs:227`. It is *not* paid per launch. Do not cite it as
> migration savings.

---

## 14. Validation strategy

| Layer | Command | Who runs it |
|---|---|---|
| Unit tests | `cargo test -p axiom-runtime` | `01`, `02`, `04` |
| Composition tests | `cargo test -p axiom` | `06` |
| App tests | `cargo test -p axiom-burnt-rubber` | `07`–`12` |
| **Golden regression** | `cargo test -p axiom-burnt-rubber --test agent_golden` | **every** manifest from `06` onward |
| Workspace | `cargo test --workspace` | `05`, `13` |
| Architecture | `cargo run -p xtask -- check-architecture` | `03`, `05`, `13` |
| Slice pins | `cargo run -p xtask -- check-slices` | `13` |
| Coverage | `bash scripts/coverage.sh` | `13` only |
| Dylint | `bash scripts/dylint-gate.sh` | `13` only |
| TS gate | `bash scripts/ts-gate.sh` | `13` only |
| Pixels | `axiom-shot` both backends vs the recorded hashes | `13` |
| WASM/browser | `localhost_servers.py` + `playwright_controller.py` | `13` |

> **Gate runs are a serial resource.** Never run two gates concurrently: dylint
> fakes a `cargo metadata` error and masks real findings, and `link.exe
> 0xc0000142` is the out-of-memory signature. Only `13` runs coverage/dylint/ts.

**Worktree hazards for any agent running in an isolated worktree:**
`.git/hooks/dylint-baseline.txt` is untracked and per-worktree — a stale copy
makes the branchless gate lie. `ts-gate` needs `node_modules` junctions
re-created. And never run `git reset --hard` in a worktree agent; it has wiped
the main tree before.

---

## 15. Merge / integration order

1. Land `01`. Verify `cargo test -p axiom-runtime`.
2. **Merge `02` + `05` + `06` as one group.** Do not land them separately —
   `main` would be red in between. After the merge run, in this order:
   `cargo build --workspace`, `cargo test --workspace`,
   `cargo run -p xtask -- check-architecture`, and **the golden run**
   (`cargo test -p axiom-burnt-rubber --test agent_golden`) — bytes must be
   unchanged, because `06` reorders `RunningApp::realize`.
3. Land `03` at any point after `01` (it touches no `.rs`).
4. Land `04`.
5. Land `07`.
6. Land `08`, `09`, `10` in any order. Each ships dead-but-tested code.
7. Land `11`. This is where `08`/`09`/`10` first become live. **Golden run.**
8. Land `12` if taken.
9. Land `13`.

**Every manifest must leave `main` green and must run the golden run from `06`
onward.** A golden diff is a bug in that manifest, never a new baseline.

---

## 16. Non-goals

1. No offline asset baking, no persistent cache, no disk artifacts, no `.axpkg`.
2. No generic asset pipeline; `modules/axiom-assets` untouched.
3. No runtime streaming change; `modules/axiom-streaming` untouched.
4. No WebGPU or platform concept in any portable layer.
5. No procedural/road/terrain/mesh/texture concept in `crates/axiom-runtime`.
6. No concurrency, no budgeting, no cancellation, no DAG, no job system.
7. No `PreparationReport`, no `PreparationContext`, no `Preparing` state.
8. No xtask hygiene scan, no new `ViolationKind`, no dylint change.
9. No GPU-side preparation manifest (§6).
10. No `mesh_set()`/`material_textures()` caching (§6, recorded for later).
11. No convergence of Burnt Rubber onto the engine proc stack.
12. No `axiom-shot` pixel-comparator extraction — so the portable GPU tolerance
    criterion stays out of scope and `13` holds the GPU arm to
    development-machine byte-identity only.
13. No golden re-blessing, no tolerance widening, no `#[allow]`, no
    `#[coverage(off)]`, no baseline raise.
14. No `CLAUDE.md` edit, including its stale platform allowlist.

---

## 17. Manifest index

| File | Stream | Wave | Owns |
|---|---|---|---|
| `01-runtime-preparation-primitive.md` | A | 1 | The two new types, `RuntimeState::Prepared`, `lib.rs` |
| `02-runtime-preparation-barrier.md` | B | 2 | `Runtime::prepare`, the `start()` gate, `runtime.rs` |
| `03-runtime-manifest-and-docs.md` | C | 2 | `layer.toml`, `ARCHITECTURE.md` |
| `04-runtime-preparation-tests.md` | D | 3 | Integration tests + the public-surface lock |
| `05-runtime-callsite-sweep.md` | E | 3 | The 9 remaining breaking files |
| `06-composition-root-preparation.md` | F | 3 | `App::prepare_with`, `AuthorTask`, `realize` reorder |
| `07-burnt-rubber-preparation-scaffold.md` | G | 4 | `preparation/mod.rs`, schedule assembly |
| `08-burnt-rubber-course-preparation.md` | H | 5 | Course compile → prepared `Arc<CoursePlan>` |
| `09-burnt-rubber-texture-preparation.md` | I | 5 | The three albedo bakes |
| `10-burnt-rubber-mesh-preparation.md` | J | 5 | Road/prop/scene mesh construction |
| `11-burnt-rubber-wiring.md` | K | 6 | `render/mod.rs` + `app.rs` reconciliation |
| `12-burnt-rubber-traffic-preparation.md` | L | 7 | P7 wander pre-draw (**optional**) |
| `13-integration.md` | M | 8 | Landing, gates, proof, cleanup |
