//! The shot: what the player authored, and the one world-space path it means.
//!
//! Three files, three jobs, and the seams between them are the reason the game
//! can promise that the ball is never secretly steered:
//!
//! * [`curve`] — the compact two-parameter shape one projection can hold.
//! * [`intent`] — the authored shot as pure data (target + two curves).
//! * [`trajectory`] — the deterministic conversion of an intent into one
//!   arc-length-uniform world path, pinned to the ball at one end and to the
//!   authored point at the other.
//!
//! Gesture code may only ever write a [`ShotIntent`]; flight code may only ever
//! read a [`Trajectory`]. Neither names the other.

pub mod curve;
pub mod intent;
pub mod trajectory;

pub use curve::BendCurve;
pub use intent::{GoalTarget, ShotIntent};
pub use trajectory::{shot_right, BallState, ResolvedShot, Trajectory};
