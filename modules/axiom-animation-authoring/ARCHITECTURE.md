# `axiom-animation-authoring` — Architecture

## What this module owns

A small, deterministic, **inspectable authoring vocabulary** for humanoid motion,
and the compiler + sampler that turn authored intent into replayable pose frames:

```text
HumanoidRigSpec                      (a named joint hierarchy + named effectors)
  +
MotionSpec                           (name, duration, rig, targets, style scalars,
  = ordered phases                    ordered phases, ordered events)
      each phase carries:
        - a root-motion command
        - ordered pose goals
        - ordered constraints
        - ordered contact declarations
        - an ease curve + layer weight
  + ordered events (named / ball_contact)
  ── MotionCompiler validates + resolves names ──▶  MotionPlan   (sample-ready)
  ── MotionSampler at a Tick ──────────────────▶  PoseFrame     (root + joint
                                                                  locals + effector
                                                                  worlds + active
                                                                  constraints +
                                                                  active contacts +
                                                                  emitted events)
```

Everything is a pure function of the authored data and the sampled tick: **no
wall-clock time, no randomness, no console output.** Sampling the same plan at the
same tick yields a `PartialEq`-equal, `Debug`-equal frame — the replay/debug
contract.

### The vocabulary

- **Rig** — `HumanoidRigSpec::standard_humanoid()` builds a PS1/low-poly athlete:
  a spine chain (`root → pelvis → spine_lower → spine_upper → chest → neck →
  head`), two arms (`shoulder → upper_arm → forearm → hand`) off the chest, two
  legs (`hip → thigh → shin → foot → toe`) off the pelvis, plus the eight named
  effectors (`left/right_foot_sole`, `right_foot_instep`, `left/right_hand`,
  `head_gaze`, `chest_forward`, `pelvis_forward`).
- **Pose goals** — `set_joint_rotation`, `aim_effector_at_target`,
  `move_effector_toward_target`, `raise_arm_for_balance`,
  `torso_twist_toward_target`, `leg_backswing`, `leg_strike`, `follow_through`.
- **Constraints** — `pin_effector_to_target`, `keep_gaze_on_target`,
  `keep_center_of_mass_over_support`, `orient_surface_toward_target`,
  `preserve_foot_contact`. A *pinning* constraint (pin / preserve) overrides its
  effector's world position to its target; the others are recorded as active in
  the frame for a consumer to honor.
- **Contacts** — a planted effector (also pins its effector to the target).
- **Events** — a `named` cue, or a `ball_contact` carrying contact surface, aim
  target, direction target, and power.

### The built-in penalty kick

`soccer_penalty_kick_v0(power)` is a worked example authored *entirely* with the
vocabulary above — six phases (`approach`, `plant`, `backswing`, `strike`,
`follow_through`, `recover`), four targets (`ball`, `net_center`,
`left_plant_spot`, `approach_start`), and a `ball_contact` at the strike tick. It
introduces **no** new engine concepts; a game or editor authors its own motions
the same way.

## What this module does NOT own

Not rendering, physics, input, assets, editor UI, browser/GPU APIs, or
game-specific app orchestration. It imports none of them. Turning a `PoseFrame`
into scene nodes, draw commands, or skinned vertices is an **app**'s job.

## Package classification & dependencies

An **isolated engine module** (`module.toml`, `kind = "engine-module"`,
`allowed_modules = []`). Engine modules may not depend on one another, so this
module consumes **no** scene/skeleton module and owns its output as neutral data.
It depends only on two layers:

- `axiom-kernel` — `Tick` (sample time), `Ratio` (finite scalars: power, amounts,
  layer weight), and the deterministic error pattern.
- `axiom-math` — `Vec3` / `Quat` / `Transform`, `Quat::from_euler_xyz`, and
  `Scalar::validate_finite` (the non-finite rejection path).

It notably does **not** depend on the sibling `axiom-animation` module (skeletal
mechanics): engine modules cannot depend on each other. If the two should ever
share a primitive (e.g. a common transform-hierarchy type), that primitive belongs
in a lower **layer**, not a cross-module dependency.

## Public surface (Module Law: one facade)

`lib.rs` exports exactly one behavioral facade, **`AnimationAuthoringApi`**, plus
its id vocabulary (`RigId`, `MotionId`, `PhaseId`, `PlanId`, `JointId`,
`EffectorId`, `TargetId`). Every other type — the rig spec, the motion spec,
phases, pose goals, constraints, events, the plan, the pose frame, and the error
types — is **internal**, reached only through the facade. The vocabulary therefore
lives behind per-variant facade methods (`add_leg_backswing`,
`add_pin_effector_to_target`, `add_ball_contact`, …), not as public enums — the
same shape by which the sibling `axiom-animation` module keeps `Pose`/`Skeleton`
private behind `AnimationApi`.

Scalars cross the boundary as value types (`Tick`, `Ratio`, `Vec3`, `Transform`),
never a naked `f32`. Non-finite values can only enter through `Vec3`/`Transform`
components, which the compiler rejects with `NonFiniteValue`.

## Determinism & branchlessness

All non-test code is **branchless** (the engine spine invariant): validation is
expressed as `.all`/`.find`/`try_fold`/`collect::<Result>` combinator chains;
per-kind resolution and pose-goal application are dispatched through `const`
fn-pointer tables indexed by the goal/event discriminant (a table lookup, never a
`match`); conditional selection uses table indices and `then_some`/`then`.

## How a future editor authors `MotionSpec` data

An editor drives the same facade: `standard_humanoid()` → `create_motion(...)` →
`add_target(...)` / `set_style(...)` → `add_phase(...)` and the per-phase
`set_phase_*` / `add_*` goal/constraint/contact methods → `add_named_event` /
`add_ball_contact`. It can read authored data back with the inspection readers
(`motion_name`, `motion_phase_names`, `motion_style`) and, once compiled,
`plan_duration` / `plan_event_count`. Nothing about the authoring path assumes a
particular UI — the `PhaseId` a phase hands back is the only handle an editor needs
to attach goals to it.

## How a future app translates `PoseFrame` into scene/render data

An app compiles a motion (`compile`) once and samples it per tick (`sample`), then
reads the frame through the `frame_*` accessors, all of which return **nameable**
math/id/primitive types:

- `frame_root(&frame) -> Transform` — the world root.
- `frame_joint_local(&frame, JointId) -> Option<Transform>` — a joint's local (map
  names to ids with `joint_id(rig, name)`).
- `frame_effector_world(&frame, EffectorId) -> Option<Transform>` — an effector's
  world transform (map names with `effector_id(rig, name)`).
- `frame_event_names` / `frame_ball_contact` — the events emitted this tick.
- `frame_active_constraint_count` / `frame_active_contact_count`.

The app composes joint locals down its own scene hierarchy (or hands the effector
worlds to an IK/attachment system) and turns events into gameplay — the module
itself writes into no scene.

## The physics-backed path: physical objectives (the second execution path)

The `frame_*` readers above are the module's **pose path** — kinematic
`PoseFrame`s an app composes directly. Alongside them, the facade emits a second,
neutral view of a compiled motion: **physical objectives**. These are the control
data a *physics bridge* (the `axiom-physical-animation` feature module) translates
into a real rigid-body simulation. The authoring module owns the **vocabulary and
the intent**; it contains no solver, no bodies, and no physics types — every
objective is a plain math/id/`Ratio` value.

Each objective is a pure, deterministic function of the compiled plan and a tick,
exposed on the existing `PlanId`:

- `active_phase_name(plan, tick) -> Option<String>` — the phase driving this tick.
- `objective_root_velocity(plan, tick) -> Option<Vec3>` — a `MoveToward` phase's
  per-tick root velocity (the approach; `Hold`/`Settle` yield `None`).
- `objective_foot_plant(plan, tick) -> Option<(EffectorId, Vec3)>` — the pinned
  effector and its world target (a contact declaration, else a pinning constraint).
- `objective_joint_motors(plan, tick) -> Vec<(JointId, Vec3, Ratio)>` — per driven
  joint, its authored Euler target and the phase's **drive** (its layer weight), so
  a `strike` outdrives a `backswing`.
- `objective_ball_impulse(plan, tick) -> Option<(EffectorId, Vec3, Ratio)>` — at a
  `ball_contact`: the contact-surface effector, the unit direction from the aim
  target toward the direction target, and the magnitude (event power).
- `objective_gaze(plan, tick) -> Option<Vec3>` — a `keep_gaze` target.

Supporting readers a bridge needs to bind a rig and place a ball —
`plan_target_position(plan, name)`, `plan_joint_id(plan, name)`,
`plan_effector_id(plan, name)`, and `frame_joint_world(&frame, JointId)` (the
composed FK world transform, used to drive a kinematic physics body at a joint) —
round out the surface. `objective_*` are computed in `physical_objective.rs`
(internal) and exposed through one dedicated `impl AnimationAuthoringApi` block.

**Why here and not in a physics module:** two modules can never share a Rust type,
so authoring publishes intent as neutral values and the composition tier (a feature
module) reads them and calls the physics facade. Authoring never depends on physics.
