# Pass 5 — Deterministic Ball Trajectory

Pass 4 let the player aim and charge, freezing a `PenaltyShotPreview` on release
while the ball stayed on the spot. Pass 5 launches that shot: the ball follows a
deterministic parametric arc from the penalty spot to the selected point on the
goal plane, its blob shadow tracks it, an optional trail follows, and it stops in
an `ArrivedAtGoalPlane` state when it gets there.

**No goal/save/miss/post is resolved, there is no goalie reaction, no net wobble,
and no scoring change.** Still fixed-camera, still fully deterministic.

## What Pass 5 adds

- **`PenaltyShotFlightState`** — the Pass 4 state enum extended with
  `BallInFlight` and `ArrivedAtGoalPlane`.
- **`penalty_ball`** module: `PenaltyBallTrajectory` (the parametric curve),
  `PenaltyBallPose` (per-tick position + shadow + trail), `PenaltyBallState`
  (a coarse ball-focused view), `PenaltyShotFlightDescriptor` (the stable,
  replayable shot descriptor), and `PenaltyBallFlight` (the live flight).
- **`PenaltyInteractionState`** gains a `flight: Option<PenaltyBallFlight>` and
  `ball_pose()` / `ball_state()`; `advance` launches and progresses the flight.
- The app overlays the live ball pose onto the ball + `shadow.ball` render items
  and appends trail samples; the HUD shows `FLIGHT` / `ARRIVED`.

## The deterministic trajectory model

One closed-form curve, sampled at fixed ticks — **not** physics, **not** a
projectile framework. It is derived only from the frozen preview (`target_x`,
`target_y`, `power`) and the fixed constants below:

```text
t      = elapsed_ticks / total_flight_ticks          (clamped to [0,1])
x      = lerp(start.x, target.x, t) + curve * sin_pi_approx(t)   // curve = 0
z      = lerp(start.z, goal_plane_z, t)              // monotonic toward the goal
base_y = lerp(start.y, target.y, t)
arc    = sin_pi_approx(t) * arc_height
y      = base_y + arc
```

`sin_pi_approx(t)` is the fixed parabola `4·t·(1−t)`: exactly `0` at both ends,
`1` at the apex `t = 0.5`, symmetric — a cheap, closed-form, platform-independent
stand-in for `sin(pi·t)` with **no external sine dependency**. Because it is `0`
at `t = 1`, the arc vanishes exactly at arrival, so the ball lands precisely on
the mapped target.

### Fixed constants (`penalty_ball.rs`)

| Constant             | Value                     | Meaning                          |
|----------------------|---------------------------|----------------------------------|
| penalty spot         | `(0, BALL_RADIUS, 11)`    | ball rest / flight start         |
| goal plane z         | `GOAL_LINE_Z = 0`         | flight end plane                 |
| goal width / height  | `GOAL_HALF_WIDTH=3.66`, `GOAL_HEIGHT=2.44` | target mapping    |
| ball radius          | `BALL_RADIUS = 0.32`      | visual size                      |
| min / max flight     | `24` / `60` ticks         | duration clamp                   |
| max arc height       | `2.2`                     | apex at zero power               |
| curve amount         | `0.0`                     | lateral curve (zero in Pass 5)   |
| trail max            | `6`                       | trail sample count               |

## Normalized aim → world target mapping

The Pass 4 target space maps to a world point on the goal plane:

- `target_x ∈ [-100, 100]` → `x = GOAL_HALF_WIDTH · target_x/100` (±3.66 at the posts),
- `target_y ∈ [0, 100]` → `y = GOAL_HEIGHT · target_y/100` (0 at the ground, 2.44 at the bar),
- `z = GOAL_LINE_Z` (always the goal plane).

## Power → flight-duration mapping

Stronger power → shorter (or equal) flight:

```
total_ticks = clamp( MAX_FLIGHT_TICKS − (MAX−MIN)·power/100 , MIN, MAX )
```

`power = 0 → 60 ticks` (slowest), `power = 100 → 24 ticks` (fastest). Power also
flattens the arc: `arc_height = MAX_ARC_HEIGHT · (1 − 0.005·power)`. All integer /
fixed-scalar math — no wall-clock, no randomness.

## The ball pose descriptor

Each flight tick produces a `PenaltyBallPose`:

- `position` — world position on the arc;
- `radius` — visual radius (`BALL_RADIUS`; the fixed camera's perspective scales
  it with depth at render time);
- `shadow_center`, `shadow_radius_x/z` — the blob shadow;
- `trail` (`[Vec3; 6]`) + `trail_len` — previous positions;

built from explicit arrays, never a map.

## Ball shadow behavior

The Pass 3 `shadow.ball` blob now tracks the ball's `x/z` while staying on the
pitch (`shadow_center.y` constant). It shrinks as the ball rises
(`factor = 1/(1 + height·0.5)`), so a lofted ball casts a smaller shadow. At rest
the factor is `1`, reproducing Pass 3's shadow exactly — so the default frame is
byte-identical to earlier passes. The shadow stays in the `ActorShadow` layer
with a deterministic order.

## Trail behavior

A cheap deterministic trail: up to `TRAIL_MAX = 6` previous positions, sampled
from the same trajectory (`position_at(elapsed − 1 − i)`). The app emits them as
small shrinking quads with stable sequential ids in the **ForegroundEffects**
layer (empty until Pass 5), so they draw after every world item and before the
HUD. The trail is empty at rest, so it does not appear in the default diorama.

## Why this is parametric projectile motion, not a physics engine

There is no integrator, no forces, no gravity constant, no collision solver, no
timestep accumulation. The whole flight is a pure function `position_at(elapsed)`
of one frozen descriptor — evaluate it at any tick and get the same answer, in
any order, on any platform. That is exactly what makes it replayable and
unit-testable, and exactly why it is *not* a general physics or projectile
system (both of which have their own broad determinism and integration concerns
this app does not need yet).

## Why goal/save/miss/post resolution is deliberately not implemented yet

Resolution needs the ball's arrival point *and* the goalie's save volumes, goal
geometry, and post geometry — none of which exist yet. Committing to an outcome
now would entangle goalie behavior, collision, and scoring before the trajectory
itself is proven. Pass 5 stops the ball at `ArrivedAtGoalPlane` and exposes the
exact arrival pose + descriptor; a later pass consumes that to resolve the shot.

## Still not implemented (later stages)

- goalie dive animation;
- goalie save volumes;
- collision (against the goalie, posts, or net);
- save / goal / miss / post resolution;
- net wobble or impact effects;
- scoring / round / best-score changes.

The HUD shows `FLIGHT` / `ARRIVED` and **never** `GOAL` / `SAVE` / `MISS` /
`POST`. See `STAGE_1.md` for the full roadmap.
