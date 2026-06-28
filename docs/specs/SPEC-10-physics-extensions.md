# SPEC-10 — Physics extensions (angular, friction, damping)

> Status: Draft
> Contract: §18 impl-order #10 (rigid-body completion)   Vocabulary: Rigid-body physics (partial — angular deferred), Friction/damping/brake (missing/partial), Knockback/impulse (have), Ballistic/jump arcs (compose)   Determinism: sim

## 1. Summary

`axiom-physics` is a real, deterministic rigid-body core, but three pieces of the
rigid-body model the contract names in §18 #10 are **deferred, not done**:
**angular dynamics** (orientation is never integrated), **friction resolution**
(the coefficient is validated and stored but the solver applies no tangential
impulse), and **damping/brake** (no velocity decay). This spec closes those three
gaps in place.

These are not exotic — they are what makes a rigid body behave like a body.
Without friction a box on any slope slides forever and a stack never settles
laterally; without angular integration nothing tumbles, spins, or topples;
without damping nothing coasts to rest or brakes. Across the 11-game catalog the
falling/colliding/knockback games (platformer, top-down brawler, physics toy,
projectile games) need all three; *knockback* and *ballistic/jump arcs* already
**compose** from the existing `apply_impulse` + gravity, so this spec adds no new
verb for them — it completes the substrate they sit on.

## 2. Current state (verified)

- **Linear core is real.** Semi-implicit (symplectic) Euler split into
  `integrate_velocities` → contact `solve` → `integrate_positions`
  (`integrator.rs`); gravity, `apply_force`/`apply_impulse`; `O(n²)` AABB broad
  phase; sphere/sphere, sphere/plane, sphere/box, box/plane narrow phase; a
  sequential-impulse solver with combined (max) restitution + Baumgarte position
  correction (`contact_solver.rs`); configurable substepping with atomic
  non-finite rollback.
- **Angular state is stored but inert.** `PhysicsBody` carries
  `angular_velocity: Vec3` and `ForceAccumulator` carries `torque: Vec3`
  (`physics_body.rs`, `force_accumulator.rs`); `MassProperties` carries
  `local_inverse_inertia: Vec3` fixed at `Vec3::ZERO`. **The integrator never
  reads torque, never integrates angular velocity, and never rotates a body** —
  `integrate_position` preserves `rotation` verbatim. No `apply_torque` on the
  facade.
- **Friction is stored but unresolved.** `PhysicsMaterial::new` validates
  `friction >= 0` and exposes `friction()` (`physics_material.rs`), but
  `contact_solver::solve` applies **only** a normal impulse — there is no
  tangential pass.
- **No damping.** Neither `PhysicsConfig` nor `PhysicsBody` carries a linear or
  angular damping coefficient; velocity changes only via gravity/force/impulse and
  the contact solve.
- **Determinism is same-binary only.** `ARCHITECTURE.md` is explicit: replay is
  proven by in-memory snapshot + step-record `assert_eq!` across runs of the
  **same build**. There is **no** cross-platform / cross-build f32 bit-determinism
  claim and no byte-serialized replay format. §17.6 (cross-instance determinism
  for netplay) is **not yet guaranteed**.
- **Math substrate is sufficient for orientation.** `axiom-math` already exposes
  `Quat::from_axis_angle`, `Quat::multiply` (Hamilton product, `const`),
  `Quat::normalize`, and `Quat::rotate` — enough to integrate orientation without
  any new lower-layer primitive.

## 3. Architectural placement

**Extend the existing `axiom-physics` engine module. No new module, no new layer.**
This is the only legal home, under the Module Law:

- These three features are **completions of the one rigid-body world**, not a new
  isolated capability. Angular integration, friction, and damping all read and
  mutate the *same* private `PhysicsBody`/`PhysicsWorld`/solver state. An engine
  module owns exactly one capability behind one facade (Module Law #8); splitting
  "angular physics" into a second module would force two modules to share
  `PhysicsBody` — forbidden (`allowed_modules = []`, Module Law #2). The primitive
  that would otherwise be "shared" already lives correctly **below** physics: the
  quaternion product is in the `math` layer.
- `axiom-physics` keeps its declared deps — layers `kernel`, `runtime`, `math` —
  and uses no new one. `Quat::multiply` is already part of the `math` public
  surface physics depends on, so the orientation integrator is built at the
  module, not by reaching into a private path or adding a math primitive
  (No-Shortcuts: the fix is at the lowest layer that *already* offers it).
- It stays a `sim`-class spine module: branchless production code, single facade,
  100% coverage. The additions are new arithmetic in existing private files
  (`integrator.rs`, `contact_solver.rs`, `mass_properties.rs`, `physics_config.rs`,
  `force_accumulator.rs`) plus new facade methods on `PhysicsApi`.

Internal landing order (each self-contained, fully covered, replay-proven):
**damping** (smallest — a velocity-decay multiply), then **friction** (tangential
solver pass), then **angular** (torque accumulation + orientation integration +
the angular term in the solver). This matches `ROADMAP.md` Phases 3/6, pulled
into one spec because §18 #10 names them as one deliverable.

## 4. API surface

### 4.1 Native (`PhysicsApi`, sim-class)

Additions only; every existing method is unchanged.

```rust
// Angular: queue a torque (the angular analogue of apply_force), drained FIFO and
// integrated in step(). Rejected on a non-dynamic/disabled body, like apply_force.
pub fn apply_torque(&mut self, body: PhysicsBodyHandle, torque: Vec3) -> PhysicsResult<()>;

// Damping/brake: per-world linear & angular velocity decay (per-step multiplicative
// factor in [0, 1]); carried as kernel Ratio, never a naked float. Folded into
// with_config so the world stays constructed from one validated config.
pub fn with_config(
    gravity: Vec3,
    solver_iterations: u32,
    max_bodies: u32,
    max_colliders: u32,
    max_substeps: u32,
    sleeping_disabled: bool,
    linear_damping: Ratio,      // NEW — 0 = no decay (today's behaviour)
    angular_damping: Ratio,     // NEW
) -> PhysicsResult<Self>;
```

- **Friction needs no new method.** It is resolved from the material already on
  the collider (`PhysicsApi::material(friction, restitution, density)`); this spec
  only makes the solver *use* the stored `friction()`.
- **Inertia is derived, not authored.** `MassProperties::dynamic` computes
  `local_inverse_inertia` from the collider shape + mass (today `Vec3::ZERO`); no
  facade argument is added (Open Q 9 on full tensor vs scalar moment).

### 4.2 TS authoring projection (contract §18 #10)

```ts
// On the body facade (projected through the SPEC-00 boundary app):
applyImpulse(body: Body, j: Vec3): void;     // have — knockback
applyForce(body: Body, f: Vec3): void;       // have
applyTorque(body: Body, t: Vec3): void;      // NEW — angular
// Damping is world config (per GameConfig.physics), not a per-call verb:
interface PhysicsConfig { gravity: Vec3; linearDamping: number; angularDamping: number; /* … */ }
// Friction is a material property already projected on collider attach; no new verb.
// Ballistic/jump arcs compose from applyImpulse + gravity — no dedicated API.
```

## 5. Data contracts

- **`PhysicsSnapshot` shape is stable.** It already surfaces each body's
  `transform` (rotation included) and `angular_velocity`; once the integrator
  writes them, replay equality covers angular state with no new field.
- **`PhysicsStepRecord`** gains an honest count for the new work (e.g.
  `frictioned_contact_count` alongside `solved_contact_count`) — same accessor
  pattern, deterministic per step.
- **`ContactReport`** is unchanged: the tangential impulse is an internal solver
  detail, not a new boundary field.
- **`PhysicsConfig`** gains `linear_damping`/`angular_damping` (`Ratio`,
  validated `0 <= d <= 1`). No type a caller must *name* is added — damping enters
  through `with_config` primitives.

## 6. Determinism

This is the load-bearing section: §17.6 (cross-instance) is the central concern,
because friction + angular **multiply the float-op count** along the spine, and
every new op is a chance for two machines to disagree.

- **Single clock / no randomness** are unchanged — `dt` still derives solely from
  the explicit `RuntimeStep`; the new code reads no clock and no `Rng`.
- **Ordered evaluation is preserved.** The friction pass walks the *same* stable
  handle-sorted manifold order as the normal pass, and the angular term is applied
  in that same order, so within one build the result stays a pure function of
  world state.
- **Deterministic tangent basis.** The friction impulse needs a tangent direction;
  it must be derived **deterministically from the contact normal** (a fixed,
  branchless construction — e.g. the larger-axis cross product), never from
  discovery order or an iterative GS that could pick a different basis per run.
  Coulomb clamp `|j_t| <= friction * j_n` is applied with `min`/`max`, not a
  branch.
- **Orientation integration** is `q' = normalize(q + 0.5·dt·(ω_quat ⊗ q))` using
  `Quat::multiply` in a fixed factor order then `Quat::normalize`; the normalize
  divide-by-length is clamped (as `MassProperties` already clamps its reciprocal)
  so a degenerate quaternion never yields `NaN`. The existing atomic non-finite
  rollback still guards the whole step.
- **The gating risk — cross-platform f32.** Today's guarantee is **same-binary**
  only. More float ops widen the gap between machines with different rounding/FMA
  contraction. To make §17.6 real, the build must pin **deterministic
  arithmetic**: a fixed operation order (already the discipline), **no fast-math /
  no FMA contraction** in spine math, and a decision (Open Q) on whether
  controlled `f32` is acceptable for the netplay tier or whether sim physics must
  move to **fixed-point**. This spec does not silently claim cross-instance
  determinism: it specifies the work and flags the decision as a prerequisite for
  SPEC-13.

## 7. Acceptance / proof

Ships with all of the below; nothing lands "tests later" (Coverage Law).

- **100% coverage, branchless.** Every new region — the tangential clamp, the
  angular integrate, the damping multiply, the inertia derivation, and both arms
  of every gate — is covered, and the solver/integrator math stays branchless
  (arithmetic gating, function-table dispatch), verified by `engine_no_branching`.
- **Friction golden — rest vs slide.** A box on an inclined plane with high
  friction stays put (tangential velocity driven to ~0 within the Coulomb cone);
  the same box with `friction = 0` slides under gravity. Asserted on committed
  snapshots.
- **Angular golden.** A body given a torque/impulse acquires angular velocity and
  its `transform.rotation` advances each step; with `angular_damping < 1` the
  spin decays monotonically toward rest; momentum/`is_finite` invariants hold.
- **Damping golden.** A coasting body's linear speed decays monotonically by the
  configured factor and reaches rest; `damping = 0` reproduces today's behaviour
  exactly (regression guard).
- **Two-world byte-equal replay.** Extend the existing two-world harness in
  `apps/axiom-physics-crucible` (`src/replay_bay.rs` already shoves a sphere by
  impulse and asserts the visible and hidden replay worlds stay in lockstep) with
  a station exercising friction + spin, proving snapshot + step-record equality
  across the two worlds and across a re-run (same-binary replay; tick-N replayed
  twice is byte-equal).
- **Determinism poison** (`tests/determinism_poison.rs`) extended so a perturbed
  replay of the angular/friction path is *detected*.

## 8. Dependencies & order

- **Structurally unblocked now.** No new layer/module/dep; `Quat::multiply` and
  `Quat::normalize` already exist in `math`. Can land against the current tree.
- **Internal order:** damping → friction → angular (§3).
- **Gates SPEC-13.** Multiplayer prediction/reconciliation (§16, §17.6) requires
  cross-instance-deterministic physics; the f32-vs-fixed-point decision (§6, Open
  Q) must be settled **before** physics is trusted on the predicted path. Until
  then physics remains a single-authority / same-binary capability and SPEC-13
  must not predict it.
- **Interacts with the deferred narrow-phase phases.** `ARCHITECTURE.md` ties
  oriented-box contacts to angular dynamics (a non-rotating body never acquires a
  rotation, so AABB geometry is exact today). Once bodies rotate, box geometry
  that *reads* orientation is needed for correctness — see Open Q.

## 9. Open questions

- **f32 vs fixed-point for §17.6.** Is controlled, no-fast-math `f32` acceptable
  as the cross-instance contract for the netplay tier, or must the sim physics
  pipeline move to deterministic fixed-point? This is the single decision gating
  netplay-grade physics; it is bigger than this module (it touches `math`) and
  should be decided with SPEC-13.
- **Full inertia tensor vs scalar moment.** The catalog is 2D-dominant; a single
  scalar (or diagonal `Vec3`) moment of inertia is far cheaper and may suffice.
  Start with the diagonal `local_inverse_inertia` already on `MassProperties`
  (derived per shape), and only adopt a full tensor if a 3D tumbling case proves
  it necessary.
- **Oriented-box contacts.** Must oriented-box narrow phase land *with* angular
  integration (so a spinning box collides correctly), or can angular ship first
  for free-flight/torque cases with oriented contacts following as ROADMAP Phase 4
  intends? Default: angular integration first (free bodies + spheres are correct
  immediately); oriented-box contacts as a fast-follow.
- **Damping placement — world vs per-body.** This spec puts damping in
  `PhysicsConfig` (one validated construction point). A per-body brake (e.g. a
  character decelerating) may later want per-body damping; defer until a second
  consumer proves the need rather than widening the facade now.
