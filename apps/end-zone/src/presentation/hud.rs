//! The gameplay HUD view model: the minimal read-out the run game needs,
//! derived purely from the authoritative attempt state. The platform edge
//! renders these strings; it never computes them.
//!
//! It is deliberately small. The player's eyes are on a defender eight yards in
//! front of them, and every glyph that is not about *this encounter* is a glyph
//! that costs them the encounter. So there are exactly four things: which
//! attempt this is, the session line, what the play is doing, and the four
//! moves — plus a single pip that says whether the leap is ready, because that
//! is the one control whose availability the player cannot otherwise see.
//!
//! Deliberately **not** shown: any hint about whether a defender can be run
//! through, jumped, or should be juked. Reading that off the geometry is the
//! entire game, and a green/red marker would answer the question the game
//! exists to ask.

use crate::attempt::{AttemptLedger, AttemptPhase, AttemptStep};
use crate::data::concept::{concept, CONCEPT_COUNT};
use crate::events::RunbackMoveCode;

/// One callable play on the pre-snap card.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayOption {
    /// The key that calls it (`1`, `2`, `3`).
    pub key: String,
    /// The concept's name (`OFF TACKLE`).
    pub name: String,
    /// The hole it opens (`OUTSIDE THE RIGHT GUARD`), so the player is choosing
    /// between three described shapes rather than three names.
    pub routes: String,
}

/// The pre-snap play card: the whole pre-snap decision, and a blocking one.
///
/// It carries **no timer**, because nothing about it drains: the call waits for
/// the player however long they take.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayCallCard {
    /// The card's headline (`CALL THE PLAY`).
    pub headline: String,
    pub plays: Vec<PlayOption>,
}

/// One of the running back's moves, as the player sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveHint {
    /// The desktop key (`A`, `D`, `S`, `W`).
    pub key: String,
    /// The swipe that does the same thing (`◀`, `▶`, `▼`, `▲`) — the same row
    /// serves both surfaces, so a phone and a keyboard can never be told
    /// different things about what the game does.
    pub swipe: String,
    /// What it does (`JUKE LEFT`).
    pub name: String,
    /// Whether it can be used right now. Only the leap is ever false, and only
    /// while it is on cooldown or the back is already in the air.
    pub ready: bool,
    /// `0..1` of the leap's cooldown still to run — the pip's drain. `0` for
    /// every move that has no cooldown.
    pub cooldown: f32,
}

/// The formatted read-out for one tick of a live session.
#[derive(Debug, Clone, PartialEq)]
pub struct HudView {
    /// `CARRY 007`.
    pub attempt: String,
    /// `AVG 6.2   BEST 31   MOVES 9`.
    pub session: String,
    /// What the play is doing right now (`CALL IT`, `HANDOFF`, `RUN`).
    pub state: String,
    /// The play card, only while the offense is waiting on a call.
    pub play_call: Option<PlayCallCard>,
    /// The four moves, from the exchange onward.
    pub moves: Vec<MoveHint>,
    /// A confirmed successful move, for a beat after it lands (`BROKE TACKLE`).
    /// The whole of the success feedback: one line, gone in under a second.
    pub flash: Option<String>,
    /// The result card, only while one is showing (`TOUCHDOWN   +40 YD`).
    pub result: Option<String>,
}

/// How long a success flash stays on screen, ticks (~0.75 s).
const FLASH_TICKS: u64 = 45;

/// Signed yards, arcade-formatted (`+14 YD`, `-7 YD`, `0 YD`).
fn yards(value: f32) -> String {
    let rounded = value.round() as i32;
    match rounded {
        0 => "0 YD".to_string(),
        n if n > 0 => format!("+{n} YD"),
        n => format!("{n} YD"),
    }
}

/// The caption for what the play is doing.
fn state_caption(phase: AttemptPhase) -> &'static str {
    match phase {
        AttemptPhase::PlayCall => "CALL IT",
        AttemptPhase::Shifting { .. } => "SET",
        AttemptPhase::Mesh { .. } => "SNAP",
        AttemptPhase::Exchange => "HANDOFF",
        AttemptPhase::Carrying => "RUN",
        AttemptPhase::Resolving | AttemptPhase::Result { .. } => "WHISTLE",
        AttemptPhase::Resetting => "NEXT UP",
    }
}

/// The headline for a confirmed success.
fn success_caption(code: RunbackMoveCode) -> &'static str {
    match code {
        RunbackMoveCode::JukeLeft | RunbackMoveCode::JukeRight => "DODGED",
        RunbackMoveCode::Shoulder => "BROKE TACKLE",
        RunbackMoveCode::Jump => "HURDLED",
    }
}

impl HudView {
    /// Derive the read-out from the attempt loop's view plus session totals.
    /// `tick` is the simulation tick, so the success flash fades on game time
    /// like everything else.
    pub fn from_attempt(step: &AttemptStep, ledger: &AttemptLedger, tick: u64) -> Self {
        HudView {
            attempt: format!("CARRY {:03}", step.attempt),
            session: format!(
                "AVG {:.1}   BEST {}   MOVES {}",
                ledger.yards_per_attempt(),
                ledger.best_yards.max(0.0).round() as i32,
                ledger.moves()
            ),
            state: state_caption(step.phase).to_string(),
            play_call: play_call_card(step),
            moves: move_hints(step),
            flash: flash(step, tick),
            result: result_card(step),
        }
    }
}

/// The four moves, from the exchange onward (`None` — an empty row — before it).
fn move_hints(step: &AttemptStep) -> Vec<MoveHint> {
    if !step.phase.shows_moves() {
        return Vec::new();
    }
    let cooldown = match step.runback.jump_cooldown_left {
        0 => 0.0,
        left => (left as f32 / crate::data::RunbackTuning::default().jump_cooldown_ticks as f32)
            .clamp(0.0, 1.0),
    };
    vec![
        MoveHint {
            key: "A".to_string(),
            swipe: "◀".to_string(),
            name: RunbackMoveCode::JukeLeft.label().to_string(),
            ready: true,
            cooldown: 0.0,
        },
        MoveHint {
            key: "D".to_string(),
            swipe: "▶".to_string(),
            name: RunbackMoveCode::JukeRight.label().to_string(),
            ready: true,
            cooldown: 0.0,
        },
        MoveHint {
            key: "S".to_string(),
            swipe: "▼".to_string(),
            name: RunbackMoveCode::Shoulder.label().to_string(),
            ready: true,
            cooldown: 0.0,
        },
        MoveHint {
            key: "W".to_string(),
            swipe: "▲".to_string(),
            name: RunbackMoveCode::Jump.label().to_string(),
            ready: step.runback.jump_available,
            cooldown,
        },
    ]
}

/// The success flash, for a beat after a confirmed move.
fn flash(step: &AttemptStep, tick: u64) -> Option<String> {
    step.runback
        .last_success
        .filter(|(_, at)| tick.saturating_sub(*at) < FLASH_TICKS)
        .map(|(code, _)| success_caption(code).to_string())
}

/// The pre-snap play card, while the offense is waiting on a call.
///
/// Nothing is pre-selected: the attempt does not start until the player calls
/// something, so showing a standing highlight would suggest a play is already in
/// when nothing has been chosen.
fn play_call_card(step: &AttemptStep) -> Option<PlayCallCard> {
    step.phase.accepts_call().then(|| PlayCallCard {
        headline: "CALL THE PLAY".to_string(),
        plays: (0..CONCEPT_COUNT)
            .map(|index| PlayOption {
                key: format!("{}", index + 1),
                name: concept(index).name.to_string(),
                routes: concept(index).blurb.to_string(),
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
    // The moves are part of the result, not a footnote: a twelve-yard carry
    // through three men is a different carry from a twelve-yard carry through
    // nobody, and this is where the game says so.
    let detail = match record.moves() {
        0 => String::new(),
        n => format!("   {n} MOVES"),
    };
    Some(format!(
        "{}   {}{detail}",
        record.outcome.label(),
        yards(record.yards)
    ))
}
