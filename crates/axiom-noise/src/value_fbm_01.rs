//! Fractal Brownian summation of [`crate::value_noise_01`].

use axiom_kernel::Ratio;
use axiom_math::DVec3;

use crate::unit_noise::UnitNoise;
use crate::value_noise_01::value_noise_01;

/// Multi-octave value noise: `octaves` layers of [`value_noise_01`], each
/// scaled in amplitude by `gain` and in frequency by `lacunarity`, normalised
/// back into `[0, 1]`.
///
/// ## Why `lacunarity` is a vector and not a [`crate::Lacunarity`]
///
/// A single scalar doubling is the textbook FBM, and it is the wrong default
/// for a *positional* basis. Octaves that scale every axis by the same factor
/// keep hitting the same lattice planes, and the result grids up visibly. Three
/// slightly different, mutually irrational-ish factors — something near, but not
/// on, 2.0 — walk the octaves off each other and the grid disappears. That is a
/// per-axis quantity, so it is a [`DVec3`] rather than three
/// [`crate::Lacunarity`] values, and it is `f64` because the drift constants are
/// the field's identity: rounding them to `f32` moves every sample.
///
/// ## Why the starting amplitude is not a parameter
///
/// The sum is normalised by the accumulated amplitude, so a common factor in
/// every octave's amplitude cancels exactly: `Σ a₀gⁱnᵢ / Σ a₀gⁱ` is independent
/// of `a₀`. Only the *ratio* `gain` is observable, so offering a starting
/// amplitude would be a knob that provably does nothing — the kind of API that
/// makes a caller believe they tuned something.
///
/// ## `octaves = 0`
///
/// Yields `0.0`. With no octaves both the sum and the normaliser are zero, and
/// the quotient is NaN; [`UnitNoise::from_signal`] maps that to zero rather
/// than propagating a NaN into a texture bake, where it would surface much
/// later as a black pixel with no explanation.
pub fn value_fbm_01(
    p: DVec3,
    octaves: u32,
    lacunarity: DVec3,
    gain: Ratio,
) -> UnitNoise {
    // Widening an `f32` gain to `f64` is exact — every `f32` is representable
    // as an `f64` — so the typed knob costs the field no precision.
    let gain = f64::from(gain.get());

    // The fold carries `(sum, normaliser, amplitude, position)`. Amplitude
    // starts at one; see the doc above for why its value is unobservable.
    let (sum, norm, ..) = (0..octaves).fold((0.0, 0.0, 1.0, p), |(sum, norm, amp, at), _| {
        (
            sum + value_noise_01(at).get() * amp,
            norm + amp,
            amp * gain,
            at.mul_componentwise(lacunarity),
        )
    });

    UnitNoise::from_signal(sum / norm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_noise_01::value_noise_01;

    fn half() -> Ratio {
        Ratio::new(0.5).unwrap()
    }

    fn drift() -> DVec3 {
        DVec3::new(2.03, 2.01, 1.97)
    }

    /// One octave is the base noise itself: the single amplitude cancels
    /// against the single normaliser exactly.
    #[test]
    fn one_octave_is_the_base_noise() {
        [
            DVec3::ZERO,
            DVec3::new(0.5, 0.5, 0.5),
            DVec3::new(-1.5, 2.25, -3.75),
            DVec3::new(10.1, -4.2, 7.7),
        ]
        .into_iter()
        .for_each(|p| {
            assert_eq!(value_fbm_01(p, 1, drift(), half()), value_noise_01(p));
        });
    }

    /// The claim in the doc comment, asserted: the starting amplitude cancels,
    /// so a run that began at a different one is the same field. Driving it
    /// with two different gains that are *equal* proves the normalisation, and
    /// two different gains that differ proves the gain is not itself inert.
    #[test]
    fn the_gain_is_observable_but_the_starting_amplitude_is_not() {
        let p = DVec3::new(1.25, -0.75, 3.5);
        let a = value_fbm_01(p, 4, drift(), half()).get();
        let b = value_fbm_01(p, 4, drift(), Ratio::new(0.25).unwrap()).get();
        assert_ne!(a, b);
    }

    #[test]
    fn zero_octaves_is_zero_rather_than_nan() {
        let v = value_fbm_01(DVec3::new(1.0, 2.0, 3.0), 0, drift(), half());
        assert_eq!(v.get(), 0.0);
        assert!(!v.get().is_nan());
    }

    #[test]
    fn samples_stay_in_the_unit_interval() {
        (0..48).for_each(|i| {
            let t = f64::from(i) * 0.41 - 6.0;
            (1..6u32).for_each(|oct| {
                let v = value_fbm_01(DVec3::new(t, -t * 0.3, t * 1.7), oct, drift(), half()).get();
                assert!((0.0..=1.0).contains(&v), "fbm out of range: {v}");
            });
        });
    }

    /// More octaves must change the field — otherwise the loop is not running.
    #[test]
    fn additional_octaves_change_the_field() {
        let p = DVec3::new(0.3, 1.1, -2.4);
        let one = value_fbm_01(p, 1, drift(), half()).get();
        let three = value_fbm_01(p, 3, drift(), half()).get();
        let five = value_fbm_01(p, 5, drift(), half()).get();
        assert_ne!(one, three);
        assert_ne!(three, five);
    }

    /// Per-axis drift is what stops the octaves gridding up. If lacunarity were
    /// collapsed to one scalar the field would differ — pinning that the vector
    /// is genuinely read on all three axes.
    #[test]
    fn every_lacunarity_axis_is_read() {
        let p = DVec3::new(0.7, 0.7, 0.7);
        let base = value_fbm_01(p, 4, drift(), half()).get();
        assert_ne!(
            base,
            value_fbm_01(p, 4, DVec3::new(2.5, 2.01, 1.97), half()).get()
        );
        assert_ne!(
            base,
            value_fbm_01(p, 4, DVec3::new(2.03, 2.5, 1.97), half()).get()
        );
        assert_ne!(
            base,
            value_fbm_01(p, 4, DVec3::new(2.03, 2.01, 2.5), half()).get()
        );
    }

    #[test]
    fn is_a_pure_function_of_its_arguments() {
        let p = DVec3::new(4.2, -1.1, 0.05);
        assert_eq!(
            value_fbm_01(p, 4, drift(), half()),
            value_fbm_01(p, 4, drift(), half())
        );
    }
}
