# Axiom ECS Gap Report

_Audit date: 2026-06-19. Inspection-only: no production code was modified, renamed,
or created during this audit. Cited paths/symbols are as they existed at audit time._

---

## 1. Executive Summary

**Axiom has an ECS-shaped prototype that is real, deterministic, well-tested, and
correctly placed as a layer — but it is a "world store with a system list," not a
real ECS engine.** It is far past a toy and far short of a serious substrate.

What genuinely exists (Layer 05, `crates/axiom-ecs`):

- A generic `World<S>` whose component storage `S` is **supplied by the consumer**
  (`crates/axiom-ecs/src/world.rs`).
- Stable, ascending, serializable entity identity over the kernel's `EntityId`
  (`crates/axiom-ecs/src/entity_registry.rs`).
- Sparse per-type component columns (`ComponentColumn<T>`,
  `crates/axiom-ecs/src/component_column.rs`).
- A two-phase (`Startup`/`Update`) system schedule advanced once per engine frame,
  gated on the frame lifecycle (`World::advance`).
- Whole-world binary serialization via a `ColumnSet`/`ErasedColumn` seam, plus an
  app-blind `DynamicComponents` byte store.

It is **deterministic by construction** (`BTreeSet`/`BTreeMap` ordering, no clock,
no RNG, no `HashMap` semantics, enforced by `tests/architecture.rs`), and it is
**genuinely consumed** by `modules/axiom-scene` (the transform hierarchy is a real
`WorldSystem`) and observed by `crates/axiom-introspect`.

What does **not** exist, at all, anywhere in the repo:

- **No query API.** Systems hand-iterate the registry and call `column.get(e)`.
- **No archetypes and no sparse-set index** — storage is a `BTreeMap` per type.
- **No command buffer / deferred structural mutation.** Spawn/despawn mutate
  immediately; despawn does not even clean up component columns (the consumer must).
- **No events, no resources, no change detection / change ticks.**
- **No generational entity IDs and no stale-handle rejection.** `EntityId` is a
  bare `u64`; ids are minted monotonically and **never reused**, so the staleness
  question is sidestepped rather than solved.
- **No archetype movement, no query filters, no system access declarations, no
  conflict detection, no replay log.**

Bluntly: this is a clean, honest **Stage-0/Stage-1 foundation**. The identity and
storage primitives are sound and the layering is correct, but everything that makes
an ECS an ECS for real engine work — queries, structural-mutation batching,
scheduling by data access, change detection, snapshot/replay — is missing. The name
"ECS" is currently aspirational relative to the feature set, though the architecture
placement is right.

---

## 2. Current ECS Inventory

All first-party ECS code lives in **one crate**, `crates/axiom-ecs` (Layer 05). It
is production code, correctly placed.

| File | ECS concept | Kind | Belongs here? | Layering |
|---|---|---|---|---|
| `crates/axiom-ecs/Cargo.toml` | Layer crate; deps = `axiom-kernel`, `axiom-frame`; dev-deps `axiom-host`/`axiom-runtime`/`axiom-math` | Manifest | Yes | Legal — depends only on lower layers |
| `crates/axiom-ecs/layer.toml` | Declares Layer 05, `depends_on = ["kernel", "frame"]`, proof export `World → FrameContext` | Manifest | Yes | Legal |
| `crates/axiom-ecs/src/lib.rs` | Curated facade: re-exports 9 symbols (`World`, `EntityRegistry`, `ComponentColumn`, `ErasedColumn`, `ColumnSet`, `DynamicComponents`, `WorldSystem`, `WorldStep`, `SchedulePhase`) | Production | Yes | Legal. **Note:** this is a multi-symbol public surface, not a single `EcsApi` facade (see §10) |
| `crates/axiom-ecs/src/entity_registry.rs` | Entity identity/lifecycle (`spawn`/`despawn`/`contains`/`iter`), `BTreeSet<EntityId>` + monotonic `next_id` | Production | Yes | Legal |
| `crates/axiom-ecs/src/world.rs` | `World<S>`: registry + generic storage `S` + system lists; `advance(tick, &FrameContext)`; `serialize`/`deserialize`/`describe` | Production | Yes | Legal — `advance` is the frame-layer adapter |
| `crates/axiom-ecs/src/component_column.rs` | `ComponentColumn<T>`: sparse `BTreeMap<EntityId, T>` store + `Reflect` (de)serialization | Production | Yes | Legal |
| `crates/axiom-ecs/src/erased_column.rs` | `ErasedColumn` trait: type-erased describe/write/read_replace over a column | Production | Yes | Legal — no `Any`/downcast |
| `crates/axiom-ecs/src/column_set.rs` | `ColumnSet` trait: a storage exposes its columns in fixed order for whole-world ops | Production | Yes | Legal |
| `crates/axiom-ecs/src/dynamic_components.rs` | `DynamicComponents`: app-blind, `Reflect`-keyed byte store (`insert`/`get`/`contains`/`remove`/`describe`) | Production | Yes | Legal — owned-value (serialized) path, not a hot-loop store |
| `crates/axiom-ecs/src/world_system.rs` | `WorldSystem<S>` trait: `run(&self, &WorldStep, &EntityRegistry, &mut S)` | Production | Yes | Legal |
| `crates/axiom-ecs/src/schedule_phase.rs` | `SchedulePhase` enum (`Startup`/`Update`) | Production | Yes | Legal |
| `crates/axiom-ecs/src/world_step.rs` | `WorldStep`: per-advance tick carrier | Production | Yes | Legal |
| `crates/axiom-ecs/src/fixtures.rs` | `#[cfg(test)]` real `EngineFrame` fixtures (active/skipped/zero-step) | Test | Yes | Legal — test-only |
| `crates/axiom-ecs/tests/architecture.rs` | 10 mechanical guards: no browser/JS/wgpu/clock/RNG/print/placeholder, curated exports, no lower layer imports ecs, ecs imports only legal layers | Test | Yes | Legal |

### Consumers of the ECS (not ECS code, but they define where it stands)

| File | How it uses ECS | Kind | Layering note |
|---|---|---|---|
| `modules/axiom-scene/src/scene_storage.rs` | Defines `SceneStorage` (the `S`) of `ComponentColumn`s; implements **real `WorldSystem`s**: `TransformPropagation`, `SpinSystem`, `PlayerMoveSystem`, `ControllerSystem` | Module (production) | Legal: module depends on layer `ecs` (declared in `module.toml`). **This is the only real ECS consumer with systems.** |
| `modules/axiom-scene/module.toml` | `allowed_layers` includes `ecs` | Manifest | Legal |
| `crates/axiom-introspect/src/world_report.rs` | `WorldReport::observe(&World<S>)` — reads `entity_count`/`system_count` | Layer 06 (production) | Legal: introspect depends on ecs |
| `modules/axiom/src/bundle.rs` | App-facing `Bundle`/`SpawnCommand` recording — translates app spawns into scene commands; **does not touch `axiom-ecs` directly** | Module (umbrella) | Legal, but note: the umbrella reuses scene's component vocabulary, not the ECS facade |

No misplaced ECS code was found. No ECS logic leaks into the kernel or into apps.

---

## 3. Architecture Placement Assessment

**Classification: Layer.** The ECS lives at `crates/axiom-ecs` with a valid
`layer.toml` (Layer 05, `depends_on = ["kernel", "frame"]`), is enforced by the
architecture checker and its own `tests/architecture.rs`, and sits below the
modules that consume it.

**This placement is correct, and it should stay a layer**, per the audit rule:

> If ECS is intended to become the shared deterministic world-state substrate used
> by scene, physics, animation, rendering, networking, replay, and tooling, then it
> should be a layer.

The repo already demonstrates this intent concretely, not hypothetically:

- `modules/axiom-scene` builds its transform hierarchy as `WorldSystem`s over
  `axiom_ecs::World` (`scene_storage.rs`) — scene is a *semantic adapter over the
  ECS layer*, exactly the layer relationship.
- `crates/axiom-introspect` (Layer 06) observes the `World` — tooling/observability
  already depends on it.
- `layer.toml`'s own prose states it is "the single generic world store every
  feature module and app composes on."

So ECS is already the shared substrate for at least scene + introspection, and is
positioned for physics/animation/networking/replay to join. Demoting it to a module
would force those future consumers into module-to-module dependencies, which the
Module Law forbids. **Keep it a layer.** The gap is depth, not placement.

One caveat worth flagging: the ECS layer is generic over `S` and owns **no concrete
components**, which is what keeps it a clean substrate. That genericity is a
strength, but it also means the ECS today provides *storage mechanism* without
*query/scheduling machinery* — the consumers (scene) currently supply the iteration
logic by hand. That is the boundary the deepening work has to respect: richer ECS
machinery must stay component-agnostic.

---

## 4. Existing Capabilities

"Present" requires code that clearly proves it. Evidence cites files/symbols.

| Capability | Status | Evidence | Risk | Recommended next step |
|---|---|---|---|---|
| Entity identity | **Present** | `EntityRegistry::spawn` mints `EntityId` (`entity_registry.rs`); kernel `EntityId` is a serializable `u64` newtype (`id_macro.rs`) | Low | Keep |
| Generational entity IDs | **Missing** | `EntityId` (kernel `id_macro.rs`) is a bare `u64`: `from_raw`/`raw`/`is_valid`, no generation | High | Add a generation to the entity handle (in ECS, not necessarily kernel) |
| Entity allocation | **Partial** | `spawn` increments `next_id` from 1 monotonically (`entity_registry.rs`) | Medium | Allocation never recycles ids → unbounded id growth; introduce free-list + generations |
| Entity despawn | **Partial** | `EntityRegistry::despawn` removes from live set | High | **Despawn does NOT free component columns** — `world.rs::despawn` doc: "column cleanup is the consumer's." Orphaned component rows are a real hazard |
| Stale handle rejection | **Missing** | No generation to validate against; `ComponentColumn::get` keys purely on `EntityId` | High | Cannot detect a stale handle today; only sidestepped because ids are never reused |
| Component registration | **Partial** | Static path: consumer lists columns in `ColumnSet`. Dynamic path: `DynamicComponents` records a type's `TypeSchema` on first `insert` | Medium | No central `ComponentRegistry`; registration is implicit |
| Component type identity | **Partial** | `Reflect::SCHEMA` / `TypeSchema` name keys `DynamicComponents`; static columns are keyed by Rust type only | Medium | Type identity is a **schema name string** in the dynamic path (collision-prone — see §5/§9); no numeric `ComponentTypeId` |
| Component storage | **Present** | `ComponentColumn<T>` = `BTreeMap<EntityId, T>` (`component_column.rs`) | Low (correctness) / High (perf) | Works; storage model will not scale (see §9) |
| Archetype storage | **Missing** | No archetype type anywhere (repo-wide grep) | Medium | Decide sparse-set vs archetype before queries deepen |
| Sparse-set storage | **Partial** | Columns are sparse (entity present iff it has the component), but backed by `BTreeMap`, not a sparse-set (dense array + index) | High | `BTreeMap` is "sparse" but not cache-friendly; not a real sparse-set |
| Query API | **Missing** | No `Query`/`QueryPlan` type; systems hand-iterate (`scene_storage.rs::SpinSystem` filters `storage.spins.iter()`) | High | This is the single biggest functional gap; see Stage 2 |
| Query filters | **Missing** | No `With`/`Without`/changed filters | High | Follows the query API |
| Mutable query safety | **Missing** | No borrow/aliasing model; a system gets `&mut S` (whole storage) and self-polices | High | Add disjoint-access query borrows |
| System abstraction | **Present** | `WorldSystem<S>` trait (`world_system.rs`); real impls in `scene_storage.rs` | Low | Keep; generalize signature when queries land |
| System access declarations | **Missing** | `WorldSystem::run` takes `&mut S`; no declared read/write component sets | High | No `SystemAccess`; cannot schedule by access |
| Schedule/stage abstraction | **Partial** | `SchedulePhase::{Startup, Update}`; two ordered `Vec`s in `World` (`world.rs`) | Medium | Only two hardcoded phases; no general stage graph |
| Deterministic system ordering | **Present** | Systems run in registration order within a phase; `Startup` once before `Update` (`world.rs::advance`, tested) | Low | Keep; preserve when stages generalize |
| Command buffer | **Missing** | No `CommandBuffer` type; mutation is immediate | High | See Stage 3 |
| Deferred structural mutation | **Missing** | `spawn`/`despawn` mutate the registry immediately during a system? — actually systems can't spawn (they get `&EntityRegistry`, not `&mut`), so structural change is impossible mid-frame | High | Systems literally cannot spawn/despawn today; commands are the fix |
| Event buffers | **Missing** | No `EventBuffer`; scene fakes per-frame "events" as `pending_moves`/`pending_controls` `Vec`s in `SceneStorage` | Medium | Promote to a real ECS event/double-buffer primitive |
| Resource storage | **Missing** | No `ResourceStorage`; singletons are smuggled as fields on the consumer's `S` (e.g. `SceneStorage::pending_moves`) | Medium | Add typed resource store |
| Change detection | **Missing** | No change ticks; no `Added`/`Changed` | High | See Stage 5 |
| Snapshot support | **Partial** | `World::serialize`/`deserialize` round-trips all columns via `ColumnSet`/`ErasedColumn` (`world.rs`, tested incl. truncation-at-every-prefix) | Medium | Serializes **components only** — not entity liveness, not systems; "snapshot" is incomplete |
| Serialization | **Present** | `ComponentColumn::reflect_write/read`; `World::serialize`; deterministic ascending-id order; schema-versioned | Low | Strong. Extend to entity set |
| Replay support | **Missing** | No `ReplayLog`; no input/command journal | High | Determinism makes it *feasible*; nothing implements it |
| Deterministic tests | **Present** | `propagation_is_deterministic_across_runs` (`scene_storage.rs`); ascending-iteration tests (`entity_registry.rs`, `component_column.rs`) | Low | Keep; add cross-run world-hash test |
| Architecture tests | **Present** | `crates/axiom-ecs/tests/architecture.rs` (10 tests) + workspace `check-architecture` | Low | Strong |
| WASM compatibility | **Present** | Pure `rlib`, `std` collections only, no platform deps (`Cargo.toml`); arch test bans browser APIs | Low | Keep |
| No browser leakage | **Present** | `tests/architecture.rs::no_browser_or_js_bindgen_apis`, `no_dom_canvas_or_browser_globals`, `no_webgpu_or_webgl_apis` | Low | Keep |
| No scene/render/physics leakage inward | **Present** | ECS is generic over `S`; owns no transform/render/scene type; `lib.rs` + `layer.toml` confirm | Low | Keep — this is the crown jewel; protect it through the deepening |

---

## 5. Determinism Assessment

The ECS layer is **deterministic by construction**, and this is actively enforced.
Findings:

- **Wall-clock time — none.** `tests/architecture.rs::no_wall_clock_time_or_randomness`
  bans `std::time`, `SystemTime`, `Instant::now`. Logical time enters only as an
  explicit `tick: u64` through `World::advance` → `WorldStep` (`world_step.rs`). Good.
- **Random APIs — none.** Same arch test bans `rand::`, `thread_rng`, `getrandom`. Good.
- **Hash-map iteration order — not used.** Every ordered structure is a `BTreeSet`
  (`entity_registry.rs`) or `BTreeMap` (`component_column.rs`, `dynamic_components.rs`),
  so iteration is ascending-key on every platform. `ComponentColumn::iter` and
  `EntityRegistry::iter` are documented and tested as ascending. **No `std::HashMap`
  is used for semantic output anywhere in the ECS.** Good — this is the right call
  and the main determinism win.
- **Global mutable state — none.** `World` owns all state; no statics. Good.
- **Nondeterministic ordering — none in ECS.** System order is registration order
  within a fixed phase order (`world.rs::advance`, tested by
  `startup_runs_once_before_update_on_each_active_advance`). Good.
- **Floating-point — not in the ECS itself.** The ECS stores `T` opaquely; FP
  determinism is the consumer's. Scene systems do FP math
  (`scene_storage.rs::ControllerSystem` quaternion build) but that is module
  territory, and it is single-threaded and order-fixed. Acceptable, but note: a
  future multi-threaded scheduler would put FP-order determinism at risk.
- **Command application order — N/A (no commands).** When commands are added
  (Stage 3) this becomes a real determinism surface; today there is nothing to
  order because mutation is immediate and structural mutation mid-system is
  impossible.
- **System execution order — deterministic** (see above).
- **Serialization stability — strong.** Columns serialize in ascending entity-id
  order with a `SchemaVersion` header; `world.rs` tests reject incompatible schema,
  wrong column count, and truncation at *every* byte prefix. This is the best-proven
  determinism property in the layer.
- **Replayability — feasible but unbuilt.** All ingredients exist (deterministic
  order, explicit tick, stable serialization) but there is no `ReplayLog` and no
  command journal, so replay cannot be demonstrated.

**One concrete determinism *hazard* (not a violation today):**
`DynamicComponents` keys columns by `TypeSchema.name()` — a `&'static str`
(`dynamic_components.rs::insert` → `put_bytes(T::SCHEMA.name(), …)`). Two distinct
types sharing a schema name collide into one column; the code handles this
*gracefully* (decode error, not UB — proven by
`type_mismatch_fails_gracefully_not_unsafely`), but a name collision is a
**silent semantic merge** that depends on insert order. For a substrate that wants
replay/networking, stringly-typed component identity is a latent determinism and
correctness risk (see §9).

---

## 6. Layering and Boundary Risks

The ECS layer itself is **clean**: it owns no rendering, scene, physics, input,
asset, animation, audio, editor, browser, or gameplay concept. It is generic over
`S` and references only `axiom-kernel` (`EntityId`, serialization, `Reflect`) and
`axiom-frame` (`FrameContext`). Verified by `lib.rs`, `layer.toml`, and
`tests/architecture.rs`. No inward leakage.

The boundary observations are all **at the consumer (module) tier**, and none
violate the Module Law — but they reveal *missing ECS primitives* being improvised
in the scene module:

- **Transforms as scene semantics:** `modules/axiom-scene/src/scene_storage.rs`
  stores `ComponentColumn<Transform>` (`locals`, `worlds`) and computes the
  hierarchy in `TransformPropagation`. This is **correct** — per the audit, storing
  components in ECS is fine; the meaning lives in the scene module, not in ECS. No
  violation. The ECS does not know what a `Transform` is. Good.
- **Input/gameplay smuggled as component-store fields:** `SceneStorage` carries
  `pending_moves: Vec<(u32, Vec3)>` and `pending_controls: Vec<(u32, Vec3, f32, f32)>`
  plus `players`/`controllers` `BTreeMap`s. These are **per-frame events and
  resources** wearing a component-storage costume because the ECS offers neither an
  `EventBuffer` nor a `ResourceStorage`. This is not an inward leak (it is all in the
  module), but it is a **symptom**: the scene is reinventing ECS machinery the layer
  should provide. When events/resources land in the ECS (Stages 3/5), this code
  should move onto them.
- **System input plumbing:** `ControllerSystem`/`PlayerMoveSystem` drain those
  `Vec`s via `std::mem::take`. That is the manual stand-in for "read this frame's
  events." Again: a missing-primitive symptom, not a boundary breach.

**Net:** no ECS↔higher-system meaning leakage exists. The risk is the inverse — the
ECS is *too thin*, so consumers grow ad-hoc event/resource shapes that will later
need to be unified onto real ECS primitives.

---

## 7. Data Model Gap

Comparison against the target substrate. Repo-wide grep for the target names
returns **no matches** in engine code except `World`, `System` (as `WorldSystem`),
and the implicit pieces noted below.

| Target item | Status | Notes / actual name |
|---|---|---|
| `World` | **Exists** | `crates/axiom-ecs/src/world.rs` — `World<S>`, generic over consumer storage |
| `EntityId` | **Exists (different home)** | Kernel `EntityId` (bare `u64`, `id_macro.rs`); ECS re-keys on it |
| `EntityGeneration` | **Missing** | No generation concept anywhere |
| `ComponentTypeId` | **Partial / different name** | Implicitly `TypeSchema.name()` (`&'static str`) in `DynamicComponents`; static columns have no runtime type id |
| `ComponentRegistry` | **Missing** | Registration is implicit (`ColumnSet` listing, or first dynamic insert) |
| `ArchetypeId` | **Missing** | No archetypes |
| `ArchetypeStorage` | **Missing** | Storage is per-type `BTreeMap` columns (`ComponentColumn`) |
| `Query` | **Missing** | Systems hand-iterate columns |
| `QueryPlan` | **Missing** | — |
| `System` | **Exists (different name)** | `WorldSystem<S>` trait (`world_system.rs`) |
| `SystemAccess` | **Missing** | No declared read/write sets |
| `Schedule` | **Partial** | `World`'s two phase-`Vec`s + `advance`; no standalone `Schedule` type |
| `Stage` | **Partial / different name** | `SchedulePhase::{Startup, Update}` enum (two fixed stages only) |
| `CommandBuffer` | **Missing** | Mutation is immediate; systems cannot mutate structure |
| `EventBuffer` | **Missing** | Faked as `Vec` fields in `SceneStorage` |
| `ResourceStorage` | **Missing** | Faked as `BTreeMap`/`Vec` fields in `SceneStorage` |
| `ChangeTick` | **Missing** | `WorldStep.tick` is a *frame* tick, not a per-component change tick |
| `WorldChangeSet` | **Missing** | No change-set output |
| `WorldSnapshot` | **Partial / different name** | `World::serialize`/`deserialize` (components only; excludes entity liveness + systems) |
| `ReplayLog` | **Missing** | — |

**Summary:** of 20 target items, ~4 exist (often differently named/scoped), ~5 are
partial, and ~11 are missing. The existing pieces cluster entirely in **identity +
storage + serialization**; the entire **query / command / scheduling-by-access /
change-detection / replay** half of an ECS is absent.

---

## 8. Test Gap

The ECS is **well-tested for what it does** (36 unit tests + 10 architecture tests
in `crates/axiom-ecs`, all passing), and scene adds real system tests. But the tests
prove a small surface because the surface is small.

**Proven today:**

- Entity ids are stable, monotonic, ascending-ordered —
  `entity_registry.rs::{spawn_mints_monotonic_ids_from_one, iter_is_ascending_by_id}`.
- Components insert/read/mutate/remove — `component_column.rs::{insert_returns_previous_and_overwrites,
  get_get_mut_contains_present_and_absent, remove_present_and_absent}`.
- Column iteration is deterministic ascending — `component_column.rs::iter_is_ascending_by_entity_id`.
- Systems run in deterministic phase/registration order —
  `world.rs::startup_runs_once_before_update_on_each_active_advance`.
- Frame-gating: skipped / zero-step frames run nothing —
  `world.rs::{advance_skips_systems_for_a_skipped_frame, advance_skips_systems_when_no_runtime_step_ran}`.
- Serialization round-trips and rejects corruption —
  `world.rs::{whole_world_round_trips, deserialize_rejects_incompatible_schema,
  deserialize_rejects_wrong_column_count, deserialize_rejects_truncation_at_every_prefix}`.
- Transform propagation is deterministic across runs —
  `scene_storage.rs::propagation_is_deterministic_across_runs`.

**Missing tests (because the feature is missing — listed for completeness):**

- Stale ids fail after despawn/reuse — **no test, no feature** (ids never reuse;
  no generation to validate).
- Despawn cleans up component columns — **no test; behavior is the opposite**
  (columns are *not* cleaned; this absence should itself be a documented hazard).
- Archetype movement — no feature, no test.
- Query order is deterministic — no query API, no test.
- Query filters (`With`/`Without`/`Changed`) — none.
- Mutable conflicts are rejected — none (no access model).
- Commands apply only at barriers — none (no commands).
- Change detection works — none.
- Snapshots round-trip **including entity liveness** — partial: components
  round-trip, but no test asserts the live entity set survives (it isn't serialized).
- Replay produces identical results — none.
- Cross-run **world hash** equality — none (determinism is tested per-property, not
  via a single canonical hash).

---

## 9. Performance and Storage Gap

The audit separates "fine for now" from "will block serious ECS depth."

**Fine for now:**

- Per-type sparse columns (`ComponentColumn<T>`) are a reasonable *conceptual*
  model (an entity is in a column iff it has the component).
- Deterministic ordering via `BTreeMap` is the right *correctness* default for an
  engine that prizes replay; it is a deliberate, defensible trade.
- `DynamicComponents` byte storage is explicitly the **cold/app-blind path**
  (`dynamic_components.rs` doc: "the static `World` remains the zero-cost borrowed
  path for the hot loop"). Reasonable as-is.

**Will block serious ECS depth:**

- **`BTreeMap`-backed columns are not cache-local.** Every component access is a
  tree lookup (`component_column.rs::get` → `BTreeMap::get`), and iteration walks
  tree nodes, not a dense array. For per-frame iteration over thousands of entities
  this is `O(n log n)`-ish with pointer chasing — the opposite of the dense
  archetype/sparse-set iteration real ECS hot loops need. This is the **#1 storage
  blocker.**
- **No way to batch-query by component set.** A system that wants "all entities with
  A and B" must iterate one column and `get` into the other(s) per entity
  (`scene_storage.rs::SpinSystem`, `propagate`). There is no archetype to iterate
  the intersection directly; cost scales with the largest column, not the
  intersection.
- **No archetype or sparse-set strategy chosen.** The columns are "sparse" only in
  the loose sense; there is no dense backing store + sparse index. A real decision
  (archetype tables vs sparse-set) is still open and must be made before queries
  deepen, or the query layer will be built on the wrong storage.
- **Stringly-typed component identity in the dynamic path.** `DynamicComponents`
  keys on `TypeSchema.name()` (`&'static str`). This is both a perf cost (string
  hashing/compare via `BTreeMap<&str, …>`) and a **correctness hazard** (name
  collisions silently merge columns — §5). A numeric `ComponentTypeId` is needed.
- **Cloning in systems.** Because a system gets `&mut S` (the whole storage) and
  cannot hold overlapping borrows, scene systems collect into temporary `Vec`s
  before mutating (`scene_storage.rs::SpinSystem` collects `updates`, `propagate`
  builds a `BTreeMap` then writes back). This per-frame allocate-collect-writeback
  is a direct consequence of the missing query-borrow model.
- **Despawn leaks component memory.** `World::despawn` removes only the registry
  entry; columns retain the row indefinitely (`world.rs` doc). Over a long session
  with churn, this is an unbounded leak *and* a correctness trap (a future reused id
  would inherit stale components — currently masked only because ids never reuse).

**Verdict:** the storage is correct and deterministic but is a *map-of-maps*, not an
engine ECS. It is adequate for the current scene (a handful of nodes) and will not
survive contact with a real entity count or a real query API. The storage decision
is the gating design choice for everything in §11.

---

## 10. Proposed ECS Target Shape

**Location:** keep it where it is — `crates/axiom-ecs` (Layer 05). Do **not** split
it into a module; it is already the shared substrate (§3).

**Dependency direction:** unchanged — `depends_on = ["kernel", "frame"]`. Identity
(`EntityId`, `Reflect`, binary IO) comes from the kernel; per-frame advance gating
from frame. It must continue to depend on **nothing higher**, and no module may be
pulled down into it. Generations and `ComponentTypeId` should live **in the ECS
layer**, not the kernel — they are ECS concepts, and the kernel must stay free of
ECS (per the kernel rules). Only the bare identity primitive stays in the kernel.

**Public facade:** introduce a single curated facade type **`EcsApi`** as the entry
point, rather than the current 9-symbol re-export barrel in `lib.rs`. `lib.rs`
should expose `EcsApi` (plus the minimal value types a caller must name, e.g.
`EntityId` re-export, `SchedulePhase`), and the internal types (`ComponentColumn`,
`ErasedColumn`, `ColumnSet`, `WorldStep`, etc.) should be reachable **through** the
facade, not as a flat top-level surface. This matches the Module Law's "one public
facade" ethos and the audit's explicit instruction: *the ECS should not expose a
giant public barrel from `lib.rs`*. The existing
`tests/architecture.rs::lib_exports_are_curated_set` should be tightened to assert
the `EcsApi`-centered surface.

**What the ECS owns:**

- Entity identity + lifecycle, **with generations** (allocation, despawn, free-list,
  stale-handle rejection).
- Component storage (a chosen archetype **or** sparse-set strategy with dense
  backing + numeric `ComponentTypeId`), still **generic over component types** —
  the layer registers types, it does not name them.
- Queries + query filters + disjoint mutable-access borrows.
- A schedule (generalized stages) with deterministic ordering and
  declared `SystemAccess`.
- Command buffers (deferred structural mutation applied at barriers).
- Events and typed resources.
- Change detection (per-component change ticks).
- World snapshot (entities + components + change state) and a replay log.

**What the ECS must NOT own (unchanged invariant — protect it):**

- No `Transform`, scene graph, or transform-hierarchy *meaning* (stays in
  `axiom-scene`).
- No rendering, physics behavior, input devices, assets, animation, audio.
- No browser/GPU/editor/gameplay concepts.
- No wall-clock time, no RNG, no `HashMap`-ordered semantics. The existing arch
  tests already enforce this and must remain.

**How higher systems consume it later:**

- **Scene** (already): defines its component types + `WorldSystem`s; the
  transform-hierarchy stays a scene system over the ECS. Its `pending_moves`/
  `pending_controls`/`players`/`controllers` should migrate onto ECS
  **events + resources** once those exist.
- **Physics / animation:** new modules defining their own component columns +
  systems, scheduled by `SystemAccess`, never depending on scene.
- **Networking / replay:** consume `WorldSnapshot` + `ReplayLog` for
  rollback/lockstep — the deterministic ordering already in place is the enabler.
- **Rendering:** reads component data through queries to build render input; the
  app/feature-module tier translates ECS data into `RenderInput` (per existing
  module-isolation rules), so render never depends on scene/ECS internals.

---

## 11. Prioritized Roadmap

Each stage is additive, lands at 100% coverage and branchless (spine rules), and
must keep the inward-cleanliness arch tests green.

### Stage 1 — Identity and Storage
- **Goal:** real entity handles (id + generation), recycling allocation, and a
  storage decision (sparse-set or archetype) with numeric `ComponentTypeId` —
  replacing the bare-`u64` + `BTreeMap` model without losing determinism.
- **Files:** `crates/axiom-ecs/src/entity_registry.rs` (generations + free-list),
  a new entity-handle type in the ECS layer, `component_column.rs` /
  new storage module, `world.rs` (despawn frees components). Possibly a kernel
  note if the handle wraps `EntityId`.
- **Tests:** id+generation stability; **stale handle rejected after despawn/reuse**;
  despawn frees component rows; insert/read/mutate/remove; deterministic ascending
  iteration preserved; cross-run world hash equal.
- **Architecture risks:** keep generations in the ECS (kernel stays ECS-free);
  preserve determinism if moving off `BTreeMap` (dense store needs an explicit
  ordering); branchless rewrite of any new allocation logic.
- **Done when:** an entity handle round-trips, a stale handle is provably rejected,
  despawn leaves no orphan component data, and determinism + coverage gates pass.

### Stage 2 — Queries
- **Goal:** a `Query` API (+ `QueryPlan`) iterating entities matching a component
  set, replacing hand-rolled column iteration; basic filters (`With`/`Without`).
- **Files:** new query module(s) in `crates/axiom-ecs`; refactor
  `modules/axiom-scene/src/scene_storage.rs` systems onto queries (consumer change,
  not ECS).
- **Tests:** query yields exactly the matching set in deterministic order; filters
  include/exclude correctly; empty/●all● edge cases.
- **Architecture risks:** queries must stay component-type-agnostic; branchless
  iteration; determinism of result order independent of insert order.
- **Done when:** a multi-component query iterates the intersection deterministically
  and at least one scene system is ported to it.

### Stage 3 — Commands and Structural Mutation
- **Goal:** a `CommandBuffer` enabling systems to spawn/despawn/insert/remove,
  applied at a barrier — making mid-frame structural change possible (it is
  impossible today).
- **Files:** new command-buffer module; `world.rs::advance` (apply barrier);
  `world_system.rs` (system gets a command sink).
- **Tests:** commands apply only at the barrier, in deterministic order; spawn/
  despawn/insert/remove via commands; interaction with queries within a frame.
- **Architecture risks:** **command application order is a new determinism
  surface** — pin it; branchless application.
- **Done when:** a system can enqueue structural changes that apply deterministically
  at the barrier, proven by test.

### Stage 4 — Systems and Scheduling
- **Goal:** generalize `SchedulePhase` into a stage graph with declared
  `SystemAccess` (read/write component sets) and deterministic ordering; reject
  conflicting mutable access.
- **Files:** `schedule_phase.rs` → schedule/stage module; `world_system.rs`
  (access declarations); `world.rs` (scheduler).
- **Tests:** deterministic ordering across stages; conflicting access rejected;
  existing two-phase behavior preserved.
- **Architecture risks:** if parallelism is ever added, FP/order determinism must
  hold — keep execution serial-deterministic first.
- **Done when:** systems declare access, the scheduler orders them deterministically,
  and a conflict is rejected by test.

### Stage 5 — Change Detection
- **Goal:** per-component change ticks (`ChangeTick`) and `Added`/`Changed` query
  filters, producing a `WorldChangeSet`.
- **Files:** storage (change ticks per row); query filters; `world_step.rs`/`world.rs`
  (tick advance).
- **Tests:** changed detected, unchanged not; ticks advance deterministically;
  change set is stable across runs.
- **Architecture risks:** change ticks must not become wall-clock; stay logical.
- **Done when:** a `Changed<T>` query returns exactly the rows mutated since last
  run, deterministically.

### Stage 6 — Snapshots and Replay
- **Goal:** a complete `WorldSnapshot` (entities + generations + components +
  change state) and a `ReplayLog` (command/input journal) that reproduces a run
  byte-for-byte.
- **Files:** extend `world.rs` serialize/deserialize to include entity liveness +
  generations; new snapshot + replay-log modules.
- **Tests:** full snapshot round-trip incl. entity set; replay of a logged run
  yields an identical world hash at each tick; truncation rejection (extend existing
  pattern).
- **Architecture risks:** snapshot stability across schema changes; keep the
  schema-versioned, truncation-safe discipline already proven in `world.rs`.
- **Done when:** a recorded run replays to byte-identical snapshots tick-by-tick.

### Stage 7 — Integration Boundaries
- **Goal:** migrate consumers onto the new primitives and lock the boundaries:
  scene's `pending_*`/`players`/`controllers` move to ECS **events + resources**;
  introspection reports queries/change sets; document how physics/render/net consume
  the ECS.
- **Files:** `modules/axiom-scene/src/scene_storage.rs` (events/resources),
  `crates/axiom-introspect/src/world_report.rs`, module manifests if deps shift.
- **Tests:** scene behavior unchanged after migration (regression); arch tests
  still prove no inward leakage; module-isolation checks pass.
- **Architecture risks:** modules must not start depending on each other; render
  must consume via app/feature-module translation, not by importing scene/ECS
  internals.
- **Done when:** no consumer reinvents ECS machinery, all arch/coverage/branchless
  gates pass, and the ECS facade (`EcsApi`) is the single entry point.

---

## 12. Red Flags

Ordered by how much they will hurt if left alone:

1. **No generational handles + despawn leaks components (`world.rs`,
   `entity_registry.rs`).** Despawn frees the registry entry but not the component
   columns, and there is no generation to invalidate a handle. This is only safe
   today because ids are *never reused*. The moment recycling is added (needed to
   bound id growth), stale handles silently read a *different* entity's leftover
   components — a classic, hard-to-debug ECS corruption. Fix in Stage 1, together.
2. **`BTreeMap`-of-`BTreeMap` storage with no archetype/sparse-set (`component_column.rs`).**
   The storage model cannot support a real query layer or real entity counts.
   Building queries (Stage 2) on top of this will bake the wrong storage into the
   API. The storage decision must precede the query API.
3. **No query API; systems hand-iterate and allocate per frame
   (`scene_storage.rs`).** Every system reinvents iteration, collects into temp
   `Vec`s/`BTreeMap`s, and writes back — both a perf and a correctness-surface
   problem, and it means there is no enforced access model.
4. **No command buffer → systems cannot perform structural mutation at all.**
   Systems receive `&EntityRegistry` (not `&mut`), so spawn/despawn mid-frame is
   impossible. Any real gameplay (spawning bullets, despawning the dead) is blocked.
5. **Stringly-typed component identity in `DynamicComponents` (`TypeSchema.name()`).**
   Name collisions silently merge columns (handled as a decode error, but still a
   semantic merge dependent on insert order). A latent determinism/correctness risk
   for networking/replay; needs a numeric `ComponentTypeId`.
6. **Events and resources are faked as fields on the consumer's storage
   (`SceneStorage::{pending_moves, pending_controls, players, controllers}`).**
   Not a layering violation, but a clear signal the layer is too thin; this pattern
   will proliferate across future modules until the ECS provides the real
   primitives.
7. **"Snapshot" is components-only (`world.rs::serialize`).** It does not capture
   entity liveness or generations, so it cannot actually restore a world's identity
   state — a trap for anyone assuming `serialize`/`deserialize` is a true snapshot.

None of these are *bugs* in the current narrow scope — they are **depth debts** that
become bugs the instant the ECS is pushed toward real use.

---

## 13. Recommended Next Prompt

> **Task: Implement ECS Stage 1 — Identity and Storage — only.**
>
> You are extending `crates/axiom-ecs` (Layer 05). Do **only** Stage 1 from
> `docs/reports/ecs-gap-report.md`. Do not implement queries, commands, scheduling,
> change detection, snapshots, or replay. Do not touch consumers beyond what compiles.
>
> **Scope:**
> 1. Give entities a **generational handle** (id + generation) **inside the ECS
>    layer** — do **not** add generations to the kernel `EntityId`; the kernel must
>    stay ECS-free. Wrap/compose the kernel `EntityId` if useful.
> 2. Make `EntityRegistry` (`crates/axiom-ecs/src/entity_registry.rs`) **recycle**
>    despawned ids via a free-list, bumping the generation on reuse, so a handle
>    from before a despawn is detectably stale.
> 3. Make `World::despawn` (`crates/axiom-ecs/src/world.rs`) **free the entity's
>    component rows** so despawn leaves no orphan data. Component columns must drop
>    the despawned entity.
> 4. Add **stale-handle rejection**: reading/mutating a component with an outdated
>    generation must return `None`/fail cleanly, never another entity's data.
> 5. Keep storage deterministic. If you change `ComponentColumn`'s backing, preserve
>    ascending-entity-id iteration exactly (the current `BTreeMap` guarantee).
>
> **Constraints (non-negotiable):**
> - Stay within Layer 05: imports only `axiom-kernel` + `axiom-frame`. Do not pull
>   in any module. Keep the inward-cleanliness arch tests green
>   (`crates/axiom-ecs/tests/architecture.rs`).
> - **Branchless** in all non-test spine code (the `engine_no_branching` gate).
> - **100% coverage** for all new/changed code, in the same change.
> - No wall-clock, no RNG, no `HashMap`-ordered semantics (existing arch tests).
> - Update the curated `lib.rs` export test if the public surface changes.
>
> **Tests required (add in the same change):**
> - id + generation are stable across spawns; ids recycle after despawn with a
>   bumped generation.
> - a handle held across a despawn+respawn of the same slot is **rejected**
>   (stale-generation), and does not read the new entity's components.
> - despawn frees all of the entity's component rows (no orphan data).
> - insert/read/mutate/remove still work; ascending-id iteration preserved.
> - a cross-run world-hash equality test proving determinism is unaffected.
>
> **Definition of done:** `cargo test --workspace` green, `cargo xtask
> check-architecture` green, `cargo dylint --all -- --all-targets` green (branchless +
> rulebook), coverage at 100% for the changed crate, and no consumer behavior
> regressed. Do not start Stage 2.

---

## Appendix — Validation command result

Command run: **`cargo test --workspace`**

**Result: success (exit code 0). All tests passed; no failures.** The workspace
compiled and the full test suite (unit, integration, architecture, and doc tests
across every layer, module, app, and tool) completed green.

Representative confirmation for the audited layer, `cargo test -p axiom-ecs`:

```
Running unittests src\lib.rs ...
test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
Running tests\architecture.rs ...
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

No failing area to report. (Note: many crates show `0 tests` for the **doc-test**
phase specifically — these crates simply carry no doc-tests; their unit/integration
tests run and pass in the earlier phases.)
