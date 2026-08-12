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

/// What the car is doing to the camera this fixed step.
///
/// Grouped rather than passed as three loose arguments because they are one
/// idea — the frame's *drive state*, as opposed to the car's pose, which the
/// camera reads from [`CarState`] directly.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CameraDrive {
    /// Forward acceleration over the step (m/s²) — the chase pull-back.
    pub forward_accel: f32,
    /// Whether boost is being spent.
    pub boosting: bool,
    /// A collision resolved this step, if there was one.
    pub impact: Option<ImpactImpulse>,
    /// Whether the car's lateral position is *chosen* rather than driven — the
    /// lane game.
    ///
    /// It changes what the camera is framing. Chasing the car is right when the
    /// car's line is the thing the player is producing; on rails the line is
    /// picked from three options and the interesting picture is **the road with
    /// the car somewhere in it**. See [`ChaseCamera::framed_position`].
    pub lane_locked: bool,
}

/// A one-shot camera kick from a collision resolved this fixed step.
///
/// Deliberately *not* a state the camera reads off the car: an impulse happens
/// once, and one collision must produce exactly one of these however many fixed
/// steps the two bodies stay overlapped. See [`crate::sim::contact`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpactImpulse {
    /// World direction the car was shoved.
    pub direction: Vec3,
    /// Kick amplitude, `0..1`, scaled by [`CameraTuning::impact_shake`].
    pub amplitude: f32,
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
    impact_direction: Vec3,
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
            impact_direction: Vec3::UNIT_Z,
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
        // Snapped onto the car rather than onto the framed subject, and that is
        // correct for both games: a snap only happens at the line or after a
        // reset, and both of those put the car on the centreline, where the two
        // are the same point.
        self.eye = ideal_eye(car.position, self.heading, tuning.distance_low, tuning.height);
        self.eye_velocity = Vec3::ZERO;
        self.fov = tuning.fov_low;
        self.roll = 0.0;
        self.impact_shake = 0.0;
        self.impact_direction = Vec3::UNIT_Z;
        self.settled = true;
        let _ = track;
    }

    /// Advance the camera one fixed step and return the pose to render.
    ///
    /// `impact` is the *impulse* from a collision resolved this step, and it is
    /// passed in rather than read off the car deliberately. The camera used to
    /// take its kick from `car.impact_strength`, which is a value held raised
    /// for the whole time an impact is "ringing" — so `max`-ing against it every
    /// step re-armed the shake continuously and produced a long flat rattle
    /// instead of a hit. An impulse arrives once, and what the player sees after
    /// that is the decay, which is what reads as force.
    pub fn step(
        &mut self,
        car: &CarState,
        track: &Track,
        tuning: &CameraTuning,
        vehicle: &VehicleTuning,
        drive: CameraDrive,
    ) -> CameraPose {
        let CameraDrive {
            forward_accel,
            boosting,
            impact,
            lane_locked,
        } = drive;

        // How fast the camera thinks the car is going, which drives the chase
        // distance, the field of view, the look-ahead and the shake.
        //
        // `car.speed()` is the magnitude of the *whole* planar velocity, lateral
        // included — right for a drifting car, which genuinely is travelling
        // that fast in that direction, and badly wrong for a crossing one. A
        // lane change now runs the lateral channel at up to
        // `sim::rails::LANE_CROSS_SPEED`, which on its own is most of the car's
        // top speed: a stationary car dodging read as a car at 92 m/s, and the
        // rig pulled back, the field of view opened and the shake came on for
        // three frames every time the player touched a lane button.
        let framed_speed = [car.speed(), car.forward_speed.abs()][usize::from(lane_locked)];
        let speed_t = (framed_speed / vehicle.top_speed.max(1.0)).clamp(0.0, 1.0);

        // What the camera is watching, and how fast that thing is moving. On
        // rails both are the *road*, which is what keeps a lane change out of
        // the camera entirely — see `framed_position`.
        let framed = Self::framed_position(car, track, lane_locked);
        let framed_velocity = Self::framed_velocity(car, lane_locked);

        self.advance_heading(car, track, tuning, lane_locked);
        let distance =
            self.chase_distance(tuning, speed_t, forward_accel, boosting) * pullback(lane_locked);
        self.advance_position(framed, framed_velocity, distance, tuning, lane_locked);
        self.advance_fov(tuning, speed_t, boosting);
        self.advance_roll(car, tuning);
        let shake = self.advance_shake(impact, tuning, speed_t, boosting);

        let eye = self.clear_of_the_road(self.eye.add(shake), car, track, tuning);
        let look_ahead = tuning.look_ahead_low
            + (tuning.look_ahead_high - tuning.look_ahead_low) * speed_t;
        // The look-at follows the framed subject too. Using the car's direction
        // of *travel* here is right for a drifting car and catastrophic for a
        // crossing one: mid-lane-change the lateral component is most of the
        // velocity, so the aim point swings tens of degrees off the road and
        // back inside three frames.
        let ahead = Self::framed_heading(car, track, lane_locked);
        let target = framed
            .add(ahead.mul_scalar(look_ahead))
            .add(Vec3::new(0.0, tuning.target_height, 0.0));

        CameraPose {
            eye,
            target,
            fov_degrees: self.fov,
            roll: self.roll,
        }
    }

    /// The point the camera is framing.
    ///
    /// The wheel game frames the car, because where the car is across the road
    /// is the thing the player is producing and the camera's job is to show it.
    ///
    /// **The lane game frames the road.** The car's lateral position there is
    /// not a result, it is a choice out of three, and it now changes in about
    /// three frames — so a camera rigidly bolted to it is a camera yanked 3.5 m
    /// sideways every time the player dodges. Pinning the rig to the centreline
    /// puts the lane change back where it belongs: the car moves *within* the
    /// frame, the road stays put, and the picture reads as the car dodging
    /// rather than as the world lurching.
    ///
    /// The height stays the car's, so crests and dips still move the camera —
    /// it is the *lateral* channel that is being taken away from it, and nothing
    /// else.
    fn framed_position(car: &CarState, track: &Track, lane_locked: bool) -> Vec3 {
        let centre = track.interpolated_at(car.distance).position;
        [
            car.position,
            Vec3::new(centre.x, car.position.y, centre.z),
        ][usize::from(lane_locked)]
    }

    /// The velocity of whatever [`Self::framed_position`] is following.
    ///
    /// This is fed forward into the position spring, and it is the single most
    /// violent term if it is wrong: the spring damps against *relative*
    /// velocity, so handing it 85 m/s of lateral during a lane change tells it
    /// the subject has just launched sideways at 300 km/h and it accelerates the
    /// eye to match. On rails the framed subject is a point sliding along the
    /// centreline, and its velocity is the forward component alone.
    fn framed_velocity(car: &CarState, lane_locked: bool) -> Vec3 {
        let along = car
            .forward()
            .mul_scalar(car.forward_speed)
            .add(Vec3::new(0.0, car.vertical_speed, 0.0));
        [car.velocity(), along][usize::from(lane_locked)]
    }

    /// The direction the framed subject is heading, for the look-at lead.
    fn framed_heading(car: &CarState, track: &Track, lane_locked: bool) -> Vec3 {
        let road = track.interpolated_at(car.distance).flat_forward();
        [car.heading_of_travel(), road][usize::from(lane_locked)]
    }

    /// The blended heading, sprung toward its target.
    fn advance_heading(
        &mut self,
        car: &CarState,
        track: &Track,
        tuning: &CameraTuning,
        lane_locked: bool,
    ) {
        let chassis = car.yaw;
        let travel = {
            let t = car.heading_of_travel();
            t.x.atan2(t.z)
        };
        // Start at the nose, lean toward where the car is actually going.
        //
        // Both of those inputs are lane-change noise on rails. `travel` swings
        // toward the crossing direction — the lateral speed is comparable to the
        // forward one for a few frames, so this is tens of degrees — and
        // `chassis` carries the cosmetic lean the lane solver paints on the nose.
        // Neither is a fact about where the road goes, and a camera that yaws to
        // follow them is a camera that whips sideways and back on every dodge.
        // So on rails the blend is simply switched off, leaving the road below.
        let blend = tuning.velocity_heading_blend * f32::from(!lane_locked);
        let mut wanted = chassis + shortest_angle(travel - chassis) * blend;
        // Then lean toward the road ahead, so a corner opens up slightly early.
        let ahead = track.sample_at(car.distance + tuning.anticipation_distance);
        let track_heading = ahead.heading;
        // On rails this is the *whole* heading rather than a lean on top of the
        // nose: `anticipation` of 1 pulls `wanted` all the way onto the road.
        let anticipation = [tuning.track_anticipation, 1.0][usize::from(lane_locked)];
        wanted += shortest_angle(track_heading - wanted) * anticipation;
        // And finally a touch of lead from the steering itself — which on rails
        // is the lean again, so it is dropped for the same reason.
        wanted += car.steer * STEER_LEAD * f32::from(!lane_locked);

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
    fn advance_position(
        &mut self,
        framed: Vec3,
        framed_velocity: Vec3,
        distance: f32,
        tuning: &CameraTuning,
        lane_locked: bool,
    ) {
        let wanted = ideal_eye(framed, self.heading, distance, rig_height(tuning, lane_locked));
        let omega = tuning.position_spring;
        let relative = self.eye_velocity.subtract(framed_velocity);
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
    /// speed, a little more of it while boosting, and a **decaying directional
    /// kick** delivered once per impact.
    ///
    /// The perceived duration of the kick falls out of the decay rather than
    /// being authored per severity: with an exponential decay at
    /// [`CameraTuning::impact_decay`], the time an amplitude `a` stays above the
    /// perceptual floor is `ln(a / floor) / decay`, so the three severities'
    /// amplitudes produce roughly 0.10 s, 0.22 s and 0.30 s of visible shake
    /// without a second set of numbers that could disagree with the first.
    fn advance_shake(
        &mut self,
        impact: Option<ImpactImpulse>,
        tuning: &CameraTuning,
        speed_t: f32,
        boosting: bool,
    ) -> Vec3 {
        self.shake_phase = (self.shake_phase + SHAKE_RATE * DT).rem_euclid(std::f32::consts::TAU);
        // The vibration is quadratic in speed, so it is genuinely absent at
        // ordinary speeds and only shows up at the top end.
        let vibration = tuning.speed_shake * speed_t * speed_t
            + if boosting { tuning.boost_shake } else { 0.0 };

        // An impulse arrives at most once per collision; everything after it is
        // decay. A sustained overlap delivers no further impulses, which is why
        // grinding along a car no longer rattles the camera indefinitely.
        if let Some(pulse) = impact {
            self.impact_shake = self
                .impact_shake
                .max(pulse.amplitude.clamp(0.0, 1.0) * tuning.impact_shake);
            self.impact_direction = pulse.direction.normalize().unwrap_or(Vec3::UNIT_Z);
        }
        self.impact_shake *= (-tuning.impact_decay * DT).exp();

        let p = self.shake_phase;
        // Three incommensurate frequencies read as noise while staying exactly
        // reproducible — no random source anywhere near the camera.
        let wobble = Vec3::new(
            (p * 1.0).sin() + (p * 2.7).sin() * 0.5,
            (p * 1.7).cos() + (p * 3.9).sin() * 0.4,
            (p * 2.3).sin() * 0.6,
        );
        let kick = self
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

/// Radians of camera lead per unit of steering input.
const STEER_LEAD: f32 = 0.14;

/// How fast the roll chases its target (per second).
const ROLL_RATE: f32 = 6.0;

/// Angular rate the shake phase advances at (rad/s).
const SHAKE_RATE: f32 = 41.0;

/// How much further back the lane game's rig sits than the wheel game's.
///
/// This is a **framing** number, not a preference, and it follows directly from
/// what the rig is now pointed at. A camera bolted to the car only ever has to
/// hold one car; a camera bolted to the centreline has to hold the whole band of
/// road the car can be anywhere in — up to two lanes either side of the point it
/// is aimed at, which is 7 m of lateral spread that simply did not exist before.
/// At the old distance the outer lanes sit at the very edge of the frame, and
/// the traffic you are about to thread is off-screen until it is too late.
///
/// Applied to the chase *height* as well as the distance, so the rig keeps the
/// `height / distance` ratio — the angle it looks down at the road — that the
/// composition was authored at. `CameraTuning::stretched` does the same thing
/// for the same reason.
const LANE_FRAME_PULLBACK: f32 = 1.75;

/// The pullback factor for a profile: [`LANE_FRAME_PULLBACK`] on rails, none on
/// the wheel game.
fn pullback(lane_locked: bool) -> f32 {
    [1.0, LANE_FRAME_PULLBACK][usize::from(lane_locked)]
}

/// The highest the eye may ever sit above the road (m).
///
/// **The rig has to fit inside the course.** The course has a roof — the tunnel
/// (`crate::render::road_mesh::TUNNEL_HEIGHT`) — and a camera above it is not a
/// high camera, it is a camera *outside the level*, looking down at the back of
/// a slab while the car it is framing is hidden underneath.
///
/// That is not hypothetical. The portrait rig already lifts the eye to hold the
/// neighbouring lane in a tall thin frame, and multiplying that by
/// [`LANE_FRAME_PULLBACK`] took a phone's eye to a measured **7.75 m** against a
/// 7 m ceiling: the tunnel rendered as a black lid with a strip of sky over it.
/// Nothing in the rig knew the course had a ceiling, so nothing stopped it.
///
/// Expressed as a bound on the *rig* rather than as a clamp applied inside
/// tunnels, deliberately. A per-frame clamp would drop the camera 1.7 m at the
/// tunnel mouth and lift it again at the exit — a lurch at exactly the two
/// moments the road is most enclosed, and one that only ever fires on a phone,
/// which is the hardest kind of bug to see coming. A bound on the rig is the
/// same shot everywhere, and it costs the phone nothing it had before: capped,
/// the eye still sits well above the height it used pre-pullback, because the
/// zoom-out is carried by the *arm* and only incidentally by the lift.
///
/// The clearance under the roof is for the tunnel's ceiling lights, which hang
/// below it.
const RIG_HEIGHT_CEILING: f32 =
    crate::render::road_mesh::TUNNEL_HEIGHT - TUNNEL_EYE_CLEARANCE;

/// How far under a tunnel roof the eye must stay (m).
const TUNNEL_EYE_CLEARANCE: f32 = 1.0;

/// The eye height for a profile, bounded by [`RIG_HEIGHT_CEILING`].
fn rig_height(tuning: &CameraTuning, lane_locked: bool) -> f32 {
    (tuning.height * pullback(lane_locked)).min(RIG_HEIGHT_CEILING)
}

/// The ideal eye position for a framed subject, a heading and a chase distance.
fn ideal_eye(framed: Vec3, heading: f32, distance: f32, height: f32) -> Vec3 {
    let (s, c) = heading.sin_cos();
    let back = Vec3::new(-s, 0.0, -c);
    framed
        .add(back.mul_scalar(distance))
        .add(Vec3::new(0.0, height, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;
    use crate::sim::contact::ContactState;
    use crate::sim::controller::{place_on_track, step as drive_step, StepReport};
    use crate::tuning::Tuning;

    fn fixture() -> (Track, CarState, ChaseCamera) {
        let track = Track::fixture(crate::DEFAULT_SEED);
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        place_on_track(&mut car, &track.sample_at(60.0), 0.0);
        let mut camera = ChaseCamera::new();
        camera.snap_to(&car, &track, &CameraTuning::DEFAULT);
        (track, car, camera)
    }

    /// One controller step with its own throwaway contact state — these are
    /// camera tests, and none of them is about collisions.
    fn drive(car: &mut CarState, command: DriveCommand, track: &Track, boost: bool) -> StepReport {
        let tuning = Tuning::DEFAULT;
        let mut contact = ContactState::new();
        let report = drive_step(car, command, track, &tuning, boost, &mut contact, None);
        contact.advance(car, &tuning.collision);
        report
    }

    /// One camera step with no collision impulse.
    fn frame(
        camera: &mut ChaseCamera,
        car: &CarState,
        track: &Track,
        tuning: &CameraTuning,
        accel: f32,
        boosting: bool,
    ) -> CameraPose {
        camera.step(
            car,
            track,
            tuning,
            &VehicleTuning::DEFAULT,
            CameraDrive {
                forward_accel: accel,
                boosting,
                ..CameraDrive::default()
            },
        )
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
        let mut pose = frame(camera, car, track, &t, 0.0, false);
        for _ in 0..steps {
            let report = drive(car, command, track, command.boost);
            pose = frame(camera, car, track, &t, report.forward_accel, command.boost);
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
        let mut previous = frame(&mut camera, &car, &track, &t, 0.0, false).fov_degrees;
        for i in 0..900 {
            // Slam between flat out and hard braking, the worst case for a
            // speed-driven field of view.
            let command = if (i / 45) % 2 == 0 {
                DriveCommand { boost: true, ..DriveCommand::FLAT_OUT }
            } else {
                DriveCommand { brake: 1.0, ..DriveCommand::IDLE }
            };
            let report = drive(&mut car, command, &track, true);
            let pose = frame(&mut camera, &car, &track, &t, report.forward_accel, command.boost);
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
        // Displace the camera hard, then let it settle behind a stationary car.
        camera.eye = car.position.add(Vec3::new(120.0, 60.0, -90.0));
        camera.eye_velocity = Vec3::ZERO;
        let mut previous = f32::INFINITY;
        for i in 0..240 {
            frame(&mut camera, &car, &track, &t, 0.0, false);
            let error = camera
                .eye
                .subtract(ideal_eye(car.position, camera.heading, t.distance_low, t.height))
                .length();
            // A critically damped spring never overshoots into a growing error.
            assert!(error < previous + 1.0e-3 || i < 4, "step {i}: {previous} -> {error}");
            previous = error;
        }
        assert!(previous < 0.5, "and it actually arrives: {previous}");
    }

    /// **The lane game's camera does not move when the car changes lane.**
    ///
    /// This is the defect it was written for: with the crossing sped up to three
    /// frames, a rig bolted to the car was yanked 3.5 m sideways — and worse,
    /// was told by the velocity feed-forward that its subject had launched
    /// sideways at 300 km/h — so a dodge threw the whole frame. Framing the road
    /// instead leaves the camera still and lets the car move inside the picture.
    #[test]
    fn a_lane_change_moves_the_car_in_the_frame_rather_than_the_frame() {
        let track = Track::fixture(crate::DEFAULT_SEED);
        let t = CameraTuning::DEFAULT;
        let vehicle = VehicleTuning::DEFAULT;

        // A car parked on the centreline, with a settled camera behind it.
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        place_on_track(&mut car, &track.sample_at(120.0), 0.0);
        car.distance = 120.0;
        let mut camera = ChaseCamera::new();
        camera.snap_to(&car, &track, &t);
        let rails = CameraDrive { lane_locked: true, ..CameraDrive::default() };
        (0..30).for_each(|_| {
            camera.step(&car, &track, &t, &vehicle, rails);
        });
        let settled = camera.eye;

        // Now put the car two lanes over, as a fast crossing does, and keep the
        // lateral speed a crossing would have.
        let sample = track.sample_at(car.distance);
        car.lateral = track.lane_lateral(&sample, 2);
        car.position = car.position.add(sample.right.mul_scalar(car.lateral));
        car.lateral_speed = 85.0;
        let mut moved = f32::NEG_INFINITY;
        (0..30).for_each(|_| {
            camera.step(&car, &track, &t, &vehicle, rails);
            moved = moved.max(camera.eye.subtract(settled).length());
        });
        assert!(
            moved < 0.5,
            "the camera chased the lane change {moved} m — the frame is supposed to hold still"
        );
    }

    /// And the wheel game's camera still does follow the car across the road,
    /// because there the line the car is on is the thing being shown.
    #[test]
    fn the_wheel_cameras_frame_still_follows_the_car_sideways() {
        let track = Track::fixture(crate::DEFAULT_SEED);
        let t = CameraTuning::DEFAULT;
        let vehicle = VehicleTuning::DEFAULT;
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        place_on_track(&mut car, &track.sample_at(120.0), 0.0);
        car.distance = 120.0;
        let mut camera = ChaseCamera::new();
        camera.snap_to(&car, &track, &t);
        (0..30).for_each(|_| {
            camera.step(&car, &track, &t, &vehicle, CameraDrive::default());
        });
        let settled = camera.eye;

        let sample = track.sample_at(car.distance);
        car.position = car.position.add(sample.right.mul_scalar(7.0));
        (0..90).for_each(|_| {
            camera.step(&car, &track, &t, &vehicle, CameraDrive::default());
        });
        assert!(
            camera.eye.subtract(settled).length() > 3.0,
            "the wheel game's camera must still track the car across the road"
        );
    }

    /// **The rig fits inside the course, on every frame shape.**
    ///
    /// The bug this exists for was invisible on a desktop and total on a phone:
    /// the portrait rig lifts the eye to hold the neighbouring lane, the lane
    /// game's pullback multiplied that lift, and the result — a measured 7.75 m
    /// — cleared the 7 m tunnel roof, so the whole section rendered as a black
    /// lid seen from above with the car hidden under it.
    ///
    /// Swept across real frame shapes rather than asserted on the authored
    /// tuning, because the authored tuning is exactly the one shape where it was
    /// never wrong.
    #[test]
    fn the_eye_stays_under_the_tunnel_roof_on_every_frame_shape() {
        let roof = crate::render::road_mesh::TUNNEL_HEIGHT;
        // Desktop wide, laptop, square-ish, tall phone, very tall phone.
        [(1920.0, 1080.0), (1280.0, 800.0), (900.0, 900.0), (390.0, 844.0), (360.0, 900.0)]
            .into_iter()
            .for_each(|(w, h)| {
                let tuning = CameraTuning::DEFAULT.framed_for_aspect(w / h);
                [false, true].into_iter().for_each(|lane_locked| {
                    let eye = rig_height(&tuning, lane_locked);
                    assert!(
                        eye < roof,
                        "{w}x{h} lane_locked={lane_locked}: the eye sits at {eye} m, \
                         through a {roof} m tunnel roof"
                    );
                    assert!(
                        eye > 0.0,
                        "{w}x{h}: the eye must still be above the road: {eye}"
                    );
                });
            });
    }

    /// The cap is a bound, not a replacement: where it does not bite, the
    /// pullback still lifts the eye with the arm, which is what keeps the rails
    /// shot the angle it was authored at.
    #[test]
    fn the_height_cap_only_bites_where_the_rig_would_not_fit() {
        let desktop = CameraTuning::DEFAULT.framed_for_aspect(16.0 / 9.0);
        assert!(
            rig_height(&desktop, true) > rig_height(&desktop, false),
            "a wide frame is nowhere near the roof, so the pullback lifts freely"
        );
        assert!(
            (rig_height(&desktop, true) - desktop.height * LANE_FRAME_PULLBACK).abs() < 1.0e-5,
            "and it lifts by exactly the pullback"
        );
    }

    /// The rails rig sits further back, because it is holding a band of road
    /// rather than a car. Asserted against the wheel rig rather than against a
    /// number, so it survives a re-tune of the shared composition.
    #[test]
    fn the_lane_games_rig_is_pulled_back_from_the_wheel_games() {
        let track = Track::fixture(crate::DEFAULT_SEED);
        let t = CameraTuning::DEFAULT;
        let vehicle = VehicleTuning::DEFAULT;
        let settle = |lane_locked: bool| {
            let mut car = CarState::parked(Vec3::ZERO, 0.0);
            place_on_track(&mut car, &track.sample_at(120.0), 0.0);
            car.distance = 120.0;
            let mut camera = ChaseCamera::new();
            camera.snap_to(&car, &track, &t);
            let drive = CameraDrive { lane_locked, ..CameraDrive::default() };
            (0..120).for_each(|_| {
                camera.step(&car, &track, &t, &vehicle, drive);
            });
            camera.eye.subtract(car.position).length()
        };
        let wheel = settle(false);
        let rails = settle(true);
        assert!(
            rails > wheel * 1.2,
            "the lane rig is not pulled back: {rails} m vs {wheel} m"
        );
    }

    #[test]
    fn the_camera_follows_travel_rather_than_the_nose_in_a_drift() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 200);
        // Establish a big slide.
        for _ in 0..40 {
            let report = drive(
                &mut car,
                DriveCommand { handbrake: true, ..DriveCommand::turning(1.0) },
                &track,
                false,
            );
            frame(&mut camera, &car, &track, &t, report.forward_accel, false);
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
                frame(&mut c, &car, &track, &tuned, 0.0, false);
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
        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 200);
        camera.step(
            &car,
            &track,
            &t,
            &VehicleTuning::DEFAULT,
            CameraDrive {
                impact: Some(ImpactImpulse {
                    direction: Vec3::UNIT_X,
                    amplitude: 1.0,
                }),
                ..CameraDrive::default()
            },
        );
        let kicked = camera.impact_shake;
        assert!(kicked > 0.0, "the hit registered");
        for _ in 0..90 {
            frame(&mut camera, &car, &track, &t, 0.0, false);
        }
        assert!(
            camera.impact_shake < kicked * 0.05,
            "and it recovers quickly: {} -> {}",
            kicked,
            camera.impact_shake
        );
    }

    /// The bug the impulse replaced: the camera used to re-arm its kick from
    /// `car.impact_strength` every step, and that value is *held raised* for the
    /// whole time an impact rings. The result was a long flat rattle instead of
    /// a hit. One impulse must decay monotonically, however long the car spends
    /// still touching whatever it hit.
    #[test]
    fn one_impulse_decays_and_is_never_re_armed_by_a_lingering_impact_state() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 200);
        // The car is left in exactly the state a collision leaves it in, and
        // stays there — which is what a sustained overlap looks like.
        car.impact_strength = 1.0;
        car.impact_steps = 200;
        car.impact_direction = Vec3::UNIT_X;

        camera.step(
            &car,
            &track,
            &t,
            &VehicleTuning::DEFAULT,
            CameraDrive {
                impact: Some(ImpactImpulse {
                    direction: Vec3::UNIT_X,
                    amplitude: 1.0,
                }),
                ..CameraDrive::default()
            },
        );
        let mut previous = camera.impact_shake;
        for step in 0..120 {
            frame(&mut camera, &car, &track, &t, 0.0, false);
            assert!(
                camera.impact_shake < previous,
                "step {step}: the kick was re-armed ({previous} -> {})",
                camera.impact_shake
            );
            previous = camera.impact_shake;
        }
    }

    /// The three severities' kicks last roughly the durations the design brief
    /// names, and they get there from one decay constant rather than three.
    #[test]
    fn each_severity_of_kick_settles_inside_its_authored_window() {
        use crate::sim::contact::Severity;
        let t = CameraTuning::DEFAULT;
        let collision = crate::tuning::CollisionTuning::DEFAULT;
        // The amplitude below which the kick is no longer visible against the
        // ordinary speed vibration.
        let floor = t.speed_shake;
        let bands = [
            (Severity::Scrape, 0.0, 0.13),
            (Severity::Bump, 0.13, 0.24),
            (Severity::MajorCrash, 0.24, 0.36),
        ];
        for (severity, low, high) in bands {
            let mut shake = severity.pulse(&collision) * t.impact_shake;
            let mut seconds = 0.0;
            while shake > floor && seconds < 2.0 {
                shake *= (-t.impact_decay * DT).exp();
                seconds += DT;
            }
            assert!(
                seconds > low && seconds < high,
                "{severity:?} shakes for {seconds} s, outside {low}..{high}"
            );
        }
    }

    #[test]
    fn shake_is_bounded_and_absent_at_a_standstill() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let still = camera.advance_shake(None, &t, 0.0, false);
        assert!(still.length() < 1.0e-6, "a parked car does not vibrate");

        run(&track, &mut car, &mut camera, DriveCommand::FLAT_OUT, 600);
        let kick = Some(ImpactImpulse {
            direction: Vec3::UNIT_X,
            amplitude: 1.0,
        });
        for step in 0..600 {
            // Feed an impulse on every single step — the worst case the camera
            // can ever be handed — and the shake still stays inside its bound.
            let shake = camera.advance_shake(kick.filter(|_| step % 30 == 0), &t, 1.0, true);
            // Three unit-amplitude terms, so the bound is generous but real.
            let bound = (t.speed_shake + t.boost_shake) * 3.0 + t.impact_shake;
            assert!(shake.length() <= bound, "shake {shake:?} exceeded {bound}");
        }
    }

    #[test]
    fn a_degenerate_impulse_direction_does_not_poison_the_shake() {
        let (_, _, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        let shake = camera.advance_shake(
            Some(ImpactImpulse {
                direction: Vec3::ZERO,
                amplitude: 1.0,
            }),
            &t,
            0.5,
            false,
        );
        assert!(shake.x.is_finite() && shake.y.is_finite() && shake.z.is_finite());
        // And an out-of-range amplitude is clamped rather than trusted.
        let absurd = camera.advance_shake(
            Some(ImpactImpulse {
                direction: Vec3::UNIT_X,
                amplitude: 40.0,
            }),
            &t,
            0.5,
            false,
        );
        assert!(absurd.length() <= t.impact_shake + (t.speed_shake * 3.0) + 1.0e-4);
    }

    /// The bug the velocity feed-forward exists to prevent: at racing speed the
    /// camera must sit at its authored chase distance, not sixteen metres
    /// further back because a spring is chasing a moving target.
    #[test]
    fn the_chase_distance_holds_at_racing_speed() {
        let (track, mut car, mut camera) = fixture();
        let t = CameraTuning::DEFAULT;
        for _ in 0..900 {
            let command = crate::script::autopilot(&car, &track);
            let report = drive(&mut car, command, &track, false);
            frame(&mut camera, &car, &track, &t, report.forward_accel, false);
        }
        assert!(car.speed() > 70.0, "the test is at speed: {}", car.speed());

        let pose = frame(&mut camera, &car, &track, &t, 0.0, false);
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
        for i in 0..4_000 {
            let steer = ((i as f32) * 0.01).sin();
            let report = drive(&mut car, DriveCommand::turning(steer), &track, true);
            let pose = frame(&mut camera, &car, &track, &t, report.forward_accel, true);
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
