//! Maps keyboard keys and touch swipes to puzzle commands.
//!
//! The mapping is pure and browser-free so it can be unit-tested without a DOM:
//! the browser shell hands us the `KeyboardEvent.key` string (or a completed
//! swipe's direction vector), and we return the [`PuzzleCommand`] it means (or
//! `None`). Arrow keys and WASD move; `q` ends the life; `r` restarts. A swipe's
//! dominant axis picks the move direction. `Tick` is never an input — it is
//! issued by the run loop.

use axiom_interface::{InterfaceInputEvent, KeyBinding, Keymap};
use axiom_math::Vec2;

use crate::direction::Direction;
use crate::game_command::PuzzleCommand;

/// Puzzle action ids — the neutral `u32`s [`puzzle_keymap`] resolves keys to, each
/// turned into a [`PuzzleCommand`] by [`command_for_action`].
const MOVE_UP: u32 = 0;
const MOVE_DOWN: u32 = 1;
const MOVE_LEFT: u32 = 2;
const MOVE_RIGHT: u32 = 3;
const RESET_LIFE: u32 = 4;
const RESTART_LEVEL: u32 = 5;

/// The puzzle's key bindings as an interface-layer [`Keymap`]. Built from
/// modifier-insensitive [`KeyBinding::key`] rows (the puzzle's keys carry no
/// modifiers): arrow keys and WASD (either case) move; `q`/`r` reset.
fn puzzle_keymap() -> Keymap {
    Keymap::new(&[
        KeyBinding::key("ArrowUp", MOVE_UP),
        KeyBinding::key("w", MOVE_UP),
        KeyBinding::key("W", MOVE_UP),
        KeyBinding::key("ArrowDown", MOVE_DOWN),
        KeyBinding::key("s", MOVE_DOWN),
        KeyBinding::key("S", MOVE_DOWN),
        KeyBinding::key("ArrowLeft", MOVE_LEFT),
        KeyBinding::key("a", MOVE_LEFT),
        KeyBinding::key("A", MOVE_LEFT),
        KeyBinding::key("ArrowRight", MOVE_RIGHT),
        KeyBinding::key("d", MOVE_RIGHT),
        KeyBinding::key("D", MOVE_RIGHT),
        KeyBinding::key("q", RESET_LIFE),
        KeyBinding::key("Q", RESET_LIFE),
        KeyBinding::key("r", RESTART_LEVEL),
        KeyBinding::key("R", RESTART_LEVEL),
    ])
}

/// The [`PuzzleCommand`] a resolved action id means.
fn command_for_action(action: u32) -> Option<PuzzleCommand> {
    match action {
        MOVE_UP => Some(PuzzleCommand::Move(Direction::Up)),
        MOVE_DOWN => Some(PuzzleCommand::Move(Direction::Down)),
        MOVE_LEFT => Some(PuzzleCommand::Move(Direction::Left)),
        MOVE_RIGHT => Some(PuzzleCommand::Move(Direction::Right)),
        RESET_LIFE => Some(PuzzleCommand::ResetLifeFromRecording),
        RESTART_LEVEL => Some(PuzzleCommand::RestartLevelFresh),
        _ => None,
    }
}

/// The command a `KeyboardEvent.key` value maps to, if any.
///
/// Movement accepts both arrow keys and WASD (upper- or lower-case). `q`/`r`
/// trigger the life/level resets. Every other key returns `None`. The mapping
/// runs through the shared interface-layer [`Keymap`]; the puzzle's keys are
/// modifier-insensitive, so the chord context is the default (no modifiers).
pub fn command_for_key(key: &str) -> Option<PuzzleCommand> {
    puzzle_keymap()
        .resolve(key, InterfaceInputEvent::default())
        .and_then(command_for_action)
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
