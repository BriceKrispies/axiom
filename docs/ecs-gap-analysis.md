# ECS gap analysis — Axiom vs Bevy (`bevy_ecs` v0.19.0)

> Companion to [`bevy-ecs-architecture.md`](bevy-ecs-architecture.md) (how Bevy's
> ECS is built) and `crates/axiom-ecs` (Axiom's ECS, Layer 05). Bevy citations use
> `reference/bevy/crates/bevy_ecs/src/...` at commit
> `c6f634ca9f406d68ba5109d921247b654cb42c10` (tag `v0.19.0`); Axiom citations use
> `crates/axiom-ecs/src/...`.

## Why this document exists

Axiom's ECS is deliberately small and shaped by Axiom's laws (branchless,
deterministic, 100% coverage, `unsafe_code = "forbid"`, no runtime type
reflection). The risk with "small" is not knowing whether you're missing
something real or just missing something you correctly refused to build. This
analysis measures Axiom against the gold-standard Rust ECS and gives every gap a
**verdict**, so a future agent knows which gaps to close, which are already
covered elsewhere in Axiom, and which Axiom's own laws forbid.

## How to read the verdicts

| Verdict | Meaning |
|---|---|
| **ADOPT** | A genuine capability gap that *fits* Axiom's laws. Should be built. Each has a branchless/deterministic sketch + a roadmap phase. |
| **LOCATED-ELSEWHERE** | Axiom already has the capability, just not inside `axiom-ecs` (it lives in `axiom-scene`, the kernel, `axiom-runtime`, or `axiom-sim-core`). The split is usually deliberate — and sometimes Bevy splits it too. |
| **DEFERRED** | Already a known, documented gap (`crates/axiom-ecs/PHASE_1_DEFERRED.md`). Blocked on one specific redesign; not faked in the meantime. |
| **REJECT-BY-LAW** | Bevy's mechanism fundamentally conflicts with an Axiom invariant. Not a gap to close — a road not taken, with the trade-off stated honestly. |

---

## The two designs in one breath each

**Bevy** — archetypal, columnar, `TypeId`-keyed, `unsafe`-backed, thread-parallel.
Iteration speed is the prime directive; structural change pays for it; iteration
order is an implementation detail; correctness of parallelism rests on
access-disjointness reasoning over `UnsafeWorldCell`. Ergonomics via derive macros
and lots of automatic machinery (universal change ticks, hooks, observers,
auto-maintained relationships).

**Axiom** — sparse per-type `BTreeMap` columns, `ComponentTypeId`-keyed (FNV-1a of
a `Reflect` schema name — `crates/axiom-ecs/src/component_type_id.rs`), 100% safe,
single-threaded, **branchless**. Deterministic ascending-`EntityId` iteration is a
*contract* (it's what makes snapshot + replay byte-identical across platforms).
The world is generic over a consumer-defined `S: ColumnSet`
(`crates/axiom-ecs/src/column_set.rs`); systems are `Startup`/`Update` phases run
in registration order, frame-gated by `FrameContext`
(`crates/axiom-ecs/src/world.rs`).

Most of Bevy's defining choices are the *opposite* of Axiom's. So the headline is:
**the biggest "gaps" are deliberate rejections, and the real adoptable gaps are
narrow** — query expressiveness and one storage-seam redesign.

---

## Capability matrix

| Capability | Bevy | Axiom today | Where in Axiom | Verdict |
|---|---|---|---|---|
| Entities (id + generation, stale detection) | `Entity` (`entity/mod.rs`) | `EntityHandle` + `EntityRegistry` | axiom-ecs | **PRESENT** (parity) |
| Component storage | archetypal Tables + SparseSets | per-type `BTreeMap` column | axiom-ecs | **REJECT-BY-LAW** (archetypes) |
| Component identity | `TypeId → ComponentId` | `ComponentTypeId` (Reflect-name hash) | axiom-ecs | **REJECT-BY-LAW** (TypeId) |
| Type-erased storage | `unsafe` `BlobArray` | `ErasedColumn` (Reflect, safe) | axiom-ecs | **PRESENT** (safe variant) |
| Multi-component query | n-ary tuples + fetch | `Query::two` (max 2) | axiom-ecs | **ADOPT** |
| Query filters `With`/`Without`/`Or` | yes (`query/filter.rs`) | presence-only | axiom-ecs | **ADOPT** |
| Query filters `Added`/`Changed` | yes (tick-based) | `TrackedColumn` (not query-wired) | axiom-ecs | **ADOPT** |
| Change detection | universal, automatic ticks | opt-in `TrackedColumn<T>` | axiom-ecs | **PRESENT** (opt-in by design) |
| Removal detection | `RemovedComponents<T>` | `ChangeKind::Removed` in `TrackedColumn` | axiom-ecs | **ADOPT** (surface it) |
| Resources (typed singletons) | `Resource` on implicit entity | — | nowhere | **DEFERRED** |
| Component insert/remove commands | bundle inserter/remover | spawn/despawn only | axiom-ecs | **DEFERRED** |
| Bundles (spawn with components) | `Bundle` + derive | manual per-column insert | axiom-ecs | **ADOPT** (after seam) |
| Required components | `#[require(...)]` | — | nowhere | **ADOPT** (low prio, after seam) |
| System scheduling | DAG, `before`/`after`, sets, run-conditions | `Startup`/`Update`, registration order | axiom-ecs | **ADOPT** (ordered, no parallelism) |
| Parallel system execution | multithreaded executor | sequential | n/a | **REJECT-BY-LAW** |
| Deferred-apply barrier | `ApplyDeferred` | `CommandBuffer::apply` | axiom-ecs | **PRESENT** |
| Events (buffered) | `Messages<M>` double-buffer | `EventBuffer<E>` | axiom-ecs | **PRESENT** (single-buffer) |
| Events (reactive/triggered) | observers + `Event` | — (explicit drain instead) | axiom-ecs | **REJECT-BY-LAW** (implicit side effects) |
| Component lifecycle hooks | `on_add`/`on_remove`/… | — (explicit systems instead) | axiom-ecs | **REJECT-BY-LAW** (hidden side effects) |
| Hierarchy / parent-child | `ChildOf`/`Children` (relationships) | `parents` column + `set_parent` | axiom-scene | **LOCATED-ELSEWHERE** |
| Transform propagation | `bevy_transform` system | `TransformPropagation` system | axiom-scene | **LOCATED-ELSEWHERE** (Bevy splits it too) |
| Generic relationships | `Relationship`/`RelationshipTarget` | `RelationStore` | axiom-sim-core | **LOCATED-ELSEWHERE** |
| Cross-system messaging | — | `MessageQueue` | kernel | **LOCATED-ELSEWHERE** |
| Entity disabling | `Disabled` + default filters | — | nowhere | **ADOPT** (low prio) |
| Snapshot / replay | (not in core) | `World` snapshot + `ReplayLog` | axiom-ecs | **AXIOM-AHEAD** |
| Determinism guarantee | not by default (ambiguity detection) | byte-identical contract | axiom-ecs | **AXIOM-AHEAD** |

---

## ADOPT — genuine gaps that fit the laws

### A1. N-ary queries + `With`/`Without`/`Or` filters  *(highest value, lowest cost)*

**Bevy:** `query/fetch.rs` (tuple `QueryData`), `query/filter.rs`
(`With`@`:142`, `Without`@`:243`, `Or`@`:350`). **Axiom:** `Query::{one,one_mut,two}`
(`crates/axiom-ecs/src/query.rs`) caps at two columns and filters only on
presence + liveness.

This is a real expressiveness gap, and it is *pure addition* — no storage change,
no laws in tension. It's all iterator combinators, which is already how Axiom's
`Query::two` is written (`a.iter().filter_map(|…| b.get(…)).filter(registry.contains)`).

- **3+ columns:** add `three`/`four`, or a small generic join over an ordered set
  of columns, intersecting on ascending `EntityId` (a k-way merge of `BTreeMap`
  iterators — branchless, deterministic).
- **`With<T>` / `Without<T>`:** `…filter(move |(e,_)| with_col.contains(*e))`
  and `…filter(move |(e,_)| !without_col.contains(*e))`. Both are presence checks
  — no new machinery, fully branchless.
- **`Or`:** combine two presence predicates with `|` (the branchless boolean
  combiner Axiom already mandates).

**Roadmap:** Phase A. Independent, fully coverable, ships first.

### A2. Change/removal filters wired into queries

**Bevy:** `Added<T>`/`Changed<T>` (`query/filter.rs:727`/`:956`),
`RemovedComponents<T>` (`lifecycle.rs:510`). **Axiom:** the data already exists —
`TrackedColumn<T>` records `(ChangeKind {Added,Changed,Removed}, tick)` per entity
(`crates/axiom-ecs/src/tracked_column.rs`) — it just isn't exposed as query
filters or a removal stream.

- **`changed_since(tick)` / `added_since(tick)`:** iterate `TrackedColumn::changes()`
  (already ascending), `filter` by `kind` and `tick >= since`. Branchless.
- **Removal stream:** `changes().filter(|(_,k,_)| k == Removed)` — surfaces what
  Bevy's `RemovedComponents` gives, deterministically, without a side buffer.

**Why ADOPT not PRESENT:** the capability is half-built (storage yes, query access
no). Closing it is small. Note Axiom's change detection stays **opt-in per column**
(you choose `TrackedColumn` over `ComponentColumn`) — unlike Bevy's universal
automatic ticks — and that asymmetry is deliberate (see R5), not a gap.

**Roadmap:** Phase A (rides with A1).

### A3. Deterministic ordered scheduler (explicit `before`/`after`, run-conditions)

**Bevy:** `Schedule`/`ScheduleGraph` (`schedule/schedule.rs:382`/`:726`),
`SystemSet`, `before`/`after`/`chain`, run-conditions. **Axiom:** two phases
(`SchedulePhase::{Startup,Update}`, `crates/axiom-ecs/src/schedule_phase.rs`) run
in registration order; ordering is "whatever order you called `register_system`."

The *parallel* part of Bevy's scheduler is rejected (R3). But the **ordering
graph** is not — explicit, data-described dependencies that compile to a single
deterministic linear order are perfectly compatible with a single-threaded,
branchless, replayable engine, and remove the footgun of order-by-registration.

- Represent dependencies as data; topologically sort once into a `Vec<SystemId>`
  (a fixed, deterministic order — the sort is build-time, not hot-path).
- Run-conditions become a predicate evaluated branchlessly with `bool.then(|| run)`
  — exactly the pattern `World::advance` already uses to gate Startup/Update.

Scope check first: today scene hand-wires five systems in the right order
(`modules/axiom-scene/src/scene.rs`). Adopt this only once manual ordering is a
real maintenance hazard — otherwise it's ceremony. Note `axiom-runtime` already
owns a `RuntimeScheduler` (different concern: stepping the sim clock), so any ECS
scheduler must not duplicate it.

**Roadmap:** Phase C (optional / on demand).

### A4. Bundles, required components, entity-disabling  *(after the seam — see D1)*

**Bevy:** `Bundle` (`bundle/mod.rs:87`), required components
(`component/required.rs`), `Disabled` (`entity_disabling.rs`). **Axiom:** spawn
then insert each column by hand; no "spawn with these components" API.

- **Bundles / spawn-with-components** depend on a typed-component-insert seam (D1).
  Once that exists, a bundle is "apply these N typed inserts at spawn" — a thin,
  deterministic convenience.
- **Required components** are bundles' auto-insert cousin; same dependency, lower
  priority.
- **Entity disabling** is independently cheap: a `Disabled` marker column + a
  default `Without<Disabled>` on the new query filters (A1). Nice-to-have.

**Roadmap:** Phase D (rides on the seam) for bundles/required; entity-disabling can
land in Phase A as a one-liner once filters exist.

---

## DEFERRED — known gaps, blocked on one redesign

Both are documented in `crates/axiom-ecs/PHASE_1_DEFERRED.md`, and both are blocked
on the **same** thing, which is why they belong together.

### D1. Typed resources & component insert/remove commands → the `ColumnSet` typed seam

**Bevy:** resources are components on an implicit entity (`resource.rs:87`);
component insert/remove is the bundle inserter/remover doing archetype moves.
**Axiom:** neither exists, because the only mechanisms Bevy-style code would use
are *banned*:

- A type-keyed `get_mut<T> -> &mut T` store needs `TypeId` + `downcast` — tripped
  by the `engine_no_runtime_type_branch` dylint; or `unsafe` — tripped by
  `unsafe_code = "forbid"`. (PHASE_1_DEFERRED.md §"typed resource storage".)
- Staging a typed `insert(entity, value: T)` against the opaque `S: ColumnSet`
  needs a typed accessor on `ErasedColumn` (reintroducing the downcast + an
  unreachable mismatch arm the seam was designed to avoid). (§"insert/remove".)

**The unlock (one redesign, both features):** give `ColumnSet` a deterministic,
non-reflective **`ComponentTypeId`-keyed typed seam** — a consumer-populated
registry of `ComponentTypeId → typed inserter/slot`. With that seam:

- `CommandBuffer::insert_component`/`remove_component` become "look up the typed
  inserter by `ComponentTypeId`, apply at the barrier" — no `TypeId`, no `unsafe`.
- Typed resources are the same mechanism over single-slot storage.

This is the keystone of the whole roadmap: it's the one structural change, and it
turns three other items (D1's two features + A4's bundles) from "blocked" to
"thin." It must be designed deliberately (it's a storage-contract change), per the
No-Shortcuts rule — not bolted on.

**Roadmap:** Phase B (the seam) → Phase D (the features on top).

---

## LOCATED-ELSEWHERE — Axiom has it, outside `axiom-ecs`

### L1. Hierarchy & transform propagation → `axiom-scene`

Bevy's `ChildOf`/`Children` (`hierarchy.rs`) + `bevy_transform` propagation map
exactly to Axiom's `parents` column + `set_parent` and the `TransformPropagation`
`WorldSystem` (`modules/axiom-scene/src/{scene,scene_storage}.rs`). Notably **Bevy
also keeps transforms out of `bevy_ecs`** (they live in `bevy_transform`), so this
split is *agreement*, not a gap. Axiom's choice to keep hierarchy in scene rather
than ecs is the same instinct: the ECS stays a generic store; a consumer owns the
domain meaning.

### L2. Generic relationships → `axiom-sim-core`

Bevy's generic `Relationship`/`RelationshipTarget` (`relationship/mod.rs:111`)
with auto-maintained reverse links is, in Axiom, the `RelationStore` inside
`axiom-sim-core` (typed relations between subjects) — a richer, domain-level model
than parent/child. Axiom does **not** need generic relationships *in the ECS
layer*: hierarchy is in scene (L1), semantic relations are in sim-core. Adding a
third generic relationship system to `axiom-ecs` would be a junk-drawer overlap.

### L3. Cross-system messaging → kernel `MessageQueue`

Bevy has no kernel; its only message-like primitive is `Messages<M>`. Axiom
deliberately has two tiers: the typed per-consumer `EventBuffer<E>` in the ECS
(`crates/axiom-ecs/src/event_buffer.rs`) and the deterministic, `(tick, id)`-ordered
`MessageQueue` in the kernel. That's a *richer* split than Bevy, located correctly.

### L4. (Buffered events) `Messages<M>` ≈ `EventBuffer<E>` — one small delta

Axiom's `EventBuffer<E>` is single-buffer (push/drain). Bevy's `Messages<M>` is a
**double** buffer so a reader can see events from the current *and* previous frame
before they age out (`message/messages.rs:95`). If a consumer ever needs "events
survive one extra frame," that's a tiny additive enhancement to `EventBuffer`
(keep two `Vec`s, swap on `update`) — branchless and deterministic. Logged here as
a known minor delta rather than a roadmap item.

---

## REJECT-BY-LAW — Bevy mechanisms Axiom's invariants forbid

These are **not** gaps to close. Each is a deliberate fork in the road.

### R1. Archetypal / columnar storage

Bevy packs same-archetype components in cache-contiguous `Table` columns
(`storage/table/`), accepting archetype *moves* on structural change and treating
iteration order as an implementation detail. Axiom's per-type `BTreeMap` columns
exist **because** ascending-`EntityId` order is a determinism *contract* — it's
what makes `World` snapshots and `ReplayLog` byte-identical across platforms
(`crates/axiom-ecs/src/{component_column,world,replay_log}.rs`), and what lets
iteration be branchless (`for_each` over an ordered map, no archetype dispatch).

- **Conflicts with:** the determinism rules ("unstable iteration order" is
  forbidden; behavior must be replayable) and the Branchless Law (archetype
  matching/dispatch is branch-heavy).
- **Honest trade-off:** Axiom gives up cache-locality and O(1)-amortized dense
  iteration. For large, hot, homogeneous component sets Bevy will iterate faster.
  `BTreeMap` is pointer-chasing, not SoA.
- **Revisit only if:** profiling shows ECS iteration is a real bottleneck. Even
  then the fix is likely a *deterministic dense column* (an ordered `Vec` with a
  parallel index) — keeping ascending order — not Bevy's archetype graph.

### R2. `TypeId`-keyed component identity

Bevy's spine is `TypeId → ComponentId` (`component/info.rs`, `TypeIdMap`). Axiom
bans runtime type reflection (`engine_no_runtime_type_branch`) and keys components
by `ComponentTypeId` = FNV-1a of the `Reflect` schema *name*
(`crates/axiom-ecs/src/component_type_id.rs`) — a static, deterministic, portable
identity. The trade-off (two types with the same schema name collide) is documented
and accepted. This is why D1 must build a *non-reflective* seam.

### R3. Multithreaded executor & parallel query iteration

Bevy's `MultiThreadedExecutor` (`schedule/executor/multi_threaded.rs:88`) runs
systems whose declared access is disjoint (`is_compatible`@`:188`) on a thread pool
via `UnsafeWorldCell`; `par_iter` splits queries across threads. This needs
`unsafe` (forbidden), interior-mutable world access, and — critically — gives up
*deterministic order* unless every ambiguity is constrained (Bevy ships an
ambiguity *detector* precisely because the default is nondeterministic).

- **Conflicts with:** `unsafe_code = "forbid"`, the single `&mut World` borrow
  model, and the determinism contract.
- **Honest trade-off:** Axiom leaves multicore throughput on the table for ECS
  system execution. (Note Axiom *does* exploit parallelism where it's safe and
  explicit — e.g. the asset-streaming Web Worker pool — at app edges, not inside
  the deterministic spine.)
- **Revisit only if:** a future design can prove disjoint access *statically* and
  merge results in a deterministic order without `unsafe`. That's a research-grade
  change, not a gap.

### R4. Component lifecycle hooks & observers (implicit reactive dispatch)

Bevy's hooks (`lifecycle.rs:149`) and observers (`observer/`) run code as a *side
effect* of structural changes / triggers. Axiom's determinism rules explicitly
forbid "side effects that are not visible in the API." Axiom's equivalent is
**explicit**: a system that reads a `TrackedColumn`'s `Added`/`Removed` changes, or
drains an `EventBuffer`, in a known scheduled order. Same outcomes, no hidden
control flow — and it stays branchless and replayable.

- **Revisit only if:** a hook model can be expressed as scheduled, ordered,
  side-effect-visible data transforms — at which point it's just A2/A3, not Bevy's
  hook system.

### R5. Universal automatic change ticks

Bevy stamps `added`/`changed` ticks on **every** component of every entity, in
`UnsafeCell`s on each column (`storage/table/column.rs:25`), all the time. Axiom
makes change tracking **opt-in** (`TrackedColumn<T>` vs plain `ComponentColumn<T>`)
— you pay for it only where a consumer needs it. This is a deliberate
cost/explicitness choice, not a missing feature; "automatic everywhere" would add
overhead and implicit state to columns that never need it.

---

## Where Axiom is *ahead* of `bevy_ecs`

Worth stating, because a gap analysis that only lists deficits is dishonest:

- **Snapshot + deterministic replay as first-class ECS primitives** — full `World`
  serialization and `ReplayLog` (`crates/axiom-ecs/src/{world,replay_log}.rs`).
  Bevy has no core-ECS replay; it's a determinism *non-goal* by default.
- **Determinism as a guarantee, not a lint** — Axiom's byte-identical contract vs
  Bevy's "run the ambiguity detector and hope you constrained everything."
- **100% safe** — no `unsafe` anywhere; Bevy's performance rests on a large
  `unsafe` surface (`BlobArray`, `UnsafeWorldCell`, unchecked fetches).
- **Reflect-based type-erasure that's safe** — `ErasedColumn`
  (`crates/axiom-ecs/src/erased_column.rs`) serializes columns with no downcast and
  no unreachable arm, where Bevy's erasure is `unsafe` blob storage.

---

## Roadmap (recommended order)

Each phase is a self-contained, fully-covered, branchless change.

- **Phase A — query expressiveness (do first).** A1 (n-ary queries + `With`/
  `Without`/`Or`) and A2 (change/removal filters over `TrackedColumn`), plus the
  one-line entity-disabling filter once `Without` exists. Pure addition, no storage
  change, highest value per unit effort.
- **Phase B — the `ColumnSet` typed seam (keystone).** Design and add a
  deterministic, non-reflective `ComponentTypeId`-keyed typed-component seam on
  `ColumnSet`. Unlocks B-dependent work. This is the one storage-contract change;
  design it deliberately (PHASE_1_DEFERRED.md is the brief).
- **Phase C — ordered scheduler (on demand).** A3, only if hand-wired registration
  order becomes a maintenance hazard. Explicit `before`/`after` + run-conditions
  compiled to one deterministic order; never parallel.
- **Phase D — typed features on the seam.** D1 (typed resources + component
  insert/remove commands) and A4 (bundles, required components) — all thin once
  Phase B exists.

Explicitly **not** on the roadmap (and why): archetypes (R1), `TypeId` identity
(R2), parallel execution (R3), hooks/observers (R4), universal auto-ticks (R5).

---

## Bottom line

Axiom's ECS is not "behind" Bevy's — it sits at a different point in the design
space, trading raw single-machine throughput for **determinism, replay,
branchless uniformity, total memory safety, and agent-readability**. Measured
honestly against the gold standard, the genuinely adoptable gaps are narrow and
concrete: **richer queries** (Phase A) and **one storage seam** (Phase B) that
unlocks resources, component commands, and bundles (Phase D). Almost everything
else Bevy has that Axiom lacks is either already located elsewhere in Axiom
(hierarchy, relations, messaging) or a capability Axiom's laws correctly refuse to
build (archetypes, `TypeId`, `unsafe`, parallel execution, hidden side effects).
The smaller, fully-correct engine is the intended outcome — this analysis just
makes sure it's small *on purpose*, not by omission.
