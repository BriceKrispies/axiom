//! **The per-frame scalars** — every number `render(ctx)` derives before it
//! reaches a pass, and the projection jitter that brackets steps 3 and 9.
//!
//! Small, and every one of them is a grouping or a clamp that a reader would
//! otherwise have to reconstruct from a uniform value.
//!
//! # Groupings that are the specification
//!
//! - `Math.min(0.1, Math.max(1 / 480, ctx.time.dt || 1 / 60))` — the `||`
//!   catches a zero or `NaN` `dt` **before** the clamp, so the clamp never sees
//!   one. [`frame_dt`].
//! - `this.settings.shutter * (1 / 60 / dt)` — `(1/60)` first, then divided by
//!   `dt`, then multiplied. **Not** `shutter / (60 * dt)`; the two disagree in
//!   the last bits and this multiplies a blur radius. [`shutter`].
//! - `(j.x * 2) / this.screenSize.width` — doubled, then divided.
//!   [`jitter_ndc`].
//!
//! # The jitter is on the world camera only
//!
//! The viewmodel has its own MSAA target and no temporal history, so a jitter
//! on `viewCamera` would be a permanent sub-pixel wobble with nothing to
//! resolve it back out. And the jitter goes on **after** `_currVP` and
//! `_invVP` are built and comes back **off** before TAA reprojects, so the
//! velocity buffer and the reprojection both see unjittered matrices — a
//! temporal jitter leaking into the motion vectors is the classic way to make
//! TAA smear a static frame.
//!
//! # Column-major, and the two elements that move
//!
//! `camera.projectionMatrix.elements[8]` and `[9]` are the third **column**'s
//! `x` and `y` — the projection's shear terms, which is where a sub-pixel
//! offset belongs (it must not scale with depth). three and [`axiom_math::Mat4`] are both
//! column-major and both store `elements[8]` at the same place, so the indices
//! transcribe unchanged; the test pins that rather than trusting it.

use axiom_math::{Mat4, Vec2};

/// `Math.max(1 / 480, ...)` — the shortest frame interval the frame graph will
/// admit, so a 480 Hz report cannot make the shutter term explode.
pub(crate) const MIN_DT: f64 = 1.0 / 480.0;

/// `Math.min(0.1, ...)` — the longest, so a tab that was backgrounded for two
/// seconds does not blur the whole frame.
pub(crate) const MAX_DT: f64 = 0.1;

/// `ctx.time.dt || 1 / 60` — the interval assumed when the frame reports none.
pub(crate) const FALLBACK_DT: f64 = 1.0 / 60.0;

/// The shutter's reference interval: the frame's own `1 / 60`.
pub(crate) const SHUTTER_REFERENCE_DT: f64 = 1.0 / 60.0;

/// `const dt = Math.min(0.1, Math.max(1 / 480, ctx.time.dt || 1 / 60))`.
///
/// The `||` runs first and catches a zero or a `NaN`, which is why the clamp
/// below it never has to answer for one — a distinction that matters in Rust,
/// where `f64::max(NaN, x)` returns `x` rather than propagating.
pub(crate) fn frame_dt(reported: f64) -> f64 {
    let falsy = (reported == 0.0) | reported.is_nan();
    let raw = [reported, FALLBACK_DT][usize::from(falsy)];
    raw.max(MIN_DT).min(MAX_DT)
}

/// The exposure metering's `dt`: `s.autoExposure ? dt : 1e3`.
///
/// `1e3` does not *disable* the adaptation — it makes one frame's worth of it
/// arbitrarily large, so the exposure snaps to the new measurement instead of
/// easing toward it. A reader who took `autoExposure: false` to mean "hold the
/// current exposure" would have it exactly backwards.
pub(crate) fn metering_dt(auto_exposure: bool, dt: f64) -> f64 {
    [1.0e3, dt][usize::from(auto_exposure)]
}

/// `const shutter = this.settings.shutter * (1 / 60 / dt)`.
///
/// The shutter angle is authored against a 60 Hz frame, so a longer frame gets
/// proportionally *less* blur per frame rather than more — the exposure time is
/// what the setting fixes, not the streak length.
pub(crate) fn shutter(setting: f64, dt: f64) -> f64 {
    setting * (SHUTTER_REFERENCE_DT / dt)
}

/// `_readAds` — the weapons subsystem's `adsProgress`, clamped into `0..1`.
///
/// `None` is "there is no weapons subsystem", and the source's
/// `typeof t === 'number' && t === t` is a `NaN` test written the long way. Both
/// answer zero: the renderer reads the transition and never requires it to
/// exist.
pub(crate) fn ads_engagement(reported: Option<f64>) -> f64 {
    reported
        .filter(|t| !t.is_nan())
        .map_or(0.0, |t| t.min(1.0).max(0.0))
}

/// `this.csm.setJitter(this.taa ? this.frame % 8 : 0)` — the cascades' own
/// temporal rotation, which only turns when there is a temporal filter behind
/// it to resolve the rotation back out.
pub(crate) fn cascade_jitter_index(taa: bool, frame: u64) -> u64 {
    [0, frame % 8][usize::from(taa)]
}

/// `_logExposure`'s gate: `if (this.frame % 90 !== 0) return`.
///
/// Only reached at all when `ctx.config.deterministic === true`, so the capture
/// harness's log carries the metering chain and a gameplay session's does not.
pub(crate) fn logs_exposure_this_frame(deterministic: bool, frame: u64) -> bool {
    deterministic & (frame % 90 == 0)
}

/// The two `projectionMatrix.elements` indices the jitter moves — the third
/// column's `x` and `y`.
pub(crate) const JITTER_ELEMENTS: [usize; 2] = [8, 9];

/// `(j.x * 2) / width`, `(j.y * 2) / height` — the sub-pixel offset in pixels,
/// as a clip-space offset. Doubled, **then** divided.
pub(crate) fn jitter_ndc(jitter_pixels: Vec2, screen: (u32, u32)) -> Vec2 {
    Vec2::new(
        (jitter_pixels.x * 2.0) / screen.0 as f32,
        (jitter_pixels.y * 2.0) / screen.1 as f32,
    )
}

/// `_applyJitter` — the projection with the offset added to elements 8 and 9,
/// and the two values it displaced, for `_removeJitter` to put back.
///
/// Returned as a pair rather than mutated in place because `_jitterSaved` is
/// exactly that: the source keeps the two originals so the removal restores the
/// *stored* values rather than subtracting the offset back off, which would
/// accumulate rounding across a session.
pub(crate) fn apply_jitter(projection: Mat4, offset: Vec2) -> (Mat4, Vec2) {
    let mut elements = projection.as_cols_array();
    let saved = Vec2::new(elements[JITTER_ELEMENTS[0]], elements[JITTER_ELEMENTS[1]]);
    elements[JITTER_ELEMENTS[0]] += offset.x;
    elements[JITTER_ELEMENTS[1]] += offset.y;
    (Mat4::from_cols_array(elements), saved)
}

/// `_removeJitter` — the saved values written back, not the offset subtracted.
pub(crate) fn remove_jitter(projection: Mat4, saved: Vec2) -> Mat4 {
    let mut elements = projection.as_cols_array();
    elements[JITTER_ELEMENTS[0]] = saved.x;
    elements[JITTER_ELEMENTS[1]] = saved.y;
    Mat4::from_cols_array(elements)
}

#[cfg(test)]
mod tests {
    use super::{
        ads_engagement, apply_jitter, cascade_jitter_index, frame_dt, jitter_ndc,
        logs_exposure_this_frame, metering_dt, remove_jitter, shutter, FALLBACK_DT,
        JITTER_ELEMENTS, MAX_DT, MIN_DT,
    };
    use axiom_math::{Mat4, Vec2};

    /// The `||` runs before the clamp, so a zero or `NaN` `dt` becomes 1/60 and
    /// not a clamp boundary.
    #[test]
    fn a_missing_frame_interval_becomes_one_sixtieth_before_the_clamp() {
        assert_eq!(frame_dt(1.0 / 60.0), 1.0 / 60.0);
        assert_eq!(frame_dt(0.0), FALLBACK_DT);
        assert_eq!(frame_dt(f64::NAN), FALLBACK_DT);
        // Both clamp ends.
        assert_eq!(frame_dt(1.0 / 1000.0), MIN_DT);
        assert_eq!(frame_dt(2.0), MAX_DT);
        assert_eq!(frame_dt(MIN_DT), MIN_DT);
        assert_eq!(frame_dt(MAX_DT), MAX_DT);
    }

    /// `shutter * ((1/60) / dt)`, not `shutter / (60 * dt)`.
    #[test]
    fn the_shutter_divides_the_reference_interval_by_the_real_one() {
        let dt = 1.0 / 144.0;
        assert_eq!(shutter(0.42, dt), 0.42 * ((1.0 / 60.0) / dt));
        // At the reference rate the setting passes through unchanged.
        assert_eq!(shutter(0.42, 1.0 / 60.0), 0.42);
        // A longer frame blurs *less* per frame, not more.
        assert!(shutter(0.42, 1.0 / 30.0) < 0.42);
    }

    /// `autoExposure: false` snaps the exposure; it does not freeze it.
    #[test]
    fn manual_exposure_snaps_rather_than_holds() {
        assert_eq!(metering_dt(true, 1.0 / 60.0), 1.0 / 60.0);
        assert_eq!(metering_dt(false, 1.0 / 60.0), 1.0e3);
    }

    /// The ADS engagement is clamped, and absent or `NaN` reads as zero.
    #[test]
    fn the_sight_picture_is_clamped_and_optional() {
        assert_eq!(ads_engagement(None), 0.0);
        assert_eq!(ads_engagement(Some(f64::NAN)), 0.0);
        assert_eq!(ads_engagement(Some(-3.0)), 0.0);
        assert_eq!(ads_engagement(Some(0.5)), 0.5);
        assert_eq!(ads_engagement(Some(1.0)), 1.0);
        assert_eq!(ads_engagement(Some(9.0)), 1.0);
    }

    /// The cascades rotate only when TAA is there to resolve the rotation.
    #[test]
    fn the_cascade_jitter_turns_only_behind_a_temporal_filter() {
        assert_eq!(cascade_jitter_index(false, 7), 0);
        assert_eq!(cascade_jitter_index(false, 12345), 0);
        assert_eq!(cascade_jitter_index(true, 7), 7);
        assert_eq!(cascade_jitter_index(true, 8), 0);
        assert_eq!(cascade_jitter_index(true, 12345), 12345 % 8);
    }

    /// The metering log fires every ninetieth frame, and only in capture mode.
    #[test]
    fn the_metering_log_is_a_capture_mode_thing_every_ninety_frames() {
        assert!(logs_exposure_this_frame(true, 0));
        assert!(logs_exposure_this_frame(true, 90));
        assert!(!logs_exposure_this_frame(true, 89));
        assert!(!logs_exposure_this_frame(false, 90));
    }

    /// `(j * 2) / size`, and the two projection elements it lands on.
    #[test]
    fn the_jitter_doubles_before_it_divides_and_moves_the_shear_terms() {
        let j = Vec2::new(0.375, -0.125);
        let ndc = jitter_ndc(j, (1920, 1080));
        assert_eq!(ndc.x, (0.375_f32 * 2.0) / 1920.0);
        assert_eq!(ndc.y, (-0.125_f32 * 2.0) / 1080.0);

        // A perspective projection's elements 8 and 9 are zero, and the jitter
        // is the only thing that ever writes them.
        let projection = Mat4::perspective(1.0, 16.0 / 9.0, 0.1, 500.0).expect("finite perspective");
        let before = projection.as_cols_array();
        assert_eq!(before[JITTER_ELEMENTS[0]], 0.0);
        assert_eq!(before[JITTER_ELEMENTS[1]], 0.0);

        let (jittered, saved) = apply_jitter(projection, ndc);
        assert_eq!(saved, Vec2::ZERO);
        let after = jittered.as_cols_array();
        assert_eq!(after[JITTER_ELEMENTS[0]], ndc.x);
        assert_eq!(after[JITTER_ELEMENTS[1]], ndc.y);
        // Nothing else moved: this is a shear, not a re-projection.
        (0..16)
            .filter(|i| !JITTER_ELEMENTS.contains(i))
            .for_each(|i| assert_eq!(after[i], before[i], "element {i} moved"));

        // The removal restores the saved values rather than subtracting the
        // offset, so a session cannot accumulate rounding into the projection.
        assert_eq!(remove_jitter(jittered, saved), projection);
    }

    /// The saved values are the ones restored, even when the projection already
    /// carried a shear — which is what makes the removal idempotent rather than
    /// an inverse operation.
    #[test]
    fn the_removal_restores_a_pre_existing_shear_rather_than_zeroing_it() {
        let mut elements = Mat4::perspective(1.0, 1.0, 0.1, 100.0)
            .expect("finite perspective")
            .as_cols_array();
        elements[JITTER_ELEMENTS[0]] = 0.03;
        elements[JITTER_ELEMENTS[1]] = -0.07;
        let sheared = Mat4::from_cols_array(elements);

        let (jittered, saved) = apply_jitter(sheared, Vec2::new(0.001, 0.002));
        assert_eq!(saved, Vec2::new(0.03, -0.07));
        assert_eq!(jittered.as_cols_array()[JITTER_ELEMENTS[0]], 0.03 + 0.001);
        assert_eq!(remove_jitter(jittered, saved), sheared);
    }
}
