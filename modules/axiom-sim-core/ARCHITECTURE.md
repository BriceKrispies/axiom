# axiom-sim-core — Architecture

## What sim-core is

`axiom-sim-core` is the **generic simulation substrate** (Phase 2): a small,
deterministic *language* for expressing simulated-world interactions. It owns six
primitives and nothing else:

- **Facts** — typed assertions about a subject entity (`FactStore`).
- **Relations** — typed, ordered links between subjects (`RelationStore`).
- **Definitions** — a registry of data-defined concepts with tags + properties
  (`DefinitionRegistry`).
- **Processes** — tick-scheduled activities woken from a deterministic wake queue
  (`ProcessQueue`).
- **Effects** — proposed mutations applied only at an explicit boundary
  (`EffectBatch` → `EffectReport`).
- **Causal journal** — structured "why did this happen" tracking (`CausalJournal`).

These are exposed through exactly one public facade, **`SimCoreApi`**.

## What sim-core is not

It is **not** a layer, and it owns **no domain meaning**. It knows nothing about
cats, beer, paws, bodies, body parts, wounds, fluids, materials behavior,
toxicology, grooming, jobs, needs, social memory, history, AI, or any gameplay
rule. It is also not a renderer, scene graph, physics engine, animation system,
input mapper, audio engine, editor, or asset loader, and references no browser or
GPU APIs. Kinds (`FactKind`, `RelationKind`, `ProcessKind`, `CausalEventKind`) and
definition categories are **opaque codes**; later phases assign them meaning.

## Why it is a *module* in this phase

The Axiom layer chain already has ECS (Layer 05) as the world-state layer with
further layers above it. sim-core builds *on* ECS but is a **capability**, not a
new rung in that ordered spine — forcing it into the layer chain would manufacture
a meaningless ordering dependency. It is therefore an **isolated engine module**
(`kind = "engine-module"`, `allowed_modules = []`) depending only on the `kernel`
and `ecs` layers. If a future need makes sim-core a shared substrate that other
*layers* must sit above, promoting it to a layer is a deliberate, separate
decision.

## Why ECS remains the storage/mutation substrate

ECS owns **entities and component storage**; sim-core owns **meaning about**
entities (facts/relations/processes/causes). sim-core does not duplicate or wrap
ECS storage and does not own the ECS world. It references ECS entities by
`axiom_ecs::EntityHandle`, and validates liveness against a borrowed
`axiom_ecs::EntityRegistry` at the mutation boundary — a stale/dead handle is
rejected (`add_fact`/`add_relation`/`schedule_process` return `None`; the
equivalent effects report `Skipped`). This keeps one source of entity truth (ECS)
and lets sim-core stay a thin, generic overlay.

## Why sim-core owns facts/relations/processes/effects/causes

These five are the irreducible vocabulary of a Dwarf-Fortress-like simulation:
*what is true* (facts), *how things relate* (relations), *what is happening over
time* (processes), *what changes* (effects), and *why* (causal journal).
Definitions add *data-defined concepts* the other five reference. Keeping this
vocabulary generic and in one module lets every later domain (materials, bodies,
fluids, jobs, …) be expressed as data + codes on top of it without each inventing
its own bespoke state model.

## How later modules or apps use sim-core

A later feature module or app holds a `SimCoreApi`, registers definitions, asserts
facts/relations about ECS entities, schedules processes, and — when processes wake
— stages effects in an `EffectBatch` and applies them at a frame/tick boundary,
reading the `CausalJournal` to explain outcomes. Because every internal type is
reached only through the facade (never re-exported), consumers compose sim-core by
calling methods and passing back the opaque values it returns — the same isolation
discipline every Axiom module follows.

## Deterministic rules

- Every store is `BTreeMap`/`BTreeSet`-backed; all iteration is ascending-key,
  identical on every platform. No `HashMap` is used for semantic output.
- Ids are minted by deterministic monotonic counters (facts/relations/processes/
  events) or order-independent name hashes (definitions) — never random, never
  wall-clock-derived, never string identity.
- Time is **logical ticks** supplied by the caller; sim-core never reads a clock.
- Processes wake by a `(WakeTick, ProcessId)` range query — deterministic order,
  no scanning of all processes, future processes never woken.
- Effects apply in FIFO order at an explicit boundary; nothing mutates a store
  except through `apply_effects`.
- The same initial state + the same process/effect sequence yields byte-identical
  iteration and values (proven in tests).

## Dependency rules

- Depends only on `axiom-kernel` and `axiom-ecs`. Never on another module, an app,
  or a tool.
- Never on scene, render, resources, webgpu, host/browser APIs, physics,
  animation, input, audio, editor, or gameplay code.
- `lib.rs` exposes exactly one public item: `SimCoreApi`.
- The whole spine is branchless (the `engine_no_branching` discipline): logic is
  expressed as iterator/combinator/table transforms, not hand-written control
  flow.

## Phase 3 — material / substance semantics

Phase 3 extends the substrate (still inside this one module — the Module Law
forbids module→module deps, so `axiom-materials` would be illegal) with the
generic *material/substance language*:

- **Quantity** (`quantity.rs`) — `Quantity`/`QuantityUnit`, integer-backed
  (`i64`, no floats), unit-checked add/subtract/compare. Exact and deterministic.
- **Material/substance catalog** (`material.rs`) — `MaterialKind`/`SubstanceKind`
  classifiers and `MaterialProperty`/`SubstanceProperty` typed property keys,
  layered over the Phase-2 `DefinitionRegistry` (which still owns name, durable
  id, duplicate rejection, string tags, generic properties). Durable identity is
  the `DefinitionId`, never a tag string.
- **Residue** (`residue.rs`) — `Residue`/`ResidueId`/`ResidueLocation`/
  `ResidueState`: a quantity of a definition at a generic location (entity or
  opaque symbol — later phases encode parts/cells/items/surfaces as codes).
- **Interaction route** (`interaction.rs`) — `InteractionRoute` (touch,
  ingestion, inhalation, wound-contact, embedded, contained, adjacent, generic),
  `InteractionRecord`, `InteractionKind`. Records *that* a route applied; no
  contact detection, eating, breathing, or collision.
- **Transfer rules** (`transfer.rs`) — `TransferRule`/`TransferMode` (fixed,
  percentage-bp, all-up-to-max, none)/`TransferResult`. A rule consumes an
  existing `InteractionRecord` and moves quantity between residues.
- **Material effect rules** (`material_effect.rs`) — `MaterialEffectRule`/
  `MaterialEffectKind`/`MaterialEffectResult`: match by material tag + route and
  *produce* Phase-2 `Effect` values (applied at the normal effect boundary).

### Why materials/substances are still generic sim-core primitives

A material is a `Definition` plus a typed classifier; a substance is the same; a
residue is a quantity of a definition somewhere; a transfer is arithmetic on
quantities along a route; an effect rule produces generic Phase-2 effects. None of
these name a real material, body, fluid, or behavior — the meanings (iron, water,
blood, alcohol) are data the *consumer* registers. Tests use toy names like
`substance-x` / `test-liquid`; production source hardcodes none.

### Why this is not a fluid sim / toxicology / body / crafting / gameplay system

There is no spatial contact detection (a transfer consumes an *already-created*
interaction record), no diffusion or pressure, no anatomy (a `ResidueLocation` is
an entity or an opaque code, never a body part), no reaction chemistry, no
crafting recipes, and no gameplay rules. The `BodyPlan` definition category and
the `WoundContact`/`Ingestion` routes are *names in a generic enum*, not
implementations. Toxicology/bodies/fluids are later phases built on top.

### How residues, interactions, transfer rules, and effect rules compose

1. Register material/substance **definitions** (tags + typed properties).
2. Create **residues** (quantities of those definitions at locations).
3. Record an **interaction** (a route between subjects, optionally naming a
   material/residue).
4. Apply a **transfer rule** to that interaction: it reduces the source residue,
   creates/accumulates the target residue (conserving quantity unless lossy), and
   emits a causal event.
5. Apply **material effect rules** matching the interaction's material tag +
   route: they produce Phase-2 effects (update a fact, emit an event, …) applied
   at the effect boundary, chained to the transfer's cause.

### How later phases build on this

Bodies map body parts to `ResidueLocation` symbol codes; fluids schedule
processes that emit transfer effects between cell residues; toxicology registers
material effect rules keyed by `toxic`/`intoxicant` tags that update need/health
facts. All of that is data + rules over this substrate — no new lower primitive.

### Determinism, causal, snapshot, dependency rules (Phase 3)

Determinism and dependency rules are unchanged from Phase 2: `BTreeMap`-backed
stores, integer quantities, logical ticks, branchless spine, only `axiom-kernel`
+ `axiom-ecs`, single `SimCoreApi` facade. **Causal:** every transfer and every
material-effect-rule application can emit a causal event with a parent cause, so
transfers/effects trace back through the journal. **Snapshot:** still deferred
(now including Phase-3 state) — see [`PHASE_2_DEFERRED.md`](PHASE_2_DEFERRED.md);
identity types binary round-trip and whole-world determinism is proven
structurally (`material_chain_is_deterministic`).

## Body / anatomy substrate

The substrate also carries a generic *body/anatomy* language (still inside this
one module — a separate module would be an illegal module→module dependency):

- **Identity** (`ids.rs`) — `BodyId`/`BodyPlanId`/`BodyPartId`/`TissueId`/
  `BodySurfaceId`/`WoundId`.
- **Tissue definitions** (`tissue.rs`) — `TissueDefinition`/`TissueKind`/
  `TissueProperty`/`TissueLayer` + a name-keyed `TissueRegistry`.
- **Body plans** (`body_plan.rs`) — `BodyPlan`/`BodyPlanPart`/`BodyPlanPartKind`/
  `BodyPlanConnection`/`BodyPlanSymmetry`/`BodyPlanCapability`, built as a draft
  (add parts, connect, finish) into a name-keyed `BodyPlanRegistry`. Reusable
  anatomical structures, **not** creatures.
- **Bodies** (`body.rs`) — `Body`/`BodyPart`/`BodyPartState`/`BodyConnection`/
  `BodyStore`: instances minted from a plan, with parts, tissue layers, and
  surfaces, optionally owned by an ECS entity (stale owners rejected).
- **Surfaces** (`body_surface.rs`) — `BodySurface`/`BodySurfaceKind`/
  `BodySurfaceState`/`SurfaceExposure`: targetable anatomical surfaces.
- **Body routes** (`body_route.rs`) — `BodyRoute`/`BodyRouteKind`/
  `BodyRouteTarget`: refine Phase-3 `InteractionRoute`s and validate which surface
  kinds a route may reach (a static table).
- **Wounds** (`wound.rs`) — `WoundRecord`/`DamageMode`/`DamageSeverity`/
  `TissueDamage`/`WoundState` + `WoundStore`: causal damage *records*, not combat.

**Why generic.** A body is a plan instance; a surface is a targetable code; a
wound is a record. None name a species, organ, or behavior — meanings are data
the consumer registers (tests use `test-core`/`substance-x`). **Not** a creature,
combat, toxicology, or healing system: no spatial contact detection, no anatomy
behavior, no reaction chemistry.

**Surfaces integrate with residues** through the existing Phase-3 seam: a
`BodySurfaceId` maps to a `ResidueLocation::symbol(id)`, so residues sit on
surfaces and transfer rules deposit onto them with no new primitive.
**Body routes refine interaction routes:** `BodyRoute::from_interaction` maps a
generic route, and `can_target` validates it against a surface kind before an
interaction is recorded (emitting a causal event). **Wound creation** validates
its body/part/tissue references and emits a causal event.

## Process scheduler

The scheduler makes long-lived simulation processes run **without scanning the
whole world every tick** — due work is found by a deterministic wake queue keyed
by `(SimTick, ProcessId)`, and processes only re-run when their subscribed
dependencies go dirty.

- **Tick model** (`sim_tick.rs`) — `SimTick`/`TickDelta` (integer logical ticks,
  checked math). The Phase-2 `WakeTick` remains for the Phase-2 process store.
- **Lifecycle** (`process_lifecycle.rs`) — `ProcessStatus` (scheduled, sleeping,
  ready, running, completed, canceled, failed), `ProcessLifecycle` (legal
  transitions via a static table), `ProcessTransition`, `ProcessExecutionRecord`.
- **Wake queue** (`process_wake_queue.rs`) — `ProcessWakeQueue`/`WakeEntry`/
  `WakeReason`: one pending wake per process, range-popped in `(tick, id)` order,
  never waking the future.
- **Dirty set** (`dirty_set.rs`) — `DirtySet`/`DirtyFact`/`DirtyRelation`/
  `DirtySubject`/`DirtyKind`/`InvalidationReason`. **Effect application marks
  dirty** (see `apply_effects`), cleared at an explicit boundary.
- **Dependencies** (`process_dependency.rs`) — `ProcessDependency`/
  `DependencyKind`/`DependencySet`/`ProcessSubscription`: processes subscribe to
  kinds of change (fact-kind, subject, …) keyed by a selector; dedup is
  deterministic.
- **Handler seam** (`process_handler.rs`) — `ProcessHandler`/`ProcessContext`/
  `ProcessOutput`/`ProcessDisposition`. A handler is a pure transform from a
  read-only context to an `EffectBatch` + disposition; it never mutates stores.
  `HandlerSpec` is the deterministic `Clone` implementation the facade drives.
- **Scheduler** (`scheduler.rs`) — `ProcessScheduler`/`SchedulerStep`/
  `SchedulerStepResult`/`SchedulerBoundary`/`ProcessExecutionOrder`. Pure
  bookkeeping; the world-coordination (running handlers, applying effects at the
  boundary, journaling) lives on `SimWorld`.

**Explicit effect boundary.** A step runs due handlers and *stashes* their
outputs; `apply_scheduler_boundary` is the only place effects apply (marking
dirty), lifecycles resolve (a failed effect forces `Failed`), and the journal
records `produced`/`applied`/transition. **Why this is not a job/AI/behavior-tree
system:** there are no jobs, reservations, goals, utility scoring, or trees — only
a generic "wake due/subscribed processes, run a handler, apply effects at a
barrier" loop. **Causal:** every scheduler-relevant change emits an event
(`scheduled`, `woke`, `started`, `completed`, `slept`, `canceled`, `failed`,
`dirty invalidation`, `woken by dirty`, `produced effects`, `effects applied`),
parented to the process so events are queryable per process.

**How later phases build on this:** a sim crucible app drives `step_scheduler` +
`apply_scheduler_boundary` each fixed tick over registered domain processes; the
domain supplies `ProcessHandler`s (the production boxed-handler shape is deferred,
see `PHASE_5_DEFERRED.md`). Everything stays deterministic: `BTreeMap`-backed
stores, integer ticks, branchless spine, single `SimCoreApi` facade.

## Deferred

Full byte-snapshot/serialization of the whole sim world (now including Phase-3
material and Phase-4/5 body + scheduler state) and a quantity-scalar `FactValue`
variant are deferred — see [`PHASE_2_DEFERRED.md`](PHASE_2_DEFERRED.md). The
arbitrary boxed `ProcessHandler` production shape is also deferred (the facade
drives deterministic `HandlerSpec` handlers). Identity types still serialize via
the kernel binary format, and determinism is proven structurally
(`material_chain_is_deterministic`, `scheduler_chain_is_deterministic`).
