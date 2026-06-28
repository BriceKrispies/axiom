# `axiom-physics` — Testing

Physics is held to Axiom's **Coverage Law**: every region, line, branch, and
function in the module is exercised by a test, at all times, with no exceptions.
It is held equally to the **Branchless Law**: non-test code contains zero control
flow. This document records *how* both are achieved and the testing principles the
module will not violate as it grows.

## The three test surfaces

1. **Inline `#[cfg(test)] mod tests`** (in every `src/*.rs` except the pure
   re-export files `lib.rs` and `ids.rs`, and `physics_world.rs`). These drive
   the crate-internal types the external test crate cannot name — the flat
   collider shape, the broad-phase pair test, the contact generators, the
   solver, the integrator passes — and, critically, exercise **both arms** of
   every branchless validation and gate. A validation written as
   `[Err(…), Ok(…)][cond as usize]` is only fully covered when a test reaches it
   with `cond == false` *and* `cond == true`; each such site has a
   rejects/accepts (or hit/miss) test pair.

   The phase-specific files carry their own focused proofs:
   - `physics_collider_shape.rs` / `physics_shape_kind.rs` — each constructor's
     valid/invalid pair, the packed-field layout, and the table indices
     (`Sphere = 0 … Plane = 3`).
   - `collider_bounds.rs` — finite shapes have a centred world AABB; a plane has
     none.
   - `broad_phase_pair.rs` — overlapping AABBs pair; separated ones do not; a
     plane pairs with every finite collider regardless of distance; two planes
     never pair; same-body and disabled-body colliders are skipped; output is
     sorted and insertion-order-independent.
   - `contact_pair.rs` — each implemented pairing's hit (normal + depth + point)
     and its miss (separated, exactly touching, or degenerate-coincident); the
     swapped orderings flip the canonical normal; unimplemented pairings
     (box/box, capsule) yield no contact; `generate_contacts` tags a real contact
     with the pair's handles.
   - `contact_solver.rs` — zero restitution removes approaching velocity; full
     restitution reverses it; a separating contact applies no impulse; static
     bodies never move; zero iterations is a no-op; position correction pushes a
     penetrating body out and ignores penetration within the slop.
   - `integrator.rs` — the velocity pass accelerates only enabled dynamic bodies
     and consumes-then-clears force/impulse; the position pass moves active
     bodies by their velocity and ignores static/disabled ones.
   - `physics_query.rs` — raycast hits spheres/boxes/planes, returns the nearest,
     breaks distance ties by the smaller body handle, respects `max_distance`,
     and skips triggers/disabled bodies/invalid rays; `overlap_sphere` finds
     spheres/boxes/planes, excludes distant colliders, de-duplicates multiple
     colliders on one body, includes triggers, and skips disabled bodies.

2. **`tests/integration.rs`** — end-to-end behavioral proofs driven **only**
   through the public facade (`PhysicsApi` + the two handle types). This is where
   the determinism guarantees are proven, and where `physics_world.rs` — the one
   source file with no inline tests — is fully covered. Every world path
   (create/attach success and every typed failure, command enqueue/drain, the
   four command-apply functions, stepping, the full collision pipeline, the
   integrator, snapshots, records, and queries) is reached from here. Scalars
   cross the facade as kernel/math value types (`Ratio`, `Meters`, `Vec3`,
   `Transform`) — never naked floats — exactly as production callers must, and
   the sealed return types (snapshot, record, material) are carried by inference,
   never named.

3. **`tests/architecture.rs`** — boundary/hygiene scans over `src/`: only the
   three legal layers (`axiom_kernel` / `axiom_runtime` / `axiom_math`) are
   imported; no sibling module is referenced; no lower layer imports
   `axiom_physics`; no browser/GPU/DOM/wasm tokens; no wall-clock/randomness/
   threads/net/process; no `println!`-family or placeholder macros; no global
   mutable state; no `HashMap`/`HashSet`; no foreign subsystem concepts
   (scene/render/asset/input/animation/audio/ECS/plugin/editor) or external
   engine crates (rapier/nalgebra/glam/bevy); no junk-drawer module names; plus
   assertions that `module.toml` is isolated (`allowed_modules = []`) and that
   `lib.rs` exports exactly one facade + one `ids` line. These are a per-module
   second line of defence behind the workspace `xtask check-architecture` gate.

## What the determinism and contact-response tests prove (spec §21)

`tests/integration.rs` proves, through the facade:

- **Replay equality (same-binary)** — the same initial world plus the same
  commands plus the same fixed step produces equal snapshots *and* equal step
  records across runs of the same build. This holds for a single force/impulse
  step, a full **800-step settling drop** (`settling_is_deterministic`), and
  dynamic↔dynamic collisions. Cross-platform / byte-serialized determinism is
  **not** claimed (a later phase); "byte-identical" means byte-equal in-memory
  snapshots within one binary.
- **Stable handle order** — bodies and colliders are assigned monotonically
  increasing handles in creation order (`1, 2, 3, …`).
- **Gravity behavior per kind** — a dynamic body accelerates and moves downward;
  static and kinematic bodies do not move under gravity; a disabled dynamic body
  does not integrate.
- **Force/impulse** — applying a force, and applying an impulse, each change a
  dynamic body's velocity in the expected direction, deterministically.
- **FIFO commands** — for a body, enqueuing disable-then-enable ends enabled and
  enable-then-disable ends disabled (the last command wins).
- **Contact response** — the heart of Phase 2:
  - a unit dynamic sphere dropped onto a static ground plane **settles** with its
    centre near `y = 0.5` (`radius` above the surface), is at rest, and **never
    tunnels** through the plane (its minimum centre `y` stays well above zero);
  - the step record, while resting, reports a **real** broad-phase pair count of
    `1` (the infinite plane always pairs with the sphere) and a **real** contact
    pair count of `1` (genuinely in contact);
  - two well-separated bodies report a broad-phase pair (the plane is infinite)
    but **zero** contacts;
  - a perfectly **elastic** (`restitution = 1`) ball **rebounds upward** after
    contact.
- **Deterministic records and snapshots** — the step record reports the exact
  counts (bodies, colliders, dynamic bodies, commands drained, events, integrated
  bodies, real broad/contact pair counts, the configured solver iteration count),
  and snapshots list bodies/colliders in insertion order.
- **Typed validation, never a panic** — invalid mass, invalid material, every
  invalid collider shape, an unknown body handle, force/impulse on a non-dynamic
  body, a non-finite force/impulse, a zero-length step, an invalid configuration,
  and body/collider capacity limits all return a typed error (`is_err()`), and a
  rejected step does not advance the world. The specific machine-readable error
  *codes* are asserted by the inline unit tests (where `PhysicsErrorCode` is
  nameable).
- **Queries through the facade** — a ray finds a collidered body and a query
  sphere overlaps it; a body with no collider is invisible to both; queries never
  mutate world state.
- **Event invariant** — only the five lifecycle event kinds are ever emitted (no
  contact/collision/trigger/overlap event appears, asserted by scanning each
  event's `Debug` rendering). There is no contact persistence yet, so this
  invariant still holds in Phase 2.

## Remediation regression suites

The legitimacy-audit remediation (see [`REMEDIATION.md`](REMEDIATION.md)) added
dedicated, strongly-asserting regression suites under `tests/`, each guarding a
class of finding so it cannot silently return:

- `dynamic_dynamic_solver.rs` — two **dynamic** bodies exchange momentum through
  `step()` (equal/unequal mass, restitution 1/0, separation, resting stack,
  deterministic replay).
- `end_to_end_contact_pairs.rs` — every implemented pairing (sphere/sphere,
  sphere/plane, sphere/box, box/plane) driven through the full pipeline with real
  body-state, count, finiteness, and replay assertions.
- `determinism_poison.rs` — finite-but-extreme inputs never poison state; a
  non-finite result rolls back atomically and no snapshot holds `NaN`/`±∞`.
- `substepping_large_dt.rs` — `max_substeps` is consumed, commands apply once,
  `StepCompleted` emits once per outer step, and a high substep count prevents the
  large-dt plane tunnelling a single substep allows.
- `event_drain.rs` — draining keeps the event log bounded across many steps.
- `disabled_body_semantics.rs` — force/impulse on a disabled body is rejected, not
  silently dropped.
- `step_record_honesty.rs` — `solved_contact_count` reflects real solver work;
  `solver_iteration_count` is metadata, never asserted as proof of solving.
- `integration_surface.rs` — neutral collider-geometry accessors, `ContactReport`
  (normal/point), and inspectable typed errors, with no world mutation.
- `doc_hygiene.rs` — fails if any `src/*.rs` doc comment reverts to a stale
  "empty scaffold" / "always 0" / "Phase 1" claim.

## Principles this module's tests will not violate

These follow directly from the Coverage Law's "no fluff" clause:

- **Every facade method is tested directly** through its real public entry point.
- **No "does not panic"-only tests.** Every test asserts on observable behavior —
  a value, an ordering, an error, a count, a settled position, a rebound — never
  merely that a call returned. The contact tests assert real physical outcomes
  (resting height, no tunnelling, velocity reversal), not just that the pipeline
  ran.
- **Coverage is reached by exercising real behavior, not by adding shims.** When
  a branchless table-index or gate needs both arms covered, the test supplies
  both real inputs; we never add a dead arm, a pass-through wrapper, or a getter
  purely to host a test, and never widen the public API just to reach internal
  state.
- **Determinism is a test target, not a comment.** Any new behavior ships with a
  replay-equality proof, the same way the settling drop does.

## Running the gates

```sh
cargo test -p axiom-physics          # the three suites above
cargo xtask check-architecture       # Module Law / Layer Law / hygiene
cargo dylint --all -- --all-targets  # Branchless Law + no-naked-float + size
scripts/coverage.ps1                 # 100% regions/lines/functions (this module included)
```

As the remaining phases land (see [`ROADMAP.md`](ROADMAP.md)), each arrives
**with the tests that cover all of it** in the same change — there is no "add
tests later." A change that drops the module below 100% coverage is, by Axiom's
definition, broken.
