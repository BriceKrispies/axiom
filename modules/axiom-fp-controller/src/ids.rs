//! The pure value-type vocabulary the [`crate::FpController`] facade traffics in:
//! the walker [`Pose`], the held [`MoveIntent`], the per-frame [`LookDelta`], the
//! [`WalkTuning`] rates/limits, and the camera [`Lens`]. All carry kernel unit
//! newtypes ([`Meters`]/[`Radians`]/[`Ratio`]) so no naked scalar crosses the
//! public surface, and all are `Copy` plain data — they hold no behaviour beyond
//! construction and unit-typed access.

use axiom_kernel::{Meters, Radians, Ratio};

/// Held directional input for a first-person walker — the six movement/turn keys
/// as booleans. The neutral intent the controller consumes each frame: the
/// browser (or an autonomous agent) sets the flags, and [`crate::FpController::step`]
/// reduces them to motion. `forward`/`backward` drive the view-forward axis,
/// `strafe_right`/`strafe_left` the view-right axis, and `turn_left`/`turn_right`
/// add a key-turn to the look yaw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MoveIntent {
    /// Move along the view-forward axis.
    pub forward: bool,
    /// Move against the view-forward axis.
    pub backward: bool,
    /// Strafe against the view-right axis.
    pub strafe_left: bool,
    /// Strafe along the view-right axis.
    pub strafe_right: bool,
    /// Turn the view left (yaw+).
    pub turn_left: bool,
    /// Turn the view right (yaw−).
    pub turn_right: bool,
}

impl MoveIntent {
    /// Field-wise OR of two intents — how an app folds an autonomous agent's
    /// control bitmask into the human keyboard state so either can drive the walk.
    pub const fn merged(self, other: Self) -> Self {
        Self {
            forward: self.forward | other.forward,
            backward: self.backward | other.backward,
            strafe_left: self.strafe_left | other.strafe_left,
            strafe_right: self.strafe_right | other.strafe_right,
            turn_left: self.turn_left | other.turn_left,
            turn_right: self.turn_right | other.turn_right,
        }
    }
}

/// Mouse-look deltas accumulated for one frame: `yaw` about world +Y and `pitch`
/// about local +X, both already in radians (the app converts pixel movement to
/// radians via [`WalkTuning::look_sensitivity`] before building this).
#[derive(Debug, Clone, Copy)]
pub struct LookDelta {
    yaw: Radians,
    pitch: Radians,
}

impl LookDelta {
    /// A look delta of `yaw` and `pitch` radians.
    pub const fn new(yaw: Radians, pitch: Radians) -> Self {
        Self { yaw, pitch }
    }

    /// The zero delta — no mouse-look this frame.
    pub const fn none() -> Self {
        Self {
            yaw: Radians::finite_or_zero(0.0),
            pitch: Radians::finite_or_zero(0.0),
        }
    }

    /// The yaw delta (about world +Y).
    pub const fn yaw(self) -> Radians {
        self.yaw
    }

    /// The pitch delta (about local +X).
    pub const fn pitch(self) -> Radians {
        self.pitch
    }
}

/// A first-person walker's pose: planar ground position (`x`, `z` metres) and the
/// look angles (`yaw` about world +Y, `pitch` about local +X). The eye height is
/// not part of the pose — it is seated on the terrain at view time via
/// [`WalkTuning::eye_height`].
#[derive(Debug, Clone, Copy)]
pub struct Pose {
    x: Meters,
    z: Meters,
    yaw: Radians,
    pitch: Radians,
}

impl Pose {
    /// A pose at planar `(x, z)` looking along `(yaw, pitch)`.
    pub const fn new(x: Meters, z: Meters, yaw: Radians, pitch: Radians) -> Self {
        Self { x, z, yaw, pitch }
    }

    /// The planar x position.
    pub const fn x(self) -> Meters {
        self.x
    }

    /// The planar z position.
    pub const fn z(self) -> Meters {
        self.z
    }

    /// The look yaw (about world +Y).
    pub const fn yaw(self) -> Radians {
        self.yaw
    }

    /// The look pitch (about local +X).
    pub const fn pitch(self) -> Radians {
        self.pitch
    }
}

/// Per-frame movement/turn/look tuning for a first-person walker: the rates and
/// limits that shape [`crate::FpController::step`]. [`WalkTuning::walk`] is the
/// shared default the gallery's first-person demos use.
#[derive(Debug, Clone, Copy)]
pub struct WalkTuning {
    move_speed: Meters,
    turn_speed: Radians,
    eye_height: Meters,
    pitch_limit: Radians,
    look_sensitivity: Ratio,
}

impl WalkTuning {
    /// A tuning with the given per-frame move speed, key-turn speed, eye height,
    /// pitch clamp, and mouse look sensitivity (radians per pixel of movement).
    pub const fn new(
        move_speed: Meters,
        turn_speed: Radians,
        eye_height: Meters,
        pitch_limit: Radians,
        look_sensitivity: Ratio,
    ) -> Self {
        Self {
            move_speed,
            turn_speed,
            eye_height,
            pitch_limit,
            look_sensitivity,
        }
    }

    /// The shared first-person walk tuning: a human-paced stroll seated 1.7 m
    /// above the floor, with a near-vertical pitch clamp and a classic FPS
    /// mouse-look sensitivity. A first-person walkable app uses this, so the
    /// rates live here once rather than duplicated as app constants.
    pub const fn walk() -> Self {
        Self {
            move_speed: Meters::finite_or_zero(0.22),
            turn_speed: Radians::finite_or_zero(0.028),
            eye_height: Meters::finite_or_zero(1.7),
            pitch_limit: Radians::finite_or_zero(1.45),
            look_sensitivity: Ratio::finite_or_zero(0.0022),
        }
    }

    /// The per-frame movement speed.
    pub const fn move_speed(self) -> Meters {
        self.move_speed
    }

    /// The per-frame key-turn speed.
    pub const fn turn_speed(self) -> Radians {
        self.turn_speed
    }

    /// The eye height seated above the terrain.
    pub const fn eye_height(self) -> Meters {
        self.eye_height
    }

    /// The symmetric pitch clamp (`±pitch_limit`).
    pub const fn pitch_limit(self) -> Radians {
        self.pitch_limit
    }

    /// The mouse-look sensitivity: radians of look per pixel of pointer movement.
    pub const fn look_sensitivity(self) -> Ratio {
        self.look_sensitivity
    }
}

impl Default for WalkTuning {
    fn default() -> Self {
        Self::walk()
    }
}

/// A perspective camera lens for the first-person view: vertical field of view,
/// viewport aspect ratio, and the near/far clip planes.
#[derive(Debug, Clone, Copy)]
pub struct Lens {
    fov: Radians,
    aspect: Ratio,
    near: Meters,
    far: Meters,
}

impl Lens {
    /// A lens with vertical field of view `fov`, viewport `aspect`, and the
    /// `near`/`far` clip planes.
    pub const fn new(fov: Radians, aspect: Ratio, near: Meters, far: Meters) -> Self {
        Self {
            fov,
            aspect,
            near,
            far,
        }
    }

    /// The vertical field of view.
    pub const fn fov(self) -> Radians {
        self.fov
    }

    /// The viewport aspect ratio (width / height).
    pub const fn aspect(self) -> Ratio {
        self.aspect
    }

    /// The near clip plane.
    pub const fn near(self) -> Meters {
        self.near
    }

    /// The far clip plane.
    pub const fn far(self) -> Meters {
        self.far
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_intent_merges_field_wise() {
        let keyboard = MoveIntent {
            forward: true,
            turn_left: true,
            ..MoveIntent::default()
        };
        let agent = MoveIntent {
            backward: true,
            turn_left: true,
            strafe_right: true,
            ..MoveIntent::default()
        };
        let merged = keyboard.merged(agent);
        assert_eq!(
            merged,
            MoveIntent {
                forward: true,
                backward: true,
                strafe_left: false,
                strafe_right: true,
                turn_left: true,
                turn_right: false,
            }
        );
        // Default is all-false.
        assert_eq!(
            MoveIntent::default().merged(MoveIntent::default()),
            MoveIntent::default()
        );
    }

    #[test]
    fn look_delta_carries_and_zeroes() {
        let l = LookDelta::new(Radians::finite_or_zero(0.3), Radians::finite_or_zero(-0.1));
        assert_eq!(l.yaw().get(), 0.3);
        assert_eq!(l.pitch().get(), -0.1);
        let z = LookDelta::none();
        assert_eq!(z.yaw().get(), 0.0);
        assert_eq!(z.pitch().get(), 0.0);
    }

    #[test]
    fn pose_carries_its_components() {
        let p = Pose::new(
            Meters::finite_or_zero(3.0),
            Meters::finite_or_zero(-4.0),
            Radians::finite_or_zero(0.5),
            Radians::finite_or_zero(-0.2),
        );
        assert_eq!(p.x().get(), 3.0);
        assert_eq!(p.z().get(), -4.0);
        assert_eq!(p.yaw().get(), 0.5);
        assert_eq!(p.pitch().get(), -0.2);
    }

    #[test]
    fn walk_tuning_exposes_shared_rates() {
        let t = WalkTuning::walk();
        assert_eq!(t.move_speed().get(), 0.22);
        assert_eq!(t.turn_speed().get(), 0.028);
        assert_eq!(t.eye_height().get(), 1.7);
        assert_eq!(t.pitch_limit().get(), 1.45);
        assert_eq!(t.look_sensitivity().get(), 0.0022);
        // Default is the shared walk tuning.
        assert_eq!(WalkTuning::default().move_speed().get(), 0.22);
    }

    #[test]
    fn walk_tuning_new_is_explicit() {
        let t = WalkTuning::new(
            Meters::finite_or_zero(1.0),
            Radians::finite_or_zero(2.0),
            Meters::finite_or_zero(3.0),
            Radians::finite_or_zero(4.0),
            Ratio::finite_or_zero(5.0),
        );
        assert_eq!(t.move_speed().get(), 1.0);
        assert_eq!(t.turn_speed().get(), 2.0);
        assert_eq!(t.eye_height().get(), 3.0);
        assert_eq!(t.pitch_limit().get(), 4.0);
        assert_eq!(t.look_sensitivity().get(), 5.0);
    }

    #[test]
    fn lens_carries_its_components() {
        let lens = Lens::new(
            Radians::finite_or_zero(1.1),
            Ratio::finite_or_zero(1.6),
            Meters::finite_or_zero(0.1),
            Meters::finite_or_zero(500.0),
        );
        assert_eq!(lens.fov().get(), 1.1);
        assert_eq!(lens.aspect().get(), 1.6);
        assert_eq!(lens.near().get(), 0.1);
        assert_eq!(lens.far().get(), 500.0);
    }

    #[test]
    fn value_types_are_debug_and_copy() {
        // Exercise the derived Debug/Clone/Copy so no derived region goes uncovered.
        let intent = MoveIntent::default();
        let intent_copy = intent;
        assert_eq!(format!("{intent:?}"), format!("{intent_copy:?}"));
        let look = LookDelta::none();
        assert!(format!("{:?}", look).contains("LookDelta"));
        let pose = Pose::new(
            Meters::finite_or_zero(0.0),
            Meters::finite_or_zero(0.0),
            Radians::finite_or_zero(0.0),
            Radians::finite_or_zero(0.0),
        );
        assert!(format!("{:?}", pose).contains("Pose"));
        let tuning = WalkTuning::walk();
        assert!(format!("{:?}", tuning).contains("WalkTuning"));
        let lens = Lens::new(
            Radians::finite_or_zero(1.0),
            Ratio::finite_or_zero(1.0),
            Meters::finite_or_zero(0.1),
            Meters::finite_or_zero(10.0),
        );
        assert!(format!("{:?}", lens.clone()).contains("Lens"));
    }
}
