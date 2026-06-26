# How Bevy's ECS Is Built — an architecture map (`bevy_ecs` v0.19.0)

> Reference companion to [`ecs-gap-analysis.md`](ecs-gap-analysis.md). This is a
> *map of someone else's engine*, kept so a future Axiom agent can reason about
> what Bevy actually does instead of from memory.

## Source under study

- **Repo:** `github.com/bevyengine/bevy` — the real engine ("Bevy is a
  refreshingly simple data-driven game engine built in Rust").
- **Tag:** `v0.19.0` · **Commit:** `c6f634ca9f406d68ba5109d921247b654cb42c10`
- **Clone:** `reference/bevy/` (git-ignored via `/reference/` in `.gitignore` —
  never part of the Axiom build).
- **Crate:** `reference/bevy/crates/bevy_ecs/` (154 source files).

All `file:line` citations below are relative to
`reference/bevy/crates/bevy_ecs/src/` at that commit. Line numbers are accurate
to the pinned SHA; treat them as "this type, near here" if Bevy is later updated.

> **Version note.** v0.19.0 is recent. Two things differ from the Bevy most docs
> describe: (1) the old `Events<E>` buffered channel is now **`Messages<M>`**
> (`message/`), while **`Event`** (`event/`) now means an *observer-triggered*
> event; (2) **relationships** (`relationship/`, `ChildOf`/`Children`) and
> **observers** (`observer/`) are first-class. Both matter for the gap analysis.

---

## 1. The shape of Bevy's ECS in one paragraph

Bevy is an **archetypal, columnar, parallel** ECS optimized for cache-friendly
iteration and multi-threaded execution, with ergonomics delivered by derive
macros and a function-as-system model. Components of the same set live together
in a **Table** (struct-of-arrays); adding/removing a component **moves** the
entity to a different archetype/table. Type identity flows from Rust's `TypeId`
into a dense `ComponentId`. Queries compile to per-archetype **fetches** whose
declared **access** (read/write sets) lets a graph **scheduler** run
non-conflicting systems on a thread pool. The whole thing leans hard on `unsafe`
(type-erased blob storage, `UnsafeWorldCell`, `UnsafeCell` ticks) to make the
safe, ergonomic surface fast. Every one of those four traits —
archetypal/columnar, `TypeId`-keyed, `unsafe`-backed, thread-parallel — is a
place Bevy and Axiom diverge.

---

## 2. Entities

`Entity` is a 64-bit id split into an index and a generation
(`entity/mod.rs` — `EntityIndex(NonMaxU32)` at `:151`, `EntityGeneration(u32)` at
`:252`, the packed `Entity` `#[repr(C, align(8))]` struct ~`:424`). The
`NonMaxU32` index gives a niche (so `Option<Entity>` is free) and reserves a
`PLACEHOLDER`. Stale-handle detection is by generation: `Entities::get_spawned`
(~`:846`) compares the handle's generation to the live `EntityMeta.generation`
and rejects a mismatch.

The `Entities` registry (~`:827`) is a parallel `Vec<EntityMeta>` where
`EntityMeta { generation, location: Option<EntityLocation>, spawned_or_despawned }`
(~`:1181`). `EntityLocation` (~`:1210`) is the dual index that makes archetypal
storage work — it carries **both** `{archetype_id, archetype_row}` **and**
`{table_id, table_row}`. Allocation uses a free list and a `RemoteAllocator`
(~`:718`) for lock-free *reservation* off the world (e.g. from `Commands`),
though actual spawn/despawn still needs `&mut World`.

- **TypeId:** no. **unsafe:** `set_location`/`mark_free` are `unsafe` (caller
  proves the slot state). **parallel:** atomic free-list reservation.

---

## 3. Storage — Tables, Columns, BlobArray, SparseSets

Each component picks one of two storage strategies at registration
(`StorageType` enum, `component/mod.rs:728`):

- **`Table`** (default) — cache-friendly dense columns, slow add/remove.
- **`SparseSet`** — fast add/remove, slower iteration.

**Table** (`storage/table/mod.rs:202`) is `{ columns: SparseSet<ComponentId,
Column>, entities: Vec<Entity> }` — a struct-of-arrays where row *r* is one
entity across every column. A **`Column`** (`storage/table/column.rs:25`) is
`{ data: BlobArray, added_ticks, changed_ticks, changed_by }` — the component
bytes plus **parallel per-row change-detection ticks** in `UnsafeCell<Tick>`.

**`BlobArray`** (`storage/blob_array.rs:16`) is the type-erased heart: a raw
`NonNull<u8>` + `Layout` + an `Option<unsafe fn(OwningPtr) drop>`. Elements are
addressed by `index * layout.size()`. This is where the ECS stops being typed —
everything below is bytes plus a layout and a drop fn.

**`ComponentSparseSet`** (`storage/sparse_set.rs:157`) is the other storage: a
dense `Column` + a `sparse: SparseArray<EntityIndex, TableRow>` giving O(1)
presence/lookup and O(1) swap-remove.

- **TypeId:** no (uses `ComponentId`). **unsafe:** pervasive — every
  `initialize`/`replace`/`get_unchecked`/`swap_remove_unchecked` is `unsafe`,
  guarded by layout invariants. **parallel:** the `UnsafeCell` ticks exist so
  change-detection can be written without `&mut Table`.

---

## 4. Archetypes — the graph that makes add/remove fast

An **`Archetype`** (`archetype.rs:383`) is the set of component types an entity
has: `{ id, table_id, edges: Edges, entities: Vec<ArchetypeEntity>, components:
SparseSet<ComponentId, ArchetypeComponentInfo>, flags }`. Multiple archetypes can
share a `Table` if they differ only in sparse-set components.

The performance trick is **`Edges`** (`archetype.rs:206`): a cache of archetype
transitions keyed by `BundleId` —
`{ insert_bundle, remove_bundle, take_bundle }`. Inserting a component bundle
follows (or computes once, then caches) an edge to the destination archetype, so
the structural move is a graph hop, not a search. The move itself: allocate a row
in the target table, bit-copy each column, update `EntityLocation`, swap-remove
the old row. `ArchetypeFlags` (`archetype.rs:363`) bitflag which lifecycle hooks
exist so the hot path can early-out.

This archetype-move-on-structural-change is the defining cost model of an
archetypal ECS: iteration is maximally cache-friendly, but adding/removing a
component is comparatively expensive and **invalidates** any `EntityLocation`.

---

## 5. Components & registration

`Component` (`component/mod.rs:511`) is a trait with `const STORAGE_TYPE`
(`:513`), an associated `Mutability` (`Mutable`/`Immutable`), five optional
lifecycle **hooks** (`on_add`/`on_insert`/`on_discard`/`on_remove`/`on_despawn`),
`register_required_components`, clone behavior, and entity-mapping. It's usually
derived (`#[derive(Component)]`).

Registration maps a Rust type to a dense **`ComponentId`** (`component/info.rs`,
id ~`:175`) via the `Components` registry (~`:369`): `{ components:
Vec<Option<ComponentInfo>>, indices: TypeIdMap<ComponentId>, queued:
RwLock<...> }`. The `TypeIdMap` is exactly the `TypeId → ComponentId` bridge.
`ComponentDescriptor` (~`:213`) carries the `Layout`, `drop` fn, `Option<TypeId>`
(None for runtime/scripting components), and mutability — the data `BlobArray`
needs. **Required components** (`component/required.rs:119`) let a component
declare others that are auto-inserted, stored as constructor closures.

- **TypeId:** yes — `TypeIdMap<ComponentId>` is the core type→id bridge.
  **unsafe:** descriptor/drop-fn contracts. **parallel:** `RwLock` queued
  registration.

---

## 6. The World and its unsafe cell

`World` owns entities, archetypes, tables, sparse sets, components registry,
resources, and `Schedules`. Resources (`resource.rs:87`, `pub trait Resource:
Component {}`) are stored as components on implicit entities
(`ResourceEntities(SyncUnsafeCell<SparseArray<ComponentId, Entity>>)` ~`:90`) —
i.e. a resource is "a component on a singleton entity."

The pivotal type is **`UnsafeWorldCell`** — a raw, lifetime-branded handle to the
world that hands out *disjoint* borrows the borrow-checker can't prove safe. It's
what lets `n` systems each hold `&World`-ish access to *different* components in
parallel. `DeferredWorld` is the restricted view hooks/observers get (structural
changes deferred). `EntityRef`/`EntityWorldMut` are the safe per-entity views.

- **unsafe:** `UnsafeWorldCell` is the foundation of Bevy's parallel safety story
  — correctness rests on access disjointness proven *elsewhere* (queries +
  scheduler), not by the type system at the call site.

---

## 7. Queries — fetch, filter, access, parallel iteration

A query is split into **data** and **filter**, both built on
`WorldQuery` (`query/world_query.rs:44`, an `unsafe trait`):

- **`QueryData`** (`query/fetch.rs:324`) — what you read: `&T`, `&mut T`, tuples,
  `Entity`, `Option<&T>`, etc. Carries `IS_READ_ONLY`/`IS_DENSE`, a per-archetype
  `Fetch`, and an `unsafe fn fetch(...)` that produces `Item<'w,'s>` for a row.
- **`QueryFilter`** (`query/filter.rs:84`) — what gates matching:
  - `With<T>` (`:142`) / `Without<T>` (`:243`) — **archetypal**: resolved by set
    membership (`access.and_with`/`and_without`), zero per-entity cost.
  - `Added<T>` (`:727`) / `Changed<T>` (`:956`) — **non-archetypal**: per-row tick
    comparison in `filter_fetch`.
  - `Or<(...)>` (`:350`) — disjunction of child filters' access.

**`QueryState<D, F>`** (`query/state.rs:79`) is the cache: `world_id`,
`archetype_generation`, `matched_tables`/`matched_archetypes` (`FixedBitSet`),
the merged `component_access: FilteredAccess`, and `is_dense`. When new archetypes
appear it incrementally matches them. The **`FilteredAccess`/`Access`**
(`query/access.rs`) read/write component-id bitsets (with an "inverted set" trick
for "all except") are the currency the scheduler uses to decide parallelism.

Parallel iteration: `QueryParIter` (`query/par_iter.rs:16`) splits matched
tables/archetypes into batches and runs them on `bevy_tasks::ComputeTaskPool`,
falling back to sequential on wasm/single-thread.

- **TypeId:** indirect (via `ComponentId` in state). **unsafe:** the whole fetch
  path is `unsafe` and trusts the declared access. **parallel:** `par_iter` +
  batching.

---

## 8. Systems, SystemParam, and Commands

A **`System`** (`system/system.rs:48`) has `In`/`Out`, `SystemStateFlags`
(`NON_SEND`/`EXCLUSIVE`/`DEFERRED`, ~`:24`), an `unsafe fn run_unsafe(&mut self,
input, UnsafeWorldCell)`, and `apply_deferred`. Functions become systems via
`IntoSystem`/`FunctionSystem` (`system/function_system.rs`), where each argument
implements **`SystemParam`** (`system/system_param.rs:218`).

`SystemParam::init_access` is the contract that makes parallelism sound: each
param **declares** its world access into a `FilteredAccessSet`, panicking on an
internal conflict (e.g. `&mut T` + `&T` of the same component in one system).
`Query`, `Res`/`ResMut`, `Commands`, `Local`, `EventReader`, etc. are all
`SystemParam`s.

**`Commands`** (`system/commands/mod.rs:105`) is deferred structural mutation. The
backing **`CommandQueue`** (`world/command_queue.rs:34`) is a *type-erased packed
byte buffer*: `{ bytes: Vec<MaybeUninit<u8>>, cursor, panic_recovery, caller }`,
each command stored as `[meta | payload]` with a fn-pointer that applies it and
advances the cursor. Applied at a barrier (`ApplyDeferred`).

- **TypeId:** `System::system_type()` returns `TypeId::of::<Self>()`. **unsafe:**
  `run_unsafe`, the packed command buffer. **parallel:** access declaration is
  *for* the scheduler.

---

## 9. Scheduling — the graph and the multithreaded executor

A **`Schedule`** (`schedule/schedule.rs:382`) holds a **`ScheduleGraph`** (~`:726`)
— a DAG of systems and `SystemSet`s with `before`/`after`/`chain` dependencies,
hierarchy, and allowed-ambiguity edges — which is compiled (topological sort +
build passes, including auto-insertion of `ApplyDeferred`) into an executable
`SystemSchedule` (`schedule/executor/mod.rs:74`) listing per-system dependency
counts and dependents.

The **`SystemExecutor`** trait (`schedule/executor/mod.rs:30`) has two real
implementations; default is multithreaded off-wasm. The
**`MultiThreadedExecutor`** (`schedule/executor/multi_threaded.rs:88`) precomputes
a conflict matrix: for every pair of systems,
`if !system2.access.is_compatible(&system1.access)` (`:188`) marks them mutually
exclusive. At runtime it tracks `num_dependencies_remaining`, `ready_systems`,
`running_systems`, runs ready+non-conflicting systems on `ComputeTaskPool`, and
serializes non-send systems to the main thread and exclusive systems against
everything. **Ambiguity detection** flags system pairs that have *no* ordering
yet *do* conflict (nondeterministic order). `ApplyDeferred`
(`schedule/executor/mod.rs:160`) is a no-op `EXCLUSIVE | NON_SEND` marker the
executor special-cases to flush command buffers.

- **unsafe + parallel:** this is the core of both — disjoint-access reasoning over
  bitsets, executed on a thread pool through `UnsafeWorldCell`. **Determinism:**
  *not* guaranteed across runs unless ordering is fully constrained; ambiguity
  detection is the tool to find the gaps.

---

## 10. Change detection

`Tick(u32)` (`change_detection/tick.rs:18`) with wrapping arithmetic;
`Tick::is_newer_than(last_run, this_run)` (~`:52`) is the comparison `Added`/
`Changed` use. Per component row there are two ticks (`ComponentTicks { added,
changed }` ~`:137`) stored in the column's `UnsafeCell<Tick>` arrays.
`Ref<T>`/`Mut<T>`/`ResMut<T>` (`change_detection/params.rs`) wrap access so that
`DerefMut` bumps the `changed` tick automatically; the `DetectChanges` trait
exposes `is_added`/`is_changed`/`last_changed`. Wraparound is bounded by
`MAX_CHANGE_AGE` (`change_detection/mod.rs:26`) plus a periodic
`check_change_ticks` clamp. This is *universal and automatic* — every component
carries change ticks whether or not anyone queries them.

---

## 11. Messages vs Events (the v0.19 split)

- **`Messages<M>`** (`message/messages.rs:95`; trait `Message`
  `message/mod.rs:100`) is the **buffered, pull** channel (the old `Events<E>`):
  a double buffer swapped once per frame (`.update()`), read by cursor-tracking
  `MessageReader` (parallel-safe) and written by `MessageWriter` (exclusive).
- **`Event`** (`event/mod.rs:88`; `EntityEvent` `:93`) is now the **trigger,
  push** model: `World::trigger(event)` synchronously runs **observers**. The
  associated `Trigger` type (`event/trigger.rs`) controls distribution
  (global vs entity-targeted, with propagation up a relationship chain).

Use **Messages** for decoupled batch flow; **Events+observers** for immediate
reactive logic.

---

## 12. Observers, lifecycle hooks, removal detection

- **Lifecycle hooks** (`lifecycle.rs:149`, `ComponentHooks`): up to five fn
  pointers per component (`on_add`/`on_insert`/`on_discard`/`on_remove`/
  `on_despawn`), each `for<'w> fn(DeferredWorld, HookContext)` (`:80`). Archetype
  flags gate dispatch.
- **Observers** (`observer/distributed_storage.rs:207`, `Observer`): a system
  spawned *as a component on an entity* that reacts to a triggered `Event`
  (lifecycle or custom), received via the `On<E, B>` param
  (`observer/system_param.rs:38`). A central `Observers` cache
  (`observer/centralized_storage.rs:26`) has fast slots for the five lifecycle
  events. Entity events can **propagate** along a relationship.
- **Removal detection** (`lifecycle.rs:510`, `RemovedComponents<T>`): a
  `SystemParam` reading a per-component `Messages<RemovedComponentEntity>` buffer
  via a cursor — i.e. removals are surfaced *as messages*.

---

## 13. Relationships & hierarchy

The generic **`Relationship`** trait (`relationship/mod.rs:111`) +
**`RelationshipTarget`** (`:114`) model a one-to-many link whose **reverse side is
auto-maintained by lifecycle hooks**: inserting the source component runs an
`on_insert` hook that updates the target's collection; removal cleans it up.

The canonical instance is hierarchy (`hierarchy.rs`): `ChildOf(Entity)` (`:107`)
with `#[relationship(relationship_target = Children)]`, and `Children(Vec<Entity>)`
auto-maintained on the parent; despawning a parent recurses. Spawning related
trees uses `Spawn`/`SpawnableList` (`spawn.rs`). **Transforms are not in
`bevy_ecs`** — they live in `bevy_transform`, propagated by a system over this
hierarchy. (This is the one place Bevy, like Axiom, keeps transforms *out* of the
core ECS.)

---

## 14. Smaller pieces

- **Bundles** (`bundle/mod.rs:87`, `Bundle`): a static, ordered set of components
  inserted together; `BundleInfo` (`bundle/info.rs:63`) caches the contributed +
  required `ComponentId`s; `BundleInserter`/`BundleRemover` drive the archetype
  move. `DynamicBundle` supports runtime-decided components.
- **Entity disabling** (`entity_disabling.rs`): a `Disabled` component +
  `DefaultQueryFilters` resource hide entities from queries without removing them.
- **Name** (`name.rs:40`): `Name(HashedStr)` — a non-unique debug/identity label.
- **Error handling** (`error/`): systems/observers/commands can be fallible
  (`Result<_, BevyError>`) routed through pluggable handlers.

---

## 15. Bevy's ECS design DNA (what the gap analysis weighs against)

1. **Archetypal + columnar** — iteration speed is the prime directive; structural
   change pays for it (archetype moves) and iteration order is an implementation
   detail, not a contract.
2. **`TypeId`-keyed** — `TypeId → ComponentId` is the spine; component identity is
   the Rust type.
3. **`unsafe`-backed** — type-erased `BlobArray` storage and `UnsafeWorldCell`
   make the safe/ergonomic surface fast; soundness rests on access reasoning, not
   the local type system.
4. **Thread-parallel by default** — the scheduler exists to run disjoint-access
   systems concurrently; determinism of order is opt-in via explicit constraints.
5. **Automatic & universal** — change ticks on every component, hooks/observers,
   required components, auto-maintained relationships: lots of implicit machinery
   for ergonomics.
6. **Derive-macro ergonomics** — `#[derive(Component/Bundle/Event/Resource)]` make
   the surface friendly; the cost is proc-macro magic and a large API.

Each of these is a deliberate axis. The next doc takes Axiom's ECS — which makes
the *opposite* call on most of them (deterministic order as a contract, no
`TypeId`, no `unsafe`, no implicit parallelism, branchless, minimal) — and asks,
gap by gap, which Bevy capabilities Axiom should adopt, which it already has
elsewhere, and which its own laws correctly forbid.
