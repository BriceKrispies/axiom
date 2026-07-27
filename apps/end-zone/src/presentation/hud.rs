//! The gameplay HUD view model: the minimal read-out the prototype needs,
//! derived purely from the authoritative attempt state. The platform edge
//! renders these strings; it never computes them.
//!
//! Deliberately **not** shown: how open any read is. The decision window tells
//! the player *which key throws to whom* and how long they have; judging who is
//! actually open is the entire game, and a green/red hint would answer the
//! question the prototype exists to ask.

use crate::attempt::{AttemptLedger, AttemptPhase, AttemptStep, SET_TICKS};
use crate::data::prototype::{concept, READ_COUNT};

/// One selectable read in the decision prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadPrompt {
    /// The key that throws it (`1`, `2`, `3`).
    pub key: String,
    /// The route's name (`SLANT`).
    pub name: String,
    /// How far this read's wind-up has charged, `0..=1`. Zero unless this is
    /// the read currently being held — the chip's fill IS the power meter.
    pub charge: f32,
}

/// The on-screen read prompt. Present for the whole live play, because the
/// reads are selectable for the whole live play — a control you can use is a
/// control you should be able to see.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionPrompt {
    /// Why the window opened (`READ IT` / `PRESSURE` / `THROW IT`), or the
    /// standing caption while the play is merely developing.
    pub headline: String,
    pub reads: Vec<ReadPrompt>,
    /// The scramble control's caption.
    pub scramble: String,
    /// `0..1` of the window still available — the timer bar's fill. Full while
    /// no window is open, since nothing is running out yet.
    pub remaining: f32,
    /// Whether a decision window is open: the game has slowed down and the
    /// clock is running. Drives the prompt's emphasis, so the moment the
    /// player is being *asked* is unmistakable from the moment they merely
    /// *may* act.
    pub urgent: bool,
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
    // Pre-snap the same three chips pick the PLAY; the keys never change
    // meaning mid-decision, only mid-phase.
    if matches!(step.phase, AttemptPhase::PreSnap { .. }) {
        return Some(concept_prompt(step));
    }
    if !step.phase.accepts_choice() {
        return None;
    }
    // In a window the headline says why it opened and the bar drains. Outside
    // one the same three chips stand, unhurried, with the bar full: the player
    // may throw at any time, they are simply not being pressed to.
    let window = match step.phase {
        AttemptPhase::DecisionWindow {
            opened_at,
            closes_at,
            trigger,
        } => {
            let span = closes_at.saturating_sub(opened_at).max(1) as f32;
            let left = (step.window_left as f32 / span).clamp(0.0, 1.0);
            Some((trigger.label(), left))
        }
        _ => None,
    };
    let (headline, remaining) = window.unwrap_or(("YOUR CALL", 1.0));
    Some(DecisionPrompt {
        headline: headline.to_string(),
        reads: (0..READ_COUNT)
            .map(|read| ReadPrompt {
                key: format!("{}", read + 1),
                name: concept(step.read.concept).read_names[read].to_string(),
                charge: match step.charging == Some(read) {
                    true => step.charge,
                    false => 0.0,
                },
            })
            .collect(),
        // Action first, key second: the same string is a keyboard hint and a
        // tappable button caption, and on a phone the key half is just noise.
        scramble: "SCRAMBLE  ·  SPACE".to_string(),
        remaining,
        urgent: window.is_some(),
    })
}

/// The pre-snap play picker: the three concepts, keyed like the three reads, so
/// the number row never changes meaning mid-decision — only mid-phase. The
/// currently-set concept shows a full chip so the standing call is always
/// visible without a separate readout.
fn concept_prompt(step: &AttemptStep) -> DecisionPrompt {
    DecisionPrompt {
        headline: "CALL IT".to_string(),
        reads: (0..crate::data::prototype::CONCEPT_COUNT)
            .map(|index| ReadPrompt {
                key: format!("{}", index + 1),
                name: concept(index).name.to_string(),
                charge: match index == step.concept {
                    true => 1.0,
                    false => 0.0,
                },
            })
            .collect(),
        scramble: format!("SET  ·  {}", concept(step.concept).name),
        remaining: (step.window_left as f32 / SET_TICKS as f32).clamp(0.0, 1.0),
        // The hold is generous on purpose; it is a beat to think in, not a
        // scramble, so it never nags.
        urgent: false,
    }
}

/// The result card while one is showing (`None` otherwise).
fn result_card(step: &AttemptStep) -> Option<String> {
    let showing = matches!(
        step.phase,
        AttemptPhase::Result { .. } | AttemptPhase::Resetting
    );
    let record = step.last.filter(|_| showing)?;
    let detail = match record.read {
        Some(read) => format!(
            "   {}",
            concept(step.read.concept).read_names[read.min(READ_COUNT - 1)]
        ),
        None => String::new(),
    };
    Some(format!(
        "{}   {}{detail}",
        record.outcome.label(),
        yards(record.yards)
    ))
}
