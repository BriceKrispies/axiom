//! Maps keyboard keys to puzzle commands.
//!
//! The mapping is pure and browser-free so it can be unit-tested without a DOM:
//! the browser shell hands us the `KeyboardEvent.key` string, and we return the
//! [`PuzzleCommand`] it means (or `None` for keys the puzzle ignores). Arrow keys
//! and WASD move; `q` ends the life; `r` restarts. `Tick` is never a key — it is
//! issued by the run loop.

use crate::direction::Direction;
use crate::game_command::PuzzleCommand;

/// The command a `KeyboardEvent.key` value maps to, if any.
///
/// Movement accepts both arrow keys and WASD (upper- or lower-case). `q`/`r`
/// trigger the life/level resets. Every other key returns `None`.
pub fn command_for_key(key: &str) -> Option<PuzzleCommand> {
    match key {
        "ArrowUp" | "w" | "W" => Some(PuzzleCommand::Move(Direction::Up)),
        "ArrowDown" | "s" | "S" => Some(PuzzleCommand::Move(Direction::Down)),
        "ArrowLeft" | "a" | "A" => Some(PuzzleCommand::Move(Direction::Left)),
        "ArrowRight" | "d" | "D" => Some(PuzzleCommand::Move(Direction::Right)),
        "q" | "Q" => Some(PuzzleCommand::ResetLifeFromRecording),
        "r" | "R" => Some(PuzzleCommand::RestartLevelFresh),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_and_wasd_move_the_same_way() {
        assert_eq!(
            command_for_key("ArrowUp"),
            Some(PuzzleCommand::Move(Direction::Up))
        );
        assert_eq!(
            command_for_key("w"),
            Some(PuzzleCommand::Move(Direction::Up))
        );
        assert_eq!(
            command_for_key("ArrowRight"),
            Some(PuzzleCommand::Move(Direction::Right))
        );
        assert_eq!(
            command_for_key("d"),
            Some(PuzzleCommand::Move(Direction::Right))
        );
    }

    #[test]
    fn q_and_r_map_to_resets() {
        assert_eq!(
            command_for_key("q"),
            Some(PuzzleCommand::ResetLifeFromRecording)
        );
        assert_eq!(command_for_key("R"), Some(PuzzleCommand::RestartLevelFresh));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(command_for_key("Tab"), None);
        assert_eq!(command_for_key(" "), None);
        assert_eq!(command_for_key("Enter"), None);
    }
}
