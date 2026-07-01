//! The physics module's identity vocabulary — the value-type handles the
//! [`crate::PhysicsApi`] facade returns and accepts.
//!
//! The handle *definitions* live in their own files (`physics_body_handle.rs`,
//! `physics_collider_handle.rs`); this module only re-exports them as the
//! crate's public noun vocabulary.

pub use crate::physics_body_handle::PhysicsBodyHandle;
pub use crate::physics_collider_handle::PhysicsColliderHandle;
