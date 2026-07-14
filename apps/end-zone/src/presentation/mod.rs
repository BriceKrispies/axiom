//! Presentation: the immutable per-tick snapshot ([`snapshot`]), the
//! event-driven juice stack ([`juice`]), and the deterministic particle
//! shapes ([`particles`]). Nothing in here can mutate the simulation; every
//! effect is bounded, clamped, seeded from `app seed ^ event id`, and decays
//! to exactly zero.

pub mod juice;
pub mod particles;
pub mod snapshot;

pub use juice::{Effect, EffectKind, JuiceStack};
pub use particles::{effect_instances, trail_instances, EffectInstance, EffectMaterial};
pub use snapshot::{capture, PlayerView, PresentationSnapshot};
