//! The top-level Quintet game: board + score + the current piece.
//!
//! [`QuintetGame`] is the pure, browser-free orchestrator the wasm shell drives.
//! It owns the [`Board`], the running score, a monotonically increasing move
//! count, and the *current* quintet to place (or `None` when the board is
//! stuck). It exposes exactly the operations the UI needs — query the current
//! piece, test/commit a placement at a board anchor, and reset — and keeps every
//! rule (placement validity, clearing, scoring, deterministic generation) here.

use crate::quintet::board::Board;
use crate::quintet::clearing::{resolve_clears, ClearOutcome};
use crate::quintet::generation::generate;
use crate::quintet::placement::{can_place, commit, nearest_valid_anchor};
use crate::quintet::quintet::QuintetMask;

/// The result of attempting to place the current quintet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceResult {
    /// The piece was placed; the clear/score outcome is attached.
    Placed(ClearOutcome),
    /// The anchor was invalid (off-board or overlapping); nothing changed.
    Rejected,
    /// There is no current piece — the board is stuck.
    Stuck,
}

/// A full game of Quintet.
#[derive(Debug, Clone)]
pub struct QuintetGame {
    board: Board,
    score: u64,
    moves: u64,
    current: Option<QuintetMask>,
}

impl Default for QuintetGame {
    fn default() -> Self {
        QuintetGame::new()
    }
}

impl QuintetGame {
    /// A new game: an empty board, zero score, and the first placeable quintet.
    pub fn new() -> Self {
        let board = Board::empty();
        let current = generate(&board, 0, 0);
        QuintetGame {
            board,
            score: 0,
            moves: 0,
            current,
        }
    }

    /// The board, for rendering.
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// The running score.
    pub fn score(&self) -> u64 {
        self.score
    }

    /// How many quintets have been placed so far.
    pub fn moves(&self) -> u64 {
        self.moves
    }

    /// The quintet currently waiting in the generator, or `None` when stuck.
    pub fn current(&self) -> Option<&QuintetMask> {
        self.current.as_ref()
    }

    /// Is the board stuck (no quintet can fit anywhere)?
    pub fn is_stuck(&self) -> bool {
        self.current.is_none()
    }

    /// Would the current quintet placed at `(anchor_x, anchor_y)` be valid? Used
    /// for the live drag preview. Always false when stuck.
    pub fn can_place_current(&self, anchor_x: i32, anchor_y: i32) -> bool {
        self.current
            .as_ref()
            .is_some_and(|mask| can_place(&self.board, mask, anchor_x, anchor_y))
    }

    /// The valid anchor for the current quintet nearest to `desired`, within a
    /// Chebyshev `radius`, or `None` when no legal spot is that close (or the
    /// game is stuck). This drives the "magnetic shadow": the drag preview snaps
    /// to the closest spot the piece actually fits instead of going red on a
    /// small misalignment.
    pub fn snap_anchor(&self, desired: (i32, i32), radius: i32) -> Option<(i32, i32)> {
        self.current
            .as_ref()
            .and_then(|mask| nearest_valid_anchor(&self.board, mask, desired, radius))
    }

    /// Try to place the current quintet at `(anchor_x, anchor_y)`. On success the
    /// piece is committed, full rows/columns clear, the score updates, and the
    /// next quintet is generated. On an invalid anchor nothing changes.
    pub fn try_place(&mut self, anchor_x: i32, anchor_y: i32) -> PlaceResult {
        let Some(mask) = self.current.clone() else {
            return PlaceResult::Stuck;
        };
        if !can_place(&self.board, &mask, anchor_x, anchor_y) {
            return PlaceResult::Rejected;
        }
        commit(&mut self.board, &mask, anchor_x, anchor_y);
        self.moves += 1;
        let outcome = resolve_clears(&mut self.board);
        self.score += outcome.score_delta;
        self.current = generate(&self.board, self.score, self.moves);
        PlaceResult::Placed(outcome)
    }

    /// Reset to a fresh game: clear the board, zero the score, drop the stuck
    /// state, and generate a new quintet for the empty board.
    pub fn reset(&mut self) {
        self.board = Board::empty();
        self.score = 0;
        self.moves = 0;
        self.current = generate(&self.board, 0, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quintet::board::BOARD_SIZE;

    /// Find a valid anchor for the current piece (there always is one unless
    /// stuck) — lets a test place the real generated piece deterministically.
    fn any_valid_anchor(game: &QuintetGame) -> (i32, i32) {
        for ay in 0..BOARD_SIZE as i32 {
            for ax in 0..BOARD_SIZE as i32 {
                if game.can_place_current(ax, ay) {
                    return (ax, ay);
                }
            }
        }
        panic!("a non-stuck game always has a valid anchor");
    }

    #[test]
    fn new_game_starts_empty_with_a_piece() {
        let game = QuintetGame::new();
        assert!(game.board().is_clear());
        assert_eq!(game.score(), 0);
        assert_eq!(game.moves(), 0);
        assert!(!game.is_stuck());
        assert_eq!(game.current().unwrap().count(), 5);
    }

    #[test]
    fn placing_a_piece_fills_five_cells_and_advances() {
        let mut game = QuintetGame::new();
        let (ax, ay) = any_valid_anchor(&game);
        let before = game.board().filled_count();
        let result = game.try_place(ax, ay);
        assert!(matches!(result, PlaceResult::Placed(_)));
        // Five new cells (the empty board can't have cleared a line with one piece).
        assert_eq!(game.board().filled_count(), before + 5);
        assert_eq!(game.moves(), 1);
        assert!(!game.is_stuck());
    }

    #[test]
    fn rejected_placement_changes_nothing() {
        let mut game = QuintetGame::new();
        // An anchor far off the board is always invalid.
        let snapshot = game.board().clone();
        let result = game.try_place(100, 100);
        assert_eq!(result, PlaceResult::Rejected);
        assert_eq!(game.board(), &snapshot);
        assert_eq!(game.moves(), 0);
        assert_eq!(game.score(), 0);
    }

    #[test]
    fn reset_clears_board_score_and_stuck() {
        let mut game = QuintetGame::new();
        // Make some moves so state is non-trivial.
        for _ in 0..3 {
            let (ax, ay) = any_valid_anchor(&game);
            game.try_place(ax, ay);
        }
        assert!(game.moves() > 0);
        game.reset();
        assert!(game.board().is_clear());
        assert_eq!(game.score(), 0);
        assert_eq!(game.moves(), 0);
        assert!(!game.is_stuck());
        assert!(game.current().is_some());
    }

    #[test]
    fn snap_anchor_finds_a_nearby_fit_and_is_none_when_stuck() {
        let game = QuintetGame::new();
        // On a fresh empty board there is always a fit near the centre.
        let snapped = game
            .snap_anchor((4, 4), 2)
            .expect("a fit exists near (4,4)");
        assert!(game.can_place_current(snapped.0, snapped.1));

        // A stuck game snaps to nothing.
        let mut stuck = QuintetGame::new();
        for y in 0..BOARD_SIZE as i32 {
            for x in 0..BOARD_SIZE as i32 {
                stuck.board.fill(x, y);
            }
        }
        stuck.current = generate(&stuck.board, stuck.score, stuck.moves);
        assert!(stuck.is_stuck());
        assert_eq!(stuck.snap_anchor((4, 4), 2), None);
    }

    #[test]
    fn a_stuck_game_reports_stuck_on_placement() {
        // Drive the game's state into the stuck condition directly: a full board
        // leaves the generator with no placeable shape, so `current` is `None`.
        let mut game = QuintetGame::new();
        for y in 0..BOARD_SIZE as i32 {
            for x in 0..BOARD_SIZE as i32 {
                game.board.fill(x, y);
            }
        }
        game.current = generate(&game.board, game.score, game.moves);
        assert!(game.is_stuck(), "a full board has no placeable quintet");
        assert!(game.current().is_none());
        // Any placement attempt while stuck reports Stuck and changes nothing.
        assert_eq!(game.try_place(0, 0), PlaceResult::Stuck);
        assert!(!game.can_place_current(0, 0));
        // Reset rescues a stuck game.
        game.reset();
        assert!(!game.is_stuck());
        assert!(game.board().is_clear());
    }
}
