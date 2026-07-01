//! # Roomed Puzzle — a deterministic top-down 2D grid puzzle (Axiom app)
//!
//! A solid player block walks a small room one cell at a time. Pressing **q**
//! freezes the current life into a **ghost** that replays the recorded path on a
//! deterministic fixed-step cadence (one move every 0.5 s); pressing **r**
//! restarts the level fresh. Ghosts are solid: they occupy cells, block
//! movement, and can stand on buttons that open doors — so the way through a
//! locked door is to leave a ghost holding the button and walk the live player
//! through.
//!
//! ## Architecture (see `ARCHITECTURE.md`)
//!
//! This is an Axiom **app** — a composition leaf, exempt from the branchless and
//! 100%-coverage spine gates — so all gameplay lives here, never pushed down into
//! a layer or module:
//!
//! * The **game core** ([`game_state`], [`game_step`], [`ghost_replay`], …) is
//!   pure, deterministic Rust. It reads no wall clock — time is the kernel's
//!   [`axiom_kernel::SimulationClock`], advanced one [`axiom_kernel::FixedStep`]
//!   per `Tick`. The only engine dependency is the kernel, used genuinely for
//!   that deterministic time.
//! * The **edit / playtest surfaces** ([`editor_model`], [`playtest_model`],
//!   [`app`]) are also pure; the [`render_model`] turns state into a neutral,
//!   depth-cued draw description.
//! * The **browser shell** (`web`, wasm32-only) is a thin 2D-`<canvas>` adapter
//!   over the pure core — the same app-local presentation pattern `axiom-growth`
//!   uses. It is the only place DOM/canvas APIs appear, and it is never compiled
//!   on native, so the core and `cargo test` stay browser-free.

pub mod coord;
pub mod direction;
pub mod group_id;
pub mod tile_kind;

pub mod level_codec;
pub mod level_definition;
pub mod level_validation;

pub mod actor_state;
pub mod ghost_replay;

pub mod game_command;
pub mod game_state;
pub mod game_step;

pub mod app;
pub mod editor_model;
pub mod input_mapping;
pub mod playtest_model;
pub mod render_model;

#[cfg(target_arch = "wasm32")]
mod web;

/// The built-in first level, embedded so the app (and tests) need no filesystem.
pub const LEVEL_001_TOML: &str = include_str!("levels/001-button-door.toml");

pub use app::{Mode, RoomedPuzzleApp};
pub use direction::Direction;
pub use game_command::{PuzzleCommand, PuzzleStepResult, StepKind};
pub use game_state::PuzzleGameState;
pub use level_definition::LevelDefinition;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_level_001_parses_and_validates() {
        let level = level_codec::from_toml(LEVEL_001_TOML).expect("embedded level parses");
        assert_eq!(level.title, "Button Door");
        assert!(level_validation::validate_level(&level).is_valid());
    }
}
