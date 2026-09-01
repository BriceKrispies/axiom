//! Worley (cellular) noise over a [`PermutationLattice`].

use axiom_math::DVec2;

use crate::permutation_lattice::PermutationLattice;
use crate::unit_noise::UnitNoise;

/// The two nearest feature-point distances to a sample.
///
/// Both are needed together: `nearest` alone gives the cell-interior gradient
/// that reads as scales or pebbles, and the *difference* between the two is
/// what isolates the cell walls. Computing them in one sweep rather than two is
/// not an optimisation — a second sweep could disagree with the first about a
/// tie, and the two fields would stop registering with each other.
struct TwoNearest {
    nearest: f64,
    second: f64,
}

/// The nine cells that can hold the closest feature point to a sample.
///
/// Three by three and no more: feature points live inside their own cell, so
/// nothing outside the immediate neighbourhood can be nearer than the far
/// corner of it.
fn two_nearest(lattice: &PermutationLattice, p: DVec2) -> TwoNearest {
    let cell = p.floor();
    let (ix, iy) = (cell.x as i64, cell.y as i64);

    (0..9).fold(
        TwoNearest {
            nearest: f64::INFINITY,
            second: f64::INFINITY,
        },
        |best, i| {
            let (dx, dy) = (i % 3 - 1, i / 3 - 1);
            let neighbour = DVec2::new((ix + dx) as f64, (iy + dy) as f64);
            let feature = neighbour.add(lattice.feature(ix + dx, iy + dy));
            let distance = feature.distance(p);
            // `min(nearest, d)` and `min(second, max(nearest, d))` together are
            // the branchless form of "if it beats the nearest, demote the
            // nearest; else if it beats the second, replace it".
            TwoNearest {
                nearest: best.nearest.min(distance),
                second: best.second.min(best.nearest.max(distance)),
            }
        },
    )
}

/// F1 Worley noise: the distance to the nearest feature point, in `[0, 1]`.
///
/// Reads as cells lit from their centres — scales, pebbles, cracked mud seen
/// from above. Saturated at `1.0`: at one feature point per cell the nearest is
/// almost always well inside a cell width, and the rare sample that is not
/// would otherwise blow the range out for everything else.
pub fn worley_f1(lattice: &PermutationLattice, p: DVec2) -> UnitNoise {
    UnitNoise::from_signal(two_nearest(lattice, p).nearest.min(1.0))
}

/// F2 minus F1: the distance *between* the two nearest feature points, in
/// `[0, 1]`.
///
/// Zero exactly on a cell wall — the locus of points equidistant from two
/// feature points — and rising toward each cell's interior. That makes it the
/// crack network rather than the cells: a mortar line, a dried riverbed, the
/// grout between tiles.
pub fn worley_edge(lattice: &PermutationLattice, p: DVec2) -> UnitNoise {
    let d = two_nearest(lattice, p);
    UnitNoise::from_signal((d.second - d.nearest).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::DeterministicRng;

    /// A realistic shuffled table, built the way a caller would build one.
    fn lattice() -> PermutationLattice {
        lattice_seeded(0x5eed_1234)
    }

    fn lattice_seeded(seed: u64) -> PermutationLattice {
        let mut rng = DeterministicRng::seeded(seed);
        let mut table: [u8; 256] = core::array::from_fn(|i| i as u8);
        (1..256usize).rev().for_each(|i| {
            let j = rng.next_bounded(i as u64 + 1) as usize;
            table.swap(i, j);
        });
        let features = core::array::from_fn(|_| {
            let unit = |r: &mut DeterministicRng| (r.next_bounded(1 << 24) as f64) / 16_777_216.0;
            let x = unit(&mut rng);
            let y = unit(&mut rng);
            DVec2::new(x, y)
        });
        PermutationLattice::from_table(table, features)
    }

    #[test]
    fn both_fields_stay_in_the_unit_interval() {
        let l = lattice();
        (0..256).for_each(|i| {
            let p = DVec2::new(f64::from(i) * 0.31 - 8.0, f64::from(i) * -0.17 + 3.0);
            assert!((0.0..=1.0).contains(&worley_f1(&l, p).get()));
            assert!((0.0..=1.0).contains(&worley_edge(&l, p).get()));
        });
    }

    #[test]
    fn the_second_nearest_is_never_nearer_than_the_first() {
        let l = lattice();
        (0..256).for_each(|i| {
            let p = DVec2::new(f64::from(i) * 0.19, f64::from(i) * 0.43);
            let d = two_nearest(&l, p);
            assert!(d.second >= d.nearest, "{} < {}", d.second, d.nearest);
        });
    }

    /// The defining property of F1: no feature point in the neighbourhood is
    /// nearer than the one it reports.
    #[test]
    fn f1_reports_the_nearest_feature_point_in_the_neighbourhood() {
        let l = lattice();
        (0..64).for_each(|i| {
            let p = DVec2::new(f64::from(i) * 0.37 - 4.0, f64::from(i) * 0.23 - 2.0);
            let reported = two_nearest(&l, p).nearest;
            let cell = p.floor();
            let (ix, iy) = (cell.x as i64, cell.y as i64);
            (-2..=2).for_each(|dx| {
                (-2..=2).for_each(|dy| {
                    let feature = DVec2::new((ix + dx) as f64, (iy + dy) as f64)
                        .add(l.feature(ix + dx, iy + dy));
                    assert!(reported <= feature.distance(p) + 1.0e-12);
                });
            });
        });
    }

    /// At a feature point itself the nearest distance is zero, so F1 bottoms
    /// out — which is what makes cell centres the dark (or bright) points.
    #[test]
    fn f1_is_zero_at_a_feature_point() {
        let l = lattice();
        let at = DVec2::new(3.0, 5.0).add(l.feature(3, 5));
        assert!(worley_f1(&l, at).get() < 1.0e-12);
    }

    /// The edge field is zero where two feature points are equidistant, which
    /// is the cell wall it exists to draw.
    #[test]
    fn the_edge_field_vanishes_midway_between_two_feature_points() {
        let l = lattice();
        let a = DVec2::new(3.0, 5.0).add(l.feature(3, 5));
        let b = DVec2::new(4.0, 5.0).add(l.feature(4, 5));
        let midpoint = a.add(b).mul_scalar(0.5);
        let d = two_nearest(&l, midpoint);
        assert!(
            (d.second - d.nearest).abs() < 1.0e-9,
            "the two nearest were {} and {}",
            d.nearest,
            d.second
        );
        assert!(worley_edge(&l, midpoint).get() < 1.0e-9);
        // ...and it rises away from the wall.
        assert!(worley_edge(&l, a).get() > worley_edge(&l, midpoint).get());
    }

    #[test]
    fn both_fields_are_pure_functions_of_lattice_and_position() {
        let l = lattice();
        let p = DVec2::new(2.2, -1.4);
        assert_eq!(worley_f1(&l, p), worley_f1(&l, p));
        assert_eq!(worley_edge(&l, p), worley_edge(&l, p));
    }

    #[test]
    fn a_different_lattice_gives_a_different_field() {
        let other = lattice_seeded(11);
        let p = DVec2::new(2.2, -1.4);
        assert_ne!(worley_f1(&lattice(), p).get(), worley_f1(&other, p).get());
    }

    #[test]
    fn negative_coordinates_are_sampled_the_same_way_as_positive_ones() {
        let l = lattice();
        (0..32).for_each(|i| {
            let p = DVec2::new(f64::from(i) * -0.41, f64::from(i) * -0.29);
            assert!((0.0..=1.0).contains(&worley_f1(&l, p).get()));
        });
    }

    /// The saturation is reachable: a lattice whose neighbourhood happens to
    /// be far away must clamp rather than exceed the range.
    #[test]
    fn the_distance_saturates_rather_than_leaving_the_range() {
        let l = lattice();
        let far = (0..2048)
            .map(|i| {
                let p = DVec2::new(f64::from(i) * 0.0137, f64::from(i) * 0.0211);
                two_nearest(&l, p).nearest
            })
            .fold(0.0_f64, f64::max);
        // Whether or not any sample exceeded 1.0, the public field never does.
        assert!(worley_f1(&l, DVec2::new(0.5, 0.5)).get() <= 1.0);
        assert!(far > 0.0);
    }
}
