//! Classic gradient noise over a [`PermutationLattice`], and its fractal forms.

use axiom_kernel::Ratio;
use axiom_math::DVec2;

use crate::lacunarity::Lacunarity;
use crate::warp_strength::WarpStrength;

use crate::permutation_lattice::PermutationLattice;
use crate::signed_noise::SignedNoise;
use crate::unit_noise::UnitNoise;

/// Perlin's quintic fade, `6t⁵ - 15t⁴ + 10t³`.
///
/// The quintic, not the cubic [`crate::value_noise_01`] uses: its *second*
/// derivative is zero at a cell boundary as well as its first, so a field
/// differentiated for a normal map has no visible creases on the lattice
/// planes. A gradient basis is the one that gets differentiated, which is why
/// the two bases fade differently rather than sharing one curve.
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Empirical scale that carries the raw gradient interpolation to roughly the
/// full `[-1, 1]`.
///
/// A 2D gradient basis peaks near `1/sqrt(2)` rather than `1`, so the raw value
/// never reaches the ends of its nominal range and every downstream remap is
/// low-contrast. The constant is measured, not derived — which is why it is
/// named here once instead of appearing as a bare multiply.
const RANGE_SCALE: f64 = 1.42;

/// Single-octave gradient noise, roughly `[-1, 1]`.
///
/// The position is split into its integer cell and fractional offset; each of
/// the four surrounding cells contributes the dot product of its gradient with
/// the offset to that corner; the four are blended by the [`fade`]-shaped
/// weights.
pub fn perlin_2d(lattice: &PermutationLattice, p: DVec2) -> SignedNoise {
    let cell = p.floor();
    let f = p.subtract(cell);
    let (ix, iy) = (cell.x as i64, cell.y as i64);
    let (u, v) = (fade(f.x), fade(f.y));

    let corner = |dx: i64, dy: i64| {
        let offset = DVec2::new(f.x - dx as f64, f.y - dy as f64);
        lattice.gradient(ix + dx, iy + dy).dot(offset)
    };

    let lower = corner(0, 0) + u * (corner(1, 0) - corner(0, 0));
    let upper = corner(0, 1) + u * (corner(1, 1) - corner(0, 1));
    SignedNoise::from_signal((lower + v * (upper - lower)) * RANGE_SCALE)
}

/// The accumulated `(sum, normaliser)` of a fractal sweep, and the position it
/// reached.
///
/// A fold over octaves carries all three; naming the tuple keeps the three
/// fractal variants below reading as the one shape they are.
struct Octaves {
    sum: f64,
    normaliser: f64,
}

/// Fold `octaves` layers of a per-octave sample, each at `lacunarity` times the
/// frequency and `gain` times the amplitude of the last.
///
/// The starting amplitude cancels against the normaliser exactly, so it is not
/// a parameter — see [`crate::value_fbm_01`] for the argument.
fn sweep(
    lattice: &PermutationLattice,
    p: DVec2,
    octaves: u32,
    lacunarity: f64,
    gain: f64,
    sample: impl Fn(SignedNoise) -> f64,
) -> Octaves {
    let (sum, normaliser, ..) = (0..octaves).fold((0.0, 0.0, 1.0, 1.0), |(sum, norm, amp, freq), _| {
        let value = sample(perlin_2d(lattice, p.mul_scalar(freq)));
        (
            sum + value * amp,
            norm + amp,
            amp * gain,
            freq * lacunarity,
        )
    });
    Octaves { sum, normaliser }
}

/// Fractal Brownian motion over [`perlin_2d`], remapped into `[0, 1]`.
///
/// `octaves = 0` yields `0.5` — the midpoint, which is what an unset mask
/// should read as. (The quotient is NaN with no octaves; `UnitNoise` maps that
/// to `0.0` and the remap then lands it at the middle of the range.)
pub fn perlin_fbm_2d(
    lattice: &PermutationLattice,
    p: DVec2,
    octaves: u32,
    lacunarity: Lacunarity,
    gain: Ratio,
) -> UnitNoise {
    let swept = sweep(
        lattice,
        p,
        octaves,
        f64::from(lacunarity.get()),
        f64::from(gain.get()),
        |n| n.get(),
    );
    SignedNoise::from_signal(swept.sum / swept.normaliser).to_unit()
}

/// Ridged multifractal over [`perlin_2d`], in `[0, 1]` — veins, cracks and
/// filaments.
///
/// Each octave contributes `(1 - |perlin|)²`, which turns the basis inside out:
/// the *zero crossings* become ridges instead of the peaks, and squaring
/// sharpens them. That is what makes it read as a crack network rather than as
/// cloud.
pub fn perlin_ridged_2d(
    lattice: &PermutationLattice,
    p: DVec2,
    octaves: u32,
    lacunarity: Lacunarity,
    gain: Ratio,
) -> UnitNoise {
    let swept = sweep(
        lattice,
        p,
        octaves,
        f64::from(lacunarity.get()),
        f64::from(gain.get()),
        |n| {
            let ridge = 1.0 - n.get().abs();
            ridge * ridge
        },
    );
    UnitNoise::from_signal(swept.sum / swept.normaliser)
}

/// Domain-warped [`perlin_fbm_2d`] — the cheapest way to stop noise looking
/// like noise.
///
/// The sample position is displaced by a second, lower-frequency evaluation of
/// the same basis before the fractal sweep, which bends the field's features
/// along themselves instead of leaving them isotropic.
///
/// The two warp offsets are sampled at deliberately unrelated positions. Using
/// one sample for both axes would displace every point along the diagonal and
/// shear the field rather than warping it.
pub fn perlin_warped_2d(
    lattice: &PermutationLattice,
    p: DVec2,
    warp: WarpStrength,
    octaves: u32,
    lacunarity: Lacunarity,
    gain: Ratio,
) -> UnitNoise {
    const WARP_FREQUENCY: f64 = 0.7;
    const X_OFFSET: DVec2 = DVec2::new(13.1, -4.2);
    const Y_OFFSET: DVec2 = DVec2::new(-8.6, 21.5);

    let base = p.mul_scalar(WARP_FREQUENCY);
    let strength = f64::from(warp.get());
    let dx = perlin_2d(lattice, base.add(X_OFFSET)).get() * strength;
    let dy = perlin_2d(lattice, base.add(Y_OFFSET)).get() * strength;
    perlin_fbm_2d(lattice, p.add(DVec2::new(dx, dy)), octaves, lacunarity, gain)
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

    fn half() -> Ratio {
        Ratio::new(0.5).unwrap()
    }

    fn drift() -> Lacunarity {
        Lacunarity::new(2.03).unwrap()
    }

    fn ridged_drift() -> Lacunarity {
        Lacunarity::new(2.11).unwrap()
    }

    fn warp(v: f32) -> WarpStrength {
        WarpStrength::new(v).unwrap()
    }

    #[test]
    fn fade_is_the_quintic_with_zero_first_and_second_derivatives_at_the_ends() {
        assert_eq!(fade(0.0), 0.0);
        assert_eq!(fade(1.0), 1.0);
        assert_eq!(fade(0.5), 0.5);
        // Flat at both ends: the finite difference just inside is tiny.
        assert!(fade(0.001) < 1.0e-7);
        assert!(1.0 - fade(0.999) < 1.0e-7);
    }

    #[test]
    fn perlin_is_zero_at_every_lattice_point() {
        let l = lattice();
        (-4..4).for_each(|x| {
            (-4..4).for_each(|y| {
                let v = perlin_2d(&l, DVec2::new(f64::from(x), f64::from(y))).get();
                assert!(v.abs() < 1.0e-12, "not zero at a lattice point: {v}");
            });
        });
    }

    #[test]
    fn perlin_stays_within_the_signed_unit_range_and_uses_most_of_it() {
        let l = lattice();
        let samples: Vec<f64> = (0..400)
            .map(|i| {
                let t = f64::from(i) * 0.137;
                perlin_2d(&l, DVec2::new(t, -t * 0.61)).get()
            })
            .collect();
        assert!(samples.iter().all(|v| (-1.0..=1.0).contains(v)));
        let peak = samples.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        assert!(peak > 0.5, "the range scale is not carrying the field: {peak}");
    }

    #[test]
    fn perlin_is_a_pure_function_of_lattice_and_position() {
        let l = lattice();
        let p = DVec2::new(1.7, 3.1);
        assert_eq!(perlin_2d(&l, p), perlin_2d(&l, p));
    }

    #[test]
    fn a_different_lattice_gives_a_different_field() {
        let other = lattice_seeded(99);
        let p = DVec2::new(1.7, 3.1);
        assert_ne!(perlin_2d(&lattice(), p).get(), perlin_2d(&other, p).get());
    }

    #[test]
    fn perlin_is_continuous_across_a_cell_boundary() {
        let l = lattice();
        let at = perlin_2d(&l, DVec2::new(1.0, 0.5)).get();
        let near = perlin_2d(&l, DVec2::new(1.0 - 1.0e-9, 0.5)).get();
        assert!((at - near).abs() < 1.0e-6);
    }

    #[test]
    fn the_fractal_forms_stay_in_the_unit_interval() {
        let l = lattice();
        (0..64).for_each(|i| {
            let p = DVec2::new(f64::from(i) * 0.31, f64::from(i) * -0.17);
            (1..5u32).for_each(|oct| {
                assert!((0.0..=1.0).contains(&perlin_fbm_2d(&l, p, oct, drift(), half()).get()));
                assert!((0.0..=1.0).contains(&perlin_ridged_2d(&l, p, oct, ridged_drift(), half()).get()));
                assert!(
                    (0.0..=1.0).contains(&perlin_warped_2d(&l, p, warp(0.5), oct, drift(), half()).get())
                );
            });
        });
    }

    #[test]
    fn more_octaves_change_every_fractal_form() {
        let l = lattice();
        let p = DVec2::new(0.7, -1.3);
        assert_ne!(
            perlin_fbm_2d(&l, p, 1, drift(), half()).get(),
            perlin_fbm_2d(&l, p, 4, drift(), half()).get()
        );
        assert_ne!(
            perlin_ridged_2d(&l, p, 1, ridged_drift(), half()).get(),
            perlin_ridged_2d(&l, p, 4, ridged_drift(), half()).get()
        );
        assert_ne!(
            perlin_warped_2d(&l, p, warp(0.5), 1, drift(), half()).get(),
            perlin_warped_2d(&l, p, warp(0.5), 4, drift(), half()).get()
        );
    }

    #[test]
    fn zero_octaves_reads_as_the_midpoint_rather_than_nan() {
        let l = lattice();
        let p = DVec2::new(1.0, 2.0);
        assert_eq!(perlin_fbm_2d(&l, p, 0, drift(), half()).get(), 0.5);
        assert_eq!(perlin_ridged_2d(&l, p, 0, ridged_drift(), half()).get(), 0.0);
    }

    /// Ridged turns the basis inside out: it peaks where perlin crosses zero,
    /// which is what makes it read as a vein rather than a cloud.
    #[test]
    fn ridged_peaks_where_the_gradient_basis_crosses_zero() {
        let l = lattice();
        // A lattice point is an exact zero crossing of the basis.
        let at_zero = perlin_ridged_2d(&l, DVec2::new(2.0, 3.0), 1, ridged_drift(), half()).get();
        assert!(at_zero > 0.99, "ridged did not peak at a zero crossing");
    }

    #[test]
    fn the_gain_is_observable() {
        let l = lattice();
        let p = DVec2::new(1.25, -0.75);
        assert_ne!(
            perlin_fbm_2d(&l, p, 4, drift(), half()).get(),
            perlin_fbm_2d(&l, p, 4, drift(), Ratio::new(0.25).unwrap()).get()
        );
    }

    #[test]
    fn the_lacunarity_is_observable() {
        let l = lattice();
        let p = DVec2::new(1.25, -0.75);
        assert_ne!(
            perlin_fbm_2d(&l, p, 4, drift(), half()).get(),
            perlin_fbm_2d(&l, p, 4, Lacunarity::new(3.0).unwrap(), half()).get()
        );
    }

    /// Warping must actually displace the sample: zero warp is the unwarped
    /// field, and a real warp is not.
    #[test]
    fn a_zero_warp_is_the_unwarped_field() {
        let l = lattice();
        let p = DVec2::new(0.4, 0.9);
        assert_eq!(
            perlin_warped_2d(&l, p, warp(0.0), 3, drift(), half()),
            perlin_fbm_2d(&l, p, 3, drift(), half())
        );
        assert_ne!(
            perlin_warped_2d(&l, p, warp(0.8), 3, drift(), half()),
            perlin_fbm_2d(&l, p, 3, drift(), half())
        );
    }
}
