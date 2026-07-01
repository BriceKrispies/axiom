//! A BFS distance — a finite step count or an explicit unreachable sentinel.

/// A distance on the [`distance field`](crate::GridApi::distance_field): a finite
/// number of grid steps, or the [`UNREACHABLE`](Dist::UNREACHABLE) sentinel.
///
/// Projected across the authoring boundary as `number | Infinity` — never a raw
/// float, and the sentinel is a real total-order maximum (every finite distance
/// is `<` it), so `min` over neighbors naturally prefers a reachable cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dist(u32);

impl Dist {
    /// The origin cell of a distance field — zero steps.
    pub const ZERO: Dist = Dist(0);
    /// The "no path" sentinel; the total-order maximum, projected as `Infinity`.
    pub const UNREACHABLE: Dist = Dist(u32::MAX);

    /// The finite step count, or `None` when unreachable. The branchless read an
    /// author (or the wasm projection) uses to turn a cell into `number |
    /// Infinity`.
    pub fn steps(self) -> Option<u32> {
        (self != Dist::UNREACHABLE).then_some(self.0)
    }

    /// Whether this cell is reachable at all — `distance != Infinity`.
    pub fn is_reachable(self) -> bool {
        self != Dist::UNREACHABLE
    }

    /// One step further out, saturating so `UNREACHABLE.plus_one()` stays
    /// `UNREACHABLE` (an unreachable neighbor never relaxes a cell to a finite
    /// distance). Crate-internal: the relaxation step of the wavefront.
    pub(crate) fn plus_one(self) -> Dist {
        Dist(self.0.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_and_unreachable_read_distinctly() {
        assert_eq!(Dist::ZERO.steps(), Some(0));
        assert_eq!(Dist(5).steps(), Some(5));
        assert_eq!(Dist::UNREACHABLE.steps(), None);
        assert!(Dist::ZERO.is_reachable());
        assert!(!Dist::UNREACHABLE.is_reachable());
    }

    #[test]
    fn plus_one_steps_and_saturates_at_unreachable() {
        assert_eq!(Dist::ZERO.plus_one(), Dist(1));
        assert_eq!(Dist(7).plus_one(), Dist(8));
        assert_eq!(Dist::UNREACHABLE.plus_one(), Dist::UNREACHABLE);
    }

    #[test]
    fn unreachable_is_the_order_maximum() {
        assert!(Dist(u32::MAX - 1) < Dist::UNREACHABLE);
        assert_eq!(Dist::ZERO.min(Dist::UNREACHABLE), Dist::ZERO);
    }
}
