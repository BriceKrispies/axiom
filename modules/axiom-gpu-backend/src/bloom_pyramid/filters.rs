//! **The two filters, the blend, and the combine** — the arithmetic of
//! `bloom.js`'s `DOWNSAMPLE` and `UPSAMPLE`, and of `composite.js`'s one bloom
//! line.
//!
//! # Tap geometry
//!
//! Both filters offset from `vUv` in units of the **source's** texel, which is
//! what makes them scale-free: the downsample's source is the previous, larger
//! level and the upsample's source is the next, smaller one, so the same offset
//! table means a different distance at each level, which is the pyramid.
//!
//! ```glsl
//! // DOWNSAMPLE — 13 taps, letters as the source names them.
//! //   a( -2, +2 )  b( 0, +2 )  c( +2, +2 )
//! //   d( -2,  0 )  e( 0,  0 )  f( +2,  0 )
//! //   g( -2, -2 )  h( 0, -2 )  i( +2, -2 )
//! //          j( -1, +1 )  k( +1, +1 )
//! //          l( -1, -1 )  m( +1, -1 )
//! // UPSAMPLE — 9 taps at ±uRadius texels.
//! //   a( -1, +1 )  b( 0, +1 )  c( +1, +1 )
//! //   d( -1,  0 )  e( 0,  0 )  f( +1,  0 )
//! //   g( -1, -1 )  h( 0, -1 )  i( +1, -1 )
//! ```
//!
//! At half-resolution the ±1 and ±2 offsets land on source texel *corners*, so
//! the hardware's bilinear filter turns each of the thirteen into a 2x2 box
//! average — thirteen fetches covering thirty-six texels. That is the whole trick
//! of the Jimenez filter and it is why the tap positions are not negotiable.
//!
//! # The two downsample arms are different algorithms, not a parameter
//!
//! Level 0 (`uParams.x > 0.5`) meters, thresholds, and Karis-averages. Every
//! level below it runs a plain fixed-weight 13-tap. They share only the taps:
//!
//! ```glsl
//! // KARIS ARM — after `*= ex` and `owBloomPrefilter` on all thirteen:
//! vec3 g0 = ( a + b + d + e ) * 0.25;   float w0 = karisWeight( g0 ) * 0.125;
//! vec3 g1 = ( b + c + e + f ) * 0.25;   float w1 = karisWeight( g1 ) * 0.125;
//! vec3 g2 = ( d + e + g + h ) * 0.25;   float w2 = karisWeight( g2 ) * 0.125;
//! vec3 g3 = ( e + f + h + i ) * 0.25;   float w3 = karisWeight( g3 ) * 0.125;
//! vec3 g4 = ( j + k + l + m ) * 0.25;   float w4 = karisWeight( g4 ) * 0.5;
//! result = ( g0*w0 + g1*w1 + g2*w2 + g3*w3 + g4*w4 ) /
//!          max( w0 + w1 + w2 + w3 + w4, 1e-5 );
//! result = min( result, vec3( 24.0 ) );
//!
//! // PLAIN ARM:
//! result  = e * 0.125;
//! result += ( a + c + g + i ) * 0.03125;
//! result += ( b + d + f + h ) * 0.0625;
//! result += ( j + k + l + m ) * 0.125;
//! ```
//!
//! The `0.125/0.125/0.125/0.5` group weights are the plain arm's weights
//! regrouped: the four inner taps carry half the filter between them, the four
//! overlapping outer quads carry an eighth each. What the Karis average adds is
//! the *renormalisation* — dividing by the weight sum, which no longer sums to
//! one once each group is scaled by its own brightness.
//!
//! `min(24)` is the firefly clamp. It applies to the karis arm only, and it is
//! also what keeps the half-float mips clear of their `65504` ceiling.
//!
//! # The division is a division
//!
//! `/ max(w0+w1+w2+w3+w4, 1e-5)` is written as three per-channel divisions here
//! and as a `vec3 / f32` in the WGSL. It is **not** a reciprocal computed once
//! and multiplied three times: five of the ten defects this port found in its
//! `sky/` slices were exactly that rewrite, and the resulting error is a
//! sub-ULP-per-channel drift that shows up only as a slow hue rotation in the
//! brightest part of the frame.
//!
//! # Accumulation is a blend, not a sum
//!
//! The upsample writes `vec4( sum * 0.0625, uWeight )` through `NormalBlending`
//! with `premultipliedAlpha = false`, i.e. `src·α + dst·(1-α)`. Adding the mips
//! outright — what most WebGL bloom does — multiplies the pyramid's total energy
//! by the level count and turns the frame into haze; blending keeps it energy
//! preserving, which is what lets `composite.js` treat `bloomStrength` as a true
//! veiling-glare percentage.

use crate::bloom_pyramid::prefilter::{karis_weight, knee_floor, prefilter};

/// The 13 downsample offsets, in **source texels**, in the source's `a`..`m`
/// order. Indexed as `a=0 b=1 c=2 d=3 e=4 f=5 g=6 h=7 i=8 j=9 k=10 l=11 m=12`.
///
/// The order is not cosmetic: [`downsample_plain`]'s three groups and
/// [`downsample_karis`]'s five are written against these indices, and the WGSL
/// declares the same table — [`crate::bloom_pyramid::parity`] renders it back and
/// compares, so the two cannot drift.
pub(crate) const DOWN_TAPS: [[f32; 2]; 13] = [
    [-2.0, 2.0],
    [0.0, 2.0],
    [2.0, 2.0],
    [-2.0, 0.0],
    [0.0, 0.0],
    [2.0, 0.0],
    [-2.0, -2.0],
    [0.0, -2.0],
    [2.0, -2.0],
    [-1.0, 1.0],
    [1.0, 1.0],
    [-1.0, -1.0],
    [1.0, -1.0],
];

/// The 9 tent-upsample offsets, in **source texels scaled by `uRadius`**, in the
/// source's `a`..`i` order — row-major from the top-left, so `e` (the centre) is
/// index 4.
pub(crate) const UP_TAPS: [[f32; 2]; 9] = [
    [-1.0, 1.0],
    [0.0, 1.0],
    [1.0, 1.0],
    [-1.0, 0.0],
    [0.0, 0.0],
    [1.0, 0.0],
    [-1.0, -1.0],
    [0.0, -1.0],
    [1.0, -1.0],
];

/// The firefly clamp the karis arm ends with: `min( result, vec3( 24.0 ) )`.
pub(crate) const FIREFLY_CLAMP: f32 = 24.0;

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], by: f32) -> [f32; 3] {
    [a[0] * by, a[1] * by, a[2] * by]
}

/// Three genuine divisions. See the module header for why this is not a
/// reciprocal multiply.
fn divide(a: [f32; 3], by: f32) -> [f32; 3] {
    [a[0] / by, a[1] / by, a[2] / by]
}

/// `fetch`'s `max( …, vec3( 0.0 ) )`, componentwise.
///
/// Applied inside both downsample arms rather than at the sample site so that
/// one function is the whole definition of what an arm does to its taps — the
/// WGSL does the same, for the same reason.
fn floor_at_zero(a: [f32; 3]) -> [f32; 3] {
    [a[0].max(0.0), a[1].max(0.0), a[2].max(0.0)]
}

/// `( p + q + r + s ) * 0.25`, left-associated exactly as GLSL groups it.
fn quad_mean(p: [f32; 3], q: [f32; 3], r: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    scale(add(add(add(p, q), r), s), 0.25)
}

/// `( p + q + r + s )`, left-associated.
fn quad_sum(p: [f32; 3], q: [f32; 3], r: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    add(add(add(p, q), r), s)
}

/// **The level-0 downsample**: exposure, soft-knee threshold, Karis average,
/// firefly clamp.
pub(crate) fn downsample_karis(
    taps: [[f32; 3]; 13],
    exposure: f32,
    threshold: f32,
    knee: f32,
) -> [f32; 3] {
    let knee = knee_floor(knee);
    // Exposure first, so the firefly clamp AND the threshold are both in
    // display-referred terms — a fixed linear threshold on unscaled radiance
    // would mean something different at every time of day.
    let t = taps.map(|c| prefilter(scale(floor_at_zero(c), exposure), threshold, knee));
    let g0 = quad_mean(t[0], t[1], t[3], t[4]);
    let g1 = quad_mean(t[1], t[2], t[4], t[5]);
    let g2 = quad_mean(t[3], t[4], t[6], t[7]);
    let g3 = quad_mean(t[4], t[5], t[7], t[8]);
    let g4 = quad_mean(t[9], t[10], t[11], t[12]);
    let w0 = karis_weight(g0) * 0.125;
    let w1 = karis_weight(g1) * 0.125;
    let w2 = karis_weight(g2) * 0.125;
    let w3 = karis_weight(g3) * 0.125;
    let w4 = karis_weight(g4) * 0.5;
    let weighted = add(
        add(add(add(scale(g0, w0), scale(g1, w1)), scale(g2, w2)), scale(g3, w3)),
        scale(g4, w4),
    );
    let result = divide(weighted, (w0 + w1 + w2 + w3 + w4).max(1e-5));
    [
        result[0].min(FIREFLY_CLAMP),
        result[1].min(FIREFLY_CLAMP),
        result[2].min(FIREFLY_CLAMP),
    ]
}

/// **Every downsample below level 0**: a fixed-weight 13-tap, no exposure, no
/// threshold, no clamp.
pub(crate) fn downsample_plain(taps: [[f32; 3]; 13]) -> [f32; 3] {
    let t = taps.map(floor_at_zero);
    let centre = scale(t[4], 0.125);
    let corners = scale(quad_sum(t[0], t[2], t[6], t[8]), 0.03125);
    let edges = scale(quad_sum(t[1], t[3], t[5], t[7]), 0.0625);
    let inner = scale(quad_sum(t[9], t[10], t[11], t[12]), 0.125);
    add(add(add(centre, corners), edges), inner)
}

/// **The 9-tap tent upsample**: `( e·4 + (b+d+f+h)·2 + (a+c+g+i) ) · 0.0625`.
///
/// No `max( …, 0 )` here — the upsample's source is a downsample's output, which
/// is already non-negative, and the source does not re-floor it.
pub(crate) fn upsample_tent(taps: [[f32; 3]; 9]) -> [f32; 3] {
    let sum = add(
        add(
            scale(taps[4], 4.0),
            scale(quad_sum(taps[1], taps[3], taps[5], taps[7]), 2.0),
        ),
        quad_sum(taps[0], taps[2], taps[6], taps[8]),
    );
    scale(sum, 0.0625)
}

/// `NormalBlending` with `premultipliedAlpha = false`: `src·α + dst·(1-α)`.
///
/// The fixed-function blender computes exactly this, in this order, from
/// `SrcAlpha`/`OneMinusSrcAlpha` — so the reference computes it in that order
/// too rather than as the algebraically equal `dst + (src-dst)·α`.
pub(crate) fn blend(destination: [f32; 3], source: [f32; 3], weight: f32) -> [f32; 3] {
    add(scale(source, weight), scale(destination, 1.0 - weight))
}

/// `composite.js`'s one bloom line, and the end of this module's responsibility:
///
/// ```glsl
/// vec3 bloom = max( texture2D( tBloom, vUv ).rgb, vec3( 0.0 ) );
/// hdr += bloom * max( uGrade.x, 0.0 );
/// ```
///
/// An **add into HDR, before the tone map**. What happens after — the cos⁴ lens
/// shading, AgX, the LUT grade — is a sibling slice's, and this function
/// deliberately stops short of it.
///
/// # `hdr` is ALREADY exposure-scaled when it gets here
///
/// The line immediately above in `composite.js` is `hdr *= exposure;`. The bloom
/// is exposure-scaled too, but by its **own** prefilter thirteen taps at a time
/// (see [`downsample_karis`]), which is why the source's comment says "already
/// exposure-scaled AND thresholded in the prefilter" and why the add is a plain
/// add rather than a second scale. Whoever wires this into the frame graph must
/// therefore apply the metered exposure to the scene *before* calling this, and
/// must **not** apply it again to the bloom. Applying it twice to the bloom
/// squares the metering; applying it to neither makes the threshold mean
/// something different at every time of day, which is the exact failure the
/// source's ordering exists to avoid.
pub(crate) fn combine(hdr: [f32; 3], bloom: [f32; 3], strength: f32) -> [f32; 3] {
    add(hdr, scale(floor_at_zero(bloom), strength.max(0.0)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        blend, combine, downsample_karis, downsample_plain, upsample_tent, DOWN_TAPS,
        FIREFLY_CLAMP, UP_TAPS,
    };
    use crate::bloom_pyramid::prefilter::{karis_weight, prefilter};
    use crate::bloom_pyramid::SOURCE_SETTINGS;

    const EPS: f32 = 1e-6;

    /// A deterministic, hue-varied tap set: no two taps equal, so a transposed
    /// index cannot pass, and channels differ so a swapped lane cannot either.
    pub(crate) fn taps13(seed: f32) -> [[f32; 3]; 13] {
        let mut index = 0.0_f32;
        [[0.0_f32; 3]; 13].map(|_| {
            index += 1.0;
            let base = seed + index;
            [base * 0.37, base * 0.11 + 0.05, base * 0.73 - 0.02]
        })
    }

    /// The same, nine wide, for the tent.
    pub(crate) fn taps9(seed: f32) -> [[f32; 3]; 9] {
        let mut index = 0.0_f32;
        [[0.0_f32; 3]; 9].map(|_| {
            index += 1.0;
            let base = seed + index;
            [base * 0.19 + 0.03, base * 0.61, base * 0.29 - 0.01]
        })
    }

    /// The tap tables, letter by letter, against the GLSL. A single transposed
    /// pair here is a silent one-texel shear in the whole pyramid.
    #[test]
    fn the_tap_tables_are_the_sources_letters() {
        assert_eq!(DOWN_TAPS.len(), 13);
        assert_eq!(UP_TAPS.len(), 9);
        // The outer ring is at ±2, the inner quad at ±1, the centre at zero.
        assert_eq!(DOWN_TAPS[4], [0.0, 0.0]);
        assert_eq!(DOWN_TAPS[0], [-2.0, 2.0]);
        assert_eq!(DOWN_TAPS[8], [2.0, -2.0]);
        assert_eq!(DOWN_TAPS[9], [-1.0, 1.0]);
        assert_eq!(DOWN_TAPS[12], [1.0, -1.0]);
        // The nine outer taps use only ±2 or 0; the four inner ones only ±1.
        assert!(DOWN_TAPS[..9].iter().flatten().all(|v| v.abs() == 2.0 || *v == 0.0));
        assert!(DOWN_TAPS[9..].iter().flatten().all(|v| v.abs() == 1.0));
        // The tent is a 3x3 at ±1, centred.
        assert_eq!(UP_TAPS[4], [0.0, 0.0]);
        assert!(UP_TAPS.iter().flatten().all(|v| v.abs() <= 1.0));
        // Each table is antisymmetric about its own centre — the outer 3x3
        // (indices 0..9) about `e`, the inner quad (9..13) about the same point,
        // and the tent about `e`. That is what makes the filters phase
        // preserving; a single mistyped sign would shear the whole pyramid.
        assert!((0..9).all(|n| DOWN_TAPS[n] == [-DOWN_TAPS[8 - n][0], -DOWN_TAPS[8 - n][1]]));
        assert!((9..13).all(|n| DOWN_TAPS[n] == [-DOWN_TAPS[21 - n][0], -DOWN_TAPS[21 - n][1]]));
        assert!((0..9).all(|n| UP_TAPS[n] == [-UP_TAPS[8 - n][0], -UP_TAPS[8 - n][1]]));
    }

    /// The plain arm's weights sum to one, so a flat field survives a downsample
    /// unchanged. That is the property that keeps a pyramid from darkening or
    /// brightening the frame just by existing.
    #[test]
    fn the_plain_downsample_preserves_a_flat_field() {
        let flat = [[0.4_f32, 0.7, 1.3]; 13];
        let out = downsample_plain(flat);
        assert!((out[0] - 0.4).abs() <= EPS);
        assert!((out[1] - 0.7).abs() <= EPS);
        assert!((out[2] - 1.3).abs() <= EPS);
        // Stated as the weights themselves: 0.125 + 4·0.03125 + 4·0.0625 + 4·0.125.
        let total = 0.125 + 4.0 * 0.03125 + 4.0 * 0.0625 + 4.0 * 0.125;
        assert_eq!(total, 1.0);
    }

    /// The plain arm's weight *distribution*, not just its sum: the four inner
    /// taps carry half the filter, the centre an eighth, the four edges a
    /// quarter, the four corners an eighth. Driving one tap at a time is what
    /// separates the four groups.
    #[test]
    fn the_plain_downsample_weights_each_group_as_the_source_does() {
        let one_at = |which: usize| {
            let mut taps = [[0.0_f32; 3]; 13];
            taps[which] = [1.0, 1.0, 1.0];
            downsample_plain(taps)[0]
        };
        assert_eq!(one_at(4), 0.125, "the centre tap e");
        [0_usize, 2, 6, 8]
            .into_iter()
            .for_each(|n| assert_eq!(one_at(n), 0.03125, "corner tap {n}"));
        [1_usize, 3, 5, 7]
            .into_iter()
            .for_each(|n| assert_eq!(one_at(n), 0.0625, "edge tap {n}"));
        [9_usize, 10, 11, 12]
            .into_iter()
            .for_each(|n| assert_eq!(one_at(n), 0.125, "inner tap {n}"));
    }

    /// Negative taps are floored, not carried: `fetch` maxes with zero, so a
    /// negative source value contributes nothing rather than subtracting.
    #[test]
    fn both_downsample_arms_floor_a_negative_tap() {
        let mut taps = [[0.0_f32; 3]; 13];
        taps[4] = [-8.0, -8.0, -8.0];
        assert_eq!(downsample_plain(taps), [0.0, 0.0, 0.0]);
        let karis = downsample_karis(taps, 1.0, 1.6, 0.9);
        assert_eq!(karis, [0.0, 0.0, 0.0]);
    }

    /// The karis arm's renormalisation: with every group equally bright the
    /// weights cancel and the arm reduces to the plain group mean, so a flat
    /// field above the threshold passes through at its prefiltered value.
    #[test]
    fn the_karis_average_renormalises_to_a_plain_mean_on_a_flat_field() {
        let level = [3.0_f32, 3.0, 3.0];
        let out = downsample_karis([level; 13], 1.0, 1.6, 0.9);
        let expected = prefilter(level, 1.6, 0.9);
        (0..3).for_each(|lane| {
            // Bound before the assertion, not formatted inside it: a format
            // argument is only evaluated when the assertion FAILS, so an
            // expression left in the message is a region no passing test reaches.
            let (got, want) = (out[lane], expected[lane]);
            assert!((got - want).abs() <= EPS, "lane {lane}: {got} vs {want}");
        });
    }

    /// **The firefly guard, stated as the thing it prevents.** One tap a thousand
    /// times brighter than its neighbours: the plain arm passes an eighth of it
    /// straight through, the Karis arm weighs that group down by its own
    /// luminance and admits far less.
    #[test]
    fn one_hot_pixel_cannot_pump_a_whole_mip() {
        let mut taps = [[2.0_f32; 3]; 13];
        taps[9] = [2000.0, 2000.0, 2000.0];
        let plain = downsample_plain(taps)[0];
        let karis = downsample_karis(taps, 1.0, 1.6, 0.9)[0];
        assert!(plain > 200.0, "the plain arm passes the firefly: {plain}");
        assert!(
            karis < plain * 0.2,
            "the Karis average must suppress the firefly: {karis} vs {plain}"
        );
        // And it is clamped on top of that.
        assert!(karis <= FIREFLY_CLAMP);
    }

    /// The clamp is a real ceiling and it is exactly 24: a uniformly enormous
    /// field comes out at the clamp rather than at its own value.
    #[test]
    fn the_karis_arm_clamps_at_twenty_four() {
        let out = downsample_karis([[500.0_f32; 3]; 13], 1.0, 1.6, 0.9);
        assert_eq!(out, [FIREFLY_CLAMP; 3]);
        // Just under it, nothing is clamped — so the ceiling is a boundary.
        let under = downsample_karis([[20.0_f32; 3]; 13], 1.0, 1.6, 0.9);
        assert!(under.iter().all(|v| *v < FIREFLY_CLAMP));
    }

    /// Exposure runs before the threshold, so metering *decides* what blooms. The
    /// same radiance is below the knee at one exposure and above it at another —
    /// which is exactly the source's stated reason for the ordering.
    #[test]
    fn exposure_is_applied_before_the_threshold() {
        let taps = [[1.0_f32; 3]; 13];
        let dark = downsample_karis(taps, 0.5, 1.6, 0.9);
        let bright = downsample_karis(taps, 4.0, 1.6, 0.9);
        assert_eq!(dark, [0.0, 0.0, 0.0], "at 0.5x, 1.0 is below the knee start");
        assert!(bright[0] > 0.0, "at 4x it is well above the threshold");
        // A scale applied *after* the threshold would have bloomed the dark
        // frame too: unscaled, 1.0 sits inside the knee and is admitted, and
        // halving that admitted value afterwards is not the same as never
        // admitting it.
        let admitted_unscaled = prefilter([1.0, 1.0, 1.0], 1.6, 0.9)[0];
        assert!(admitted_unscaled > 0.0, "1.0 is inside the knee, so it is admitted");
        assert!(admitted_unscaled * 0.5 > 0.0, "post-scaling would have kept it");
    }

    /// The weight sum's `max( …, 1e-5 )` cannot be provoked by a real frame —
    /// the smallest Karis weight is `1/(1+L)`, which for any finite `L` keeps the
    /// sum well above `1e-5` — but a group luminance of `1e6` drives it to the
    /// floor's neighbourhood, and the result must stay finite there.
    #[test]
    fn an_enormous_group_luminance_keeps_the_weight_sum_finite() {
        let out = downsample_karis([[1.0e30_f32; 3]; 13], 1.0, 1.6, 0.9);
        assert!(out.iter().all(|v| v.is_finite()), "got {out:?}");
        assert_eq!(out, [FIREFLY_CLAMP; 3]);
    }

    /// The tent's weights are `1 2 1 / 2 4 2 / 1 2 1` over sixteen — the kernel
    /// that makes the upsample a tent and not a box. Driving one tap at a time
    /// reads them straight off.
    #[test]
    fn the_tent_kernel_is_one_two_four_over_sixteen() {
        let one_at = |which: usize| {
            let mut taps = [[0.0_f32; 3]; 9];
            taps[which] = [1.0, 1.0, 1.0];
            upsample_tent(taps)[0]
        };
        assert_eq!(one_at(4), 0.25);
        [1_usize, 3, 5, 7]
            .into_iter()
            .for_each(|n| assert_eq!(one_at(n), 0.125, "edge tap {n}"));
        [0_usize, 2, 6, 8]
            .into_iter()
            .for_each(|n| assert_eq!(one_at(n), 0.0625, "corner tap {n}"));
        // And they sum to one: a flat field survives the tent unchanged.
        let flat = upsample_tent([[0.6_f32, 1.1, 2.2]; 9]);
        assert!((flat[0] - 0.6).abs() <= EPS);
        assert!((flat[1] - 1.1).abs() <= EPS);
        assert!((flat[2] - 2.2).abs() <= EPS);
    }

    /// The blend is a lerp, not a sum, and its two endpoints are exact.
    #[test]
    fn the_blend_is_a_lerp_with_exact_endpoints() {
        let dst = [1.0_f32, 2.0, 3.0];
        let src = [5.0_f32, 6.0, 7.0];
        assert_eq!(blend(dst, src, 0.0), dst);
        assert_eq!(blend(dst, src, 1.0), src);
        let half = blend(dst, src, 0.5);
        assert_eq!(half, [3.0, 4.0, 5.0]);
        // At the source's wide-level weight it is 34% of the way across.
        let wide = blend(dst, src, 0.34);
        assert!((wide[0] - (5.0 * 0.34 + 1.0 * 0.66)).abs() <= EPS);
    }

    /// **Energy preservation.** Blending a level in at 0.5 leaves a flat pyramid
    /// flat; summing it would double it. Six blended levels of an equal field are
    /// still that field — the property that lets `bloomStrength` mean a
    /// percentage.
    #[test]
    fn blending_the_pyramid_preserves_energy_where_summing_would_not() {
        let field = [1.0_f32, 1.0, 1.0];
        let blended = (0..6).fold(field, |acc, _| blend(acc, field, 0.5));
        assert_eq!(blended, field);
        let summed = (0..6).fold(field, |acc, _| super::add(acc, field));
        assert_eq!(summed[0], 7.0, "summing multiplies the energy by the level count");
    }

    /// **A zero-strength bloom is bit-identical to no bloom.** Not "close to" —
    /// the same bits, for every HDR value in the table and for an arbitrarily
    /// violent bloom.
    #[test]
    fn a_zero_strength_bloom_is_bit_identical_to_no_bloom() {
        let bloom = [1.0e6_f32, 42.0, 7.5];
        let scene: Vec<[f32; 3]> = (0..32)
            .map(|n| {
                let v = n as f32;
                [v * 0.031, v * 1.7 + 0.004, v * v * 0.5]
            })
            .collect();
        scene.iter().for_each(|hdr| {
            let out = combine(*hdr, bloom, 0.0);
            (0..3).for_each(|lane| {
                let (got, want) = (out[lane], hdr[lane]);
                assert_eq!(
                    got.to_bits(),
                    want.to_bits(),
                    "lane {lane} moved at zero strength: {got} vs {want}"
                );
            });
        });
        // A negative strength is floored to zero, so it is the same no-op — this
        // is `max( uGrade.x, 0.0 )`, not an accident of sign.
        let negative = combine(scene[7], bloom, -3.0);
        (0..3).for_each(|lane| assert_eq!(negative[lane].to_bits(), scene[7][lane].to_bits()));
        // And the strength is not inert: any positive value moves the frame, so
        // the identity above is a disable rather than a dead function.
        assert_ne!(combine(scene[7], bloom, 1e-6), scene[7]);
    }

    /// The one value for which "bit-identical" is not literally true, stated
    /// rather than hidden: `-0.0 + 0.0` is `+0.0`, so a negative-zero scene
    /// channel normalises. GLSL's `hdr += …` does exactly the same, so this is
    /// the source's behaviour and not a divergence.
    #[test]
    fn negative_zero_normalises_exactly_as_the_glsl_does() {
        let out = combine([-0.0, -0.0, -0.0], [0.0, 0.0, 0.0], 0.0);
        assert_eq!(out, [0.0, 0.0, 0.0]);
        assert_eq!(out[0].to_bits(), 0.0_f32.to_bits());
        assert_ne!(out[0].to_bits(), (-0.0_f32).to_bits());
    }

    /// The combine floors a negative bloom sample and scales by the strength.
    #[test]
    fn the_combine_floors_the_bloom_and_scales_it() {
        let out = combine([1.0, 1.0, 1.0], [-4.0, 2.0, 0.0], 0.14);
        assert_eq!(out[0], 1.0);
        assert!((out[1] - (1.0 + 2.0 * 0.14)).abs() <= EPS);
        assert_eq!(out[2], 1.0);
        // The authored strength is the source's 0.14.
        assert_eq!(SOURCE_SETTINGS.strength, 0.14);
    }

    /// The two arms are genuinely different functions of the same taps, at every
    /// seed — so `uParams.x` selects an algorithm rather than tweaking one.
    #[test]
    fn the_two_downsample_arms_disagree_on_real_taps() {
        (0..8).for_each(|n| {
            let taps = taps13(n as f32);
            let plain = downsample_plain(taps);
            let karis = downsample_karis(taps, 1.0, 1.6, 0.9);
            assert_ne!(plain, karis, "the arms coincided at seed {n}");
        });
    }

    /// The Karis weight is what the arm actually divides by: recomputing the
    /// weighted mean by hand from the five groups reproduces the arm exactly,
    /// which pins the group membership (`a b d e`, `b c e f`, …) rather than just
    /// its output.
    #[test]
    fn the_five_karis_groups_are_the_sources_quads() {
        let taps = taps13(2.0);
        let t = taps.map(|c| prefilter(c, 1.6, 0.9));
        let mean = |p: [f32; 3], q: [f32; 3], r: [f32; 3], s: [f32; 3]| {
            [0, 1, 2].map(|l| (((p[l] + q[l]) + r[l]) + s[l]) * 0.25)
        };
        let groups = [
            mean(t[0], t[1], t[3], t[4]),
            mean(t[1], t[2], t[4], t[5]),
            mean(t[3], t[4], t[6], t[7]),
            mean(t[4], t[5], t[7], t[8]),
            mean(t[9], t[10], t[11], t[12]),
        ];
        let weights = [0.125, 0.125, 0.125, 0.125, 0.5]
            .iter()
            .zip(groups.iter())
            .map(|(share, group)| karis_weight(*group) * share)
            .collect::<Vec<f32>>();
        let total: f32 = weights[0] + weights[1] + weights[2] + weights[3] + weights[4];
        let expected = [0, 1, 2].map(|l| {
            let n = ((((groups[0][l] * weights[0]) + groups[1][l] * weights[1])
                + groups[2][l] * weights[2])
                + groups[3][l] * weights[3])
                + groups[4][l] * weights[4];
            (n / total.max(1e-5)).min(FIREFLY_CLAMP)
        });
        let actual = downsample_karis(taps, 1.0, 1.6, 0.9);
        (0..3).for_each(|l| assert_eq!(actual[l].to_bits(), expected[l].to_bits(), "lane {l}"));
    }

    /// The tent's own sample table is exercised, so the helper the parity module
    /// shares is covered here too.
    #[test]
    fn the_shared_sample_tables_are_hue_varied_and_distinct() {
        let down = taps13(0.5);
        assert!((0..13).all(|n| (n + 1..13).all(|m| down[n] != down[m])));
        let up = taps9(0.5);
        assert!((0..9).all(|n| (n + 1..9).all(|m| up[n] != up[m])));
        assert!(upsample_tent(up).iter().all(|v| v.is_finite()));
    }
}
