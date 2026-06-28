# SPEC-02 — Entities, components, queries, hierarchy

> Status: Draft
> Contract: §4, §4.1   Vocabulary: Spawn/despawn + pooling, Offset-group / formation (transform hierarchy)   Determinism: sim

## 1. Summary

This projects the contract's `World` (§4) and its transform hierarchy (§4.1)
across the wasm boundary so a TS author can `spawn`/`despawn` entities, attach and
read components, run presence queries, and parent entities into rigid groups —
the substrate every game stands on. All 11 vocabulary games need entities; the
formation/attached-part/turret-on-tank games (the shmups, the platformer's
moving-platform riders, any vehicle) need the hierarchy.

It is **mostly a projection**, not new engine: the deterministic world model
(`axiom-ecs`) and the transform hierarchy (`axiom-scene`) already exist and are
proven. The work is the TS surface plus three small, precisely-scoped native gaps
(§2). No new module — adding one would duplicate the world model the Module Law
says must live in a single layer.

## 2. Current state (verified)

- **Entity lifecycle exists, deterministic.** `axiom-ecs` (Layer 05)
  `EntityRegistry` mints generational [`EntityHandle`]s from raw id 1, ascending
  by slot (a `BTreeMap`), recycling freed slots at a bumped generation — a stale
  handle is detectably invalid (`is_current`/`is_stale`). `World::spawn` /
  `spawn_handle` / `despawn(EntityId)` / `despawn_handle(EntityHandle)`; despawn
  drops every component row (`ColumnSet::remove_entity`), no orphans.
- **Structural change is barrier-applied.** `CommandBuffer` queues spawn/despawn
  FIFO and applies at an explicit barrier; `CommandReport::spawned()` returns the
  produced handles in order; `ComponentCommandBuffer` does typed component
  insert/remove. This is the determinism-safe path for systems that mutate
  structure mid-iteration.
- **Two component-store shapes exist.** The *static* borrowed path: `World<S>`
  over a consumer-defined `ColumnSet` of `ComponentColumn`s, queried by
  `Query::{one,two,three,four,two_opt,one_mut}` — zero-cost, ascending-id,
  live-filtered, but the column types are named at compile time. The *app-blind*
  path: `DynamicComponents`, a `Reflect`-keyed store of opaque serialized records
  with `insert`/`get`/`contains`/`remove` by `T: Reflect` — no compile-time
  knowledge of the type, a clean error on schema-name collision, no `unsafe`.
- **Transform + hierarchy exist** in `axiom-scene`. Nodes are entities
  (`SceneNodeId`); `set_parent`/`clear_parent` (cycle- and self-parent-rejecting),
  `parent_of`, `local_transform`, `world_transform`, `update_world_transforms`,
  `advance`. `axiom_math::Transform::combine` composes parent⊗child; propagation
  writes each node's `worlds` column from `locals`. The `Transform` component is
  the node's **local** transform; `world_transform` is the composed result.

**Native gaps (small, additive):**

1. **`childrenOf` — no reverse index.** Hierarchy stores only the child→parent
   edge (`parents` column). There is no enumeration of a node's children. Add a
   deterministic (ascending-id) children query in `axiom-scene`.
2. **Subtree despawn — no cascade.** `despawn_node` removes only the named entity;
   the contract requires despawning a parent to despawn its whole subtree. Add the
   cascade in `axiom-scene` (built on gap 1).
3. **Dynamic presence query by kind.** `DynamicComponents` can read/write a record
   by kind but cannot enumerate *the entities that have a kind*, nor intersect
   several kinds. The contract's `query(...kinds)` over author-declared (opaque)
   components needs this. Add a kind-keyed entity enumeration + all-kinds
   intersection to `DynamicComponents`, ascending-id — the dynamic mirror of
   `Query::two/three/four` (the "richer queries" the Bevy-ECS gap analysis flagged
   as the one genuinely adoptable query gap, kept inside the existing layer).

- **No TS surface.** The `World`/`Transform` interfaces do not exist anywhere in
  `packages/`; nothing author-facing projects them.

## 3. Architectural placement

Extend two existing spine units and add one projection. **No new module** — the
world model is one layer (`axiom-ecs`) by the Module Law's "shared primitive →
lower layer, not a third module" rule, and the hierarchy is already the scene
module's job.

1. **`axiom-ecs` (Layer 05) — extend `DynamicComponents`** with gap 3 (kind-keyed
   presence enumeration + intersection). This is the lowest correct layer: the
   dynamic store already owns the kind→column map; "which entities have this kind"
   is a read over data it already holds, not a new concept. `sim`-class,
   branchless, 100% covered. Legal: a layer extending its own primitive.

2. **`axiom-scene` (engine module) — add `children_of` and subtree `despawn`**
   (gaps 1–2). Hierarchy lives here; the reverse index and the cascade are reads
   and removals over the `parents` column the module already owns. `allowed_layers`
   unchanged (it already depends on `axiom-ecs`/`axiom-math`/`axiom-frame`);
   `allowed_modules = []` unchanged. `sim`-class, branchless, 100% covered.

3. **TS projection — owned by the runtime app `apps/axiom-game-runtime`
   (SPEC-00), surfaced through `@axiom/game`.** The `#[wasm_bindgen]` boundary owns
   the **entity handle table** (TS `Entity` number ↔ native `EntityHandle`) and the
   **component-kind registry** (TS `ComponentKind` ↔ `Reflect` schema name).
   Author-declared components are opaque records: the boundary serializes the JS
   value to bytes for `DynamicComponents` and back, never inspecting gameplay
   meaning (Vocabulary Law). The built-in `Transform` component is the one kind the
   boundary knows structurally — it routes to `axiom-scene`'s `locals` column and
   to `worldTransform`. This wiring is app-tier because it translates two module
   contracts (`axiom-ecs` opaque records ⊗ `axiom-scene` transforms) into the one
   `World` surface — exactly the cross-module glue only an app may write.

The split is the determinism boundary made physical: the **stores and queries are
deterministic spine** (ecs/scene), the **handle/kind tables are app-owned** (they
are opaque, re-bound on replay, never serialized into sim state), and the **`World`
verbs are the author's words** (TS).

## 4. API surface

### 4.1 Native

`axiom-ecs` — extend `DynamicComponents` (sim-class):

```rust
impl DynamicComponents {
    // Entities carrying component kind `name`, ascending id. (gap 3)
    pub fn entities_with(&self, name: &'static str) -> impl Iterator<Item = EntityId> + '_;
    // Entities carrying every kind in `names`, ascending id — the intersection
    // behind World.query(...kinds). Empty `names` is defined as no rows.
    pub fn entities_with_all(&self, names: &[&'static str]) -> Vec<EntityId>;
}
```

`axiom-scene` — extend `SceneApi` (sim-class):

```rust
impl SceneApi {
    // Direct children of `node`, ascending id (empty if none / missing). (gap 1)
    pub fn children_of(&self, node: SceneNodeId) -> Vec<SceneNodeId>;
    // Despawn `node` AND its whole subtree; returns whether `node` existed. (gap 2)
    pub fn despawn_subtree(&mut self, node: SceneNodeId) -> bool;
}
```

`despawn_subtree` is `despawn_node` made recursive over `children_of`; the
existing `despawn_node` stays as the single-node primitive it composes. Both are
barrier-safe (operate on committed state).

### 4.2 TS authoring projection (the contract, §4 + §4.1)

```ts
interface World {
  spawn(...components: Component[]): Entity;
  despawn(e: Entity): void;                                  // despawns the subtree (§4.1)
  alive(e: Entity): boolean;

  get<C extends Component>(e: Entity, kind: ComponentKind<C>): Result<C>;
  set<C extends Component>(e: Entity, value: C): void;       // add or replace
  remove<C extends Component>(e: Entity, kind: ComponentKind<C>): void;
  has(e: Entity, kind: ComponentKind): boolean;

  query(...kinds: ComponentKind[]): Entity[];                // entities having all kinds, stable order

  // §4.1 hierarchy
  setParent(child: Entity, parent: Entity | null): void;     // null detaches to the root
  parentOf(e: Entity): Result<Entity>;
  childrenOf(e: Entity): Entity[];
  worldTransform(e: Entity): Transform;                      // resolved (composed) for this tick
}

interface Transform extends Component {                      // the node's LOCAL transform
  position: Vec3;        // 2D uses z = 0 (or a layer index)
  rotation: number;      // radians about z for 2D; quaternion form for 3D (§11)
  scale: Vec3;
}
```

`World` is reached only through `Sim.world` (SPEC-00 §4.2) — there is no free
`World` constructor; the engine owns the world. `query` returns a **fresh,
stable-ordered array for the current tick**; the author must not retain it across
ticks.

## 5. Data contracts

- **`Entity`** — opaque number (contract §0.2), an index into the app's handle
  table over a native `EntityHandle`. **Never serialized into sim state**; a
  replay re-binds it. `alive(e)` is `is_current(handle)` — a stale handle reads
  `false`, and a stale handle passed to `get`/`set`/`despawn` is a clean no-op
  (mirroring `despawn_handle`'s stale-safety), never a throw.
- **`ComponentKind<C>`** — opaque kind token, an index into the app's kind
  registry over a `Reflect` schema name. The built-in `Transform` kind is reserved
  and routes to the scene's `locals` column; all other kinds are author-declared
  opaque records stored in `DynamicComponents`.
- **`Component`** — an opaque typed record. The engine stores its bytes and never
  reads gameplay meaning. `Transform` is the sole engine-defined member.
- **`Result<T> = T | null`** — query miss, absent component, `parentOf` of a root,
  or `get` on a dead entity all return `null`, not a throw (§0.2).

The handle table and kind registry are app-owned (SPEC-00 open question on table
ownership applies); they are bootstrap state, outside the coverage gate, never
part of the deterministic snapshot.

## 6. Determinism

This is a `sim`-class spec; it meets §17:

- **Stable iteration order is the load-bearing guarantee.** Every enumeration —
  `Query::*`, `DynamicComponents::entities_with*`, `children_of`, `node_transforms`
  — yields entities in **ascending `EntityId` order**, because the registry and
  every column are `BTreeMap`s keyed by id. `World.query(...)` therefore returns
  the same order every run and across machines (§17.4, §17.6); the order does not
  depend on insertion sequence, hashing, or pointer identity. An author iterating a
  query and mutating gets reproducible results.
- **Spawn ids are deterministic and replay-stable.** Slots mint monotonically and
  recycle LIFO with bumped generations, so a re-run with the same command stream
  reproduces identical `(slot, generation)` pairs (proven by
  `snapshot_restore_preserves_future_spawn_determinism`). Entities are opaque
  across the boundary, so the TS `Entity` numbers a replay sees may differ — only
  native ids and state hashes are guaranteed identical.
- **Structural change is barrier-ordered**, not interleaved with iteration:
  spawns/despawns route through `CommandBuffer` and apply FIFO at a known point, so
  two runs apply the same mutations in the same order.
- **No clock, no RNG, no raw input** enters any verb here — they are pure functions
  of world state and the command stream. Component records are author data; the
  engine never derives them from real time.
- **The hierarchy composition is deterministic arithmetic** (`Transform::combine`
  over `axiom-math`), so `worldTransform` reproduces bit-for-bit.

## 7. Acceptance / proof

Native (ships in the same change, the gate is part of done):

- `DynamicComponents::entities_with` / `entities_with_all`: 100% covered,
  branchless. Tests assert ascending-id order, the empty-`names` rule, the
  single-kind and multi-kind intersection (including a kind absent on some
  entities), and exclusion of a despawned entity's stale rows.
- `SceneApi::children_of`: ascending-id children, empty for a leaf and for a
  missing node. `SceneApi::despawn_subtree`: a parent with a multi-level subtree is
  fully removed (every descendant gone from every column), a leaf despawns as
  before, an absent node is a clean `false`. Golden: build parent→child→grandchild,
  `despawn_subtree(parent)`, assert `node_count == 0`.
- Both extensions: branchless, no console/junk-drawer, pass
  `cargo xtask check-architecture` and the dylints.

TS (`@axiom/game`, per `STATIC_ANALYSIS.md`): tsgo + Oxlint (branch ban) + 100%
coverage. A test game spawns entities with a built-in `Transform` and an
author-declared component, runs `get`/`set`/`has`/`remove`, `query(A, B)` and
asserts the returned array is the ascending intersection, parents a child and
checks `worldTransform` equals the composed transform, then `despawn`s the parent
and asserts the child is gone and `alive` is `false`. **Replay proof:** the same
authored command stream run twice yields the same per-tick state-hash sequence
(SPEC-00's harness), and tick-N vs tick-N+k differ only where the sim advanced
them.

## 8. Dependencies & order

- **Depends on SPEC-00** (the `Sim`/`World` boundary and handle tables) and is the
  `world` member of `Sim` (§2). It can land its native gaps independently of
  SPEC-00, but the TS surface needs SPEC-00's boundary first.
- **`Transform` math is SPEC-03-adjacent**: `worldTransform` and the `Vec3` fields
  reuse `axiom-math`; no new math is required here (composition already exists).
- **Everything spatial depends on this**: SPEC-03 spatial queries, the 2D/3D
  surfaces (SPEC-04/11), physics (SPEC-10), and netcode replication (SPEC-13, which
  snapshots the component store) all read entities/components defined here. Build
  it immediately after SPEC-01, before the surfaces (contract §18 step 3).

## 9. Open questions

- **Built-in `Transform` storage.** Does `Transform` live in `axiom-scene`'s typed
  `locals` column (fast, but the boundary special-cases one kind), or as a
  `Reflect` record in `DynamicComponents` like every other kind (uniform, but it
  loses the scene's propagation path)? Lean scene-typed: the hierarchy must compose
  it every tick, which the dynamic store cannot do — the special case is real, not
  convenience.
- **Component mutation granularity.** `set` replaces the whole record. Do authors
  need a mutate-in-place path for hot per-tick component edits, or is
  read-modify-`set` sufficient given the serialize cost of the dynamic store? Defer
  until a profiled consumer shows the round-trip matters; the static `Query`
  borrowed path already exists for engine-internal hot loops.
- **`query` result lifetime.** The array is per-tick; should the boundary hand back
  an iterator/cursor instead of an allocated array to discourage retention across
  ticks? Default to an array (matches the contract's `Entity[]`), revisit if
  retention bugs appear.
- **Re-parenting and world-transform timing.** `worldTransform` reads the last
  propagation; a `setParent` followed by a same-tick `worldTransform` read before
  the next `advance` returns the pre-reparent composition. Confirm the contract's
  "resolved transform for this tick" means post-`advance`, and document that
  authors read it in `onRender`/next tick, not mid-mutation.
