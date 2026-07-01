//! Row/column clearing and scoring after a placement.
//!
//! After a quintet is committed, every fully-filled row and every fully-filled
//! column clears **simultaneously**. A cell at the intersection of a cleared row
//! and a cleared column is removed — and counted — exactly once.
//!
//! Scoring rewards multi-line clears super-linearly:
//!
//! ```text
//! lines_cleared = (#full rows) + (#full columns)
//! cleared_blocks = number of UNIQUE cells removed
//! score_delta    = cleared_blocks * lines_cleared      (0 when nothing clears)
//! ```
//!
//! So one full row removes 10 blocks for 10 points; two full rows remove 20 for
//! 40; one row + one column sharing a cell remove 19 unique blocks for 38.

use crate::quintet::board::{Board, BOARD_SIZE};

/// What a single placement's clear step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearOutcome {
    /// How many full rows were cleared.
    pub rows_cleared: usize,
    /// How many full columns were cleared.
    pub cols_cleared: usize,
    /// Unique cells removed (intersection counted once).
    pub cleared_blocks: usize,
    /// Points awarded for this clear.
    pub score_delta: u64,
}

impl ClearOutcome {
    /// Total lines (rows + columns) cleared.
    pub fn lines_cleared(&self) -> usize {
        self.rows_cleared + self.cols_cleared
    }
}

/// Detect every full row and column on `board`, clear them all at once, and
/// report the score earned. The board is left with those cells empty.
pub fn resolve_clears(board: &mut Board) -> ClearOutcome {
    let n = BOARD_SIZE as i32;
    let full_rows: Vec<i32> = (0..n).filter(|&y| board.row_full(y)).collect();
    let full_cols: Vec<i32> = (0..n).filter(|&x| board.col_full(x)).collect();

    // The union of all cells in any full row or column — a cell shared by a
    // cleared row and column appears once.
    let mut cells: Vec<(i32, i32)> = full_rows
        .iter()
        .flat_map(|&y| (0..n).map(move |x| (x, y)))
        .chain(full_cols.iter().flat_map(|&x| (0..n).map(move |y| (x, y))))
        .collect();
    cells.sort_unstable();
    cells.dedup();

    let cleared_blocks = cells.len();
    let lines_cleared = full_rows.len() + full_cols.len();
    cells.iter().for_each(|&(x, y)| board.clear_cell(x, y));

    ClearOutcome {
        rows_cleared: full_rows.len(),
        cols_cleared: full_cols.len(),
        cleared_blocks,
        score_delta: cleared_blocks as u64 * lines_cleared as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_row(board: &mut Board, y: i32) {
        for x in 0..BOARD_SIZE as i32 {
            board.fill(x, y);
        }
    }

    fn fill_col(board: &mut Board, x: i32) {
        for y in 0..BOARD_SIZE as i32 {
            board.fill(x, y);
        }
    }

    #[test]
    fn nothing_clears_scores_nothing() {
        let mut board = Board::empty();
        board.fill(0, 0);
        let out = resolve_clears(&mut board);
        assert_eq!(out.lines_cleared(), 0);
        assert_eq!(out.cleared_blocks, 0);
        assert_eq!(out.score_delta, 0);
        assert!(board.is_filled(0, 0), "a non-full row is left untouched");
    }

    #[test]
    fn single_row_clear_scores_ten() {
        let mut board = Board::empty();
        fill_row(&mut board, 4);
        let out = resolve_clears(&mut board);
        assert_eq!((out.rows_cleared, out.cols_cleared), (1, 0));
        assert_eq!(out.cleared_blocks, 10);
        assert_eq!(out.score_delta, 10);
        assert!(board.is_clear());
    }

    #[test]
    fn two_row_clear_scores_forty() {
        let mut board = Board::empty();
        fill_row(&mut board, 2);
        fill_row(&mut board, 7);
        let out = resolve_clears(&mut board);
        assert_eq!((out.rows_cleared, out.cols_cleared), (2, 0));
        assert_eq!(out.cleared_blocks, 20);
        assert_eq!(out.score_delta, 40);
        assert!(board.is_clear());
    }

    #[test]
    fn row_plus_column_counts_intersection_once() {
        let mut board = Board::empty();
        fill_row(&mut board, 5);
        fill_col(&mut board, 5);
        let out = resolve_clears(&mut board);
        assert_eq!((out.rows_cleared, out.cols_cleared), (1, 1));
        assert_eq!(out.cleared_blocks, 19);
        assert_eq!(out.lines_cleared(), 2);
        assert_eq!(out.score_delta, 38);
        assert!(board.is_clear());
    }
}
