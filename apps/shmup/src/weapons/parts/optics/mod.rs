//! Ported from Claude-of-Duty `src/weapons/parts.js` — optics and the pistol
//! slide: `buildOptic` (`:1215-1637`), `buildMiniReflex` (`:1886-1971`),
//! `buildSlide` (`:1971-2072`). Split into one file per builder — `buildOptic`
//! alone carries ~430 lines of source plus the comments documenting real
//! published dimensions and the reasoning behind them (segment budget,
//! aperture budget, absolute co-witness mount geometry), which the port
//! recipe requires carrying forward; one flat `optics.rs` would bury that
//! under the other two builders.
//!
//! See `docs/work-manifests/claude-of-duty-port/03-weapon-geometry-api.md`
//! for the fixed Rust primitive/`Assembly` API these are written against.

mod mini_reflex;
mod slide;
mod tube_sight;

pub use mini_reflex::{build_mini_reflex, MiniReflexOpts, MiniReflexResult};
pub use slide::{build_slide, SlideOpts, SlideResult};
pub use tube_sight::{build_optic, OpticOpts, OpticResult};
