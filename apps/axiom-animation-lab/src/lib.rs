//! # Axiom Animation Lab (app)
//!
//! A tiny deterministic, headless proof app: scrub, play, and inspect a low-poly
//! humanoid rig performing a right-foot soccer kick. It composes the math layer
//! and the `axiom-animation` module into a frame-by-frame debug view — a ground
//! plane, a ball, the skeleton drawn as bone lines with joint markers, distinct
//! right-foot and plant-foot markers, and the `KickContact` frame marker —
//! emitted both as deterministic SVG ([`svg::render_frame`]) and as a terminal
//! inspection table ([`inspect::inspection_table`]).
//!
//! It is a composition **leaf**: nothing in the engine depends on it, and the
//! determinism it demonstrates comes from the animation module underneath. No
//! skinning, IK, physics, or blending — just a scrubbable kick.

pub mod inspect;
pub mod scene;
pub mod svg;
