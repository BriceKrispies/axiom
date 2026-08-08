//! The explicit gameplay state, and the commands that move it.
//!
//! One enum, not a drawer of booleans. Everything that varies with where the
//! player is in an attempt — which prompt shows, whether the aim overlay is
//! live, which projection the sculpt panel is editing, whether the preview is
//! drawn, whether the ball moves — is a function of this value, so there is no
//! state to get out of step with itself.

/// Where an attempt is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// A beat to settle after a reset before the editor accepts anything.
    Ready,
    /// Touching inside the goal chooses where the ball should finish.
    TargetSelection,
    /// Dragging the top-down projection bends the shot.
    HorizontalSculpt,
    /// Dragging the side projection lofts or flattens it.
    VerticalSculpt,
    /// The shot is committed: the editor fades, the preview brightens.
    ShotReady,
    /// The run-up. The ball leaves on the contact tick and not before.
    Kicking,
    /// The ball is on the authored path (until something physically hits it).
    BallInFlight,
    /// The result is up.
    Resolution,
    /// The wipe back to a fresh attempt.
    Reset,
}

impl Phase {
    /// Whether the player may currently re-aim by touching the goal. Re-aiming
    /// is allowed throughout editing, not only in its own stage — a player who
    /// changes their mind about the corner while bending the shot should not
    /// have to walk backwards through the flow.
    pub fn accepts_aim(self) -> bool {
        matches!(
            self,
            Phase::TargetSelection | Phase::HorizontalSculpt | Phase::VerticalSculpt
        )
    }

    /// Whether a sculpt panel is up, and which projection it edits.
    pub fn sculpting(self) -> Option<Projection> {
        match self {
            Phase::HorizontalSculpt => Some(Projection::Horizontal),
            Phase::VerticalSculpt => Some(Projection::Vertical),
            _ => None,
        }
    }

    /// Whether the authored path is drawn in the world this phase.
    pub fn shows_preview(self) -> bool {
        matches!(
            self,
            Phase::TargetSelection
                | Phase::HorizontalSculpt
                | Phase::VerticalSculpt
                | Phase::ShotReady
        )
    }

    /// Whether the whole editing interface is live.
    pub fn editing(self) -> bool {
        self.accepts_aim()
    }

    /// The stage this one steps forward to when the player commits, if any.
    pub fn advanced(self) -> Option<Phase> {
        match self {
            Phase::TargetSelection => Some(Phase::HorizontalSculpt),
            Phase::HorizontalSculpt => Some(Phase::VerticalSculpt),
            Phase::VerticalSculpt => Some(Phase::ShotReady),
            _ => None,
        }
    }

    /// The stage this one steps back to, if any. Going back is always allowed
    /// while editing and never allowed after the commit.
    pub fn backed(self) -> Option<Phase> {
        match self {
            Phase::HorizontalSculpt => Some(Phase::TargetSelection),
            Phase::VerticalSculpt => Some(Phase::HorizontalSculpt),
            _ => None,
        }
    }

    /// The very short label this stage puts in front of the player. The design
    /// rule is one word: the interaction teaches the mechanic, not the text.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Ready | Phase::TargetSelection => "AIM",
            Phase::HorizontalSculpt => "BEND",
            Phase::VerticalSculpt => "HEIGHT",
            Phase::ShotReady | Phase::Kicking => "KICK",
            Phase::BallInFlight => "",
            Phase::Resolution | Phase::Reset => "",
        }
    }

    /// What the single bottom action button says here.
    pub fn action_label(self) -> &'static str {
        match self {
            Phase::TargetSelection => "BEND",
            Phase::HorizontalSculpt => "HEIGHT",
            Phase::VerticalSculpt => "KICK",
            _ => "",
        }
    }
}

/// Which projection of the one shot a sculpt panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    /// Looking down: forward runs up the panel, bend runs across it.
    Horizontal,
    /// Looking from the side: forward runs across the panel, height runs up it.
    Vertical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_editing_stages_take_input() {
        let editing = [
            Phase::TargetSelection,
            Phase::HorizontalSculpt,
            Phase::VerticalSculpt,
        ];
        editing.iter().for_each(|p| {
            assert!(p.accepts_aim());
            assert!(p.editing());
            assert!(p.shows_preview());
        });
        [
            Phase::Ready,
            Phase::ShotReady,
            Phase::Kicking,
            Phase::BallInFlight,
            Phase::Resolution,
            Phase::Reset,
        ]
        .iter()
        .for_each(|p| assert!(!p.accepts_aim()));
        assert!(Phase::ShotReady.shows_preview());
        assert!(!Phase::BallInFlight.shows_preview());
    }

    #[test]
    fn the_editing_flow_runs_forward_and_back() {
        assert_eq!(
            Phase::TargetSelection.advanced(),
            Some(Phase::HorizontalSculpt)
        );
        assert_eq!(
            Phase::HorizontalSculpt.advanced(),
            Some(Phase::VerticalSculpt)
        );
        assert_eq!(Phase::VerticalSculpt.advanced(), Some(Phase::ShotReady));
        assert_eq!(Phase::ShotReady.advanced(), None);
        assert_eq!(
            Phase::VerticalSculpt.backed(),
            Some(Phase::HorizontalSculpt)
        );
        assert_eq!(
            Phase::HorizontalSculpt.backed(),
            Some(Phase::TargetSelection)
        );
        assert_eq!(Phase::TargetSelection.backed(), None);
        assert_eq!(Phase::Kicking.backed(), None);
    }

    #[test]
    fn each_stage_names_itself_in_one_word() {
        assert_eq!(Phase::TargetSelection.label(), "AIM");
        assert_eq!(Phase::HorizontalSculpt.label(), "BEND");
        assert_eq!(Phase::VerticalSculpt.label(), "HEIGHT");
        assert_eq!(Phase::ShotReady.label(), "KICK");
        assert_eq!(Phase::Ready.label(), "AIM");
        assert_eq!(Phase::Kicking.label(), "KICK");
        assert_eq!(Phase::BallInFlight.label(), "");
        assert_eq!(Phase::Resolution.label(), "");
        assert_eq!(Phase::Reset.label(), "");
        assert_eq!(Phase::TargetSelection.action_label(), "BEND");
        assert_eq!(Phase::VerticalSculpt.action_label(), "KICK");
        assert_eq!(Phase::BallInFlight.action_label(), "");
    }

    #[test]
    fn the_sculpt_stages_name_their_projection() {
        assert_eq!(
            Phase::HorizontalSculpt.sculpting(),
            Some(Projection::Horizontal)
        );
        assert_eq!(Phase::VerticalSculpt.sculpting(), Some(Projection::Vertical));
        assert_eq!(Phase::TargetSelection.sculpting(), None);
        assert_ne!(Projection::Horizontal, Projection::Vertical);
    }
}
