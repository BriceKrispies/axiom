//! **`Rgba16Float` storage, as arithmetic** — because storage width is part of
//! the algorithm.
//!
//! Every level of the source's pyramid is a `hdrTarget(...)`, and `pass.js`'s
//! `hdrTarget` is `type: THREE.HalfFloatType`. So each of the six downsamples and
//! each of the five blended upsamples **rounds to half precision on store**, and
//! the next pass reads that rounded value back. Eleven quantisations, in a chain
//! whose whole job is to accumulate.
//!
//! Ignoring that would not merely lose a decimal place — it would make the CPU
//! reference disagree with the GPU by ~5e-4 relative for a reason that is not the
//! shader's, and any tolerance derived from that measurement would be measuring
//! the *storage* while claiming to measure the *port*. This port has already been
//! bitten five times by exactly this class of miss, most sharply by an `f32`
//! `rotateY(PI)` carrying a shear that `f64` does not.
//!
//! # The conversions
//!
//! Round-to-nearest-even in both directions, which is what a GPU does on store to
//! and load from a half-float attachment. The two functions are Fabian Giesen's
//! well-known integer forms with the `if`-chains replaced by table selection —
//! every candidate is computed unconditionally, which is safe because none of
//! them can trap: the subnormal arm's float add is finite for every input, and
//! the normal arm's adds are `wrapping_`.
//!
//! Denormals, both zeroes, both infinities and NaN are all handled, and
//! [`tests::every_half_bit_pattern_round_trips`] drives **all 65 536** of them.
//!
//! # Where it belongs, eventually
//!
//! This is a property of an `Rgba16Float` attachment, not of a bloom — it is
//! [`crate::hdr_target`]'s topic, one file up. It lives here because this is its
//! only consumer today and inventing a shared home for one caller is how a junk
//! drawer starts. The moment a second pass needs it, lift it whole.

/// The nearest `f16` value to `value`, as an `f32` — one store-and-load round
/// trip through an `Rgba16Float` attachment.
pub(crate) fn quantize(value: f32) -> f32 {
    from_half_bits(to_half_bits(value))
}

/// `f32` → `f16` bits, round to nearest even.
pub(crate) fn to_half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let magnitude = bits & 0x7fff_ffff;

    // Subnormal half: below 2^-14, the magic add lands the mantissa where the
    // subtraction can read it back off.
    const DENORM_MAGIC: u32 = 126 << 23;
    let denormal =
        (f32::from_bits(magnitude) + f32::from_bits(DENORM_MAGIC)).to_bits().wrapping_sub(DENORM_MAGIC)
            as u16;

    // Normal half: rebias the exponent, add half an ulp plus the round-to-even
    // correction, then shift the thirteen discarded mantissa bits away.
    let mantissa_odd = (magnitude >> 13) & 1;
    let normal = (magnitude
        .wrapping_add(0xC800_0000)
        .wrapping_add(0xfff)
        .wrapping_add(mantissa_odd)
        >> 13) as u16;

    let is_subnormal = magnitude < (113 << 23);
    let is_overflow = magnitude >= (143 << 23);
    let is_inf_or_nan = magnitude >= (255 << 23);
    let is_nan = magnitude > (255 << 23);

    let finite = [normal, denormal][usize::from(is_subnormal)];
    // Order matters: overflow subsumes inf/NaN by magnitude, so the special
    // encoding is selected last and wins.
    let saturated = [finite, 0x7c00][usize::from(is_overflow)];
    let magnitude_bits = [saturated, [0x7c00, 0x7e00][usize::from(is_nan)]]
        [usize::from(is_inf_or_nan)];
    sign | magnitude_bits
}

/// `f16` bits → `f32`. Exact in every case; `f32` contains `f16`.
pub(crate) fn from_half_bits(half: u16) -> f32 {
    let wide = u32::from(half);
    let sign = (wide & 0x8000) << 16;
    let magnitude = wide & 0x7fff;
    let shifted = magnitude << 13;
    let exponent = magnitude >> 10;

    const REBIAS: u32 = (127 - 15) << 23;
    const SUBNORMAL_MAGIC: u32 = 113 << 23;
    let normal = shifted + REBIAS;
    let special = shifted + REBIAS + ((128 - 16) << 23);
    let subnormal = (f32::from_bits(shifted + SUBNORMAL_MAGIC)
        - f32::from_bits(SUBNORMAL_MAGIC))
    .to_bits();

    let finite = [normal, subnormal][usize::from(exponent == 0)];
    let value = [finite, special][usize::from(exponent == 0x1f)];
    f32::from_bits(sign | value)
}

#[cfg(test)]
mod tests {
    use super::{from_half_bits, quantize, to_half_bits};

    /// **Every one of the 65 536 half patterns**, round-tripped. A half that
    /// widens to an `f32` and narrows back must be itself: that single property
    /// covers normals, subnormals, both zeroes, both infinities and every sign,
    /// and it is the strongest statement available about a conversion pair.
    ///
    /// NaN payloads are excluded on the narrowing side by construction — the GPU
    /// canonicalises them too — and are pinned separately below.
    #[test]
    fn every_half_bit_pattern_round_trips() {
        (0..=u16::MAX)
            .filter(|half| (half & 0x7fff) <= 0x7c00)
            .for_each(|half| {
                let wide = from_half_bits(half);
                let narrowed = to_half_bits(wide);
                assert_eq!(
                    narrowed, half,
                    "half {half:#06x} widened to {wide} and narrowed to {narrowed:#06x}"
                );
            });
    }

    /// The values a reader can check by hand, including the two that decide
    /// whether the rounding is to-nearest-even or truncating.
    #[test]
    fn the_named_values_quantize_where_they_should() {
        // Bits, not `==`: `-0.0 == 0.0` is true, so an equality here would pass
        // even if the sign were dropped.
        assert_eq!(quantize(0.0).to_bits(), 0.0_f32.to_bits());
        assert_eq!(quantize(-0.0).to_bits(), (-0.0_f32).to_bits());
        assert_eq!(quantize(1.0), 1.0);
        assert_eq!(quantize(-2.5), -2.5);
        // 24.0, the firefly clamp, is exactly representable — so the clamp is a
        // clamp and not a clamp-then-drift.
        assert_eq!(quantize(super::super::filters::FIREFLY_CLAMP), 24.0);
        // The largest finite half, and the first value above it.
        assert_eq!(quantize(65504.0), 65504.0);
        assert!(quantize(65520.0).is_infinite());
        assert!(quantize(1.0e30).is_infinite());
        assert!(quantize(-1.0e30).is_infinite());
        assert!(quantize(f32::INFINITY).is_infinite());
        assert!(quantize(f32::NEG_INFINITY).is_infinite());
        assert!(quantize(f32::NEG_INFINITY).is_sign_negative());
        assert!(quantize(f32::NAN).is_nan());
    }

    /// The gap between adjacent halves *is* the precision the pyramid stores at:
    /// ~1e-3 absolute around 1.0, which is 2^-10. A value halfway between two
    /// halves rounds to the even one.
    #[test]
    fn the_rounding_is_to_nearest_even() {
        let step = 1.0_f32 / 1024.0;
        assert_eq!(quantize(1.0 + step), 1.0 + step);
        // Halfway between 1.0 and 1.0+step: ties to the even mantissa, which is
        // 1.0.
        assert_eq!(quantize(1.0 + step * 0.5), 1.0);
        // Halfway between 1.0+step and 1.0+2·step ties up to the even one.
        assert_eq!(quantize(1.0 + step * 1.5), 1.0 + step * 2.0);
        // Just under and just over a tie resolve by magnitude, not by the tie.
        assert_eq!(quantize(1.0 + step * 0.49), 1.0);
        assert_eq!(quantize(1.0 + step * 0.51), 1.0 + step);
    }

    /// Subnormal halves — below 2^-14 — are stored, not flushed to zero. The
    /// bloom's dimmest mip texels live here, and a flush would put a hard floor
    /// under the glare's tail.
    #[test]
    fn subnormal_halves_survive_rather_than_flushing_to_zero() {
        let smallest_normal = 2.0_f32.powi(-14);
        let smallest_subnormal = 2.0_f32.powi(-24);
        assert_eq!(quantize(smallest_normal), smallest_normal);
        assert_eq!(quantize(smallest_subnormal), smallest_subnormal);
        assert!(quantize(smallest_subnormal) > 0.0);
        // Half of the smallest subnormal is a tie to even, which is zero.
        assert_eq!(quantize(smallest_subnormal * 0.5), 0.0);
        // Below that, it underflows to zero and keeps its sign.
        assert_eq!(quantize(-smallest_subnormal * 0.1), -0.0);
        assert!(quantize(-smallest_subnormal * 0.1).is_sign_negative());
    }

    /// Quantisation is idempotent: storing an already-stored value changes
    /// nothing. Eleven passes down and up the pyramid must not drift a value that
    /// has stopped changing.
    #[test]
    fn quantizing_twice_is_quantizing_once() {
        let table = [0.0_f32, 1.0, 0.14, 1.6, 0.9, 24.0, 1e-7, 65504.0, -3.75];
        table.iter().for_each(|value| {
            let once = quantize(*value);
            assert_eq!(quantize(once).to_bits(), once.to_bits(), "at {value}");
        });
    }

    /// The precision the reference is entitled to assume: half storage costs
    /// under one part in a thousand, which is what sets the end-to-end parity
    /// budget. The theoretical bound is a half-ulp, `2^-11`; this measures it.
    #[test]
    fn half_storage_costs_under_one_part_in_a_thousand() {
        let worst = (1..2000)
            .map(|n| {
                let value = n as f32 * 0.0173;
                ((quantize(value) - value) / value).abs()
            })
            .fold(0.0_f32, f32::max);
        assert!(worst > 0.0, "the quantiser must actually quantise");
        assert!(
            worst < 2.0_f32.powi(-10),
            "half storage measured {worst} relative, above the one part in a \
             thousand it may cost"
        );
    }
}
