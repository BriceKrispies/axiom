//! The third-person chase camera.
//!
//! A racing chase camera has one job that is much harder than it looks: show the
//! player where the car is **going**, while still feeling attached to the car.
//! Rigidly bolting the camera behind the chassis fails during a drift — the
//! camera swings with the nose and the road disappears out of frame exactly when
//! you need to see it. Following the velocity vector alone fails too — the
//! camera lags every direction change and the steering feels disconnected.
//!
//! So the heading is a **blend**, and the blend is the design:
//!
//! * the chassis nose, so the camera is attached to the car;
//! * the direction of travel, so a slide stays readable;
//! * the road ahead, so a corner is revealed slightly before you arrive;
//! * the steering input, so turning in leads the camera rather than trailing it.
//!
//! Everything else — field of view, chase distance, roll, shake — is a bounded
//! function of speed, boost and impact, smoothed so nothing ever snaps. All of
//! it advances on the **fixed simulation step**: the camera is part of the
//! deterministic state, not something presentation improvises from a wall clock.

use axiom_math::Vec3;

use crate::sim::car::CarState;
use crate::track::{shortest_angle, Track};
use crate::tuning::{CameraTuning, VehicleTuning, DT};

/// Where the camera is and how it is looking, for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    /// Eye position.
    pub eye: Vec3,
    /// Look-at target.
    pub target: Vec3,
    /// Vertical field of view (degrees).
    pub fov_degrees: f32,
    /// Camera roll (radians) about the view direction.
    pub roll: f32,
}

impl CameraPose {
    /// The up vector the engine's `looking_at` should be handed, with the roll
    /// baked in. Rolling the *up vector* is how a `looking_at` camera leans
    /// without a second rotation stage anywhere.
    pub fn up(&self) -> Vec3 {
        let view = self
            .target
            .subtract(self.eye)
            .normalize()
            .unwrap_or(Vec3::UNIT_Z);
        let right = Vec3::UNIT_Y
            .cross(view)
            .normalize()
            .unwrap_or(Vec3::UNIT_X);
        let (s, c) = self.roll.sin_cos();
        Vec3::UNIT_Y
            .mul_scalar(c)
            .add(right.mul_scalar(s))
            .normalize()
            .unwrap_or(Vec3::UNIT_Y)
    }

    /// Interpolate between two poses for a render frame between two sim steps.
    pub fn lerp(a: CameraPose, b: CameraPose, t: f32) -> CameraPose {
        let t = t.clamp(0.0, 1.0);
        CameraPose {
            eye: a.eye.add(b.eye.subtract(a.eye).mul_scalar(t)),
            target: a.target.add(b.target.subtract(a.target).mul_scalar(t)),
            fov_degrees: a.fov_degrees + (b.fov_degrees - a.fov_degrees) * t,
            roll: a.roll + (b.roll - a.roll) * t,
        }
    }
}

/// The chase camera's persistent, deterministic state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChaseCamera {
    eye: Vec3,
    eye_velocity: Vec3,
    heading: f32,
    fov: f32,
    roll: f32,
    impact_shake: f32,
    /// A monotonically advancing phase the vibration is derived from — a
    /// deterministic stand-in for noise, so shake replays exactly.
    shake_phase: f32,
    settled: bool,
}

impl ChaseCamera {
    /// A camera that has not framed anything yet; the first update snaps it into
    /// place rather than springing in from the origin.
    pub const fn new() -> ChaseCamera {
        ChaseCamera {
            eye: Vec3::ZERO,
            eye_velocity: Vec3::ZERO,
            heading: 0.0,
            fov: CameraTuning::DEFAULT.fov_low,
            roll: 0.0,
            impact_shake: 0.0,
            shake_phase: 0.0,
            settled: false,
        }
    }

    /// Drop the camera straight onto its ideal pose for the car's current state.
    /// Used at the start line and after a reset, so the camera does not sweep in
    /// from wherever it happened to be.
    pub fn snap_to(&mut self, car: &CarState, track: &Track, tuning: &CameraTuning) {
        let forward = car.forward();
        self.heading = forward.x.atan2(forward.z);
        self.eye = ideal_eye(car, self.heading, tuning.distance_low, tuning);
        self.eye_velocity = Vec3::ZERO;
        self.fov = tuning.fov_low;
        self.roll = 0.0;
        self.impact_shake = 0.0;
        self.settled = true;
        let _ = track;
    }

    /// Advance the camera one fixed step and return the pose to render.
    pub fn step(
        &mut self,
        car: &CarState,
        track: &Track,
        tuning: &CameraTuning,
        vehicle: &VehicleTuning,
        forward_accel: f32,
        boosting: bool,
    ) -> CameraPose {
        let speed_t = (car.speed() / vehicle.top_speed.max(1.0)).clamp(0.0, 1.0);

        self.advance_heading(car, track, tuning);
        let distance = self.chase_distance(tuning, speed_t, forward_accel, boosting);
        self.advance_position(car, distance, tuning);
        self.advance_fov(tuning, speed_t, boosting);
        self.advance_roll(car, tuning);
        let shake = self.advance_shake(car, tuning, speed_t, boosting);

        let eye = self.clear_of_the_road(self.eye.add(shake), car, track, tuning);
        let look_ahead = tuning.look_ahead_low
            + (tuning.look_ahead_high - tuning.look_ahead_low) * speed_t;
        let target = car
            .position
            .add(car.heading_of_travel().mul_scalar(look_ahead))
            .add(Vec3::new(0.0, TARGET_HEIGHT, 0.0));

        CameraPose {
            eye,
            target,
            fov_degrees: self.fov,
            roll: self.roll,
        }
    }

    /// The blended heading, sprung toward its target.
    fn advance_heading(&mut self, car: &CarState, track: &Track, tuning: &CameraTuning) {
        let chassis = car.yaw;
        let travel = {
            let t = car.heading_of_travel();
            t.x.atan2(t.z)
        };
        // Start at the nose, lean toward where the car is actually going.
        let mut wanted = chassis + shortest_angle(travel - chassis) * tuning.velocity_heading_blend;
        // Then lean toward the road ahead, so a corner opens up slightly early.
        let ahead = track.sample_at(car.distance + tuning.anticipation_distance);
        let track_heading = ahead.heading;
        wanted += shortest_angle(track_heading - wanted) * tuning.track_anticipation;
        // And finally a touch of lead from the steering itself.
        wanted += car.steer * STEER_LEAD;

        if !self.settled {
            self.heading = wanted;
            self.settled = true;
        }
        let k = 1.0 - (-tuning.heading_spring * DT).exp();
        self.heading += shortest_angle(wanted - self.heading) * k;
    }

    /// Chase distance for this step, including the accel/brake pull.
    fn chase_distance(
        &self,
        tuning: &CameraTuning,
        speed_t: f32,
        forward_accel: f32,
        boosting: bool,
    ) -> f32 {
        let base = tuning.distance_low + (tuning.distance_high - tuning.distance_low) * speed_t;
        let boost = if boosting { tuning.distance_boost } else { 0.0 };
        // Accelerating pulls the camera back; braking lets it close up.
        let pull = (forward_accel * tuning.accel_pullback)
            .clamp(-tuning.accel_pullback_limit, tuning.accel_pullback_limit);
        base + boost + pull
    }

    /// Critically damped spring toward the ideal eye position, with the car's
    /// own velocity fed forward.
    ///
    /// The feed-forward is not a refinement, it is the difference between a
    /// working chase camera and a useless one. A plain damped spring chasing a
    /// *moving* target settles at a steady-state lag of roughly `2 * v / omega`:
    /// at 88 m/s with `omega = 11` that is sixteen metres of lag on top of the
    /// intended chase distance, and the car ends up a speck in the middle of the
    /// screen. Damping against the *relative* velocity instead - the difference
    /// between how fast the camera is moving and how fast the car is - leaves
    /// the spring only the residual to correct, so the chase distance at
    /// 320 km/h is the chase distance that was authored.
    fn advance_position(&mut self, car: &CarState, distance: f32, tuning: &CameraTuning) {
        let wanted = ideal_eye(car, self.heading, distance, tuning);
        let omega = tuning.position_spring;
        let relative = self.eye_velocity.subtract(car.velocity());
        let accel = wanted
            .subtract(self.eye)
            .mul_scalar(omega * omega)
            .subtract(relative.mul_scalar(2.0 * omega));
        self.eye_velocity = self.eye_velocity.add(accel.mul_scalar(DT));
        self.eye = self.eye.add(self.eye_velocity.mul_scalar(DT));
    }

    /// Smoothly chase the field of view toward its speed- and boost-driven
    /// target. Never a snap: a stepped field of view reads as a glitch.
    fn advance_fov(&mut self, tuning: &CameraTuning, speed_t: f32, boosting: bool) {
        let natural = tuning.fov_low + (tuning.fov_high - tuning.fov_low) * speed_t;
        let wanted = if boosting {
            natural.max(tuning.fov_boost)
        } else {
            natural
        };
        let k = 1.0 - (-tuning.fov_rate * DT).exp();
        self.fov += (wanted - self.fov) * k;
        self.fov = self.fov.clamp(tuning.fov_low, tuning.fov_boost);
    }

    /// Roll into the turn, hard-limited so the horizon stays readable.
    fn advance_roll(&mut self, car: &CarState, tuning: &CameraTuning) {
        let limit = tuning.turn_roll_limit.to_radians();
        let wanted = (-car.yaw_rate * tuning.turn_roll.to_radians()).clamp(-limit, limit);
        let k = 1.0 - (-ROLL_RATE * DT).exp();
        self.roll += (wanted - self.roll) * k;
    }

    /// The layered, bounded shake: a fine vibration that only exists at real
    /// speed, a little more of it while boosting, and a decaying directional
    /// kick after an impact.
    fn advance_shake(
        &mut self,
        car: &CarState,
        tuning: &CameraTuning,
        speed_t: f32,
        boosting: bool,
    ) -> Vec3 {
        self.shake_phase = (self.shake_phase + SHAKE_RATE * DT).rem_euclid(std::f32::consts::TAU);
        // The vibration is quadratic in speed, so it is genuinely absent at
        // ordinary speeds and only shows up at the top end.
        let vibration = tuning.speed_shake * speed_t * speed_t
            + if boosting { tuning.boost_shake } else { 0.0 };

        // A fresh impact raises the kick; it then decays on its own.
        let fresh = car.impact_strength * tuning.impact_shake;
        self.impact_shake = self.impact_shake.max(fresh);
        self.impact_shake *= (-tuning.impact_decay * DT).exp();

        let p = self.shake_phase;
        // Three incommensurate frequencies read as noise while staying exactly
        // reproducible — no random source anywhere near the camera.
        let wobble = Vec3::new(
            (p * 1.0).sin() + (p * 2.7).sin() * 0.5,
            (p * 1.7).cos() + (p * 3.9).sin() * 0.4,
            (p * 2.3).sin() * 0.6,
        );
        let kick = car
            .impact_direction
            .mul_scalar(self.impact_shake * (p * 5.0).sin());
        wobble.mul_scalar(vibration).add(kick)
    }

    /// Keep the eye above the road surface behind the car.
    ///
    /// This is the whole camera-obstruction story, and it is deliberately not a
    /// general solution: the only geometry a chase camera can realistically be
    /// pushed into here is the road it is following, and the road's height is a
    /// table lookup we already do every step. Building a general camera-collision
    /// system for one case would be a new engine subsystem to justify one metre
    /// of clearance.
    fn clear_of_the_road(
        &self,
        eye: Vec3,
        car: &CarState,
        track: &Track,
        tuning: &CameraTuning,
    ) -> Vec3 {
        let behind = track.interpolated_at((car.distance - tuning.distance_high).max(0.0));
        let floor = behind.position.y + tuning.min_ground_clearance;
        Vec3::new(eye.x, eye.y.max(floor), eye.z)
    }
}

impl Default for ChaseCamera {
    fn default() -> Self {
        ChaseCamera::new()
    }
}

/// Height above the car the look target sits at (m) — aiming a little above the
/// bonnet rather than at the road keeps the horizon in frame.
const TARGET_HEIGHT: f32 = 0.9;

/// Radians of camera lead per unit of steering input.
const STEER_LEAD: f32 = 0.14;

/// How fast the roll chases its target (per second).
const ROLL_RATE: f32 = 6.0;

/// Angular rate the shake phase advances at (rad/s).
const SHAKE_RATE: f32 = 41.0;

/// The ideal eye position for a car, a heading and a chase distance.
fn ideal_eye(car: &CarState, heading: f32, distance: f32, tuning: &CameraTuning) -> Vec3 {
    let (s, c) = heading.sin_cos();
    let back = Vec3::new(-s, 0.0, -c);
    car.position
        .add(back.mul_scalar(distance))
        .add(Vec3::new(0.0, tuning.height, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;
    use crate::sim::controller::{place_on_track, step as drive_step};
    use crate::tuning::CourseTuning;

    fn fixture() -> (Track, CarState, ChaseCamera) {
        let track = Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT);
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        place_on_track(&mut car, &track.sample_at(60.0), 0.0);
        let mut camera = ChaseCamera::new();
        camera.snap_to(&car, &track, &CameraTuning::DEFAULT);
        (track, car, camera)
    }

    /// Run the car and the camera together for `steps`, returning the last pose.
    fn run(
        track: &Track,
        car: &mut CarState,
        camera: &mut ChaseCamera,
        command: DriveCommand,
        steps: u32,
    ) -> CameraPose {
        let t = CameraTuning::DEFAULT;
        let v = VehicleTuning::DEFAULT;
        let mut pose = camera.step(car, track, &t, &v, 0.0, false);
        for _ in 0..steps {
            let report = drive_step(car, command, track, &v, command.boost);
            pose = camera.step(car, track, &t, &v, report.forward_accel, command.boost);
        }
        pose
    }

    #[test]
    fn the_camera_sits_behind_and_above_the_car() {
        let (track, mut car, mut camera) = fixture();
        let pose = run(&track, &mut car, &mut camera, DriveCommand::IDLE, 30);
        let to_camera = pose.eye.subtract(car.position);
        assert!(to_camera.dot(car.forward()) < -3.0, "behind the car");
        assert!(to_camera.y > 1.0, "and above it");
        let planar = Vec3::new(to_camera.x, 0.0, to_camera.z).length();
        let t = CameraTuning::DEFAULT;
        assert!(
            (t.distance_low - 1.5..=t.distance_high + 2.0).contains(&planar),
            "at a sane chase distance: {planar}"
        );
    }

    #[test]
    fn the_camera_looks_ahead_of_the_car() {
        let (track, mut car, mut camera) = fixture();
        let pose = run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 200);
        let ahead = pose.target.subtract(car.position);
        assert!(ahead.dot(car.heading_of_travel()) > 4.0, "the target leads the car");
    }

    #[test]
    fn field_of_view_rises_with_speed_and_stays_inside_its_band() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let parked = run(&track, &mut car, &mut camera, DriveCommand::IDLE, 60);
        let fast = run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 600);
        assert!(
            fast.fov_degrees > parked.fov_degrees + 8.0,
            "speed widens the view: {} -> {}",
            parked.fov_degrees,
            fast.fov_degrees
        );
        assert!((t.fov_low..=t.fov_boost).contains(&fast.fov_degrees));
        assert!((t.fov_low..=t.fov_boost).contains(&parked.fov_degrees));
    }

    #[test]
    fn boosting_widens_the_view_further_than_speed_alone() {
        let (track, mut car, mut camera) = fixture();
        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 600);
        let natural = camera.fov;
        let boosted = run(
            &track,
            &mut car,
            &mut camera,
            DriveCommand { boost: true, ..DriveCommand::FLAT_OUT },
            90,
        );
        assert!(
            boosted.fov_degrees > natural,
            "boost widens further: {natural} -> {}",
            boosted.fov_degrees
        );
        assert!(boosted.fov_degrees <= CameraTuning::DEFAULT.fov_boost + 1.0e-3);
    }

    #[test]
    fn the_field_of_view_never_snaps() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let v = VehicleTuning::DEFAULT;
        let mut previous = camera.step(&car, &track, &t, &v, 0.0, false).fov_degrees;
        for i in 0..900 {
            // Slam between flat out and hard braking, the worst case for a
            // speed-driven field of view.
            let command = if (i / 45) % 2 == 0 {
                DriveCommand { boost: true, ..DriveCommand::FLAT_OUT }
            } else {
                DriveCommand { brake: 1.0, ..DriveCommand::IDLE }
            };
            let report = drive_step(&mut car, command, &track, &v, true);
            let pose = camera.step(&car, &track, &t, &v, report.forward_accel, command.boost);
            let jump = (pose.fov_degrees - previous).abs();
            assert!(jump < 2.0, "step {i} moved the field of view by {jump} degrees");
            previous = pose.fov_degrees;
        }
    }

    #[test]
    fn the_chase_distance_grows_with_speed() {
        let (_, car, camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let slow = camera.chase_distance(&t, 0.0, 0.0, false);
        let fast = camera.chase_distance(&t, 1.0, 0.0, false);
        let boosted = camera.chase_distance(&t, 1.0, 0.0, true);
        assert!(fast > slow, "faster is further back");
        assert!(boosted > fast, "and boosting further still");
        assert_eq!(slow, t.distance_low);
        let _ = car;
    }

    #[test]
    fn acceleration_pulls_back_and_braking_compresses_within_the_limit() {
        let (_, _, camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let neutral = camera.chase_distance(&t, 0.5, 0.0, false);
        let accelerating = camera.chase_distance(&t, 0.5, 40.0, false);
        let braking = camera.chase_distance(&t, 0.5, -60.0, false);
        assert!(accelerating > neutral, "throttle pulls back");
        assert!(braking < neutral, "braking closes up");
        assert!((accelerating - neutral) <= t.accel_pullback_limit + 1.0e-4);
        assert!((neutral - braking) <= t.accel_pullback_limit + 1.0e-4);
        // And an absurd acceleration is still clamped.
        let absurd = camera.chase_distance(&t, 0.5, 1.0e6, false);
        assert!((absurd - neutral) <= t.accel_pullback_limit + 1.0e-4);
    }

    #[test]
    fn the_position_spring_converges_rather_than_oscillating() {
        let (track, car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let v = VehicleTuning::DEFAULT;
        // Displace the camera hard, then let it settle behind a stationary car.
        camera.eye = car.position.add(Vec3::new(120.0, 60.0, -90.0));
        camera.eye_velocity = Vec3::ZERO;
        let mut previous = f32::INFINITY;
        for i in 0..240 {
            camera.step(&car, &track, &t, &v, 0.0, false);
            let error = camera
                .eye
                .subtract(ideal_eye(&car, camera.heading, t.distance_low, &t))
                .length();
            // A critically damped spring never overshoots into a growing error.
            assert!(error < previous + 1.0e-3 || i < 4, "step {i}: {previous} -> {error}");
            previous = error;
        }
        assert!(previous < 0.5, "and it actually arrives: {previous}");
    }

    #[test]
    fn the_camera_follows_travel_rather_than_the_nose_in_a_drift() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let v = VehicleTuning::DEFAULT;
        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 200);
        // Establish a big slide.
        for _ in 0..40 {
            let report = drive_step(
                &mut car,
                DriveCommand { handbrake: true, ..DriveCommand::turning(1.0) },
                &track,
                &v,
                false,
            );
            camera.step(&car, &track, &t, &v, report.forward_accel, false);
        }
        assert!(car.drifting, "the car really is sideways");
        let travel = car.heading_of_travel();
        let travel_yaw = travel.x.atan2(travel.z);
        assert!(
            shortest_angle(travel_yaw - car.yaw).abs() > 0.1,
            "and the nose and the travel direction genuinely disagree"
        );

        // Release the steering first: the camera's steering *lead* is a separate
        // term of the same size as the effect being measured, so leaving full
        // lock applied would drown the blend it is trying to isolate. The car is
        // still sliding, which is all the test needs.
        car.steer = 0.0;

        // Run the identical drift through two cameras that differ ONLY in the
        // velocity blend. The blended one must end up nearer the direction of
        // travel — that is the design claim, isolated from the road's own
        // curvature and the steering lead, which move both cameras equally.
        let settle = |blend: f32| {
            let mut c = ChaseCamera::new();
            c.snap_to(&car, &track, &t);
            let tuned = CameraTuning { velocity_heading_blend: blend, ..t };
            for _ in 0..90 {
                c.step(&car, &track, &tuned, &v, 0.0, false);
            }
            shortest_angle(travel_yaw - c.heading).abs()
        };
        let blended = settle(t.velocity_heading_blend);
        let nose_locked = settle(0.0);
        assert!(
            blended < nose_locked,
            "the velocity blend pulls the camera toward the slide: {blended} vs {nose_locked}"
        );
    }

    #[test]
    fn roll_stays_inside_its_limit_even_at_full_lock() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let limit = t.turn_roll_limit.to_radians();
        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 120);
        for _ in 0..600 {
            let pose = run(&track, &mut car, &mut camera, DriveCommand::turning(1.0), 1);
            assert!(
                pose.roll.abs() <= limit + 1.0e-4,
                "roll {} exceeded the {limit} rad limit",
                pose.roll
            );
        }
    }

    #[test]
    fn an_impact_kicks_the_camera_and_the_kick_decays() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let v = VehicleTuning::DEFAULT;
        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 200);
        car.impact_strength = 1.0;
        car.impact_direction = Vec3::UNIT_X;
        camera.step(&car, &track, &t, &v, 0.0, false);
        let kicked = camera.impact_shake;
        assert!(kicked > 0.0, "the hit registered");
        car.impact_strength = 0.0;
        for _ in 0..90 {
            camera.step(&car, &track, &t, &v, 0.0, false);
        }
        assert!(
            camera.impact_shake < kicked * 0.05,
            "and it recovers quickly: {} -> {}",
            kicked,
            camera.impact_shake
        );
    }

    #[test]
    fn shake_is_bounded_and_absent_at_a_standstill() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let still = camera.advance_shake(&car, &t, 0.0, false);
        assert!(still.length() < 1.0e-6, "a parked car does not vibrate");

        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 600);
        for _ in 0..600 {
            let shake = camera.advance_shake(&car, &t, 1.0, true);
            // Three unit-amplitude terms, so the bound is generous but real.
            let bound = (t.speed_shake + t.boost_shake) * 3.0 + t.impact_shake;
            assert!(shake.length() <= bound, "shake {shake:?} exceeded {bound}");
        }
    }

    /// The bug the velocity feed-forward exists to prevent: at racing speed the
    /// camera must sit at its authored chase distance, not sixteen metres
    /// further back because a spring is chasing a moving target.
    #[test]
    fn the_chase_distance_holds_at_racing_speed() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let v = VehicleTuning::DEFAULT;
        for _ in 0..900 {
            let command = crate::script::autopilot(&car, &track);
            let report = drive_step(&mut car, command, &track, &v, false);
            camera.step(&car, &track, &t, &v, report.forward_accel, false);
        }
        assert!(car.speed() > 70.0, "the test is at speed: {}", car.speed());

        let pose = camera.step(&car, &track, &t, &v, 0.0, false);
        let offset = pose.eye.subtract(car.position);
        let planar = Vec3::new(offset.x, 0.0, offset.z).length();
        assert!(
            planar < t.distance_high + 3.0,
            "the camera is {planar} m back; the authored maximum is {}",
            t.distance_high
        );
        assert!(planar > t.distance_low - 3.0, "and it has not run into the car");
        assert!(
            (offset.y - t.height).abs() < 2.5,
            "and it is near the authored height: {}",
            offset.y
        );
    }

    #[test]
    fn the_camera_never_drops_below_the_road() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let v = VehicleTuning::DEFAULT;
        for i in 0..4_000 {
            let steer = ((i as f32) * 0.01).sin();
            let report = drive_step(&mut car, DriveCommand::turning(steer), &track, &v, true);
            let pose = camera.step(&car, &track, &t, &v, report.forward_accel, true);
            let behind = track.interpolated_at((car.distance - t.distance_high).max(0.0));
            assert!(
                pose.eye.y >= behind.position.y + t.min_ground_clearance - 1.0e-3,
                "step {i}: eye {} vs road {}",
                pose.eye.y,
                behind.position.y
            );
            assert!(pose.eye.x.is_finite() && pose.eye.y.is_finite() && pose.eye.z.is_finite());
        }
    }

    #[test]
    fn snapping_places_the_camera_without_a_sweep() {
        let (track, car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        camera.eye = Vec3::new(500.0, 500.0, 500.0);
        camera.eye_velocity = Vec3::new(90.0, 0.0, 0.0);
        camera.snap_to(&car, &track, &t);
        assert_eq!(camera.eye_velocity, Vec3::ZERO);
        assert_eq!(camera.fov, t.fov_low);
        assert_eq!(camera.roll, 0.0);
        assert!(camera.eye.distance(car.position) < t.distance_low + t.height + 1.0);
    }

    #[test]
    fn the_rolled_up_vector_stays_unit_and_leans_the_right_way() {
        let pose = CameraPose {
            eye: Vec3::new(0.0, 2.0, -8.0),
            target: Vec3::new(0.0, 1.0, 0.0),
            fov_degrees: 70.0,
            roll: 0.0,
        };
        let upright = pose.up();
        assert!((upright.length() - 1.0).abs() < 1.0e-5);
        assert!(upright.y > 0.99, "no roll is straight up");

        let rolled = CameraPose { roll: 0.3, ..pose }.up();
        assert!((rolled.length() - 1.0).abs() < 1.0e-5);
        assert!(rolled.y < upright.y, "rolling tips the up vector over");
        assert!(rolled.x.abs() > 0.1);
    }

    #[test]
    fn a_degenerate_pose_still_produces_a_usable_up_vector() {
        let pose = CameraPose {
            eye: Vec3::ZERO,
            target: Vec3::ZERO,
            fov_degrees: 70.0,
            roll: 0.0,
        };
        assert!((pose.up().length() - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn pose_interpolation_is_bounded_at_both_ends() {
        let a = CameraPose {
            eye: Vec3::ZERO,
            target: Vec3::UNIT_Z,
            fov_degrees: 60.0,
            roll: 0.0,
        };
        let b = CameraPose {
            eye: Vec3::new(10.0, 4.0, 2.0),
            target: Vec3::new(0.0, 0.0, 30.0),
            fov_degrees: 90.0,
            roll: 0.2,
        };
        assert_eq!(CameraPose::lerp(a, b, 0.0), a);
        assert_eq!(CameraPose::lerp(a, b, 1.0), b);
        assert_eq!(CameraPose::lerp(a, b, -3.0), a);
        assert_eq!(CameraPose::lerp(a, b, 7.0), b);
        let mid = CameraPose::lerp(a, b, 0.5);
        assert!((mid.fov_degrees - 75.0).abs() < 1.0e-5);
        assert!((mid.eye.x - 5.0).abs() < 1.0e-5);
    }

    #[test]
    fn the_default_camera_is_a_fresh_one() {
        assert_eq!(ChaseCamera::default(), ChaseCamera::new());
    }
}
