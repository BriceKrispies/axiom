# axiom-ecs — Phase 1 deferred items

Phase 1 strengthened the ECS into a trustworthy deterministic substrate
(generational handles, registry lifecycle + free list, despawn cleanup,
`ComponentTypeId`, queries, command buffer, event buffer, change detection, full
snapshot, replay log, `EcsApi`). Two sub-items were deliberately deferred. This
file records exactly what and why, so the boundary is explicit.

## Deferred: typed **resource storage** (`Resources`)

Typed resource storage — at most one value per type, with `insert<T>` / `get<T>` /
`get_mut<T> -> &mut T` / `remove<T>` keyed by the type — was implemented and then
removed, because the only safe mechanism for it is **banned by the engine's own
architecture lint**.

### Why (the structural blocker)

A heterogeneous, type-keyed store that hands back a borrowed `&mut T` requires
runtime type identity + downcasting (`std::any::TypeId` + `Box<dyn Any>` +
`downcast_mut`). That is *safe* Rust (no `unsafe`), but it trips the
`engine_no_runtime_type_branch` dylint:

> runtime type reflection is banned in non-test engine code; it defeats the
> engine's static, deterministic data model

So the two viable routes are both closed:

- **`Any` + `TypeId`** — banned by `engine_no_runtime_type_branch`.
- **`unsafe` downcasting** — banned by `unsafe_code = "forbid"` and the
  No-Shortcuts rule.

The `Reflect`-bytes route used by `DynamicComponents` cannot substitute: it keys by
schema name (not type) and can only return an **owned** `T` (values live as bytes),
so it cannot provide the required `get_mut -> &mut T`. A store generic over a single
consumer-defined `R` struct is not "typed resource storage by type" either — it is
just a struct, and offers none of the `insert<T>/get<T>` surface.

This is exactly the case the task anticipated ("if implementing typed resource
storage would require unsafe or broad architecture churn, stop and write a short
design note — do not hack it").

### Suggested home for the deferred work

Resource storage should be designed alongside the static-typed component-mutation
seam above: once `ColumnSet`/the world gains a deterministic, non-reflective
type-keyed seam (e.g. a registry of `ComponentTypeId`-keyed typed slots populated
by the consumer), the same mechanism gives type-keyed resources without runtime
type reflection. Until that seam exists, resources are deferred rather than
implemented with a banned mechanism.

---


## Deferred: generic component **insert/remove** commands in `CommandBuffer`

`CommandBuffer` implements `spawn` and `despawn` (structural lifecycle) and
applies them at an explicit barrier. It does **not** implement
`insert_component` / `remove_component` commands.

### Why (the structural blocker)

The world is generic over a consumer-defined storage `S: ColumnSet`. `ColumnSet`
exposes its columns only as **type-erased** `&dyn ErasedColumn` views whose
surface is `describe` / `entry_count` / `write` / `read_replace` /
`remove_entity` — none of which can accept a *typed component value* of an
arbitrary `T`. There is deliberately no `get<T>` / `insert<T>` on `ErasedColumn`:
`erased_column.rs` documents that avoiding a typed erased accessor is what keeps
the column seam free of `downcast` and of an unreachable mismatch arm.

To stage a typed `insert(entity, value: T)` and apply it later against an opaque
`S`, the command buffer would need one of:

1. a downcast-based typed path on `ErasedColumn` (`Any` + `downcast_mut`) keyed by
   `ComponentTypeId` — which reintroduces exactly the typed-erased accessor (and
   its unreachable-mismatch arm) the column seam was designed to avoid; or
2. a registry mapping `ComponentTypeId -> typed inserter closure` that the
   consumer populates per component type — a new registration subsystem; or
3. boxing component values as `Reflect` bytes and a per-type deserialize-into-
   column seam — i.e. promoting the `DynamicComponents` byte path into the static
   `World` and giving every `ColumnSet` a "apply bytes for type id" entry point.

Each is a real redesign of the storage/command contract, larger than Phase 1's
"smallest structurally correct" mandate and worth designing on its own. None can
be faked safely: doing it with `unsafe` or stringly-typed shortcuts is exactly
the kind of debt the No-Shortcuts rule forbids.

### What exists instead (no capability gap for Phase 1)

- Structural change through commands at a barrier works today via `spawn` /
  `despawn`, with FIFO ordering and clean stale-handle handling.
- Direct typed component mutation works immediately through `World::storage_mut()`
  and the typed `ComponentColumn<T>` / `TrackedColumn<T>` APIs (with change
  detection), and queries read/iterate them — the live path systems use now.

### Suggested home for the deferred work

A later phase should choose option (2) or (3) above (a typed-component
registration seam on `ColumnSet`), then add `CommandBuffer::insert_component` /
`remove_component` on top of it, keyed by `ComponentTypeId`. That is a storage-
contract change to design deliberately, not a patch to bolt on here.
