//! The attempt: the explicit state machine and the four systems it steps.
//!
//! * [`phase`] — where the attempt is, as one enum.
//! * [`ball`] — on the authored path, or free after a real contact.
//! * [`keeper`] — a commitment, and the physical dive that executes it.
//! * [`keeper_read`] — what it decides from one glimpse of the early flight.
//! * [`resolution`] — what happened, and the tally.
//! * [`session`] — the machine that owns all four.

pub mod ball;
pub mod keeper;
pub mod keeper_read;
pub mod phase;
pub mod resolution;
pub mod session;

pub use ball::{Ball, BallMotion};
pub use keeper::{drawable_prediction, Keeper};
pub use keeper_read::{predict_crossing, KeeperRead};
pub use phase::{Phase, Projection};
pub use resolution::{ShotResult, Tally};
pub use session::{EditorCommand, Session};
