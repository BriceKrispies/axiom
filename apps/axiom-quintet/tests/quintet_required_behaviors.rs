//! The required Quintet game-logic behaviours, driven directly against the pure
//! core (no browser). One test (or small cluster) per item in the task spec, so
//! the rules the engine actually runs on are proven without the DOM.

use axiom_quintet::board::{Board, BOARD_SIZE};
use axiom_quintet::clearing::resolve_clears;
use axiom_quintet::generation::{catalog, generate, placeable_shapes};
use axiom_quintet::placement::{can_place, can_place_anywhere, commit};
use axiom_quintet::quintet::{QuintetMask, QUINTET_CELLS};

/// A plus / X-pentomino, used as a known-valid placement shape.
fn plus() -> QuintetMask {
    QuintetMask::from_rows(&["oxo", "xxx", "oxo"])
}

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

fn fill_all(board: &mut Board) {
    for y in 0..BOARD_SIZE as i32 {
        fill_row(board, y);
    }
}

#[test]
fn valid_connected_quintet_passes_validation() {
    // The allowed-style P-shape from the task.
    let p = QuintetMask::from_rows(&["xxooo", "xoooo", "xoooo", "xoooo"]);
    assert_eq!(p.count(), QUINTET_CELLS);
    assert!(p.is_connected());
    assert!(p.is_valid());

    // A T-pentomino is also valid.
    let t = QuintetMask::from_rows(&["xxx", "oxo", "oxo"]);
    assert!(t.is_valid());
}

#[test]
fn diagonal_line_shape_fails_validation() {
    let diag = QuintetMask::from_rows(&["xoooo", "oxooo", "ooxoo", "oooxo", "oooox"]);
    assert_eq!(diag.count(), 5);
    assert!(diag.is_diagonal_line());
    assert!(!diag.is_connected());
    assert!(!diag.is_valid());
}

#[test]
fn disconnected_shape_fails_validation() {
    // A 2×2 block plus a stranded fifth cell touching nothing.
    let disc = QuintetMask::from_rows(&["xxooo", "xxooo", "ooooo", "ooooo", "oooox"]);
    assert_eq!(disc.count(), 5);
    assert!(!disc.is_connected());
    assert!(!disc.is_valid());

    // Two runs that meet only at a corner ((2,0) touches (3,1) diagonally) also
    // fail — corner contact is not orthogonal connectivity.
    let corner = QuintetMask::from_rows(&["xxxoo", "oooxx"]);
    assert_eq!(corner.count(), 5);
    assert!(!corner.is_connected());
    assert!(!corner.is_valid());
}

#[test]
fn generated_pieces_have_exactly_five_cells() {
    for shape in catalog() {
        assert_eq!(shape.count(), QUINTET_CELLS);
    }
    let board = Board::empty();
    let piece = generate(&board, 0, 0).expect("empty board is playable");
    assert_eq!(piece.count(), QUINTET_CELLS);
}

#[test]
fn generated_pieces_are_always_placeable() {
    let board = Board::empty();
    // Sweep many seeds (score/move combinations); every generated piece must fit.
    for moves in 0..64 {
        let piece = generate(&board, moves, moves * 3).expect("not stuck");
        assert!(
            can_place_anywhere(&board, &piece),
            "every offered quintet must fit somewhere"
        );
    }

    // Even on a partially-filled board with a guaranteed empty region, the
    // generator only offers fitting pieces.
    let mut partial = Board::empty();
    fill_row(&mut partial, 0);
    fill_row(&mut partial, 1);
    let piece = generate(&partial, 5, 9).expect("plenty of room remains");
    assert!(can_place_anywhere(&partial, &piece));
}

#[test]
fn placement_fails_when_overlapping_filled_cells() {
    let mut board = Board::empty();
    board.fill(1, 1); // the plus's centre
    assert!(!can_place(&board, &plus(), 0, 0));
}

#[test]
fn placement_fails_outside_the_board() {
    let board = Board::empty();
    let m = plus();
    assert!(!can_place(&board, &m, 8, 0)); // right arm at x = 10
    assert!(!can_place(&board, &m, 0, -1)); // top arm above the board
    assert!(!can_place(&board, &m, 9, 9)); // off the bottom-right corner
}

#[test]
fn placement_succeeds_on_empty_cells() {
    let mut board = Board::empty();
    let m = plus();
    assert!(can_place(&board, &m, 4, 4));
    commit(&mut board, &m, 4, 4);
    assert_eq!(board.filled_count(), 5);
    for &(mx, my) in m.cells() {
        assert!(board.is_filled(4 + mx, 4 + my));
    }
}

#[test]
fn single_row_clear_scores_ten() {
    let mut board = Board::empty();
    fill_row(&mut board, 3);
    let out = resolve_clears(&mut board);
    assert_eq!(out.lines_cleared(), 1);
    assert_eq!(out.cleared_blocks, 10);
    assert_eq!(out.score_delta, 10);
    assert!(board.is_clear());
}

#[test]
fn two_row_clear_scores_forty() {
    let mut board = Board::empty();
    fill_row(&mut board, 1);
    fill_row(&mut board, 8);
    let out = resolve_clears(&mut board);
    assert_eq!(out.lines_cleared(), 2);
    assert_eq!(out.cleared_blocks, 20);
    assert_eq!(out.score_delta, 40);
}

#[test]
fn row_plus_column_clear_counts_intersection_once() {
    let mut board = Board::empty();
    fill_row(&mut board, 5);
    fill_col(&mut board, 5);
    let out = resolve_clears(&mut board);
    assert_eq!((out.rows_cleared, out.cols_cleared), (1, 1));
    assert_eq!(out.cleared_blocks, 19); // 10 + 10 - 1 shared
    assert_eq!(out.score_delta, 38); // 19 * 2
    assert!(board.is_clear());
}

#[test]
fn stuck_when_no_quintet_fits() {
    // Completely full board: no placement, no offered shape.
    let mut full = Board::empty();
    fill_all(&mut full);
    assert!(placeable_shapes(&full).is_empty());
    assert!(generate(&full, 0, 0).is_none());

    // A board with only a 2×2 pocket left is also stuck (no 5-cell shape fits).
    let mut pocket = Board::empty();
    fill_all(&mut pocket);
    for y in 0..2 {
        for x in 0..2 {
            pocket.clear_cell(x, y);
        }
    }
    assert!(generate(&pocket, 0, 0).is_none());
}
