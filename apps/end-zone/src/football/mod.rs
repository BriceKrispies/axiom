//! The football subsystem: explicit ball states, deterministic ballistic
//! flight (integrated by the engine's physics facade), possession sockets, and
//! catch evaluation. The visual model lives in [`model`]; everything else here
//! is authoritative simulation.

pub mod flight;
pub mod model;
pub mod possession;
pub mod sim;
pub mod state;
pub mod targeting;

pub use flight::{predict_position, solve_throw, FlightInfo};
pub use targeting::{best, candidates, ThrowCandidate};
pub use possession::{carry_socket, evaluate_catch, CatchVerdict};
pub use state::{BallSim, BallState};
