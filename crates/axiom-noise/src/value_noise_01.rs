//! Smooth value noise over the [`crate::hash_01`] lattice.

use axiom_math::DVec3;

use crate::hash_01::hash_01;
use crate::unit_noise::UnitNoise;

/// The cubic smoothstep fade, `3t² - 2t³`.
///
/// Deliberately *not* Perlin's quintic `6t⁵ - 15t⁴ + 10t³` used by
/// [`crate::value_noise`]. The quintic has a zero second derivative at the cell
/// boundary, which matters when the noise is differentiated — for a normal map,
/// say. This basis is sampled for position-driven variation and never
/// differentiated, and the two curves give visibly different fields, so the
/// choice is part of the basis rather than an implementation detail to
/// harmonise away.
fn fade(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// The eight corners of a unit cell, as the `(dx, dy, dz)` bits of `0..8`.
///
/// The order is load-bearing: `dx` is bit 0, `dy` bit 1, `dz` bit 2, so the
/// corners are visited x-fastest, then y, then z. Floating-point addition is
/// not associative, so a different visit order is a different (if
/// imperceptibly) function — and this basis is pinned by exact-equality
/// goldens, which would catch the reordering as a failure rather than a drift.
fn corner_offsets(corner: u32) -> (u32, u32, u32) {
    (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1)
}

/// Trilinearly interpolated value noise over the integer-hash lattice, with a
/// period of one unit.
///
/// The position is split into its integer cell and fractional offset; the eight
/// surrounding corners are hashed by [`hash_01`]; and the eight samples are
/// blended by the [`fade`]-shaped fractional weights. Like the hash beneath it,
/// this is a pure function of position with no seed and no stream.
///
/// The result is a convex combination of values already in `[0, 1)` — the eight
/// weights are non-negative and sum to exactly one — so it cannot leave that
/// interval and [`UnitNoise`]'s clamp never fires.
pub fn value_noise_01(p: DVec3) -> UnitNoise {
    let cell = p.floor();
    let f = p.subtract(cell);
    let faded = DVec3::new(fade(f.x), fade(f.y), fade(f.z));

    let signal = (0..8u32)
        .map(|corner| {
            let (dx, dy, dz) = corner_offsets(corner);
            // `[1 - t, t][bit]` is the branchless read of "the far weight for
            // the near corner, the near weight for the far one".
            let wx = [1.0 - faded.x, faded.x][dx as usize];
            let wy = [1.0 - faded.y, faded.y][dy as usize];
            let wz = [1.0 - faded.z, faded.z][dz as usize];
            let corner_pos = cell.add(DVec3::new(
                f64::from(dx),
                f64::from(dy),
                f64::from(dz),
            ));
            hash_01(corner_pos).get() * wx * wy * wz
        })
        .sum::<f64>();

    UnitNoise::from_signal(signal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_is_the_cubic_smoothstep() {
        assert_eq!(fade(0.0), 0.0);
        assert_eq!(fade(1.0), 1.0);
        assert_eq!(fade(0.5), 0.5);
        // Strictly increasing and S-shaped: below the line under 0.5, above it
        // over 0.5. This is what distinguishes it from a linear blend.
        assert!(fade(0.25) < 0.25);
        assert!(fade(0.75) > 0.75);
    }

    #[test]
    fn corner_offsets_visit_x_fastest_then_y_then_z() {
        let visited: Vec<(u32, u32, u32)> = (0..8).map(corner_offsets).collect();
        assert_eq!(
            visited,
            vec![
                (0, 0, 0),
                (1, 0, 0),
                (0, 1, 0),
                (1, 1, 0),
                (0, 0, 1),
                (1, 0, 1),
                (0, 1, 1),
                (1, 1, 1),
            ]
        );
    }

    /// At an integer position every fractional weight is zero, so the blend
    /// collapses onto exactly one corner — the cell's own hash.
    #[test]
    fn integer_positions_reduce_to_the_corner_hash() {
        [
            DVec3::ZERO,
            DVec3::new(1.0, 1.0, 1.0),
            DVec3::new(-3.0, 7.0, 2.0),
        ]
        .into_iter()
        .for_each(|p| assert_eq!(value_noise_01(p), hash_01(p)));
    }

    #[test]
    fn samples_stay_in_the_unit_interval() {
        (0..64).for_each(|i| {
            let t = f64::from(i) * 0.31 - 8.0;
            let v = value_noise_01(DVec3::new(t, t * 0.5, -t * 0.25)).get();
            assert!((0.0..1.0).contains(&v), "value noise out of range: {v}");
        });
    }

    #[test]
    fn is_a_pure_function_of_position() {
        let p = DVec3::new(1.7, -0.3, 4.25);
        assert_eq!(value_noise_01(p), value_noise_01(p));
    }

    /// Continuity across a cell boundary: approaching an integer plane from
    /// below must converge on the value *at* it. A basis that failed this would
    /// show a hard seam on every lattice plane.
    #[test]
    fn is_continuous_across_a_cell_boundary() {
        let at = value_noise_01(DVec3::new(1.0, 0.5, 0.5)).get();
        let near = value_noise_01(DVec3::new(1.0 - 1.0e-9, 0.5, 0.5)).get();
        assert!((at - near).abs() < 1.0e-6);
    }

    /// The three axes must each actually drive the result — a transposition bug
    /// that dropped one would still pass a range check.
    #[test]
    fn every_axis_moves_the_sample() {
        let base = value_noise_01(DVec3::new(0.5, 0.5, 0.5)).get();
        assert_ne!(base, value_noise_01(DVec3::new(0.6, 0.5, 0.5)).get());
        assert_ne!(base, value_noise_01(DVec3::new(0.5, 0.6, 0.5)).get());
        assert_ne!(base, value_noise_01(DVec3::new(0.5, 0.5, 0.6)).get());
    }
}
