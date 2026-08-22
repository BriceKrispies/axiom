//! **The soft-knee highlight prefilter, and the Karis weight.**
//!
//! Transcribed from the GLSL of `src/render/bloom.js`'s `DOWNSAMPLE`:
//!
//! ```glsl
//! float owLum( vec3 c ) { return dot( c, vec3( 0.2126, 0.7152, 0.0722 ) ); }  // glsl.js
//! float karisWeight( vec3 c ) { return 1.0 / ( 1.0 + owLum( c ) ); }
//!
//! vec3 owBloomPrefilter( vec3 c, float thr, float knee ) {
//!   float l = max( max( c.r, c.g ), c.b );
//!   float soft = clamp( l - thr + knee, 0.0, 2.0 * knee );
//!   soft = soft * soft / ( 4.0 * knee + 1e-5 );
//!   return c * ( max( soft, l - thr ) / max( l, 1e-4 ) );
//! }
//! ```
//!
//! Four details a tidier rewrite loses, each of which is pinned by a test below:
//!
//! - **`l` is the max channel, not the luminance.** A red tracer at
//!   `(1.6, 0, 0)` has a Rec.709 luma of `0.34`; judged on that it never blooms
//!   under a threshold of `1.6`. Judged on its max channel it measures `1.6` and
//!   blooms exactly as hard as a white light of the same peak. This is the single
//!   largest divergence from what [`crate::post_chain`] did.
//! - **The denominator is `4·knee + 1e-5`, not `4·knee`.** At the floored knee of
//!   `1e-4` that epsilon is 2.5% of the denominator, so it is a value, not a
//!   guard against a division that cannot happen.
//! - **The scale is not clamped to `0..=1`.** It cannot exceed one for a
//!   non-negative `c` — `max(soft, l-thr) ≤ l` throughout — so a clamp would be
//!   inert, and inert code that looks protective is worse than none.
//! - **`l - thr` is computed once and used twice.** GLSL's `l - thr + knee`
//!   groups left, so the subtraction inside the knee and the one inside the `max`
//!   are the same value; naming it is faithful, not a re-association.
//!
//! The knee floor `max( uParams.z, 1e-4 )` lives in the *caller* in the source
//! (the downsample's karis arm applies it once, before the thirteen calls), so it
//! is [`knee_floor`] here rather than a line inside [`prefilter`].

/// Rec.709 relative luminance — `owLum` from `glsl.js`.
///
/// Written out rather than expressed as a dot product on either side: a `dot`
/// builtin is free to factor its three products however it likes, and this
/// value feeds a reciprocal whose result weights every tap.
pub(crate) fn luminance(colour: [f32; 3]) -> f32 {
    colour[0] * 0.2126 + colour[1] * 0.7152 + colour[2] * 0.0722
}

/// `karisWeight` — the reciprocal-of-one-plus-luminance weight that stops one hot
/// pixel from pumping a whole mip.
///
/// A tap group at luminance 0 weighs `1.0`; at luminance 99 it weighs `0.01`. The
/// downsample divides by the weight sum, so this is a *luminance-weighted mean*,
/// which is the Karis average.
pub(crate) fn karis_weight(colour: [f32; 3]) -> f32 {
    1.0 / (1.0 + luminance(colour))
}

/// `max( uParams.z, 1e-4 )` — the knee the karis arm actually uses.
///
/// Separate from [`prefilter`] because the source applies it once per pass, not
/// once per tap, and because a caller that forgets it would divide by `1e-5`
/// instead of by `4·knee + 1e-5` and blow every highlight to a solid disc.
pub(crate) fn knee_floor(knee: f32) -> f32 {
    knee.max(1e-4)
}

/// `owBloomPrefilter` — a quadratic soft knee below the threshold, linear above.
///
/// `knee` is expected to have been through [`knee_floor`] already.
pub(crate) fn prefilter(colour: [f32; 3], threshold: f32, knee: f32) -> [f32; 3] {
    let level = colour[0].max(colour[1]).max(colour[2]);
    // GLSL `l - thr + knee` is `(l - thr) + knee`, so this subtraction is shared
    // with the `max` below by the language, not by an optimisation.
    let surplus = level - threshold;
    // GLSL `clamp(x, lo, hi)` is `min(max(x, lo), hi)`, written out because a
    // builtin is permitted to factor differently.
    let soft = (surplus + knee).max(0.0).min(2.0 * knee);
    let curved = soft * soft / (4.0 * knee + 1e-5);
    let scale = curved.max(surplus) / level.max(1e-4);
    [colour[0] * scale, colour[1] * scale, colour[2] * scale]
}

#[cfg(test)]
mod tests {
    use super::{karis_weight, knee_floor, luminance, prefilter};

    /// The tolerance every approximate comparison below uses: two `f32`
    /// expressions of the same value, on one machine, differ by at most a few
    /// ULP at these magnitudes.
    const EPS: f32 = 1e-6;

    /// `SOURCE_SETTINGS`' numbers, which every boundary case is stated in.
    const THR: f32 = 1.6;
    const KNEE: f32 = 0.9;

    /// The Rec.709 weights, in the order the source writes them. A transposed
    /// pair here would tint every bloom and nothing else would notice.
    #[test]
    fn luminance_is_rec709_channel_by_channel() {
        assert_eq!(luminance([1.0, 0.0, 0.0]), 0.2126);
        assert_eq!(luminance([0.0, 1.0, 0.0]), 0.7152);
        assert_eq!(luminance([0.0, 0.0, 1.0]), 0.0722);
        assert_eq!(luminance([0.0, 0.0, 0.0]), 0.0);
        // The three weights sum to one, so white is unity.
        assert!((luminance([1.0, 1.0, 1.0]) - 1.0).abs() <= EPS);
    }

    /// The Karis weight is a reciprocal, and it is what makes the average an
    /// average of *dimmer* things: a 100x brighter group weighs ~100x less.
    #[test]
    fn the_karis_weight_falls_as_the_reciprocal_of_brightness() {
        assert_eq!(karis_weight([0.0, 0.0, 0.0]), 1.0);
        assert!((karis_weight([1.0, 1.0, 1.0]) - 0.5).abs() <= EPS);
        let hot = karis_weight([99.0, 99.0, 99.0]);
        assert!((hot - 0.01).abs() <= EPS, "a 99x group must weigh ~1/100, got {hot}");
        // Monotone decreasing: a brighter group never outweighs a dimmer one.
        assert!(karis_weight([2.0, 0.0, 0.0]) < karis_weight([1.0, 0.0, 0.0]));
    }

    /// The floor is a floor, not a clamp: it raises a small or negative knee and
    /// leaves an authored one alone.
    #[test]
    fn the_knee_floor_only_raises() {
        assert_eq!(knee_floor(0.9), 0.9);
        assert_eq!(knee_floor(1e-4), 1e-4);
        assert_eq!(knee_floor(0.0), 1e-4);
        assert_eq!(knee_floor(-5.0), 1e-4);
        assert_eq!(knee_floor(1e-9), 1e-4);
    }

    /// **The lower boundary.** At and below `threshold - knee` nothing is
    /// admitted at all: the quadratic term is exactly zero there and the linear
    /// term is negative, so the `max` picks zero and the whole tap vanishes. This
    /// is the edge that decides whether a night sky glows.
    #[test]
    fn nothing_below_the_knee_start_is_admitted() {
        let start = THR - KNEE;
        assert_eq!(prefilter([start, start, start], THR, KNEE), [0.0, 0.0, 0.0]);
        assert_eq!(prefilter([0.0, 0.0, 0.0], THR, KNEE), [0.0, 0.0, 0.0]);
        assert_eq!(prefilter([0.3, 0.1, 0.2], THR, KNEE), [0.0, 0.0, 0.0]);
        // Just above it, something is admitted — so the zero above is a boundary
        // and not a dead function.
        let just_over = prefilter([start + 0.05, 0.0, 0.0], THR, KNEE);
        assert!(just_over[0] > 0.0, "the knee must admit light just above its start");
    }

    /// **The threshold itself.** The soft knee's whole purpose is that `l == thr`
    /// is *not* where the effect switches on — it is the midpoint of a quadratic
    /// ramp, admitting `knee²/(4·knee + 1e-5)` of the pixel. A hard cut here
    /// would draw a contour line across every gradient that crosses it.
    #[test]
    fn the_threshold_is_the_middle_of_the_ramp_not_its_start() {
        let at = prefilter([THR, THR, THR], THR, KNEE);
        let expected_scale = (KNEE * KNEE / (4.0 * KNEE + 1e-5)) / THR;
        assert!((at[0] / THR - expected_scale).abs() <= EPS);
        assert!(at[0] > 0.0, "the soft knee admits light at the threshold");
        assert!(at[0] < THR, "and not all of it");
    }

    /// **The upper boundary.** At `threshold + knee` the quadratic saturates and
    /// the linear arm takes over, and the two agree there to within the epsilon
    /// in the denominator — so the curve is continuous, which is why there is no
    /// visible ring at the knee's outer edge.
    #[test]
    fn the_quadratic_hands_over_to_the_linear_arm_continuously() {
        let end = THR + KNEE;
        let quadratic_at_end = (2.0 * KNEE) * (2.0 * KNEE) / (4.0 * KNEE + 1e-5);
        let linear_at_end = KNEE;
        assert!(
            (quadratic_at_end - linear_at_end).abs() < 1e-5,
            "the two arms must meet: {quadratic_at_end} vs {linear_at_end}"
        );
        // The linear arm is the larger of the two at the handover, so `max` picks
        // it — and keeps picking it for everything brighter.
        assert!(linear_at_end > quadratic_at_end);
        let out = prefilter([end, end, end], THR, KNEE);
        assert!((out[0] - end * (KNEE / end)).abs() <= EPS);
    }

    /// Far above the threshold the prefilter is asymptotically transparent: it
    /// subtracts the threshold and keeps the rest, which is what makes a bright
    /// light bloom in proportion to how bright it is rather than to a constant.
    #[test]
    fn a_bright_light_keeps_its_surplus() {
        let out = prefilter([100.0, 100.0, 100.0], THR, KNEE);
        let surplus = out[0];
        assert!((surplus - (100.0 - THR)).abs() <= 1e-3, "got {surplus}");
        let brighter = prefilter([1000.0, 1000.0, 1000.0], THR, KNEE);
        assert!(brighter[0] > out[0] * 9.0, "10x the light must bloom ~10x as hard");
    }

    /// **The divergence that matters most.** The source drives the prefilter with
    /// the MAX CHANNEL. A saturated tracer blooms; the same pixel judged on its
    /// Rec.709 luminance — which is what `post_chain`'s `contribution` did — does
    /// not bloom at all.
    #[test]
    fn a_saturated_red_blooms_because_the_driver_is_the_max_channel() {
        let tracer = [1.9, 0.0, 0.0];
        let out = prefilter(tracer, THR, KNEE);
        assert!(out[0] > 0.0, "a red tracer above the threshold must bloom");
        // What a luma-driven prefilter would have seen instead: well under the
        // knee start, so nothing at all.
        let luma = luminance(tracer);
        assert!(luma < THR - KNEE, "the luma {luma} is below the knee start");
        assert_eq!(prefilter([luma, luma, luma], THR, KNEE), [0.0, 0.0, 0.0]);
    }

    /// The prefilter scales the colour, it does not desaturate it: the ratio
    /// between channels is preserved exactly, so a bloomed orange muzzle flash
    /// stays orange instead of drifting toward white.
    #[test]
    fn the_prefilter_preserves_hue() {
        let flash = [3.0, 1.5, 0.5];
        let out = prefilter(flash, THR, KNEE);
        assert!((out[0] / out[1] - 2.0).abs() <= EPS);
        assert!((out[0] / out[2] - 6.0).abs() <= EPS);
    }

    /// The `1e-4` floor on `l` is what keeps a black tap from dividing by zero,
    /// and the numerator is zero there anyway — so the result is a clean zero
    /// rather than a NaN that would poison the weighted mean above it.
    #[test]
    fn a_black_tap_yields_zero_and_not_a_nan() {
        let out = prefilter([0.0, 0.0, 0.0], THR, KNEE);
        assert!(out.iter().all(|v| v.is_finite()));
        assert_eq!(out, [0.0, 0.0, 0.0]);
        // Even at the floored knee, where the denominator is at its smallest.
        let tiny = prefilter([0.0, 0.0, 0.0], THR, knee_floor(0.0));
        assert_eq!(tiny, [0.0, 0.0, 0.0]);
    }

    /// The `+ 1e-5` in the denominator is a *value*, not a guard. At the floored
    /// knee it moves the quadratic arm by 2.5%, which is exactly the regime the
    /// floor puts a zero-knee bloom into.
    #[test]
    fn the_denominator_epsilon_is_load_bearing_at_the_floored_knee() {
        let knee = knee_floor(0.0);
        let level = THR + knee * 0.5;
        let with_epsilon = prefilter([level, 0.0, 0.0], THR, knee)[0];
        let without = {
            let soft = (level - THR + knee).max(0.0).min(2.0 * knee);
            let curved = soft * soft / (4.0 * knee);
            level * (curved.max(level - THR) / level.max(1e-4))
        };
        let relative = ((with_epsilon - without) / without).abs();
        assert!(
            relative > 0.02,
            "dropping the epsilon at the floored knee must move the result by \
             more than 2%, moved {relative}"
        );
    }
}
