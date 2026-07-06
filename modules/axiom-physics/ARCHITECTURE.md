# `axiom-physics` — Architecture

`axiom-physics` is a **deterministic 3D rigid-body physics engine module**. It is
modeled architecturally after the rigid-body cores of Unreal Chaos, Unity
Physics, and Godot Physics, but built to Axiom's strict laws: a single public
facade, branchless production code, 100% test coverage, and no hidden state.

This document explains *what* the module is, *where* it sits, *why* its public
surface looks the way it does, and exactly what Phase 2 does and does not do.

## It is a module, not a layer

Physics is an **engine module** (`modules/axiom-physics`, `module.toml` with
`kind = "engine-module"`), not a layer in the ordered spine. That classification
is deliberate and load-bearing:

- A **module is an isolated capability.** It depends on a curated set of *layers*
  and on **no other module** (`allowed_modules = []`). Nothing depends on
  physics except an app or a future feature module.
- Physics owns one thing — the rigid-body **world** — and exposes it through one
  facade. It is not a substrate that other engine code builds the spine on top
  of, so it is not a layer.

It depends only on three completed layers, and genuinely uses each:

| Layer          | What physics consumes from it                                             |
|----------------|---------------------------------------------------------------------------|
| `axiom-kernel` | identity/handle discipline, the dimensioned scalars `Ratio`/`Meters`, the deterministic time primitives, and `MathError` plumbing via math |
| `axiom-runtime`| the explicit deterministic fixed step (`RuntimeStep`) that drives `step()` |
| `axiom-math`   | `Vec3`, `Transform`, `Aabb`, and `Ray` (poses, velocities, forces, bounds, and ray geometry) |

It depends on **nothing else** — no scene, renderer, mesh, asset, input,
animation, audio, ECS world, plugin host, editor, browser API, GPU API,
wall-clock, randomness, or external physics/math/ECS crate. Those forbidden
tokens are scanned for by `tests/architecture.rs` and by the workspace
`xtask check-architecture` gate.

## The single facade

Per Module Law #8, `lib.rs` exports **exactly one** behavioral facade,
[`PhysicsApi`], plus its **identity vocabulary** — the two handle types
[`PhysicsBodyHandle`] and [`PhysicsColliderHandle`] re-exported through one
`pub use ids::{…}` line. Every other type (configs, bodies, colliders, shapes,
materials, snapshots, records, events, errors, manifolds) is **sealed**: it lives
in a private module and is reachable only *through* the facade.

### Why the facade takes primitives and returns sealed values

A caller in an app cannot *name* a sealed type. That single fact dictates the
whole public shape:

- **Construction takes primitives.** Because a caller cannot build a
  `PhysicsBodyDesc`, `PhysicsColliderShape`, `PhysicsConfig`, or `PhysicsMaterial`
  and pass it in, the facade offers primitive entry points and builds those
  internal types itself: `create_static_body` / `create_dynamic_body` /
  `create_kinematic_body`, `with_config(gravity, …)`, and the shape-specific
  `attach_*_collider` methods. This mirrors `SceneApi::spawn_*` rather than
  taking a `Node`.
- **The one exception that proves the rule: `PhysicsApi::material(…)`.** A
  material is validated up front and then handed to an `attach_*_collider` call.
  The caller never names its type — type inference carries the value from
  `material(…)` straight into the attach method. This keeps each attach method's
  argument list small (one material value instead of three loose scalars) while
  still validating ranges once, early.
- **Rich results are returned by-value and read through accessors.**
  `snapshot()` returns a `PhysicsSnapshot`, `latest_step_record()` a
  `PhysicsStepRecord`; both are sealed `pub struct`s used without being named
  (the verified `SceneSnapshot` pattern). `events()` returns `&[PhysicsEvent]`.
  `raycast` / `overlap_sphere` return body handles (nameable identity vocabulary).

### Scalar carriers — why `Ratio` and `Meters`, never naked floats

The `engine_no_unitless_float_public_api` dylint forbids a naked `f32`/`f64` in
any public signature outside the scalar-floor crates (kernel/math). Physics is a
module, so every scalar crossing the facade is carried by a nameable wrapper:

- **`Ratio`** (kernel) carries unit-relative finite scalars: mass, friction,
  restitution, density. The kernel has no `Mass`/`Friction` unit and physics may
  not modify a lower layer to add one, so `Ratio` is the sanctioned finite-scalar
  carrier. Physics then enforces the *extra* physical constraints itself
  (`mass > 0`, `friction >= 0`, `restitution ∈ [0, 1]`, `density > 0`) and
  returns a typed error on violation.
- **`Meters`** (kernel) carries lengths: collider radius, capsule half-height,
  plane distance, query distances.
- **`Vec3` / `Transform`** (math) carry directions, extents, gravity, forces,
  impulses, velocities, and body poses.

Because `Transform`'s components are raw `f32` that math does not validate on
construction, physics screens every incoming transform for finiteness before a
body is created (`transform_is_finite` in `physics_body_desc.rs`).

## Collider shapes are a flat tagged value

A [`PhysicsColliderShape`] is **not** a payload-carrying enum. It is a fieldless
tag ([`PhysicsShapeKind`] — `Sphere = 0`, `Box = 1`, `Capsule = 2`, `Plane = 3`)
plus a uniform set of geometry fields:

```text
kind: PhysicsShapeKind
half_extents: Vec3   // local axis-aligned half-size (finite kinds; ZERO for a plane)
radius: f32          // sphere/capsule rounding radius (0 otherwise)
normal: Vec3         // unit plane normal (ZERO for non-planes)
offset: f32          // plane signed offset n·x = offset (0 for non-planes)
```

This shape is **the** reason the geometry code can stay branchless. The broad
phase, narrow phase, and queries read a shape's parameters with plain field
access (`shape.radius()`, `shape.half_extents()`, `shape.normal()`) and dispatch
on `kind().index()` into fixed function tables — never a `match` on variant
payloads, which the Branchless Law forbids. The four constructors validate their
inputs (positive radii/extents, finite non-zero plane normal — normalized on
construction) and pack them into the flat representation; the packed fields are
private `f32`/`Vec3`, never a public naked float.

A plane is the only **infinite** kind (`is_finite()` is false): it has no finite
AABB and is handled analytically by the narrow phase and ray/plane queries
instead of by bounds culling.

## The world and the step pipeline

[`PhysicsWorld`] (private) is the heart of the module. It owns, and is the sole
mutator of:

- the validated `PhysicsConfig` (gravity, solver iterations, body/collider
  capacities, substeps, sleeping flag);
- **`Vec`-backed, insertion-ordered** body and collider storage — never a
  `HashMap`, so iteration order is deterministic and snapshots are byte-stable;
- a **FIFO command queue** (`Vec`), a monotonic **event log** (`Vec`), the step
  counter, the monotonic id allocators, and the latest step record.

`step(RuntimeStep)` rejects a zero-nanosecond delta with a typed `InvalidStep`
error, then converts the explicit step to `dt` seconds in exactly one documented
place (`nanos as f32 / 1_000_000_000.0`). `step_inner` runs the standard
impulse-based pipeline **in this exact order**:

1. **drain queued commands FIFO** and apply them (force/impulse accumulate;
   enable/disable flip a body and emit a lifecycle event);
2. **broad phase** — candidate collider pairs (`broad_phase_pair::detect_pairs`);
3. **narrow phase** — contact manifolds for the overlapping pairs
   (`contact_pair::generate_contacts`);
4. **integrate velocities** — gravity + force + impulse → velocity, accumulators
   cleared (`integrator::integrate_velocities`);
5. **solve** — sequential-impulse contact velocity solve (`contact_solver::solve`);
6. **integrate positions** — translation advanced by the solved velocity
   (`integrator::integrate_positions`);
7. **correct positions** — Baumgarte penetration correction
   (`contact_solver::correct_positions`);
8. advance the step index, emit `StepCompleted`, and record the step.

The solver runs **between** the velocity and position integration passes — the
reason the integrator is split into two halves rather than one monolithic update.

### Broad phase (`broad_phase_pair.rs` + `collider_bounds.rs`)

A deterministic `O(n²)` AABB-overlap candidate pairing — **no acceleration
structure yet** (a later-phase item). Each finite collider is resolved to a world
AABB (`world_aabb`: `center ± half_extents`, where `center` is the owning body's
translation); a plane returns `None` because it is infinite. A pair is a
candidate when:

- both colliders **and** both owning bodies are enabled, and
- the colliders are on **different** bodies, and
- the geometry could overlap: **both finite with overlapping AABBs**, **or
  exactly one is a plane** (a plane cannot be culled by bounds, so every finite
  collider always pairs with every plane). **Two planes never pair** (no finite
  shape to contact).

Output pairs store their two handles in ascending order and the list is **sorted
by `(a, b)` collider handle**, so the candidate set is a deterministic function of
world state, independent of insertion or scan order.

> **Per-collider disable is reserved but has no public surface yet.** The pair
> test honors a per-collider `enabled()` flag, but the facade currently exposes
> only body-level enable/disable; there is no public API to disable an individual
> collider. The field is wired through for a later phase.

### Narrow phase (`contact_pair.rs` + `contact_manifold.rs`)

Real contact generation for the four classical pairings — **sphere/sphere,
sphere/plane, sphere/box, box/plane** — in both collider orderings. Dispatch is a
branchless 16-entry function table indexed by `kind_a.index() * 4 + kind_b.index()`;
the reversed orderings (box/sphere, plane/sphere, plane/box) reuse the canonical
generator with arguments swapped and the resulting normal flipped, so each
geometry test is written once. The two box pairings are **orientation-aware**:
`sphere/box` resolves the sphere against the box's closest surface point *in the
box's local frame* (via the box rotation's conjugate), and `box/plane` projects
the half-extents onto the plane normal expressed in the box's rotated axes — so a
tilted platform collides on its true faces. At the identity rotation both reduce
exactly to the axis-aligned tests. **Every other pairing — box/box, any capsule
pair, plane/plane — deterministically produces no contact** and is a documented
later-phase item.

Conventions (deterministic, documented):

- The contact **normal points from collider A toward collider B**, where A/B are
  the pair's colliders in ascending handle order. To separate the pair, B moves
  along `+normal` and A along `-normal`, each scaled by inverse mass. Because A/B
  roles are fixed by handle order, the normal is a stable function of world
  state, never of discovery order.
- A plane is a one-sided **solid half-space**; its stored unit normal points to
  the **empty** side, so a body crossing to the solid side penetrates.
- Contact requires **strictly positive** penetration: a pair touching exactly at
  the boundary (`depth == 0`) produces no contact.
- **Degenerate coincident configurations** have no defined normal and produce no
  contact: equal sphere centres, or a sphere centre inside a box.

A `ContactManifold` carries both colliders, both bodies, the unit normal, the
penetration depth, and a world contact point. The contact point is surfaced to
apps through `latest_contacts()` (`ContactReport`), but is **not yet used inside
the solver**: the landed friction solver derives its tangent basis from the
contact normal alone, and the contact point will be consumed when
**contact-induced angular dynamics** land (a contact lever arm needs the point).

### Contact solver (`contact_solver.rs`)

A deterministic **sequential-impulse** solver in two stages:

1. **Velocity solve** (`solve`) runs `PhysicsConfig::solver_iterations` passes
   (default 8). Each pass walks the manifolds in their stable sorted order and
   applies a normal impulse that removes the approaching relative velocity,
   scaled by `1 + combined_restitution` — where the combined restitution is the
   **larger** of the two colliders' restitutions, so a bouncy body rebounds off
   any surface — **then a tangential friction impulse** (see below). Impulses are
   split by inverse mass, so a static or kinematic body (zero inverse mass) is
   never moved. A separating contact gets no impulse.
2. **Position correction** (`correct_positions`) is a Baumgarte-style push
   (slop `0.01 m`, beta `0.2`) that removes the residual penetration beyond the
   slop, again split by inverse mass, so resting stacks neither sink nor jitter.

Gating is arithmetic (`approaching.then(…)`, `(depth - slop).max(0.0)`, the
Coulomb `clamp`), never control flow.

> **Friction is resolved (tangential solver landed).** Material friction is no
> longer merely stored: after the normal impulse, `solve_contact` applies a
> **tangential friction impulse** along a deterministic orthonormal tangent basis
> derived **only from the contact normal** (`tangent_basis` crosses the normal
> with the world axis it is least aligned with, then completes the pair — so the
> basis is a pure function of world state, never of discovery order). The combined
> coefficient is the geometric mean `sqrt(μ_a·μ_b)` of the two colliders'
> frictions, and the impulse is clamped to the Coulomb cone `|j_t| ≤ μ·j_n` with
> `max`/`min` (never a branch, and never `f32::clamp`, which would panic on a NaN
> bound). The friction pass walks the same stable handle-sorted manifold order as
> the normal pass. The step record's `frictioned_contact_count` reports the
> contacts that received a genuine tangential impulse (approaching, positive
> combined friction, nonzero tangential speed). **The friction impulse is linear
> only** — it changes linear velocities; the contact solver applies no *angular*
> impulse, so a frictional slide does not (yet) induce spin (see below).

### The integrator (`integrator.rs`)

Semi-implicit (symplectic) Euler over the explicit fixed step, split so the
solver runs between the two halves. It now integrates **both** linear and angular
motion:

- `integrate_velocities` applies gravity, accumulated force, and impulse to each
  enabled dynamic body's linear velocity **and accumulated torque (scaled by the
  body's diagonal inverse inertia) to its angular velocity**, then decays both by
  the configured per-step **linear/angular damping**, clears the accumulators, and
  returns the count of bodies that actually integrated;
- `integrate_positions` advances each enabled dynamic body's translation by its
  (now solved) linear velocity **and its orientation by its angular velocity**.

Motion is gated **arithmetically, not by a branch**: an `active_factor` of `1.0`
for an enabled dynamic body and `0.0` otherwise multiplies every contribution, so
static, kinematic, and disabled bodies collapse to "no change" with zero control
flow. Static and kinematic bodies additionally carry **zero inverse mass**, so
even the force/impulse terms vanish.

**Orientation integration (landed, deterministic, NaN-safe).** Orientation
advances by `q' = normalize(q + 0.5·dt·(ω_quat ⊗ q))`, where `ω_quat` is the pure
quaternion `(ω, 0)`, using `Quat::multiply` in a fixed factor order. The normalize
divides by a length clamped to `f32::MIN_POSITIVE`, so a degenerate quaternion can
never yield a `NaN`, and an inactive body keeps its exact stored orientation
(the candidate is selected away by table index, not a branch). So a body **does**
acquire rotation through simulation — from an applied torque.

> **Angular dynamics (landed).** Torque (`apply_torque`) spins a body, orientation
> integrates, and the **contact solver applies the angular half of every impulse**
> about the contact lever arm (`ω += I⁻¹·(r × J)` for both the normal and the two
> friction tangents). So an off-centre hit induces spin and a frictional slide
> converts into roll — a torque-driven sphere on a frictional surface genuinely
> rolls forward. An immovable body's zero inverse inertia makes its angular delta
> vanish exactly, with no branch.

### Pose handling: box rotation honoured, scale ignored

A collider is placed at its owning body's **translation** and **rotation**; only
transform **scale is ignored** (the module carries no collider scale). Rotation is
threaded into both the bounds (`world_aabb` takes a `Quat`) and the narrow phase
(the contact generators receive each body's rotation): a **box** bounds and
collides as an oriented box (OBB), while a **sphere/capsule** is rotation-invariant
and keeps its tight axis-aligned bound. Spatial queries (`physics_query.rs`) remain
rotation-unaware by design — they pass the identity rotation so a query never
*over*-reports a rotated box (exact ray/OBB casting is a later-phase item), whereas
the broad phase must not *miss* a candidate and so uses the true rotation. `box/box`
contacts remain a documented later-phase item.

### Spatial queries (`physics_query.rs`)

Both queries are pure reads — they take `&PhysicsWorld`, never mutate it, and both
skip disabled bodies/colliders. Results are deterministic functions of world state
with explicit tie-breaking and ordering.

- **`raycast`** returns the **nearest** body hit within `max_distance`, or `None`.
  Queries are **exact** per shape kind: exact ray/sphere, exact ray/AABB (a box
  *is* its axis-aligned extents), and analytic ray/plane. **Capsule is excluded**
  (never hit) rather than approximated — a documented deferral. Among hits, the
  smallest entry distance wins, ties broken by the **smaller body handle**.
  Triggers are **excluded** — a ray reports solid geometry only.
- **`overlap_sphere`** returns the bodies overlapping the query sphere as a
  **sorted, de-duplicated** handle list. Exact per kind: exact sphere/sphere,
  exact closest-point sphere/AABB, signed-distance sphere/plane; **capsule
  excluded**. Triggers are **included** — overlap is a presence query.

Shape handling dispatches on `kind().index()` into per-kind function tables, not a
`match`.

### Step record

Each step records the real pipeline counts, summed across the step's substeps:
`broad_phase_pair_count`, `contact_pair_count`, `solved_contact_count` (contacts
the solver actually resolved — approaching contacts that received an impulse),
`frictioned_contact_count` (contacts that received a genuine tangential friction
impulse — approaching, positive combined friction, nonzero tangential speed),
the integrated-body count, body / collider / dynamic-body counts, commands
drained, events emitted, and `substep_count`. `solver_iteration_count` is the
**configured** sequential-impulse budget — diagnostic metadata, **not** a measure
of work performed (use `solved_contact_count` for that). All are read through
`PhysicsStepRecord` accessors and are deterministic per step.

### Substepped, atomic stepping

A single fixed step is split into `max_substeps` deterministic substeps (the first
`remainder` substeps each take one extra nanosecond so durations sum exactly to
the step), shortening each integration interval so a fast body cannot tunnel
through thin geometry in one jump. Commands apply **once** before substepping;
`StepCompleted` emits **once** per outer step. A step **commits only if every
resulting body is finite** — a finite-but-extreme force/impulse/gravity that would
drive computed state to `NaN`/`±∞` rolls the world back to its exact pre-step state
(bodies, events, command queue untouched) and returns `NonFiniteStepResult`. So no
snapshot can ever carry a non-finite value. Substepping *mitigates* tunnelling; it
is **not** continuous collision detection (CCD remains deferred).

### Integration surfaces (neutral data for apps)

Physics exposes neutral, read-only data an app translates into scene/render/debug
state — never the reverse. `ColliderSnapshot::shape()` carries public geometry
accessors (`is_sphere`/`is_box`/`is_capsule`/`is_plane`, `sphere_radius`,
`box_half_extents`, `capsule_radius`/`capsule_half_height`, `plane_normal`/
`plane_distance`, as `Meters`/`Vec3`/`bool`). `PhysicsApi::latest_contacts()`
returns `ContactReport`s (body/collider handles, contact normal, penetration depth
as `Meters`, world contact point). `PhysicsApi::drain_events()` drains the event
log so it stays bounded. `PhysicsError` exposes `raw_code()` + `is_*` predicates so
callers can inspect error kind without naming the internal code enum.

## Events

The event log still carries **only the five lifecycle events**: `BodyCreated`,
`ColliderAttached`, `BodyEnabled`, `BodyDisabled`, and `StepCompleted`. **No
collision or trigger lifecycle events exist** — there is no contact persistence
across steps yet, so contact enter/stay/exit and trigger overlap events are a
documented later-phase item.

## Determinism

The module is deterministic by construction:

- no wall-clock — time enters only as the explicit `RuntimeStep`;
- no randomness, no global mutable state, no thread/async/net/process use;
- ordered `Vec` storage and ordered event/command logs — stable iteration;
- broad-phase output sorted by handle, manifolds processed in that stable order
  every solver iteration;
- handles are monotonic `u64`s assigned at creation and never reused.

The same sequence of facade calls plus the same fixed step always produces the
same handles, snapshot, step record, and event log. This is proven at the facade
level: a dropped sphere settles on a static plane without tunnelling and replays
identically (snapshot **and** step record), an elastic (`restitution = 1`) ball
rebounds, and two dynamic bodies exchange momentum reproducibly.

> **Same-binary replay — that is the precise claim.** Determinism is verified by
> snapshot + step-record equality (`assert_eq!`) across runs of the **same build**.
> The module makes **no** claim of cross-platform / cross-build f32
> bit-determinism, and has no on-disk byte-serialization / replay-to-bytes format
> yet — both are later-phase items. ("Byte-identical" elsewhere in these docs means
> byte-equal in-memory snapshots within one binary, not a serialized golden vector.)

## Composition lives in apps, not here

Physics never reads or writes a scene, renderer, or any other module. Turning a
`PhysicsSnapshot` into scene transforms or render state is the job of an **app**
(or a future feature module that lists physics in `allowed_modules`) — never of
physics itself. That keeps physics a re-composable black box: a future native
app, test app, or WASM app can each drive the same world.

## Explicitly deferred (not done)

Scheduled, in order, in [`ROADMAP.md`](ROADMAP.md). Each is genuinely **not**
implemented yet (and is never claimed to be):

- **capsule contacts** — every capsule pairing produces no contact, and capsule is
  excluded from exact queries;
- **box/box contacts** — two box colliders never contact each other (the marble
  spine only needs a dynamic sphere against static boxes; `sphere/box` and
  `box/plane` are orientation-aware, but box/box is unimplemented);
- **exact ray/OBB and ray/capsule casting** — `raycast`/`overlap_sphere` treat a
  box as axis-aligned (identity rotation) and never hit a capsule;
- **collision / trigger lifecycle events** — no contact persistence across steps,
  so contact enter/stay/exit and trigger overlap events are not emitted (the
  `latest_contacts()` report exposes the current step's contacts, but there is no
  cross-step event stream yet);
- **true continuous collision detection** — substepping mitigates large-dt
  tunnelling but is not CCD;
- **sleeping**;
- **broad-phase acceleration structures** — still `O(n²)`;
- **serialization / replay-to-bytes tooling and a cross-platform determinism
  proof** — replay is proven via same-binary snapshot+record equality only;
- **character controller, static mesh / convex colliders, debug-draw contracts**.

The testing discipline that keeps every phase honest is in
[`TESTING.md`](TESTING.md).
