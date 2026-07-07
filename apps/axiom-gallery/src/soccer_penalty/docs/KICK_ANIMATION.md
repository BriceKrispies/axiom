# Penalty kick animation — authored/kinematic kicker + physics ball (default)

The soccer penalty game's kick is a **hybrid**: the kicker is a deterministic
**authored / kinematic** animation, and the ball is a real **`axiom-physics`**
projectile. This is the stable default. It replaced an earlier physics-backed
*full-humanoid* path that rendered a broken, inverted kicker (see
[Why the physics humanoid is disabled](#why-the-physics-humanoid-is-disabled-experimental)).

## The runtime pipeline

```text
game state (aim + power)
  → SoccerPenaltyKickMotionSpec        (penalty_kick_motion — authored, 9 phases)
  → compile → MotionPlan
  → SoccerPenaltyKickPose              (penalty_kick_pose — pure forward kinematics)
       AnimationAuthoringApi.sample(plan, tick)  →  frame_joint_world(joint) × 13
  → KinematicKickFrame per tick        (13 joint world transforms, no physics body)
  → the visible kicker boxes           (penalty_kicker: figure geometry + kinematic transforms)
  → a real ball impulse at the strike  (penalty_ball: axiom-physics projectile to the aimed target)
  → visible kicker sprinting, planting, twisting, striking, following through, recovering
```

Composition lives in the **app** (the composition root): the app owns the
`AnimationAuthoringApi` and reads its `frame_*` accessors to pose its own scene. No
soccer logic lives in `axiom-animation-authoring` or `axiom-physics`.

**No physics body is ever a render transform.** Every one of the 13 kicker joints
comes straight from the authored pose evaluated by forward kinematics, so the whole
figure is coherent, upright, and readable. That is the fix.

## The kicker — authored/kinematic

`SoccerPenaltyKickPose::simulate(style)` authors the nine-phase kick and, for each
tick `0..DURATION`, samples the compiled plan and reads the 13 joint world
transforms (`pelvis, chest, head, left/right thigh/shin/foot, left/right
upper_arm/forearm`) into a `KinematicKickFrame`. The whole kick is aim-independent,
so it is simulated once and cached; the game samples it by authored tick.

The nine phases (`penalty_kick_motion::build_kick`), in order:
`setup → sprint_approach → pre_plant → plant → backswing → hip_drive → strike →
follow_through → recover`. Each is authored as data — targets, root motion, pose
goals, constraints, contacts, and a `ball_contact` event — and evaluated purely by
FK. The per-phase drive still reads as a real kick because the authored magnitudes
(backswing, hip_drive, strike, follow-through) shape the pose.

### Coordinate conventions

- **Authored POSE space** (`KinematicKickFrame::root`/`joints`): `+Z` is forward,
  toward the goal; the run-up interpolates from `approach_start` (`−Z`) up to the ball
  at the origin.
- **WORLD/game space** (`penalty_scene` / `penalty_ball`): the goal line is `z = 0`
  and the kicker/ball sit at `+Z`, so *toward the goal* is **decreasing z**. The
  kicker rig maps pose→world by flipping z (`world_z = KICKER_Z − pose_z`).

## The ball — real axiom-physics

The ball is a real `axiom-physics` dynamic body (`penalty_ball`). On the tick after a
shot locks it is launched from the penalty spot by a **real impulse** and integrated
under gravity to the player's aimed goal-plane target — never teleported; every
interior sample is a physics-integrated position (`PenaltyBallTrajectory`, calibrated
through the integrator so it lands exactly on the aim). The authored **`ball_contact`
event at `STRIKE_CONTACT_TICK`** is what bridges the kicker's strike to that ball
launch; strike power comes from `style.power`, strike direction from the authored
`net_center − ball`. After the strike the ball's visual position comes from the
physics-captured path.

## Inspecting fixed ticks (debug)

`SoccerPenaltyKickPose::debug_snapshot()` returns one `KinematicKickPhaseSnapshot`
per phase at its representative tick (`penalty_kick_motion::PHASE_SAMPLE_TICKS`):
the active phase, root/pelvis/head/left-foot/right-foot/right-instep positions, and —
paired with a representative centred ball flight — the ball position, ball velocity,
whether `ball_contact` fired, and whether the ball impulse has been applied. The kick
is inspectable tick-by-tick without a renderer. `SoccerPenaltyKickPose::frame(tick)`
gives the full captured frame for any tick.

`SoccerPenaltyKickPose::validate()` asserts the pose invariants at every phase tick
(finite transforms; root at/above ground; head above pelvis; feet below pelvis — the
kicking foot may rise through strike/follow-through; limbs on their own side except
the follow-through cross; the left sole reaches the plant; the right instep approaches
the ball at the strike).

## Determinism

The pose evaluation and the ball projectile are same-binary deterministic; the whole
kick is a pure function of its style, simulated once and cached. Identical style →
identical snapshots and ball state (proven in
`tests/soccer_penalty_kick_animation.rs`).

## No visible physics junk

The default render path draws only intended diorama objects (there is no physics-debug
render path, and no physics body is a render transform), so there is no orphan
collider/capsule in normal mode. Goalie save-volume debug markers are off by default
and opt-in. Any future physics-debug rendering must be explicitly named and opt-in.

## Why the physics humanoid is disabled (experimental)

A physics-backed *full-humanoid* kicker exists — `penalty_physics_kick` +
`penalty_muscle`, driving the same authored plan through the `axiom-physical-animation`
bridge over real `axiom-physics` bodies — but it is **disabled by default** and
compiled in only behind the `experimental_physical_humanoid_kicker` cargo feature.

`axiom-physics` has rigid bodies, forces, impulses, torques and kinematic bodies, but
**no joints or motors**. In that bridge the 12 limb bodies are driven *kinematically*
to the authored pose each tick, but the **pelvis is a free dynamic body** — force-driven
(anti-gravity + approach + balance) and torque-driven (`apply_torque` with **no
orientation control**). It free-integrates, drifts, and tumbles. `penalty_physics_kick`
then reads that drifting/inverting pelvis body as **joint index 0's render transform**,
so the pelvis box (and its shorts-group `MetaSurface` capsule) detaches from the
kinematically-driven limbs — the inverted body and the orphan capsule. A free dynamic
body with no orientation control is structurally unfit as a skeletal render transform.

### Invariants required before re-enabling the physical humanoid

The bridge itself is sound (it keeps its own always-on, fully-covered module tests in
`axiom-physical-animation`); the defect is the *app* using a raw dynamic body as a
render transform. Before the physical humanoid can become the default again, the app
must guarantee, at every sampled phase tick, that the rendered figure satisfies the
same invariants `SoccerPenaltyKickPose::validate` checks — in particular:

1. **The pelvis body's orientation is controlled**, not free — e.g. the dynamic pelvis
   is orientation-constrained (or its render transform is re-synced to the authored
   root each tick) so it can never invert or tumble.
2. **No render transform ever diverges from the coherent skeleton** — the pelvis stays
   attached to the limb cluster (bounded distance to the authored root), so no part
   detaches into an orphan capsule.
3. All the `validate()` pose invariants hold on the *physics-read* frames, not just the
   authored ones (head above pelvis, feet below pelvis, finite transforms, correct
   sides).

Until those hold on the physics path, the authored/kinematic pose is the default. The
experimental path is documented in `PHYSICS_BACKED_KICK.md` and exercised (only under
the feature) by `tests/soccer_penalty_physics_kick.rs` + `tests/soccer_penalty_muscle.rs`.
