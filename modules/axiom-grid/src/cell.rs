//! An integer grid coordinate, with the total order that makes paths
//! byte-identical.

use std::cmp::Ordering;

/// An integer `(x, y)` cell coordinate on a [`Grid`](crate::Grid).
///
/// Its total order is **lexicographic on `(y, x)`** — row first, then column.
/// That order is the deterministic tie-break the path queries use when two cells
/// are otherwise equal (same distance, same cost), so a reconstructed path is
/// *the* canonical shortest path, not merely *a* shortest path (SPEC-06 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    /// Column index (may be negative or out of bounds; `Grid::get` is OOB-safe).
    pub x: i32,
    /// Row index (may be negative or out of bounds; `Grid::get` is OOB-safe).
    pub y: i32,
}

impl Cell {
    /// Construct a cell at `(x, y)`.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

impl Ord for Cell {
    /// Lexicographic on `(y, x)` — the canonical tie-break order. Branchless: a
    /// tuple comparison, not hand-written control flow.
    fn cmp(&self, other: &Self) -> Ordering {
        (self.y, self.x).cmp(&(other.y, other.x))
    }
}

impl PartialOrd for Cell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_lexicographic_on_y_then_x() {
        // Lower row sorts first regardless of column.
        assert!(Cell::new(9, 0) < Cell::new(0, 1));
        // Within a row, lower column sorts first.
        assert!(Cell::new(0, 2) < Cell::new(1, 2));
        // Equality.
        assert_eq!(Cell::new(3, 4).cmp(&Cell::new(3, 4)), Ordering::Equal);
        assert_eq!(Cell::new(3, 4), Cell::new(3, 4));
        // partial_cmp agrees with cmp.
        assert_eq!(
            Cell::new(1, 2).partial_cmp(&Cell::new(2, 2)),
            Some(Ordering::Less)
        );
    }
}
