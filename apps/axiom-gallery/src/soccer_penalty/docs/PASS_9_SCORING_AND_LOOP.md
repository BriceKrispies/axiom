# Pass 9 — Deterministic Scoring, Round Advancement & the Playable Loop

Pass 8 said *what happened* to each shot. Pass 9 turns that into a complete short
game: aim → charge → release → watch → result → **points** → next round → after
five rounds, a `SessionComplete` summary → reset and replay.

**No net wobble, no post shake, no ball deflection, no crowd polish, no
persistence.** This pass adds the score/round/session layer only.

## What Pass 9 adds

- **`penalty_scoring`**: `PenaltyScoreRule`, `PenaltyScoreBreakdown`,
  `PenaltyScoreAward`, and the fixed base/bonus tables.
- **`penalty_session`**: `PenaltyScoreState`, `PenaltyBestScore`,
  `PenaltyRoundState` (history item), `PenaltyLoopState`, `PenaltyRoundAdvance`,
  `PenaltyContinueIntent`, `PenaltySessionSummary`, and the driver
  `PenaltySessionState`.
- `PenaltyInputIntent` gains `continue_pressed` (+ `continuing()`); the HUD gains
  score/round/best/award/prompt session fields and `from_session`; the app gains
  `build_session_frame`.

## The 5-round session model

A session is exactly `SESSION_ROUNDS = 5` rounds. It starts at round 1 (shown as
`1 / 5`) with score 0. Internally the round index is zero-based
(`round_index ∈ 0..5`); the HUD always shows the one-based `round_number()`
(`1..=5`). After each resolved shot the score is awarded **once**, the round is
appended to an ordered history vector, and the session waits at a between-rounds
prompt for a continue. Continuing starts the next round (if any remain); the
sixth continue (after round 5) enters `SessionComplete`.

The high-level `PenaltyLoopState` maps onto the Pass 4–8 shot states:
`RoundAiming`←`Aiming`, `RoundCharging`←`Charging`,
`RoundBallInFlight`←`LockedPreview`/`BallInFlight`/`ContactDetected`/
`ArrivedAtGoalPlane`, `RoundResolved`←`Resolved`, `RoundAwarded`←the award tick,
then `BetweenRounds` and `SessionComplete`. The earlier per-shot state machine is
unchanged and still fully tested — the session simply wraps it.

## The scoring table

Base points per result:

| Result | Base |
|--------|------|
| Goal   | 500  |
| Post   | 100  |
| Save   | 0    |
| Miss   | 0    |

## The power bonus rules (Goal only)

- power in `70..=90` → **+150**
- power `> 90` → **+75**
- otherwise → **+0**

## The placement bonus rules (Goal only)

Corner zones in normalized target space (`x ∈ [-100,100]`, `y ∈ [0,100]`):

- upper-left: `x ≤ -70 and y ≥ 70`
- upper-right: `x ≥ 70 and y ≥ 70`
- lower-left: `x ≤ -70 and y ≤ 30`
- lower-right: `x ≥ 70 and y ≤ 30`

Bonus: **+250** for an upper corner, **+150** for any (lower) corner, else **+0**.

## The streak bonus rules (Goal only)

Consecutive goals build a streak (`streak_after = streak_before + 1` on a goal,
reset to `0` on Save/Post/Miss). The 2nd+ consecutive goal adds
`+100 × (streak_after − 1)`: 1st goal `+0`, 2nd `+100`, 3rd `+200`, …

Each `PenaltyScoreAward` records the round number, result kind, every bonus, the
total, and the score/streak **before and after** — fully deterministic and
structurally comparable.

## The round advancement model

`record_resolved` is the session's single scoring entry point (called by
`advance` when a shot first resolves, and directly by tests for forced outcomes):
it computes the award, applies the score, appends the round to history, updates
the best score, and enters `BetweenRounds`. From there a `continue` intent
(`PenaltyContinueIntent::Continue`) advances: `next_advance()` reports
`AdvancedToRound(n)` while `history.len() < 5`, else `SessionComplete`. Advancing
resets the shot (ball at spot, aim centered, power 0, preview/flight/contact/
result cleared, goalie idle, dive lane cleared) but keeps the score and history.

## The session complete model

After the fifth round is awarded, one more continue enters `SessionComplete`,
which is frozen except for reset. `summary()` returns the `final_score`,
`best_score`, and the ordered round history. The HUD shows `PLAY AGAIN`.

## The app-local best score model

`PenaltyBestScore` starts at 0 and updates **immediately after each award** when
the current score exceeds it (`best = max(best, score)`), so it also reflects the
final score when a session completes. A `reset` starts a fresh session (round 1,
score 0, empty history) but **preserves the best score**. Best score lives only
in this in-memory model.

## Why there is no persistence yet

Best score is app-local model state only — no `localStorage`, cookies, files,
server, or global mutable state. Persisting a profile / leaderboard is a separate
concern (storage, identity, networking) deliberately outside this pass; a later
integration can read the in-memory best score and persist it.

## Why this is still app-level, not an engine scoring framework

Every rule here — the base table, the corner zones, the streak curve, the
five-round structure — is specific to this penalty game. There are no generic
score channels, rule graphs, or session abstractions, and nothing is meant to be
reused by another app. It is one deterministic state machine over explicit
ordered vectors, living entirely inside `apps/axiom-soccer-penalty`.

## Still not implemented (later stages)

- net wobble;
- post / crossbar shake;
- ball deflection;
- crowd reaction;
- advanced result polish effects;
- persistent player profile / server leaderboard integration.

See `STAGE_1.md` for the full roadmap.
