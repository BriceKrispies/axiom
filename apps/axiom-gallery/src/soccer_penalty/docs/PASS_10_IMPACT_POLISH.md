# Pass 10 — Deterministic Impact Polish & Result Juice

Pass 9 made the game playable and scored. Pass 10 makes resolved shots *feel*
satisfying: net wobble on a goal, post/crossbar shake on a post, a save impact
flash + fake ball deflection, a miss drift, crowd reactions, a little camera
juice, and an animated result banner + score popup.

**No real physics, no ragdoll, no particle engine, no browser APIs.** Every
effect is a deterministic, tick-driven, app-local *descriptor* — a visual
consequence of an already-resolved result. Pass 8 result classification and Pass
9 scoring are untouched.

## What Pass 10 adds

- **`penalty_effects`**: `PenaltyImpactEffectKind`, `PenaltyImpactEffectTimeline`,
  `PenaltyImpactEffectState`, `PenaltyEffectDescriptor`, `PenaltyNetWobble`(+`Node`),
  `PenaltyGoalFrameShake`, `PenaltyBallDeflectionVisual`, `PenaltyCrowdReaction`,
  `PenaltyCameraJuice`, `PenaltyResultBanner`, `PenaltyScorePopup`, and
  `PenaltyEffectRenderItem`.
- The session carries a live `effect` that starts when a shot resolves (and when
  the session completes), ticks while between rounds / session-complete, and
  clears on continue/reset.
- The HUD gains an animated `banner` + `score_popup`; the app applies the effect
  to the frame (deflect the ball, shake the hit post, bounce the crowd, add net
  wobble + flash render items, offset the camera).

## The deterministic effect timeline model

Each result kind maps to a fixed-duration timeline (all tick-driven, replayable):

| Effect            | Ticks |
|-------------------|-------|
| Goal              | 72    |
| Save              | 54    |
| Post              | 54    |
| Miss              | 42    |
| SessionComplete   | 90    |

`PenaltyImpactEffectState::describe()` produces the full `PenaltyEffectDescriptor`
at the current tick, including normalized `progress` (`0..=1000`). Nothing reads
the wall clock or frame duration; the same `(result, tick, final pose, award)`
always yields the same descriptor.

## The net wobble model (Goal only)

The net stays fake line/grid geometry — no cloth simulation. It is a fixed,
ordered grid of `PenaltyNetWobbleNode`s (5×4 per rear/front panel). Each node's
displacement is a pure function of: the impact point (final ball pose), the
node's distance from impact (a `1/(1+d·k)` falloff), the effect tick (a fixed
decay), and a fixed oscillation lookup table (no trig dependency). The nodes are
emitted as render items that **still sort into `RearNet` / `FrontNet`** (Pass 2
layering preserved).

## The post/crossbar shake model (Post only)

Only the hit part shakes: `HitLeftPost → LeftPost`, `HitRightPost → RightPost`,
`HitCrossbar → Crossbar`. The shake is a small tick-driven offset from a fixed
lookup table applied to that part's `GoalFrame` render item. It does **not** move
the goal collision volumes and does **not** alter the resolved result.

## The save impact flash model (Save only)

A `PenaltyEffectRenderItem` flash at the contact point, fading over ~`POP_TICKS`,
rendered in `ForegroundEffects`.

## The fake ball deflection visual model (Save)

`PenaltyBallDeflectionVisual` slides the ball's *visual* final pose from the
contact point to a biased end point over `DEFLECT_TICKS` — no physics re-run, no
change to the result. The bias depends on the save: left hand → away to the
right, right hand → away to the left, torso/body → down and forward.

## The miss drift visual model (Miss)

The same deflection descriptor, drifting the ball slightly past the goal plane
(down and behind) over fixed ticks. No collision, no physics, no score change.

## The crowd reaction model

`PenaltyCrowdReaction` gives each crowd card (by stable ordinal) a deterministic
bounce offset and a color pulse, from fixed bounce/oscillation tables scaled by a
per-result amplitude: Goal strongest, then Save, Post, Miss; SessionComplete
celebrates only if the final score is > 0. Cards stay in the `Crowd` layer.

## The camera juice descriptor

`PenaltyCameraJuice` is a small **additive** offset to the fixed Stage 1 camera —
not a camera controller and not input-driven. It is a decaying shake over the
first `CAMERA_SHAKE_TICKS`, **zero before an effect starts** and **zero after**
the shake window (and cleared between rounds). The base camera stays
authoritative.

## The result banner and score popup

`PenaltyResultBanner` carries the big word (`GOAL` / `SAVE` / `POST` / `MISS` /
`FINAL SCORE`) with a deterministic pop scale + pulse; `PenaltyScorePopup`
carries the Pass 9 awarded `+N` with a pop scale. Both are unlit HUD-model
descriptors; the banner rides the existing `Hud`-layer instruction slot. The HUD
still renders last.

## Why this is not physics

There is no integrator, no forces, no collision response, no restitution. Net
wobble, deflection, drift, shake, and camera juice are all closed-form lookups of
the effect tick.

## Why this is not ragdoll

Nothing articulates or simulates a body under forces. The goalie's pose is still
the Pass 7 authored clip; the deflection is a straight visual slide of the ball.

## Why this is not a generic particle/effects engine

Every effect is a specific, hand-authored descriptor for one penalty game — one
net grid, one shake table, one crowd bounce, one banner. There are no generic
emitters, particle pools, timelines, or reusable effect graphs.

## Why Pass 10 does not change scoring or result resolution

Effects are consumers of the already-resolved `PenaltyShotResult` (Pass 8) and
the already-computed `PenaltyScoreAward` (Pass 9). The session awards points
exactly once, on resolution, *before* the effect starts; ticking the effect never
re-awards, never re-classifies, and never changes the round count.

## Still not implemented (later stages)

- persistent leaderboards;
- server analytics;
- asset import;
- audio;
- richer animation blending;
- generalized reusable sports/effects modules.

See `STAGE_1.md` for the full roadmap.
