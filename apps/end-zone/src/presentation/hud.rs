//! The gameplay HUD view model: the minimal read-out the prototype needs,
//! derived purely from the authoritative attempt state. The platform edge
//! renders these strings; it never computes them.
//!
//! Deliberately **not** shown: how open any read is. The decision window tells
//! the player *which key throws to whom* and how long they have; judging who is
//! actually open is the entire game, and a green/red hint would answer the
//! question the prototype exists to ask.

use crate::attempt::{AttemptLedger, AttemptPhase, AttemptStep};
use crate::data::prototype::{READ_COUNT, READ_NAMES};

/// One selectable read in the decision prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPrompt {
    /// The key that throws it (`1`, `2`, `3`).
    pub key: String,
    /// The route's name (`QUICK OUT`).
    pub name: String,
}

/// The decision window's on-screen prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionPrompt {
    /// Why the window opened (`READ IT` / `PRESSURE` / `THROW IT`).
    pub headline: String,
    pub reads: Vec<ReadPrompt>,
    /// The scramble control's caption.
    pub scramble: String,
    /// `0..1` of the window still available — the timer bar's fill.
    pub remaining: f32,
}

/// The formatted read-out for one tick of a live session.
#[derive(Debug, Clone, PartialEq)]
pub struct HudView {
    /// `ATTEMPT 007`.
    pub attempt: String,
    /// `AVG 6.2   BEST 31   INT 2`.
    pub session: String,
    /// What the play is doing right now (`WATCH`, `BALL IN AIR`, `SCRAMBLE`).
    pub state: String,
    /// The decision prompt, only while a window is open.
    pub decision: Option<DecisionPrompt>,
    /// The result card, only while one is showing (`COMPLETE  +14 YD`).
    pub result: Option<String>,
}

/// Signed yards, arcade-formatted (`+14 YD`, `-7 YD`, `0 YD`).
fn yards(value: f32) -> String {
    let rounded = value.round() as i32;
    match rounded {
        0 => "0 YD".to_string(),
        n if n > 0 => format!("+{n} YD"),
        n => format!("{n} YD"),
    }
}

/// The caption for a phase that is not asking anything of the player.
fn state_caption(phase: AttemptPhase) -> &'static str {
    match phase {
        AttemptPhase::PreSnap { .. } => "SET",
        AttemptPhase::Developing => "WATCH",
        AttemptPhase::DecisionWindow { .. } => "DECIDE",
        AttemptPhase::PassInFlight { .. } => "BALL IN AIR",
        AttemptPhase::Scrambling => "SCRAMBLE",
        AttemptPhase::Resolving | AttemptPhase::Result { .. } => "WHISTLE",
        AttemptPhase::Resetting => "NEXT UP",
    }
}

impl HudView {
    /// Derive the read-out from the attempt loop's view plus session totals.
    pub fn from_attempt(step: &AttemptStep, ledger: &AttemptLedger) -> Self {
        HudView {
            attempt: format!("ATTEMPT {:03}", step.attempt),
            session: format!(
                "AVG {:.1}   BEST {}   INT {}",
                ledger.yards_per_attempt(),
                ledger.best_yards.max(0.0).round() as i32,
                ledger.interceptions
            ),
            state: state_caption(step.phase).to_string(),
            decision: decision_prompt(step),
            result: result_card(step),
        }
    }
}

/// The prompt for an open window (`None` otherwise).
fn decision_prompt(step: &AttemptStep) -> Option<DecisionPrompt> {
    let AttemptPhase::DecisionWindow {
        opened_at,
        closes_at,
        trigger,
    } = step.phase
    else {
        return None;
    };
    let span = closes_at.saturating_sub(opened_at).max(1) as f32;
    Some(DecisionPrompt {
        headline: trigger.label().to_string(),
        reads: (0..READ_COUNT)
            .map(|read| ReadPrompt {
                key: format!("{}", read + 1),
                name: READ_NAMES[read].to_string(),
            })
            .collect(),
        // Action first, key second: the same string is a keyboard hint and a
        // tappable button caption, and on a phone the key half is just noise.
        scramble: "SCRAMBLE  ·  SPACE".to_string(),
        remaining: (step.window_left as f32 / span).clamp(0.0, 1.0),
    })
}

/// The result card while one is showing (`None` otherwise).
fn result_card(step: &AttemptStep) -> Option<String> {
    let showing = matches!(
        step.phase,
        AttemptPhase::Result { .. } | AttemptPhase::Resetting
    );
    let record = step.last.filter(|_| showing)?;
    let detail = match record.read {
        Some(read) => format!("   {}", READ_NAMES[read.min(READ_COUNT - 1)]),
        None => String::new(),
    };
    Some(format!(
        "{}   {}{detail}",
        record.outcome.label(),
        yards(record.yards)
    ))
}
