//! `new THREE.Color(hex)` — three.js's sRGB→linear decode, in one place.
//!
//! Not a port of a Claude-of-Duty file. This module exists for the same reason
//! [`crate::jsmath`] does: a primitive the source leans on was implemented
//! twice, the two copies disagreed numerically, and one of them was wrong at
//! its own call site.
//!
//! ## Why this is not `materials::noise::ow_srgb`
//!
//! The engine has **two** sRGB decodes and they are not interchangeable:
//!
//! | | expression | used by |
//! |---|---|---|
//! | GLSL `owSRGB` ([`crate::materials::noise::ow_srgb`]) | `((c + 0.055) / 1.055)^2.4`, `c / 12.92` below the knee | the surface generators, inside shader bodies |
//! | three's `SRGBToLinear` (here) | `(c * 0.9478672986 + 0.0521327014)^2.4`, `c * 0.0773993808` below | every hex colour that reaches a uniform via `new THREE.Color(...)` |
//!
//! They are algebraically identical and numerically different: three writes the
//! transform pre-multiplied, and **float arithmetic is not associative**, so
//! **254 of the 256 byte values differ**, by up to 1.08e-11. That is four
//! orders above the `1e-12` relative tolerance the material goldens are pinned
//! at, so picking the wrong one is a test failure rather than a quiet drift —
//! which is how it was caught.
//!
//! The knee comparison is `<` here, matching three (`owSRGB`'s is `>`). The two
//! disagree only at exactly `c == 0.04045`, which no `n / 255` ever produces.
//!
//! ## The defect this module closes
//!
//! `materials::surfaces::metal::hex_to_linear_tint` documented itself as
//! "`new THREE.Color(hex)` under Three's default (enabled) `ColorManagement`"
//! and then called `ow_srgb` — the GLSL one. Its own unit test asserted that it
//! matched `ow_srgb`, so the test pinned the bug in place rather than catching
//! it. `materials::system` decodes `tintA`/`tintB` through that function, and
//! its golden (captured from the real `MaterialSystem`, which goes through
//! `new THREE.Color`) disagreed on three assertions by ~4e-11.
//!
//! It was found by a third slice: `weapons::materials`, porting a file where
//! *every* colour goes through `new THREE.Color`, transcribed three's form,
//! captured all 256 byte decodes, and reported the mismatch in a neighbour it
//! was not allowed to edit.
//!
//! Both callers now share this one definition, and the metal test asserts the
//! decode it actually performs.

/// Three's `SRGBToLinear` (`three/src/math/ColorManagement.js`), per channel.
///
/// Deliberately **not** [`crate::materials::noise::ow_srgb`] — see the module
/// doc. Transcribe the pre-multiplied constants literally; folding them back
/// into `(c + 0.055) / 1.055` re-introduces the difference this exists to
/// remove.
pub fn srgb_to_linear(c: f64) -> f64 {
    if c < 0.04045 {
        c * 0.0773993808
    } else {
        (c * 0.9478672986 + 0.0521327014).powf(2.4)
    }
}

/// `new THREE.Color(hex)` — unpack an sRGB hex triplet and decode each channel
/// through [`srgb_to_linear`].
pub fn hex_to_linear(hex: u32) -> [f64; 3] {
    [
        srgb_to_linear(f64::from((hex >> 16) & 0xff) / 255.0),
        srgb_to_linear(f64::from((hex >> 8) & 0xff) / 255.0),
        srgb_to_linear(f64::from(hex & 0xff) / 255.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::noise::ow_srgb;
    use crate::materials::noise::Vec3;

    /// The whole reason this module exists, stated as a test: the two decodes
    /// are algebraically equal and numerically different, on almost every byte.
    ///
    /// If this ever fails, the two really have converged and one of them can
    /// go — but check *why* before deleting either.
    #[test]
    fn three_and_the_glsl_decode_disagree_on_almost_every_byte_value() {
        let differing = (0u32..=255)
            .filter(|&n| {
                let c = f64::from(n) / 255.0;
                let three = srgb_to_linear(c);
                let glsl = ow_srgb(Vec3::new(c, c, c)).x;
                three.to_bits() != glsl.to_bits()
            })
            .count();
        assert_eq!(
            differing, 254,
            "three's SRGBToLinear and the GLSL owSRGB should differ on 254 of \
             256 byte values; if that changed, the material goldens need a look",
        );
    }

    /// The magnitude matters as much as the count: it has to be big enough to
    /// break a `1e-12` relative pin, or nobody would ever notice picking wrong.
    #[test]
    fn the_disagreement_is_large_enough_to_break_a_material_golden() {
        let worst = (0u32..=255)
            .map(|n| {
                let c = f64::from(n) / 255.0;
                let three = srgb_to_linear(c);
                let glsl = ow_srgb(Vec3::new(c, c, c)).x;
                (three - glsl).abs()
            })
            .fold(0.0_f64, f64::max);
        assert!(
            worst > 1e-12,
            "worst disagreement {worst:e} is within the goldens' tolerance, so \
             the distinction would no longer be observable",
        );
    }

    #[test]
    fn the_knee_is_below_every_byte_value_so_the_comparison_never_decides() {
        // three uses `<` and the GLSL uses `>`; they differ only at exactly
        // 0.04045. No `n / 255` lands there, so the branch direction is not
        // load-bearing — but say so rather than leaving it to chance.
        assert!(!(0u32..=255).any(|n| f64::from(n) / 255.0 == 0.04045));
    }
}
