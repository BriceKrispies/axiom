# Virtual Muscle — the active control layer

`axiom-physics` **simulates**; it does not **control**. A rigid body under gravity
with no active control is a ragdoll. The **`VirtualMuscleController`** in this
module is the active-control layer that turns authored *intent* into the
force/torque/target/constraint commands that keep the character upright and make
the motion read as a deliberate action rather than a collapse.

```text
authored objectives (axiom-animation-authoring)
  → VirtualMuscleController.command(profile, style, phase, objectives, body)
  → VirtualMuscleCommand { support mode, CoM, support target, per-group weight +
                           max_torque, plant strength, balance force, upright
                           torque, recovery damping, strike impulse }
  → applied to axiom-physics inside advance_muscled()
      · balance force  → apply_force on the dynamic pelvis
      · upright torque → apply_torque on the dynamic pelvis
      · kinematic limbs tracked to the authored pose (weighted by the groups)
      · plant hold / ball impulse as before
  → PhysicsApi.step → PhysicalAnimationFrame (carries the command; read via
                       the frame_muscle_* / frame_support_* / frame_balance_* accessors)
```

## The pieces

- **Muscle groups** (`muscle_group.rs`) — ten named groups (`core, pelvis, spine,
  neck_head, left_leg, right_leg, left_ankle, right_ankle, left_arm, right_arm`),
  each with `(stiffness, damping, max_torque, rest_weight)`. Addressed by a stable
  `u8` code so callers configure/read them through the facade without naming a
  private type (the module keeps its **one public facade**).
- **Profile + style + phase** (`muscle_profile.rs`) — `VirtualMuscleProfile` (the
  per-group base params), `MuscleStyle` (`muscle_strength`, `muscle_damping`,
  `balance_strength` gains), `SupportMode` (`both_feet / left_foot / right_foot /
  airborne`), and `MusclePhaseProfile` (the caller's per-tick support mode + group
  emphasis).
- **Controller + stages** (`virtual_muscle.rs`) — a pure `command(...)`:
  - **rest posture** — a baseline per-group stabilization weight that fades as the
    authored motor drive rises, so the character holds a stable posture when no
    strong authored action overrides it; plus the pelvis upright torque.
  - **balance** — a deterministic centre-of-mass (the mean of the bound body
    positions) and a horizontal correction force toward the support target
    (`both_mid / left / right / CoM-fallback`), scaled by `balance_strength`.
  - **foot plant** — a plant-hold strength that releases (→0) when the authored
    plant objective ends (so the plant softens after the strike).
  - **strike / follow-through / recovery shaping** — per-group `max_torque =
    base × muscle_strength × weight` and `recovery_damping = muscle_damping ×
    (1 − drive)`. The *policy* (which group is emphasized in which phase) is the
    caller's `MusclePhaseProfile`; the module only does the math.

## What is physically complete vs simplified

`axiom-physics` has **no joints or motors**, so a fully torque-driven articulated
ragdoll is impossible (an unconstrained dynamic limb would detach). Given that:

- **Real, dynamic control** acts only on the **pelvis** (the single dynamic body):
  the balance force and upright torque are genuine `apply_force`/`apply_torque`
  calls that keep the pelvis over its support and upright — this is what prevents
  ragdoll collapse.
- **The limbs stay kinematic**: their "joint motor targets" are the kinematic
  drive targets (the authored pose). The muscle group weights/`max_torque` are
  computed, reported, and available to shape those targets, but the limbs are not
  torque-driven joints.
- **Centre of mass** is an unweighted mean of body positions (stable + tested),
  not mass-weighted.
- **Balance** is a horizontal PD toward the support target + a pelvis upright
  torque — enough to hold the character over its support, not a full
  inverted-pendulum stabilizer.

Everything is deterministic (no wall-clock, no randomness): identical inputs →
identical commands and identical simulated frames. `advance` (muscle-free) and
`advance_muscled` are separate paths; the muscle command is `None` on the former.
The soccer per-phase policy that drives this lives in the app
(`apps/axiom-gallery/src/soccer_penalty/penalty_muscle.rs`), not in this module.
