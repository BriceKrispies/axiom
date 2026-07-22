//! Commitment locking (hysteresis) and the persistent AI memory. The arbiter is
//! the ONLY writer of a player's [`Commitment`]: it keeps the current action
//! unless it becomes invalid, an emergency (higher priority band) preempts it,
//! or its minimum-commitment window has lapsed AND a meaningfully better action
//! exists. This is what stops players thrashing between actions every tick.

use crate::config::PLAYER_COUNT;
use crate::football::BallSituation;

use super::action::{Priority, ScoredAction};
use super::perception::Responsibility;
use super::PlayerIntent;

/// How much higher (in within-band score) a same-band action must be to preempt
/// a live commitment once its minimum window has lapsed. An emergency (a higher
/// band) always preempts regardless of this margin.
pub const SWITCH_MARGIN: f32 = 120.0;

/// One player's locked-in action and when it was taken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Commitment {
    pub intent: PlayerIntent,
    pub priority: Priority,
    pub reason: &'static str,
    pub since_tick: u64,
    pub min_ticks: u32,
}

/// The only persistent AI state carried across ticks: each player's commitment,
/// the quarterback scramble detector's counter, and the cached ball situation
/// (for the debug overlay and tests). Reset with `roles`/`intents` on every
/// (re)formation.
#[derive(Debug, Clone)]
pub struct AiMemory {
    pub commitments: [Option<Commitment>; PLAYER_COUNT],
    /// Consecutive ticks the quarterback has shown run intent outside the
    /// pocket; the scramble commit fires once it crosses the tuned threshold.
    pub qb_downfield_ticks: u32,
    /// The situation derived this tick (cached for debug/tests).
    pub situation: BallSituation,
    /// This tick's coordinated defensive responsibilities (cached for the debug
    /// overlay + tests), indexed by [`crate::identity::PlayerId`].
    pub responsibilities: [Responsibility; PLAYER_COUNT],
}

impl AiMemory {
    pub fn new() -> Self {
        AiMemory {
            commitments: [None; PLAYER_COUNT],
            qb_downfield_ticks: 0,
            situation: BallSituation::PreSnap,
            responsibilities: [Responsibility::None; PLAYER_COUNT],
        }
    }

    /// Clear every commitment and the scramble counter (a fresh formation).
    pub fn reset(&mut self) {
        *self = AiMemory::new();
    }

    /// Ticks of committed action `player` has left before it may freely switch
    /// (0 when uncommitted) — a debug read-out.
    pub fn commitment_ticks_left(&self, player: usize, tick: u64) -> u32 {
        self.commitments[player]
            .map(|c| {
                let elapsed = tick.saturating_sub(c.since_tick) as u32;
                c.min_ticks.saturating_sub(elapsed)
            })
            .unwrap_or(0)
    }
}

impl Default for AiMemory {
    fn default() -> Self {
        AiMemory::new()
    }
}

/// Pick one action from this tick's candidates under commitment locking, and
/// update `slot`. A user-controlled player takes the top action but holds NO
/// commitment (so releasing the stick never resumes a stale AI lock).
pub fn arbitrate(
    candidates: &[ScoredAction],
    slot: &mut Option<Commitment>,
    tick: u64,
    user_controlled: bool,
) -> PlayerIntent {
    let top = pick_top(candidates);
    if user_controlled {
        *slot = None;
        return top.map(|t| t.intent).unwrap_or(PlayerIntent::Hold);
    }
    let Some(top) = top else {
        *slot = None;
        return PlayerIntent::Hold;
    };
    if let Some(current) = slot {
        let locked = tick.saturating_sub(current.since_tick) < u64::from(current.min_ticks);
        let emergency = top.priority > current.priority;
        // A live candidate for the SAME action keeps the commitment valid and
        // refreshes its target point (so a lock still tracks a moving target).
        let refreshed = candidates
            .iter()
            .copied()
            .find(|s| s.intent.same_action(&current.intent));
        if let Some(fresh) = refreshed {
            let preempt = emergency || (!locked && top.score() > fresh.score() + SWITCH_MARGIN);
            if !preempt {
                current.intent = fresh.intent;
                current.priority = fresh.priority;
                current.reason = fresh.reason;
                return current.intent;
            }
        }
        // Otherwise the committed action is invalid (no longer offered) or
        // preempted — fall through and switch to the top action.
    }
    *slot = Some(Commitment {
        intent: top.intent,
        priority: top.priority,
        reason: top.reason,
        since_tick: tick,
        min_ticks: top.min_ticks,
    });
    top.intent
}

/// The highest-scoring candidate, first-wins on ties (candidate order is the
/// deterministic generation order).
fn pick_top(candidates: &[ScoredAction]) -> Option<ScoredAction> {
    candidates
        .iter()
        .copied()
        .reduce(|a, b| if b.score() > a.score() { b } else { a })
}
