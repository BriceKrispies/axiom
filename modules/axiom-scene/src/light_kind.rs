//! Coarse light-type enumeration.

/// The coarse type of a [`crate::Light`].
///
/// `axiom-scene` only models the two light shapes a deterministic engine
/// frame needs to *describe*: directional and point. Shadowing, area
/// lights, IES profiles, photometry, and image-based lighting are
/// renderer concerns, not scene-module concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightKind {
    Directional,
    Point,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(LightKind::Directional, LightKind::Point);
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let a = LightKind::Directional;
        let b = a;
        assert_eq!(a, b);
    }
}
