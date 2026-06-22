# axiom-sim-core — Testing

Tests are split into **unit tests** (inside each `src/*.rs`, with full access to
internal types) and **integration tests** (`tests/integration.rs`, driving only
the public `SimCoreApi`). Architecture-boundary tests live in
`tests/architecture.rs`. Run them with `cargo test -p axiom-sim-core`.

## Deterministic ID tests (`src/ids.rs`)

For every id (`FactId`, `RelationId`, `DefinitionId`, `RuleId`, `ProcessId`,
`CausalEventId`): raw round-trip, total ordering, equality, hashing, kernel binary
round-trip, and clean truncation failure at every byte prefix.

## Fact store tests (`src/fact.rs`)

Insert/get/update/remove; clean `None`/`false` for missing facts; queries by kind
and by subject return ascending-id results; value variants are distinct;
cause/value/tick are carried.

## Relation store tests (`src/relation.rs`)

Branchless, mutually-exclusive endpoint accessors (`as_entity`/`as_symbol`);
insert/get/remove with ordered endpoints and strength; queries by kind and by
endpoint are ascending; clean `None` for missing relations.

## Definition registry tests (`src/definition.rs`)

`TagSet`/`PropertySet` determinism and lexicographic order; duplicate durable
names rejected cleanly with the registry left unchanged; ids are
order-independent and name-derived; tags and properties are queryable.

## Process queue tests (`src/process.rs`)

Schedule/get/cancel/reschedule; `wake_due` returns due processes in `(tick, id)`
order, excludes future processes, and does not re-wake; ascending iteration; clean
`false` for missing processes.

## Effect batch tests (`src/effect.rs`, `src/sim_world.rs`)

Builders stage effects in FIFO order with the correct `EffectKind`; `kind_at`
reports staged kinds; the report counts and indexes outcomes. In `sim_world.rs`,
application proves `Applied` for valid mutations, `Skipped` for effects whose
entity subject/endpoint is dead, and `Failed` for invalid fact/relation/process
ids — never a panic; the empty batch applies nothing.

## Causal journal tests (`src/causal.rs`)

Append/get with full field round-trip; queries by subject and by parent cause are
ascending; parent/child chains link via `CauseRef::Event`.

## ECS integration tests (`tests/integration.rs`)

A tiny real `axiom_ecs::EntityRegistry` provides entity handles. The generic chain
— *entity A has fact X → process P wakes → P emits effect Y → Y updates fact X → a
causal event records P caused Y* — is exercised end-to-end through `SimCoreApi`.
Stale (despawned) handles are rejected for facts, relations, and processes. ECS
handle references are deterministic across runs.

## Determinism / replay expectations

`same_sequence_produces_identical_state` runs the full chain twice on fresh worlds
and asserts identical fact ids, causal-event ids, and fact values — i.e. the same
initial state plus the same process/effect sequence produces an identical final
state. Effects are also shown to apply **only at the boundary**, not while
staging.

## Architecture tests (`tests/architecture.rs`)

`lib.rs` exports only `SimCoreApi`; `module.toml` declares `allowed_modules = []`;
source imports only `axiom-kernel`/`axiom-ecs`; no references to other modules,
apps, or tools; no browser/GPU/DOM, wall-clock, randomness, console/placeholder
macros, global mutable state, file IO, junk-drawer modules, or render/scene/
physics/animation/audio/input/gameplay concepts.

## Phase 3 — quantity tests (`src/quantity.rs`)

Construction rejects negative amounts and out-of-range unit codes; add/subtract
are exact and unit-checked; incompatible units and overflow/insufficiency fail
cleanly (`None`); comparison is deterministic within a unit.

## Phase 3 — material/substance definition tests (`src/material.rs`, `src/definition.rs`)

Materials and substances are classified separately (a substance is not a
material); typed classifier codes and typed numeric properties round-trip;
re-cataloging is rejected. `DefinitionRegistry::by_tag`/`by_property` queries are
ascending; duplicate durable names are rejected (Phase 2). Through the facade:
`register_material`/`register_substance` with tags + typed properties,
`material_kind_code`/`material_property`, `definitions_by_tag`.

## Phase 3 — residue tests (`src/residue.rs`)

Branchless, mutually-exclusive location accessors; create/get/set-quantity/remove
with clean misses; queries by location and by definition are ascending.

## Phase 3 — interaction route tests (`src/interaction.rs`)

Route codes validate and round-trip; an out-of-range code fails cleanly; records
round-trip all fields; queries by subject and by route are ascending.

## Phase 3 — transfer rule tests (`src/transfer.rs`, `src/sim_world.rs`)

Modes compute amounts deterministically; invalid percentages are rejected at
registration. In `sim_world.rs`, `apply_transfer` proves: Applied (conserves
quantity — source down, target created), accumulation into an existing target,
lossy (no deposit), and clean `RouteMismatch` / `InsufficientQuantity` /
`InvalidSource` / `IncompatibleUnits` failures. Each successful transfer emits a
causal event.

## Phase 3 — material effect rule tests (`src/material_effect.rs`)

Rules match only on the right tag + route; matching produces Phase-2 effects into
a batch; route/tag mismatch produces nothing. The facade applies produced effects
only at the boundary.

## Phase 3 — causal chain tests (`tests/integration.rs`)

`generic_material_chain_runs_end_to_end` drives the abstract chain (substance-x →
residue → touch interaction → transfer → causal event → material effect rule →
fact update → traceable cause chain). `material_chain_is_deterministic` proves the
same initial state + same operations yields identical final state.

## Body / anatomy tests

- **Body ids** (`src/ids.rs`) — ordering/equality/hash/binary round-trip for the
  six body identity types.
- **Tissue definitions** (`src/tissue.rs`) — kind codes; register/dup-name
  rejection; tag/property queries; name-derived ids.
- **Body plans** (`src/body_plan.rs`) — draft build, duplicate part-name and
  invalid-connection rejection, ordered part queries, query by kind/capability.
- **Body instantiation** (`src/body.rs`) — deterministic minting, part/surface
  queries by body/kind, connected parts, part/surface state, owner queries.
- **Body surfaces / routes** (`src/body_surface.rs`, `src/body_route.rs`) — kind
  codes; route→surface validation table; interaction-route→body-route mapping.
- **Wounds** (`src/wound.rs`) — create/get/update; queries by body/part/mode/
  severity.
- **Anatomy facade** (`src/facade/anatomy/tests.rs`) — every facade method,
  including residue-on-surface placement, route-validated surface interactions,
  wound causal events, and connection relations.
- **Integration** (`tests/integration.rs`) — `generic_body_interaction_chain_*`:
  substance → residue → surface interaction → transfer onto a body surface →
  wound, with `body_chain_is_deterministic`.

## Process-scheduler tests

- **Tick** (`src/sim_tick.rs`) — construct/order; checked add succeeds/overflows.
- **Process lifecycle** (`src/process_lifecycle.rs`) — legal transitions; illegal
  transitions rejected cleanly; terminal dead-ends; execution records.
- **Wake queue** (`src/process_wake_queue.rs`) — due popped in `(tick, id)` order,
  future excluded; repeated schedule keeps one pending; cancel; non-consuming peek.
- **Dirty set** (`src/dirty_set.rs`) — mark/query/clear facts/relations/subjects;
  last-write-wins; reasons.
- **Dependencies** (`src/process_dependency.rs`) — subscribe dedups and indexes
  both ways; subscriptions/dependencies queries.
- **Handler seam** (`src/process_handler.rs`) — each `HandlerSpec` produces the
  right effects/disposition; a bespoke `ProcessHandler` proves the trait is generic.
- **Scheduler** (`src/scheduler.rs`) — register/schedule/take-due advances to
  Running; finalize completes/reschedules; cancel skips dead due entries.
- **Scheduler facade** (`src/facade/scheduling/tests.rs`) — every facade method:
  register/schedule/step/boundary, dirty-dependency wake, manual dirty marking +
  inspection, failing/canceling/rescheduling processes, invalid codes, and
  `scheduler_chain_is_deterministic`.
- **Integration** (`tests/integration.rs`) — `generic_scheduler_chain_*`: fact-x
  update → dirty → invalidation wakes process-p → step → handler updates fact-y at
  the boundary → causal records; effects apply only at the boundary; deterministic.

## Effect boundary tests

`src/sim_world.rs` proves effect application marks dirty entries, and the
scheduler boundary applies effects (failed effects force `Failed`), records an
execution, and journals. The facade tests assert effects are *not* applied during
`step_scheduler`, only at `apply_scheduler_boundary`.

## Causal chain tests

Scheduler causal events are parented to the process (`scheduler_events_for_process`)
so they are queryable per process; the integration chains assert the full
lifecycle event sequence and that re-running yields an identical event count.

## Snapshot / replay (deferred)

Full byte-snapshot of the whole sim world (now including Phase-4 body + Phase-5
scheduler/dirty state) is deferred (see `PHASE_2_DEFERRED.md`). Identity-type
binary round-trips are tested; whole-world determinism — including the full
scheduler chain — is proven structurally (deterministic re-run equality), and the
arbitrary `ProcessHandler` production shape is deferred while deterministic
`HandlerSpec` handlers are fully tested.
