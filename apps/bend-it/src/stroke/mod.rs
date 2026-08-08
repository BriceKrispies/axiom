//! The drawing: capture a line, read it as a shot, and show it while it lasts.
//!
//! This is the whole player interface. There is one gesture — draw a line — and
//! four files behind it:
//!
//! * [`line`] — the polyline itself: decimation, length, orientation, resampling.
//! * [`capture`] — neutral pointer samples in, a finished drawing out.
//! * [`interpret`] — the reading: a drawing becomes the closest legal shot.
//! * [`fit`] — the maths that does it: a closed-form least-squares solve onto
//!   the space of shots the kicker can take.
//! * [`pace`] — how fast it was drawn, and what that means for the ball.
//! * [`view`] — what the screen draws while the line exists.
//!
//! Nothing in here may touch the shot after it has been read. The interpretation
//! produces a [`ShotIntent`] and hands it over; from that moment the trajectory
//! layer owns the shot and the ball follows what was drawn.
//!
//! [`ShotIntent`]: crate::shot::ShotIntent

pub mod capture;
pub mod fit;
pub mod interpret;
pub mod line;
pub mod pace;
pub mod view;

pub use capture::{Drawing, StrokeCapture};
pub use interpret::{interpret, Reading};
pub use line::Stroke;
pub use pace::Pace;
pub use view::{hint_for, speed_readout, GameView, StrokeView};
