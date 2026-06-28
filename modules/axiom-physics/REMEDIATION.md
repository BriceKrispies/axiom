# `axiom-physics` — Remediation Pass

This document records the root-cause remediation that resolved the findings of
[`LEGITIMACY_AUDIT.md`](LEGITIMACY_AUDIT.md), moving the module from
**PARTIALLY LEGITIMATE BUT OVERCLAIMED** toward **LEGITIMATE FOUNDATION**. It is
the issue → fix → tests → files → remaining-limitations ledger; the audit's
original verdict is preserved unchanged, with a remediation-result section
appended to it.

The remediation did not relax a single law: every change stays branchless in
production code (`engine_no_branching` clean), keeps the single `PhysicsApi`
facade (Module Law #8), holds the module at 100% coverage, carries no naked float
across a public API, and remains headless / WASM-clean. No lower layer
(`axiom-kernel` / `axiom-runtime` / `axiom-math`) was modified.

---

## Blockers — all fixed

### B1 — Documentation contradicted by its own passing tests
**Fix.** Every stale Phase-1 doc-comment was rewritten to describe the real,
current pipeline; the genuinely-deferred items (friction, capsule/box-box
contacts, collision/trigger events, angular dynamics) are stated as deferrals,
not absences. The lying counts/scaffold claims in `lib.rs`, `physics_event.rs`,
`physics_material.rs` (restitution *is* solved — only friction is deferred),
`physics_collider.rs`, `physics_step_record.rs`, `physics_step_result.rs`, and
the angular comments are gone.
**Regression guard.** A new `tests/doc_hygiene.rs` scans `src/*.rs` and fails if
any file describes current behavior with `"empty scaffold"`, `"always 0"`,
`"no contact work"`, `"no broad phase"`, `"no narrow phase"`, `"no contact
solver"`, or the rotting label `"Phase 1"`. The doc rot cannot return.

### B2 — Determinism poison path (finite-but-extreme inputs → `Inf`/`NaN`)
**Fix.** `PhysicsWorld::step` is now **atomic**: it snapshots body state, runs the
substeps, and commits **only if every resulting body is finite**
(`PhysicsBody::is_finite_state`). A non-finite result rolls the world back to its
exact pre-step state — bodies, event log, and command queue all untouched — and
returns a deterministic `NonFiniteStepResult` error. A committed body, and
therefore every snapshot, can never carry a non-finite value, so replay equality
cannot be silently broken by extreme input.
**Tests.** `tests/determinism_poison.rs` (8): extreme impulse/force/gravity,
summed-impulse overflow, solver overflow, snapshot-unchanged-after-rejection,
deterministic recovery after a rejected step, and a global "no snapshot ever
holds a non-finite value" sweep.

### B3 — Two-body solver and per-pairing contact response unproven
**Fix.** The solver math was already correct; the gap was test coverage.
**Tests.** `tests/dynamic_dynamic_solver.rs` (7) proves two *dynamic* bodies
exchange momentum through `step()` (equal and unequal masses, restitution 1 and
0, separation, deterministic replay, a two-sphere resting stack) plus solver-unit
twins in `contact_solver.rs`. `tests/end_to_end_contact_pairs.rs` (4) drives every
implemented pairing (sphere/sphere, sphere/plane, sphere/box, box/plane) through
the full `step()` pipeline, asserting broad/contact counts, an observable
body-state effect, finiteness, and replay-identical snapshot+record.

---

## Major issues — fixed (one partially, with an honest documented deferral)

### M1 — Queries were AABB/bounding-sphere approximations behind exact names
**Fix.** `raycast` and `overlap_sphere` are now **exact** for every supported
shape: exact ray/sphere, exact ray/AABB (a box *is* its extents), analytic
ray/plane; exact sphere/sphere, exact closest-point sphere/AABB, signed-distance
sphere/plane. **Capsule is explicitly excluded** (never hit/reported) rather than
silently approximated — documented at the query site and on the facade.
**Tests (in `physics_query.rs`).** Including `raycast_misses_a_sphere_it_only_
clips_the_aabb_of` (the red-team false positive), nearest-hit, handle tie-break,
`overlap_sphere_does_not_report_sphere_outside_exact_radius`, sorted output,
no-mutation, disabled-skip, explicit trigger policy, and capsule-exclusion proofs.

### M2 — Large-dt tunnelling and an inert `max_substeps`
**Fix.** `max_substeps` is now **live**: a step is split into that many
deterministic substeps (the first `remainder` substeps each take one extra
nanosecond so substep durations sum exactly to the step). Commands apply **once**
before substepping; `StepCompleted` emits **once** per outer step;
`substep_count` is recorded. A fast body that tunnels through a plane at
`max_substeps = 1` does not tunnel at a higher count.
**Tests.** `tests/substepping_large_dt.rs` (8), incl.
`large_step_is_substepped_and_does_not_tunnel_through_plane`,
`commands_apply_once_across_substeps`, `step_completed_event_emits_once_for_outer_
step`, remainder determinism, and replay.
**Honest scope.** Substepping *mitigates* tunnelling; it is **not** continuous
collision detection. True CCD remains deferred (ROADMAP).

### M3 — Unbounded event log
**Fix.** `PhysicsApi::drain_events()` drains and clears the log in deterministic
order; `events()` remains the read-only view. An app drains once consumed, so the
`StepCompleted`-per-step growth is bounded.
**Tests.** `tests/event_drain.rs` (6), incl.
`step_completed_does_not_grow_without_bound_when_drained_each_step` (1000 steps).

### M4 — Integration surfaces (collider geometry / contacts / collision events)
**Fixed (2 of 3).**
- **Collider geometry is no longer opaque.** `PhysicsColliderShape` gained public,
  neutral accessors — `is_sphere/is_box/is_capsule/is_plane`, `sphere_radius`,
  `box_half_extents`, `capsule_radius`, `capsule_half_height`, `plane_normal`,
  `plane_distance` (returning `Meters`/`Vec3`/`bool`, never a naked float) — so an
  app can read a `ColliderSnapshot`'s geometry without naming the internal tag.
- **Contacts are now reachable.** `PhysicsApi::latest_contacts()` returns neutral
  `ContactReport`s (body/collider handles, contact normal, penetration depth as
  `Meters`, and the world contact **point** — the formerly-dead manifold field is
  now consumed and exposed).
**Deferred (1 of 3), documented.** Collision/trigger **lifecycle events**
(enter/stay/exit) are **not** implemented this pass — see "Deferred" below.
**Tests.** `tests/integration_surface.rs` (13).

---

## Minor issues

| # | Issue | Resolution |
|---|---|---|
| m1 | `solver_iteration_count` echo theater | Added `solved_contact_count` (real solved-contact work); `solver_iteration_count` is now documented as configured metadata; tests no longer assert it as proof of solving (`tests/step_record_honesty.rs`). |
| m2 | Contact `point` stored but dead | Now consumed: `ContactManifold::point()` accessor feeds `ContactReport::point()` exposed via `latest_contacts()`. |
| m3 | Typed error unreachable to callers | `PhysicsError` gained `raw_code() -> u16` plus `is_*` predicates (invalid-mass, body-not-found, non-dynamic force/impulse, disabled-body, non-finite-step) — law-clean inspection with **no** new `lib.rs` export. |
| m4 | Disabled-body force silently dropped | Force/impulse on a disabled body now returns `OperationOnDisabledBody` (`tests/disabled_body_semantics.rs`). |
| m5 | Derive/Debug "coverage theater" | Retained: these are sanctioned **Coverage-Law exercises** (the derived `Debug`/`Clone`/`PartialEq` regions must be touched). The *behavior* they sit beside is proven by the new strong-assertion tests; they are not the proof. |
| m6 | Hidden O(n) `find` in the hot path | Body/collider lookups in the solver and world are now O(1) dense-index lookups (handles are 1-based, creation-ordered, never removed). The broad phase remains O(n²) — a documented, accepted Phase-bounded cost. |
| m7 | Per-step allocation | The atomic-rollback design adds one per-step body-snapshot clone (the cost of poison-safety) and still allocates the per-substep pair/manifold vectors. This is an accepted, documented Phase-bounded cost; pooling is a future optimization. |
| m8 | "Deterministic" wording overclaimed | Determinism is documented as **same-binary, same-process replay** (proven by snapshot+record equality). Cross-platform/byte-serialized determinism is explicitly **not** claimed and remains a future item (ROADMAP). |

---

## Deferred this pass (honest, documented — not faked)

- **Collision / trigger lifecycle events** (enter/stay/exit). Implementing correct,
  deterministic, fully-covered contact persistence across steps — and separating
  trigger overlaps from solver contacts across the broad/narrow/solver pipeline —
  is a substantial feature. Rather than ship a half-correct or faked event system
  (a No-Shortcuts violation), it is deferred. The `PhysicsEvent` enum remains
  truthful: it emits only the five lifecycle events and advertises no collision
  variant. ROADMAP Phase 5.
- **Friction** (tangential impulse). Material friction is validated and stored but
  not yet solved. ROADMAP Phase 3.
- **Capsule and box/box contacts**, and **exact capsule queries**. ROADMAP Phase 4.
- **Angular / rotational dynamics.** Angular velocity and torque are stored but
  never integrated; colliders are placed by translation only. ROADMAP Phase 6.
- **True continuous collision detection** (substepping mitigates, does not
  replace). **Broad-phase acceleration structure** (O(n²) today). **Cross-platform
  / byte-serialized determinism proof.** ROADMAP Phases 7+.

---

## Files changed

**Production (`src/`)** — behavioral: `physics_world.rs` (atomic substepped step,
rollback, dense-index lookups, disabled-body validation, event drain, contact
retention), `contact_solver.rs` (dense index, `count_solved_contacts`),
`integrator`-adjacent `physics_body.rs` (`is_finite_state`, `Clone`),
`physics_config.rs` (`max_substeps` accessor), `physics_step_result.rs` /
`physics_step_record.rs` (solved-contact + substep counts, honest docs),
`physics_error_code.rs` (+`OperationOnDisabledBody`, +`NonFiniteStepResult`),
`physics_error.rs` (`raw_code` + predicates), `physics_collider_shape.rs` (public
geometry accessors), `contact_manifold.rs` (`point()`), `physics_query.rs` (exact
queries), `physics_api.rs` (`drain_events`, `latest_contacts`), `lib.rs`
(`mod contact_report`). **New file:** `contact_report.rs`. **Doc-comment-only:**
`physics_event.rs`, `physics_material.rs`, `physics_collider.rs`,
`mass_properties.rs`, `physics_body_kind.rs`, `integrator.rs`,
`broad_phase_pair.rs`, `contact_pair.rs`, `collider_bounds.rs`,
`physics_shape_kind.rs`.

**Tests (`tests/`)** — new: `dynamic_dynamic_solver.rs`,
`end_to_end_contact_pairs.rs`, `determinism_poison.rs`, `substepping_large_dt.rs`,
`event_drain.rs`, `disabled_body_semantics.rs`, `step_record_honesty.rs`,
`integration_surface.rs`, `doc_hygiene.rs` (57 new integration tests). Two renames
in `integration.rs` (`step_record_reports_deterministic_phase1_counts` →
`…real_counts_for_a_single_body`; `only_lifecycle_events…` →
`no_collision_or_trigger_events_are_emitted_yet`). Plus new inline unit tests in
`contact_solver.rs`, `physics_body.rs`, `physics_world.rs`,
`physics_collider_shape.rs`, `physics_error.rs`, `contact_report.rs`.

No lower-layer, app, or tooling files were changed.
