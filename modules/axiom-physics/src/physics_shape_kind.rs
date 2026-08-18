//! The five deterministic collider-shape discriminants.

/// Which primitive a [`crate::physics_collider_shape::PhysicsColliderShape`] is.
///
/// This is the **fieldless tag** of the (otherwise flat) shape value. Keeping the
/// discriminant separate from the geometry is what lets the broad phase, narrow
/// phase, and queries read a shape's parameters with plain field access and
/// dispatch on `kind as usize` into function tables — never a `match` on a
/// payload-carrying enum (the Branchless Law). The declaration order **is** the
/// table order: `Sphere = 0`, `Box = 1`, `Capsule = 2`, `Plane = 3`,
/// `Heightfield = 4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PhysicsShapeKind {
    /// A sphere — finite, rounded, AABB-bounded.
    Sphere,
    /// An axis-aligned box — finite, AABB-bounded.
    Box,
    /// A capsule (cylinder + hemispherical caps along local Y) — finite,
    /// AABB-bounded.
    Capsule,
    /// An infinite half-space plane — **not** AABB-bounded (it has no finite
    /// extent), so it never participates in the finite broad phase and is always
    /// a narrow-phase candidate.
    Plane,
    /// A static single-valued **heightfield** surface — finite (the grid's XZ
    /// footprint + height range give it a bounding box), oriented like a box.
    Heightfield,
}

impl PhysicsShapeKind {
    /// How many kinds there are — the exact length every dispatch table keyed on
    /// [`PhysicsShapeKind::index`] must have.
    ///
    /// This constant is not decoration: it is what makes the tables *impossible*
    /// to under-size. A table written as `[RayFn; 4]` while the enum grew a fifth
    /// variant compiles cleanly and panics at runtime the first time the new kind
    /// is queried — which is precisely the defect that shipped when `Heightfield`
    /// was added and only the narrow phase's table was widened. Sizing every table
    /// by `COUNT` (or `COUNT * COUNT` for the pairing matrix) turns that silent
    /// runtime panic into a compile error at each dispatch site, so adding a sixth
    /// kind cannot be half-done.
    pub(crate) const COUNT: usize = 5;

    /// `true` iff the shape has a finite axis-aligned bounding box (everything
    /// except [`PhysicsShapeKind::Plane`]). The broad phase reads this to decide
    /// whether a collider can be culled by AABB overlap.
    pub(crate) fn is_finite(self) -> bool {
        self != PhysicsShapeKind::Plane
    }

    /// The stable table index (`0..5`) used to dispatch contact generation and
    /// AABB construction without branching.
    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_is_the_only_non_finite_kind() {
        assert!(PhysicsShapeKind::Sphere.is_finite());
        assert!(PhysicsShapeKind::Box.is_finite());
        assert!(PhysicsShapeKind::Capsule.is_finite());
        assert!(!PhysicsShapeKind::Plane.is_finite());
        assert!(PhysicsShapeKind::Heightfield.is_finite());
    }

    #[test]
    fn indices_follow_declaration_order() {
        assert_eq!(PhysicsShapeKind::Sphere.index(), 0);
        assert_eq!(PhysicsShapeKind::Box.index(), 1);
        assert_eq!(PhysicsShapeKind::Capsule.index(), 2);
        assert_eq!(PhysicsShapeKind::Plane.index(), 3);
        assert_eq!(PhysicsShapeKind::Heightfield.index(), 4);
    }

    #[test]
    fn count_covers_every_declared_kind() {
        // Every kind's index must be addressable in a `COUNT`-sized table — the
        // invariant the dispatch tables rely on to be exhaustive.
        let kinds = [
            PhysicsShapeKind::Sphere,
            PhysicsShapeKind::Box,
            PhysicsShapeKind::Capsule,
            PhysicsShapeKind::Plane,
            PhysicsShapeKind::Heightfield,
        ];
        assert_eq!(kinds.len(), PhysicsShapeKind::COUNT);
        assert!(kinds.iter().all(|k| k.index() < PhysicsShapeKind::COUNT));
        assert_eq!(
            kinds
                .iter()
                .map(|k| k.index())
                .max()
                .map(|m| m + 1)
                .unwrap_or(0),
            PhysicsShapeKind::COUNT
        );
    }

    #[test]
    fn derives_are_exercised() {
        let k = PhysicsShapeKind::Capsule;
        let c = k;
        assert_eq!(k, c);
        assert_ne!(PhysicsShapeKind::Sphere, PhysicsShapeKind::Box);
        assert!(format!("{k:?}").contains("Capsule"));
    }
}
