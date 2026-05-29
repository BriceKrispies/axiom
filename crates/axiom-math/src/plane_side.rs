//! Which side of a plane a point sits on.

/// The signed classification of a point against a [`crate::Plane`].
///
/// `Front` is the half-space the plane's normal points into; `Back` is the
/// opposite side; `On` means the point lies within the plane's epsilon
/// tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaneSide {
    Front,
    Back,
    On,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(PlaneSide::Front, PlaneSide::Back);
        assert_ne!(PlaneSide::Front, PlaneSide::On);
        assert_ne!(PlaneSide::Back, PlaneSide::On);
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let s = PlaneSide::Front;
        let t = s;
        assert_eq!(s, t);
    }
}
