# `axiom-animation-authoring` — Testing

The module is held to the engine spine's **100% coverage** invariant. Every public
facade method and every internal region is exercised by a test. Tests come in two
layers: per-file `#[cfg(test)]` unit tests (one module = one concern), and the
public-API integration slice at `tests/penalty_kick_slice.rs`.

## Deterministic sampling tests

- **Replay/debug identity** — `MotionSampler` samples are a pure function of
  `(plan, tick)`. `sampling_is_replayable` (unit) and
  `the_same_plan_sampled_twice_at_the_same_tick_is_identical` (slice) assert two
  samples at the same tick are `PartialEq`-equal *and* `Debug`-equal. There is no
  wall-clock time, no randomness, and no console output anywhere in the module, so
  a `PoseFrame` is fully reproducible across runs.
- **Forward kinematics + goal application** — `every_goal_applier_runs_and_shifts_
  the_pose_off_bind` drives one of every pose-goal kind through the sampler and
  checks the posed joint locals differ from the bind pose; the ease/strength math
  is unit-tested in `motion_phase.rs`.
- **Root motion** — `root_moves_toward_the_target_over_a_move_phase` and
  `a_tick_in_no_phase_holds_the_carried_root_and_empty_records` cover the
  move/hold/settle root fold and the no-active-phase fallback.
- **Pins** — `a_pin_constraint_and_a_contact_override_their_effector_worlds`
  confirms both a pin constraint and a contact override their effector's world
  position to the target.

## Validation-failure tests

`motion_compiler.rs` exercises every rejection path with its exact
`AuthoringErrorCode`:

- `InvalidTickRange` — empty/inverted phase spans, phase ends past the duration,
  events at or beyond the duration.
- `OverlappingPhases` — any pair of overlapping phase spans.
- `NonFiniteValue` — a non-finite target position, style scalar, goal amount,
  layer weight, or event power.
- `UnknownJoint` / `UnknownEffector` / `UnknownTarget` — a goal, constraint,
  contact, root motion, or event referencing a name absent from the rig or the
  motion's targets. Side-driven joint goals (raise-arm / leg / torso) are compiled
  against a *deficient* rig to reach their internal joint-lookup error arms.

The facade's own missing-id paths (`RigNotFound`, `MotionNotFound`,
`PhaseNotFound`, `PlanNotFound`) are covered in `authoring_api.rs`. The slice test
re-verifies the headline rejections (unknown joint/effector/target, invalid range,
overlap, non-finite) through the **public** API, asserting `compile(...).is_err()`
(the error *type* is internal, so its code is checked only in the unit tests).

## Penalty-kick vertical-slice tests

`tests/penalty_kick_slice.rs` drives `soccer_penalty_kick_v0` through the public
facade and asserts every authored property:

- the standard humanoid hierarchy is valid (25 joints, 8 effectors);
- the **approach** phase moves the root toward the ball;
- the **plant** phase pins `left_foot_sole` exactly at `left_plant_spot`;
- the **backswing** places the right foot behind the body relative to the strike
  direction (`+Z`);
- the **strike** emits exactly one `ball_contact`, and it fires within the strike
  phase span;
- the **follow-through** moves the right foot past the ball toward the net;
- `style.power` changes the emitted `ball_contact` power deterministically (a low
  power yields a strictly smaller emitted power than a high one).

`penalty_kick.rs` additionally pins the exact strike tick and power internally.

## Replay / debug expectations

- A `PoseFrame` derives `Debug` + `PartialEq`; equality is structural over the
  root, joint locals, effector worlds, active constraints, active contacts, and
  events. Two runs of the same compiled plan at the same tick are byte-for-byte
  reproducible.
- Errors carry a stable `AuthoringErrorCode` (`raw()` → a fixed `u16`) so replay
  logs and assertions can pin *which* failure occurred without depending on the
  human-readable message; error identity is the code alone (message excluded).

## Running

```sh
cargo test -p axiom-animation-authoring        # unit + integration slice
cargo xtask check-architecture                 # Layer + Module Law
bash scripts/dylint-gate.sh                    # Branchless Law
bash scripts/coverage.sh                        # 100% coverage gate
```

## Physical-objective coverage

`physical_objective.rs` and the facade's `objective_*` / `plan_*` /
`frame_joint_world` readers are covered through the public facade against the
built-in penalty kick: the approach yields a `+Z` root velocity (and `Hold` yields
`None`); the plant pins the left foot at `left_plant_spot`; `strike` motor drive
exceeds `backswing`; the `ball_contact` tick yields a unit `+Z` impulse scaled by
power (with a degenerate coincident-target case exercising the direction math); and
missing-plan errors propagate. These prove the neutral control data the physics
bridge consumes without any physics type entering this module.
