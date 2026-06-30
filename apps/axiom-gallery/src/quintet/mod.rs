//! # Quintet — a deterministic block-breaking placement game (Axiom app)
//!
//! Drag a generated **quintet** (a 5-cell polyomino) from the side panel onto a
//! 10×10 board. Filling a whole row or column clears it for score; the more
//! lines a single placement clears, the more each cleared block is worth. Every
//! quintet the generator offers is guaranteed to fit *somewhere* on the current
//! board — and when nothing can fit, the game shows a clear **stuck** state with
//! the reset button still available.
//!
//! ## Architecture (see `ARCHITECTURE.md`)
//!
//! This is an Axiom **app** — a composition leaf, exempt from the branchless and
//! 100%-coverage spine gates — so all gameplay lives here, never pushed into a
//! layer or module:
//!
//! * The **game core** ([`board`], [`quintet`], [`placement`], [`clearing`],
//!   [`generation`], [`game`]) is pure, deterministic Rust. Generation is a pure
//!   function of `(board, score, move-count)` via the kernel's
//!   [`axiom_kernel::DeterministicRng`] — the only engine dependency, used
//!   genuinely so the game reads no wall clock and no unseeded entropy.
//! * The **browser shell** (`web`, wasm32-only) is a thin 2D-`<canvas>` adapter
//!   over the pure core, with pointer drag-and-drop — the same app-local
//!   presentation pattern `axiom-roomed-puzzle` and `axiom-growth` use. It is the
//!   only place DOM/canvas APIs appear, and it is never compiled on native, so
//!   the core and `cargo test` stay browser-free.

// --- The deterministic game core ---
pub mod board;
pub mod clearing;
pub mod game;
pub mod generation;
pub mod placement;
pub mod quintet;

// --- In-browser play surface (wasm32 only) ---
// The 2D-canvas presentation arm. Never compiled on native, so the deterministic
// core and `cargo test` are untouched.
#[cfg(target_arch = "wasm32")]
mod web;

// Headline re-exports for ergonomic `use axiom_quintet::...`.
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
