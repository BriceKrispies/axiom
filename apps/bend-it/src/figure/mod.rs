//! The soccer humanoid: one figure, two kits, and the poses that drive it.
//!
//! * [`model`] — the 17-box figure itself, descended from the End Zone arcade
//!   footballer with every piece of football equipment removed and a kit put on.
//! * [`pose`] — the pose type, the pieces poses are built from, and the
//!   distance-driven gait.
//! * [`rig`] — the visual body root, and the hop to world-space boxes (delegated
//!   to `axiom-figure`, which owns chain resolution).
//! * [`kicker`] — the run-up and the strike, timed so the boot meets the ball on
//!   the launch tick.
//! * [`keeper_pose`] — the keeper's body, which is simultaneously the pose drawn
//!   and the capsules the ball is tested against.

pub mod keeper_pose;
pub mod kicker;
pub mod model;
pub mod pose;
pub mod rig;

pub use keeper_pose::{keeper_frame, KeeperFrame, KeeperMotion};
pub use kicker::{kick_frame, KickPlan, STRIKE_FOOT};
pub use model::{soccer_figure, FIGURE_HEIGHT, PART_COUNT, PARTS, TAG_COUNT};
pub use pose::JointPose;
pub use rig::{body_transform, world_parts};
