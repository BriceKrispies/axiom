//! What happened. The ledger is pure bookkeeping over resolved attempts — it
//! feeds the result card and the session summary and **nothing else**. It never
//! scales the defense, unlocks anything, or changes a rule: this prototype is
//! testing whether the decision is fun, not building a progression.

use crate::events::PlayEndReason;
use crate::identity::PlayerId;

/// How one attempt ended. Every variant is something the player can *see*
/// happen on the field — there is no hidden roll behind any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// Caught, then run down or out of bounds.
    Complete,
    /// Caught and carried across the goal line.
    Touchdown,
    /// The pass hit the turf — thrown away, broken up, or simply missed.
    Incomplete,
    /// A defender took it. The worst outcome, and the price of the deep read.
    Intercepted,
    /// The rush got home while the quarterback still had the ball.
    Sacked,
    /// The quarterback ran it himself and was brought down.
    Scramble,
    /// The quarterback scrambled across the goal line.
    ScrambleTouchdown,
}

impl AttemptOutcome {
    /// The result card's headline.
    pub fn label(self) -> &'static str {
        match self {
            AttemptOutcome::Complete => "COMPLETE",
            AttemptOutcome::Touchdown => "TOUCHDOWN",
            AttemptOutcome::Incomplete => "INCOMPLETE",
            AttemptOutcome::Intercepted => "INTERCEPTED",
            AttemptOutcome::Sacked => "SACKED",
            AttemptOutcome::Scramble => "SCRAMBLE",
            AttemptOutcome::ScrambleTouchdown => "SCRAMBLE TD",
        }
    }

    /// Whether the offense kept the ball and gained ground.
    pub fn is_gain(self) -> bool {
        matches!(
            self,
            AttemptOutcome::Complete
                | AttemptOutcome::Touchdown
                | AttemptOutcome::Scramble
                | AttemptOutcome::ScrambleTouchdown
        )
    }

    /// Whether the ball was thrown and caught by the intended receiver.
    pub fn is_completion(self) -> bool {
        matches!(self, AttemptOutcome::Complete | AttemptOutcome::Touchdown)
    }

    /// Classify a resolved play from what the player did and what the field
    /// shows. `threw` is the read committed to (if any), `scrambled` is whether
    /// the player took the quarterback out of the pocket himself, and `carrier`
    /// is whoever held the ball when the whistle blew.
    ///
    /// The distinction that matters: a quarterback tackled with the ball is a
    /// **sack** if he never chose to run, and a **scramble** if he did. Same
    /// end state, opposite readings of the decision.
    pub fn classify(
        reason: PlayEndReason,
        threw: Option<usize>,
        scrambled: bool,
        carrier: Option<PlayerId>,
        quarterback: PlayerId,
    ) -> Self {
        let qb_has_it = carrier == Some(quarterback);
        match reason {
            PlayEndReason::Intercepted => AttemptOutcome::Intercepted,
            PlayEndReason::Incomplete => AttemptOutcome::Incomplete,
            PlayEndReason::BrokeFree => match qb_has_it {
                true => AttemptOutcome::ScrambleTouchdown,
                false => AttemptOutcome::Touchdown,
            },
            // Tackled / out of bounds: who had the ball, and why.
            PlayEndReason::Tackled | PlayEndReason::OutOfBounds => match (qb_has_it, threw) {
                (true, _) if scrambled => AttemptOutcome::Scramble,
                (true, _) => AttemptOutcome::Sacked,
                (false, Some(_)) => AttemptOutcome::Complete,
                // A receiver has it with no throw recorded: the loop lost track
                // of the decision, but the offense still ran the ball.
                (false, None) => AttemptOutcome::Scramble,
            },
        }
    }
}

/// One resolved attempt — exactly what the result card needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttemptRecord {
    /// 1-based attempt number within the session.
    pub index: u32,
    pub outcome: AttemptOutcome,
    /// Net yards from the line of scrimmage (negative on a sack).
    pub yards: f32,
    /// The read the player threw, if they threw one.
    pub read: Option<usize>,
    /// How many decision windows this attempt offered before it resolved.
    pub windows: u32,
    /// Whether the player let every window close without choosing.
    pub declined: bool,
}

/// Running session totals. Read-only outside the controller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttemptLedger {
    pub attempts: u32,
    pub completions: u32,
    pub touchdowns: u32,
    pub interceptions: u32,
    pub sacks: u32,
    pub scrambles: u32,
    pub total_yards: f32,
    pub best_yards: f32,
    /// The most recent resolved attempt (the result card's source).
    pub last: Option<AttemptRecord>,
}

impl AttemptLedger {
    pub fn new() -> Self {
        AttemptLedger {
            attempts: 0,
            completions: 0,
            touchdowns: 0,
            interceptions: 0,
            sacks: 0,
            scrambles: 0,
            total_yards: 0.0,
            best_yards: 0.0,
            last: None,
        }
    }

    /// Record a resolved attempt.
    pub fn record(&mut self, record: AttemptRecord) {
        self.attempts += 1;
        self.completions += u32::from(record.outcome.is_completion());
        self.touchdowns += u32::from(matches!(
            record.outcome,
            AttemptOutcome::Touchdown | AttemptOutcome::ScrambleTouchdown
        ));
        self.interceptions += u32::from(record.outcome == AttemptOutcome::Intercepted);
        self.sacks += u32::from(record.outcome == AttemptOutcome::Sacked);
        self.scrambles += u32::from(matches!(
            record.outcome,
            AttemptOutcome::Scramble | AttemptOutcome::ScrambleTouchdown
        ));
        self.total_yards += record.yards;
        self.best_yards = self.best_yards.max(record.yards);
        self.last = Some(record);
    }

    /// Average yards per attempt (zero before the first one resolves).
    pub fn yards_per_attempt(&self) -> f32 {
        match self.attempts {
            0 => 0.0,
            n => self.total_yards / n as f32,
        }
    }

    /// The end-of-session snapshot.
    pub fn summary(&self) -> SessionSummary {
        SessionSummary {
            attempts: self.attempts,
            completions: self.completions,
            touchdowns: self.touchdowns,
            interceptions: self.interceptions,
            sacks: self.sacks,
            best_yards: self.best_yards.max(0.0).round() as u32,
            yards_per_attempt: self.yards_per_attempt(),
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
    pub completions: u32,
    pub touchdowns: u32,
    pub interceptions: u32,
    pub sacks: u32,
    pub best_yards: u32,
    pub yards_per_attempt: f32,
}
