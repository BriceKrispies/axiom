//! The interactive orbit camera — **app policy**, not an engine capability.
//!
//! A crucible whose whole point is that every triangle came out of an operator
//! is only half a proof if you cannot walk around the geometry and look at it.
//! This module owns that: a target-relative spherical camera (`yaw`, `pitch`,
//! `distance` about a `target`) that the page's pointer gestures drive and the
//! per-frame closure applies through `RunningApp::set_camera`.
//!
//! It deliberately lives in the app. "How should a drag rotate the view" is a
//! product decision — a turntable here, a fly-cam in a shooter, a rail in a
//! cutscene — and pushing it into a layer or module would bake one answer into
//! the engine for every future app. The engine already gives everything this
//! needs: a camera you can re-author every frame for free (see the reuse note on
//! [`axiom::prelude::RunningApp::set_camera`]) and validated math types.
//!
//! It is also browser-free, so it compiles and is tested on native: the geometry
//! of an orbit has nothing to do with a DOM. `src/pointer_input.rs` is the
//! wasm32-only half that turns real pointer events into calls on this type.

use axiom::prelude::*;

use crate::install::{crucible_camera, CAMERA_EYE, CAMERA_FOV_DEGREES, CAMERA_TARGET};

/// How far the camera may tip above or below the horizon, in radians (~83.1°).
///
/// This is the clamp that makes [`Transform::looking_at`] total for this camera:
/// its one failure mode is a look direction parallel to `up`, and at ±1.45 rad
/// the view direction is still ~6.9° off the pole. It also stops the horizon
/// flipping through vertical, which is the classic turntable sickness.
const PITCH_LIMIT: f32 = 1.45;

/// The orbit distance band, in world units.
///
/// The scene spans 192 units end to end (the terrain plate) and the
/// smallest thing worth inspecting is a single primitive in the reference row, a
/// couple of units across. `4.0` puts the eye just outside one of those without
/// letting the near plane (0.4) clip through it; `400.0` is twice the authored
/// framing distance, so it pulls back well past the whole crucible while staying
/// far inside the 700-unit far plane, so the user can neither fly to infinity
/// nor invert through the target.
const MIN_DISTANCE: f32 = 4.0;
const MAX_DISTANCE: f32 = 400.0;

/// Radians of rotation per unit of drag, where one unit is a full canvas height.
/// A drag from the top of the canvas to the bottom is therefore half a turn.
const ORBIT_RADIANS_PER_UNIT: f32 = std::f32::consts::PI;

/// Distance multiplier per pixel of wheel travel, applied exponentially. One
/// ~100px Chrome notch is `e^0.15` ≈ 1.16× — a sixth of the distance per notch,
/// so the zoom feels the same whether you are 4 or 400 units out.
const WHEEL_ZOOM_PER_PIXEL: f32 = 0.0015;

/// The orbit camera's state: everything the framing is a pure function of.
#[derive(Debug, Clone)]
pub struct OrbitState {
    /// The point the camera looks at and rotates around.
    target: Vec3,
    /// Rotation about world +Y. Unbounded — it wraps naturally through `sin`/`cos`.
    yaw: f32,
    /// Elevation above the horizon, clamped to ±[`PITCH_LIMIT`].
    pitch: f32,
    /// Eye-to-target distance, clamped to [`MIN_DISTANCE`]..=[`MAX_DISTANCE`].
    distance: f32,
    /// The last transform that resolved successfully. `looking_at` is fallible,
    /// and the pitch clamp is what guarantees it here — but "guaranteed" is not
    /// "assumed": on the impossible error we keep the previous framing rather
    /// than panicking a live page.
    transform: Transform,
}

impl OrbitState {
    /// Seed the orbit from the authored framing in `install.rs`, so the opening
    /// shot is exactly the one the app has always presented. `yaw`, `pitch` and
    /// `distance` are *derived* from that eye/target pair rather than typed out
    /// again — there is one authored camera, and this is a different coordinate
    /// system for it, not a second copy of it.
    pub fn framed() -> OrbitState {
        let eye = Vec3::new(CAMERA_EYE[0], CAMERA_EYE[1], CAMERA_EYE[2]);
        let target = Vec3::new(CAMERA_TARGET[0], CAMERA_TARGET[1], CAMERA_TARGET[2]);
        let offset = eye.subtract(target);
        let distance = offset.length().clamp(MIN_DISTANCE, MAX_DISTANCE);
        let mut state = OrbitState {
            target,
            yaw: offset.x.atan2(offset.z),
            pitch: (offset.y / distance).asin().clamp(-PITCH_LIMIT, PITCH_LIMIT),
            distance,
            transform: Transform::from_translation(eye),
        };
        state.refresh();
        state
    }

    /// Orbit by a drag, in canvas-height units (a full-height drag is 1.0).
    ///
    /// The signs implement the "grab the scene" metaphor: drag right and the
    /// scene follows your finger to the right (the camera swings left); drag
    /// down and the top of the scene tips toward you (the camera rises).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * ORBIT_RADIANS_PER_UNIT;
        self.pitch = (self.pitch + dy * ORBIT_RADIANS_PER_UNIT).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.refresh();
    }

    /// Multiply the orbit distance by `factor` (>1 pulls back, <1 moves in),
    /// clamped to the distance band. A non-finite or non-positive factor is
    /// ignored rather than allowed to poison the state.
    pub fn zoom_by(&mut self, factor: f32) {
        let factor = if factor.is_finite() && factor > 0.0 {
            factor
        } else {
            1.0
        };
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        self.refresh();
    }

    /// Zoom from a wheel event's vertical delta, already converted to pixels.
    /// Positive (scroll away) pulls back. The pixels→factor curve lives here so
    /// the DOM half stays pure plumbing.
    pub fn zoom_by_wheel(&mut self, delta_pixels: f32) {
        self.zoom_by((delta_pixels * WHEEL_ZOOM_PER_PIXEL).exp());
    }

    /// Pan the target across the view plane by a drag in canvas-height units.
    ///
    /// The world distance per unit of drag is proportional to `distance` and to
    /// the vertical field of view, so a drag moves the scene the same *apparent*
    /// amount at every zoom level — at the exact 1:1 rate, a point under your
    /// finger stays under your finger.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let (right, up) = self.basis();
        let world_per_unit = self.distance * pan_units_per_drag();
        self.target = self
            .target
            .add(right.mul_scalar(-dx * world_per_unit))
            .add(up.mul_scalar(dy * world_per_unit));
        self.refresh();
    }

    /// The eye position this state puts the camera at.
    pub fn eye(&self) -> Vec3 {
        let horizontal = self.distance * self.pitch.cos();
        self.target.add(Vec3::new(
            horizontal * self.yaw.sin(),
            self.distance * self.pitch.sin(),
            horizontal * self.yaw.cos(),
        ))
    }

    /// The camera transform for the current state.
    pub fn camera_transform(&self) -> Transform {
        self.transform
    }

    /// Re-author the running app's camera from this state. Cheap enough to call
    /// every frame: `set_camera` reuses the existing camera node in place.
    pub fn apply(&self, running: &mut RunningApp) {
        running.set_camera(crucible_camera(), self.transform);
    }

    /// The current orbit distance, in world units.
    pub fn distance(&self) -> f32 {
        self.distance
    }

    /// The point the camera is orbiting.
    pub fn target(&self) -> Vec3 {
        self.target
    }

    /// Rotation about world +Y, in radians.
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// Elevation above the horizon, in radians.
    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// Unit view direction, from the eye toward the target.
    fn forward(&self) -> Vec3 {
        let horizontal = self.pitch.cos();
        Vec3::new(
            -horizontal * self.yaw.sin(),
            -self.pitch.sin(),
            -horizontal * self.yaw.cos(),
        )
    }

    /// The camera's own right and up axes — the plane a pan slides the target
    /// along. Right-handed look-at: `right = forward × up_world`, `up = right ×
    /// forward`. The pitch clamp keeps `forward` off the pole, so the cross
    /// product is never degenerate; the fallback exists only so a bad state can
    /// never panic a live page.
    fn basis(&self) -> (Vec3, Vec3) {
        let forward = self.forward();
        let right = forward
            .cross(Vec3::UNIT_Y)
            .normalize()
            .unwrap_or(Vec3::UNIT_X);
        (right, right.cross(forward))
    }

    /// Recompute the camera transform, keeping the previous one if the look
    /// direction is somehow degenerate.
    fn refresh(&mut self) {
        if let Ok(transform) =
            Transform::from_translation(self.eye()).looking_at(self.target, Vec3::UNIT_Y)
        {
            self.transform = transform;
        }
    }
}

/// World units of pan per unit of drag, per unit of orbit distance: the height
/// of the view frustum at the target, which is `2·tan(fov_y/2)`. Multiplying by
/// `distance` gives the world height the canvas spans at the target plane, which
/// is what makes a pan track the finger exactly.
fn pan_units_per_drag() -> f32 {
    2.0 * (CAMERA_FOV_DEGREES.to_radians() * 0.5).tan()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A finite-precision comparison for derived angles/positions.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-3
    }

    #[test]
    fn the_seed_reproduces_the_authored_framing_exactly() {
        let state = OrbitState::framed();
        let eye = state.eye();
        assert!(close(eye.x, CAMERA_EYE[0]), "{eye:?}");
        assert!(close(eye.y, CAMERA_EYE[1]), "{eye:?}");
        assert!(close(eye.z, CAMERA_EYE[2]), "{eye:?}");
        assert_eq!(
            state.target(),
            Vec3::new(CAMERA_TARGET[0], CAMERA_TARGET[1], CAMERA_TARGET[2])
        );
        // ...and the transform matches the one `install_camera` authors.
        let authored = Transform::from_translation(Vec3::new(
            CAMERA_EYE[0],
            CAMERA_EYE[1],
            CAMERA_EYE[2],
        ))
        .looking_at(
            Vec3::new(CAMERA_TARGET[0], CAMERA_TARGET[1], CAMERA_TARGET[2]),
            Vec3::UNIT_Y,
        )
        .expect("the authored framing is a valid look direction");
        let live = state.camera_transform();
        assert!(close(live.rotation.x, authored.rotation.x));
        assert!(close(live.rotation.y, authored.rotation.y));
        assert!(close(live.rotation.z, authored.rotation.z));
        assert!(close(live.rotation.w, authored.rotation.w));
    }

    #[test]
    fn pitch_is_clamped_off_both_poles_and_never_flips() {
        let mut state = OrbitState::framed();
        for _ in 0..40 {
            state.orbit(0.0, 1.0);
        }
        assert!(close(state.pitch(), PITCH_LIMIT));
        // Still a valid, non-degenerate look: the eye is above the target and
        // the transform resolved rather than falling back.
        assert!(state.eye().y > state.target().y);
        for _ in 0..80 {
            state.orbit(0.0, -1.0);
        }
        assert!(close(state.pitch(), -PITCH_LIMIT));
        assert!(state.eye().y < state.target().y);
    }

    #[test]
    fn yaw_is_unbounded_and_a_full_turn_returns_the_same_eye() {
        let mut state = OrbitState::framed();
        let start = state.eye();
        // Two full turns' worth of drag: 4.0 canvas-heights = 4π radians.
        for _ in 0..8 {
            state.orbit(0.5, 0.0);
        }
        assert!(state.yaw().abs() > 6.0, "yaw wrapped instead of accumulating");
        let end = state.eye();
        assert!(close(start.x, end.x) && close(start.z, end.z), "{end:?}");
    }

    #[test]
    fn zoom_is_clamped_to_the_distance_band_at_both_ends() {
        let mut state = OrbitState::framed();
        for _ in 0..200 {
            state.zoom_by_wheel(-100.0);
        }
        assert!(close(state.distance(), MIN_DISTANCE));
        for _ in 0..400 {
            state.zoom_by_wheel(100.0);
        }
        assert!(close(state.distance(), MAX_DISTANCE));
        // A degenerate factor is ignored, not absorbed.
        state.zoom_by(f32::NAN);
        assert!(close(state.distance(), MAX_DISTANCE));
        state.zoom_by(0.0);
        assert!(close(state.distance(), MAX_DISTANCE));
    }

    #[test]
    fn zooming_moves_the_eye_but_never_the_target() {
        let mut state = OrbitState::framed();
        let target = state.target();
        let before = state.eye();
        state.zoom_by(0.5);
        assert!(close(state.distance(), before.subtract(target).length() * 0.5));
        assert_eq!(state.target(), target);
        assert!(state.eye().distance(before) > 1.0);
    }

    #[test]
    fn panning_slides_the_target_across_the_view_plane_scaled_by_distance() {
        let mut state = OrbitState::framed();
        let start = state.target();
        state.pan(0.25, 0.0);
        let near_shift = state.target().distance(start);
        assert!(near_shift > 0.0);
        // The same drag at twice the distance moves twice as far in world units.
        let mut far = OrbitState::framed();
        far.zoom_by(2.0);
        let far_start = far.target();
        far.pan(0.25, 0.0);
        assert!(close(far.target().distance(far_start), near_shift * 2.0));
        // A pan never changes the orbit angles or the distance.
        assert!(close(far.distance(), state.distance() * 2.0));
        assert!(close(far.yaw(), state.yaw()));
        assert!(close(far.pitch(), state.pitch()));
    }

    #[test]
    fn a_pan_moves_the_eye_and_the_target_by_the_same_vector() {
        let mut state = OrbitState::framed();
        let eye_before = state.eye();
        let target_before = state.target();
        state.pan(-0.2, 0.3);
        let eye_delta = state.eye().subtract(eye_before);
        let target_delta = state.target().subtract(target_before);
        assert!(close(eye_delta.x, target_delta.x));
        assert!(close(eye_delta.y, target_delta.y));
        assert!(close(eye_delta.z, target_delta.z));
        assert!(target_delta.length() > 1.0);
    }

    #[test]
    fn the_applied_camera_is_the_state_the_gestures_left_behind() {
        let mut state = OrbitState::framed();
        state.orbit(0.3, -0.1);
        state.zoom_by(0.6);
        let mut app = crate::crucible_core(
            crate::CrucibleVariant::Coarse,
            crate::DebugView::Shaded,
        );
        state.apply(&mut app);
        let before = app.tick(0).camera_view_proj();
        state.orbit(0.4, 0.0);
        state.apply(&mut app);
        let after = app.tick(1).camera_view_proj();
        assert_ne!(before, after, "orbiting did not change the camera matrix");
    }
}
