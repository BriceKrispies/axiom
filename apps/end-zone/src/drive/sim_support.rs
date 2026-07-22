//! The drive-support mutators the controller drives the simulation with: moving
//! the line of scrimmage, installing a freshly-composed play, reloading the
//! heat-scaled defense, blowing a stuck play dead, and reading the ball's spot.
//! These are `SimState` methods (the drive owns *when* they run; the sim owns
//! *what* they do).

use crate::ai::{compile_assignments, AssignmentKind};
use crate::data::player::RosterDefinition;
use crate::data::{BehaviorTuning, PlayDefinition};
use crate::field::{z_to_yards_from_own_goal, OffenseFrame};
use crate::identity::PlayerId;
use crate::state::SimState;

impl SimState {
    /// Move the line of scrimmage to `yards_from_own_goal` (`1..=99`) and
    /// recompile the play for the new frame.
    pub fn respot(&mut self, yards_from_own_goal: f32) {
        let clamped = yards_from_own_goal.clamp(1.0, 99.0);
        self.frame = OffenseFrame::at_yard_line(clamped, self.frame.direction);
        self.assignments = compile_assignments(&self.play, &self.frame);
        self.quarterback = self.locate_quarterback();
    }

    /// Install a freshly-composed play (offense + chosen defensive call) at its
    /// line of scrimmage, recompiling assignments for the new frame. The next
    /// `BeginPlay` re-lines-up both teams from this play's formations.
    pub fn install_play(&mut self, play: PlayDefinition) {
        self.frame = OffenseFrame::at_yard_line(play.line_of_scrimmage, play.drive_direction);
        self.play = play;
        self.assignments = compile_assignments(&self.play, &self.frame);
        self.quarterback = self.locate_quarterback();
    }

    /// The [`PlayerId`] running the quarterback assignment (slot 0 if none).
    fn locate_quarterback(&self) -> PlayerId {
        self.assignments
            .iter()
            .enumerate()
            .find(|(_, a)| matches!(a.kind, AssignmentKind::Quarterback { .. }))
            .map(|(i, _)| PlayerId(i as u8))
            .unwrap_or(PlayerId(0))
    }

    /// Replace the defense roster and shared contact tuning (heat escalation).
    pub fn reload_defense(&mut self, defense: RosterDefinition, tuning: BehaviorTuning) {
        self.rosters.1 = defense;
        self.tuning = tuning;
    }

    /// Blow the play dead where the ball currently is (the sack / dead-ball
    /// path the play clock uses when a held ball never resolves).
    pub fn blow_dead(&mut self) {
        self.end_play(crate::events::PlayEndReason::Tackled);
    }

    /// How far the ball currently sits from the offense's own goal, in yards:
    /// the live carrier's spot, else the ball's resting spot.
    pub fn ball_yard_line(&self) -> f32 {
        let world = self
            .ball
            .carrier()
            .map(|c| self.players[c.index()].pos)
            .unwrap_or(self.ball.pos);
        z_to_yards_from_own_goal(world.z, self.frame.direction)
    }
}
