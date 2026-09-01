//! The result of a closest-feature query between two shapes.

use crate::dvec3::DVec3;

/// The closest pair of points between two shapes, with the squared distance
/// between them.
///
/// Squared, not the distance: every caller either compares it against another
/// squared distance or against a squared radius, and the square root is a
/// transcendental that would be computed once per candidate and thrown away.
/// A capsule sweep evaluates this against every triangle in a BVH leaf.
///
/// `on_first` lies on the shape the query was called on, `on_second` on the
/// argument. `first_parameter` is where along the first shape the point sits
/// — `0.0` at its start, `1.0` at its end — which a caller needs to know
/// *when* along a swept segment the contact happened, not just where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DClosestPair {
    /// Squared distance between [`DClosestPair::on_first`] and
    /// [`DClosestPair::on_second`].
    pub distance_squared: f64,
    /// The closest point on the shape the query was called on.
    pub on_first: DVec3,
    /// The closest point on the shape passed as the argument.
    pub on_second: DVec3,
    /// Parameter along the first shape, in `[0, 1]`.
    pub first_parameter: f64,
    /// Parameter along the second shape, in `[0, 1]`.
    ///
    /// Meaningful only where the second shape *has* a parameter — a segment
    /// does, a triangle does not. Segment-vs-triangle leaves this at `0.0` on
    /// every path, and nothing reads it back; it is not a second barycentric
    /// coordinate in disguise.
    pub second_parameter: f64,
}

impl DClosestPair {
    /// A pair at infinite separation, for seeding a minimum fold.
    ///
    /// Every real candidate compares strictly less than this, so the first one
    /// always wins and the fold needs no "is this the first iteration" flag.
    pub const FARTHEST: DClosestPair = DClosestPair {
        distance_squared: f64::INFINITY,
        on_first: DVec3::ZERO,
        on_second: DVec3::ZERO,
        first_parameter: 0.0,
        second_parameter: 0.0,
    };

    /// The nearer of two pairs, keeping `self` on a tie.
    ///
    /// The tie rule is load-bearing rather than arbitrary: a segment touching a
    /// triangle exactly on an edge is equidistant from two of the sub-queries,
    /// and which one wins decides which contact point — and therefore which
    /// contact normal — a solver is handed. Keeping the earlier candidate makes
    /// that a property of the query order, which is fixed, instead of a
    /// property of floating-point luck.
    pub fn nearer(self, other: DClosestPair) -> DClosestPair {
        [self, other][usize::from(other.distance_squared < self.distance_squared)]
    }

    /// The distance between the two points.
    pub fn distance(self) -> f64 {
        self.distance_squared.sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(d2: f64) -> DClosestPair {
        DClosestPair {
            distance_squared: d2,
            ..DClosestPair::FARTHEST
        }
    }

    #[test]
    fn the_farthest_pair_loses_to_every_real_candidate() {
        assert_eq!(
            DClosestPair::FARTHEST.nearer(pair(1.0e30)).distance_squared,
            1.0e30
        );
    }

    #[test]
    fn nearer_picks_the_smaller_squared_distance() {
        assert_eq!(pair(4.0).nearer(pair(1.0)).distance_squared, 1.0);
        assert_eq!(pair(1.0).nearer(pair(4.0)).distance_squared, 1.0);
    }

    #[test]
    fn a_tie_keeps_the_earlier_candidate() {
        let first = DClosestPair {
            on_first: DVec3::UNIT_X,
            ..pair(2.0)
        };
        let second = DClosestPair {
            on_first: DVec3::UNIT_Y,
            ..pair(2.0)
        };
        assert_eq!(first.nearer(second).on_first, DVec3::UNIT_X);
    }

    #[test]
    fn distance_is_the_root_of_the_squared_distance() {
        assert_eq!(pair(9.0).distance(), 3.0);
        assert_eq!(pair(0.0).distance(), 0.0);
    }

}
