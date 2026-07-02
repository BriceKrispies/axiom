//! The single deterministic step function: `(state, command) -> result`.
//!
//! This is the one entry point through which the world changes. It is a thin,
//! total dispatcher over [`PuzzleCommand`] onto the transition methods of
//! [`PuzzleGameState`]; keeping it separate gives the simulation one obvious
//! "apply a command here" door for the playtest session, the browser shell, and
//! the tests to share.

use crate::zanzoban::game_command::{PuzzleCommand, PuzzleStepResult};
use crate::zanzoban::game_state::PuzzleGameState;

/// Apply one command to the state and return what happened. Deterministic: the
/// result and the new state are a pure function of the prior state and the
/// command.
pub fn step(state: &mut PuzzleGameState, command: PuzzleCommand) -> PuzzleStepResult {
    match command {
        PuzzleCommand::Move(direction) => state.apply_player_move(direction),
        PuzzleCommand::ResetLifeFromRecording => state.reset_life_from_recording(),
        PuzzleCommand::RestartLevelFresh => state.restart_fresh(),
        PuzzleCommand::Tick => state.tick(),
    }
}

/// Apply a whole command stream in order, returning the per-command results.
/// Convenience for driving scripted sequences (tests, the solvability proof).
pub fn run(state: &mut PuzzleGameState, commands: &[PuzzleCommand]) -> Vec<PuzzleStepResult> {
    commands.iter().map(|&c| step(state, c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zanzoban::coord::GridCoord;
    use crate::zanzoban::direction::Direction;
    use crate::zanzoban::group_id::GroupId;
    use crate::zanzoban::level_definition::{Button, Door, LevelDefinition};

    fn corridor() -> LevelDefinition {
        LevelDefinition {
            title: "corridor".into(),
            width: 5,
            height: 1,
            entrance: GridCoord::new(0, 0),
            exit: GridCoord::new(4, 0),
            walls: vec![],
            buttons: vec![Button {
                position: GridCoord::new(1, 0),
                group: GroupId::new("main"),
            }],
            doors: vec![Door {
                position: GridCoord::new(3, 0),
                group: GroupId::new("main"),
            }],
            wells: Vec::new(),
            switches: Vec::new(),
            crates: Vec::new(),
            hazards: Vec::new(),
            rules: Default::default(),
        }
    }

    #[test]
    fn step_dispatches_each_command() {
        let mut s = PuzzleGameState::new(corridor());
        let moved = step(&mut s, PuzzleCommand::Move(Direction::Right));
        assert!(moved.player_moved());
        let reset = step(&mut s, PuzzleCommand::ResetLifeFromRecording);
        assert!(matches!(
            reset.kind,
            crate::zanzoban::game_command::StepKind::LifeReset
        ));
        assert_eq!(s.ghost_count(), 1);
        let restart = step(&mut s, PuzzleCommand::RestartLevelFresh);
        assert!(matches!(
            restart.kind,
            crate::zanzoban::game_command::StepKind::LevelRestarted
        ));
        assert_eq!(s.ghost_count(), 0);
        let ticked = step(&mut s, PuzzleCommand::Tick);
        assert!(matches!(
            ticked.kind,
            crate::zanzoban::game_command::StepKind::Ticked { .. }
        ));
    }

    #[test]
    fn run_applies_a_stream_in_order() {
        let mut s = PuzzleGameState::new(corridor());
        let results = run(
            &mut s,
            &[
                PuzzleCommand::Move(Direction::Right),
                PuzzleCommand::Move(Direction::Right),
            ],
        );
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.player_moved()));
        assert_eq!(s.player().position, GridCoord::new(2, 0));
    }
}
