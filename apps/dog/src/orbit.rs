//! The interactive orbit camera — **app policy**, not an engine capability.
//!
//! A scene whose whole point is that every triangle came out of an operator
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
//!
//! # The lock
//!
//! The camera can be held still — see [`CameraLock`]. That bit lives **on this
//! state**, not at the browser edge, and the reason is arithmetic: there are four
//! ways a gesture reaches the camera (drag, shift/right-drag pan, wheel, pinch)
//! and a check per path is four chances to miss one, today or the next time a
//! gesture is added. A locked [`OrbitState`] cannot be moved by *any* caller, so
//! the guarantee holds for callers that do not exist yet.
//!
//! What the browser additionally does while locked — stop calling
//! `preventDefault()` and give the canvas's `touch-action` back, so the page
//! scrolls normally under the finger — is in `src/pointer_input.rs`, because it
//! is a fact about the DOM and not about a camera.

use axiom::prelude::*;

use crate::camera_lock::CameraLock;
use crate::install::{scene_camera, CAMERA_FOV_DEGREES};
use crate::stage::Stage;

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
/// framing distance, so it pulls back well past the whole field while staying
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

/// A ray into the scene, in world space — what a point on the canvas *means*.
///
/// This is the camera's other job. A locked camera stops being something the
/// user moves and becomes something they reach through: the dogs are draggable
/// (see `src/herd.rs`) and the only way a pointer position can name one is by
/// being turned back into a ray through the same lens the frame was drawn with.
/// It lives here because that inversion is made of the eye, the basis and the
/// field of view, which are exactly what this type already owns.
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    /// Where the ray starts: the eye.
    pub origin: Vec3,
    /// Unit direction into the scene.
    pub direction: Vec3,
}

impl Ray {
    /// Where this ray crosses the horizontal plane at height `y`, if it crosses
    /// it in front of the eye at all.
    ///
    /// A ray parallel to the plane (a camera at the horizon) meets it nowhere,
    /// and one crossing it *behind* the eye is the user pointing at sky. Both
    /// are `None` rather than a large or negative number pretending to be a
    /// position.
    pub fn on_plane(&self, y: f32) -> Option<Vec3> {
        let slope = self.direction.y;
        (slope.abs() > 1.0e-5)
            .then(|| (y - self.origin.y) / slope)
            .filter(|distance| *distance > 0.0)
            .map(|distance| self.origin.add(self.direction.mul_scalar(distance)))
    }

    /// How far along the ray `point` is at its closest, and how far off the ray
    /// it is there. A point behind the eye reports its distance at the eye, so a
    /// caller filtering on `distance > 0` drops it.
    pub fn approach(&self, point: Vec3) -> (f32, f32) {
        let along = point.subtract(self.origin).dot(self.direction);
        let nearest = self.origin.add(self.direction.mul_scalar(along.max(0.0)));
        (along, point.distance(nearest))
    }
}

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
    /// Whether the gestures reach the camera at all. While this holds, every
    /// mutator below is a no-op and the framing is exactly the one the lock
    /// caught it on.
    lock: CameraLock,
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
        OrbitState::for_stage(Stage::Field)
    }

    /// Seed the orbit from `stage`'s own authored framing — the wide shot over
    /// the whole field, or the close one on the single still dog.
    ///
    /// This is the whole of what a stage button does to the camera: the two
    /// framings are authored once each (`install.rs` and `study.rs`) and named
    /// by [`Stage::framing`], and switching stage re-seeds the orbit from the
    /// other one. Everything after that is the user's gestures, on both stages
    /// alike — the study is inspected with exactly the camera the field is.
    pub fn for_stage(stage: Stage) -> OrbitState {
        let (eye, target) = stage.framing();
        let eye = Vec3::new(eye[0], eye[1], eye[2]);
        let target = Vec3::new(target[0], target[1], target[2]);
        let offset = eye.subtract(target);
        let distance = offset.length().clamp(MIN_DISTANCE, MAX_DISTANCE);
        let mut state = OrbitState {
            target,
            yaw: offset.x.atan2(offset.z),
            pitch: (offset.y / distance).asin().clamp(-PITCH_LIMIT, PITCH_LIMIT),
            distance,
            lock: CameraLock::Free,
            transform: Transform::from_translation(eye),
        };
        state.refresh();
        state
    }

    /// This orbit, seeded as it was, but locked or free as `lock` says.
    ///
    /// Re-seeding is how both the opening frame and the stage switch reach a new
    /// framing, and neither of them is a *gesture* — the lock stops the user
    /// moving the camera, it does not stop the page choosing which shot to open
    /// on. Carrying the bit across the seed is what keeps a locked page locked
    /// through a stage change and through the reload the detail dial triggers.
    pub fn with_lock(mut self, lock: CameraLock) -> OrbitState {
        self.lock = lock;
        self
    }

    /// Whether the camera is currently answering gestures.
    pub fn lock(&self) -> CameraLock {
        self.lock
    }

    /// Flip the lock, and report the state the flip landed on — which is what
    /// the button that pressed it has to relabel itself with.
    pub fn toggle_lock(&mut self) -> CameraLock {
        self.lock = self.lock.toggled();
        self.lock
    }

    /// Orbit by a drag, in canvas-height units (a full-height drag is 1.0).
    ///
    /// The signs implement the "grab the scene" metaphor: drag right and the
    /// scene follows your finger to the right (the camera swings left); drag
    /// down and the top of the scene tips toward you (the camera rises).
    ///
    /// A locked camera ignores the drag entirely.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        if self.lock.holds() {
            return;
        }
        self.yaw -= dx * ORBIT_RADIANS_PER_UNIT;
        self.pitch = (self.pitch + dy * ORBIT_RADIANS_PER_UNIT).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.refresh();
    }

    /// Multiply the orbit distance by `factor` (>1 pulls back, <1 moves in),
    /// clamped to the distance band. A non-finite or non-positive factor is
    /// ignored rather than allowed to poison the state, and so is any factor at
    /// all while the camera is locked.
    pub fn zoom_by(&mut self, factor: f32) {
        if self.lock.holds() {
            return;
        }
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
    ///
    /// A locked camera ignores the drag entirely.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if self.lock.holds() {
            return;
        }
        let (right, up) = self.basis();
        let world_per_unit = self.distance * pan_units_per_drag();
        self.target = self
            .target
            .add(right.mul_scalar(-dx * world_per_unit))
            .add(up.mul_scalar(dy * world_per_unit));
        self.refresh();
    }

    /// The world ray through a point on the canvas, given in **normalised
    /// device coordinates** — `(-1, -1)` bottom-left, `(1, 1)` top-right — and
    /// the canvas's width-over-height aspect.
    ///
    /// This is the exact inverse of the projection the frame was drawn with:
    /// the same [`CAMERA_FOV_DEGREES`] lens, the same right/up basis a pan
    /// slides along, and the same eye. Anything else would make the dog under
    /// the pointer and the dog the app picks two different animals.
    pub fn ray(&self, ndc_x: f32, ndc_y: f32, aspect: f32) -> Ray {
        let extent = (CAMERA_FOV_DEGREES.to_radians() * 0.5).tan();
        let (right, up) = self.basis();
        let direction = self
            .forward()
            .add(right.mul_scalar(ndc_x * extent * aspect.max(1.0e-3)))
            .add(up.mul_scalar(ndc_y * extent));
        Ray {
            origin: self.eye(),
            direction: direction.normalize().unwrap_or(self.forward()),
        }
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
        running.set_camera(scene_camera(), self.transform);
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

    use crate::install::{CAMERA_EYE, CAMERA_TARGET};

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
    fn each_stage_seeds_its_own_framing_and_the_study_is_the_close_one() {
        let field = OrbitState::for_stage(Stage::Field);
        let study = OrbitState::for_stage(Stage::Study);
        // `framed()` is the field stage, not a second copy of it.
        assert!(close(field.distance(), OrbitState::framed().distance()));
        let (eye, target) = Stage::Study.framing();
        assert!(close(study.eye().x, eye[0]), "{:?}", study.eye());
        assert!(close(study.eye().y, eye[1]), "{:?}", study.eye());
        assert!(close(study.eye().z, eye[2]), "{:?}", study.eye());
        assert_eq!(study.target(), Vec3::new(target[0], target[1], target[2]));
        // Close enough to inspect one animal, and still outside it — a full
        // orbit at this distance never puts the eye inside the dog.
        assert!(study.distance() > 12.0 && study.distance() < 30.0);
        assert!(study.distance() * 4.0 < field.distance());
        // ...and it is still an ordinary orbit: the user can pull all the way
        // back out to the field's own distance from it.
        let mut opened = study.clone();
        (0..300).for_each(|_| opened.zoom_by_wheel(100.0));
        assert!(opened.distance() > field.distance());
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
    fn the_pick_ray_is_the_exact_inverse_of_the_projection_the_frame_draws_with() {
        // The one claim that decides whether dragging a dog works at all, and
        // the one a screenshot cannot settle: the ray under a pointer has to
        // invert the *same* matrix the GPU drew the frame with. A flipped
        // right-hand axis, a forgotten aspect or a `y` counted the other way up
        // all still produce a plausible ray — one that picks the wrong animal.
        //
        // So the loop is closed against the engine itself: take a world point,
        // put it through the frame's own view-projection to find the pixel it
        // lands on, and demand that the ray through that pixel comes back to the
        // point.
        let config = crate::SceneConfig::defaults();
        let mut app = crate::headless_app(
            crate::SceneVariant::Coarse,
            crate::DebugView::Shaded,
            &config,
        );
        let state = OrbitState::framed();
        state.apply(&mut app);
        let view_proj = Mat4::from_cols_array(app.tick(0).camera_view_proj());
        let aspect = crate::WIDTH as f32 / crate::HEIGHT as f32;

        // Points spread across the field, including well off centre where a
        // sign or aspect error is largest.
        for point in [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(60.0, 2.0, 0.0),
            Vec3::new(-60.0, 2.0, 0.0),
            Vec3::new(0.0, 2.0, 70.0),
            Vec3::new(-45.0, 6.0, -55.0),
        ] {
            let clip = view_proj.transform_vec4(Vec4::new(point.x, point.y, point.z, 1.0));
            assert!(clip.w > 0.0, "{point:?} is behind the camera");
            let ray = state.ray(clip.x / clip.w, clip.y / clip.w, aspect);
            let (along, off) = ray.approach(point);
            assert!(along > 0.0, "the ray points away from {point:?}");
            assert!(
                off < 0.05,
                "the ray through {point:?}'s own pixel misses it by {off}"
            );
        }

        // ...and a pixel off to one side is *not* the middle of the screen: a
        // ray that ignored its coordinates would pass the test above for the
        // centre point alone.
        let middle = state.ray(0.0, 0.0, aspect);
        let corner = state.ray(0.9, 0.9, aspect);
        assert!(middle.direction.distance(corner.direction) > 0.3);
    }

    #[test]
    fn a_locked_camera_ignores_every_gesture_and_a_freed_one_takes_them_all_again() {
        let free = OrbitState::framed();
        let mut locked = free.clone().with_lock(CameraLock::Locked);
        assert!(locked.lock().holds());
        // Every path into the camera, all of them ignored: drag, pan, wheel and
        // the raw factor a pinch calls.
        locked.orbit(0.4, -0.25);
        locked.pan(0.3, 0.3);
        locked.zoom_by_wheel(-400.0);
        locked.zoom_by(0.25);
        assert!(close(locked.yaw(), free.yaw()));
        assert!(close(locked.pitch(), free.pitch()));
        assert!(close(locked.distance(), free.distance()));
        assert_eq!(locked.target(), free.target());
        assert_eq!(locked.eye(), free.eye());

        // Unlocking is not a ratchet: the same gestures land normally after it,
        // from exactly where the lock caught the shot.
        assert!(!locked.toggle_lock().holds());
        locked.orbit(0.4, -0.25);
        locked.zoom_by(0.25);
        assert!(!close(locked.yaw(), free.yaw()));
        assert!(close(locked.distance(), free.distance() * 0.25));
        // ...and locking again holds the *new* shot, not the one it opened on.
        let held = locked.toggle_lock();
        assert!(held.holds());
        let eye = locked.eye();
        locked.orbit(1.0, 0.0);
        assert_eq!(locked.eye(), eye);
    }

    #[test]
    fn the_lock_survives_a_stage_re_seed_but_the_framing_is_the_new_stage_s() {
        // The stage switch re-seeds the orbit (see `stage_input.rs`). A locked
        // page must come back locked, on the shot the new stage authored — the
        // lock stops the *user* moving the camera, not the page choosing where
        // to open.
        let held = OrbitState::framed().with_lock(CameraLock::Locked).lock();
        let locked_study = OrbitState::for_stage(Stage::Study).with_lock(held);
        assert!(locked_study.lock().holds());
        let (eye, _) = Stage::Study.framing();
        assert!(close(locked_study.eye().x, eye[0]));
        assert!(close(locked_study.eye().y, eye[1]));
        assert!(close(locked_study.eye().z, eye[2]));
        // A fresh orbit is free: nothing is locked unless something locked it.
        assert!(!OrbitState::for_stage(Stage::Field).lock().holds());
        assert!(!OrbitState::framed().lock().holds());
    }

    #[test]
    fn a_locked_camera_holds_the_matrix_the_frame_draws_with() {
        let mut app = crate::headless_app(
            crate::SceneVariant::Coarse,
            crate::DebugView::Shaded,
            &crate::SceneConfig::defaults(),
        );
        let mut state = OrbitState::framed().with_lock(CameraLock::Locked);
        state.apply(&mut app);
        let before = app.tick(0).camera_view_proj();
        // The gestures that moved the matrix in the test above.
        state.orbit(0.4, 0.0);
        state.zoom_by(0.6);
        state.apply(&mut app);
        assert_eq!(before, app.tick(1).camera_view_proj(), "a locked camera moved");
        // And the lock is the only thing holding it: freed, the same gestures
        // change the matrix the very next frame.
        state.toggle_lock();
        state.orbit(0.4, 0.0);
        state.apply(&mut app);
        assert_ne!(before, app.tick(2).camera_view_proj());
    }

    #[test]
    fn the_applied_camera_is_the_state_the_gestures_left_behind() {
        let mut state = OrbitState::framed();
        state.orbit(0.3, -0.1);
        state.zoom_by(0.6);
        let mut app = crate::headless_app(
            crate::SceneVariant::Coarse,
            crate::DebugView::Shaded,
            &crate::SceneConfig::defaults(),
        );
        state.apply(&mut app);
        let before = app.tick(0).camera_view_proj();
        state.orbit(0.4, 0.0);
        state.apply(&mut app);
        let after = app.tick(1).camera_view_proj();
        assert_ne!(before, after, "orbiting did not change the camera matrix");
    }
}
