//! The 10×10 board: a flat grid of filled / empty square cells.
//!
//! A [`Board`] is the pure, browser-free state every other piece of game logic
//! reads and writes. A cell address is a plain integer `(x, y)` — `x` the column
//! (0 at the left), `y` the row (0 at the top). Addresses are `i32` (not `u32`)
//! so a quintet dragged off the edge produces a representable out-of-bounds
//! coordinate (`-1`, or `BOARD_SIZE`) that placement can reject instead of
//! underflowing.

pub const BOARD_SIZE: usize = 10;

const CELL_COUNT: usize = BOARD_SIZE * BOARD_SIZE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    /// Row-major occupancy; index = `y * BOARD_SIZE + x`.
    filled: [bool; CELL_COUNT],
}

impl Default for Board {
    fn default() -> Self {
        Board::empty()
    }
}

impl Board {
    pub fn empty() -> Self {
        Board {
            filled: [false; CELL_COUNT],
        }
    }

    pub const fn in_bounds(x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as i64) < BOARD_SIZE as i64 && (y as i64) < BOARD_SIZE as i64
    }

    fn index(x: i32, y: i32) -> usize {
        y as usize * BOARD_SIZE + x as usize
    }

    /// Is `(x, y)` filled? Out-of-bounds cells read as not-filled.
    pub fn is_filled(&self, x: i32, y: i32) -> bool {
        Board::in_bounds(x, y) && self.filled[Board::index(x, y)]
    }

    /// Is `(x, y)` an empty, in-bounds cell? Out-of-bounds cells are *not* empty
    /// (you cannot place onto them).
    pub fn is_empty_cell(&self, x: i32, y: i32) -> bool {
        Board::in_bounds(x, y) && !self.filled[Board::index(x, y)]
    }

    /// Mark `(x, y)` filled. Out-of-bounds writes are ignored.
    pub fn fill(&mut self, x: i32, y: i32) {
        if Board::in_bounds(x, y) {
            self.filled[Board::index(x, y)] = true;
        }
    }

    /// Mark `(x, y)` empty. Out-of-bounds writes are ignored.
    pub fn clear_cell(&mut self, x: i32, y: i32) {
        if Board::in_bounds(x, y) {
            self.filled[Board::index(x, y)] = false;
        }
    }

    pub fn is_clear(&self) -> bool {
        self.filled.iter().all(|&f| !f)
    }

    pub fn row_full(&self, y: i32) -> bool {
        (0..BOARD_SIZE as i32).all(|x| self.is_filled(x, y))
    }

    pub fn col_full(&self, x: i32) -> bool {
        (0..BOARD_SIZE as i32).all(|y| self.is_filled(x, y))
    }

    /// Used to seed deterministic generation.
    pub fn filled_count(&self) -> usize {
        self.filled.iter().filter(|&&f| f).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_board_is_clear_and_unfilled() {
        let b = Board::empty();
        assert!(b.is_clear());
        assert_eq!(b.filled_count(), 0);
        assert!(b.is_empty_cell(0, 0));
        assert!(b.is_empty_cell(9, 9));
        assert!(!b.is_filled(0, 0));
    }

    #[test]
    fn fill_and_clear_round_trip() {
        let mut b = Board::empty();
        b.fill(3, 4);
        assert!(b.is_filled(3, 4));
        assert!(!b.is_empty_cell(3, 4));
        assert_eq!(b.filled_count(), 1);
        b.clear_cell(3, 4);
        assert!(b.is_empty_cell(3, 4));
        assert!(b.is_clear());
    }

    #[test]
    fn out_of_bounds_is_neither_filled_nor_empty() {
        let b = Board::empty();
        assert!(!Board::in_bounds(-1, 0));
        assert!(!Board::in_bounds(0, -1));
        assert!(!Board::in_bounds(BOARD_SIZE as i32, 0));
        assert!(!Board::in_bounds(0, BOARD_SIZE as i32));
        assert!(!b.is_filled(-1, 0));
        assert!(!b.is_empty_cell(-1, 0));
        assert!(!b.is_empty_cell(BOARD_SIZE as i32, 0));
    }

    #[test]
    fn out_of_bounds_writes_are_ignored() {
        let mut b = Board::empty();
        b.fill(-1, -1);
        b.fill(BOARD_SIZE as i32, 0);
        b.clear_cell(-5, 5);
        assert!(b.is_clear());
    }

    #[test]
    fn row_and_column_fullness() {
        let mut b = Board::empty();
        assert!(!b.row_full(0));
        assert!(!b.col_full(0));
        for x in 0..BOARD_SIZE as i32 {
            b.fill(x, 0);
        }
        assert!(b.row_full(0));
        assert!(!b.row_full(1));
        let mut c = Board::empty();
        for y in 0..BOARD_SIZE as i32 {
            c.fill(0, y);
        }
        assert!(c.col_full(0));
        assert!(!c.col_full(1));
    }
}
