//! The virtual-control synthesizer: pointer samples in, control intent out.

use axiom_kernel::Radians;
use axiom_math::Vec2;

use crate::control_frame::ControlFrame;

/// Fraction of the surface width that is the left (movement) zone; the remainder
/// is the right (look) zone.
const MOVE_ZONE_SPLIT: f32 = 0.5;
/// Virtual-joystick radius as a fraction of the surface's shorter edge: a drag
/// of this far from the thumb's first touch is full deflection.
const STICK_RADIUS_FRACTION: f32 = 0.18;
/// Look sensitivity: radians of view rotation per pixel of look-drag.
const LOOK_RADIANS_PER_PIXEL: f32 = 0.005;
/// Minimum swipe length, as a fraction of the surface's shorter edge, before a
/// completed drag counts as a directional swipe (shorter drags are ignored).
const SWIPE_MIN_FRACTION: f32 = 0.10;
/// Floor for divisors derived from positions, so a degenerate (zero-area)
/// surface or a zero-length drag can never divide by zero.
const TINY: f32 = 1.0e-6;

/// Stateful synthesizer for the standard mobile control scheme: a left-thumb
/// virtual joystick for movement and a right-thumb drag for looking. It is
/// equally the desktop scheme — a mouse with its button down is just one more
/// pointer — so a single code path serves touch, pen, and mouse.
///
/// It holds only the small per-frame state the scheme needs (the joystick's
/// anchor and the look pointer's last position), so two instances driven with
/// the same surface and pointer samples reach byte-identical control frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchControls {
    /// Where the active move thumb first touched (surface pixels); `None` when
    /// no move pointer is active. Joystick deflection is measured from here.
    move_anchor: Option<Vec2>,
    /// The look pointer's position last frame; `None` when none was active. The
    /// look delta is this-frame minus last-frame.
    prev_look: Option<Vec2>,
    /// Where the current swipe gesture began (surface pixels); `None` between
    /// gestures. Used only by [`Self::swipe`], independent of the analog state.
    swipe_start: Option<Vec2>,
    /// The latest position of the swiping pointer, so the gesture's
    /// displacement can be measured at lift (when no sample remains).
    swipe_last: Option<Vec2>,
}

impl TouchControls {
    /// A fresh synthesizer with no active pointers.
    pub const fn new() -> Self {
        TouchControls {
            move_anchor: None,
            prev_look: None,
            swipe_start: None,
            swipe_last: None,
        }
    }

    /// Synthesize one frame of control intent from the active pointers over a
    /// `surface`-sized area (device pixels, +x right, +y down). Each pointer is
    /// `(position, is_down)`; only `is_down` pointers drive control. The first
    /// down pointer in the left zone is the movement thumb, the first in the
    /// right zone is the look thumb.
    pub fn update(&mut self, surface: Vec2, pointers: &[(Vec2, bool)]) -> ControlFrame {
        let split_x = surface.x * MOVE_ZONE_SPLIT;
        let move_pos = pointers
            .iter()
            .filter(|p| p.1)
            .find(|p| p.0.x < split_x)
            .map(|p| p.0);
        let look_pos = pointers
            .iter()
            .filter(|p| p.1)
            .find(|p| p.0.x >= split_x)
            .map(|p| p.0);

        // Anchor: adopt the current position on first touch, keep it while the
        // thumb stays down, clear it when the thumb lifts (`move_pos` is `None`).
        let anchor = move_pos.map(|mp| self.move_anchor.unwrap_or(mp));
        self.move_anchor = anchor;

        let radius = (surface.x.min(surface.y) * STICK_RADIUS_FRACTION).max(TINY);
        let move_vector = move_pos
            .zip(anchor)
            .map(|(mp, a)| thumbstick(mp.subtract(a), radius))
            .unwrap_or(Vec2::ZERO);

        // Look delta is this-frame minus last-frame, zero when either is absent;
        // then remember this frame's look position (clearing it on lift, so the
        // view does not jump when the thumb returns).
        let look_delta = look_pos
            .zip(self.prev_look)
            .map(|(now, prev)| now.subtract(prev))
            .unwrap_or(Vec2::ZERO);
        self.prev_look = look_pos;

        // +x is right, +y is down. Dragging right turns the view right (negative
        // yaw, since +yaw turns left); dragging up looks up (positive pitch).
        let yaw = radians(look_delta.x * -LOOK_RADIANS_PER_PIXEL);
        let pitch = radians(look_delta.y * -LOOK_RADIANS_PER_PIXEL);
        ControlFrame::new(move_vector, yaw, pitch)
    }

    /// Detect a directional **swipe** over `surface`-sized pixels: the discrete
    /// counterpart of [`Self::update`], for grid/turn-based games. While a
    /// pointer is down the gesture's start and latest positions are tracked; on
    /// lift (no down pointer this call), if the start→end displacement exceeds a
    /// fraction of the surface's shorter edge, its **unit direction** is returned
    /// (+x right, +y down) — exactly one direction per completed swipe. `None`
    /// mid-gesture, for a too-short flick, or when no gesture is in progress. The
    /// caller maps the direction to its own discrete command (4- or 8-way).
    ///
    /// Uses its own state, independent of [`Self::update`]; an app uses one
    /// scheme or the other, never both at once.
    pub fn swipe(&mut self, surface: Vec2, pointers: &[(Vec2, bool)]) -> Option<Vec2> {
        let down = pointers.iter().filter(|p| p.1).map(|p| p.0).next();
        // While a pointer is down, latch the start (first sample) and track the
        // latest position; an empty option iterates zero times (no `if`).
        down.iter().for_each(|pos| {
            self.swipe_start = self.swipe_start.or(Some(*pos));
            self.swipe_last = Some(*pos);
        });

        // The gesture completes on the frame the pointer lifts (none down).
        let lifted = down.is_none();
        let threshold = (surface.x.min(surface.y) * SWIPE_MIN_FRACTION).max(TINY);
        let result = lifted
            .then_some(self.swipe_start.zip(self.swipe_last))
            .flatten()
            .map(|(start, last)| last.subtract(start))
            .filter(|d| d.length() >= threshold)
            .and_then(|d| d.normalize().ok());

        // Reset the gesture state on lift (branchless): clear when lifted, keep
        // the just-latched start/last while the gesture is still in progress.
        self.swipe_start = lifted.then_some(None).unwrap_or(self.swipe_start);
        self.swipe_last = lifted.then_some(None).unwrap_or(self.swipe_last);
        result
    }
}

impl Default for TouchControls {
    fn default() -> Self {
        TouchControls::new()
    }
}

/// Map a raw drag offset (pixels from the joystick anchor) to a deflection
/// within the unit disc. Branchless: the scale `min(len, radius) / (max(len,
/// TINY) · radius)` is `1/radius` inside the radius (linear ramp), `1/len`
/// outside (clamped to unit length), and `0` exactly at the centre.
fn thumbstick(offset: Vec2, radius: f32) -> Vec2 {
    let len = offset.length();
    let scale = len.min(radius) / (len.max(TINY) * radius);
    offset.mul_scalar(scale)
}

/// A finite `f32` as [`Radians`]. The look delta is finite by construction
/// (finite positions times a finite constant), so the non-finite arm of
/// [`Radians::new`] is unreachable here.
fn radians(value: f32) -> Radians {
    Radians::new(value).expect("a finite look delta yields finite radians")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The surface used across tests: a 1000×600 landscape area, so the zone
    /// split is at x = 500 and the joystick radius is 600 · 0.18 = 108 px.
    fn surface() -> Vec2 {
        Vec2::new(1000.0, 600.0)
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    #[test]
    fn no_pointers_yields_neutral_intent() {
        let mut controls = TouchControls::new();
        let frame = controls.update(surface(), &[]);
        assert_eq!(frame.move_vector(), Vec2::ZERO);
        assert_eq!(frame.yaw().get(), 0.0);
        assert_eq!(frame.pitch().get(), 0.0);
    }

    #[test]
    fn default_matches_new() {
        let mut a = TouchControls::new();
        let mut b = TouchControls::default();
        assert_eq!(a, b);
        // And drives identically.
        assert_eq!(a.update(surface(), &[]), b.update(surface(), &[]));
    }

    #[test]
    fn a_pointer_that_is_not_down_is_ignored() {
        // A hovering mouse (not down) in the move zone drives nothing.
        let mut controls = TouchControls::new();
        let frame = controls.update(surface(), &[(Vec2::new(100.0, 300.0), false)]);
        assert_eq!(frame.move_vector(), Vec2::ZERO);
    }

    #[test]
    fn move_thumb_deflects_from_its_first_touch() {
        // First touch anchors at (200,300); same frame yields zero deflection.
        let mut controls = TouchControls::new();
        let f0 = controls.update(surface(), &[(Vec2::new(200.0, 300.0), true)]);
        assert!(approx(f0.move_vector().x, 0.0) && approx(f0.move_vector().y, 0.0));
        // Next frame, the thumb has moved +54px in x (= half the 108px radius),
        // so deflection is +0.5 in x and 0 in y.
        let f1 = controls.update(surface(), &[(Vec2::new(254.0, 300.0), true)]);
        assert!(approx(f1.move_vector().x, 0.5));
        assert!(approx(f1.move_vector().y, 0.0));
    }

    #[test]
    fn move_deflection_clamps_to_the_unit_disc() {
        // A drag of 250px (> the 108px radius), still inside the left move zone
        // (x < 500), clamps to unit length.
        let mut controls = TouchControls::new();
        controls.update(surface(), &[(Vec2::new(200.0, 300.0), true)]);
        let f = controls.update(surface(), &[(Vec2::new(450.0, 300.0), true)]);
        assert!(approx(f.move_vector().x, 1.0));
        assert!(approx(f.move_vector().y, 0.0));
    }

    #[test]
    fn lifting_the_move_thumb_clears_the_anchor() {
        let mut controls = TouchControls::new();
        controls.update(surface(), &[(Vec2::new(200.0, 300.0), true)]);
        // Lift: no pointers ⇒ anchor cleared, neutral move.
        let lifted = controls.update(surface(), &[]);
        assert_eq!(lifted.move_vector(), Vec2::ZERO);
        // Touch again far away: this is the new anchor, so deflection is zero.
        let retouch = controls.update(surface(), &[(Vec2::new(400.0, 300.0), true)]);
        assert!(approx(retouch.move_vector().x, 0.0));
    }

    #[test]
    fn look_thumb_drag_produces_yaw_and_pitch() {
        // Look pointer in the right zone. First frame anchors look (no delta).
        let mut controls = TouchControls::new();
        let f0 = controls.update(surface(), &[(Vec2::new(700.0, 300.0), true)]);
        assert_eq!(f0.yaw().get(), 0.0);
        assert_eq!(f0.pitch().get(), 0.0);
        // Drag +20px right, -10px up (screen y down). yaw = -20*0.005 = -0.1
        // (turn right); pitch = -(-10)*0.005 = +0.05 (look up).
        let f1 = controls.update(surface(), &[(Vec2::new(720.0, 290.0), true)]);
        assert!(approx(f1.yaw().get(), -0.1));
        assert!(approx(f1.pitch().get(), 0.05));
    }

    #[test]
    fn lifting_the_look_thumb_resets_so_the_view_does_not_jump() {
        let mut controls = TouchControls::new();
        controls.update(surface(), &[(Vec2::new(700.0, 300.0), true)]);
        controls.update(surface(), &[(Vec2::new(720.0, 300.0), true)]);
        // Lift: no look pointer ⇒ zero look delta and prev_look cleared.
        let lifted = controls.update(surface(), &[]);
        assert_eq!(lifted.yaw().get(), 0.0);
        // Re-touch far away: first frame after re-touch must not jump.
        let retouch = controls.update(surface(), &[(Vec2::new(900.0, 300.0), true)]);
        assert_eq!(retouch.yaw().get(), 0.0);
    }

    #[test]
    fn move_and_look_thumbs_drive_independently() {
        // One thumb each side, same frame: both zones resolve their own pointer.
        let mut controls = TouchControls::new();
        controls.update(
            surface(),
            &[(Vec2::new(200.0, 300.0), true), (Vec2::new(700.0, 300.0), true)],
        );
        let f = controls.update(
            surface(),
            &[(Vec2::new(254.0, 300.0), true), (Vec2::new(720.0, 300.0), true)],
        );
        assert!(approx(f.move_vector().x, 0.5));
        assert!(approx(f.yaw().get(), -0.1));
    }

    #[test]
    fn centre_drag_of_zero_length_is_neutral() {
        // Anchor and current coincide: thumbstick's centre case (len = 0).
        let mut controls = TouchControls::new();
        controls.update(surface(), &[(Vec2::new(300.0, 300.0), true)]);
        let f = controls.update(surface(), &[(Vec2::new(300.0, 300.0), true)]);
        assert_eq!(f.move_vector(), Vec2::ZERO);
    }

    #[test]
    fn degenerate_zero_surface_does_not_divide_by_zero() {
        // A zero-area surface floors the radius to TINY; a down pointer at the
        // origin still yields a finite, neutral frame rather than NaN.
        let mut controls = TouchControls::new();
        let f = controls.update(Vec2::ZERO, &[(Vec2::ZERO, true)]);
        assert!(f.move_vector().x.is_finite() && f.move_vector().y.is_finite());
    }

    // --- swipe (discrete gesture) ---
    // On the 1000×600 surface the swipe threshold is 600 · 0.10 = 60 px.

    #[test]
    fn swipe_with_no_gesture_in_progress_is_none() {
        let mut controls = TouchControls::new();
        assert_eq!(controls.swipe(surface(), &[]), None);
    }

    #[test]
    fn swipe_mid_gesture_is_none_until_lift() {
        // A pointer is still down: the gesture has not completed.
        let mut controls = TouchControls::new();
        assert_eq!(
            controls.swipe(surface(), &[(Vec2::new(700.0, 300.0), true)]),
            None
        );
    }

    #[test]
    fn horizontal_swipe_returns_a_unit_right_or_left_direction() {
        let mut controls = TouchControls::new();
        controls.swipe(surface(), &[(Vec2::new(700.0, 300.0), true)]); // start
        controls.swipe(surface(), &[(Vec2::new(800.0, 300.0), true)]); // drag +100 x
        let dir = controls.swipe(surface(), &[]).expect("a completed swipe"); // lift
        assert!(approx(dir.x, 1.0));
        assert!(approx(dir.y, 0.0));
    }

    #[test]
    fn vertical_swipe_up_returns_a_unit_up_direction() {
        // Screen +y is down, so an upward swipe has negative y.
        let mut controls = TouchControls::new();
        controls.swipe(surface(), &[(Vec2::new(300.0, 400.0), true)]);
        controls.swipe(surface(), &[(Vec2::new(300.0, 300.0), true)]); // drag -100 y
        let dir = controls.swipe(surface(), &[]).expect("a completed swipe");
        assert!(approx(dir.x, 0.0));
        assert!(approx(dir.y, -1.0));
    }

    #[test]
    fn a_too_short_flick_is_not_a_swipe() {
        // 20px drag is under the 60px threshold ⇒ no swipe on lift.
        let mut controls = TouchControls::new();
        controls.swipe(surface(), &[(Vec2::new(300.0, 300.0), true)]);
        controls.swipe(surface(), &[(Vec2::new(320.0, 300.0), true)]);
        assert_eq!(controls.swipe(surface(), &[]), None);
    }

    #[test]
    fn swipe_state_resets_so_a_second_swipe_is_independent() {
        let mut controls = TouchControls::new();
        // First swipe: right.
        controls.swipe(surface(), &[(Vec2::new(200.0, 300.0), true)]);
        controls.swipe(surface(), &[(Vec2::new(320.0, 300.0), true)]);
        let first = controls.swipe(surface(), &[]).expect("first swipe");
        assert!(approx(first.x, 1.0));
        // Second swipe: down — must not be biased by the first's start point.
        controls.swipe(surface(), &[(Vec2::new(500.0, 100.0), true)]);
        controls.swipe(surface(), &[(Vec2::new(500.0, 250.0), true)]);
        let second = controls.swipe(surface(), &[]).expect("second swipe");
        assert!(approx(second.x, 0.0));
        assert!(approx(second.y, 1.0));
    }

    #[test]
    fn swipe_on_a_degenerate_surface_stays_finite() {
        // Zero surface floors the threshold to TINY; a real drag still yields a
        // finite unit direction rather than NaN.
        let mut controls = TouchControls::new();
        controls.swipe(Vec2::ZERO, &[(Vec2::new(5.0, 0.0), true)]);
        controls.swipe(Vec2::ZERO, &[(Vec2::new(50.0, 0.0), true)]);
        let dir = controls.swipe(Vec2::ZERO, &[]).expect("a finite swipe");
        assert!(dir.x.is_finite() && dir.y.is_finite());
    }
}
