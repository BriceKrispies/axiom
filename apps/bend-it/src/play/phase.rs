//! The explicit gameplay state, and the commands that move it.
//!
//! One enum, not a drawer of booleans. It is deliberately short: the game is
//! *draw a line, watch the shot*, and every stage that was really a planning
//! step rather than a moment of play is gone.

/// Where an attempt is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// A beat to settle after a reset, before the pitch accepts a drawing.
    Ready,
    /// Drawing. The line is live and nothing has been decided.
    Aiming,
    /// The line has been read. It flicks away, the kicker starts moving.
    ShotReady,
    /// The run-up. The ball leaves on the contact tick and not before.
    Kicking,
    /// The ball is on the authored path, until something physically hits it.
    BallInFlight,
    /// The result is up.
    Resolution,
    /// The wipe back to a fresh attempt.
    Reset,
}

impl Phase {
    /// Whether a drawing is accepted right now.
    pub fn accepts_drawing(self) -> bool {
        matches!(self, Phase::Aiming)
    }

    /// Whether a **dive** is accepted right now.
    ///
    /// From the moment the rival starts moving to the instant the ball crosses
    /// the line. The window is deliberately wide, because its width *is* the
    /// decision: go early and you are guessing with a whole dive behind you, wait
    /// and you know more with less than a third of a second to use it. Narrowing
    /// it to "after the strike" would delete the bet and leave a reaction test.
    pub fn accepts_dive(self) -> bool {
        matches!(self, Phase::ShotReady | Phase::Kicking | Phase::BallInFlight)
    }

    /// Whether the authored path is drawn in the world this phase.
    ///
    /// Only during the commit beat: while the player is still drawing there is
    /// nothing authored yet, and once the ball moves the ball *is* the preview.
    pub fn shows_preview(self) -> bool {
        matches!(self, Phase::ShotReady)
    }

    /// Whether the shot has been decided.
    pub fn committed(self) -> bool {
        matches!(
            self,
            Phase::ShotReady | Phase::Kicking | Phase::BallInFlight | Phase::Resolution
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_aiming_stage_takes_a_drawing() {
        assert!(Phase::Aiming.accepts_drawing());
        [
            Phase::Ready,
            Phase::ShotReady,
            Phase::Kicking,
            Phase::BallInFlight,
            Phase::Resolution,
            Phase::Reset,
        ]
        .iter()
        .for_each(|p| assert!(!p.accepts_drawing(), "{p:?} must not take a drawing"));
    }

    #[test]
    fn a_dive_may_be_called_from_the_run_up_until_the_ball_crosses() {
        [Phase::ShotReady, Phase::Kicking, Phase::BallInFlight]
            .iter()
            .for_each(|p| assert!(p.accepts_dive(), "{p:?} must take a dive"));
        [Phase::Ready, Phase::Aiming, Phase::Resolution, Phase::Reset]
            .iter()
            .for_each(|p| assert!(!p.accepts_dive(), "{p:?} must not take a dive"));
        // The two windows never overlap: at any moment the player is either
        // drawing a shot or calling a dive, never both.
        [
            Phase::Ready,
            Phase::Aiming,
            Phase::ShotReady,
            Phase::Kicking,
            Phase::BallInFlight,
            Phase::Resolution,
            Phase::Reset,
        ]
        .iter()
        .for_each(|p| assert!(!(p.accepts_dive() & p.accepts_drawing()), "{p:?}"));
    }

    #[test]
    fn the_preview_exists_only_in_the_beat_between_the_line_and_the_kick() {
        assert!(Phase::ShotReady.shows_preview());
        [
            Phase::Ready,
            Phase::Aiming,
            Phase::Kicking,
            Phase::BallInFlight,
            Phase::Resolution,
            Phase::Reset,
        ]
        .iter()
        .for_each(|p| assert!(!p.shows_preview(), "{p:?} must not show a preview"));
    }

    #[test]
    fn everything_after_the_line_counts_as_committed() {
        [
            Phase::ShotReady,
            Phase::Kicking,
            Phase::BallInFlight,
            Phase::Resolution,
        ]
        .iter()
        .for_each(|p| assert!(p.committed()));
        [Phase::Ready, Phase::Aiming, Phase::Reset]
            .iter()
            .for_each(|p| assert!(!p.committed()));
    }
}
