//! Placing a quintet onto the board: validity, enumeration, and commit.
//!
//! A *placement* anchors a [`QuintetMask`] at a board cell `(anchor_x,
//! anchor_y)`: the mask's occupied cell `(mx, my)` lands on board cell
//! `(anchor_x + mx, anchor_y + my)`. A placement is **valid** only if every one
//! of those landing cells is in bounds *and* currently empty — so a quintet may
//! never hang off the edge or overlap a filled block.

use crate::board::{Board, BOARD_SIZE};
use crate::quintet::QuintetMask;

/// Can `mask` be placed with its top-left mask cell anchored at `(anchor_x,
/// anchor_y)`? True only when every occupied cell lands on an empty, in-bounds
/// board cell.
pub fn can_place(board: &Board, mask: &QuintetMask, anchor_x: i32, anchor_y: i32) -> bool {
    mask.cells()
        .iter()
        .all(|&(mx, my)| board.is_empty_cell(anchor_x + mx, anchor_y + my))
}

/// Is there *any* anchor at which `mask` fits on `board`? This is the predicate
/// that decides whether a shape can be offered (and, across all shapes, whether
/// the game is stuck).
pub fn can_place_anywhere(board: &Board, mask: &QuintetMask) -> bool {
    (0..BOARD_SIZE as i32).any(|ay| (0..BOARD_SIZE as i32).any(|ax| can_place(board, mask, ax, ay)))
}

/// Write `mask`'s occupied cells onto `board` at the given anchor. The caller is
/// responsible for having checked [`can_place`] first; out-of-bounds cells are
/// ignored by the board, but a real game only ever commits a valid placement.
pub fn commit(board: &mut Board, mask: &QuintetMask, anchor_x: i32, anchor_y: i32) {
    mask.cells()
        .iter()
        .for_each(|&(mx, my)| board.fill(anchor_x + mx, anchor_y + my));
}

/// The valid anchor *nearest* to `desired` within a Chebyshev `radius`, or `None`
/// when no placement fits within that window. This is the "magnetic shadow":
/// when the player drags a piece roughly over a spot it fits, the preview snaps
/// to the closest legal anchor instead of rigidly tracking the cursor and going
/// red on a one-cell misalignment.
///
/// Candidates are every anchor offset by up to `radius` cells from `desired`;
/// the winner minimizes squared Euclidean distance to `desired`, with ties
/// broken deterministically toward the top-left (smaller `y`, then `x`) so the
/// snap never flickers between equidistant spots.
pub fn nearest_valid_anchor(
    board: &Board,
    mask: &QuintetMask,
    desired: (i32, i32),
    radius: i32,
) -> Option<(i32, i32)> {
    (-radius..=radius)
        .flat_map(|dy| (-radius..=radius).map(move |dx| (desired.0 + dx, desired.1 + dy)))
        .filter(|&(ax, ay)| can_place(board, mask, ax, ay))
        .min_by_key(|&(ax, ay)| {
            let (ddx, ddy) = (ax - desired.0, ay - desired.1);
            (ddx * ddx + ddy * ddy, ay, ax)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plus() -> QuintetMask {
        // The X-pentomino (a plus): occupies (1,0),(0,1),(1,1),(2,1),(1,2).
        QuintetMask::from_rows(&["oxo", "xxx", "oxo"])
    }

    #[test]
    fn placement_succeeds_on_empty_cells() {
        let board = Board::empty();
        let m = plus();
        assert!(can_place(&board, &m, 0, 0));
        assert!(can_place_anywhere(&board, &m));
    }

    #[test]
    fn placement_fails_outside_the_board() {
        let board = Board::empty();
        let m = plus();
        // Anchored so the plus's right arm (mask x=2) lands at board x=10.
        assert!(!can_place(&board, &m, 8, 0));
        // Anchored so the top arm lands above the board.
        assert!(!can_place(&board, &m, 0, -1));
    }

    #[test]
    fn placement_fails_when_overlapping_a_filled_cell() {
        let mut board = Board::empty();
        // Fill the centre of where the plus would sit.
        board.fill(1, 1);
        let m = plus();
        assert!(!can_place(&board, &m, 0, 0));
        // But it still fits elsewhere.
        assert!(can_place(&board, &m, 5, 5));
    }

    #[test]
    fn commit_fills_exactly_the_masks_cells() {
        let mut board = Board::empty();
        let m = plus();
        commit(&mut board, &m, 3, 3);
        assert_eq!(board.filled_count(), 5);
        for &(mx, my) in m.cells() {
            assert!(board.is_filled(3 + mx, 3 + my));
        }
        // A neighbour the plus does not cover stays empty.
        assert!(board.is_empty_cell(3, 3));
    }

    #[test]
    fn nearest_anchor_returns_the_desired_spot_when_it_already_fits() {
        let board = Board::empty();
        let m = plus();
        // (4,4) is a valid anchor on an empty board, so it snaps to itself.
        assert_eq!(nearest_valid_anchor(&board, &m, (4, 4), 2), Some((4, 4)));
    }

    #[test]
    fn nearest_anchor_snaps_to_an_adjacent_valid_spot() {
        let mut board = Board::empty();
        // Block the plus's centre at the desired anchor (1,1) → desired (0,0)
        // is invalid, but a one-cell nudge fits.
        board.fill(1, 1);
        let m = plus();
        assert!(!can_place(&board, &m, 0, 0));
        let snapped = nearest_valid_anchor(&board, &m, (0, 0), 2).expect("a fit is nearby");
        assert!(can_place(&board, &m, snapped.0, snapped.1));
        // It is within the search radius of the desired anchor.
        assert!(snapped.0.abs() <= 2 && snapped.1.abs() <= 2);
    }

    #[test]
    fn nearest_anchor_snaps_an_off_board_drag_back_on() {
        let board = Board::empty();
        let m = plus();
        // Desired anchor hangs off the top-left; the nearest legal anchor on an
        // empty board is (0,0).
        assert_eq!(nearest_valid_anchor(&board, &m, (-1, -1), 2), Some((0, 0)));
    }

    #[test]
    fn nearest_anchor_is_none_when_nothing_fits_within_radius() {
        let mut board = Board::empty();
        // Fill the whole board, then open a single 3×3 pocket far from the
        // desired anchor so no plus fits within a small radius of it.
        for y in 0..BOARD_SIZE as i32 {
            for x in 0..BOARD_SIZE as i32 {
                board.fill(x, y);
            }
        }
        let m = plus();
        // Radius 1 around a fully-filled neighbourhood: no fit.
        assert_eq!(nearest_valid_anchor(&board, &m, (5, 5), 1), None);
    }

    #[test]
    fn nearest_anchor_picks_the_closest_of_several_valid_spots() {
        let board = Board::empty();
        let m = plus();
        // On an empty board every nearby anchor fits; the closest to the desired
        // anchor is the desired anchor itself.
        assert_eq!(nearest_valid_anchor(&board, &m, (3, 3), 2), Some((3, 3)));
    }

    #[test]
    fn can_place_anywhere_is_false_on_a_full_board() {
        let mut board = Board::empty();
        for y in 0..BOARD_SIZE as i32 {
            for x in 0..BOARD_SIZE as i32 {
                board.fill(x, y);
            }
        }
        assert!(!can_place_anywhere(&board, &plus()));
    }
}
