//! Pass 9 — the deterministic 5-round session / playable loop.
//!
//! Wraps the single-shot [`PenaltyInteractionState`] (Passes 4–8) in a session
//! layer: after each shot resolves it awards points once, records the round,
//! updates the best score, and waits for a continue/reset intent to advance
//! through five rounds into a `SessionComplete` summary.
//!
//! This is **not** a scoring/sports framework and **not** a persistence system:
//! it is one app-local state machine over explicit ordered vectors. Best score
//! lives only in this in-memory model — no storage, no server, no globals. No
//! wall-clock, no randomness, no maps.

use axiom_math::Vec3;

use crate::soccer_penalty::penalty_effects::{PenaltyEffectDescriptor, PenaltyImpactEffectState};
use crate::soccer_penalty::penalty_input::PenaltyInputIntent;
use crate::soccer_penalty::penalty_interaction::{PenaltyInteractionState, PenaltyShotFlightState};
use crate::soccer_penalty::penalty_result::PenaltyShotResult;
use crate::soccer_penalty::penalty_scoring::{PenaltyScoreAward, PenaltyScoreRule};

/// A session is exactly five rounds.
pub const SESSION_ROUNDS: u32 = 5;

/// The running score + streak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyScoreState {
    pub score: u32,
    pub streak: u32,
}

impl PenaltyScoreState {
    pub const fn zero() -> Self {
        Self { score: 0, streak: 0 }
    }
}

/// The app-local best score (in-memory only; no persistence).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyBestScore {
    pub best: u32,
}

impl PenaltyBestScore {
    pub const fn zero() -> Self {
        Self { best: 0 }
    }

    /// Update the best score if `score` exceeds it (immediately after each
    /// award — see PASS_9 docs).
    pub fn updated(self, score: u32) -> Self {
        Self { best: self.best.max(score) }
    }
}

/// One completed round's record (history item).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyRoundState {
    pub round_number: u32,
    pub target_x: i32,
    pub target_y: i32,
    pub power: i32,
    pub result: PenaltyShotResult,
    pub award: PenaltyScoreAward,
    pub final_ball_position: Vec3,
}

/// The high-level loop state (a session view over the per-shot states).
///
/// Mapping onto the Pass 4–8 shot states: `RoundAiming`←`Aiming`,
/// `RoundCharging`←`Charging`, `RoundBallInFlight`←`LockedPreview`/`BallInFlight`
/// /`ContactDetected`/`ArrivedAtGoalPlane`, `RoundResolved`←`Resolved` (the tick
/// the result is known), `RoundAwarded`←the tick points are granted, then
/// `BetweenRounds` (waiting for continue) and finally `SessionComplete`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyLoopState {
    RoundAiming,
    RoundCharging,
    RoundBallInFlight,
    RoundResolved,
    RoundAwarded,
    BetweenRounds,
    SessionComplete,
}

/// The outcome of a continue at the between-rounds prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyRoundAdvance {
    AdvancedToRound(u32),
    SessionComplete,
}

/// The deterministic continue intent, derived from the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyContinueIntent {
    Reset,
    Continue,
    Wait,
}

impl PenaltyContinueIntent {
    /// Reset wins over continue; otherwise wait.
    pub fn from_intent(intent: PenaltyInputIntent) -> Self {
        intent
            .reset_pressed
            .then_some(PenaltyContinueIntent::Reset)
            .or(intent.continue_pressed.then_some(PenaltyContinueIntent::Continue))
            .unwrap_or(PenaltyContinueIntent::Wait)
    }
}

/// A finished session's summary.
#[derive(Debug, Clone, PartialEq)]
pub struct PenaltySessionSummary {
    pub final_score: u32,
    pub best_score: u32,
    pub rounds: Vec<PenaltyRoundState>,
}

/// The full deterministic session state: the current shot + score/round/best +
/// ordered round history + the loop state.
#[derive(Debug, Clone, PartialEq)]
pub struct PenaltySessionState {
    pub shot: PenaltyInteractionState,
    pub score: PenaltyScoreState,
    pub best: PenaltyBestScore,
    /// Zero-based current round index (`0..SESSION_ROUNDS`).
    pub round_index: u32,
    pub history: Vec<PenaltyRoundState>,
    pub loop_state: PenaltyLoopState,
    /// The most recent award (for the between-rounds HUD).
    pub last_award: Option<PenaltyScoreAward>,
    /// The live impact-polish effect (Pass 10); ticked while between rounds /
    /// session-complete, cleared on continue/reset.
    pub effect: Option<PenaltyImpactEffectState>,
}

impl PenaltySessionState {
    /// A fresh session: round 1, score 0, best 0.
    pub fn new() -> Self {
        Self::with_best(PenaltyBestScore::zero())
    }

    /// A fresh session that keeps an existing best score.
    pub fn with_best(best: PenaltyBestScore) -> Self {
        Self {
            shot: PenaltyInteractionState::start(),
            score: PenaltyScoreState::zero(),
            best,
            round_index: 0,
            history: Vec::new(),
            loop_state: PenaltyLoopState::RoundAiming,
            last_award: None,
            effect: None,
        }
    }

    /// The one-based round number shown to the player (`1..=5`).
    pub fn round_number(&self) -> u32 {
        self.round_index + 1
    }

    /// Advance one fixed tick with the given intent. Reset always starts a fresh
    /// session (best preserved). Deterministic and total.
    pub fn advance(self, intent: PenaltyInputIntent) -> Self {
        if intent.reset_pressed {
            return Self::with_best(self.best);
        }
        match self.loop_state {
            PenaltyLoopState::RoundAiming
            | PenaltyLoopState::RoundCharging
            | PenaltyLoopState::RoundBallInFlight
            | PenaltyLoopState::RoundResolved => self.step_shot(intent),
            PenaltyLoopState::RoundAwarded | PenaltyLoopState::BetweenRounds => {
                self.step_between(intent)
            }
            // Session complete: freeze the loop, but keep ticking the
            // celebration effect (reset is handled above).
            PenaltyLoopState::SessionComplete => self.tick_effect(),
        }
    }

    /// Advance the live effect one tick (no-op if none).
    fn tick_effect(self) -> Self {
        Self { effect: self.effect.map(|e| e.advanced()), ..self }
    }

    /// The current effect descriptor bundle, if an effect is playing.
    pub fn effect_descriptor(&self) -> Option<PenaltyEffectDescriptor> {
        self.effect.map(|e| e.describe())
    }

    /// The additive camera offset from the current effect (zero if none).
    pub fn camera_offset(&self) -> Vec3 {
        self.effect.map(|e| e.describe().camera.offset).unwrap_or(Vec3::ZERO)
    }

    /// Step the current shot; award once when it first resolves.
    fn step_shot(self, intent: PenaltyInputIntent) -> Self {
        let shot = self.shot.advance(intent);
        match shot.state {
            PenaltyShotFlightState::Resolved => self.award_on_resolve(shot),
            other => Self { shot, loop_state: loop_from_shot(other), ..self },
        }
    }

    /// Compute + apply the award for a resolved shot exactly once.
    fn award_on_resolve(self, shot: PenaltyInteractionState) -> Self {
        let target = shot.preview;
        let resolved = shot.resolved;
        match (target, resolved) {
            (Some(preview), Some(res)) => Self { shot, ..self }.record_resolved(
                preview.target_x,
                preview.target_y,
                preview.power,
                res.result,
                res.final_ball_position,
            ),
            // Defensive: a resolved shot always has both; keep the shot frozen.
            _ => Self { shot, loop_state: PenaltyLoopState::BetweenRounds, ..self },
        }
    }

    /// The session's scoring entry point: award the current round from an
    /// explicit resolved outcome, append history, update best, and wait for a
    /// continue. Called by [`Self::advance`] on shot resolution, and directly by
    /// tests to score forced outcomes.
    pub fn record_resolved(
        self,
        target_x: i32,
        target_y: i32,
        power: i32,
        result: PenaltyShotResult,
        final_ball_position: Vec3,
    ) -> Self {
        let award = PenaltyScoreRule::award(
            self.round_number(),
            result,
            power,
            target_x,
            target_y,
            self.score.score,
            self.score.streak,
        );
        let score = PenaltyScoreState { score: award.score_after, streak: award.streak_after };
        let best = self.best.updated(score.score);
        let item = PenaltyRoundState {
            round_number: self.round_number(),
            target_x,
            target_y,
            power,
            result,
            award,
            final_ball_position,
        };
        let mut history = self.history;
        history.push(item);
        // Start the deterministic impact-polish effect for this result (Pass 10).
        let effect = Some(PenaltyImpactEffectState::for_result(result, final_ball_position, award.total));
        Self {
            score,
            best,
            history,
            loop_state: PenaltyLoopState::BetweenRounds,
            last_award: Some(award),
            effect,
            ..self
        }
    }

    /// Handle the between-rounds prompt: continue to the next round, or finish
    /// the session after the fifth round.
    fn step_between(self, intent: PenaltyInputIntent) -> Self {
        match PenaltyContinueIntent::from_intent(intent) {
            PenaltyContinueIntent::Continue => self.continue_round(),
            // Waiting: keep ticking the impact-polish effect timeline.
            _ => self.tick_effect(),
        }
    }

    /// Continue: start the next round (ball/aim/power/result/goalie reset) if
    /// rounds remain, else complete the session.
    fn continue_round(self) -> Self {
        let advance = self.next_advance();
        match advance {
            // Next round: reset the shot + clear the effect; keep score/history.
            PenaltyRoundAdvance::AdvancedToRound(_) => Self {
                shot: PenaltyInteractionState::start(),
                round_index: self.history.len() as u32,
                loop_state: PenaltyLoopState::RoundAiming,
                last_award: None,
                effect: None,
                ..self
            },
            // Session over: start the celebration effect.
            PenaltyRoundAdvance::SessionComplete => Self {
                loop_state: PenaltyLoopState::SessionComplete,
                effect: Some(PenaltyImpactEffectState::session_complete(self.score.score)),
                ..self
            },
        }
    }

    /// What a continue would do from the current between-rounds state.
    pub fn next_advance(&self) -> PenaltyRoundAdvance {
        if self.history.len() < SESSION_ROUNDS as usize { PenaltyRoundAdvance::AdvancedToRound(self.history.len() as u32 + 1) } else { PenaltyRoundAdvance::SessionComplete }
    }

    /// The session summary (final score, best, ordered rounds).
    pub fn summary(&self) -> PenaltySessionSummary {
        PenaltySessionSummary {
            final_score: self.score.score,
            best_score: self.best.best,
            rounds: self.history.clone(),
        }
    }
}

impl Default for PenaltySessionState {
    fn default() -> Self {
        Self::new()
    }
}

fn loop_from_shot(state: PenaltyShotFlightState) -> PenaltyLoopState {
    match state {
        PenaltyShotFlightState::Aiming => PenaltyLoopState::RoundAiming,
        PenaltyShotFlightState::Charging => PenaltyLoopState::RoundCharging,
        _ => PenaltyLoopState::RoundBallInFlight,
    }
}
