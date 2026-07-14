//! The camera rig: a first-person eye at head height, and a third-person orbit
//! that trails behind and above the player showing their humanoid body. Both
//! views share the player's mouse-driven yaw/pitch (the rig adds no aim of its
//! own), so toggling never re-aims the crosshair. The third-person eye is
//! exponentially smoothed and clamped above the field so it cannot clip the
//! floor. Toggled by key or scrolled through with the wheel.

use axiom::prelude::Vec3;

use super::sports_lab_physics::DT;

/// The two views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    FirstPerson,
    ThirdPerson,
}

/// Third-person orbit distance limits (wheel-adjustable) and default.
const TP_DISTANCE_MIN: f32 = 2.4;
const TP_DISTANCE_MAX: f32 = 9.0;
const TP_DISTANCE_DEFAULT: f32 = 4.8;

/// Wheel notches → orbit distance change.
const ZOOM_STEP: f32 = 0.7;

/// Orbit anchor above the feet (chest height) and look-ahead along the view.
const TP_ANCHOR_Y: f32 = 1.35;

/// Base upward elevation of the third-person orbit (radians): looking level
/// still frames the player from above and behind.
const TP_PITCH_BIAS: f32 = 0.35;

/// Exponential smoothing rate for the third-person eye (per second).
const TP_EYE_SMOOTHING: f32 = 14.0;

/// Lowest the third-person eye may sit (keeps it out of the floor).
const TP_EYE_MIN_Y: f32 = 0.3;

/// The camera rig state.
#[derive(Debug)]
pub struct CameraRig {
    mode: CameraMode,
    distance: f32,
    /// Smoothed third-person eye (undefined while in first person).
    eye: Vec3,
    /// Whether `eye` holds a live smoothed value.
    eye_live: bool,
}

impl CameraRig {
    /// A rig starting in first person.
    pub fn new() -> Self {
        CameraRig {
            mode: CameraMode::FirstPerson,
            distance: TP_DISTANCE_DEFAULT,
            eye: Vec3::ZERO,
            eye_live: false,
        }
    }

    /// The current view.
    pub fn mode(&self) -> CameraMode {
        self.mode
    }

    /// Apply this step's view input: an explicit toggle flips the mode; wheel
    /// zoom (+out/−in) grows or shrinks the orbit, entering third person when
    /// zooming out of first person and snapping back to first person when the
    /// orbit shrinks below its minimum.
    pub fn apply(&mut self, toggle: bool, zoom: f32) {
        if toggle {
            self.set_mode(match self.mode {
                CameraMode::FirstPerson => CameraMode::ThirdPerson,
                CameraMode::ThirdPerson => CameraMode::FirstPerson,
            });
        }
        if zoom > 0.0 && self.mode == CameraMode::FirstPerson {
            self.set_mode(CameraMode::ThirdPerson);
        } else if zoom != 0.0 && self.mode == CameraMode::ThirdPerson {
            self.distance += zoom * ZOOM_STEP;
            if self.distance < TP_DISTANCE_MIN - 0.01 {
                self.set_mode(CameraMode::FirstPerson);
            }
            self.distance = self.distance.clamp(TP_DISTANCE_MIN, TP_DISTANCE_MAX);
        }
    }

    fn set_mode(&mut self, mode: CameraMode) {
        if mode != self.mode {
            self.mode = mode;
            self.eye_live = false; // re-seat the smoothed eye on entry
            if mode == CameraMode::ThirdPerson {
                self.distance = self.distance.clamp(TP_DISTANCE_MIN, TP_DISTANCE_MAX);
            }
        }
    }

    /// The `(eye, target)` pair for this step's render, advancing the smoothed
    /// third-person eye. `feet` is the player's ground position; `eye_fp` the
    /// first-person eye; `look` the unit look direction (yaw+pitch).
    ///
    /// Third person orbits behind the player with a base elevation: looking
    /// level frames the player from above and behind; looking down raises the
    /// orbit further; looking up swings it low (never below [`TP_EYE_MIN_Y`]).
    pub fn eye_target(&mut self, feet: Vec3, eye_fp: Vec3, look: Vec3) -> (Vec3, Vec3) {
        match self.mode {
            CameraMode::FirstPerson => (eye_fp, eye_fp.add(look)),
            CameraMode::ThirdPerson => {
                let anchor = feet.add(Vec3::new(0.0, TP_ANCHOR_Y, 0.0));
                // Split the look into its horizontal facing + pitch, then bias
                // the orbit's elevation upward from the look pitch.
                let horiz = (look.x * look.x + look.z * look.z).sqrt().max(1e-4);
                let facing = Vec3::new(look.x / horiz, 0.0, look.z / horiz);
                let pitch = look.y.clamp(-1.0, 1.0).asin();
                let elevation = (TP_PITCH_BIAS - pitch).clamp(-0.15, 1.25);
                let mut want = anchor
                    .subtract(facing.mul_scalar(self.distance * elevation.cos()))
                    .add(Vec3::new(0.0, self.distance * elevation.sin(), 0.0));
                want.y = want.y.max(TP_EYE_MIN_Y);
                if !self.eye_live {
                    self.eye = want;
                    self.eye_live = true;
                } else {
                    let k = (1.0 - (-TP_EYE_SMOOTHING * DT).exp()).clamp(0.0, 1.0);
                    self.eye = self.eye.add(want.subtract(self.eye).mul_scalar(k));
                    self.eye.y = self.eye.y.max(TP_EYE_MIN_Y);
                }
                (self.eye, anchor.add(look.mul_scalar(1.2)))
            }
        }
    }
}

impl Default for CameraRig {
    fn default() -> Self {
        CameraRig::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEET: Vec3 = Vec3::new(0.0, 0.0, 6.5);
    const EYE_FP: Vec3 = Vec3::new(0.0, 1.7, 6.5);
    const LOOK: Vec3 = Vec3::new(0.0, 0.0, -1.0); // facing −Z, level

    #[test]
    fn first_person_looks_from_the_eye_along_the_look() {
        let mut rig = CameraRig::new();
        assert_eq!(rig.mode(), CameraMode::FirstPerson);
        let (eye, target) = rig.eye_target(FEET, EYE_FP, LOOK);
        assert_eq!(eye, EYE_FP);
        assert!(target.z < eye.z, "target leads along −Z");
    }

    #[test]
    fn toggling_enters_third_person_behind_and_above() {
        let mut rig = CameraRig::new();
        rig.apply(true, 0.0);
        assert_eq!(rig.mode(), CameraMode::ThirdPerson);
        let (eye, target) = rig.eye_target(FEET, EYE_FP, LOOK);
        assert!(eye.z > FEET.z + 2.0, "the eye trails behind a −Z facing");
        assert!(eye.y > 1.0, "the eye sits above the player");
        assert!(target.z < eye.z, "it looks toward/past the player");
        // Toggling again returns to first person.
        rig.apply(true, 0.0);
        assert_eq!(rig.mode(), CameraMode::FirstPerson);
    }

    #[test]
    fn wheel_zoom_enters_widens_and_leaves_third_person() {
        let mut rig = CameraRig::new();
        rig.apply(false, 1.0); // zoom out of first person
        assert_eq!(rig.mode(), CameraMode::ThirdPerson);
        let (near, _) = rig.eye_target(FEET, EYE_FP, LOOK);
        for _ in 0..6 {
            rig.apply(false, 1.0);
        }
        // Settle the smoothing to compare distances fairly.
        let mut far = near;
        for _ in 0..300 {
            far = rig.eye_target(FEET, EYE_FP, LOOK).0;
        }
        assert!(
            far.subtract(FEET).length() > near.subtract(FEET).length() + 1.0,
            "zooming out widens the orbit"
        );
        // Zooming all the way in drops back to first person.
        for _ in 0..30 {
            rig.apply(false, -1.0);
        }
        assert_eq!(rig.mode(), CameraMode::FirstPerson);
    }

    #[test]
    fn the_third_person_eye_never_clips_the_floor() {
        let mut rig = CameraRig::new();
        rig.apply(true, 0.0);
        // Look almost straight up: the orbit would push the eye underground.
        let up_look = Vec3::new(0.0, 0.99, -0.14);
        for _ in 0..120 {
            let (eye, _) = rig.eye_target(FEET, EYE_FP, up_look);
            assert!(
                eye.y >= TP_EYE_MIN_Y - 1e-4,
                "eye stays above the field, y={}",
                eye.y
            );
        }
    }
}
