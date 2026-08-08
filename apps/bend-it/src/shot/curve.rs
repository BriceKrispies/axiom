//! The curve the player sculpts: a two-parameter cubic Bézier *offset*.
//!
//! The player is not drawing a path. They are deforming a valid one, and the
//! whole editable state of one projection is two numbers.
//!
//! Write the shot's straight line from ball to target as the base, and express
//! everything the player does as an offset away from it. A cubic Bézier whose
//! two end offsets are pinned to zero has exactly two free control weights:
//!
//! ```text
//! offset(u) = 3(1-u)²u · w1  +  3(1-u)u² · w2
//! ```
//!
//! That is the compact parameter space the design asks for. It cannot produce a
//! loop, a cusp, or a path that leaves the ball or misses the target — those
//! failures are not *rejected*, they are unrepresentable. And it still spans
//! everything a shot needs: `w1 ≈ w2` is a symmetric arc, `w1 ≫ w2` peaks early
//! (a shot that breaks late looks like it dips), `w1 ≈ -w2` is the double
//! movement of a knuckling strike.
//!
//! Direction, magnitude and the location of maximum bend are all *read out* of
//! `(w1, w2)` rather than stored — which is why the player never sees a slider
//! for any of them.

/// One projection's editable shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BendCurve {
    pub w1: f32,
    pub w2: f32,
}

/// The two cubic Bernstein weights the free control points ride on.
fn basis(u: f32) -> (f32, f32) {
    let v = 1.0 - u;
    (3.0 * v * v * u, 3.0 * v * u * u)
}

/// How finely the curve is scanned when its peak is measured.
const PEAK_SAMPLES: usize = 48;

impl BendCurve {
    /// No deformation at all: the shot runs straight to the target.
    pub const STRAIGHT: BendCurve = BendCurve { w1: 0.0, w2: 0.0 };

    /// The offset from the straight line at shot progress `u ∈ [0, 1]`. Zero at
    /// both ends, always — which is the guarantee that the trajectory starts at
    /// the ball and finishes on the authored point.
    pub fn offset(&self, u: f32) -> f32 {
        let (b1, b2) = basis(u.clamp(0.0, 1.0));
        b1 * self.w1 + b2 * self.w2
    }

    /// The rate of change of the offset — the local slope of the deformation,
    /// which the ball's spin and the kicker's approach angle both read.
    pub fn slope(&self, u: f32) -> f32 {
        let u = u.clamp(0.0, 1.0);
        let v = 1.0 - u;
        3.0 * (v * v - 2.0 * v * u) * self.w1 + 3.0 * (2.0 * v * u - u * u) * self.w2
    }

    /// The curve that puts exactly `displacement` of offset at progress `u`,
    /// choosing the **smallest** weights that do so.
    ///
    /// The minimum-norm solution is what makes direct manipulation feel right: of
    /// all the curves through the player's fingertip, it is the one that bulges
    /// closest to where they grabbed and stays flattest everywhere else. Grab
    /// near the ball and the shot breaks early; grab near the goal and it breaks
    /// late — from one drag, with no control to explain.
    pub fn through(u: f32, displacement: f32, peak_margin: f32) -> BendCurve {
        let margin = peak_margin.clamp(0.0, 0.45);
        let (b1, b2) = basis(u.clamp(margin, 1.0 - margin));
        let denominator = (b1 * b1 + b2 * b2).max(1.0e-6);
        BendCurve {
            w1: displacement * b1 / denominator,
            w2: displacement * b2 / denominator,
        }
    }

    /// Where the curve bulges most, as `(progress, signed offset)`.
    pub fn peak(&self) -> (f32, f32) {
        (0..=PEAK_SAMPLES)
            .map(|i| {
                let u = i as f32 / PEAK_SAMPLES as f32;
                (u, self.offset(u))
            })
            .fold((0.5, 0.0), |best, candidate| {
                let keep = candidate.1.abs() > best.1.abs();
                [best, candidate][usize::from(keep)]
            })
    }

    /// The signed magnitude of the deformation: how far, and which way.
    pub fn magnitude(&self) -> f32 {
        self.peak().1
    }

    /// The same shape, scaled so its peak sits inside `[min, max]`. Scaling
    /// rather than clipping is deliberate — a clipped curve develops a flat spot
    /// where the player was pushing hardest, which reads as the control breaking.
    pub fn bounded(&self, min: f32, max: f32) -> BendCurve {
        let peak = self.magnitude();
        let limit = [min.abs(), max][usize::from(peak >= 0.0)].max(1.0e-4);
        let scale = (limit / peak.abs().max(1.0e-6)).min(1.0);
        BendCurve {
            w1: self.w1 * scale,
            w2: self.w2 * scale,
        }
    }

    /// The smallest uniform scale in `[0, 1]` that keeps `base(u) + offset(u)`
    /// at or above `floor` for every `u`, applied.
    ///
    /// This is how the height projection is stopped from going underground
    /// without stopping it from dipping: the *shape* the player drew survives, it
    /// is simply not allowed to break the turf.
    pub fn floored(&self, floor_gap: impl Fn(f32) -> f32) -> BendCurve {
        let scale = (0..=PEAK_SAMPLES)
            .map(|i| {
                let u = i as f32 / PEAK_SAMPLES as f32;
                let offset = self.offset(u);
                let gap = floor_gap(u);
                // Only a downward offset deeper than the available gap binds.
                let binding = (offset < -gap) & (offset < 0.0);
                [1.0f32, (gap / offset.abs().max(1.0e-6)).clamp(0.0, 1.0)]
                    [usize::from(binding)]
            })
            .fold(1.0f32, f32::min);
        BendCurve {
            w1: self.w1 * scale,
            w2: self.w2 * scale,
        }
    }
}

impl Default for BendCurve {
    fn default() -> Self {
        BendCurve::STRAIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_curve_is_pinned_to_zero_at_both_ends() {
        let curve = BendCurve { w1: 3.0, w2: -1.0 };
        assert_eq!(curve.offset(0.0), 0.0);
        assert_eq!(curve.offset(1.0), 0.0);
        // Out-of-range progress clamps rather than exploding.
        assert_eq!(curve.offset(-1.0), 0.0);
        assert_eq!(curve.offset(2.0), 0.0);
        assert_eq!(BendCurve::STRAIGHT.offset(0.5), 0.0);
        assert_eq!(BendCurve::default(), BendCurve::STRAIGHT);
    }

    #[test]
    fn a_grab_puts_the_offset_exactly_under_the_finger() {
        for u in [0.25f32, 0.5, 0.75] {
            let curve = BendCurve::through(u, 2.0, 0.14);
            assert!(
                (curve.offset(u) - 2.0).abs() < 1.0e-3,
                "grab at {u} landed at {}",
                curve.offset(u)
            );
        }
    }

    #[test]
    fn a_grab_near_an_endpoint_is_pulled_in_to_stay_solvable() {
        let curve = BendCurve::through(0.0, 2.0, 0.14);
        assert!(curve.offset(0.14) > 0.5, "an edge grab still bends");
        assert!(curve.w1.is_finite() && curve.w2.is_finite());
        let far = BendCurve::through(1.0, -2.0, 0.14);
        assert!(far.offset(0.86) < -0.5);
    }

    #[test]
    fn where_you_grab_is_where_it_breaks() {
        let early = BendCurve::through(0.28, 2.0, 0.14).peak().0;
        let late = BendCurve::through(0.72, 2.0, 0.14).peak().0;
        assert!(early < 0.5, "an early grab peaks early: {early}");
        assert!(late > 0.5, "a late grab peaks late: {late}");
        assert!(early < late);
    }

    #[test]
    fn bounding_scales_the_shape_instead_of_clipping_it() {
        let big = BendCurve::through(0.5, 10.0, 0.14);
        let bounded = big.bounded(-4.0, 4.0);
        assert!((bounded.magnitude() - 4.0).abs() < 1.0e-2);
        // The shape is preserved: the weight ratio is unchanged.
        assert!((bounded.w1 / bounded.w2 - big.w1 / big.w2).abs() < 1.0e-3);
        // A negative peak is bounded against the negative limit.
        let down = BendCurve::through(0.5, -10.0, 0.14).bounded(-1.5, 4.0);
        assert!((down.magnitude() + 1.5).abs() < 1.0e-2);
        // Something already inside the bounds is left alone.
        let small = BendCurve::through(0.5, 1.0, 0.14);
        assert_eq!(small.bounded(-4.0, 4.0), small);
        // A perfectly straight curve survives bounding without dividing by zero.
        assert_eq!(BendCurve::STRAIGHT.bounded(-1.0, 1.0), BendCurve::STRAIGHT);
    }

    #[test]
    fn flooring_stops_a_dip_from_going_through_the_turf() {
        // A dip of 3 m under a line that is only 1 m above the ground.
        let dip = BendCurve::through(0.5, -3.0, 0.14);
        let safe = dip.floored(|_| 1.0);
        (0..=20).for_each(|i| {
            let u = i as f32 / 20.0;
            assert!(
                1.0 + safe.offset(u) > -1.0e-3,
                "u={u} broke the floor at {}",
                safe.offset(u)
            );
        });
        // An upward curve is never scaled by the floor.
        let lift = BendCurve::through(0.5, 3.0, 0.14);
        assert_eq!(lift.floored(|_| 1.0), lift);
    }

    #[test]
    fn the_slope_reverses_across_the_peak() {
        let curve = BendCurve::through(0.5, 2.0, 0.14);
        assert!(curve.slope(0.2) > 0.0);
        assert!(curve.slope(0.8) < 0.0);
        assert_eq!(BendCurve::STRAIGHT.slope(0.4), 0.0);
    }
}
