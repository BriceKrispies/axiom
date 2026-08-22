//! **The fallback image-based environment** — the analytic sky the reference
//! synthesises when the sky subsystem has not yet handed the renderer one.
//!
//! Ported from Claude-of-Duty `src/render/env.js` (106 lines), as called by
//! `src/render/index.js:299`
//! (`buildFallbackEnvironment(renderer, this._dirFromLight(this.sun, this.sunDir))`).
//!
//! # What this actually is, and what it is not
//!
//! `env.js` builds **one 512x256 RGBA half-float equirectangular radiance map**
//! from a closed-form sky model — Rayleigh gradient, a two-lobe Mie forward
//! scatter around the sun, a low-sun warm/cool split, and a ground-bounce arm
//! below the horizon — and then runs it through three's `PMREMGenerator` to get
//! a prefiltered environment. The equirect is kept alive afterwards so it can
//! also serve as the scene background.
//!
//! Everything above "and then runs it through PMREM" is here, exactly. The PMREM
//! prefilter is **not** — see [`the deferral`](#the-pmrem-prefilter-is-deferred)
//! below, which names the expiry condition and the file that has to change.
//!
//! # `1.0` here is not `1.0` on the shmup's sky
//!
//! The file's own header calls its values "radiometric-ish (a clear sky zenith
//! around 8 cd/m^2 relative to a sun of ~30)". That is a **different scale** from
//! the one `apps/shmup/src/sky/atmosphere.rs` documents, where
//! `1 unit = SCENE_LUX = 25000 lux` and a clear zenith is ~`0.06` radiance units.
//! The two differ by roughly two orders of magnitude *and* in what the number
//! means. Nothing here converts between them: this is a *fallback* the real sky
//! displaces, and the reference wires it that way too (`index.js` overwrites
//! `scene.environment` the moment the sky subsystem publishes one). A caller that
//! mixes this into a frame already carrying `sky/atmosphere`'s output has made an
//! error the types cannot catch, so it is written down here.
//!
//! # Storage width is part of the algorithm
//!
//! `env.js` writes into a `Uint16Array` through `THREE.DataUtils.toHalfFloat`
//! and declares the texture `THREE.HalfFloatType`. So the environment the
//! renderer samples is **f16**, and the quantisation is part of what it looks
//! like. This module therefore produces `u16` half-float bits, not `f32`.
//!
//! And it must produce them through **three's** conversion, which is not the one
//! this crate already has. [`crate::bloom_pyramid::half_storage::to_half_bits`]
//! rounds to nearest even, because that is what an `Rgba16Float` *attachment*
//! does. Three's `DataUtils.toHalfFloat` is the fox-toolkit table method
//! (`three/src/extras/DataUtils.js`) and **truncates** the mantissa:
//! `baseTable[e] + ((f & 0x007fffff) >> shiftTable[e])` has no rounding term.
//! Reusing the bloom helper here would be wrong on roughly half of all inputs —
//! exactly the "a wrong implementation propagated by citation" defect this port
//! has already recorded once. [`three_to_half_float`] is the source's function.
//!
//! # The PMREM prefilter is deferred
//!
//! `PMREMGenerator.fromEquirectangular` is a multi-pass GPU prefilter (equirect
//! to cubemap, then a chain of roughness-blurred mips in a packed layout) living
//! inside three. Porting it is a separate slice, and **there is nothing in Axiom
//! for it to feed**: the frame contract has no environment lane at all. The
//! engine's whole indirect vocabulary today is [`axiom_host::FrameAmbient`], a
//! two-band hemisphere, which is the *fill* term the reference carries
//! **alongside** its PMREM, not the PMREM (see [`crate::indirect_lighting::volumes`]).
//!
//! **Expiry check.** This deferral becomes a defect the moment either of these
//! lands, and whoever lands them owns re-opening it:
//!
//! - a prefiltered-environment lane on the frame — `crates/axiom-host/src/`
//!   (a `FrameEnvironment` peer of `frame_ambient.rs`), plus the sampling in
//!   `modules/axiom-gpu-backend/src/scene_wgsl.rs`'s lighting suffix; or
//! - a specular-IBL term in the lighting model, which is the same two files.
//!
//! Until then this module produces the equirect and stops, which is the honest
//! amount of it that has a consumer.

/// The equirect's width in texels (`env.js:69`, `const W = 512`).
pub(crate) const WIDTH: usize = 512;

/// The equirect's height in texels (`env.js:70`, `const H = 256`).
pub(crate) const HEIGHT: usize = 256;

/// `toHalf(1)` — the alpha every texel carries (`env.js:88`). `1.0f32` is
/// exponent index 127, so the normal arm gives `(0 + 15) << 10` with a zero
/// mantissa.
pub(crate) const HALF_ONE: u16 = 0x3c00;

/// The clamp `THREE.DataUtils.toHalfFloat` applies before converting
/// (`DataUtils.js`: `clamp( val, -65504, 65504 )`). 65504 is the largest finite
/// f16.
const HALF_MAX: f64 = 65504.0;

/// `baseTable[index]` from three's `_generateTables` (`DataUtils.js`), computed
/// rather than materialised — the arithmetic is the loop body, arm for arm.
///
/// `index` is the 9-bit `(f >> 23) & 0x1ff`: the low 8 bits are the f32 exponent
/// field and bit 8 is the sign. The table's five arms, keyed on `e = i - 127`:
///
/// | `e` | base (positive) | shift |
/// |---|---|---|
/// | `< -27` (zero / underflow) | `0x0000` | 24 |
/// | `-27 ..= -15` (f16 denormal) | `0x0400 >> (-e - 14)` | `-e - 1` |
/// | `-14 ..= 15` (f16 normal) | `(e + 15) << 10` | 13 |
/// | `16 ..= 127` (overflow to Inf) | `0x7c00` | 24 |
/// | `>= 128` (Inf / NaN) | `0x7c00` | 13 |
///
/// and the sign bit ORs `0x8000` into every one of them, uniformly — which is
/// why the source's `baseTable[ i | 0x100 ] = … | 0x8000` collapses to one term
/// here rather than five.
fn half_base(index: usize) -> u32 {
    let e = (index & 0xff) as i32 - 127;
    // Both shift-derived arms are evaluated on every call, so both shift amounts
    // are clamped into range: outside its own arm each is meaningless, and an
    // out-of-range `<<`/`>>` panics in a debug build rather than producing the
    // ignored value the branchless select is about to discard.
    let denormal = 0x0400_u32 >> (-e - 14).clamp(0, 31);
    let normal = ((e + 15).clamp(0, 30) as u32) << 10;
    let arm = usize::from(e >= -27) + usize::from(e >= -14) + usize::from(e > 15) + usize::from(e >= 128);
    let positive = [0x0000_u32, denormal, normal, 0x7c00, 0x7c00][arm];
    positive | (((index >> 8) & 1) as u32) * 0x8000
}

/// `shiftTable[index]` from three's `_generateTables`. See [`half_base`] for the
/// arm table; the shift does not depend on the sign bit.
fn half_shift(index: usize) -> u32 {
    let e = (index & 0xff) as i32 - 127;
    let denormal = (-e - 1).clamp(0, 31) as u32;
    let arm = usize::from(e >= -27) + usize::from(e >= -14) + usize::from(e > 15) + usize::from(e >= 128);
    [24, denormal, 13, 24, 13][arm]
}

/// `THREE.DataUtils.toHalfFloat` (`three/src/extras/DataUtils.js`).
///
/// Three narrowings happen here and all three are the source's:
///
/// 1. the JS number arrives as **f64** and is clamped in f64
///    (`clamp(val, -65504, 65504)`);
/// 2. `_tables.floatView[0] = val` narrows it to **f32**, round-to-nearest-even;
/// 3. the table converts f32 to f16 by **truncating** the mantissa.
///
/// Step 3 is the one that differs from every rounding half-float converter,
/// including this crate's own. It is not an approximation of round-to-nearest —
/// it is a different function, biased toward zero by up to one f16 ULP on every
/// value whose f32 mantissa has any bit below the retained ten.
///
/// `Math.min`/`Math.max` propagate NaN and Rust's `f64::min`/`f64::max` discard
/// it, so the NaN case is selected explicitly rather than left to the difference.
pub(crate) fn three_to_half_float(val: f64) -> u16 {
    // `THREE.MathUtils.clamp( value, min, max )` is `Math.max( min, Math.min( max,
    // value ) )` — written out in that order because the two orders differ on NaN
    // and this is the order the source uses.
    let finite = f64::max(-HALF_MAX, f64::min(HALF_MAX, val));
    let clamped = [finite, f64::NAN][usize::from(val.is_nan())];
    let bits = (clamped as f32).to_bits();
    let index = ((bits >> 23) & 0x1ff) as usize;
    // `+`, not `|`: the source adds, and on the NaN arm (base `0x7c00`, shift 13)
    // the addend can carry into the exponent field, which is how a signalling NaN
    // becomes `0x7e00`.
    (half_base(index) + ((bits & 0x007f_ffff) >> half_shift(index))) as u16
}

/// `skyRadiance( dir, sunDir, out )` (`env.js:14-66`).
///
/// Evaluated wholly in **f64** because JavaScript numbers are f64 and nothing in
/// `env.js` narrows before [`three_to_half_float`] does. `dir` is a unit vector,
/// `sun_dir` the direction **toward** the sun (`index.js:299` passes
/// `_dirFromLight(this.sun, this.sunDir)`).
///
/// # Two source properties worth naming
///
/// **`cosTheta`'s `-0.2` floor is dead.** `Math.max(-0.2, dir.y)` is read exactly
/// once, by `Math.pow(1.0 - Math.max(0, cosTheta), 5.0)`, which re-clamps at 0 —
/// so the -0.2 can never influence a result. It is ported as written, with this
/// note, because dead computation in the source is still part of the source.
///
/// **The ground arm is selected by `dir.y < 0.0`, not by `cosTheta`.** At
/// `dir.y == -0.0` the comparison is false and the sky arm runs, which is the
/// behaviour reproduced here.
pub(crate) fn sky_radiance(dir: [f64; 3], sun_dir: [f64; 3]) -> [f64; 3] {
    let cos_theta = f64::max(-0.2, dir[1]);
    let cos_gamma = dir[0] * sun_dir[0] + dir[1] * sun_dir[1] + dir[2] * sun_dir[2];
    let gamma = f64::min(1.0, f64::max(-1.0, cos_gamma)).acos();

    let sun_up = f64::max(0.0, sun_dir[1]);
    let day_factor = f64::max(0.02, sun_up).powf(0.45);

    // Rayleigh: bright near the horizon, deep blue at zenith.
    let horizon = (1.0 - f64::max(0.0, cos_theta)).powf(5.0);
    let zenith = [0.14, 0.28, 0.62];
    let horizon_col = [0.72, 0.76, 0.82];
    let r0 = zenith[0] + (horizon_col[0] - zenith[0]) * horizon;
    let g0 = zenith[1] + (horizon_col[1] - zenith[1]) * horizon;
    let b0 = zenith[2] + (horizon_col[2] - zenith[2]) * horizon;

    // Mie forward scattering around the sun. Two lobes, summed — not a single
    // re-associated exponential.
    let mie = 1.5 * (-gamma * 3.2).exp() + 0.35 * (-gamma * 0.6).exp();
    let warm = [1.0, 0.72, 0.42];
    let r1 = r0 + mie * warm[0] * 0.55;
    let g1 = g0 + mie * warm[1] * 0.55;
    let b1 = b0 + mie * warm[2] * 0.55;

    // Warm the low sun.
    let low_sun = (1.0 - f64::min(1.0, sun_up * 2.4)).powf(2.0);
    let r2 = r1 * (1.0 + low_sun * 0.5);
    let g2 = g1 * (1.0 - low_sun * 0.12);
    let b2 = b1 * (1.0 - low_sun * 0.35);

    // Scaled so sky irradiance is ~20% of a 4.0-intensity sun.
    let scale = 0.34 * day_factor;

    // ground bounce: dry concrete/dirt albedo, darker and warmer. `gr * 1.05 * t`
    // is `(0.62 * 1.05) * t` — left-associated, as JavaScript evaluates it.
    let t = f64::min(1.0, -dir[1] * 3.0);
    let gr = 0.62;
    let below = [
        (r2 * (1.0 - t) + gr * 1.05 * t) * scale,
        (g2 * (1.0 - t) + gr * 0.95 * t) * scale,
        (b2 * (1.0 - t) + gr * 0.78 * t) * scale,
    ];
    let above = [r2 * scale, g2 * scale, b2 * scale];
    [above, below][usize::from(dir[1] < 0.0)]
}

/// The unit direction texel `(x, y)` of the equirect samples (`env.js:76-84`).
///
/// Row 0 is the **nadir**: "DataTexture rows start at the bottom of the image in
/// GL, so row 0 must be the nadir for an equirectangular map to come out the
/// right way up." That inversion is the `1 -` in `theta`.
///
/// The grouping of `phi` is the source's: `((x + 0.5) / W) * Math.PI * 2 -
/// Math.PI` is `((((x + 0.5) / W) * PI) * 2) - PI`, not `((x + 0.5) / W) * TAU`.
pub(crate) fn equirect_direction(x: usize, y: usize) -> [f64; 3] {
    let theta = (1.0 - (y as f64 + 0.5) / HEIGHT as f64) * std::f64::consts::PI;
    let sin_t = theta.sin();
    let cos_t = theta.cos();
    let phi = ((x as f64 + 0.5) / WIDTH as f64) * std::f64::consts::PI * 2.0 - std::f64::consts::PI;
    [sin_t * phi.sin(), cos_t, sin_t * phi.cos()]
}

/// `buildFallbackEnvironment` (`env.js:68-105`), minus the PMREM prefilter and
/// the three texture object.
///
/// Returns the equirect's texel data as `WIDTH * HEIGHT * 4` **half-float bits**
/// in RGBA order, row 0 first — the exact contents of `env.js`'s `Uint16Array`.
/// Upload as `Rgba16Float`, `EquirectangularReflectionMapping`, linear min/mag,
/// no colour-space conversion (`env.js:92-97`: `THREE.NoColorSpace`).
pub(crate) fn build_fallback_environment(sun_dir: [f64; 3]) -> Vec<u16> {
    (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let rgb = sky_radiance(equirect_direction(x, y), sun_dir);
            [
                three_to_half_float(rgb[0]),
                three_to_half_float(rgb[1]),
                three_to_half_float(rgb[2]),
                three_to_half_float(1.0),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode f16 bits to `f32`, for reading assertions back in the units the
    /// source thinks in. Exact in every case — `f32` contains `f16` — and
    /// deliberately a *test* helper: nothing in the port needs it, and the crate
    /// already has `bloom_pyramid::half_storage::from_half_bits` for the one
    /// place that does.
    fn from_half(bits: u16) -> f32 {
        let sign = u32::from(bits & 0x8000) << 16;
        let exp = u32::from((bits >> 10) & 0x1f);
        let mantissa = u32::from(bits & 0x3ff);
        // Normal: rebias 15 -> 127. Denormal/zero: scale the mantissa by 2^-24.
        let normal = f32::from_bits(sign | ((exp + 112) << 23) | (mantissa << 13));
        let denormal = f32::from_bits(sign) + (mantissa as f32) * 2.0_f32.powi(-24) * [1.0, -1.0][usize::from(bits >> 15)];
        [normal, denormal][usize::from(exp == 0)]
    }

    #[test]
    fn the_map_is_the_sources_512_by_256_rgba() {
        assert_eq!(WIDTH, 512, "env.js:69 declares W = 512");
        assert_eq!(HEIGHT, 256, "env.js:70 declares H = 256");
        let data = build_fallback_environment([0.3, 0.7, 0.6]);
        assert_eq!(
            data.len(),
            WIDTH * HEIGHT * 4,
            "env.js allocates Uint16Array(W * H * 4) and fills every lane"
        );
    }

    #[test]
    fn every_texel_carries_half_one_in_alpha() {
        assert_eq!(
            three_to_half_float(1.0),
            HALF_ONE,
            "toHalf(1) must be the f16 encoding of one"
        );
        let data = build_fallback_environment([0.0, 1.0, 0.0]);
        let wrong = data
            .iter()
            .skip(3)
            .step_by(4)
            .filter(|a| **a != HALF_ONE)
            .count();
        assert_eq!(wrong, 0, "{wrong} alpha lanes are not toHalf(1)");
    }

    #[test]
    fn three_truncates_the_mantissa_where_this_crates_attachment_helper_rounds() {
        // 1 + 2^-11 is exactly halfway between two f16 values (1.0 and 1 + 2^-10).
        // Round-to-nearest-even gives 1.0 here, and truncation also gives 1.0 —
        // so the halfway case does NOT separate them. What separates them is any
        // value strictly above the midpoint: 1 + 3*2^-12 rounds UP to 1 + 2^-10
        // (0x3c01) and truncates DOWN to 1.0 (0x3c00).
        let above_midpoint = 1.0 + 3.0 * 2.0_f64.powi(-12);
        assert_eq!(
            three_to_half_float(above_midpoint),
            0x3c00,
            "three's table truncates: {above_midpoint} must land on 1.0, not on the nearer 1+2^-10"
        );
        assert_eq!(
            crate::bloom_pyramid::half_storage::to_half_bits(above_midpoint as f32),
            0x3c01,
            "the crate's attachment helper rounds to nearest, which is a DIFFERENT function"
        );
    }

    #[test]
    fn truncation_biases_a_representative_sweep_toward_zero() {
        // Not a single case: over a sweep of the radiance range this map actually
        // covers, count how often the two converters disagree. If they agreed
        // often enough for the choice not to matter, this port could reuse the
        // existing helper — the number is what says it cannot.
        let (differ, never_further_from_zero) = (0..4096)
            .map(|i| 0.001 * f64::from(i) + 0.000_137)
            .map(|v| {
                let truncated = from_half(three_to_half_float(v));
                let rounded = from_half(crate::bloom_pyramid::half_storage::to_half_bits(v as f32));
                (
                    usize::from(truncated != rounded),
                    usize::from(truncated.abs() <= rounded.abs() || truncated == rounded),
                )
            })
            .fold((0, 0), |(d, b), (dd, bb)| (d + dd, b + bb));
        assert!(
            differ > 1500,
            "only {differ}/4096 sampled radiances differ between truncation and \
             round-to-nearest; if that were small the converter choice would be cosmetic"
        );
        assert_eq!(
            never_further_from_zero, 4096,
            "truncation must never land further from zero than rounding does"
        );
    }

    #[test]
    fn the_half_tables_reproduce_every_arm_of_three_generate_tables() {
        // The five arms, keyed by the f32 exponent field `i` (so `e = i - 127`),
        // with the base/shift the JS loop writes. Positive sign first, then the
        // `| 0x100` mirror.
        let cases: [(usize, u32, u32); 6] = [
            // e = -100  -> "very small number"
            (27, 0x0000, 24),
            // e = -20   -> denormal: base 0x0400 >> 6 = 0x0010, shift 19
            (107, 0x0010, 19),
            // e = 0     -> normal: base (0 + 15) << 10 = 0x3c00, shift 13
            (127, 0x3c00, 13),
            // e = 15    -> normal, top of the arm: (15 + 15) << 10 = 0x7800
            (142, 0x7800, 13),
            // e = 16    -> "large number": Inf, shift 24 so the mantissa vanishes
            (143, 0x7c00, 24),
            // e = 128   -> "stay": Inf/NaN, shift 13 so a NaN payload survives
            (255, 0x7c00, 13),
        ];
        cases.iter().for_each(|(index, base, shift)| {
            assert_eq!(half_base(*index), *base, "baseTable[{index}]");
            assert_eq!(half_shift(*index), *shift, "shiftTable[{index}]");
            assert_eq!(
                half_base(index | 0x100),
                base | 0x8000,
                "baseTable[{index} | 0x100] is the same base with the sign bit"
            );
            assert_eq!(
                half_shift(index | 0x100),
                *shift,
                "shiftTable does not depend on the sign bit"
            );
        });
        // The two boundaries the arm select has to place exactly.
        assert_eq!(half_shift(99), 24, "e = -28 is still the 'very small' arm");
        assert_eq!(half_shift(100), 26, "e = -27 is the first denormal arm entry (-e - 1)");
        assert_eq!(half_base(100), 0x0000, "and its base underflows to zero (0x400 >> 13)");
        assert_eq!(half_shift(112), 14, "e = -15 is the last denormal arm entry");
        assert_eq!(half_base(112), 0x0200, "0x400 >> 1 at the top of the denormal arm");
        assert_eq!(half_base(113), 0x0400, "e = -14 is the first normal arm entry");
        assert_eq!(half_shift(113), 13, "and it takes the normal arm's shift");
    }

    #[test]
    fn the_conversion_clamps_saturates_and_signs_the_way_three_does() {
        assert_eq!(three_to_half_float(0.0), 0x0000, "+0 is +0");
        assert_eq!(three_to_half_float(-0.0), 0x8000, "-0 keeps its sign bit");
        assert_eq!(three_to_half_float(-1.0), 0xbc00, "-1 is 1 with the sign bit");
        assert_eq!(
            three_to_half_float(1.0e30),
            0x7bff,
            "the clamp to 65504 pins an overflow at the largest finite f16, NOT at Inf"
        );
        assert_eq!(
            three_to_half_float(-1.0e30),
            0xfbff,
            "and the same on the negative side"
        );
        assert_eq!(
            three_to_half_float(1.0e-12),
            0x0000,
            "far under the denormal floor flushes to zero"
        );
        assert_eq!(
            three_to_half_float(f64::NAN) & 0x7c00,
            0x7c00,
            "Math.min/Math.max propagate NaN, so a NaN must reach the table and stay non-finite"
        );
        assert_ne!(
            three_to_half_float(f64::NAN) & 0x03ff,
            0,
            "and it must stay a NaN rather than collapsing to Inf"
        );
        assert_eq!(
            three_to_half_float(f64::INFINITY),
            0x7bff,
            "Infinity is clamped like any other overflow, because the clamp runs FIRST"
        );
    }

    #[test]
    fn the_equirect_grid_puts_the_nadir_on_row_zero_and_wraps_in_phi() {
        let bottom = equirect_direction(0, 0);
        let top = equirect_direction(0, HEIGHT - 1);
        assert!(
            bottom[1] < -0.99,
            "row 0 must be the nadir (env.js:72-74); got y = {}",
            bottom[1]
        );
        assert!(
            top[1] > 0.99,
            "the last row must be the zenith; got y = {}",
            top[1]
        );
        // Unit length by construction, everywhere.
        let worst = (0..HEIGHT)
            .step_by(17)
            .flat_map(|y| (0..WIDTH).step_by(31).map(move |x| (x, y)))
            .map(|(x, y)| {
                let d = equirect_direction(x, y);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2] - 1.0).abs()
            })
            .fold(0.0_f64, f64::max);
        assert!(
            worst < 1.0e-15,
            "the equirect directions must be unit; worst |len^2 - 1| = {worst:e}"
        );
        // phi runs -PI at the left texel centre to just under +PI at the right,
        // so the seam is at -Z, and x = W/2 looks along +Z.
        let left = equirect_direction(0, HEIGHT / 2);
        let middle = equirect_direction(WIDTH / 2, HEIGHT / 2);
        assert!(
            left[2] < -0.999,
            "the left column is the -Z seam; got z = {}",
            left[2]
        );
        assert!(
            middle[2] > 0.999,
            "the centre column looks along +Z; got z = {}",
            middle[2]
        );
    }

    #[test]
    fn the_dead_minus_zero_point_two_floor_cannot_change_a_result() {
        // `Math.max(-0.2, dir.y)` is re-clamped at 0 by the only expression that
        // reads it, so replacing the floor with anything <= 0 is invisible. Proved
        // by evaluating the horizon term both ways across the whole band the floor
        // could possibly bite in.
        let worst = (0..401)
            .map(|i| -0.2 - f64::from(i) * 0.002)
            .map(|y| {
                let floored = (1.0 - f64::max(0.0, f64::max(-0.2, y))).powf(5.0);
                let unfloored = (1.0 - f64::max(0.0, y)).powf(5.0);
                (floored - unfloored).abs()
            })
            .fold(0.0_f64, f64::max);
        assert_eq!(
            worst, 0.0,
            "the -0.2 floor is dead; if this ever fails the source's shape changed"
        );
    }

    #[test]
    fn the_sky_is_blue_overhead_and_pale_at_the_horizon() {
        // The sun is deliberately NOT overhead. With `sun = [0, 1, 0]` the zenith
        // view looks straight down the Mie forward-scatter lobe — `gamma = 0`, so
        // `mie` is its maximum 1.85 — and the warm lobe swamps the Rayleigh blue.
        // That is the shader being right, not wrong: point a camera at the sun and
        // you get the sun. Sampling the zenith says something about *sky* colour
        // only when the sun is somewhere else.
        //
        // 53 degrees up, in the x-y plane: high enough that `lowSun` is exactly
        // zero (it vanishes above ~24.6 degrees), so this measures the Rayleigh
        // gradient alone.
        let sun = [0.6, 0.8, 0.0];
        let zenith = sky_radiance([0.0, 1.0, 0.0], sun);
        let horizon = sky_radiance([1.0, 0.0, 0.0], sun);
        assert!(
            zenith[2] > zenith[1] && zenith[1] > zenith[0],
            "the zenith must be blue-dominant: {zenith:?}"
        );
        // At the horizon the Rayleigh gradient reaches `horizonCol`, which is very
        // nearly neutral; the sun is overhead so the Mie lobe is far away.
        let spread = horizon[2] - horizon[0];
        assert!(
            spread.abs() < 0.05,
            "the horizon band is near-neutral; B-R = {spread}"
        );
        assert!(
            horizon[0] > zenith[0],
            "the horizon is brighter in red than the zenith: {horizon:?} vs {zenith:?}"
        );
    }

    #[test]
    fn the_mie_lobe_peaks_exactly_at_the_sun() {
        let sun = [0.0, 0.5, 0.866_025_403_784_438_6];
        let at_sun = sky_radiance(sun, sun);
        let away = sky_radiance([0.0, 0.5, -0.866_025_403_784_438_6], sun);
        // gamma = 0 gives 1.5 + 0.35 = 1.85 of Mie, times 0.55 of `warm`.
        assert!(
            at_sun[0] > away[0] * 3.0,
            "the forward lobe must dominate: at the sun {at_sun:?}, opposite {away:?}"
        );
        // Two lobes, so the falloff is not a single exponential: at 90 degrees the
        // wide lobe (exp(-1.5708 * 0.6)) still contributes visibly.
        let side = sky_radiance([1.0, 0.0, 0.0], sun);
        assert!(
            side[0] > away[0],
            "the wide 0.6-rate lobe must still be above the anti-solar value: \
             side {side:?}, opposite {away:?}"
        );
    }

    #[test]
    fn the_ground_arm_engages_strictly_below_the_horizon() {
        let sun = [0.0, 0.8, 0.6];
        // -0.0 is NOT < 0.0, so it takes the sky arm — same as JavaScript.
        let minus_zero = sky_radiance([1.0, -0.0, 0.0], sun);
        let plus_zero = sky_radiance([1.0, 0.0, 0.0], sun);
        assert_eq!(
            minus_zero, plus_zero,
            "dir.y = -0.0 must take the sky arm, because -0.0 < 0.0 is false"
        );
        // The bounce reaches full strength at y = -1/3 and stays there.
        let third = sky_radiance([0.0, -1.0 / 3.0, 0.0], sun);
        let nadir = sky_radiance([0.0, -1.0, 0.0], sun);
        assert!(
            (third[0] - nadir[0]).abs() < 1.0e-12,
            "t saturates at min(1, -y*3), so -1/3 and -1 must agree: {third:?} vs {nadir:?}"
        );
        assert!(
            nadir[0] > nadir[1] && nadir[1] > nadir[2],
            "the ground bounce is warm (1.05 / 0.95 / 0.78 of a 0.62 albedo): {nadir:?}"
        );
    }

    #[test]
    fn a_low_sun_warms_and_a_dead_sun_floors_the_day_factor() {
        // **The two suns are chosen so that only `lowSun` differs between them.**
        //
        // Both lie in the y-z plane, and the view is `+x`, so `cosGamma` is 0 for
        // both: identical `gamma`, identical Mie lobe. The view direction is the
        // same, so the Rayleigh `horizon` term is identical too. `dayFactor` does
        // differ, but it is `scale`, which multiplies all three channels equally
        // and therefore cancels in an R/B ratio.
        //
        // Comparing the *zenith* under a *zenith* sun — which this test used to
        // do — is not a control at all: it points the view down the Mie lobe and
        // reads a warm sky that has nothing to do with `lowSun`.
        let across = [1.0, 0.0, 0.0];
        // 30 degrees up: `lowSun = (1 - min(1, 0.5*2.4))^2 = 0`.
        let high = sky_radiance(across, [0.0, 0.5, 0.866_025_403_784_438_6]);
        // ~2.9 degrees up: `lowSun = (1 - 0.12)^2 = 0.7744`.
        let low = sky_radiance(across, [0.0, 0.05, 0.998_749_217_771_909_2]);
        // lowSun = (1 - min(1, sunUp*2.4))^2, so it is zero for any sun above
        // ~24.6 degrees and rises quadratically below it.
        let high_ratio = high[0] / high[2];
        let low_ratio = low[0] / low[2];
        assert!(
            low_ratio > high_ratio,
            "a low sun must push red against blue: R/B {low_ratio} (low) vs {high_ratio} (high)"
        );
        // dayFactor floors at 0.02^0.45, so a sun below the horizon does not
        // produce a black environment.
        //
        // Kept on `across` for the same reason as above: both suns have `x = 0`,
        // so viewing along `+x` gives them the same `cosGamma` of 0 and the same
        // Mie lobe. Comparing them down the zenith would put one at `gamma = pi`
        // and the other at `pi/2` and measure the lobe, not the floor.
        let night = sky_radiance(across, [0.0, -1.0, 0.0]);
        let floored = sky_radiance(across, [0.0, 0.0, 1.0]);
        assert_eq!(
            night[2], floored[2],
            "sunUp clamps at 0, so any sun at or below the horizon shares one dayFactor"
        );
        assert!(
            night[2] > 0.0,
            "the 0.02 floor keeps the night environment non-black: {night:?}"
        );
    }

    #[test]
    fn the_gamma_clamp_absorbs_a_non_unit_direction() {
        // `Math.acos(Math.min(1, Math.max(-1, cosGamma)))` is the only guard, and
        // it has to hold for a dot product that rounds just past +-1.
        let over = sky_radiance([0.0, 1.0, 0.0], [0.0, 1.000_000_1, 0.0]);
        let under = sky_radiance([0.0, -1.0, 0.0], [0.0, 1.000_000_1, 0.0]);
        assert!(
            over.iter().all(|c| c.is_finite()),
            "cosGamma > 1 must clamp rather than produce NaN: {over:?}"
        );
        assert!(
            under.iter().all(|c| c.is_finite()),
            "cosGamma < -1 must clamp rather than produce NaN: {under:?}"
        );
    }

    #[test]
    fn the_whole_map_is_finite_and_quantised_into_f16() {
        let data = build_fallback_environment([0.2, 0.3, 0.932_737_905_308_881_5]);
        let bad = data.iter().filter(|h| (**h & 0x7c00) == 0x7c00).count();
        assert_eq!(bad, 0, "{bad} texels encode a non-finite f16");
        // The values live in the band env.js's header claims ("a clear sky zenith
        // around 8 cd/m^2 relative to a sun of ~30", pre-PMREM and pre-exposure).
        let peak = data
            .iter()
            .step_by(4)
            .map(|h| from_half(*h))
            .fold(0.0_f32, f32::max);
        assert!(
            (0.1..4.0).contains(&peak),
            "the peak red radiance {peak} is outside the scale env.js documents; \
             a value near 25000-lux scene units would mean the wrong sky was ported"
        );
    }

    #[test]
    fn the_map_is_deterministic_and_moves_with_the_sun() {
        let a = build_fallback_environment([0.0, 0.6, 0.8]);
        let b = build_fallback_environment([0.0, 0.6, 0.8]);
        assert_eq!(a, b, "the same sun must produce byte-identical texels");
        let moved = build_fallback_environment([0.0, 0.6, -0.8]);
        assert_ne!(
            a, moved,
            "moving the sun to the opposite azimuth must move the Mie lobe"
        );
    }

    #[test]
    fn nothing_in_the_present_path_builds_this_environment_yet() {
        // The PMREM prefilter is unported and the frame contract has no
        // environment lane, so no pass consumes this map. Stated as a test rather
        // than a comment so the deferral cannot quietly expire: when a
        // `FrameEnvironment` lane lands in `crates/axiom-host` and
        // `scene_wgsl.rs` samples it, this assertion fails and the PMREM question
        // is re-opened deliberately. See the module docs.
        let sources = [
            include_str!("scene_wgsl.rs"),
            include_str!("scene_renderer.rs"),
            include_str!("live_gpu_binding.rs"),
            include_str!("offscreen.rs"),
        ];
        let wired = sources
            .iter()
            .filter(|s| s.contains("build_fallback_environment"))
            .count();
        assert_eq!(
            wired, 0,
            "{wired} render path(s) now build the fallback environment; the PMREM \
             deferral in this module's docs has expired and must be revisited"
        );
    }
}
