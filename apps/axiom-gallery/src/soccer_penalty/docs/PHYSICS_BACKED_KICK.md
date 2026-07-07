# Physics-backed penalty kick — EXPERIMENTAL (disabled by default)

> **This documents the experimental full-humanoid kicker, which is NOT the default.**
> The default kicker is the authored/kinematic pose — see
> [`KICK_ANIMATION.md`](KICK_ANIMATION.md). The path below is compiled in only with
> `--features experimental_physical_humanoid_kicker` and, as shipped, renders a broken
> (inverted / orphan-capsule) kicker because it poses the figure's root box from a
> free-integrating dynamic pelvis body. See *Why the physics humanoid is disabled* in
> `KICK_ANIMATION.md` for the root cause and the invariants required before it can
> become the default again. It is kept, behind the feature, so its binding can be
> proven correct later; its `axiom-physical-animation` bridge keeps its own always-on,
> fully-covered module tests.

The experimental kicker animation and ball flight run on the engine's
**procedural-animation authoring** system, executed through the **physics**
module. This replaces the old ad-hoc path (a baked `kick_right.clip` posed by a
frame index, and a closed-form parametric ball arc).

## The runtime pipeline

```text
game state (aim + power)
  → SoccerPenaltyKickMotionSpec        (penalty_kick_motion — authored, 9 phases)
  → compile → MotionPlan
  → PhysicalAnimationApi.advance()      (penalty_physics_kick, via axiom-physical-animation)
  → PhysicalAnimationFrame per tick     (physics body transforms + ball state + objectives)
  → PhysicalKickFrame (captured)        (penalty_physics_kick)
  → the visible kicker boxes            (penalty_kicker: figure geometry + physics transforms)
  → a real ball impulse at the strike   (penalty_ball: axiom-physics projectile to the aimed target)
  → visible kicker sprinting, planting, twisting, striking, following through, recovering
```

Composition lives in the **app** (the composition root): the app owns the
`AnimationAuthoringApi` (authoring) and the `PhysicalAnimationApi` (bridge) and
wires the two module facades. No soccer logic lives in `axiom-physics`; no
rendering lives in `axiom-animation-authoring`.

## The nine authored phases → physics objectives

Each phase is authored as data (`penalty_kick_motion::build_kick`) and realised by
the bridge as physics objectives:

| Phase | Authored objectives | Physics realisation |
|---|---|---|
| `setup` | gaze on ball, hold | kinematic hold, gaze objective |
| `sprint_approach` | root `move_toward(approach_start → ball)`, torso lean, arm pump, gaze | **force on the dynamic pelvis body** toward the ball (root-velocity objective) |
| `pre_plant` | settle root, pelvis turn, arms spread, right-leg prep | kinematic limb drive |
| `plant` | contact + pin `left_foot_sole` at `left_plant_spot`, COM over support, arms wide | **left-foot plant objective** (kinematic hold at the plant spot) |
| `backswing` | `leg_backswing(right)`, instep aim behind, chest counter-rotate, arms balance, preserve plant | kinematic limbs; drive weight **below** hip_drive |
| `hip_drive` | right-hip forward rotation, torso rotate-through, arms counter, preserve plant | kinematic limbs; drive weight **above** backswing |
| `strike` | `leg_strike(right, ball)`, instep aim + orient at ball/net, torso twist, gaze, preserve plant | **`ball_contact` event → real `apply_impulse` on the ball**, direction `net_center − ball`, magnitude ← `power` |
| `follow_through` | right leg past ball toward net, torso continue, arm reach, plant released | kinematic limbs; no plant pin |
| `recover` | settle, right foot returns, gaze to net | softest drive weight |

The per-phase **layer weight** is the physical *drive*: `hip_drive` (~0.9) drives
harder than `backswing` (~0.5), and `recover` (~0.3) is softest — so the strike
reads as a real kick, not a uniform sweep.

## What is physics-backed vs pose-authored (honest scope)

`axiom-physics` has rigid bodies, forces, impulses, torques and kinematic bodies,
but **no joints or motors** — so a fully active ragdoll is out of scope. The kick
is the sanctioned **hybrid** (see `axiom-physical-animation/ARCHITECTURE.md`):

- **Physics-driven (dynamic):** the **ball** (a real strike impulse, then gravity;
  never teleported) and the **pelvis/root** (a dynamic body force-driven up the
  run-up).
- **Pose-authored (kinematic):** the **limbs** and the **planted foot** are
  kinematic bodies driven from the authored pose each fixed step. "Joint motors"
  are kinematic drives; the "plant" is a kinematic hold — the closest available
  public physics mechanisms.

The **game ball** is launched as its own `axiom-physics` projectile
(`penalty_ball`) aimed at the player's chosen corner, so it honours the aim; the
bridge's own ball proves the strike is a real impulse. Both go through
`axiom-physics`.

## Tuning the kick style

`SoccerPenaltyKickStyle` carries nine `[0,1]` scalars, each threaded into the
authored magnitudes / drive weights / impulse:

`power`, `urgency`, `runup_speed`, `plant_stability`, `torso_twist`,
`arm_balance`, `backswing_amount`, `follow_through_amount`, `recovery_settle`.

- `SoccerPenaltyKickStyle::default_style()` is the balanced kick.
- `SoccerPenaltyKickStyle::from_power(0..=100)` maps the game's power meter (power
  itself + a gentle rise in urgency / run-up / follow-through).
- `power` sets both the authored `ball_contact` power and the physics strike
  impulse; higher `power` → measurably faster ball
  (`PenaltyPhysicsKick::strike_launch_speed`).

## Inspecting fixed ticks (debug)

`PenaltyPhysicsKick::debug_snapshot()` returns one
`PhysicalKickPhaseSnapshot` per phase at its representative tick
(`penalty_kick_motion::PHASE_SAMPLE_TICKS`): the active phase, the motor drive,
whether the foot is planted / sprinting / striking, and the pelvis / right-foot /
ball-velocity read — the kick can be inspected tick-by-tick without a renderer.
`PenaltyPhysicsKick::frame(tick)` gives the full captured frame for any tick.

## Determinism

The bridge and the ball projectile are same-binary deterministic; the whole kick
is a pure function of its style, simulated once and cached. Identical style →
identical captured poses and ball state (proven in
`tests/soccer_penalty_physics_kick.rs`).

## Active control — the virtual-muscle layer (staying upright)

The kicker no longer relies on an ad-hoc anti-gravity hold to stay up: it runs on
the engine's **`VirtualMuscleController`** (in `axiom-physical-animation`), driven
by the soccer muscle **policy** in `penalty_muscle.rs`.

- **Engine = mechanism, app = policy.** The engine owns muscle groups, the PD
  balance controller, the centre-of-mass estimate, and the plant/recovery math.
  `penalty_muscle::phase_profile_for(phase)` is the soccer policy — one row per
  phase giving the **support mode** and per-group **weight** (the task's
  StrikePreparation / StrikeDrive / FollowThrough / Recovery controllers, as
  data). `penalty_physics_kick` configures the engine (`set_muscle_profile` /
  `set_muscle_style`) and advances each tick through `advance_muscled` with that
  phase's policy.
- **Per-phase support + emphasis:** setup/sprint/pre_plant → both feet; plant /
  backswing / hip_drive / strike → **left foot**, with the left leg+ankle+core
  stabilizing and the pelvis+right-leg drive rising into the strike (hip_drive >
  backswing); follow_through → both feet as weight leaves the plant (plant hold
  releases); recover → both feet, low drive, rest posture restored.
- **How it keeps the kicker up:** the balance controller estimates the CoM from
  the physics bodies and applies a real `apply_force` on the dynamic pelvis toward
  the support target, plus an `apply_torque` upright correction — so the pelvis
  tracks its support instead of drifting.

### Tuning the muscle style

`SoccerPenaltyKickStyle` gained three scalars, mapped into the engine `MuscleStyle`:

- `muscle_strength` — scales every group's `max_torque` (harder-held pose).
- `balance_strength` — scales the pelvis balance-correction force.
- `muscle_damping` — scales the recovery/settling damping.

### Inspecting fixed ticks

`PenaltyPhysicsKick::debug_snapshot()` reports, per phase tick: the active phase,
support mode, centre of mass, support target, motor drive, plant strength,
recovery damping, the major group weights (pelvis / core / left-leg / right-leg),
and any strike impulse — the active control is inspectable without a renderer.
