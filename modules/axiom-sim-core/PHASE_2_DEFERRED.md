# axiom-sim-core — Phase 2 deferred items

Phase 2 delivered the full generic substrate: deterministic identity types, the
fact / relation / definition / process / effect models, the causal journal, the
`SimWorld` state, ECS-handle references with liveness rejection, and the
`SimCoreApi` facade. Two sub-items were deliberately deferred. This file records
exactly what and why so the boundary is explicit. Nothing is faked.

## 1. Full sim-world byte snapshot / serialization

**Deferred:** a `snapshot`/`restore` (byte round-trip) of the *entire* sim world —
all five stores plus their id counters.

**What exists instead:** the six identity types (`FactId`, `RelationId`,
`DefinitionId`, `RuleId`, `ProcessId`, `CausalEventId`) implement the kernel
`Reflect` binary format and have round-trip + truncation tests. Whole-world
**determinism** is proven structurally rather than by bytes:
`tests/integration.rs::same_sequence_produces_identical_state` runs the full
process/effect chain twice on fresh worlds and asserts identical ids and values.

**Why (the structural reason):** a faithful byte snapshot requires a *branchless*
tagged-union codec for every heterogeneous value in the substrate — `FactValue`
(5 variants), `Effect` (8 kinds), `CauseRef` (4 variants), `RelationEndpoint`,
`CausalEvent`, and each store's id counter and free-ordering. Deserializing a
tagged union branchlessly means a per-type read-dispatch table with clean
out-of-range handling, written and covered for each of those types. That is a
substantial, self-contained serialization pass — larger than "the smallest
structurally correct version of Phase 2" — and is best designed as its own unit of
work rather than bolted on here. The kernel `Reflect` surface also covers only
`u32/u64/f32/bool/EntityId` (not `i64`), so several payloads would need bespoke
two's-complement encoders, reinforcing that this is its own effort.

**Suggested home:** a dedicated snapshot pass that adds a branchless
`Reflect`-style codec to each value/store type and a `SimWorld` snapshot/restore,
with round-trip + truncation tests mirroring the ECS `World::write_snapshot`
pattern.

## 2. Quantity-scalar `FactValue` variant

**Deferred:** a `FactValue::Scalar` (quantity-like real number).

**What exists instead:** `FactValue` is `Signed(i64)`, `Unsigned(u64)`,
`Symbol(u64)`, `Bool(bool)`, `Entity(EntityHandle)` — enough to represent every
substrate need so far, and fully `Eq`/`Hash`.

**Why:** the task makes the scalar conditional on the kernel scalar policy
supporting it *cleanly*. The kernel scalar (`Ratio`, backed by `f32`) is **not
`Eq`/`Hash`** and carries float-equality determinism caveats. Adding it to
`FactValue` would force the entire fact model to drop `Eq`/`Hash` and total-value
equality — the property the determinism tests and `BTree`-friendly comparisons
rely on. Integer and symbol values cover current needs; a fixed-point or
`Ratio`-based scalar can be introduced deliberately once a deterministic,
`Eq`-able scalar representation is chosen.

**Suggested home:** introduce a deterministic fixed-point scalar (or an `Eq`-able
wrapper over the kernel scalar) and add the `FactValue` variant alongside the
snapshot codec above, so value serialization and the new variant land together.

## 3. Phase 3 additions to the deferred snapshot

Phase 3 (materials/substances/residues/interactions/transfers/material-effect
rules) is built on the same stores and reuses the deferred-snapshot decision: the
full byte snapshot, when implemented, must additionally cover the Phase-3 state —
`Quantity`/`QuantityUnit`, the `MaterialCatalog` (classifier + typed properties),
`ResidueStore` (incl. `ResidueLocation`/`ResidueState`), the `InteractionStore`,
the `TransferRuleStore` (incl. `TransferMode`), and the `MaterialEffectRuleStore`
(incl. `MaterialEffectRuleParams`). These add more tagged unions
(`TransferMode`, `ResidueLocation`, `MaterialEffectKind`) needing the same
branchless codec, and `i64`-backed `Quantity` amounts (the kernel `Reflect`
covers only `u32/u64/f32/bool/EntityId`), so the snapshot remains one deliberate
serialization pass rather than a piecemeal add-on.

Phase-3 **determinism** is proven the same structural way as Phase 2:
`tests/integration.rs::material_chain_is_deterministic` runs the full
substance/residue/transfer/effect chain twice on fresh worlds and asserts
identical fact ids, residue amounts, and causal-event ids. Identity newtypes
(`ResidueId`, `InteractionId`, `TransferRuleId`, `MaterialEffectRuleId`) still
binary round-trip via the kernel format and are tested.

## 4. Phase 4 / Phase 5 additions to the deferred snapshot

The full byte snapshot, when implemented, must also cover Phase-4 body/anatomy
state (`TissueRegistry`, `BodyPlanRegistry`, `BodyStore` incl. parts/surfaces,
`WoundStore`) and Phase-5 scheduler state (`ProcessScheduler` processes +
lifecycles, `ProcessWakeQueue`, `DependencySet`, the execution log, and the
`DirtySet` where it persists across boundaries). These add more tagged unions and
`i64`-backed amounts needing the same branchless codec, so the snapshot remains
one deliberate serialization pass. Determinism for both phases is proven
structurally (`body_chain_is_deterministic`, `scheduler_chain_is_deterministic`).

## 5. Phase 5: arbitrary `ProcessHandler` production shape (deferred)

The scheduler executes processes through the `ProcessHandler` seam, but the
**only** handler the facade can register is the deterministic, `Clone`
`HandlerSpec` (a tagged set of canned behaviors: complete / update-fact /
add-fact / reschedule / fail / cancel). An arbitrary consumer-supplied handler —
a boxed `dyn ProcessHandler` with a *rich* `ProcessContext` that can read facts,
relations, residues, bodies, and wounds — is deferred.

**Why.** A boxed handler that reads sim state needs a `ProcessContext` borrowing
`&SimWorld` while the scheduler (a field of `SimWorld`) is mid-step — a
self-borrow the current two-phase design avoids by cloning `HandlerSpec` (a `Copy`
value) and giving handlers a minimal context (subject + tick). Supporting an
arbitrary boxed handler with world-read access is a real ownership redesign
(e.g. splitting the scheduler out of `SimWorld`, or passing a read-only world view
distinct from the scheduler), larger than "the smallest structurally correct
version of Phase 5." The `HandlerSpec` seam is sufficient to prove deterministic
scheduler execution end-to-end (`generic_scheduler_chain_runs_end_to_end`).

**Suggested home.** A later phase that re-cuts the scheduler/world ownership so a
`ProcessContext` can carry a read-only world view, then adds
`register_process_with_handler(Box<dyn ProcessHandler>)` alongside the existing
spec-based registrations.

## Not deferred (explicitly delivered)

ECS integration *is* delivered: facts/relations/processes/causal events and
residues/interactions reference `axiom_ecs::EntityHandle`, and stale/dead handles
are rejected via `EntityRegistry::is_current` at the mutation boundary (`None`
from the facade constructors; `Skipped` from effect application). No ECS seam was
missing. Phase-3 quantity movement is fully implemented (transfer rules conserve
quantity unless explicitly lossy, fail cleanly on insufficiency / unit mismatch /
route mismatch, and emit causal events).
