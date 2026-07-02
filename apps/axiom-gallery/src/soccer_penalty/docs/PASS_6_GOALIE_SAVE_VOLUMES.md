# Pass 6 — Deterministic Goalie Save Volumes & Contact Detection

Pass 5 flew the ball along a deterministic arc to the goal plane. Pass 6 puts the
goalie in the way: the static keeper gets a small set of invisible collision
volumes, and the ball sphere is tested against them each flight tick to produce
**neutral, pre-result contact facts** a later pass can resolve.

**This decides no shot outcome.** There is no save/goal/miss/post, no goalie
dive, no animation, no scoring — only deterministic contact information.

## What Pass 6 adds

- **`penalty_goalie`** module: `PenaltyGoalieVolume`, `PenaltyGoalieVolumeKind`,
  `PenaltyGoalieVolumeSet`, `PenaltyGoalieContact`, `PenaltyGoalieContactKind`,
  `PenaltyGoalieContactFrame`, `PenaltyGoalieContactDetector`, and
  `PenaltyGoalieDebugDescriptor`.
- A new **`ContactDetected`** state in the shot state machine.
- A `contact: Option<PenaltyGoalieContactFrame>` on the interaction state.
- Optional, off-by-default debug visualization of the volumes.

## Why goalie contact is represented with simple deterministic volumes

The keeper is static and the ball is a sphere on a closed-form path, so the
"does the keeper touch it" question is answerable with a handful of fixed shapes
and one sphere-overlap test — no integrator, no broad-phase, no contact manifold.
Simple volumes are deterministic, replayable, trivially testable, and exactly
enough to feed a later resolution pass. Anything heavier (a real collision
system, a physics engine, a character controller) would add nondeterminism and
scope this pass explicitly avoids.

## Volume kinds and priority order

Four volumes, attached to the Stage 1 goalie, in **strict priority order**:

| Priority | Kind        | Shape        | Placement                         |
|----------|-------------|--------------|-----------------------------------|
| 0        | `LeftHand`  | sphere       | goalie's left hand, small radius  |
| 1        | `RightHand` | sphere       | goalie's right hand, small radius |
| 2        | `Torso`     | AABB         | goalie torso, medium              |
| 3        | `Body`      | AABB         | broad standing-keeper silhouette  |

The declaration order **is** the priority (`derive(Ord)`), stored in an explicit
ordered array — never a map. If the ball overlaps several volumes on the same
tick, the first in priority order wins (hands beat torso beats body). Contact
detection stops at the first contacting tick and freezes the ball
(`ContactDetected`).

## The ball-sphere vs goalie-volume detection model

- The ball is a sphere of the fixed Pass 5 `BALL_RADIUS`.
- Sphere vs sphere: overlap when `distance(centers) ≤ r_volume + r_ball`.
- Sphere vs AABB: overlap when the ball center is within `r_ball` of the box's
  closest point (component-wise clamp).
- Each `BallInFlight` tick, `PenaltyGoalieContactDetector::detect` walks the
  volumes in priority order and returns a `PenaltyGoalieContactFrame` carrying:
  the shot-local tick, the ball position, the ball radius, and — on contact —
  the contacted volume kind, its stable ordinal, the neutral contact kind, and
  an approximate contact point. No contact → `contact: None`.

The neutral contact labels are `Hand` / `Torso` / `Body` / `None` — deliberately
**not** `Save` / `Goal` / `Miss` / `Post` / `Score`.

## State model

```
… BallInFlight ──contact──► ContactDetected   (ball frozen, frame stored)
        │
        └──no contact, reached plane──► ArrivedAtGoalPlane
reset_pressed ─────────────────────────► Aiming (contact cleared)
```

- `Aiming` / `Charging` / `LockedPreview` — no contact detection (ball at spot).
- `BallInFlight` — sample the ball pose and test the volumes every tick.
- `ContactDetected` — the first contact frame is stored and the ball freezes
  (pre-resolution; **not** a final result).
- `ArrivedAtGoalPlane` — the ball reached the plane untouched (still unresolved).
- `reset_pressed` — clears the contact and returns to `Aiming`.

## Why this is not a physics engine

There are no forces, no gravity, no timestep integration, no collision response,
no restitution, no manifolds. The ball does not deflect — on contact it simply
freezes and the fact is recorded. It is one overlap predicate evaluated over four
fixed shapes.

## Why this is not ragdoll

The goalie never moves. The volumes are rigid, fixed descriptors bolted to a
static puppet; there is no articulated body, no joints, no simulation of limbs.

## Why this is not final save/goal/miss/post resolution

Resolving a shot needs more than "did the keeper touch it": it needs goal-line
and post geometry, the keeper's *reach* over time (a dive), and the scoring
rules. Pass 6 provides only the raw contact fact; committing to an outcome now
would entangle goalie animation, geometry, and scoring before the contact model
is proven. The HUD shows the neutral `CONTACT` / `ARRIVED` states and (in debug)
a `HAND` / `TORSO` / `BODY` / `NONE` label — never a result word.

## How later passes will attach these volumes to animated goalie poses

Today the volume set is fixed to the static keeper's rest pose. When the goalie
gains dive/pose clips (a later pass), each pose will supply the volume centers
(hands/torso follow the animated puppet parts), and the same
`PenaltyGoalieContactDetector` will run against the *posed* set — the detection
model is unchanged; only where the volumes sit each tick changes.

## How later passes will use contact information to resolve SAVE / BLOCK

A resolution pass will read the frozen `PenaltyGoalieContactFrame` (and the
ball's arrival point when there is no contact) and map it to a real outcome: a
`Hand`/`Torso`/`Body` contact inside the goal mouth becomes a `SAVE`/`BLOCK`, a
clear arrival inside the frame becomes a `GOAL`, outside becomes a `MISS`, and a
post-adjacent path becomes a `POST` — then update the score. None of that exists
yet.

## Debug visualization

`PenaltyGoalieDebugDescriptor` is **off by default**. When enabled (in tests or
dev), `SoccerPenaltyApp::build_frame_with_debug` appends one billboard quad per
volume (green hands, yellow torso, red body — all unlit) with stable sequential
ids in the `ForegroundEffects` layer, and sets a neutral HUD `debug_contact`
label. Debug descriptors are purely cosmetic: they never influence contact
detection, and they never appear in production frames (the default
`build_frame` / `build_stage1` path). Nothing is printed to the console.

## Still not implemented (later stages)

- goalie dive animation;
- goalie pose clips;
- volume attachment to animated puppet parts;
- ball deflection;
- final save / goal / miss / post resolution;
- net wobble or impact effects;
- scoring / round / best-score changes.

See `STAGE_1.md` for the full roadmap.
