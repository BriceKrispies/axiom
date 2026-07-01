//! A ghost's recorded path and its fixed-step replay, composed from kernel
//! primitives.
//!
//! A [`GhostReplay`] is the immutable snapshot of a finished life's successful
//! moves plus a fixed-step cursor. It composes two kernel primitives:
//! [`axiom_kernel::ReplayTimeline`] holds the recorded moves and the replay
//! cursor, and [`axiom_kernel::TickDivider`] paces consumption at one move every
//! [`GHOST_STEP_TICKS`] ticks (0.5 s at the app's 60-tick/second fixed step). No
//! wall clock is read; the cadence is pure tick counting. (Both primitives
//! graduated out of this app into the kernel — see the kernel's `ARCHITECTURE.md`.)
//!
//! ## Consume-on-attempt
//!
//! The cursor advances one move per step window regardless of whether the move
//! succeeds (the caller applies it with the normal collision rules). The
//! original life's moves were all valid when recorded; if a cell is now blocked,
//! the ghost stays put for that step but still consumes the move — the recording
//! plays forward in real time rather than stalling. Once every move is consumed
//! the ghost is *finished* and never moves again until the level is restarted.

use axiom_kernel::{ReplayTimeline, TickDivider};

use crate::zanzoban::direction::Direction;

/// Ticks between a ghost's recorded moves: one move per 0.5 s at the app's
/// 60-tick/second fixed step (`60 / 2 == 30`). The relationship to the fixed
/// step is asserted in `crate::zanzoban::game_state`'s tests, which own the tick rate.
pub const GHOST_STEP_TICKS: u32 = 30;

/// An immutable recorded path plus a fixed-step replay cursor, built from the
/// kernel's [`ReplayTimeline`] (the tape) and [`TickDivider`] (the cadence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostReplay {
    /// The recorded moves and replay cursor.
    tape: ReplayTimeline<Direction>,
    /// The 0.5-second consumption cadence.
    schedule: TickDivider,
}

impl GhostReplay {
    /// Build a replay from a recorded life's moves. The first move is consumed
    /// [`GHOST_STEP_TICKS`] ticks after creation (the ghost spends its first half
    /// second on the entrance, exactly as the player did before their first
    /// move).
    pub fn new(moves: Vec<Direction>) -> Self {
        GhostReplay {
            tape: ReplayTimeline::from_recorded(moves),
            schedule: TickDivider::new(GHOST_STEP_TICKS)
                .expect("GHOST_STEP_TICKS is a non-zero period"),
        }
    }

    /// The full recorded path (immutable).
    pub fn moves(&self) -> &[Direction] {
        self.tape.recorded()
    }

    /// How many moves have been consumed.
    pub fn applied(&self) -> usize {
        self.tape.position()
    }

    /// How many moves remain to be consumed.
    pub fn remaining(&self) -> usize {
        self.tape.remaining()
    }

    /// Has the ghost consumed its entire recorded path? A finished ghost stays
    /// in its final cell forever (until the level is restarted).
    pub fn is_finished(&self) -> bool {
        self.tape.is_finished()
    }

    /// Advance the replay by one fixed tick.
    ///
    /// Returns `Some(direction)` on the tick a move is due (consuming it from the
    /// tape), or `None` between step windows and forever once finished. The
    /// caller applies the returned direction with the normal collision rules;
    /// the move is consumed whether or not it physically succeeds.
    pub fn advance_tick(&mut self) -> Option<Direction> {
        let due = self.schedule.advance();
        due.then(|| self.tape.advance().copied()).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_replay_is_immediately_finished() {
        let mut r = GhostReplay::new(vec![]);
        assert!(r.is_finished());
        assert_eq!(r.advance_tick(), None);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn consumes_one_move_every_step_window() {
        let mut r = GhostReplay::new(vec![Direction::Right, Direction::Up]);
        for _ in 0..(GHOST_STEP_TICKS - 1) {
            assert_eq!(r.advance_tick(), None);
        }
        assert_eq!(r.advance_tick(), Some(Direction::Right));
        assert_eq!(r.applied(), 1);
        assert_eq!(r.remaining(), 1);
        for _ in 0..(GHOST_STEP_TICKS - 1) {
            assert_eq!(r.advance_tick(), None);
        }
        assert_eq!(r.advance_tick(), Some(Direction::Up));
        assert!(r.is_finished());
    }

    #[test]
    fn finished_ghost_never_moves_again() {
        let mut r = GhostReplay::new(vec![Direction::Right]);
        for _ in 0..GHOST_STEP_TICKS {
            r.advance_tick();
        }
        assert!(r.is_finished());
        for _ in 0..1000 {
            assert_eq!(r.advance_tick(), None);
        }
    }

    #[test]
    fn exposes_the_recorded_path() {
        let r = GhostReplay::new(vec![Direction::Left, Direction::Down]);
        assert_eq!(r.moves(), &[Direction::Left, Direction::Down]);
        assert_eq!(r.applied(), 0);
    }
}
