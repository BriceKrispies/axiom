//! **The pyramid's shape**: how many levels, how big each one is, and what the
//! upsample does at each of them.
//!
//! Two functions, both straight transcriptions.
//!
//! ```js
//! setSize(w, h) {
//!   let mw = w, mh = h;
//!   for (let i = 0; i < this.levels; i++) {
//!     mw = Math.max(1, Math.floor(mw / 2));
//!     mh = Math.max(1, Math.floor(mh / 2));
//!     this.mips.push({ rt: hdrTarget(mw, mh), w: mw, h: mh });
//!     if (mw <= 2 || mh <= 2) break;                       // AFTER the push
//!   }
//! }
//! ```
//!
//! The `break` is **after** the push, so the level that first reaches two texels
//! in either axis is *kept* and is the last. A `while` guard written before the
//! push would drop it, costing the pyramid its widest, softest level — the one
//! that carries the low-frequency glare.
//!
//! ```js
//! for (let i = n - 1; i > 0; i--) {
//!   const wide = i >= n - 2;
//!   uu.uRadius.value = wide ? 0.62 : 1.0;
//!   uu.uWeight.value = wide ? 0.34 : 0.5;
//!   this.up.render(renderer, this.mips[i - 1].rt);
//! }
//! ```
//!
//! The upsample walks **down** the index, from the smallest mip into its larger
//! neighbour, and the top two levels are tightened. The source's own reason: a
//! tent at radius 1 on a 1/64-resolution mip is a thirty-pixel halo, and that
//! halo is what dissolves a roofline against a bright sky. Narrowing the widest
//! two keeps the low-frequency component of the glare and takes away its reach.

/// `new Bloom( this.qLevel >= 2 ? 6 : 5 )` — the high tier's level count.
pub(crate) const LEVELS_HIGH: usize = 6;

/// The low tier's. One fewer level is one fewer octave of glare, not a fifth
/// less of it.
pub(crate) const LEVELS_LOW: usize = 5;

/// `uRadius`/`uWeight` on every level except the widest two.
pub(crate) const NARROW_STEP: (f32, f32) = (1.0, 0.5);

/// `uRadius`/`uWeight` on the widest two levels.
pub(crate) const WIDE_STEP: (f32, f32) = (0.62, 0.34);

/// `setSize` — the mip dimensions, largest first, at most `levels` of them.
///
/// Empty when `levels` is zero, which is the `n === 0 → return null` arm of
/// `render`: no pyramid, no bloom texture, and the composite falls back to a
/// strength of zero.
pub(crate) fn mip_sizes(width: u32, height: u32, levels: usize) -> Vec<(u32, u32)> {
    let halved: Vec<(u32, u32)> = (0..levels)
        .scan((width, height), |size, _| {
            *size = ((size.0 / 2).max(1), (size.1 / 2).max(1));
            Some(*size)
        })
        .collect();
    // `position` finds the first level that hit the floor; `+ 1` is the source's
    // break-after-push, keeping that level and dropping everything below it.
    let stop = halved
        .iter()
        .position(|&(w, h)| (w <= 2) | (h <= 2))
        .map_or(halved.len(), |first| first + 1);
    halved.into_iter().take(stop).collect()
}

/// The `(uRadius, uWeight)` the upsample *into* `level - 1` runs with, for a
/// pyramid of `count` levels.
///
/// `wide = i >= n - 2`, restated as `i + 2 >= n` so it cannot underflow for a
/// one-level pyramid (where the upsample loop does not run at all, and this is
/// never asked).
pub(crate) fn upsample_step(level: usize, count: usize) -> (f32, f32) {
    let wide = level + 2 >= count;
    [NARROW_STEP, WIDE_STEP][usize::from(wide)]
}

#[cfg(test)]
mod tests {
    use super::{mip_sizes, upsample_step, LEVELS_HIGH, LEVELS_LOW, NARROW_STEP, WIDE_STEP};

    /// A 1920x1080 frame at the high tier: six levels, each a floored half of the
    /// one above, none of them hitting the two-texel floor.
    #[test]
    fn a_full_hd_frame_gets_all_six_levels() {
        let sizes = mip_sizes(1920, 1080, LEVELS_HIGH);
        assert_eq!(
            sizes,
            vec![(960, 540), (480, 270), (240, 135), (120, 67), (60, 33), (30, 16)]
        );
        // The odd dimensions floor rather than round: 135 / 2 is 67, not 68.
        assert_eq!(sizes[3].1, 67);
        assert_eq!(sizes.len(), LEVELS_HIGH);
        // And the low tier is the same chain, one level shorter.
        assert_eq!(mip_sizes(1920, 1080, LEVELS_LOW), sizes[..LEVELS_LOW].to_vec());
    }

    /// **The break is after the push.** A frame narrow enough to reach two texels
    /// keeps that level as its last, rather than stopping before it.
    #[test]
    fn the_level_that_reaches_two_texels_is_kept_and_is_the_last() {
        // 64x8 halves to 32x4 then 16x2 — the second level hits the floor.
        let sizes = mip_sizes(64, 8, 6);
        assert_eq!(sizes, vec![(32, 4), (16, 2)]);
        // One more halving would have been legal by the level budget; the break
        // is what stopped it.
        assert!(sizes.len() < 6);
        // Exactly at the boundary: a level of three texels does not stop it.
        assert_eq!(mip_sizes(64, 12, 6), vec![(32, 6), (16, 3), (8, 1)]);
    }

    /// The `max(1, …)` floor: a one-texel axis stays at one rather than becoming
    /// zero, which would be a zero-area render target.
    #[test]
    fn an_axis_never_floors_below_one_texel() {
        assert_eq!(mip_sizes(1, 1, 6), vec![(1, 1)]);
        assert_eq!(mip_sizes(3, 3, 6), vec![(1, 1)]);
        assert_eq!(mip_sizes(5, 5, 6), vec![(2, 2)]);
        assert!(mip_sizes(1, 1024, 6).iter().all(|(w, _)| *w == 1));
    }

    /// Zero levels is the `n === 0` arm: no pyramid at all.
    #[test]
    fn zero_levels_is_an_empty_pyramid() {
        assert!(mip_sizes(1920, 1080, 0).is_empty());
        assert!(mip_sizes(0, 0, 0).is_empty());
        // A zero-sized frame still floors to one texel rather than vanishing —
        // the level budget is what decides emptiness, not the size.
        assert_eq!(mip_sizes(0, 0, 3), vec![(1, 1)]);
    }

    /// The schedule: the widest two levels are tightened, everything below them
    /// runs the full tent at a half blend.
    #[test]
    fn only_the_widest_two_levels_are_tightened() {
        let count = 6;
        assert_eq!(upsample_step(5, count), WIDE_STEP);
        assert_eq!(upsample_step(4, count), WIDE_STEP);
        assert_eq!(upsample_step(3, count), NARROW_STEP);
        assert_eq!(upsample_step(2, count), NARROW_STEP);
        assert_eq!(upsample_step(1, count), NARROW_STEP);
        // The wide step is genuinely narrower AND lighter, not one or the other.
        assert!(WIDE_STEP.0 < NARROW_STEP.0);
        assert!(WIDE_STEP.1 < NARROW_STEP.1);
        assert_eq!(WIDE_STEP, (0.62, 0.34));
        assert_eq!(NARROW_STEP, (1.0, 0.5));
    }

    /// A two-level pyramid is *all* wide levels — `i >= n - 2` with `n = 2` is
    /// true at the only index the loop visits. The restatement as `i + 2 >= n`
    /// must agree with the source's subtraction wherever the source can evaluate
    /// it, and must not underflow where it cannot.
    #[test]
    fn the_wide_test_matches_the_sources_subtraction_and_never_underflows() {
        assert_eq!(upsample_step(1, 2), WIDE_STEP);
        // Exhaustive agreement over every pyramid the source can build.
        (2..=LEVELS_HIGH).for_each(|count| {
            (1..count).for_each(|level| {
                let source_form = level >= count - 2;
                let ours = upsample_step(level, count);
                assert_eq!(
                    ours,
                    [NARROW_STEP, WIDE_STEP][usize::from(source_form)],
                    "level {level} of {count}"
                );
            });
        });
        // And the one case the source's form cannot evaluate at all: a
        // single-level pyramid, whose upsample loop never runs.
        assert_eq!(upsample_step(0, 1), WIDE_STEP);
    }
}
