# `axiom-physics` — Adversarial Legitimacy Audit

> An adversarial team of ten prosecutorial subagents audited `modules/axiom-physics`
> read-only (one red-team agent ran a temporary scratch test, since deleted). The
> charge was to determine whether this is a legitimate real physics module or
> scaffold-shaped theater. The brief was hostile: assume it cheats, do not accept
> docs / passing tests / no-ops / "deterministic" claims as evidence without the
> specific proof named, and do not sugarcoat. This document records the findings.

---

## 1. Final verdict

**PARTIALLY LEGITIMATE BUT OVERCLAIMED.**

The engine is **real, not theater.** A red-team scratch test driven only through the
public facade proved genuine physics actually happens: two equal-mass dynamic
spheres collide head-on and *exchange momentum exactly* (`+1/-1 → -1.0/+1.0`,
`contact_pair_count = 1`); a dropped sphere settles on a plane near `y = 0.5`
without tunnelling; an elastic ball rebounds; a five-sphere stack stays stable and
finite; and two independent runs replay snapshot-equal. The architecture is
spotless — a correctly isolated engine module, single facade, three legal layer
deps, branchless, 100% covered, zero boundary leaks. This is a legitimate
foundation for a linear rigid-body core.

It is **overclaimed** on four counts that a legitimacy audit cannot waive:

1. **The in-source documentation lies about the current code.** Multiple `///`
   doc-comments still assert Phase-1 behavior — "the collision pipeline and queries
   exist as deterministic, empty scaffolds," broad/contact counts "always `0`," the
   solver does "no contact work" — that is **directly contradicted by the module's
   own passing tests** (which assert those counts equal `1` and that the ball
   bounces). Documentation that the code's own test suite refutes is the textbook
   structural lie this audit was sent to find.
2. **`raycast` and `overlap_sphere` are AABB / bounding-sphere approximations
   wearing exact-sounding names**, and the facade docs do not disclose it. The
   red team confirmed a ray that geometrically *misses* a sphere
   (`axis_dist = 0.693 > radius 0.5`) still returns a hit.
3. **The central two-body claim is unproven by the committed suite.** Dynamic↔dynamic
   momentum exchange *works* (red team proved it), but **no committed test exercises
   it** — every solver test pairs a dynamic body with a *static* one (`inv_mass = 0`),
   so the inverse-mass impulse split that makes this a two-body solver is never
   asserted. Likewise, of the four implemented contact pairings, only sphere/plane
   is validated end-to-end through `step()`.
4. **The advertised determinism invariant is silently breakable.** Validation
   screens input *finiteness* but never *magnitude*, and integrator/accumulator
   *outputs* are stored without re-validation, so a finite-but-extreme force,
   impulse, summed impulse, or gravity can drive stored state to `Inf`/`NaN` —
   corrupting subsequent steps, snapshots, and the replay equality the module
   claims. Untested.

The module is a genuine, working, deterministic **linear** rigid-body engine whose
documentation, query-method naming, and test suite claim more than is proven or
exact. Fixing the four items above (mostly doc + test + a magnitude guard, no
architectural change) would move it to a clean **LEGITIMATE FOUNDATION**.

---

## 2. Executive summary

| Dimension | Verdict |
|---|---|
| Architecture | **PASS** — clean, isolated, branchless, single facade, no leaks |
| Determinism (same-binary replay) | **PASS** — ordered storage, explicit tie-breaks, monotonic handles, single dt source |
| Determinism (under extreme-but-finite input) | **FAIL** — silent `Inf`/`NaN` poison path, untested |
| Physics behavior | **REAL but partial** — momentum exchange, settling, restitution all real; angular/friction are honest no-ops |
| API surface | **PASS structurally / FAIL on honesty** — all 21 methods tested; query names + step-record doc mislead |
| Tests | **74/100** — real numeric assertions, but dynamic-dynamic untested + echo/derive theater |
| Documentation | **FAIL (remediable)** — Markdown honest; inline `///` comments stale & contradicted by tests |
| Error/validation | **PASS inputs / FAIL state-integrity** — every invalid input rejected pre-store; magnitude unbounded |
| Performance/scaling | **Acceptable Phase-2 + one HIGH risk** — unbounded event log; hidden O(n) solver `find` |
| Engine integration | **PASS boundaries / PARTIAL capability** — body poses wireable; collider geometry opaque, no contact/collision data |

It is not scaffold-only (real physics runs), not architecturally invalid
(architecture passes cleanly), and not physics-theater (the named concepts map to
working, tested code). It is a real foundation that overclaims in its docs, its
query naming, and the completeness implied by its test suite.

---

## 3. Architecture legitimacy — **PASS**

Prosecuted on nine counts; all clean.

- **Engine module, not a layer.** `module.toml:12` → `kind = "engine-module"`; no
  `layer.toml` exists.
- **Single facade.** `lib.rs` has exactly two top-level `pub use`:
  `pub use ids::{PhysicsBodyHandle, PhysicsColliderHandle};` (`lib.rs:72`) and
  `pub use physics_api::PhysicsApi;` (`lib.rs:73`). All 28 other modules are private
  `mod`. Compliant with Module Law #8.
- **Manifest isolation.** `module.toml:18` `allowed_modules = []`;
  `allowed_layers = ["kernel","runtime","math"]`.
- **Cargo deps.** Exactly `axiom-kernel`, `axiom-runtime`, `axiom-math`
  (`Cargo.toml:17-19`); `unsafe_code = "forbid"`. No external physics/math/ECS crate.
- **Forbidden imports.** Grep for scene/render/assets/input/ECS/web_sys/js_sys/
  wasm_bindgen/WebGPU/WebGL/canvas/SystemTime/Instant/rand/getrandom/thread/tokio/
  lazy_static/once_cell/HashMap/HashSet/BTreeMap/println!-family → the only hits are
  *prose in doc comments disclaiming these very concepts*. Zero real imports.
- **No junk-drawer files** (utils/helpers/common/misc/shared/prelude).
- **No lower-layer back-import** — grep `axiom_physics` across `crates/` → nothing.
- **Public-path-only cross-layer access** — every `use` hits a root export, never a
  private module path.

Required architectural fixes: **none.**

---

## 4. Determinism legitimacy — **PASS for same-binary replay; one untested hole (see §9); cross-platform unproven**

**Proven (with the test that proves each):**

- Ordered storage everywhere — bodies/colliders/commands/events/pairs/manifolds/
  snapshots/query-results are all `Vec`; `tests/architecture.rs` mechanically bans
  `HashMap/HashSet` (`:359`) and `BTreeMap/BTreeSet/LinkedList` (`:412`).
- Explicit, tested tie-breaks — broad-phase pairs normalized ascending and
  `sort_by_key` (`broad_phase_pair.rs:35-41,117`, test `:224,:281`); raycast nearest
  ties broken by smaller body handle (`physics_query.rs:98-102`, test `:290`);
  overlap output `sort()+dedup()` (test `architecture.rs:465`).
- Monotonic, never-reused handles — `u64` counters, no destroy API
  (`physics_world.rs:135-141`), tests `integration.rs:81,96`.
- Single dt source — `dt = nanos as f32 / NANOS_PER_SECOND` in one place
  (`physics_world.rs:272-273`), nanos only from `step.fixed_delta_nanos()`; no wall
  clock anywhere.
- Replay equality — `identical_inputs_produce_byte_identical_snapshots_and_records`
  (`integration.rs:48`, full snapshot **and** record), `identical_worlds_replay…`
  (`architecture.rs:489`, 120 steps), `settling_is_deterministic`
  (`integration.rs:629`, 800 steps); FIFO drain (`integration.rs:208`);
  insertion-order independence of broad phase (`broad_phase_pair.rs:281`); red-team
  scenario 11 confirmed two independent stack-drop runs are snapshot-equal.

**Scope caveat (mild overclaim):** every determinism proof is in-process
`assert_eq!(run(), run())`. There is **no** cross-platform / native-vs-wasm replay
test and no serialized golden vector. The math uses only correctly-rounded IEEE-754
ops with no transcendentals, so cross-platform bit-determinism is *plausible* but
**unproven**, and Rust gives no guarantee against backend FMA-contraction
differences. The docs use unqualified "deterministic" and "replays
byte-identically" (`ARCHITECTURE.md:308`, `ROADMAP.md:9`, `TESTING.md:78`); the one
caveat (`ARCHITECTURE.md:311`) scopes *serialization*, not *float portability*. The
honest scope is: **same-binary, same-process replay — verified; cross-platform
f32 bit-equality — neither precisely claimed nor proven.**

---

## 5. Physics-behavior legitimacy — **REAL but partial**

| Capability | Verdict | Proof |
|---|---|---|
| Gravity moves dynamic; static/kinematic frozen | REAL | `integration.rs:117,140,151`; `integrator.rs:96` |
| Force / impulse → dynamic velocity | REAL | `integration.rs:166,187` |
| Inverse mass (static/kinematic = 0; dynamic = 1/m) | REAL | `mass_properties.rs:78,84` |
| Broad phase generates AABB-overlap pairs (hit + miss) | REAL | `broad_phase_pair.rs:213,239,258,265` |
| Narrow phase sphere/sphere, sphere/plane, sphere/box, box/plane (normal + depth) | REAL | `contact_pair.rs:291-383` |
| Contact solver changes motion + corrects penetration | REAL (dynamic-vs-static tested; dynamic-dynamic proven only by red team) | `contact_solver.rs:186,224`; `integration.rs:612` |
| Restitution (bounce vs no-bounce) | REAL | `contact_solver.rs:186,198`; `integration.rs:656` |
| **Dynamic↔dynamic momentum exchange** | REAL but **UNTESTED in suite** | red-team scenario 8 (`+1/-1 → -1.0/+1.0`); solver math `contact_solver.rs:90-96` |
| Friction | HONEST NO-OP | `contact_solver.rs:16-19` (documented; material friction stored, never read) |
| Angular / rotational dynamics | HONEST NO-OP | `integrator.rs` linear only; torque accumulated, never read |
| Colliders honor body rotation | NO (translation only) | `contact_pair.rs:237-239`, `collider_bounds.rs:5-8` (documented) |
| `box/box`, any capsule contact | NOT IMPLEMENTED (returns no contact) | `contact_pair.rs:50-67` (documented) |

The full pipeline is genuinely wired end-to-end
(`physics_world.rs:283-297`: broad → narrow → integrate velocities → solve →
integrate positions → correct penetration). Narrow-phase geometry is mathematically
correct (A→B normal convention, signed penetration depth, degenerate-config
rejection). The deliberate no-ops (friction, angular) are documented at their sites.
This is real linear rigid-body physics — **partial in scope, with one serious
unproven gap (dynamic-dynamic) and approximate queries (see §6).**

---

## 6. API-surface legitimacy — **PASS structurally / FAIL on honesty**

- **Every one of the 21 public `PhysicsApi` methods is directly tested.** No
  untested public method.
- **Facade is correctly sealed.** Exactly one behavioral type + the `ids`
  vocabulary; all rich return types (`PhysicsSnapshot`, `PhysicsStepRecord`,
  `PhysicsMaterial`, `PhysicsEvent`) are pub-in-private, returned by value,
  read-only. No mutable internal storage escapes.
- **Errors are deterministic typed values, never panics**, and `step` forces an
  explicit `RuntimeStep` (no internal clock).

Honesty defects:

- **MISLEADING — `raycast`/`overlap_sphere` facade docs conceal the approximation.**
  `raycast` tests finite shapes against their world AABB (`physics_query.rs:139-143`)
  and `overlap_sphere` against a `half_extents.length()` bounding sphere (≈1.732·r
  for a sphere) (`:158-171`). The module-level docs disclose this ("conservative …
  documented Phase-2 approximation", `:10-19`); the **facade method docs**
  (`physics_api.rs:187-204`) say only "nearest solid body hit" / "bodies overlapping
  a query sphere." An app author reading the public surface is not told these are
  conservative.
- **MISLEADING — `latest_step_record` doc is provably false** (`physics_step_record.rs:8-12`
  says counts are "always 0"; the module's own test asserts they equal `1`).
- **MINOR — typed error code is unreachable to external callers.** `PhysicsErrorCode`
  is not re-exported (Module Law #8), so an app gets an error it can only
  `.is_err()`/`Debug`-print, not `match` on. The "meaningful typed errors" guarantee
  is only half-delivered across the facade.
- **MINOR — `events()` returns a borrow of internal storage** (read-only, sealed
  `Copy` enum — acceptable, noted for consistency with the by-value `snapshot()`).

---

## 7. Test legitimacy — **74/100**

Strength: most assertions are on **real numbers** — normals, penetration depths,
exact velocities, settled heights, rebound velocities, nearest-hit handles,
tie-breaks. The suite is materially better than a compiles-only module.

Weaknesses:

- **Dynamic-dynamic momentum exchange is never tested** through `step()`. Every
  solver test uses `static_body(2)` (`inv_b = 0`), so `add_velocity(body_b, … · inv_b)`
  always adds zero — the inverse-mass split is executed for region coverage but its
  *behavior* is never observed. This is the suite's #1 hole.
- **Only 1 of 4 implemented contact pairings is validated end-to-end.** sphere/plane
  goes through `step()`; sphere/sphere, sphere/box, box/plane are exercised only on
  the bare generator functions, never through integrate→solve→correct.
- **Echo theater.** `assert_eq!(rec.solver_iteration_count(), 8)` at
  `integration.rs:283,652` and `physics_step_record.rs:154` asserts a *config
  constant*, not work performed — it would pass with an empty solver body.
- **Derive theater.** ~17 `derives_are_exercised`/`debug_is_exercised` tests exist to
  paint derived `Debug`/`Clone`/`PartialEq` regions for the Coverage Law; their
  `format!(…).contains("TypeName")` checks prove near-nothing about behavior.
- **Dead state.** `ContactManifold.point` is computed in every generator and
  asserted at generator level, then **consumed by nothing** (no accessor, no solver
  use) — it survives only because `#[derive(Debug)]` reads it.

Tests that would still pass if the solver / narrow phase / queries were replaced by
no-ops returning empty are enumerated in the Test Prosecutor's report (all the
negative/existence-only and `solver_iteration_count == 8` cases).

---

## 8. Documentation legitimacy — **FAIL (remediable, doc-only)**

The three Markdown files (`ARCHITECTURE.md`, `ROADMAP.md`, `TESTING.md`) are
**honest and complete**: every Phase-2 approximation on the audit checklist
(O(n²) broad phase, AABB/bounding-sphere queries, no friction, no box/box, no
capsule, no collision events, no angular, translation-only colliders, no byte
serialization) has a disclosing sentence and a roadmap phase, and the
"modeled after Unreal Chaos / Unity / Godot" phrasing is explicitly qualified and
disclaims feature parity (`ROADMAP.md:15-17`). No *undocumented* limitations were
found.

The failure is in the **in-source `///`/`//!` doc-comments**, which are a stale
Phase-1 layer now contradicted by the live Phase-2 code and its own passing tests:

| Stale claim (file:line) | Reality | Verdict |
|---|---|---|
| `lib.rs:27-32` "collision pipeline and queries exist as deterministic, empty scaffolds" | All real and tested | **FALSE** |
| `physics_step_result.rs:8-10` / `physics_step_record.rs:9-12,106,111` "counts always 0 / solver no-op" | Real counts; refuted by `integration.rs:641-652` (asserts `== 1`) | **FALSE** |
| `physics_material.rs:10-12` "have no dynamic effect" | Restitution *is* resolved (`contact_solver.rs:79-96`) | MISLEADING (true of friction, false of restitution) |
| `physics_collider.rs:10-13` "no broad phase, narrow phase, or contact solver yet" | All three exist | **FALSE** |
| `lib.rs:1` "(Phase 1)"; `physics_event.rs:9` "Phase 1 emits only lifecycle events"; angular comments | Label rot (some still factually true) | MISLEADING label |

Documentation that the code's own test suite refutes is a genuine honesty defect,
not cosmetic. It is doc-only to fix (no code change).

---

## 9. Error-and-validation legitimacy — **PASS on inputs / FAIL on state integrity**

**Input validation: PASS.** Every enumerated invalid input class is rejected with a
correct, machine-stable `PhysicsErrorCode`, and in every case the check runs
*before* state is stored:

- Scalars (mass/friction/restitution/density/radius/half-height) cross as kernel
  `Ratio`/`Meters`, whose only constructors reject NaN/±Inf (`ratio.rs:25`,
  `meters.rs:26`) — so non-finite scalars are un-constructable and never reach
  physics.
- The raw-`Vec3`/`Transform` doors (gravity, body transform, force/impulse, box
  extents, plane normal, query points) are each screened by a `*_is_finite` helper
  before store (`physics_body_desc.rs:15-29`, `physics_world.rs:219`,
  `physics_collider_shape.rs:69,112`, `physics_query.rs:87,113`).
- Capacity (+1), zero step, zero solver iterations, zero max_bodies/max_colliders,
  unknown handle, force/impulse on non-dynamic body — all rejected with the correct
  code and test (full table in the Error Prosecutor's report).

**Panic-safety: PASS.** No validation failure panics; bad query inputs degrade to
`None`/empty. The one reciprocal is clamped to stay finite.

**State integrity: FAIL (the determinism hole).** Validation screens input
*finiteness* but never *magnitude*, and integrator/accumulator *outputs* are stored
**without re-validation**:

- `integrate_velocity`/`integrate_position` (`integrator.rs:53,71`) `set_*` with no
  finiteness re-check.
- `ForceAccumulator::apply_*` (`force_accumulator.rs:32-38`) sums Vec3s; each addend
  is finite, the sum is not re-validated.
- `PhysicsConfig::new` checks gravity finiteness, not magnitude (`physics_config.rs:48`).

Reachable through *validated* inputs: two finite impulses near `f32::MAX` sum to
`+Inf`; a single ~`3.4e38` impulse overflows `translation` to `±Inf` within a few
steps; finite gravity `3.4e38` overflows every velocity in one step. A stored `NaN`
makes `snapshot() != snapshot()`, **breaking the replay invariant the module
advertises.** No test drives an extreme-but-finite value, so the gap is invisible to
the suite. Root-cause fix belongs at the integrator's *writes* and the accumulator's
*sum* (a finiteness/clamp guard), or as a bounded-vector primitive in `axiom-math` —
not as more input checks.

---

## 10. Performance-and-scaling legitimacy — **Acceptable Phase-2 + one HIGH risk**

**Documented and acceptable for Phase 2:**

- O(n²) AABB broad phase — admitted `broad_phase_pair.rs:6`, `ARCHITECTURE.md:156`,
  `ROADMAP.md:93`.
- Per-call snapshot/query Vec allocation — by-value, pull-based.
- Capacity limits enforced (`physics_world.rs:127,158`) and boundary-(+1)-tested
  (`integration.rs:428,439`).

**Hidden risks:**

- **HIGH — unbounded event log.** `StepCompleted` is pushed every step
  (`physics_world.rs:300-302`); the log is **never** drained/cleared (no `clear`/
  `drain` anywhere) and the facade exposes only `events() -> &[…]` with **no drain
  method**. At 60 Hz that is ~216k entries/hour growing forever, even with zero
  gameplay activity. Documented as a "monotonic event log" but never flagged as a
  growth risk.
- **MEDIUM — hidden O(iterations · manifolds · bodies).** Each `solve_contact` does
  linear `bodies.iter().find` ×4 + `colliders.iter().find` ×2
  (`contact_solver.rs:38-97`); `generate_contacts` does linear `find` ×4 per pair
  (`contact_pair.rs:227-231`). A handle→index map collapses all of these to O(1).
  Undocumented.
- **LOW-MEDIUM — 4 fresh Vecs/step, no pooling** (`physics_world.rs:277`,
  `broad_phase_pair.rs:81,105`, `contact_pair.rs:254`). Steady per-frame garbage,
  not admitted as a limitation.

The step record exposes useful per-step counts but no wall-clock duration (correct
for determinism) and no *total* event-log size accessor.

---

## 11. Engine-integration legitimacy — **PASS boundaries / PARTIAL capability**

**Ready:** body-pose rendering (per-body transform/velocity/kind/enabled in stable
order), external handle→scene-node mapping (public id vocabulary), fixed-step drive
from `axiom-runtime`, headless + WASM execution, deterministic replay, readable
material params. `PhysicsWorld` is `pub(crate)` and mutates only its own state — no
outward mutation, no scene/render/app import.

**Integration blockers (all additive facade exposures, none requiring a boundary
change; all listed as deferred in `ARCHITECTURE.md:324-340`):**

1. **Collider geometry is opaque.** `ColliderSnapshot::shape()` returns a
   `PhysicsColliderShape` whose accessors are all `pub(crate)` and whose
   `PhysicsShapeKind` is `pub(crate)` (`physics_collider_shape.rs:132-155`,
   `physics_shape_kind.rs:12`). An app receives the shape but **cannot read a single
   field** — cannot tell a sphere from a box, cannot get extents/radius/normal. Blocks
   collider rendering and debug-draw.
2. **No contact data contract.** `ContactManifold` is `pub(crate)`; the contact
   **point has no accessor anywhere**. Blocks contact visualization.
3. **No collision/trigger events.** `PhysicsEvent` has only five lifecycle variants;
   gameplay cannot react to contacts even though the solver resolves them.

---

## 12. Red-team findings

A temporary scratch integration test (since deleted; tree restored) drove the crate
through its public facade only. Observed results:

| # | Scenario | Result | Severity |
|---|---|---|---|
| 1 | NaN transform body | `Err` | OK |
| 2 | NaN force + Inf impulse | both `Err`; next step finite | OK |
| 3 | Exceed `max_bodies = 2` | 3rd `Err` | OK |
| 4 | Exceed `max_colliders = 2` | 3rd `Err` | OK |
| 5 | Disable then `apply_force` | `Ok` returned, force silently dropped (zeroed at integrate) | MINOR |
| 6 | Step 0 nanos | `Err`, `step_index` stays 0 | OK |
| 7 | **Enormous dt (10 s), sphere above plane** | **pos.y = −975**, vel finite, `contact_pairs = 0` — tunnels clean through | **CRITICAL** |
| 8 | **Dynamic-dynamic head-on, e = 1** | left/right `+1/−1 → −1.0/+1.0`, `contact_pairs = 1` — exact momentum exchange | OK (positive) |
| 9 | **Ray through sphere AABB corner, missing sphere** | `axis_dist 0.693 > radius 0.5` yet returns `Some` (hit) | **MAJOR** |
| 10 | 5 dynamic spheres stacked, 500 steps | Ys `[0.479,1.448,2.421,3.400,4.384]`, finite, stable but sag ~0.016–0.031 past slop | MINOR |
| 11 | Determinism: 2 fresh stack-drop runs | snapshot-equal `true` | OK |
| 12 | Creation-order independence of query output | sorted both ways | OK |

**Headline:** large-dt tunneling (scenario 7) is the genuine exploit — a discrete
narrow phase evaluated at step-start positions, a single integration step, and an
**inert `max_substeps`** config field (validated non-zero at `physics_config.rs`,
but **never read by `step_inner` and exposed by no accessor** — the one knob that
could mitigate tunneling does nothing) combine to let a fast/large-step body pass
through a solid plane undetected. Scenario 8 is the positive counter-finding: the
solver is a *real* two-body solver, not a static-only fake — the capability the
committed suite fails to test.

---

## 13. Overclaimed capabilities

1. **Query precision.** `raycast`/`overlap_sphere` named and documented (at the
   facade) as exact spatial queries; actually AABB / √3-inflated-bounding-sphere
   approximations for spheres/capsules → confirmed false positives.
2. **Collision pipeline status in `///` docs.** Multiple comments claim "empty
   scaffolds / counts always 0 / solver no-op" — refuted by passing tests.
3. **Material effect.** `physics_material.rs:10-12` claims materials "have no dynamic
   effect"; restitution genuinely affects the solve.
4. **`max_substeps`.** Presented as a configurable knob; validated then never
   consumed — substepping is unimplemented.
5. **"Byte-identically" / unqualified "deterministic."** Proven only as same-binary
   in-process replay; cross-platform f32 bit-determinism not scoped or proven.
6. **`solver_iteration_count` as work performed.** Reports the config constant (8)
   even with zero contacts; tests assert it as if it proved solving.

---

## 14. Proven capabilities

- Deterministic, ordered, monotonic-handle world with single-source fixed-step time.
- Same-binary replay equality over 1, 120, and 800 steps (snapshot + record).
- Gravity per body kind; force/impulse velocity changes; inverse-mass correctness.
- O(n²) AABB broad phase with sorted, insertion-order-independent output.
- Narrow-phase contacts for sphere/sphere, sphere/plane, sphere/box, box/plane with
  correct normal direction and signed penetration depth.
- Sequential-impulse solver with restitution and Baumgarte position correction:
  settling without tunnelling, elastic rebound, **and real dynamic-dynamic momentum
  exchange** (proven by red team).
- `raycast` (nearest, tie-broken, triggers excluded) and `overlap_sphere` (sorted,
  deduped, triggers included) — *exact for boxes, conservative for spheres/capsules*.
- Complete input validation with machine-stable typed error codes, no panics.
- Clean architecture: isolated module, single sealed facade, branchless, 100%
  covered, zero boundary leaks, headless + WASM-clean.

---

## 15. Missing capabilities

- **Tested** dynamic↔dynamic momentum exchange and unequal-mass impulse split
  (works, but no committed test).
- **End-to-end** contact response for sphere/sphere, sphere/box, box/plane through
  `step()` (only sphere/plane is validated end-to-end).
- Magnitude/overflow guard on stored state (determinism poison path, §9).
- Friction (tangential impulse) — Phase 3.
- Capsule contacts and box/box contacts — Phase 4.
- Collision/trigger lifecycle events — Phase 5.
- Angular/rotational dynamics, inertia, oriented colliders — Phase 6.
- Exact (non-AABB) sphere/capsule queries.
- Continuous collision / substepping to prevent large-dt tunnelling (and a live
  `max_substeps`).
- Public collider-geometry accessors, a contact/contact-point data contract, and an
  event-log drain — needed for full app integration.
- Cross-platform / byte-serialized replay proof.

---

## 16. Blockers

> Issues that leave a core claim false, unproven, or the advertised determinism
> breakable. These are what stand between the current state and a clean
> **LEGITIMATE FOUNDATION**.

1. **Documentation contradicted by its own passing tests** (§8). Stale `///`
   comments assert "empty scaffolds / counts always 0 / no contact solver / no
   dynamic effect" while the tests assert the opposite. A structural lie; doc-only
   to fix.
2. **Determinism poison path** (§9). Finite-but-extreme force/impulse/summed-impulse/
   gravity silently drives stored state to `Inf`/`NaN`, breaking replay equality.
   Untested. Fix at the integrator writes / accumulator sum.
3. **Central two-body solver claim unproven by the suite** (§5, §7). Dynamic-dynamic
   momentum exchange and three of four contact pairings have no end-to-end test,
   despite the behavior working.

---

## 17. Major issues

1. **Approximate queries behind exact-sounding names, undisclosed at the facade**
   (§6, red-team #9) — `raycast`/`overlap_sphere` report false positives.
2. **Large-dt tunnelling + inert `max_substeps`** (red-team #7) — fast/large-step
   bodies pass through solid geometry undetected; the only mitigating config knob is
   validated but never consumed.
3. **Unbounded event log** (§10) — `StepCompleted` pushed every step, never drained,
   no drain API; permanent memory growth.
4. **Integration capability blockers** (§11) — opaque collider geometry, no
   contact-point accessor, no collision events; an app can render body poses but not
   collider shapes, contacts, or gameplay reactions.

---

## 18. Minor issues

1. `solver_iteration_count` reports the config constant even with zero contacts;
   tests assert it as proof of solving (echo theater).
2. `ContactManifold.point` stored and generator-asserted but consumed by nothing
   (dead state).
3. Typed error code unreachable to external callers (sealed; only `.is_err()`).
4. Disabled-body `apply_force`/`apply_impulse` silently accepted (no error),
   asymmetric with static/kinematic rejection, undocumented.
5. ~17 derive/debug-exercising tests are coverage theater (execute regions, assert
   little).
6. Hidden O(n) `find` inside solver/narrow loops, undocumented (§10).
7. Per-step allocation of 4 fresh Vecs, no pooling, undocumented (§10).
8. "Byte-identically"/"deterministic" wording not scoped to same-binary replay (§4).
9. Stack sags ~0.016–0.031 past the 0.01 slop under load (expected for low-β
   frictionless sequential impulse; cosmetic).

---

## 19. Exact commands run

```sh
cargo xtask check-architecture        # baseline, before dispatching the audit team
cargo test --workspace                # baseline, before dispatching the audit team
cargo test -p axiom-physics           # run by the Physics, Error, and Red-Team prosecutors
cargo test -p axiom-physics --test _redteam_scratch -- --nocapture
                                      # red-team temporary scratch test (file since deleted)
```

The audit itself was read-only over `modules/axiom-physics/{src,tests}/`,
`module.toml`, `Cargo.toml`, the three Markdown docs, root `Cargo.toml`, and the
architecture rules in `CLAUDE.md` / `crates/xtask`. No production source or existing
test file was modified. The red team's single scratch test
(`tests/_redteam_scratch.rs`) was created, run, and deleted; the tree was restored.

---

## 20. Exact final status of `cargo xtask check-architecture`

```
OK: all layers satisfy the Axiom Layer Law.
```

Exit 0. The module passes the Layer Law, Module Law, and hygiene gates.

---

## 21. Exact final status of `cargo test --workspace`

```
suites: 166 | total passed: 3173 | total failed: 0
```

All workspace tests pass (this includes the `axiom-physics` inline, integration, and
architecture suites). The red team separately confirmed `axiom-physics`'s own suite
green (34 integration tests passing) before and after its scratch test.

---

## 22. Remediation plan (ordered by severity)

### Blockers — do first

1. **Reconcile the in-source docs with the code (doc-only).** Rewrite every stale
   `///`/`//!` comment that asserts Phase-1 absence:
   - `lib.rs:1-39` — retitle Phase 2; delete "collision pipeline and queries exist as
     deterministic, empty scaffolds"; list the *actual* deferred items (friction,
     capsule/box-box, angular, collision events).
   - `physics_step_result.rs:8-10`, `physics_step_record.rs:9-12,106,111` — delete
     "always 0 / no-op / no collision pipeline"; state the counts are real.
   - `physics_material.rs:10-12` — narrow to friction only; restitution *is* resolved.
   - `physics_collider.rs:10-13` — delete "no broad/narrow phase or solver yet."
   - `physics_event.rs:9`, the angular comments, and `integration.rs:257,280` —
     replace bare "Phase 1" with the specific deferring phase so labels stop rotting.

2. **Close the determinism poison path.** Add a finiteness/clamp guard at the
   integrator's writes (`set_linear_velocity`/`set_transform`, `integrator.rs:53,71`)
   and on `ForceAccumulator` summation (`force_accumulator.rs:32-38`), or introduce a
   bounded-finite vector primitive in `axiom-math`. Then add tests:
   `extreme_finite_impulse_does_not_poison_state`,
   `summed_impulses_cannot_overflow_to_non_finite`,
   `extreme_finite_gravity_keeps_state_finite_and_replayable`.

3. **Prove the two-body solver and the remaining contact pairings.** Add committed
   tests:
   - `dynamic_dynamic_momentum_exchange_through_step` (two dynamic spheres head-on;
     both velocities change; total momentum conserved).
   - `unequal_mass_dynamic_pair_splits_impulse_by_inverse_mass` (lighter body gains
     more Δv).
   - `solve_splits_impulse_between_two_dynamic_bodies_by_inverse_mass` (solver-unit
     twin asserting `inv_b > 0` actually moves body B).
   - `sphere_sphere_contact_response_through_step`, `sphere_box_..._through_step`,
     `box_plane_..._through_step` (the three untested-end-to-end pairings).
   - `two_dynamic_spheres_settle_into_a_resting_stack` (position correction split
     across two movable bodies).
   - `dynamic_dynamic_collision_replays_byte_identically`.

### Major — do next

4. **Disclose query approximation at the facade, or make queries exact.** Either add
   the conservative-bound caveat to the `raycast`/`overlap_sphere` doc-comments in
   `physics_api.rs:187-204` (cheap, honest), or implement exact ray-sphere /
   sphere-sphere tests in `physics_query.rs` (correct). Add a regression test for the
   AABB-corner false positive (`raycast_misses_a_sphere_it_only_clips_the_aabb_of`).

5. **Address large-dt tunnelling and the dead `max_substeps`.** Either wire
   `max_substeps` into `step_inner` (sub-step the integration so a large dt is split),
   or remove the field and document the fixed-step-only contract explicitly. Add
   `large_step_is_substepped_and_does_not_tunnel` or a documented
   `fixed_step_contract` test, plus an accessor if the field stays.

6. **Bound or drain the event log.** Add a facade `drain_events()` / `clear_events()`
   contract (or a ring buffer), so `StepCompleted` accumulation cannot grow without
   bound. Test `event_log_can_be_drained_and_does_not_grow_without_bound`.

7. **Open the integration surfaces (additive, when their phase lands).** Publish
   collider-geometry accessors on `ColliderSnapshot` (kind + extents/radius/normal),
   a neutral contact contract (collider/body pair + normal + depth + **point**)
   through the facade, and collision/trigger event variants — each with its own
   tests.

### Minor — opportunistic

8. Stop asserting `solver_iteration_count == 8` as proof of solving; assert a real
   solved delta instead, and surface the count only as metadata.
9. Remove the dead `ContactManifold.point` field *or* give it a public accessor and a
   consumer (it is needed by Phase 4/6 anyway).
10. Document the O(n) solver `find` and per-step allocation as known Phase-2 costs, or
    introduce a handle→index map to remove the `find`s.
11. Decide whether disabled-body force/impulse should error (symmetry with
    static/kinematic) and document/test the chosen behavior.
12. Scope "deterministic"/"byte-identically" wording to same-binary replay until a
    cross-platform or serialized-golden proof exists.

---

*Audit conducted read-only by ten adversarial prosecutorial subagents
(Architecture, Determinism, Physics Reality, API Surface, Test, Documentation,
Error/Validation, Performance/Scaling, Engine Integration, Red Team). No production
code or test was modified. Per the audit brief, production code is not to be changed
in response to this document unless explicitly requested as a separate follow-up
task.*

---

# Remediation result (appended — original verdict above is preserved)

A subsequent, explicitly-requested remediation pass resolved the findings above.
Full ledger: [`REMEDIATION.md`](REMEDIATION.md). This section records the outcome;
it does **not** rewrite the original audit or its verdict.

## Updated standing: **LEGITIMATE FOUNDATION**

The four overclaim counts that produced the original *PARTIALLY LEGITIMATE BUT
OVERCLAIMED* verdict are closed:

1. **Docs no longer lie.** Every stale Phase-1 claim was corrected to match the
   real pipeline, and `tests/doc_hygiene.rs` mechanically prevents the rot from
   returning.
2. **Queries are exact** for sphere/box/plane (capsule explicitly excluded, not
   approximated); the red-team AABB-corner false positive is now a passing
   regression test.
3. **The two-body solver and every implemented contact pairing are proven** through
   `step()` (dynamic↔dynamic momentum exchange, equal/unequal mass, restitution
   1/0, resting stack, all four pairings end-to-end).
4. **The determinism invariant is no longer silently breakable.** A step that would
   produce non-finite state is rejected atomically (no partial commit, world rolled
   back), so no snapshot can carry `NaN`/`±∞`.

Majors and minors: large-dt tunnelling fixed via live `max_substeps`; the event
log is now drainable; collider geometry and contacts are exposed as neutral
data; the solver/world hot-path `find` is now O(1); `solved_contact_count` makes
solver work honest; disabled-body force/impulse is rejected; typed errors are
inspectable via predicates/`raw_code`. Determinism is now documented precisely as
**same-binary replay**.

### Blockers resolved: 3 / 3 · Majors resolved: 4 / 4 (one partial) · Minors addressed: 8 / 8

The one **partial** is M4: collider-geometry and contact-data surfaces shipped, but
**collision/trigger lifecycle events remain deferred** — an honest, documented
deferral (ROADMAP Phase 5), not a faked API. Also still deferred (and documented):
friction, capsule/box-box contacts, angular dynamics, true CCD, a broad-phase
acceleration structure, and a cross-platform determinism proof. None of these were
ever claimed as present; documenting an unbuilt feature as future work is not an
overclaim.

## Laws held through remediation

- `cargo xtask check-architecture` → `OK: all layers satisfy the Axiom Layer Law.`
- `cargo test --workspace` → green (all suites pass; physics adds 57 integration
  tests + new unit tests).
- `scripts/coverage.ps1` → **100.00%** regions / lines / functions across the spine;
  every `axiom-physics` file at 100%.
- `cargo dylint --all` → no `engine_no_branching` and no
  `engine_no_unitless_float_public_api` findings on `axiom-physics` (Branchless Law
  and the naked-float ban hold).
- Single `PhysicsApi` facade preserved (Module Law #8); no lower layer modified;
  module stays headless / WASM-clean.
