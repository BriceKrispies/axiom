//! Deterministic physics lifecycle events.

use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_handle::PhysicsColliderHandle;

/// A deterministic event emitted by a [`crate::PhysicsApi`] world, in the order
/// it occurred.
///
/// The world currently emits only the **lifecycle** events listed below.
/// Collision/trigger lifecycle events — contact enter/stay/exit and trigger
/// events — are not implemented yet; they are a documented deferral that arrives
/// with the contact lifecycle (see `ROADMAP.md`). Because the only event variants
/// that exist are the five below, the "no collision events" invariant holds by
/// construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsEvent {
    /// A body was created.
    BodyCreated { body: PhysicsBodyHandle },
    /// A collider was attached to a body.
    ColliderAttached {
        collider: PhysicsColliderHandle,
        body: PhysicsBodyHandle,
    },
    /// A body was enabled by a command.
    BodyEnabled { body: PhysicsBodyHandle },
    /// A body was disabled by a command.
    BodyDisabled { body: PhysicsBodyHandle },
    /// A world step completed, carrying the completed step index.
    StepCompleted { step_index: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_carry_their_payloads() {
        let b = PhysicsBodyHandle::from_raw(1);
        let c = PhysicsColliderHandle::from_raw(2);
        assert_eq!(
            PhysicsEvent::BodyCreated { body: b },
            PhysicsEvent::BodyCreated { body: b }
        );
        assert_ne!(
            PhysicsEvent::BodyEnabled { body: b },
            PhysicsEvent::BodyDisabled { body: b }
        );
        let attached = PhysicsEvent::ColliderAttached {
            collider: c,
            body: b,
        };
        assert!(format!("{attached:?}").contains("ColliderAttached"));
        let step = PhysicsEvent::StepCompleted { step_index: 7 };
        let copied = step;
        assert_eq!(step, copied);
        assert!(format!("{step:?}").contains("StepCompleted"));
    }
}
