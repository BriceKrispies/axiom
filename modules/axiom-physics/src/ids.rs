//! The physics module's identity vocabulary — the value-type handles the
//! [`crate::PhysicsApi`] facade returns and accepts.
//!
//! Kept in an `ids` module so the single `pub use ids::{…}` line in `lib.rs` is
//! published as identity vocabulary (Module Law #8), not counted as a second
//! behavioral facade. The handle *definitions* live in their own files
//! (`physics_body_handle.rs`, `physics_collider_handle.rs`); this module only
//! re-exports them as the crate's public noun vocabulary.

pub use crate::physics_body_handle::PhysicsBodyHandle;
pub use crate::physics_collider_handle::PhysicsColliderHandle;
