//! The reusable camera framework: named modes ([`modes`]), a critically
//! damped spring rig ([`rig`]), an additive impulse stack ([`impulse`]), and
//! the event-driven director ([`director`]) that composes them:
//!
//! `final camera pose = directed base pose + camera impulse stack`
//!
//! Everything advances on fixed simulation ticks; shake never drifts the base.

pub mod director;
pub mod impulse;
pub mod modes;
pub mod rig;

pub use director::CameraDirector;
pub use impulse::{CameraImpulse, ImpulseSample, ImpulseStack};
pub use modes::{CameraMode, CameraPose};
