//! A graph-hop distance — a finite number of region hops, or an explicit
//! unreachable sentinel.

/// A distance measured in whole region hops over the [`RegionGraph`], as
/// produced by [`ocean_distance`](crate::ocean_distance): either a finite hop
/// count or the [`UNREACHABLE`](HopDistance::UNREACHABLE) sentinel (no path to a
/// source).
///
/// Mirrors the sanctioned `axiom-grid` `Dist` shape: the sentinel is a real
/// total-order maximum (every finite hop is `<` it), so a `min` over neighbours
/// naturally prefers a reachable region, and [`plus_one`](HopDistance::plus_one)
/// saturates so relaxing off an unreachable neighbour never fabricates a finite
/// distance. Being an integer newtype (not a float), it carries a unit — "region
/// hops" — without tripping the unitless-float rule.
///
/// [`RegionGraph`]: axiom_geosphere::RegionGraph
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HopDistance(u32);

impl HopDistance {
    /// A source region — zero hops from itself.
    pub const ZERO: HopDistance = HopDistance(0);
    /// The "no path to any source" sentinel; the total-order maximum.
    pub const UNREACHABLE: HopDistance = HopDistance(u32::MAX);

    /// The finite hop count, or `None` when unreachable. The branchless read a
    /// consumer uses to fold this into a moisture / decay field.
    pub fn steps(self) -> Option<u32> {
        (self != HopDistance::UNREACHABLE).then_some(self.0)
    }

    /// Whether this region reaches any source at all — `distance != UNREACHABLE`.
    pub fn is_reachable(self) -> bool {
        self != HopDistance::UNREACHABLE
    }

    /// One hop further out, saturating so `UNREACHABLE.plus_one()` stays
    /// `UNREACHABLE` (an unreachable neighbour never relaxes a region to a finite
    /// distance). Crate-internal: the relaxation step of the wavefront.
    pub(crate) fn plus_one(self) -> HopDistance {
        HopDistance(self.0.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_and_unreachable_read_distinctly() {
        assert_eq!(HopDistance::ZERO.steps(), Some(0));
        assert_eq!(HopDistance(4).steps(), Some(4));
        assert_eq!(HopDistance::UNREACHABLE.steps(), None);
        assert!(HopDistance::ZERO.is_reachable());
        assert!(!HopDistance::UNREACHABLE.is_reachable());
    }

    #[test]
    fn plus_one_steps_and_saturates_at_unreachable() {
        assert_eq!(HopDistance::ZERO.plus_one(), HopDistance(1));
        assert_eq!(HopDistance(9).plus_one(), HopDistance(10));
        // The sentinel never relaxes to a finite distance.
        assert_eq!(
            HopDistance::UNREACHABLE.plus_one(),
            HopDistance::UNREACHABLE
        );
    }

    #[test]
    fn unreachable_is_the_order_maximum() {
        assert!(HopDistance(u32::MAX - 1) < HopDistance::UNREACHABLE);
        assert_eq!(
            HopDistance::ZERO.min(HopDistance::UNREACHABLE),
            HopDistance::ZERO
        );
    }
}
