//! The commands that drive the deterministic game core, and the result of one.
//!
//! Every change to a [`crate::roomed_puzzle::game_state::PuzzleGameState`] goes through exactly
//! one [`PuzzleCommand`]. The four commands are the whole interface: three are
//! player intents (`Move`, `q`, `r`) and one is the fixed-step clock tick that
//! advances ghost replay. Applying a command returns a [`PuzzleStepResult`]
//! describing what happened — enough for the presentation layer to react without
//! re-deriving state.

use crate::roomed_puzzle::direction::Direction;

/// A single command applied to the game state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCommand {
    /// Move the live player one cell in a direction (an arrow key / WASD press).
    Move(Direction),
    /// End the current life: snapshot the recording into a new ghost, reset the
    /// live player to the entrance, and clear the recording. Triggered by `q`.
    ResetLifeFromRecording,
    /// Restart the level fresh: reset the player, clear every ghost, clear the
    /// recording, and restore the initial state. Triggered by `r`.
    RestartLevelFresh,
    /// Advance the simulation by one fixed step. Drives ghost replay; the live
    /// player is unaffected. Issued by the run loop, never by a key.
    Tick,
}

/// What a single command did to the game state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzleStepResult {
    /// The categorised outcome.
    pub kind: StepKind,
    /// Whether, after this command, the live player stands on the exit.
    pub solved: bool,
}

/// The categorised outcome of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// The live player successfully moved (and the move was recorded).
    PlayerMoved(Direction),
    /// The live player's move was blocked (and was **not** recorded).
    PlayerMoveRejected(Direction),
    /// A life ended and a ghost was created (`q`).
    LifeReset,
    /// The level was restarted (`r`).
    LevelRestarted,
    /// A fixed-step tick advanced; `ghosts_stepped` ghosts physically moved.
    Ticked {
        /// How many ghosts changed cell on this tick.
        ghosts_stepped: u32,
    },
}

impl PuzzleStepResult {
    /// Construct a result.
    pub const fn new(kind: StepKind, solved: bool) -> Self {
        PuzzleStepResult { kind, solved }
    }

    /// Did a live-player move succeed (and therefore get recorded)?
    pub const fn player_moved(&self) -> bool {
        matches!(self.kind, StepKind::PlayerMoved(_))
    }

    /// Was a live-player move rejected?
    pub const fn player_move_rejected(&self) -> bool {
        matches!(self.kind, StepKind::PlayerMoveRejected(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_predicates_classify_kinds() {
        let moved = PuzzleStepResult::new(StepKind::PlayerMoved(Direction::Up), false);
        assert!(moved.player_moved());
        assert!(!moved.player_move_rejected());

        let blocked = PuzzleStepResult::new(StepKind::PlayerMoveRejected(Direction::Up), false);
        assert!(blocked.player_move_rejected());
        assert!(!blocked.player_moved());

        let ticked = PuzzleStepResult::new(StepKind::Ticked { ghosts_stepped: 2 }, false);
        assert!(!ticked.player_moved());
    }
}
