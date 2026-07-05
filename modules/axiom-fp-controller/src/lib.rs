//! # Axiom FP Controller — Engine Module
//!
//! Owns the engine's **first-person walk+look controller**: the deterministic
//! math that turns a frame of held movement/turn input plus mouse-look deltas
//! into a new ground [`Pose`], and builds the perspective camera view-projection
//! that pose implies.
//!
//! ## What it folds in
//! - [`FpController::step`] integrates one frame: accumulate key-turn + look yaw
//!   into `yaw` (about world +Y), accumulate look pitch into `pitch` (about local
//!   +X, clamped to the tuning's pitch limit), and translate the planar position
//!   by the forward/strafe axes **rotated by yaw only** — so looking up or down
//!   never tilts movement off the horizontal plane. This is the same discipline
//!   the scene layer's `ControllerSystem` applies to a scene node, expressed here
//!   as a standalone pose for apps that feed a camera matrix directly.
//! - [`FpController::eye_position`] seats the eye an [`WalkTuning`] eye-height
//!   above the terrain sample under the walker.
//! - [`FpController::view_projection`] builds the `proj · view` clip matrix for
//!   that eye through a [`Lens`].
//!
//! ## What it is / is not
//! - It **is** pure, deterministic, replayable math: the same
//!   `(Pose, MoveIntent, LookDelta, WalkTuning)` reproduces a byte-identical next
//!   pose and matrix, fully testable on native exactly as it drives the browser.
//! - It is **not** a browser event source. It never references `web_sys` /
//!   `KeyboardEvent` / `MouseEvent` / the DOM (Module Law #9). Decoding raw
//!   pointer-lock + key events into a [`MoveIntent`] / [`LookDelta`] lives in the
//!   app (the platform edge).
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioural facade — [`FpController`] — plus
//! the pure value-type vocabulary it traffics in ([`Pose`], [`MoveIntent`],
//! [`LookDelta`], [`WalkTuning`], [`Lens`]). Every operation is reached through
//! the facade.

mod controller;
mod ids;

pub use controller::FpController;
pub use ids::{Lens, LookDelta, MoveIntent, Pose, WalkTuning};
