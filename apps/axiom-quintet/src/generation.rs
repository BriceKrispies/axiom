//! Deterministic generation of the next quintet — always one that fits.
//!
//! Generation is a pure function of `(board, score, move-count)`: those are
//! folded into a seed for the kernel's [`DeterministicRng`], so a given game
//! state always produces the same next piece and a whole game is replayable.
//!
//! The shape pool is the [`catalog`] of every distinct *fixed* pentomino (all
//! rotations and reflections) — each one a real 5-cell orthogonally-connected
//! quintet by construction, so no diagonal line or disconnected shape can ever
//! be offered. From the pool we keep only the shapes that actually fit somewhere
//! on the current board ([`can_place_anywhere`]); the generator picks one of
//! those. If *no* shape fits, the board is stuck and generation yields `None`.

use axiom_kernel::DeterministicRng;
use std::collections::BTreeSet;

use crate::board::{Board, BOARD_SIZE};
use crate::placement::can_place_anywhere;
use crate::quintet::{QuintetMask, ORTHOGONAL, QUINTET_CELLS};

/// Every distinct fixed pentomino, normalized. Built by growing connected cell
/// sets one orthogonal step at a time from a single seed cell and canonicalising
/// each result, so the catalog is exactly the set of valid quintet shapes.
pub fn catalog() -> Vec<QuintetMask> {
    let mut shapes: BTreeSet<QuintetMask> = BTreeSet::new();
    shapes.insert(QuintetMask::from_coords(&[(0, 0)]));

    // Grow from size 1 to QUINTET_CELLS: each round adds one orthogonally
    // adjacent cell to every shape, in every possible direction.
    for _ in 1..QUINTET_CELLS {
        let mut grown: BTreeSet<QuintetMask> = BTreeSet::new();
        for shape in &shapes {
            for &(x, y) in shape.cells() {
                for (dx, dy) in ORTHOGONAL {
                    let (nx, ny) = (x + dx, y + dy);
                    if !shape.contains(nx, ny) {
                        let mut cells = shape.cells().to_vec();
                        cells.push((nx, ny));
                        grown.insert(QuintetMask::from_coords(&cells));
                    }
                }
            }
        }
        shapes = grown;
    }

    shapes.into_iter().collect()
}

/// The catalog shapes that have at least one valid placement on `board`.
pub fn placeable_shapes(board: &Board) -> Vec<QuintetMask> {
    catalog()
        .into_iter()
        .filter(|mask| can_place_anywhere(board, mask))
        .collect()
}

/// The next quintet for this board, or `None` when the board is stuck (no shape
/// fits anywhere). The choice is deterministic in `(board, score, moves)`.
pub fn generate(board: &Board, score: u64, moves: u64) -> Option<QuintetMask> {
    let placeable = placeable_shapes(board);
    (!placeable.is_empty()).then(|| {
        let mut rng = DeterministicRng::seeded(seed(board, score, moves));
        let index = rng.next_bounded(placeable.len() as u64) as usize;
        placeable[index].clone()
    })
}

/// Fold the board occupancy, score, and move count into a 64-bit seed (an
/// FNV-1a hash over the cells, mixed with score and moves).
fn seed(board: &Board, score: u64, moves: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for y in 0..BOARD_SIZE as i32 {
        for x in 0..BOARD_SIZE as i32 {
            hash ^= board.is_filled(x, y) as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash ^ score.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ moves.rotate_left(32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_all_valid_five_cell_shapes() {
        let shapes = catalog();
        // The 12 free pentominoes across all rotations/reflections give the 63
        // fixed pentominoes; every one is a legal quintet.
        assert_eq!(shapes.len(), 63);
        for shape in &shapes {
            assert_eq!(shape.count(), 5);
            assert!(shape.is_valid(), "catalog shape must be a valid quintet");
        }
    }

    #[test]
    fn generated_piece_has_exactly_five_cells_and_fits() {
        let board = Board::empty();
        let piece = generate(&board, 0, 0).expect("empty board is never stuck");
        assert_eq!(piece.count(), 5);
        assert!(piece.is_valid());
        assert!(can_place_anywhere(&board, &piece));
    }

    #[test]
    fn generation_is_deterministic_for_a_state() {
        let board = Board::empty();
        assert_eq!(generate(&board, 3, 7), generate(&board, 3, 7));
    }

    #[test]
    fn distinct_states_can_yield_distinct_pieces() {
        // The seed varies with score/moves, so at least some neighbouring states
        // differ — proves the seed actually feeds the choice.
        let board = Board::empty();
        let differ = (0..32).any(|m| generate(&board, 0, 0) != generate(&board, 0, m));
        assert!(differ, "the move counter must influence generation");
    }

    #[test]
    fn full_board_is_stuck() {
        let mut board = Board::empty();
        for y in 0..BOARD_SIZE as i32 {
            for x in 0..BOARD_SIZE as i32 {
                board.fill(x, y);
            }
        }
        assert!(placeable_shapes(&board).is_empty());
        assert!(generate(&board, 0, 0).is_none());
    }

    #[test]
    fn board_with_only_a_tiny_gap_is_stuck() {
        // Fill everything except a 2×2 corner pocket: no 5-cell shape fits.
        let mut board = Board::empty();
        for y in 0..BOARD_SIZE as i32 {
            for x in 0..BOARD_SIZE as i32 {
                board.fill(x, y);
            }
        }
        for y in 0..2 {
            for x in 0..2 {
                board.clear_cell(x, y);
            }
        }
        assert!(generate(&board, 0, 0).is_none());
    }
}
