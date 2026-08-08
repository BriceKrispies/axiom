//! The panel: a projection of the shot you can put your thumb on.
//!
//! The panel is not a picture of the curve with a handle somewhere on it. **The
//! whole panel is the handle.** Grab anywhere inside it and two things are read
//! off where you grabbed:
//!
//! * the position *along the shot* becomes where the curve peaks — grab near the
//!   ball and it breaks early, grab near the goal and it breaks late;
//! * the movement *across the shot* becomes how far it bends, one to one.
//!
//! That is the entire control. It needs no handle to find, no pixel to hit, and
//! no explanation, and it is why the design brief's "curve direction, curve
//! magnitude, approximate location of maximum bend" are all reachable from a
//! single drag with nothing on screen that looks like a slider.
//!
//! Dragging is **relative**: the offset already under the grab point is
//! remembered, and the drag adds to it. Absolute dragging would snap the curve to
//! the finger the instant it touched down, which turns every accidental brush
//! into an edit.
//!
//! The two projections use the orientation that is natural for what they show —
//! the top-down panel runs the shot away from you up the screen, the side panel
//! runs it left to right — and in both, the gesture the brief asks for is the
//! literal one: drag left to bend left, drag up to lift.

use axiom::prelude::Vec2;

use crate::play::Projection;
use crate::shot::BendCurve;
use crate::tuning::SculptTuning;

use super::layout::Rect;

/// How much wider than the authored maximum the panel's own scale is, so a curve
/// at full bend has visible air around it rather than touching the edge.
const HEADROOM: f32 = 1.22;

/// One projection's panel: the rectangle, and the mapping between it and the
/// curve it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SculptPanel {
    pub rect: Rect,
    pub projection: Projection,
    /// The offset in metres the panel's positive edge represents.
    pub span_positive: f32,
    /// The offset in metres the panel's negative edge represents.
    pub span_negative: f32,
}

impl SculptPanel {
    /// Build the panel for a projection, scaled to that axis's authored bounds.
    pub fn new(rect: Rect, projection: Projection, tuning: &SculptTuning) -> SculptPanel {
        SculptPanel {
            rect,
            projection,
            span_positive: tuning.max_offset * HEADROOM,
            span_negative: tuning.min_offset * HEADROOM,
        }
    }

    /// Shot progress (`0` at the ball, `1` at the goal) under a screen point.
    pub fn progress_at(&self, screen: Vec2) -> f32 {
        let n = self.rect.normalized(self.rect.clamp(screen));
        match self.projection {
            // Top-down: the shot runs away from the viewer, up the panel.
            Projection::Horizontal => 1.0 - n.y,
            // Side elevation: the shot runs left to right.
            Projection::Vertical => n.x,
        }
    }

    /// Metres of curve offset per pixel of drag across the panel's offset axis.
    pub fn metres_per_pixel(&self) -> f32 {
        let extent = self.span_positive - self.span_negative;
        let pixels = match self.projection {
            Projection::Horizontal => self.rect.w,
            Projection::Vertical => self.rect.h,
        };
        extent / pixels.max(1.0)
    }

    /// The signed offset movement a screen delta represents, in metres.
    ///
    /// Screen `y` grows downward and height grows upward, which is the one sign
    /// flip in the whole editor and it lives here rather than in six call sites.
    pub fn offset_delta(&self, delta: Vec2) -> f32 {
        let pixels = match self.projection {
            Projection::Horizontal => delta.x,
            Projection::Vertical => -delta.y,
        };
        pixels * self.metres_per_pixel()
    }

    /// Where a `(progress, offset)` pair sits on the screen — how the curve, the
    /// baseline and the handle are all drawn.
    pub fn plot(&self, progress: f32, offset: f32) -> Vec2 {
        let p = progress.clamp(0.0, 1.0);
        let extent = (self.span_positive - self.span_negative).max(1.0e-4);
        let across = ((offset - self.span_negative) / extent).clamp(0.0, 1.0);
        match self.projection {
            Projection::Horizontal => Vec2::new(
                self.rect.x + across * self.rect.w,
                self.rect.y + (1.0 - p) * self.rect.h,
            ),
            Projection::Vertical => Vec2::new(
                self.rect.x + p * self.rect.w,
                self.rect.y + (1.0 - across) * self.rect.h,
            ),
        }
    }

    /// A polyline of the curve across the panel.
    pub fn polyline(&self, curve: &BendCurve, segments: usize) -> Vec<Vec2> {
        (0..=segments.max(2))
            .map(|i| {
                let u = i as f32 / segments.max(2) as f32;
                self.plot(u, curve.offset(u))
            })
            .collect()
    }

    /// The straight reference the curve is a deformation of.
    pub fn baseline(&self) -> [Vec2; 2] {
        [self.plot(0.0, 0.0), self.plot(1.0, 0.0)]
    }
}

/// A drag in progress on a panel: what it grabbed, and where it started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grab {
    /// Shot progress the gesture grabbed at — this becomes where the curve peaks.
    pub progress: f32,
    /// The offset that was already there, so the drag can be relative.
    pub base_offset: f32,
    /// Where the gesture started, in pixels.
    pub origin: Vec2,
}

impl Grab {
    /// Take hold of a curve at a screen point.
    pub fn begin(panel: &SculptPanel, curve: &BendCurve, at: Vec2) -> Grab {
        let progress = panel.progress_at(at);
        Grab {
            progress,
            base_offset: curve.offset(progress),
            origin: at,
        }
    }

    /// The curve this drag now describes.
    pub fn curve(&self, panel: &SculptPanel, at: Vec2, tuning: &SculptTuning) -> BendCurve {
        let moved = panel.offset_delta(at.subtract(self.origin)) * tuning.drag_gain;
        BendCurve::through(
            self.progress,
            self.base_offset + moved,
            tuning.peak_margin,
        )
        .bounded(tuning.min_offset, tuning.max_offset)
    }

    /// Where the feedback handle is drawn: on the curve at the grabbed progress,
    /// but lifted clear of the finger so a thumb never covers the one thing worth
    /// looking at.
    pub fn handle(&self, panel: &SculptPanel, curve: &BendCurve, lift: f32) -> Vec2 {
        let on_curve = panel.plot(self.progress, curve.offset(self.progress));
        Vec2::new(on_curve.x, on_curve.y - lift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    fn panels() -> (SculptPanel, SculptPanel) {
        let t = Tuning::DEFAULT;
        let rect = Rect::new(20.0, 400.0, 350.0, 260.0);
        (
            SculptPanel::new(rect, Projection::Horizontal, &t.bend),
            SculptPanel::new(rect, Projection::Vertical, &t.loft),
        )
    }

    #[test]
    fn the_ball_end_of_each_panel_is_where_the_shot_starts() {
        let (top_down, side) = panels();
        // Top-down: the ball is at the bottom, the goal at the top.
        assert!(top_down.progress_at(Vec2::new(200.0, 655.0)) < 0.1);
        assert!(top_down.progress_at(Vec2::new(200.0, 405.0)) > 0.9);
        // Side: the ball is on the left, the goal on the right.
        assert!(side.progress_at(Vec2::new(25.0, 500.0)) < 0.1);
        assert!(side.progress_at(Vec2::new(365.0, 500.0)) > 0.9);
        // A point outside the panel clamps rather than escaping the range.
        assert_eq!(top_down.progress_at(Vec2::new(-900.0, -900.0)), 1.0);
    }

    #[test]
    fn dragging_right_bends_right_and_dragging_up_lifts() {
        let t = Tuning::DEFAULT;
        let (top_down, side) = panels();
        let straight = BendCurve::STRAIGHT;

        let grab = Grab::begin(&top_down, &straight, top_down.rect.centre());
        let right = grab.curve(&top_down, top_down.rect.centre().add(Vec2::new(60.0, 0.0)), &t.bend);
        let left = grab.curve(&top_down, top_down.rect.centre().add(Vec2::new(-60.0, 0.0)), &t.bend);
        assert!(right.magnitude() > 0.2, "drag right must bend right");
        assert!(left.magnitude() < -0.2, "drag left must bend left");

        let grab = Grab::begin(&side, &straight, side.rect.centre());
        let up = grab.curve(&side, side.rect.centre().add(Vec2::new(0.0, -60.0)), &t.loft);
        let down = grab.curve(&side, side.rect.centre().add(Vec2::new(0.0, 60.0)), &t.loft);
        assert!(up.magnitude() > 0.2, "drag up must loft");
        assert!(down.magnitude() < -0.05, "drag down must flatten and dip");
    }

    #[test]
    fn the_drag_is_one_to_one_and_relative_to_what_was_already_there() {
        let t = Tuning::DEFAULT;
        let (panel, _) = panels();
        let existing = BendCurve::through(0.5, 1.0, 0.14);
        let start = panel.plot(0.5, existing.offset(0.5));
        let grab = Grab::begin(&panel, &existing, start);
        // Touching down without moving changes nothing.
        assert!((grab.curve(&panel, start, &t.bend).offset(0.5) - 1.0).abs() < 0.05);
        // Moving n pixels moves the curve by exactly n pixels' worth of metres.
        let pixels = 40.0;
        let moved = grab.curve(&panel, start.add(Vec2::new(pixels, 0.0)), &t.bend);
        let expected = 1.0 + pixels * panel.metres_per_pixel();
        assert!(
            (moved.offset(0.5) - expected).abs() < 0.05,
            "expected {expected}, got {}",
            moved.offset(0.5)
        );
    }

    #[test]
    fn where_you_grab_is_where_the_curve_breaks() {
        let t = Tuning::DEFAULT;
        let (panel, _) = panels();
        let near_ball = panel.rect.y + panel.rect.h * 0.85;
        let near_goal = panel.rect.y + panel.rect.h * 0.15;
        let grab_early = Grab::begin(&panel, &BendCurve::STRAIGHT, Vec2::new(200.0, near_ball));
        let grab_late = Grab::begin(&panel, &BendCurve::STRAIGHT, Vec2::new(200.0, near_goal));
        let early = grab_early.curve(&panel, Vec2::new(260.0, near_ball), &t.bend);
        let late = grab_late.curve(&panel, Vec2::new(260.0, near_goal), &t.bend);
        assert!(early.peak().0 < 0.4, "an early grab peaks at {}", early.peak().0);
        assert!(late.peak().0 > 0.6, "a late grab peaks at {}", late.peak().0);
    }

    #[test]
    fn a_drag_can_never_author_a_curve_outside_the_authored_bounds() {
        let t = Tuning::DEFAULT;
        let (top_down, side) = panels();
        let grab = Grab::begin(&top_down, &BendCurve::STRAIGHT, top_down.rect.centre());
        let miles = grab.curve(&top_down, Vec2::new(90_000.0, 0.0), &t.bend);
        assert!(miles.magnitude() <= t.bend.max_offset + 1.0e-3);
        let grab = Grab::begin(&side, &BendCurve::STRAIGHT, side.rect.centre());
        let under = grab.curve(&side, Vec2::new(0.0, 90_000.0), &t.loft);
        assert!(under.magnitude() >= t.loft.min_offset - 1.0e-3);
    }

    #[test]
    fn the_plotted_curve_stays_inside_its_panel_and_the_handle_lifts_clear() {
        let t = Tuning::DEFAULT;
        let (panel, _) = panels();
        let curve = BendCurve::through(0.5, t.bend.max_offset, 0.14);
        panel.polyline(&curve, 24).iter().for_each(|p| {
            assert!(panel.rect.contains(*p), "{p:?} escaped the panel");
        });
        assert_eq!(panel.polyline(&curve, 0).len(), 3, "a degenerate count still draws");
        let base = panel.baseline();
        assert!((base[0].x - base[1].x).abs() < 1.0e-3, "the reference is straight");
        let grab = Grab::begin(&panel, &curve, panel.rect.centre());
        let handle = grab.handle(&panel, &curve, 40.0);
        assert!(handle.y < panel.plot(grab.progress, curve.offset(grab.progress)).y);
    }
}
