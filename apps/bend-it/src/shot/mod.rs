//! The shot: what the player authored, and the one world-space path it means.
//!
//! Three files, three jobs, and the seams between them are the reason the game
//! can promise that the ball is never secretly steered:
//!
//! * [`path`] — the shape of a flight, as the player drew it: sampled offsets
//!   from the straight line, kept rather than fitted.
//! * [`curve`] — a compact two-parameter shape, now only a *generator* for the
//!   things that author a shot without a hand (the matrix, the agent, tests).
//! * [`intent`] — the authored shot as pure data (a point, a shape, a tempo).
//! * [`trajectory`] — the deterministic conversion of an intent into one
//!   arc-length-uniform world path, pinned to the ball at one end and to the
//!   authored point at the other.
//!
//! Gesture code may only ever write a [`ShotIntent`]; flight code may only ever
//! read a [`Trajectory`]. Neither names the other.

pub mod curve;
pub mod path;
pub mod intent;
pub mod trajectory;

pub use curve::BendCurve;
pub use path::{ShotPath, SHAPE_SAMPLES};
pub use intent::{GoalTarget, ShotIntent};
pub use trajectory::{shot_right, BallState, ResolvedShot, Trajectory};
