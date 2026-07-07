# `axiom-physical-animation` — Architecture

A **feature module** (`kind = "feature-module"`): the composition tier that bridges
the deterministic **animation-authoring** vocabulary to the real **axiom-physics**
engine. It binds a humanoid rig to physics bodies, translates a compiled motion's
physical objectives into physics forces / impulses / kinematic drives, steps the
world deterministically, and reads back `PhysicalAnimationFrame`s.

```text
MotionPlan (axiom-animation-authoring)
  -> physical objectives  (root velocity / foot plant / joint motors / ball impulse / gaze)
  -> HumanoidPhysicsBinding  (rig joints -> axiom-physics bodies)
  -> apply to axiom-physics  (apply_force / apply_impulse / set_body_transform)
  -> PhysicsApi::step  (deterministic fixed RuntimeStep)
  -> PhysicsApi::snapshot -> PhysicalAnimationFrame
```

## Placement under the Axiom laws

- **Feature module**, the sanctioned exception to "modules never depend on
  modules": `allowed_modules = ["animation-authoring", "physics"]`, over the
  `kernel`, `math`, and `runtime` layers. Verified by `cargo xtask
  check-architecture`.
- **One facade.** `lib.rs` exports exactly `PhysicalAnimationApi`. The binding,
  objectives, frame, and errors are reached only through it (`PhysicalAnimationFrame`
  is `pub` but not re-exported — callers hold it by inference and read it via the
  `frame_*` accessors, exactly as authoring's `PoseFrame` works).
- **Branchless / 100% covered / deterministic**, like every spine crate.
- **Owns no simulation and no authoring.** `axiom-physics` owns every body,
  collider, contact, and step; `axiom-animation-authoring` owns the vocabulary. This
  module only *translates*. It references no physics-private type by name — the
  physics facade's `PhysicsMaterial` / result types are obtained and used by
  inference, and every physics/authoring failure is funneled through the generic
  `phys` / `auth` helpers into a `PhysicalError`.

## The hybrid, and why (the engine's real capabilities)

`axiom-physics` has dynamic/kinematic/static bodies, forces, impulses, torques,
colliders, contacts, gravity, and a fixed `RuntimeStep` — but **no joints and no
motors**, and building them is explicitly out of scope ("do not build a physics
engine"). So an articulated, motor-driven ragdoll is impossible; the honest,
deterministic realization is a **hybrid**:

| Element | Physics kind | How it is driven |
|---|---|---|
| Soccer **ball** | dynamic sphere | a **real `apply_impulse`** at the strike, then flies under gravity — **never teleported** |
| **Pelvis / root** | dynamic body | an anti-gravity hold + an approach **`apply_force`** toward the ball |
| **Limbs** (chest, head, arms, legs) | kinematic bodies | `set_body_transform` from the authored pose each step |
| **Planted foot** | kinematic body | a kinematic **hold** at the plant target |

A "joint-motor rotation target" becomes a kinematic drive and a "foot plant" a
kinematic hold — the closest public mechanisms given no joints/constraints. The
carried *drive scalar* (a phase's layer weight) still orders the phases (`strike` >
`backswing` > `recover`) and is reported per frame. Humanoid colliders are triggers,
so they never solver-collide with the ball: the ball's motion is purely its impulse
+ gravity, which keeps the whole slice deterministic and legible. This hybrid is a
documented consequence of the engine's real capabilities — not a shortcut, and the
ball is genuinely physics-driven.

## No new `axiom-physics` API

The physics facade already exposes impulse, force, torque, kinematic bodies,
snapshots, contacts, and step records — everything the bridge needs. **No physics
public API was added.** If a future articulated path is wanted, that is a physics
capability (joints/motors) added *in* `axiom-physics` with its own tests, not faked
here.

## Files

- `physical_animation_api.rs` — the `PhysicalAnimationApi` facade + controller:
  owns the `PhysicsApi` world, the binding, and the ball; `new` / `bind_standard_humanoid`
  / `attach_ball` / `advance`, plus the `frame_*` readers. The per-tick `step_once`
  inlines all pose-frame work (the pose type is not nameable), applying objectives
  as iterator chains and `Option::into_iter().try_for_each` conditional side effects.
- `humanoid_binding.rs` — `HumanoidPhysicsBinding` + the deterministic builder: one
  physics body per bound joint (pelvis dynamic, the rest kinematic), the foot-effector
  map, and the ball body. Body kind is chosen by a `fn`-pointer table, not a branch.
- `physical_frame.rs` — `PhysicalAnimationFrame`: body/effector transforms, active
  phase, the applied objectives, contact count, events, physics step index, and ball
  state. Assembled from a `FrameParts` bundle.
- `physical_error{,_code}.rs`, `physical_result.rs` — the deterministic error trio
  and the `phys` / `auth` funnels.

## What this module does not do

It renders nothing, hosts nothing, and knows no soccer rules — it exposes a physical
frame; an app reads it. The penalty kick is a **vertical slice** proving the path,
not engine law.

## The virtual-muscle active-control layer

`axiom-physics` simulates but does not *control* — a body under gravity with no
active control is a ragdoll. The **`VirtualMuscleController`** (`virtual_muscle.rs`,
`muscle_group.rs`, `muscle_profile.rs`) is the deterministic active-control layer
that turns authored objectives + style into balance / upright / plant / recovery
commands, applied to the dynamic pelvis inside `advance_muscled`. It is generic
(no soccer concepts): the caller supplies a per-tick support mode + per-group
weights via primitive facade args (`set_muscle_profile`, `set_muscle_style`,
`advance_muscled(support_mode: u8, group_phase_weights: [Ratio; 10])`) and reads
the result via the `frame_muscle_*` / `frame_support_*` / `frame_balance_*` /
`frame_center_of_mass` accessors. The muscle types stay private (the module keeps
its single public facade). See **`MUSCLE.md`** for the full design and the
simplifications (limbs kinematic → real muscle force only on the pelvis).
