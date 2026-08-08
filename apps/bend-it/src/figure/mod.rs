//! The soccer humanoid: one figure, two kits, and the poses that drive it.
//!
//! * [`model`] — the 17-box figure itself, descended from the End Zone arcade
//!   footballer with every piece of football equipment removed and a kit put on.
//! * [`pose`] — the pose type, the pieces poses are built from, and the
//!   distance-driven gait.
//! * [`rig`] — the visual body root, and the hop to world-space boxes (delegated
//!   to `axiom-figure`, which owns chain resolution).
//! * [`ik`] — two-bone inverse kinematics: put the foot *there*, and let the
//!   joints work out how.
//! * [`joints`] — what a joint can actually DO. Every pose is put through the
//!   figure's own ranges of motion before anything draws or tests against it, so
//!   a solve cannot hand back a hip that abducts 125° or a knee that bends the
//!   wrong way.
//! * [`strike`] — what the drawing asks the body for, and the driven pendulum
//!   that is the striking leg. The contact tick comes out of the integration.
//! * [`kicker`] — the run-up, the plant and the swing, assembled: the body a
//!   drawing produces.
//! * [`keeper_pose`] — the keeper's body, which is simultaneously the pose drawn
//!   and the capsules the ball is tested against.

pub mod ik;
pub mod joints;
pub mod keeper_pose;
pub mod kicker;
pub mod model;
pub mod pose;
pub mod rig;
pub mod strike;

pub use keeper_pose::{arm_reach, keeper_frame, stretch_from_hips, KeeperFrame, KeeperMotion};
pub use kicker::{kick_frame, KickPlan, STRIKE_FOOT};
pub use model::{soccer_figure, FIGURE_HEIGHT, PART_COUNT, PARTS, TAG_COUNT};
pub use joints::{constrain, Range, RANGES};
pub use pose::JointPose;
pub use rig::{body_transform, world_parts};
pub use strike::{KickDrive, Swing};
