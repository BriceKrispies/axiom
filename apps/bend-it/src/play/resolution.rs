//! How an attempt finished, and the running tally.
//!
//! A result is *observed*, never chosen: the session reports what the geometry
//! did. There is no arm of this enum that can be reached by deciding it should
//! happen.

use crate::pitch::FrameMember;

/// The end of one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotResult {
    /// The ball crossed the plane inside the frame, untouched.
    Goal,
    /// The keeper's own reach or body intersected the ball's path.
    Save,
    /// The ball struck the frame.
    Frame(FrameMember),
    /// The ball crossed the plane outside the frame. Authored endpoints are
    /// constrained inside the goal, so this can only follow a deflection.
    Miss,
}

impl ShotResult {
    /// The banner, in the game's one-word register.
    pub fn banner(self) -> &'static str {
        match self {
            ShotResult::Goal => "GOAL",
            ShotResult::Save => "SAVED",
            ShotResult::Frame(FrameMember::Crossbar) => "CROSSBAR",
            ShotResult::Frame(_) => "POST",
            ShotResult::Miss => "WIDE",
        }
    }

    /// Whether it counts.
    pub fn scored(self) -> bool {
        matches!(self, ShotResult::Goal)
    }
}

/// The running tally across a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    pub attempts: u32,
    pub goals: u32,
}

impl Tally {
    /// Record a finished attempt.
    pub fn record(&mut self, result: ShotResult) {
        self.attempts += 1;
        self.goals += u32::from(result.scored());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_result_names_itself() {
        assert_eq!(ShotResult::Goal.banner(), "GOAL");
        assert_eq!(ShotResult::Save.banner(), "SAVED");
        assert_eq!(
            ShotResult::Frame(FrameMember::Crossbar).banner(),
            "CROSSBAR"
        );
        assert_eq!(ShotResult::Frame(FrameMember::LeftPost).banner(), "POST");
        assert_eq!(ShotResult::Frame(FrameMember::RightPost).banner(), "POST");
        assert_eq!(ShotResult::Miss.banner(), "WIDE");
    }

    #[test]
    fn only_a_goal_counts() {
        let mut tally = Tally::default();
        tally.record(ShotResult::Goal);
        tally.record(ShotResult::Save);
        tally.record(ShotResult::Frame(FrameMember::LeftPost));
        tally.record(ShotResult::Miss);
        assert_eq!(tally, Tally { attempts: 4, goals: 1 });
        assert!(ShotResult::Goal.scored());
        assert!(!ShotResult::Save.scored());
    }
}
