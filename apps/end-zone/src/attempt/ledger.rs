//! What happened. The ledger is pure bookkeeping over resolved attempts — it
//! feeds the result card and the session summary and **nothing else**. It never
//! scales the defense, unlocks anything, or changes a rule.
//!
//! It counts two different things, and the distinction is the game's: **yards**,
//! which is what a football game measures, and **moves** — dodges, broken
//! tackles, defenders hurdled — which is what *this* game is about. A
//! twelve-yard carry through three men is a better carry than a twelve-yard
//! carry through nobody, and the ledger is where that is written down.

use crate::events::PlayEndReason;
use crate::identity::PlayerId;

/// How one carry ended. Every variant is something the player can *see* happen
/// on the field — there is no hidden roll behind any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// The back was brought down.
    Tackled,
    /// He carried it across the goal line.
    Touchdown,
    /// He ran out of bounds.
    OutOfBounds,
    /// The exchange never happened — the quarterback was still holding it when
    /// the play died. A muffed handoff, and the honest name for one.
    Botched,
}

impl AttemptOutcome {
    /// The result card's headline.
    pub fn label(self) -> &'static str {
        match self {
            AttemptOutcome::Tackled => "TACKLED",
            AttemptOutcome::Touchdown => "TOUCHDOWN",
            AttemptOutcome::OutOfBounds => "OUT OF BOUNDS",
            AttemptOutcome::Botched => "NO HANDOFF",
        }
    }

    /// Whether the back actually carried the ball.
    pub fn is_carry(self) -> bool {
        !matches!(self, AttemptOutcome::Botched)
    }

    /// Classify a resolved play: who had the ball when the whistle blew decides
    /// whether this was a carry at all, and how it ended decides the rest.
    pub fn classify(
        reason: PlayEndReason,
        carrier: Option<PlayerId>,
        back: PlayerId,
    ) -> Self {
        let back_had_it = carrier == Some(back);
        match (reason, back_had_it) {
            (PlayEndReason::BrokeFree, _) => AttemptOutcome::Touchdown,
            (PlayEndReason::OutOfBounds, _) => AttemptOutcome::OutOfBounds,
            (_, true) => AttemptOutcome::Tackled,
            // Dead behind the line with the back empty-handed: the exchange
            // never happened. (`Incomplete` / `Intercepted` cannot arise on a
            // run — no pass is ever thrown — so they land here too, correctly:
            // whatever went wrong, the back never got it.)
            (_, false) => AttemptOutcome::Botched,
        }
    }
}

/// One resolved attempt — exactly what the result card needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttemptRecord {
    /// 1-based attempt number within the session.
    pub index: u32,
    pub outcome: AttemptOutcome,
    /// Net yards from the line of scrimmage.
    pub yards: f32,
    /// Confirmed successful moves on this carry.
    pub dodges: u32,
    pub broken: u32,
    pub hurdled: u32,
}

impl AttemptRecord {
    /// Every confirmed move on this carry.
    pub fn moves(&self) -> u32 {
        self.dodges + self.broken + self.hurdled
    }
}

/// Running session totals. Read-only outside the controller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttemptLedger {
    pub attempts: u32,
    pub touchdowns: u32,
    pub tackled: u32,
    pub total_yards: f32,
    pub best_yards: f32,
    pub dodges: u32,
    pub broken: u32,
    pub hurdled: u32,
    /// The most recent resolved attempt (the result card's source).
    pub last: Option<AttemptRecord>,
}

impl AttemptLedger {
    pub fn new() -> Self {
        AttemptLedger {
            attempts: 0,
            touchdowns: 0,
            tackled: 0,
            total_yards: 0.0,
            best_yards: 0.0,
            dodges: 0,
            broken: 0,
            hurdled: 0,
            last: None,
        }
    }

    /// Record a resolved attempt.
    pub fn record(&mut self, record: AttemptRecord) {
        self.attempts += 1;
        self.touchdowns += u32::from(record.outcome == AttemptOutcome::Touchdown);
        self.tackled += u32::from(record.outcome == AttemptOutcome::Tackled);
        self.total_yards += record.yards;
        self.best_yards = self.best_yards.max(record.yards);
        self.dodges += record.dodges;
        self.broken += record.broken;
        self.hurdled += record.hurdled;
        self.last = Some(record);
    }

    /// Average yards per carry (zero before the first one resolves).
    pub fn yards_per_attempt(&self) -> f32 {
        match self.attempts {
            0 => 0.0,
            n => self.total_yards / n as f32,
        }
    }

    /// Every confirmed move this session.
    pub fn moves(&self) -> u32 {
        self.dodges + self.broken + self.hurdled
    }

    /// The end-of-session snapshot.
    pub fn summary(&self) -> SessionSummary {
        SessionSummary {
            attempts: self.attempts,
            touchdowns: self.touchdowns,
            best_yards: self.best_yards.max(0.0).round() as u32,
            yards_per_attempt: self.yards_per_attempt(),
            dodges: self.dodges,
            broken: self.broken,
            hurdled: self.hurdled,
        }
    }
}

impl Default for AttemptLedger {
    fn default() -> Self {
        AttemptLedger::new()
    }
}

/// The summary shown when the player ends a session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionSummary {
    pub attempts: u32,
    pub touchdowns: u32,
    pub best_yards: u32,
    pub yards_per_attempt: f32,
    pub dodges: u32,
    pub broken: u32,
    pub hurdled: u32,
}
