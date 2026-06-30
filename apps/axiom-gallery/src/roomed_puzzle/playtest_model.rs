//! The playtest-mode model.
//!
//! [`PlaytestSession`] is the playtest-mode counterpart to
//! [`crate::roomed_puzzle::editor_model::EditorModel`]: it owns the live
//! [`PuzzleGameState`], applies commands through the one deterministic
//! [`step`] door, and exposes the render model and a status line the browser
//! shell draws. It is built from a *validated* level (the app gates the switch on
//! [`EditorModel::can_playtest`](crate::roomed_puzzle::editor_model::EditorModel::can_playtest)).

use crate::roomed_puzzle::game_command::{PuzzleCommand, PuzzleStepResult};
use crate::roomed_puzzle::game_state::PuzzleGameState;
use crate::roomed_puzzle::game_step::step;
use crate::roomed_puzzle::input_mapping::command_for_key;
use crate::roomed_puzzle::level_definition::LevelDefinition;
use crate::roomed_puzzle::render_model::RenderModel;

/// One-line controls help shown under the playtest board.
pub const CONTROLS_HELP: &str = "Arrows / WASD: move · q: leave a ghost & reset · r: restart fresh";

/// A live playtest of one level.
#[derive(Debug, Clone)]
pub struct PlaytestSession {
    state: PuzzleGameState,
}

impl PlaytestSession {
    /// Start a playtest of `level` (assumed valid).
    pub fn new(level: LevelDefinition) -> Self {
        PlaytestSession {
            state: PuzzleGameState::new(level),
        }
    }

    /// Apply one command and return what happened.
    pub fn apply(&mut self, command: PuzzleCommand) -> PuzzleStepResult {
        step(&mut self.state, command)
    }

    /// Apply the command a key maps to, if any (movement, `q`, `r`). Returns the
    /// result, or `None` for keys the puzzle ignores.
    pub fn apply_key(&mut self, key: &str) -> Option<PuzzleStepResult> {
        command_for_key(key).map(|command| self.apply(command))
    }

    /// Advance one fixed-step tick (drives ghost replay).
    pub fn tick(&mut self) -> PuzzleStepResult {
        self.apply(PuzzleCommand::Tick)
    }

    /// The current frame to draw.
    pub fn render_model(&self) -> RenderModel {
        RenderModel::from_state(&self.state)
    }

    /// The underlying state (for inspection / tests).
    pub fn state(&self) -> &PuzzleGameState {
        &self.state
    }

    /// Has the live player reached the exit?
    pub fn is_solved(&self) -> bool {
        self.state.is_solved()
    }

    /// How many ghosts exist.
    pub fn ghost_count(&self) -> usize {
        self.state.ghost_count()
    }

    /// The current simulation tick.
    pub fn tick_count(&self) -> u64 {
        self.state.current_tick()
    }

    /// A one-line status: ghost count, recorded-move count, and goal/solved.
    pub fn status_line(&self) -> String {
        let goal = if self.is_solved() {
            "Solved! Press r to play again.".to_string()
        } else {
            "Reach the exit.".to_string()
        };
        format!(
            "Ghosts: {} · Recorded this life: {} · {}",
            self.ghost_count(),
            self.state.recording_len(),
            goal
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roomed_puzzle::coord::GridCoord;
    use crate::roomed_puzzle::direction::Direction;
    use crate::roomed_puzzle::group_id::GroupId;
    use crate::roomed_puzzle::level_definition::{Button, Door};

    fn corridor() -> LevelDefinition {
        LevelDefinition {
            title: "c".into(),
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
        }
    }

    #[test]
    fn apply_key_routes_moves_and_resets() {
        let mut p = PlaytestSession::new(corridor());
        assert!(p.apply_key("ArrowRight").unwrap().player_moved());
        assert_eq!(p.state().player().position, GridCoord::new(1, 0));
        // q leaves a ghost and resets.
        p.apply_key("q");
        assert_eq!(p.ghost_count(), 1);
        assert_eq!(p.state().player().position, GridCoord::new(0, 0));
        // An ignored key returns None.
        assert!(p.apply_key("Tab").is_none());
    }

    #[test]
    fn status_line_reports_progress_and_solved() {
        // entrance(0) · button(1) · door(2) · exit(3): the button is adjacent to
        // the door, so the player can hold it open and step straight through —
        // soloable, unlike the wider `corridor()` (which needs a ghost).
        let adjacent = LevelDefinition {
            title: "adjacent".into(),
            width: 4,
            height: 1,
            entrance: GridCoord::new(0, 0),
            exit: GridCoord::new(3, 0),
            walls: vec![],
            buttons: vec![Button {
                position: GridCoord::new(1, 0),
                group: GroupId::new("main"),
            }],
            doors: vec![Door {
                position: GridCoord::new(2, 0),
                group: GroupId::new("main"),
            }],
        };
        let mut p = PlaytestSession::new(adjacent);
        assert!(p.status_line().contains("Ghosts: 0"));
        // 0->1 (button, door opens), 1->2 (through the open door), 2->3 (exit).
        for _ in 0..3 {
            p.apply(PuzzleCommand::Move(Direction::Right));
        }
        assert!(p.is_solved());
        assert!(p.status_line().contains("Solved"));
    }
}
