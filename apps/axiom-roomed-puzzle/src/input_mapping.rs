//! Maps keyboard keys and touch swipes to puzzle commands.
//!
//! The mapping is pure and browser-free so it can be unit-tested without a DOM:
//! the browser shell hands us the `KeyboardEvent.key` string (or a completed
//! swipe's direction vector), and we return the [`PuzzleCommand`] it means (or
//! `None`). Arrow keys and WASD move; `q` ends the life; `r` restarts. A swipe's
//! dominant axis picks the move direction. `Tick` is never an input — it is
//! issued by the run loop.

use axiom_math::Vec2;

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

/// The command a completed swipe maps to, from its direction vector (screen
/// space: +x is right, +y is down). The dominant axis chooses the cardinal
/// move; a zero vector is not a swipe and maps to nothing. `q`/`r` are not
/// swipe-reachable — they have on-screen buttons instead.
pub fn command_for_swipe(direction: Vec2) -> Option<PuzzleCommand> {
    if direction.x == 0.0 && direction.y == 0.0 {
        return None;
    }
    let dir = if direction.x.abs() >= direction.y.abs() {
        if direction.x >= 0.0 {
            Direction::Right
        } else {
            Direction::Left
        }
    } else if direction.y >= 0.0 {
        Direction::Down
    } else {
        Direction::Up
    };
    Some(PuzzleCommand::Move(dir))
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

    #[test]
    fn swipes_map_to_the_dominant_cardinal() {
        assert_eq!(
            command_for_swipe(Vec2::new(1.0, 0.0)),
            Some(PuzzleCommand::Move(Direction::Right))
        );
        assert_eq!(
            command_for_swipe(Vec2::new(-1.0, 0.1)),
            Some(PuzzleCommand::Move(Direction::Left))
        );
        // Screen +y is down, so a negative y is an upward swipe.
        assert_eq!(
            command_for_swipe(Vec2::new(0.1, -1.0)),
            Some(PuzzleCommand::Move(Direction::Up))
        );
        assert_eq!(
            command_for_swipe(Vec2::new(0.0, 1.0)),
            Some(PuzzleCommand::Move(Direction::Down))
        );
    }

    #[test]
    fn a_zero_swipe_is_ignored() {
        assert_eq!(command_for_swipe(Vec2::ZERO), None);
    }
}
