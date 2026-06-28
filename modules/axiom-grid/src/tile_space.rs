//! The tile↔world mapping: pure geometry over an origin and a cell size.

use axiom_kernel::Meters;
use axiom_math::Vec2;

use crate::cell::Cell;

/// The mapping between integer [`Cell`] coordinates and world [`Vec2`] points.
///
/// A pure function of two fields — the world-space `origin` of cell `(0, 0)`'s
/// corner and the `cell_size` — with no stored grid and no engine state.
/// `tile_to_world` returns the **center** of a cell, so a thing placed there sits
/// in the middle of its tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileSpace {
    origin: Vec2,
    cell_size: f32,
}

impl TileSpace {
    /// Crate-internal constructor; authored through
    /// [`GridApi::tile_space`](crate::GridApi::tile_space). Takes a dimensioned
    /// [`Meters`] cell size (the public API forbids a naked `f32`).
    pub(crate) fn new(origin: Vec2, cell_size: Meters) -> Self {
        Self {
            origin,
            cell_size: cell_size.get(),
        }
    }

    /// The world-space **center** of cell `(x, y)`.
    pub fn tile_to_world(self, x: i32, y: i32) -> Vec2 {
        Vec2::new(
            self.origin.x + (x as f32 + 0.5) * self.cell_size,
            self.origin.y + (y as f32 + 0.5) * self.cell_size,
        )
    }

    /// The cell containing world point `p` (floored toward the lower-left).
    pub fn world_to_tile(self, p: Vec2) -> Cell {
        Cell::new(
            ((p.x - self.origin.x) / self.cell_size).floor() as i32,
            ((p.y - self.origin.y) / self.cell_size).floor() as i32,
        )
    }

    /// Snap `p` to the center of the cell that contains it — idempotent, so
    /// snapping an already-snapped point is a fixed point.
    pub fn snap_to_cell(self, p: Vec2) -> Vec2 {
        let cell = self.world_to_tile(p);
        self.tile_to_world(cell.x, cell.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meters(v: f32) -> Meters {
        Meters::new(v).expect("finite positive cell size")
    }

    #[test]
    fn tile_to_world_returns_the_cell_center() {
        let ts = TileSpace::new(Vec2::ZERO, meters(10.0));
        // Cell (0,0)'s center is at (5, 5) for a 10-unit cell.
        assert_eq!(ts.tile_to_world(0, 0), Vec2::new(5.0, 5.0));
        assert_eq!(ts.tile_to_world(2, 1), Vec2::new(25.0, 15.0));
    }

    #[test]
    fn world_to_tile_floors_into_the_containing_cell() {
        let ts = TileSpace::new(Vec2::new(0.0, 0.0), meters(10.0));
        assert_eq!(ts.world_to_tile(Vec2::new(0.0, 0.0)), Cell::new(0, 0));
        assert_eq!(ts.world_to_tile(Vec2::new(9.9, 0.1)), Cell::new(0, 0));
        assert_eq!(ts.world_to_tile(Vec2::new(10.0, 25.0)), Cell::new(1, 2));
        // A point left of the origin floors to a negative cell.
        assert_eq!(ts.world_to_tile(Vec2::new(-1.0, 0.0)), Cell::new(-1, 0));
    }

    #[test]
    fn round_trip_and_snap_are_stable() {
        let ts = TileSpace::new(Vec2::new(1.0, 2.0), meters(4.0));
        // tile -> world -> tile recovers the cell.
        let cell = Cell::new(3, 5);
        let world = ts.tile_to_world(cell.x, cell.y);
        assert_eq!(ts.world_to_tile(world), cell);
        // snap is idempotent: snapping a center yields the same center.
        let snapped = ts.snap_to_cell(Vec2::new(6.7, 9.1));
        assert_eq!(ts.snap_to_cell(snapped), snapped);
    }
}
