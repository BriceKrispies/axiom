# Pass 8 — Deterministic Shot Result Resolution

Passes 5–7 gave a deterministic ball flight and a diving goalie with animated
save volumes, but the game never said *what happened*. Pass 8 classifies each
shot into one final result — **`Goal` / `Save` / `Miss` / `Post`** — using fixed
goal-mouth and goal-frame tests, and shows it on the HUD.

**No score/round/best changes, no net wobble, no post shake, no deflection, no
crowd polish.** This pass only *reports* the outcome.

## What Pass 8 adds

- **`penalty_result`** module: `PenaltyShotResultKind`, `PenaltyShotResultDetail`,
  `PenaltyShotResult`, `PenaltyShotResultResolver`, `PenaltyGoalMouth`,
  `PenaltyGoalFrameVolume`, `PenaltyGoalFrameVolumeKind`,
  `PenaltyGoalFrameVolumeSet`, `PenaltyGoalPlaneCrossing`,
  `PenaltyResolvedShotState`, and `PenaltyResultHudDescriptor`.
- A new **`Resolved`** state; a `resolved: Option<PenaltyResolvedShotState>` on
  the interaction state; and a `result` descriptor on the HUD.
- A small widening of the aim range so shots can go *wide* / *high* (misses).

## The final result kinds

`Goal`, `Save`, `Miss`, `Post`. Each carries a `PenaltyShotResultDetail`:

- Save → `SavedByLeftHand` / `SavedByRightHand` / `SavedByTorso` / `SavedByBody`
- Post → `HitLeftPost` / `HitRightPost` / `HitCrossbar`
- Goal → `Scored`
- Miss → `MissedLeft` / `MissedRight` / `MissedHigh` / `MissedWideOrHigh`

## The result priority order

Highest first:

1. **goalie contact → `Save`**
2. **post / crossbar → `Post`**
3. **inside the goal mouth → `Goal`**
4. **otherwise → `Miss`**

Goalie contact is detected *during* flight (Pass 7), so the ball freezes in
`ContactDetected` before it ever reaches the goal plane — a save therefore always
precedes (and outranks) a post/goal/miss automatically. Post is tested before
goal, so a ball that is inside the mouth *and* clipping a post is a `Post`.

## State flow

```
BallInFlight ──contact──► ContactDetected ──(next tick)──► Resolved (Save)
        │
        └──reaches plane──► ArrivedAtGoalPlane ──(next tick)──► Resolved (Goal/Miss/Post)
reset_pressed ─────────────────────────────────────────────► Aiming (result cleared)
```

`ContactDetected` / `ArrivedAtGoalPlane` are still observable for one tick (Pass
5–7 behavior is unchanged); resolution happens on the *following* tick into
`Resolved`. Once resolved, the ball pose and result are frozen; the goalie
continues (or clamps at) its dive clip. Only `reset` leaves `Resolved`.

## The goal-mouth constants

`PenaltyGoalMouth::stage1()` uses the true Stage 1 goal dimensions:

| Field                | Value                          |
|----------------------|--------------------------------|
| goal plane z         | `GOAL_LINE_Z = 0`              |
| left / right post x  | `∓GOAL_HALF_WIDTH = ∓3.66`     |
| ground y             | `GROUND_Y = 0`                |
| crossbar y           | `GOAL_HEIGHT = 2.44`          |
| post / bar thickness | `POST_THICKNESS = 0.12`       |

`contains_center(x, y)` tests the ball **center** against these bounds (Pass 8
uses the center only, per scope).

Note: the aim range now reaches ~35% beyond the frame at the extremes
(`AIM_HALF_SPAN` / `AIM_TOP` in `penalty_ball`), so the goal mouth is the *inner*
portion of the aim range — that is what makes wide/high misses reachable while a
centered aim stays comfortably inside the goal.

## The goal-frame / post / crossbar volume model

`PenaltyGoalFrameVolumeSet::stage1()` holds three narrow axis-aligned boxes in
priority order: `LeftPost`, `RightPost`, `Crossbar`. `first_hit` returns the
first overlapping volume (ball treated as a sphere of the fixed Pass 5 radius,
closest-point test). Explicit ordered array — no maps.

## How goalie contact becomes a save

If Pass 7 recorded a contact frame, `PenaltyShotResultResolver::from_contact`
maps the contacted **volume** to a keyed save: `LeftHand → SavedByLeftHand`,
`RightHand → SavedByRightHand`, `Torso → SavedByTorso`, `Body → SavedByBody`.
(The neutral Pass 6/7 contact model already distinguishes left vs right hand by
volume kind, so no extension was needed.)

## How post, goal, and miss are classified

At the goal plane a `PenaltyGoalPlaneCrossing` records the ball center, radius,
normalized target, whether the center is inside the mouth, and any frame hit.
`from_crossing` then resolves:

- a **frame hit** → `Post` (`HitLeftPost` / `HitRightPost` / `HitCrossbar`);
- else **inside the mouth** → `Goal` (`Scored`);
- else → `Miss`, keyed by the crossing position (`MissedLeft` if left of the
  posts, `MissedRight` if right, `MissedHigh` if over the bar, else
  `MissedWideOrHigh`).

## Ball final pose

The ball is frozen at resolution: on a `Save` at the contact-frame position, on
`Goal` / `Miss` / `Post` at the goal-plane crossing position. There is no
deflection off the keeper, posts, or net — the ball simply stops.

## Why score changes are deliberately deferred

Scoring is its own concern (score/round/best totals, best-score tracking, round
progression). Pass 8's job is only to *classify* the shot deterministically; the
scoreboard stays static (`SCORE 1250` / `ROUND 3 / 5` / `BEST 2520`). A later
pass consumes the `PenaltyShotResult` to update the totals.

## Why net wobble and post reaction are deliberately deferred

Those are polish/effects (a wobbling net mesh, a shaking post, particles). They
add nondeterministic-looking motion and their own animation state, none of which
is needed to say *what happened*. Pass 8 freezes the ball and reports the result;
the reactions come later.

## Why this is still not a physics engine

There is no integrator, no forces, no collision response, no restitution, no
deflection. Resolution is a handful of fixed AABB/sphere overlap predicates and a
strict priority order evaluated once, at the moment the ball freezes.

## Still not implemented (later stages)

- score changes;
- round advancement;
- net wobble;
- post / crossbar shake;
- ball deflection;
- crowd reaction;
- result polish effects.

See `STAGE_1.md` for the full roadmap.
