//! Deterministic per-step command input.
//! A command is a flattened, tag-dispatched record rather than a data-carrying
//! enum: the engine's Branchless Law forbids `match`-ing a payload enum, so —
//! exactly like the kernel's `RuntimeCommand` and sim-core's `Effect` — the
//! payload lives in flat fields and the [`PhysicsCommandKind`] discriminant
//! selects behavior by index (see the apply table in `physics_world.rs`). The
//! world drains commands in FIFO order before integration.

use axiom_math::Vec3;

use crate::physics_body_handle::PhysicsBodyHandle;

/// The five deterministic command kinds. The discriminant order is load-bearing:
/// it indexes the command-apply table in `physics_world.rs`, so the two must stay
/// in lock-step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhysicsCommandKind {
    /// Add a continuous force to a dynamic body.
    ApplyForce,
    /// Add an instantaneous impulse to a dynamic body.
    ApplyImpulse,
    /// Enable a body.
    EnableBody,
    /// Disable a body.
    DisableBody,
    /// Add a continuous torque to a dynamic body.
    ApplyTorque,
}

/// One queued command: a kind discriminant plus a flat payload (`body`, and a
/// `vector` carrying the force/impulse — zero for enable/disable).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PhysicsCommand {
    kind: PhysicsCommandKind,
    body: PhysicsBodyHandle,
    vector: Vec3,
}

impl PhysicsCommand {
    /// Stage a continuous force on `body`.
    pub(crate) fn apply_force(body: PhysicsBodyHandle, force: Vec3) -> Self {
        PhysicsCommand {
            kind: PhysicsCommandKind::ApplyForce,
            body,
            vector: force,
        }
    }

    /// Stage an instantaneous impulse on `body`.
    pub(crate) fn apply_impulse(body: PhysicsBodyHandle, impulse: Vec3) -> Self {
        PhysicsCommand {
            kind: PhysicsCommandKind::ApplyImpulse,
            body,
            vector: impulse,
        }
    }

    /// Stage a continuous torque on `body`.
    pub(crate) fn apply_torque(body: PhysicsBodyHandle, torque: Vec3) -> Self {
        PhysicsCommand {
            kind: PhysicsCommandKind::ApplyTorque,
            body,
            vector: torque,
        }
    }

    /// Stage enabling `body`.
    pub(crate) fn enable_body(body: PhysicsBodyHandle) -> Self {
        PhysicsCommand {
            kind: PhysicsCommandKind::EnableBody,
            body,
            vector: Vec3::ZERO,
        }
    }

    /// Stage disabling `body`.
    pub(crate) fn disable_body(body: PhysicsBodyHandle) -> Self {
        PhysicsCommand {
            kind: PhysicsCommandKind::DisableBody,
            body,
            vector: Vec3::ZERO,
        }
    }

    /// The command kind (used to index the apply table).
    pub(crate) fn kind(&self) -> PhysicsCommandKind {
        self.kind
    }

    /// The target body.
    pub(crate) fn body(&self) -> PhysicsBodyHandle {
        self.body
    }

    /// The force/impulse payload (zero for enable/disable).
    pub(crate) fn vector(&self) -> Vec3 {
        self.vector
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_kind_body_and_vector() {
        let b = PhysicsBodyHandle::from_raw(3);
        let f = PhysicsCommand::apply_force(b, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(f.kind(), PhysicsCommandKind::ApplyForce);
        assert_eq!(f.body(), b);
        assert_eq!(f.vector(), Vec3::new(1.0, 0.0, 0.0));

        let j = PhysicsCommand::apply_impulse(b, Vec3::new(0.0, 2.0, 0.0));
        assert_eq!(j.kind(), PhysicsCommandKind::ApplyImpulse);
        assert_eq!(j.vector(), Vec3::new(0.0, 2.0, 0.0));

        let t = PhysicsCommand::apply_torque(b, Vec3::new(0.0, 0.0, 3.0));
        assert_eq!(t.kind(), PhysicsCommandKind::ApplyTorque);
        assert_eq!(t.vector(), Vec3::new(0.0, 0.0, 3.0));

        assert_eq!(
            PhysicsCommand::enable_body(b).kind(),
            PhysicsCommandKind::EnableBody
        );
        assert_eq!(
            PhysicsCommand::disable_body(b).kind(),
            PhysicsCommandKind::DisableBody
        );
        assert_eq!(PhysicsCommand::enable_body(b).vector(), Vec3::ZERO);
    }

    #[test]
    fn discriminants_match_apply_table_indices() {
        assert_eq!(PhysicsCommandKind::ApplyForce as usize, 0);
        assert_eq!(PhysicsCommandKind::ApplyImpulse as usize, 1);
        assert_eq!(PhysicsCommandKind::EnableBody as usize, 2);
        assert_eq!(PhysicsCommandKind::DisableBody as usize, 3);
        assert_eq!(PhysicsCommandKind::ApplyTorque as usize, 4);
    }

    #[test]
    fn derives_are_exercised() {
        let c = PhysicsCommand::enable_body(PhysicsBodyHandle::from_raw(1));
        let d = c;
        assert_eq!(c, d);
        assert_ne!(c, PhysicsCommand::disable_body(PhysicsBodyHandle::from_raw(1)));
        assert!(format!("{c:?}").contains("EnableBody"));
    }
}
