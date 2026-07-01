//! # Quintet — a deterministic block-breaking placement game (Axiom app)
//!
//! Drag a generated **quintet** (a 5-cell polyomino) from the side panel onto a
//! 10×10 board. Filling a whole row or column clears it for score; the more
//! lines a single placement clears, the more each cleared block is worth. Every
//! quintet the generator offers is guaranteed to fit *somewhere* on the current
//! board — and when nothing can fit, the game shows a clear **stuck** state with
//! the reset button still available.
//!
pub mod board;
pub mod clearing;
pub mod game;
pub mod generation;
pub mod placement;
pub mod quintet;

#[cfg(target_arch = "wasm32")]
mod web;

pub use board::{Board, BOARD_SIZE};
pub use game::{PlaceResult, QuintetGame};
pub use quintet::{QuintetMask, QUINTET_CELLS};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_game_is_immediately_playable() {
        let game = QuintetGame::new();
        assert!(!game.is_stuck());
        assert_eq!(game.current().unwrap().count(), QUINTET_CELLS);
        assert_eq!(game.board().filled_count(), 0);
    }
}
