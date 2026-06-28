# `axiom-physics` — Roadmap

The current state is a **real Phase 2 rigid-body engine**: a deterministic world,
bodies/colliders, mass properties, forces/impulses, a FIFO command queue,
lifecycle events, deterministic snapshots and step records, a split linear
integrator, an `O(n²)` AABB broad phase, a narrow phase for the four classical
primitive pairings, a sequential-impulse contact solver, exact spatial queries,
deterministic substepping, and atomic non-finite rollback. Bodies fall, collide,
rest, and bounce, and a full drop replays identically across runs of the same
build (same-binary replay; cross-platform determinism is a later phase).

The phases below are built **in order**, each landing as a self-contained change
*with the tests that fully cover it* and a replay-equality proof. No phase relaxes
a law: every addition stays branchless in production code, keeps the single
facade, and holds the module at 100% coverage. This is a primitive-shape
rigid-body core — it does **not** target, and will not claim, Unreal/Unity/Godot
feature parity, and has no character controllers, vehicles, cloth, fluids, GPU
acceleration, or networking.

## Phase 1 — rigid-body scaffold + linear integration (done)

The deterministic world, bodies/colliders, mass properties, force accumulation,
commands, lifecycle events, snapshots, step records, and the linear
(semi-implicit Euler) integrator. Bodies fall under gravity per kind; the
collision pipeline and queries were deterministic empty scaffolds.

## Phase 2 — collision pipeline + solver + real queries (done)

The empty scaffolds are now real, deterministic implementations:

- **Flat tagged collider shape** — `PhysicsColliderShape` became a fieldless tag
  (`PhysicsShapeKind`) plus uniform geometry fields, so all geometry code reads
  parameters by field access and dispatches on `kind().index()` into function
  tables, never a `match` (Branchless Law).
- **Broad phase** — a deterministic `O(n²)` AABB-overlap candidate pairing over
  world AABBs (`broad_phase_pair.rs` + `collider_bounds.rs`); finite/finite pairs
  are culled by AABB overlap, every finite collider always pairs with every
  (infinite) plane, two planes never pair; output sorted by collider handle.
- **Narrow phase** — real contact generation for sphere/sphere, sphere/plane,
  sphere/box, and box/plane (and their reversed orderings), via a branchless
  16-entry dispatch table; A→B normal convention, planes as solid half-spaces,
  strictly-positive penetration, degenerate-config rejection
  (`contact_pair.rs` + `contact_manifold.rs`).
- **Sequential-impulse solver** — `solver_iterations` normal-impulse passes with
  combined (max) restitution, split by inverse mass, followed by Baumgarte
  position correction (slop `0.01`, beta `0.2`) (`contact_solver.rs`).
- **Split integrator** — `integrate_velocities` and `integrate_positions` so the
  solver runs between them; linear only (`integrator.rs`).
- **Exact queries** — `raycast` (exact ray/sphere, ray/AABB, ray/plane; capsule
  excluded; nearest hit, ties by smaller body handle, triggers excluded) and
  `overlap_sphere` (exact sphere/sphere, sphere/AABB, sphere/plane; capsule
  excluded; sorted, de-duplicated body handles, triggers included)
  (`physics_query.rs`).
- **Honest step-record counts** — `broad_phase_pair_count`, `contact_pair_count`,
  `solved_contact_count` (real solved-contact work), and `substep_count` report
  actual pipeline work; `solver_iteration_count` is documented as configured
  metadata.

Proven at the facade: a dropped sphere settles on a static plane without
tunnelling and replays identically (snapshot + record); an elastic ball rebounds;
two dynamic bodies exchange momentum. Determinism is **same-binary replay** —
cross-platform / byte-serialized determinism is not claimed (a later phase).

### Phase 2 hardening (remediation pass)

Following the legitimacy audit (see [`LEGITIMACY_AUDIT.md`](LEGITIMACY_AUDIT.md) and
[`REMEDIATION.md`](REMEDIATION.md)), Phase 2 was hardened without relaxing a law:
queries made **exact**; `max_substeps` made **live** (deterministic substepping
that mitigates large-dt tunnelling); steps made **atomic** (a non-finite result
rolls back deterministically, so no snapshot can carry `NaN`/`±∞`); the event log
made **drainable**; collider geometry, contact reports (`latest_contacts()`), and
inspectable typed errors exposed as **neutral integration surfaces**;
disabled-body force/impulse now **rejected** rather than silently dropped.

Phase 2 still deliberately **excludes** friction, capsule contacts, box/box
contacts, collision/trigger lifecycle **events**, angular dynamics, true CCD, a
broad-phase acceleration structure, and cross-platform determinism — these are the
phases below.

## Phase 3 — friction solver

Add tangential impulses to the contact solver, using each contact's combined
material friction (currently validated and stored but unresolved), so resting and
sliding contacts behave correctly. The friction solve runs alongside the existing
normal-impulse passes and stays branchless and deterministic.

## Phase 4 — capsule and box/box contacts

Fill the remaining entries of the narrow-phase dispatch table: capsule pairings
(capsule/sphere, capsule/plane, capsule/box, capsule/capsule) and box/box. This
completes contact generation across the existing primitive vocabulary. (The
contact point is already exposed via `PhysicsApi::latest_contacts()`; these new
pairings simply extend which contacts populate it.)

## Phase 5 — collision and trigger lifecycle events

Add contact persistence across steps and emit contact lifecycle (enter/stay/exit)
and trigger overlap events to the event log, honoring each collider's
`is_trigger` flag (triggers report overlap without a solver response). This is the
first phase to emit non-lifecycle events; the "no collision events" invariant ends
here, by design.

## Phase 6 — angular / rotational dynamics

Integrate orientation: apply torque and angular velocity, derive inertia from
shape and mass, and extend the contact solver with angular impulse terms so
contacts impart spin. Collider placement begins honoring body rotation (oriented
boxes), retiring the Phase-2 translation-only simplification.

## Phase 7 — broad-phase acceleration structure

Replace the `O(n²)` scan with a deterministic acceleration structure (e.g.
sweep-and-prune or a uniform grid over body AABBs), preserving the exact same
sorted candidate-pair output so the upper phases are unchanged and replay stays
byte-identical, while large worlds stop paying the quadratic cost.

## Phase 8 — kinematic character controller

Build a deterministic kinematic character controller on top of the solver and
queries (sweep/slide against the resolved geometry), the first higher-level
movement primitive over the rigid-body core.

## Phase 9 — static triangle mesh and convex hull colliders

Extend the collider vocabulary with static triangle meshes and convex hulls, plus
the corresponding broad/narrow-phase and query support, so worlds can collide
against authored level geometry rather than only primitive shapes.

## Phase 10 — debug-draw contracts and deterministic recording tools

Expose neutral debug-draw data contracts (returned by-value, consumed by an app's
renderer) and deterministic physics recording / replay-to-bytes tooling, so a
world's evolution can be serialized, replayed, and visually inspected — the
physics analogue of the engine's existing recording tooling, and the first
byte-level serialization of physics state (replay today is proven only by snapshot
equality).
