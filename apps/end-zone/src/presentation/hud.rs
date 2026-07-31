//! The gameplay HUD view model: the minimal read-out the prototype needs,
//! derived purely from the authoritative attempt state. The platform edge
//! renders these strings; it never computes them.
//!
//! Deliberately **not** shown: how open any read is. The decision window tells
//! the player *which key throws to whom* and how long they have; judging who is
//! actually open is the entire game, and a green/red hint would answer the
//! question the prototype exists to ask.

use crate::attempt::{AttemptLedger, AttemptPhase, AttemptStep};
use crate::data::prototype::{concept, CONCEPT_COUNT, READ_COUNT};

/// One selectable read in the decision prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadPrompt {
    /// The key that throws it (`1`, `2`, `3`).
    pub key: String,
    /// The route's name (`SLANT`).
    pub name: String,
}

/// One callable play on the pre-snap card.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayOption {
    /// The key that calls it (`1`, `2`, `3`).
    pub key: String,
    /// The concept's name (`TRIPLE READ`).
    pub name: String,
    /// Its three routes in read order (`SLANT · DIG · POST`), so the player is
    /// choosing between three described shapes rather than three names.
    pub routes: String,
}

/// The pre-snap play card: the whole pre-snap decision, and a blocking one.
///
/// It is its own view model rather than a dressed-up [`DecisionPrompt`] because
/// it asks a different question with different stakes and — crucially — **no
/// timer**. Nothing about it drains, so it carries no `remaining` and no
/// urgency; borrowing the prompt's shape would have meant a bar that stands
/// permanently full, which reads as broken rather than as unhurried.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayCallCard {
    /// The card's headline (`CALL THE PLAY`).
    pub headline: String,
    pub plays: Vec<PlayOption>,
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
    /// The play card, only while the offense is waiting on a call.
    pub play_call: Option<PlayCallCard>,
    /// The decision prompt, only once the ball is live.
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
        AttemptPhase::PlayCall => "CALL IT",
        AttemptPhase::Shifting { .. } => "SET",
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
            play_call: play_call_card(step),
            decision: decision_prompt(step),
            result: result_card(step),
        }
    }
}

/// The read prompt, once the ball is live (`None` otherwise).
fn decision_prompt(step: &AttemptStep) -> Option<DecisionPrompt> {
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
            })
            .collect(),
        // Action first, key second: the same string is a keyboard hint and a
        // tappable button caption, and on a phone the key half is just noise.
        scramble: "SCRAMBLE  ·  SPACE".to_string(),
        remaining,
        urgent: window.is_some(),
    })
}

/// The pre-snap play card, while the offense is waiting on a call.
///
/// Nothing is pre-selected: the attempt does not start until the player calls
/// something, so showing a standing highlight would suggest a play is already
/// in when nothing has been chosen. The number row keeps one grammar across the
/// whole attempt — `1`/`2`/`3` are the three plays here and the three reads once
/// the ball is live, so the keys change meaning only when the phase does.
fn play_call_card(step: &AttemptStep) -> Option<PlayCallCard> {
    matches!(step.phase, AttemptPhase::PlayCall).then(|| PlayCallCard {
        headline: "CALL THE PLAY".to_string(),
        plays: (0..CONCEPT_COUNT)
            .map(|index| PlayOption {
                key: format!("{}", index + 1),
                name: concept(index).name.to_string(),
                routes: concept(index).read_names.join("  ·  "),
            })
            .collect(),
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
