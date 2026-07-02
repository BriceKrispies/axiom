//! The chunky arcade HUD model.
//!
//! Pass 4 makes the HUD a deterministic *view* of the interaction state: the
//! score/round/best stay static, but the power meter and aim reticle now
//! reflect the current [`PenaltyInteractionState`], and an instruction label
//! reflects the phase. It is still pure data — no drawing, no input — and the
//! HUD is always unlit and rendered last.
//!
//! `PenaltyHudModel::from_state` is a pure function of the interaction state and
//! the fixed score constants, so identical states produce identical HUDs.

use crate::soccer_penalty::penalty_interaction::{PenaltyInteractionState, PenaltyShotFlightState, POWER_MAX};
use crate::soccer_penalty::penalty_effects::{PenaltyResultBanner, PenaltyScorePopup};
use crate::soccer_penalty::penalty_result::PenaltyResultHudDescriptor;
use crate::soccer_penalty::penalty_scoring::PenaltyScoreAward;
use crate::soccer_penalty::penalty_session::{PenaltyLoopState, PenaltySessionState};

/// A normalized 2D screen position, `0.0..=1.0` on each axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPos {
    pub x: f32,
    pub y: f32,
}

impl ScreenPos {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// The shot-power meter view: current value, normalized fill, chunky segment
/// count, whether it is frozen, and the label (`POWER` / `LOCKED`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyPowerView {
    pub power: u32,
    pub fill: f32,
    pub segments: u32,
    pub locked: bool,
    pub label: &'static str,
}

/// The aim reticle view: the target-space coordinate plus its normalized
/// on-screen placement over the goal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyReticleView {
    pub target_x: i32,
    pub target_y: i32,
    pub position: ScreenPos,
    pub radius: f32,
    pub visible: bool,
}

/// The complete deterministic HUD model for a given interaction state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyHudModel {
    pub score: u32,
    pub round_current: u32,
    pub round_total: u32,
    pub best: u32,
    pub power: PenaltyPowerView,
    pub reticle: PenaltyReticleView,
    /// The phase instruction: `AIM` / `HOLD` / `RELEASE` / `FLIGHT` / `CONTACT`
    /// / `ARRIVED`.
    pub instruction: &'static str,
    /// A neutral debug contact label (`HAND` / `TORSO` / `BODY` / `NONE`), only
    /// set when goalie debug visualization is enabled (Pass 6). `None` in
    /// production.
    pub debug_contact: Option<&'static str>,
    /// The final result descriptor once the shot is `Resolved` (Pass 8):
    /// `GOAL` / `SAVE` / `POST` / `MISS` plus an optional detail. `None` until
    /// resolved.
    pub result: Option<PenaltyResultHudDescriptor>,
    /// The most recent score award (Pass 9). `None` for a shot-only HUD.
    pub award: Option<PenaltyScoreAward>,
    /// A between-rounds / session-complete prompt (Pass 9): `CONTINUE` /
    /// `PLAY AGAIN`, or `None` mid-round.
    pub prompt: Option<&'static str>,
    /// Whether the session has finished (Pass 9).
    pub session_complete: bool,
    /// The animated result banner (Pass 10): `GOAL`/`SAVE`/`POST`/`MISS`/
    /// `FINAL SCORE` with a deterministic scale/pulse. `None` when no effect.
    pub banner: Option<PenaltyResultBanner>,
    /// The animated score popup (Pass 10): the awarded `+N` with a pop scale.
    pub score_popup: Option<PenaltyScorePopup>,
}

// Fixed HUD values (Stage 1 scoreboard; static in every pass so far).
pub const STAGE1_SCORE: u32 = 1250;
pub const STAGE1_ROUND_CURRENT: u32 = 3;
pub const STAGE1_ROUND_TOTAL: u32 = 5;
pub const STAGE1_BEST: u32 = 2520;
pub const POWER_SEGMENTS: u32 = 10;
pub const RETICLE_RADIUS: f32 = 0.045;

// The goal-mouth rectangle in normalized screen space that the reticle maps
// into. Centered where the goal is composed; target `(0, 50)` maps to the
// center.
pub const RETICLE_CENTER: ScreenPos = ScreenPos::new(0.5, 0.42);
pub const RETICLE_HALF_WIDTH: f32 = 0.14;
pub const RETICLE_HALF_HEIGHT: f32 = 0.10;

/// Map a target-space aim `(x ∈ [-100,100], y ∈ [0,100])` to its normalized
/// on-screen position over the goal.
pub fn reticle_screen_pos(target_x: i32, target_y: i32) -> ScreenPos {
    let nx = target_x as f32 / 100.0;
    let ny = (target_y as f32 - 50.0) / 50.0; // 0 at center, +1 at top
    ScreenPos::new(
        RETICLE_CENTER.x + nx * RETICLE_HALF_WIDTH,
        RETICLE_CENTER.y - ny * RETICLE_HALF_HEIGHT,
    )
}

impl PenaltyHudModel {
    /// The HUD for the start (default) interaction state.
    pub fn stage1() -> Self {
        Self::from_state(&PenaltyInteractionState::start())
    }

    /// Derive the HUD from an interaction state (pure).
    pub fn from_state(state: &PenaltyInteractionState) -> Self {
        // The power meter freezes (LOCKED) once the shot is committed, through
        // flight and arrival.
        let locked = !matches!(
            state.state,
            PenaltyShotFlightState::Aiming | PenaltyShotFlightState::Charging
        );
        let power = state.power.power.max(0) as u32;
        // Pass 8: once resolved, the HUD shows the final result word.
        let result = state.resolved.map(|r| PenaltyResultHudDescriptor::from_result(r.result));
        let instruction = match state.state {
            PenaltyShotFlightState::Aiming => "AIM",
            PenaltyShotFlightState::Charging => "HOLD",
            PenaltyShotFlightState::LockedPreview => "RELEASE",
            PenaltyShotFlightState::BallInFlight => "FLIGHT",
            PenaltyShotFlightState::ContactDetected => "CONTACT",
            PenaltyShotFlightState::ArrivedAtGoalPlane => "ARRIVED",
            PenaltyShotFlightState::Resolved => result.map(|r| r.result_text).unwrap_or("RESULT"),
        };
        Self {
            score: STAGE1_SCORE,
            round_current: STAGE1_ROUND_CURRENT,
            round_total: STAGE1_ROUND_TOTAL,
            best: STAGE1_BEST,
            power: PenaltyPowerView {
                power,
                fill: power as f32 / POWER_MAX as f32,
                segments: POWER_SEGMENTS,
                locked,
                label: ["POWER", "LOCKED"][locked as usize],
            },
            reticle: PenaltyReticleView {
                target_x: state.aim.target_x,
                target_y: state.aim.target_y,
                position: reticle_screen_pos(state.aim.target_x, state.aim.target_y),
                radius: RETICLE_RADIUS,
                visible: true,
            },
            instruction,
            debug_contact: None,
            result,
            award: None,
            prompt: None,
            session_complete: false,
            banner: None,
            score_popup: None,
        }
    }

    /// Derive the full session HUD (Pass 9 + Pass 10): the shot HUD with the
    /// dynamic score / round / best, the last award, the loop prompt, and the
    /// impact-polish banner + score popup overlaid.
    pub fn from_session(session: &PenaltySessionState) -> Self {
        let base = Self::from_state(&session.shot);
        let prompt = match session.loop_state {
            PenaltyLoopState::SessionComplete => Some("PLAY AGAIN"),
            PenaltyLoopState::BetweenRounds | PenaltyLoopState::RoundAwarded => Some("CONTINUE"),
            _ => None,
        };
        let descriptor = session.effect_descriptor();
        Self {
            score: session.score.score,
            round_current: session.round_number(),
            round_total: crate::soccer_penalty::penalty_session::SESSION_ROUNDS,
            best: session.best.best,
            award: session.last_award,
            prompt,
            session_complete: matches!(session.loop_state, PenaltyLoopState::SessionComplete),
            banner: descriptor.as_ref().map(|d| d.banner),
            score_popup: descriptor.as_ref().and_then(|d| d.score_popup),
            ..base
        }
    }

    /// `"+650"` for the current award, if any.
    pub fn award_text(&self) -> Option<String> {
        self.award.map(|a| format!("+{}", a.total))
    }

    /// `"SCORE 1250"`.
    pub fn score_text(&self) -> String {
        format!("SCORE {}", self.score)
    }

    /// `"ROUND 3 / 5"`.
    pub fn round_text(&self) -> String {
        format!("ROUND {} / {}", self.round_current, self.round_total)
    }

    /// `"BEST 2520"`.
    pub fn best_text(&self) -> String {
        format!("BEST {}", self.best)
    }
}
