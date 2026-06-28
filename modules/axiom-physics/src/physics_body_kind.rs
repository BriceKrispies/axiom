//! The three deterministic rigid-body kinds.

/// How a rigid body participates in the simulation.
///
/// The world supports exactly the three classical rigid-body kinds; there is
/// deliberately no `Character` kind (a documented deferral) and no `Trigger` kind
/// (a trigger is collider behavior, not a body kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicsBodyKind {
    /// Never moves. Infinite mass, zero inverse mass; unaffected by forces,
    /// impulses, or gravity.
    Static,
    /// Moves under accumulated force, impulse, and gravity. Finite positive
    /// mass, non-zero inverse mass.
    Dynamic,
    /// Moves only by explicit control (not by forces). Zero inverse mass; it
    /// does not self-advance and is unaffected by gravity.
    Kinematic,
}

impl PhysicsBodyKind {
    /// `true` iff this kind integrates under force/impulse/gravity (i.e.
    /// [`PhysicsBodyKind::Dynamic`]). The integrator reads this branchlessly to
    /// gate motion: only dynamic bodies accelerate.
    pub(crate) fn is_dynamic(self) -> bool {
        self == PhysicsBodyKind::Dynamic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_dynamic_is_dynamic() {
        assert!(PhysicsBodyKind::Dynamic.is_dynamic());
        assert!(!PhysicsBodyKind::Static.is_dynamic());
        assert!(!PhysicsBodyKind::Kinematic.is_dynamic());
    }

    #[test]
    fn derives_are_exercised() {
        let k = PhysicsBodyKind::Dynamic;
        let c = k;
        assert_eq!(k, c);
        assert_ne!(PhysicsBodyKind::Static, PhysicsBodyKind::Kinematic);
        assert!(format!("{k:?}").contains("Dynamic"));
    }
}
