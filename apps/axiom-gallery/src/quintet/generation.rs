//! Deterministic generation of the next quintet — always one that fits.
//!
//! Generation is a pure function of `(board, score, move-count)`, routed through
//! the engine's procedural-generation substrate (Phase 8 of the procgen roadmap:
//! this app's content is now recipe-driven). The game state is encoded into a
//! content [`Address`], a single-`draw` [`Recipe`] is evaluated at that address
//! under a fixed root seed ([`ProcApi`]), and the artifact's drawn word selects
//! one of the shapes that fit. So a given game state always produces the same next
//! piece and a whole game is replayable — now on `space`/`entropy`/`proc` rather
//! than a hand-rolled seed fed to a raw RNG.
//!
//! The shape pool is the [`catalog`] of every distinct *fixed* pentomino (all
//! rotations and reflections) — each one a real 5-cell orthogonally-connected
//! quintet by construction, so no diagonal line or disconnected shape can ever
//! be offered. From the pool we keep only the shapes that actually fit somewhere
//! on the current board ([`can_place_anywhere`]); the generator picks one of
//! those. If *no* shape fits, the board is stuck and generation yields `None`.

use std::collections::BTreeSet;

use axiom_kernel::StableHash;
use axiom_proc::{ProcApi, Recipe};
use axiom_space::{Address, SpaceApi};

use crate::quintet::board::{Board, BOARD_SIZE};
use crate::quintet::placement::can_place_anywhere;
use crate::quintet::quintet::{QuintetMask, ORTHOGONAL, QUINTET_CELLS};

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

/// The fixed root world seed for quintet generation ("Quintet" in ASCII). The
/// game state supplies the *address*, the recipe supplies the *version*; this is
/// the constant seed they are keyed under.
const GENERATION_SEED: u64 = 0x0051_7569_6e74_6574;
/// The piece-selection recipe version. Bump it to deliberately re-key generation
/// (and re-golden) — versioning is a first-class input.
const PIECE_RECIPE_VERSION: u32 = 1;

/// The piece-selection recipe: a single entropy draw whose value selects a shape.
/// Trivial by design — the substrate, not the recipe, is what this migration
/// proves; a richer recipe slots in here without touching the call site.
fn piece_recipe() -> Recipe {
    let mut recipe = Recipe::new(PIECE_RECIPE_VERSION);
    recipe.draw();
    recipe
}

/// Encode the game state `(board, score, moves)` into a content address: the
/// board occupancy is digested into one key, then score and moves are appended as
/// child segments, so distinct states are distinct sites.
fn site(board: &Board, score: u64, moves: u64) -> Address {
    let mut occupancy = Vec::with_capacity(BOARD_SIZE * BOARD_SIZE);
    for y in 0..BOARD_SIZE as i32 {
        for x in 0..BOARD_SIZE as i32 {
            occupancy.push(u8::from(board.is_filled(x, y)));
        }
    }
    let board_key = StableHash::of_bytes(&occupancy).raw();
    let board_site = SpaceApi::child(&SpaceApi::root(), board_key);
    let score_site = SpaceApi::child(&board_site, score);
    SpaceApi::child(&score_site, moves)
}

/// The next quintet for this board, or `None` when the board is stuck (no shape
/// fits anywhere). The choice is deterministic in `(board, score, moves)`,
/// produced by evaluating [`piece_recipe`] at the state's [`site`] and reducing
/// the artifact's drawn word over the placeable shapes.
pub fn generate(board: &Board, score: u64, moves: u64) -> Option<QuintetMask> {
    let placeable = placeable_shapes(board);
    (!placeable.is_empty()).then(|| {
        let address = site(board, score, moves);
        let (artifact, _trace) = ProcApi::evaluate(&piece_recipe(), GENERATION_SEED, &address)
            .expect("the single-draw piece recipe is a valid DAG");
        let draw = artifact.words()[0];
        let index = (draw % placeable.len() as u64) as usize;
        placeable[index].clone()
    })
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
    fn recipe_driven_generation_reproduces_across_a_state_sweep() {
        // Phase 8: every (board, score, moves) state reproduces its piece exactly
        // when re-evaluated — the substrate keying is fully deterministic.
        let board = Board::empty();
        for state in 0..64u64 {
            let (score, moves) = (state, state * 3 + 1);
            assert_eq!(
                generate(&board, score, moves),
                generate(&board, score, moves)
            );
        }
    }

    #[test]
    fn perturbing_then_restoring_the_state_restores_the_piece() {
        // Phase 8 metamorphic: a changed state can change the piece, and restoring
        // the exact state restores the exact piece (the address keys it).
        let board = Board::empty();
        let base = generate(&board, 4, 9);
        assert!((0..32).any(|m| generate(&board, 4, m) != base));
        assert_eq!(generate(&board, 4, 9), base);
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
