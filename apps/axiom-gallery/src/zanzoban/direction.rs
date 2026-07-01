//! The four cardinal move directions.
//!
//! The player and ghosts move one cell at a time along a cardinal direction.
//! Directions are pure data: a [`Direction`] maps to a `(dx, dy)` cell delta in
//! top-down screen space (`y` increases downward), and that is the only thing
//! the simulation reads from it.

/// A single-cell cardinal move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Toward the top of the room (`-y`).
    Up,
    /// Toward the bottom of the room (`+y`).
    Down,
    /// Toward the left of the room (`-x`).
    Left,
    /// Toward the right of the room (`+x`).
    Right,
}

impl Direction {
    /// Every direction, in a stable order. Handy for exhaustive tests.
    pub const ALL: [Direction; 4] = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];

    /// The `(dx, dy)` cell delta this direction applies. `y` grows downward, so
    /// [`Direction::Up`] is `-1` in `y`.
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
        }
    }

    /// The opposite direction. Useful for tests and undo-style reasoning.
    pub const fn opposite(self) -> Direction {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_are_unit_cardinal() {
        assert_eq!(Direction::Up.delta(), (0, -1));
        assert_eq!(Direction::Down.delta(), (0, 1));
        assert_eq!(Direction::Left.delta(), (-1, 0));
        assert_eq!(Direction::Right.delta(), (1, 0));
    }

    #[test]
    fn opposite_round_trips() {
        for d in Direction::ALL {
            assert_eq!(d.opposite().opposite(), d);
            let (dx, dy) = d.delta();
            let (ox, oy) = d.opposite().delta();
            assert_eq!((dx + ox, dy + oy), (0, 0));
        }
    }
}
