//! The attempt: the explicit state machine and the four systems it steps.
//!
//! * [`phase`] — where the attempt is, as one enum.
//! * [`ball`] — on the authored path, or free after a real contact.
//! * [`dive_call`] — what the player asks for when they are the one in the goal.
//! * [`keeper`] — a commitment, and the physical dive that executes it.
//! * [`keeper_read`] — what it decides from one glimpse of the early flight.
//! * [`nerve`] — the seeded roll that makes one penalty unlike the next.
//! * [`resolution`] — what happened, and the tally.
//! * [`rival`] — the other team's taker, for the five kicks that are not yours.
//! * [`session`] — the machine that owns all four.
//! * [`shootout`] — five each, alternating, then sudden death: the frame that
//!   makes a penalty matter.

pub mod ball;
pub mod dive_call;
pub mod keeper;
pub mod keeper_read;
pub mod nerve;
pub mod phase;
pub mod resolution;
pub mod rival;
pub mod session;
pub mod shootout;

pub use ball::{Ball, BallMotion};
pub use dive_call::DiveCall;
pub use keeper::{drawable_prediction, Keeper};
pub use nerve::KeeperNerve;
pub use keeper_read::{predict_crossing, KeeperRead};
pub use phase::Phase;
pub use resolution::{ShotResult, Tally};
pub use session::{PlayCommand, Session};
pub use shootout::{Outcome, Shootout, Side, ROUNDS};
