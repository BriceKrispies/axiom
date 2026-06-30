//! Phase 1 — stored golden capture for the deterministic Quintet game.
//!
//! Quintet generation is a pure function of `(board, score, move-count)` via
//! the kernel's `DeterministicRng`, so a fixed placement script produces a
//! fully reproducible game. This test drives such a script — always placing
//! the current generated piece at its first valid board anchor — and pins the
//! resulting trajectory (the generated-piece cell sets, the board occupancy,
//! and the running score after each move) as committed golden bytes. Every
//! captured value is integer/cell data (no `f32`), so the bytes are
//! platform-stable.
//!
//! A *missing* golden is captured on the next run; an *existing* golden must
//! match. To re-capture after an intended change, delete the golden or run
//! with `AXIOM_REGOLD=1`, then review the diff.

use std::path::PathBuf;

use axiom_gallery::quintet::{Board, QuintetGame, BOARD_SIZE};

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("quintet/golden");
    p.push(format!("{name}.bin"));
    p
}

fn assert_golden(name: &str, actual: &[u8]) {
    let path = golden_path(name);
    let force = std::env::var_os("AXIOM_REGOLD").is_some();
    match std::fs::read(&path).ok() {
        Some(expected) if !force => assert_eq!(
            actual,
            expected.as_slice(),
            "golden mismatch for `{name}` ({} vs {} bytes): Quintet output \
             drifted. If intended, re-capture (delete the golden or set \
             AXIOM_REGOLD=1).",
            actual.len(),
            expected.len(),
        ),
        _ => {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create golden dir");
            std::fs::write(&path, actual).expect("write golden");
        }
    }
}

/// The first valid anchor for the current piece, scanning row-major. Returns
/// `None` only when the game is stuck.
fn first_valid_anchor(game: &QuintetGame) -> Option<(i32, i32)> {
    for ay in 0..BOARD_SIZE as i32 {
        for ax in 0..BOARD_SIZE as i32 {
            if game.can_place_current(ax, ay) {
                return Some((ax, ay));
            }
        }
    }
    None
}

/// Canonical bytes of a board's occupancy (row-major bit per cell, packed into
/// the byte stream one bool-byte per cell for simplicity and stability).
fn encode_board(out: &mut Vec<u8>, board: &Board) {
    for y in 0..BOARD_SIZE as i32 {
        for x in 0..BOARD_SIZE as i32 {
            out.push(u8::from(board.is_filled(x, y)));
        }
    }
}

/// Drive a deterministic placement script for up to `max_moves`, capturing the
/// full trajectory as canonical bytes: per move, the generated piece's cell
/// list, the chosen anchor, the post-placement score, and the board occupancy.
fn run_script(max_moves: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut game = QuintetGame::new();
    let mut placed = 0u32;
    for _ in 0..max_moves {
        // Record the current generated piece's cells (or a 0 count if stuck).
        match game.current() {
            Some(mask) => {
                let cells = mask.cells();
                out.extend_from_slice(&(cells.len() as u32).to_le_bytes());
                for &(cx, cy) in cells {
                    out.extend_from_slice(&cx.to_le_bytes());
                    out.extend_from_slice(&cy.to_le_bytes());
                }
            }
            None => out.extend_from_slice(&0u32.to_le_bytes()),
        }
        match first_valid_anchor(&game) {
            Some((ax, ay)) => {
                out.extend_from_slice(&ax.to_le_bytes());
                out.extend_from_slice(&ay.to_le_bytes());
                game.try_place(ax, ay);
                placed += 1;
                out.extend_from_slice(&game.score().to_le_bytes());
                out.extend_from_slice(&game.moves().to_le_bytes());
                encode_board(&mut out, game.board());
            }
            None => {
                // Stuck: mark with a sentinel anchor and stop.
                out.extend_from_slice(&(-1i32).to_le_bytes());
                out.extend_from_slice(&(-1i32).to_le_bytes());
                break;
            }
        }
    }
    // Trailing summary: total placed + final score.
    out.extend_from_slice(&placed.to_le_bytes());
    out.extend_from_slice(&game.score().to_le_bytes());
    out
}

#[test]
fn golden_quintet_script_trajectory() {
    assert_golden("quintet_script_trajectory", &run_script(40));
}

#[test]
fn script_is_deterministic() {
    // The whole script is a pure function — identical across runs.
    assert_eq!(run_script(40), run_script(40));
}

#[test]
fn the_script_actually_places_pieces() {
    // Guard against a degenerate golden: the captured trajectory must be
    // non-trivial (real placements happened, the bytes aren't just the
    // empty-board summary).
    let bytes = run_script(40);
    assert!(
        bytes.len() > 256,
        "the captured trajectory should span many moves, got {} bytes",
        bytes.len()
    );
}
