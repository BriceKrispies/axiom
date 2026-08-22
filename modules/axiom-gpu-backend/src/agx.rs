//! **AgX** — the filmic tone map the reference actually runs, transcribed from
//! its GLSL.
//!
//! Ported from Claude-of-Duty `src/render/glsl.js:110-160` (the `TONEMAP`
//! chunk), as called by `src/render/composite.js:132`
//! (`owAgX( hdr, uLook.x, uLook.y, uLook.z )`).
//!
//! # This is not the grade that is here today, and it is not ACES
//!
//! `crate::post_chain`'s composite ends in a reciprocal *rolloff* shoulder
//! plus [`axiom_host::FramePostProcess`]'s exposure/contrast/saturation grade —
//! an LDR curve applied to values that a `Rgba8UnormSrgb` intermediate has
//! already clamped to display white. It is frequently *described* as ACES; it is
//! not, and neither is this. AgX and ACES differ in every constant:
//!
//! | | ACES (`owACES`, `glsl.js:167`) | AgX (here) |
//! |---|---|---|
//! | working space | the ACES "RRT fit" basis | Rec.2020, then a second *inset* |
//! | curve | the Narkowicz/Hill rational fit | a 6th-order polynomial in **log** space |
//! | in-matrix row 0 | `0.59719, 0.35458, 0.04823` | `0.6274, 0.3293, 0.0433` then `0.856627…, 0.0951…, 0.0482…` |
//!
//! The source file defines `owACES` as well and the composite **does not call
//! it**. Nothing in this module is derived from it.
//!
//! # Reading the GLSL matrices
//!
//! GLSL's `mat3( a, b, c )` takes **columns**, and `m * v` is
//! `c0 * v.x + c1 * v.y + c2 * v.z`. So the mathematical *rows* used below are
//! the first components of each column vector, in order — transposing the
//! literal text as it appears in `glsl.js`. Getting this backwards is a silent
//! hue rotation, not a compile error, which is why the expansion is written out
//! term by term on both sides rather than delegated to a matrix type.
//!
//! Every matrix product is written out as
//! `m00 * v.x + m01 * v.y + m02 * v.z` in that order and left-associated,
//! identically in the WGSL and in the CPU reference. The GLSL itself says only
//! `m * v`, whose factoring the specification leaves open, so this expansion is
//! *a* choice — but it is the same choice on both sides, which is what makes the
//! parity number below mean something.
//!
//! # The error budget is the contrast polynomial, not the `pow`s
//!
//! That is a **measured** finding and it is the opposite of what this module's
//! first draft claimed. The parity sweep isolates each stage (see
//! `parity::TOLERANCE`), and on a Vulkan adapter:
//!
//! | stage | worst scaled deviation |
//! |---|---|
//! | the four transcendentals alone (`log2`, `pow(x,1)`, `pow(x,2.2)`, `pow(x,power)`) | `1.2e-7` |
//! | the two input matrices | `1.1e-7` |
//! | contrast + outset + the output matrix, over the raw unit input | `4.8e-7` |
//! | **the whole chain** | **`8.3e-6`** |
//!
//! Seventy times the worst individual stage, from calls that are each within two
//! ULP. The cause is [`contrast`]: `15.5x⁶ − 40.14x⁵ + 31.96x⁴ − …` reaches
//! **intermediates of magnitude 40 to produce a result near 1**, so it is
//! catastrophically cancelling. One f32 rounding at magnitude 40 is `3.8e-6`
//! *absolute*, and that lands undiminished on a unit result; the final
//! `pow(_, 2.2)` then multiplies it by 2.2. An `fma` contraction — which a GPU
//! is entitled to and Rust is not — is enough to move exactly one of those
//! roundings.
//!
//! Two consequences. First, **the polynomial's grouping is load-bearing** in a
//! way stronger than the usual "float is not associative": re-associating it
//! changes the frame by parts in a million, not parts in a billion. Second, no
//! amount of care in the transcription closes this gap, so the budget is what it
//! is and shrinking it would mean rewriting the source's curve.
//!
//! (`pow(x, 1.0)` — the shipped default — is still not *free*: Rust's `powf`
//! returns `x` exactly for an exponent of one where a GPU may evaluate
//! `exp2(1.0 * log2(x))`. It is simply not the dominant term, which is why it is
//! measured rather than assumed.)
//!
//! # HDR in, display-referred out
//!
//! [`agx`] expects **linear scene radiance already multiplied by the exposure
//! scalar** — `composite.js` applies `hdr *= exposure`, adds bloom and the
//! `cos^4` lens falloff in linear light, and only then calls `owAgX`. Feeding it
//! values that an 8-bit sRGB attachment has already clamped to `1.0` throws away
//! every stop the curve exists to compress. See [`crate::exposure`] for the
//! measurement that produces the exposure scalar, and this crate's
//! `surface_encode::scene_target_format` for the intermediate that must become
//! `Rgba16Float` before either is worth switching on.

use axiom_math::Vec3;

/// The bottom of AgX's log window, in stops relative to mid grey
/// (`glsl.js:141`). Declared `f32` so `MAX_EV - MIN_EV` is an `f32` subtraction
/// on both sides — a WGSL const-expression over bare literals would fold in
/// `AbstractFloat` and land on a different `f32`.
pub(crate) const MIN_EV: f32 = -12.47393;

/// The top of AgX's log window (`glsl.js:142`).
pub(crate) const MAX_EV: f32 = 4.026069;

/// The composite's shipped look: `uLook = vec4( 1.0, 1.0, 1.08, 1 )`
/// (`composite.js:344`) — slope, power, saturation, and a fourth lane that is an
/// exposure multiplier consumed before the tone map, not by it.
///
/// The source's comment on those first two is emphatic and worth keeping:
/// **slope is 1.0 and must stay there.** It multiplies the *log-normalised*
/// value, so 1.05 is not "5% brighter", it is +0.5 EV applied after AgX has
/// already placed mid grey. `power > 1` costs whole stops in the shadows,
/// because `minEv..maxEv` spans 16.5 of them.
pub(crate) const LOOK_SLOPE: f32 = 1.0;

/// The shipped look's `power` (`composite.js:344`). See [`LOOK_SLOPE`].
pub(crate) const LOOK_POWER: f32 = 1.0;

/// The shipped look's log-space saturation (`composite.js:344`).
pub(crate) const LOOK_SATURATION: f32 = 1.08;

/// AgX as WGSL: the same functions, in the same order, as the GLSL `TONEMAP`
/// chunk.
///
/// A `&str` with no bindings and no entry point, so it concatenates in front of
/// whichever pass needs it — exactly how `crate::surface_encode`'s transfer
/// curve and `material_shader`'s twelve layers are composed. Nothing in this
/// crate concatenates it yet; see the module docs and
/// `tests::nothing_in_the_present_path_compiles_this_yet`.
///
/// `clamp`, `mix` and `dot` are written out rather than called: WGSL's builtins
/// are permitted to factor differently from GLSL's, and this text has to mean
/// exactly what `glsl.js` means.
pub(crate) const AGX_WGSL: &str = r#"
// AgX, from Claude-of-Duty `src/render/glsl.js:110-160`. See `agx.rs` for why
// the matrices below are the TRANSPOSE of the literal text there: GLSL's
// `mat3(a, b, c)` takes columns.

const AXIOM_AGX_MIN_EV: f32 = -12.47393;
const AXIOM_AGX_MAX_EV: f32 = 4.026069;

fn axiom_agx_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

fn axiom_agx_clamp3(v: vec3<f32>, lo: f32, hi: f32) -> vec3<f32> {
    return vec3<f32>(
        axiom_agx_clamp(v.x, lo, hi),
        axiom_agx_clamp(v.y, lo, hi),
        axiom_agx_clamp(v.z, lo, hi),
    );
}

// `owLum` — Rec.709 luminance (`glsl.js:31`), applied here to the
// LOG-NORMALISED Rec.2020 value. That is what the source does; it is not a
// mistake to "fix".
fn axiom_agx_lum(c: vec3<f32>) -> f32 {
    return c.x * 0.2126 + c.y * 0.7152 + c.z * 0.0722;
}

fn axiom_agx_rec2020_from_srgb(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.6274 * c.x + 0.3293 * c.y + 0.0433 * c.z,
        0.0691 * c.x + 0.9195 * c.y + 0.0113 * c.z,
        0.0164 * c.x + 0.0880 * c.y + 0.8956 * c.z,
    );
}

fn axiom_agx_srgb_from_rec2020(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
         1.6605 * c.x - 0.5876 * c.y - 0.0728 * c.z,
        -0.1246 * c.x + 1.1329 * c.y - 0.0083 * c.z,
        -0.0182 * c.x - 0.1006 * c.y + 1.1187 * c.z,
    );
}

fn axiom_agx_inset(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.856627153315983 * c.x + 0.0951212405381588 * c.y + 0.0482516061458583 * c.z,
        0.137318972929847 * c.x + 0.761241990602591 * c.y + 0.101439036467562 * c.z,
        0.11189821299995 * c.x + 0.0767994186031903 * c.y + 0.811302368396859 * c.z,
    );
}

fn axiom_agx_outset(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
         1.1271005818144368 * c.x - 0.11060664309660323 * c.y - 0.016493938717834573 * c.z,
        -0.1413297634984383 * c.x + 1.157823702216272 * c.y - 0.016493938717834257 * c.z,
        -0.14132976349843826 * c.x - 0.11060664309660294 * c.y + 1.2519364065950405 * c.z,
    );
}

// `owAgxContrast` (`glsl.js:123-128`). The grouping is the specification:
// `x4` is `(x*x)*(x*x)`, and every term keeps the source's factor order.
fn axiom_agx_contrast(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x
         + 0.4298 * x2 + 0.1191 * x - 0.00232;
}

// `owAgX` (`glsl.js:132-160`), step for step.
fn axiom_agx(color_in: vec3<f32>, slope: f32, power: f32, sat: f32) -> vec3<f32> {
    var color = axiom_agx_rec2020_from_srgb(color_in);
    color = axiom_agx_inset(color);
    color = vec3<f32>(max(color.x, 1e-10), max(color.y, 1e-10), max(color.z, 1e-10));
    color = vec3<f32>(
        (log2(color.x) - AXIOM_AGX_MIN_EV) / (AXIOM_AGX_MAX_EV - AXIOM_AGX_MIN_EV),
        (log2(color.y) - AXIOM_AGX_MIN_EV) / (AXIOM_AGX_MAX_EV - AXIOM_AGX_MIN_EV),
        (log2(color.z) - AXIOM_AGX_MIN_EV) / (AXIOM_AGX_MAX_EV - AXIOM_AGX_MIN_EV),
    );
    color = axiom_agx_clamp3(color, 0.0, 1.0);

    // look: slope / power / saturation in log space
    color = vec3<f32>(
        pow(max(color.x * slope, 0.0), power),
        pow(max(color.y * slope, 0.0), power),
        pow(max(color.z * slope, 0.0), power),
    );
    let l = axiom_agx_lum(color);
    color = vec3<f32>(
        l + sat * (color.x - l),
        l + sat * (color.y - l),
        l + sat * (color.z - l),
    );

    color = axiom_agx_contrast(axiom_agx_clamp3(color, 0.0, 1.0));
    color = axiom_agx_outset(color);
    color = vec3<f32>(
        pow(max(color.x, 0.0), 2.2),
        pow(max(color.y, 0.0), 2.2),
        pow(max(color.z, 0.0), 2.2),
    );
    color = axiom_agx_srgb_from_rec2020(color);
    return axiom_agx_clamp3(color, 0.0, 1.0);
}
"#;

/// GLSL `clamp( x, lo, hi )` — `min( max( x, lo ), hi )`, written out because
/// that expansion is the specification and a builtin's is not guaranteed to be.
fn glsl_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    f32::min(f32::max(x, lo), hi)
}

/// GLSL `clamp` over a `vec3`.
fn glsl_clamp3(v: Vec3, lo: f32, hi: f32) -> Vec3 {
    Vec3::new(
        glsl_clamp(v.x, lo, hi),
        glsl_clamp(v.y, lo, hi),
        glsl_clamp(v.z, lo, hi),
    )
}

/// `owLum` (`glsl.js:31`), Rec.709 luminance, term order as `dot` expands it.
pub(crate) fn lum(c: Vec3) -> f32 {
    c.x * 0.2126 + c.y * 0.7152 + c.z * 0.0722
}

/// `OW_REC2020_FROM_SRGB * c` (`glsl.js:114-117`).
pub(crate) fn rec2020_from_srgb(c: Vec3) -> Vec3 {
    Vec3::new(
        0.6274 * c.x + 0.3293 * c.y + 0.0433 * c.z,
        0.0691 * c.x + 0.9195 * c.y + 0.0113 * c.z,
        0.0164 * c.x + 0.0880 * c.y + 0.8956 * c.z,
    )
}

/// `OW_SRGB_FROM_REC2020 * c` (`glsl.js:118-121`).
pub(crate) fn srgb_from_rec2020(c: Vec3) -> Vec3 {
    Vec3::new(
        1.6605 * c.x - 0.5876 * c.y - 0.0728 * c.z,
        -0.1246 * c.x + 1.1329 * c.y - 0.0083 * c.z,
        -0.0182 * c.x - 0.1006 * c.y + 1.1187 * c.z,
    )
}

/// AgX's `inset * c` (`glsl.js:133-136`).
pub(crate) fn inset(c: Vec3) -> Vec3 {
    Vec3::new(
        0.856_627_153_315_983 * c.x + 0.095_121_240_538_158_8 * c.y + 0.048_251_606_145_858_3 * c.z,
        0.137_318_972_929_847 * c.x + 0.761_241_990_602_591 * c.y + 0.101_439_036_467_562 * c.z,
        0.111_898_212_999_95 * c.x + 0.076_799_418_603_190_3 * c.y + 0.811_302_368_396_859 * c.z,
    )
}

/// AgX's `outset * c` (`glsl.js:137-140`).
pub(crate) fn outset(c: Vec3) -> Vec3 {
    Vec3::new(
        1.127_100_581_814_436_8 * c.x
            - 0.110_606_643_096_603_23 * c.y
            - 0.016_493_938_717_834_573 * c.z,
        -0.141_329_763_498_438_3 * c.x + 1.157_823_702_216_272 * c.y
            - 0.016_493_938_717_834_257 * c.z,
        -0.141_329_763_498_438_26 * c.x - 0.110_606_643_096_602_94 * c.y
            + 1.251_936_406_595_040_5 * c.z,
    )
}

/// One channel of `owAgxContrast` (`glsl.js:123-128`).
///
/// The 6th-order polynomial AgX applies to its log-normalised value. The
/// grouping is transcribed, not tidied: `x4` is `(x*x)*(x*x)`, `x^6` is
/// `x4 * x2`, `x^5` is `x4 * x`, and the seven terms are summed left to right.
fn contrast_channel(x: f32) -> f32 {
    let x2 = x * x;
    let x4 = x2 * x2;
    15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x
        - 0.00232
}

/// `owAgxContrast` (`glsl.js:123-128`).
pub(crate) fn contrast(x: Vec3) -> Vec3 {
    Vec3::new(
        contrast_channel(x.x),
        contrast_channel(x.y),
        contrast_channel(x.z),
    )
}

/// `owAgX( color, slope, power, sat )` (`glsl.js:132-160`) — **the semantic
/// definition** this crate's WGSL is a mirror of.
///
/// `color` is linear sRGB-primaries scene radiance, already multiplied by the
/// exposure scalar; the result is display-referred sRGB *primaries* in `0..=1`,
/// still linear-light (the composite encodes it separately, `composite.js:142`).
pub(crate) fn agx(color: Vec3, slope: f32, power: f32, sat: f32) -> Vec3 {
    let color = rec2020_from_srgb(color);
    let color = inset(color);
    let color = Vec3::new(
        f32::max(color.x, 1e-10),
        f32::max(color.y, 1e-10),
        f32::max(color.z, 1e-10),
    );
    let span = MAX_EV - MIN_EV;
    let color = Vec3::new(
        (color.x.log2() - MIN_EV) / span,
        (color.y.log2() - MIN_EV) / span,
        (color.z.log2() - MIN_EV) / span,
    );
    let color = glsl_clamp3(color, 0.0, 1.0);

    // look: slope / power / saturation in log space
    let color = Vec3::new(
        f32::max(color.x * slope, 0.0).powf(power),
        f32::max(color.y * slope, 0.0).powf(power),
        f32::max(color.z * slope, 0.0).powf(power),
    );
    let l = lum(color);
    let color = Vec3::new(
        l + sat * (color.x - l),
        l + sat * (color.y - l),
        l + sat * (color.z - l),
    );

    let color = contrast(glsl_clamp3(color, 0.0, 1.0));
    let color = outset(color);
    let color = Vec3::new(
        f32::max(color.x, 0.0).powf(2.2),
        f32::max(color.y, 0.0).powf(2.2),
        f32::max(color.z, 0.0).powf(2.2),
    );
    let color = srgb_from_rec2020(color);
    glsl_clamp3(color, 0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The look the composite ships, so a test reads the same curve the frame
    /// would.
    fn shipped(c: Vec3) -> Vec3 {
        agx(c, LOOK_SLOPE, LOOK_POWER, LOOK_SATURATION)
    }

    #[test]
    fn the_log_window_is_the_sources_and_spans_sixteen_and_a_half_stops() {
        assert_eq!(MIN_EV, -12.47393, "glsl.js:141");
        assert_eq!(MAX_EV, 4.026069, "glsl.js:142");
        let span = MAX_EV - MIN_EV;
        assert!(
            (span - 16.5).abs() < 1.0e-5,
            "AgX's window is 16.5 stops; got {span}"
        );
    }

    #[test]
    fn the_shipped_look_is_the_composites_ulook() {
        // `composite.js:344` — vec4( 1.0, 1.0, 1.08, 1 ).
        assert_eq!(LOOK_SLOPE, 1.0);
        assert_eq!(LOOK_POWER, 1.0);
        assert_eq!(LOOK_SATURATION, 1.08);
    }

    /// The primary matrices must be inverses of one another, which is the one
    /// property a transposition error cannot fake: transposing either one breaks
    /// the round trip on any colour that is not grey.
    #[test]
    fn the_rec2020_matrices_round_trip() {
        [
            Vec3::new(0.9, 0.2, 0.05),
            Vec3::new(0.03, 0.61, 0.44),
            Vec3::new(0.18, 0.18, 0.18),
        ]
        .iter()
        .for_each(|c| {
            let back = srgb_from_rec2020(rec2020_from_srgb(*c));
            [(back.x, c.x), (back.y, c.y), (back.z, c.z)]
                .iter()
                .for_each(|(got, want)| {
                    assert!(
                        (got - want).abs() < 2.0e-4,
                        "sRGB -> Rec.2020 -> sRGB moved {want} to {got}"
                    );
                });
        });
    }

    /// The AgX inset/outset pair are likewise near-inverses (they are not exact
    /// inverses by construction — the outset deliberately re-expands slightly
    /// less than the inset compressed, which is the "AgX rendering intent").
    #[test]
    fn the_inset_and_outset_are_near_inverses_but_not_exact_ones() {
        let c = Vec3::new(0.4, 0.25, 0.7);
        let back = outset(inset(c));
        let worst = [(back.x, c.x), (back.y, c.y), (back.z, c.z)]
            .iter()
            .map(|(g, w)| (g - w).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            worst > 1.0e-4,
            "the outset is not the inset's inverse; if it were, AgX would have no chroma intent"
        );
        assert!(
            worst < 0.1,
            "...but it is close to one; worst channel moved {worst}"
        );
        // A neutral is preserved far more tightly than a saturated colour: both
        // matrices have near-unit row sums, which is what keeps grey grey.
        let grey = Vec3::new(0.18, 0.18, 0.18);
        let grey_back = outset(inset(grey));
        assert!(
            (grey_back.x - grey.x).abs() < 1.0e-3,
            "grey must survive the inset/outset pair"
        );
    }

    /// Every matrix here is stated as ROWS, transposed out of the GLSL's
    /// column-major `mat3(...)`. This pins the first column of the text — the
    /// numbers a copy-paste would have put in row 0.
    #[test]
    fn the_matrices_are_the_transpose_of_the_glsl_text() {
        // `mat3( vec3( 0.6274, 0.0691, 0.0164 ), ... )` — the first *column*.
        // Applying the matrix to the red basis vector must therefore return it.
        let red = rec2020_from_srgb(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!((red.x, red.y, red.z), (0.6274, 0.0691, 0.0164));
        let inset_red = inset(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(
            (inset_red.x, inset_red.y, inset_red.z),
            (0.856_627_153_315_983, 0.137_318_972_929_847, 0.111_898_212_999_95)
        );
        let outset_red = outset(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(
            (outset_red.x, outset_red.y, outset_red.z),
            (
                1.127_100_581_814_436_8,
                -0.141_329_763_498_438_3,
                -0.141_329_763_498_438_26
            )
        );
        let back_red = srgb_from_rec2020(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!((back_red.x, back_red.y, back_red.z), (1.6605, -0.1246, -0.0182));
    }

    /// The contrast polynomial is monotone across the unit interval and stops
    /// just short of both ends: `-0.00232` at zero and `0.99858` at one.
    ///
    /// Neither end is a round number, and that is measured rather than assumed —
    /// AgX's curve is a fit, and the *first* draft of this test asserted the
    /// polynomial overshot white. It does not, which is why the whole tone map's
    /// ceiling is 0.997 rather than 1.0 (see
    /// [`the_tone_map_is_monotone_and_bounded`]).
    #[test]
    fn the_contrast_polynomial_is_monotone_and_stops_just_short_of_both_ends() {
        assert_eq!(contrast_channel(0.0), -0.00232, "the constant term");
        let one = contrast_channel(1.0);
        assert!(
            (0.998..1.0).contains(&one),
            "the polynomial lands just under display white; got {one}"
        );
        let rising = (0..=100)
            .map(|i| contrast_channel(i as f32 / 100.0))
            .collect::<Vec<f32>>();
        rising.windows(2).for_each(|w| {
            assert!(w[1] > w[0], "the contrast curve must rise: {:?}", w);
        });
        // AgX does **not** pivot about the middle of its log window, and
        // assuming it did is the mistake that would put mid grey in the wrong
        // place: 0.5 in comes out at 0.29. What actually sits near the pivot is
        // 18% scene grey, which lands at 0.606 of the window.
        // Values are bound before the assertion, never passed as a trailing
        // format argument: an argument expression is only evaluated when the
        // assertion FAILS, which is a region no passing run can reach and a
        // hole in the coverage gate. Same reason, everywhere below.
        let mid = contrast_channel(0.5);
        assert!(
            (mid - 0.2915).abs() < 1.0e-3,
            "the middle of the window maps to 0.29, not 0.5; got {mid}"
        );
        let grey_at = (0.18_f32.log2() - MIN_EV) / (MAX_EV - MIN_EV);
        assert!(
            (grey_at - 0.6061).abs() < 1.0e-3,
            "18% grey sits at 0.606 of the log window; got {grey_at}"
        );
        let at_grey = contrast_channel(grey_at);
        assert!(
            (at_grey - 0.4968).abs() < 1.0e-3,
            "...and the curve puts it just under half way; got {at_grey}"
        );
    }

    /// The whole curve is monotone in scene radiance and saturates at black and
    /// white, which is what makes it a tone *map*.
    #[test]
    fn the_tone_map_is_monotone_and_bounded() {
        let stops: Vec<f32> = (0..=40).map(|i| (i as f32 - 24.0).exp2()).collect();
        let out: Vec<f32> = stops
            .iter()
            .map(|s| shipped(Vec3::new(*s, *s, *s)).x)
            .collect();
        out.windows(2).for_each(|w| {
            assert!(
                w[1] >= w[0],
                "the tone map must not fall as the scene brightens: {:?}",
                w
            );
        });
        out.iter().for_each(|v| {
            assert!(
                (0.0..=1.0).contains(v),
                "AgX clamps to the unit interval; got {v}"
            );
        });
        assert_eq!(out[0], 0.0, "2^-24 is below the log window's floor");
        // The ceiling is 0.99698, NOT 1.0 — code value 254, not 255. AgX's
        // contrast polynomial tops out at 0.99858 (see
        // `the_contrast_polynomial_is_monotone_and_stops_just_short_of_both_ends`)
        // and the outset plus the 2.2 encode take a little more off. Measured,
        // and worth knowing: nothing this curve produces is ever pure white, so a
        // downstream test that asserts a blown highlight reaches 1.0 is wrong.
        let ceiling = out[out.len() - 1];
        assert!(
            (0.996..0.998).contains(&ceiling),
            "AgX's own ceiling is just under display white; got {ceiling}"
        );
    }

    /// Mid grey. AgX's whole point is where it puts 18% scene reflectance, and
    /// the source's comment (`composite.js:336-343`) says a slope above 1.0 with
    /// a contrast pivot below mid grey is what put "18% scene grey on code value
    /// 153" — i.e. too high. With the shipped slope of 1.0, 0.18 in must land
    /// near 0.18 out in linear light, which is code value ~118 once the
    /// composite's sRGB encode runs.
    #[test]
    fn mid_grey_lands_near_mid_grey() {
        let out = shipped(Vec3::new(0.18, 0.18, 0.18)).x;
        assert!(
            (0.12..0.24).contains(&out),
            "18% scene grey mapped to {out} linear, which is not mid grey"
        );
    }

    /// Black is black and stays black; the polynomial's `-0.00232` cannot leak
    /// a negative out through the final clamp.
    #[test]
    fn black_maps_to_exact_black() {
        let out = shipped(Vec3::new(0.0, 0.0, 0.0));
        assert_eq!((out.x, out.y, out.z), (0.0, 0.0, 0.0));
    }

    /// A negative input — which a bloom subtract or a filtered tap can produce —
    /// is floored by the same `max(color, 1e-10)` the source uses, so it cannot
    /// turn into a `NaN` inside `log2`.
    #[test]
    fn a_negative_radiance_is_floored_rather_than_producing_a_nan() {
        let out = shipped(Vec3::new(-3.0, -0.0001, 0.5));
        assert!(out.x.is_finite() & out.y.is_finite() & out.z.is_finite());
        assert_eq!(out.x, 0.0, "a negative channel bottoms out at black");
    }

    /// Saturation above one pushes a colour away from its own luminance in log
    /// space, and the shipped 1.08 does so measurably without leaving the gamut.
    #[test]
    fn the_look_saturation_separates_a_colour_from_its_neutral() {
        let c = Vec3::new(0.6, 0.22, 0.09);
        let neutral = agx(c, LOOK_SLOPE, LOOK_POWER, 1.0);
        let punchy = agx(c, LOOK_SLOPE, LOOK_POWER, LOOK_SATURATION);
        let spread = |v: Vec3| v.x - v.z;
        let punchy_spread = spread(punchy);
        let neutral_spread = spread(neutral);
        assert!(
            punchy_spread > neutral_spread,
            "sat 1.08 must widen the red/blue separation:              {punchy_spread} vs {neutral_spread}"
        );
        // ...and a scene neutral is *almost* untouched by saturation, at any
        // amount — but not exactly, and the difference is a real property of the
        // curve rather than a defect. The Rec.2020 and inset matrices have
        // near-unit but unequal row sums, so an RGB-equal input is no longer
        // channel-equal by the time the saturation is applied, and the log
        // normalisation spreads that further. Measured at ~1.4e-4 relative; an
        // earlier draft of this test asserted exact equality and was wrong.
        let grey = Vec3::new(0.3, 0.3, 0.3);
        let a = agx(grey, LOOK_SLOPE, LOOK_POWER, 1.0);
        let b = agx(grey, LOOK_SLOPE, LOOK_POWER, 3.0);
        let drift = [(a.x, b.x), (a.y, b.y), (a.z, b.z)]
            .iter()
            .map(|(l, r)| (l - r).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            drift > 0.0,
            "the inset does not preserve neutrality exactly, so saturation must \
             move a scene grey a little"
        );
        assert!(drift < 1.0e-3, "...but only a little; got {drift}");
    }

    /// A slope below one darkens and a power above one crushes the shadows —
    /// the two knobs the source warns about, exercised so neither is dead code.
    #[test]
    fn the_slope_and_power_knobs_move_the_curve_in_the_documented_direction() {
        let c = Vec3::new(0.18, 0.18, 0.18);
        let base = shipped(c).x;
        let flatter = agx(c, 0.8, LOOK_POWER, LOOK_SATURATION).x;
        let crushed = agx(c, LOOK_SLOPE, 1.35, LOOK_SATURATION).x;
        assert!(flatter < base, "a slope below one darkens: {flatter} vs {base}");
        assert!(
            crushed < base,
            "a power above one costs stops in the shadows: {crushed} vs {base}"
        );
    }

    /// AgX's hue behaviour, and the reason the source chose it over ACES: a
    /// blown *saturated* channel desaturates toward white instead of skewing.
    /// Four stops over white, a pure red must have brought its other two
    /// channels up rather than clipping to primary red.
    #[test]
    fn a_blown_saturated_highlight_desaturates_rather_than_skewing() {
        let hot = shipped(Vec3::new(16.0, 0.4, 0.4));
        assert!(hot.x > 0.99, "the red channel is blown; got {}", hot.x);
        assert!(
            hot.y > 0.2,
            "AgX must lift the unblown channels toward white; got {}",
            hot.y
        );
    }

    /// Every WGSL entry point the CPU reference above claims to mirror is
    /// actually declared in the shader text.
    #[test]
    fn the_wgsl_declares_every_function_this_module_mirrors() {
        [
            "fn axiom_agx_clamp(",
            "fn axiom_agx_clamp3(",
            "fn axiom_agx_lum(",
            "fn axiom_agx_rec2020_from_srgb(",
            "fn axiom_agx_srgb_from_rec2020(",
            "fn axiom_agx_inset(",
            "fn axiom_agx_outset(",
            "fn axiom_agx_contrast(",
            "fn axiom_agx(",
        ]
        .iter()
        .for_each(|needle| {
            assert!(AGX_WGSL.contains(needle), "AGX_WGSL is missing {needle}");
        });
    }

    /// The WGSL must not reach for a builtin whose factoring the specification
    /// leaves open where the GLSL's is pinned. `pow`, `log2`, `min` and `max`
    /// are the permitted ones — the first two because both sides approximate
    /// them anyway and the last two because they are exact.
    #[test]
    fn the_wgsl_calls_no_unspecified_builtin() {
        // The written-out names are removed first, so a needle that is a
        // SUFFIX of one of them (`step(` inside `smoothstep(`) cannot produce a
        // false positive and cannot hide a real one either.
        let stripped = AGX_WGSL
            .replace("axiom_agx_clamp3(", "")
            .replace("axiom_agx_clamp(", "");
        ["clamp(", "mix(", "dot(", "smoothstep(", "step("]
            .iter()
            .for_each(|needle| {
                assert!(
                    !stripped.contains(needle),
                    "AGX_WGSL calls the {needle} builtin"
                );
            });
        // The guard has teeth: the written-out names really are there.
        assert!(AGX_WGSL.contains("axiom_agx_clamp("));
        assert!(AGX_WGSL.contains("axiom_agx_clamp3("));
    }

    /// **The opt-in proof, now that the wiring exists.**
    ///
    /// This replaces the earlier `nothing_in_the_present_path_compiles_this_yet`,
    /// under the terms that test set for its own replacement: AgX is wired in, so
    /// the guard is no longer "nobody references it" but "only the opted-in arm
    /// can". Two halves, both source scans, because the property is about which
    /// *text* reaches a shader module:
    ///
    /// * The two present paths that are not the HDR composite —
    ///   [`crate::upscale`], the plain blit an app with no post chain presents
    ///   through, and [`crate::surface_encode`], the funnel every post shader's
    ///   text goes through — must still never mention it.
    /// * In [`crate::post_chain`] it must appear **only** inside the `Some` arm of
    ///   `composite_source`, whose other arm is the untouched LDR source. That the
    ///   other arm really is untouched is proven, byte for byte, by
    ///   `post_chain::tests::the_ldr_composite_source_is_exactly_what_it_always_was`
    ///   and, on real pixels, by
    ///   `offscreen::tests::a_frame_that_authors_no_tonemap_is_byte_identical`.
    #[test]
    fn only_the_opted_in_composite_compiles_this() {
        [
            ("upscale.rs", include_str!("upscale.rs")),
            ("surface_encode.rs", include_str!("surface_encode.rs")),
        ]
        .iter()
        .for_each(|(name, source)| {
            assert!(
                !source.contains("AGX_WGSL") & !source.contains("agx::"),
                "{name} now references AgX; a present path that is not the HDR \
                 composite has picked up the tone map"
            );
        });
        let post = include_str!("post_chain.rs");
        assert!(
            post.contains("crate::agx::AGX_WGSL"),
            "post_chain no longer splices AgX at all; the HDR arm has been lost"
        );
        assert!(
            post.contains("fn composite_source(tonemap: Option<&axiom_host::FrameTonemap>)"),
            "the splice is no longer gated on an authored tone map"
        );
    }
}

// The CPU reference above is the semantic definition; this holds it up against a
// real GPU running `AGX_WGSL`. Compiled only with `--features offscreen`, and it
// ASSERTS an adapter was acquired rather than skipping. The harness shape is
// `crate::material_shader::cloth`'s, which is in turn
// `crate::surface_program::parity`'s; neither is reusable from here because both
// are `pub(super)` to a module this slice may not edit.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::*;

    /// How many contexts one run compares, and the target's width.
    const SAMPLES: usize = 24;

    /// Sixteen-byte lanes per context in the uniform block.
    const LANES: usize = 2;

    /// `copy_texture_to_buffer` requires each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// The agreement budget, **relative above unit magnitude**: a deviation is
    /// scored as `|got - want| / max(|want|, 1)`.
    ///
    /// Where the deviation comes from — **derived from the sweep's own
    /// per-stage numbers**, which is why [`agx_pow_fs`] exists at all. On the
    /// Vulkan adapter this was measured on:
    ///
    /// | entry point | worst scaled |
    /// |---|---|
    /// | `agx_pow_fs` (the four transcendentals alone) | `1.23e-7` |
    /// | `agx_inset_fs` (`rec2020` then `inset`) | `1.15e-7` |
    /// | `agx_outset_fs` (`contrast`, `outset`, `srgb`) | `4.77e-7` |
    /// | `agx_fs` (the whole curve) | **`8.34e-6`** |
    ///
    /// So it is **not** the `pow`s and **not** the matrices: each is inside two
    /// ULP. It is [`super::contrast`], which computes a result near 1 out of
    /// terms reaching magnitude 40 (`15.5x⁶ − 40.14x⁵ + 31.96x⁴ − …`). One f32
    /// rounding at that magnitude is `3.8e-6` absolute and lands undiminished on
    /// the unit-scale result; the final `pow(_, 2.2)` scales it by 2.2. A single
    /// `fma` contraction — permitted to the GPU, unavailable to Rust — moves one
    /// of those roundings, and `8.3e-6` is what that costs. `agx_outset_fs` sees
    /// far less of it only because its inputs are the raw clamped contexts, most
    /// of which sit low in the polynomial's range rather than up where the terms
    /// are largest.
    ///
    /// The outputs live in `0..=1`, so the `max(_, 1)` floor makes this an
    /// absolute budget in practice; the relative form is kept so the
    /// intermediate-valued entry points (the inset reaches ~40) are scored on the
    /// same scale.
    ///
    /// **Measured, not fitted**: [`MEASURED_WORST`] is what this machine reports
    /// and is asserted, so the justification cannot rot. The budget is 2.4x it —
    /// room for a second contracted multiply-add inside the polynomial on another
    /// vendor, and no more.
    const TOLERANCE: f32 = 2.0e-5;

    /// The worst scaled deviation this module has actually been measured at
    /// (Vulkan, `agx_fs` sample 16: GPU `0.9316661` vs CPU `0.93165773`).
    /// Updated only from a real run, and only ever from the *worst* adapter
    /// measured — never nudged to make a run pass.
    const MEASURED_WORST: f32 = 8.4e-6;

    /// One context: an HDR colour and the three look knobs.
    struct Context {
        color: Vec3,
        slope: f32,
        power: f32,
        sat: f32,
    }

    /// The contexts, chosen to cross every regime the curve has: below the log
    /// window's floor, across mid grey, and several stops over display white;
    /// a negative channel; strongly saturated hues in each primary; and look
    /// knobs on both sides of the shipped defaults, including `power == 1.0`
    /// exactly (the shipped value, and the one `powf` special-cases).
    fn contexts() -> Vec<Context> {
        (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                // 2^-14 .. 2^9 — the window's floor to nine stops over white.
                let level = (t - 14.0).exp2();
                Context {
                    color: Vec3::new(
                        level * (0.2 + t * 0.07),
                        level * (0.9 - t * 0.03),
                        // Every fourth context drives a channel negative, which
                        // the `max(color, 1e-10)` floor has to absorb.
                        level * [0.45, -0.2, 1.6, 0.05][index % 4],
                    ),
                    slope: [1.0, 0.8, 1.0, 1.15][index % 4],
                    power: [1.0, 1.0, 1.35, 0.85][index % 4],
                    sat: 1.08 + (t * 0.21).sin() * 0.4,
                }
            })
            .collect()
    }

    /// The harness: a fullscreen triangle whose fragment stage evaluates the
    /// entry point at the context its pixel column names.
    const HARNESS_WGSL: &str = r#"
struct AgxContexts { items: array<vec4<f32>, 48> };
@group(0) @binding(0) var<uniform> ctx: AgxContexts;

fn lane(index: u32, slot: u32) -> vec4<f32> { return ctx.items[index * 2u + slot]; }

@vertex
fn agx_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn agx_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 0u);
    let b = lane(i, 1u);
    return vec4<f32>(axiom_agx(a.xyz, a.w, b.x, b.y), 0.0);
}

// The two matrices AgX applies on the way IN, and the luminance the look's
// saturation is taken about. Their outputs leave the unit band, which is what
// the relative scoring in `compare` is for.
@fragment
fn agx_inset_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 0u);
    let v = axiom_agx_inset(axiom_agx_rec2020_from_srgb(a.xyz));
    return vec4<f32>(v, axiom_agx_lum(a.xyz));
}

// The two on the way OUT, over a clamped input so the polynomial's own domain
// is what is being compared.
@fragment
fn agx_outset_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 0u);
    let unit = axiom_agx_clamp3(a.xyz, 0.0, 1.0);
    let v = axiom_agx_srgb_from_rec2020(axiom_agx_outset(axiom_agx_contrast(unit)));
    return vec4<f32>(v, 0.0);
}

// The four transcendentals `axiom_agx` calls, isolated, so the budget below is
// ATTRIBUTED rather than argued. Nothing in the tone map uses this entry point;
// it exists purely so the measurement can say which call costs what.
@fragment
fn agx_pow_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 0u);
    let b = lane(i, 1u);
    let u = axiom_agx_clamp(a.x, 0.0, 1.0);
    return vec4<f32>(
        pow(u, 1.0),
        pow(u, 2.2),
        pow(max(u * a.w, 0.0), b.x),
        log2(max(a.x, 1e-10)),
    );
}
"#;

    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
        fn acquire() -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            Gpu {
                device,
                queue,
                backend: gpu.backend,
            }
        }

        fn render(&self, module: &wgpu::ShaderModule, entry: &str, uniform: &[u8]) -> Vec<[f32; 4]> {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-agx-parity-bgl"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                });
            let buffer = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-agx-parity-uniform"),
                    contents: uniform,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-agx-parity-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-agx-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-agx-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("agx_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba32Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-agx-parity-target"),
                size: wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-agx-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-agx-parity-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::Wait)
                .expect("the readback must complete");
            let mapped = slice.get_mapped_range();
            (0..SAMPLES)
                .map(|sample| {
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = sample * 16 + lane * 4;
                        f32::from_le_bytes([
                            mapped[at],
                            mapped[at + 1],
                            mapped[at + 2],
                            mapped[at + 3],
                        ])
                    })
                })
                .collect()
        }
    }

    /// The uniform block: two `vec4` per context, in the order `lane()` unpacks.
    fn uniform_bytes(contexts: &[Context]) -> Vec<u8> {
        let bytes: Vec<u8> = contexts
            .iter()
            .flat_map(|c| {
                [
                    [c.color.x, c.color.y, c.color.z, c.slope],
                    [c.power, c.sat, 0.0, 0.0],
                ]
            })
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect();
        // An equality, never a `resize`: a `resize` to a smaller length is a
        // silent truncation, and `crate::exposure`'s harness lost a whole day of
        // confidence to exactly that (its lane count was 26 packed against 24
        // strided). Fail loudly on the mismatch instead.
        assert_eq!(
            bytes.len(),
            SAMPLES * LANES * 16,
            "LANES must match what this function packs and what HARNESS_WGSL strides by"
        );
        bytes
    }

    /// Compare one entry point's four lanes against the CPU reference, and
    /// return the worst scaled deviation over the whole sweep together with the
    /// lane it came from.
    ///
    /// One assertion at the end rather than one per lane, so a run reports the
    /// *worst* disagreement rather than the first — which is what a budget has
    /// to be set from.
    fn compare(
        gpu: &Gpu,
        module: &wgpu::ShaderModule,
        entry: &str,
        expected: &[[f32; 4]],
    ) -> (f32, String) {
        let actual = gpu.render(module, entry, &uniform_bytes(&contexts()));
        actual
            .iter()
            .zip(expected)
            .enumerate()
            .flat_map(|(sample, (got, want))| {
                got.iter()
                    .zip(want)
                    .enumerate()
                    .map(move |(lane, (g, w))| (sample, lane, *g, *w))
            })
            .map(|(sample, lane, got, want)| {
                let scaled = (got - want).abs() / f32::max(want.abs(), 1.0);
                (
                    scaled,
                    format!("{entry} sample {sample} lane {lane}: GPU {got} vs CPU {want}"),
                )
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .expect("the sweep compares at least one lane")
    }

    #[test]
    fn agx_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (module, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-agx-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl(format!("{AGX_WGSL}\n{HARNESS_WGSL}").into()),
                })
        });
        assert!(
            failure.is_none(),
            "AGX_WGSL must compile"
        );

        let ctx = contexts();
        let agx_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let v = agx(c.color, c.slope, c.power, c.sat);
                [v.x, v.y, v.z, 0.0]
            })
            .collect();
        let inset_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let v = inset(rec2020_from_srgb(c.color));
                [v.x, v.y, v.z, lum(c.color)]
            })
            .collect();
        let outset_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let v = srgb_from_rec2020(outset(contrast(glsl_clamp3(c.color, 0.0, 1.0))));
                [v.x, v.y, v.z, 0.0]
            })
            .collect();

        let pow_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let u = glsl_clamp(c.color.x, 0.0, 1.0);
                [
                    u.powf(1.0),
                    u.powf(2.2),
                    f32::max(u * c.slope, 0.0).powf(c.power),
                    f32::max(c.color.x, 1e-10).log2(),
                ]
            })
            .collect();

        let per_entry = [
            ("agx_fs", agx_expected),
            ("agx_inset_fs", inset_expected),
            ("agx_outset_fs", outset_expected),
            ("agx_pow_fs", pow_expected),
        ]
        .iter()
        .map(|(entry, expected)| compare(&gpu, &module, entry, expected))
        .collect::<Vec<(f32, String)>>();
        // Every entry point's worst, not just the overall one: the budget below
        // has to be ATTRIBUTABLE, and "which stage costs what" is only visible
        // if the failure message carries all of them.
        let summary = per_entry
            .iter()
            .map(|(w, at)| format!("{w:e} at {at}"))
            .collect::<Vec<String>>()
            .join(" | ");
        let (worst, at) = per_entry
            .iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .cloned()
            .expect("at least one entry point is compared");

        assert!(
            worst <= TOLERANCE,
            "AgX parity on {:?}: worst scaled delta {worst:e} exceeds the budget \
             {TOLERANCE:e}, at {at}",
            gpu.backend
        );
        // The budget must stay a *measurement* plus headroom, never a number
        // fitted to the miss that happened to be observed — so the measurement
        // is asserted, not printed. (Not printed at all: console output is
        // banned in a module and the hygiene scan is not `cfg(test)`-aware.)
        assert!(
            worst <= MEASURED_WORST,
            "AgX parity on {:?}: this adapter deviates by {worst:e} (at {at}), more than \
             the recorded {MEASURED_WORST:e}; redo the error account, do not raise it.              Per entry point: {summary}",
            gpu.backend
        );
    }
}
