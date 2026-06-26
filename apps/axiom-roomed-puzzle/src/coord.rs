//! Integer grid coordinates and the room's fixed dimensions.
//!
//! The puzzle plays out on a small rectangular grid of cells. A [`GridCoord`] is
//! a pure integer `(x, y)` cell address — `x` is the column (0 at the left), `y`
//! is the row (0 at the top, increasing downward, matching the top-down screen
//! presentation). Coordinates are stored as `i32` (not `u32`) so a step off the
//! edge produces a representable out-of-bounds coordinate (`-1`, or `width`)
//! that movement and validation can reject, instead of underflowing.

use crate::direction::Direction;

/// The standard room size every authored level uses today (Level 001 is 10×10).
/// The engine supports any size up to [`MAX_DIMENSION`]; these are the canonical
/// defaults the editor opens with.
pub const GRID_WIDTH: u32 = 10;
/// The standard room height. See [`GRID_WIDTH`].
pub const GRID_HEIGHT: u32 = 10;

/// The largest grid dimension a level may declare. A level whose width or height
/// exceeds this is rejected by validation as unreasonable (it guards the editor
/// and the renderer against a pathological hand-edited TOML). 256 is far larger
/// than any hand-authored room yet small enough to bound allocation.
pub const MAX_DIMENSION: u32 = 256;

/// A single grid cell address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GridCoord {
    /// Column, 0 at the left.
    pub x: i32,
    /// Row, 0 at the top, increasing downward.
    pub y: i32,
}

impl GridCoord {
    /// A coordinate at column `x`, row `y`.
    pub const fn new(x: i32, y: i32) -> Self {
        GridCoord { x, y }
    }

    /// This coordinate translated by `(dx, dy)`. Used to compute the cell a move
    /// would land on; the result may be out of bounds (that is the caller's to
    /// reject).
    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        GridCoord {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    /// The neighbouring coordinate one step in `direction`.
    pub fn stepped(self, direction: Direction) -> Self {
        let (dx, dy) = direction.delta();
        self.offset(dx, dy)
    }

    /// Is this coordinate inside a `width`×`height` grid?
    pub const fn in_bounds(self, width: u32, height: u32) -> bool {
        self.x >= 0
            && self.y >= 0
            && (self.x as i64) < width as i64
            && (self.y as i64) < height as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_and_step_agree() {
        let c = GridCoord::new(3, 4);
        assert_eq!(c.offset(1, 0), GridCoord::new(4, 4));
        assert_eq!(c.stepped(Direction::Right), GridCoord::new(4, 4));
        assert_eq!(c.stepped(Direction::Up), GridCoord::new(3, 3));
        assert_eq!(c.stepped(Direction::Down), GridCoord::new(3, 5));
        assert_eq!(c.stepped(Direction::Left), GridCoord::new(2, 4));
    }

    #[test]
    fn in_bounds_rejects_edges_and_negatives() {
        assert!(GridCoord::new(0, 0).in_bounds(10, 10));
        assert!(GridCoord::new(9, 9).in_bounds(10, 10));
        assert!(!GridCoord::new(-1, 0).in_bounds(10, 10));
        assert!(!GridCoord::new(0, -1).in_bounds(10, 10));
        assert!(!GridCoord::new(10, 0).in_bounds(10, 10));
        assert!(!GridCoord::new(0, 10).in_bounds(10, 10));
    }

    #[test]
    fn standard_room_constants() {
        assert_eq!(GRID_WIDTH, 10);
        assert_eq!(GRID_HEIGHT, 10);
        const { assert!(MAX_DIMENSION >= GRID_WIDTH && MAX_DIMENSION >= GRID_HEIGHT) };
    }
}
