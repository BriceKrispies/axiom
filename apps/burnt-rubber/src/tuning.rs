//! **Every authored number in Burnt Rubber lives here.**
//!
//! The controller, the camera, the course generator and the traffic model read
//! their constants from the four tuning records below rather than embedding
//! magic numbers at their call sites. That is the whole point: a tuning pass is
//! an edit to *this* file, and a reader who wants to know how the car feels does
//! not have to read the integrator to find out.
//!
//! Units are SI throughout — metres, seconds, radians — because the engine's
//! dimensioned kernel types (`Meters`, `Ratio`) and the scene are in metres. The
//! HUD's km/h is a presentation conversion at the very edge ([`crate::hud`]),
//! never a unit the simulation thinks in.

use crate::sim::chassis::ChassisGeometry;

/// The fixed simulation step. Everything deterministic advances by exactly this.
pub const FIXED_STEP_NANOS: u64 = 16_666_667;

/// The fixed step in seconds — the `dt` every integrator uses.
pub const DT: f32 = 1.0 / 60.0;

/// The authored arcade vehicle model.
///
/// This is not a tyre/drivetrain simulation and is not trying to be. It is a
/// forward/lateral velocity split in the chassis frame with authored
/// accelerations, an authored grip curve, and an authored steering-authority
/// curve — chosen so the car is responsive within the first second and stays
/// controllable at 320 km/h.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VehicleTuning {
    /// Forward acceleration at a standstill (m/s²). Deliberately violent.
    pub accel: f32,
    /// The speed (m/s) at which forward acceleration has fallen to half.
    pub accel_falloff_speed: f32,
    /// Natural top speed off boost (m/s).
    pub top_speed: f32,
    /// Extra top speed while boosting (m/s).
    pub boost_top_speed_bonus: f32,
    /// Extra acceleration while boosting (m/s²).
    ///
    /// Deliberately absurd. Boost is not a 10% bonus, it is the moment the game
    /// stops pretending to be a car: on top of [`Self::accel`] this is well over
    /// ten g, and combined with the throttle it presses for you, the dirt it
    /// ignores and the traffic it goes through, holding it should feel like
    /// cheating. Everything that makes it *fair* lives in the meter, not here.
    pub boost_accel_bonus: f32,
    /// Braking deceleration (m/s²).
    pub brake_decel: f32,
    /// Reverse acceleration (m/s²) once stopped.
    pub reverse_accel: f32,
    /// The deliberately limited reverse top speed (m/s).
    pub reverse_top_speed: f32,
    /// Coasting drag coefficient (per second, exponential).
    ///
    /// Deliberately small. Lifting off should let the car *carry* — an arcade
    /// racer that sheds a third of its speed in two seconds of coasting punishes
    /// the player for looking at a corner. Braking is what slows the car down,
    /// and braking is a separate, much larger number.
    pub coast_drag: f32,
    /// Rolling resistance, a constant deceleration (m/s²). Small, for the same
    /// reason as [`Self::coast_drag`].
    pub rolling_resistance: f32,
    /// Peak yaw rate at low speed (rad/s).
    pub max_yaw_rate: f32,
    /// Speed (m/s) at which steering authority has fallen to half. This is the
    /// single most important handling number: too low and the car is a barge at
    /// speed, too high and a flick at 300 km/h spins it.
    pub steer_falloff_speed: f32,
    /// The floor steering authority never falls below (fraction of
    /// [`Self::max_yaw_rate`]) — without it, top speed becomes uncontrollable.
    pub steer_authority_floor: f32,
    /// How fast the steering input itself ramps toward the held value (per s).
    pub steer_input_rate: f32,
    /// Lateral grip: the per-second exponential rate lateral velocity bleeds off
    /// on tarmac. High = the car sticks; low = it slides.
    ///
    /// The number that matters is the *ratio* to the steering authority. A turn
    /// at speed `v` with yaw rate `ω` settles at roughly `v·ω/grip` of lateral
    /// slide; grip has to be high enough that a hard turn on tarmac stays under
    /// [`Self::drift_threshold`], or the car is drifting whenever it corners and
    /// the handbrake — which is supposed to be *how* you drift — becomes noise.
    pub grip: f32,
    /// Lateral grip while the handbrake is held.
    pub handbrake_grip: f32,
    /// Lateral grip off the tarmac.
    pub offroad_grip: f32,
    /// Extra forward drag off the tarmac (per second, exponential).
    ///
    /// This is a *rate*, so its bite scales with speed — which makes it very
    /// easy to set far too high without noticing. Running wide should cost about
    /// what a hard brake costs; if `offroad_drag * top_speed` climbs past
    /// [`Self::brake_decel`] then the dirt is stopping the car harder than the
    /// brakes can, and a mistake at speed stops being a mistake and becomes a
    /// crash. The relationship is asserted in the tests below.
    pub offroad_drag: f32,
    /// Fraction of normal acceleration available off the tarmac.
    pub offroad_accel_scale: f32,
    /// Extra yaw the handbrake adds, as a multiplier on the steering authority.
    pub handbrake_yaw_gain: f32,
    /// Lateral speed (m/s) above which the car counts as drifting.
    pub drift_threshold: f32,
    /// Lateral speed (m/s) below which a drift has recovered (hysteresis).
    pub drift_release: f32,
    /// Counter-steer assist: how strongly the chassis is pulled back toward the
    /// velocity heading while drifting (per second). This is the "forgiving
    /// drift window" — without it a slide is a spin.
    pub drift_recovery: f32,
    /// Gravity (m/s²) applied when airborne over a crest.
    pub gravity: f32,
    /// Vertical speed (m/s) below which the car re-grounds onto the road surface.
    pub ground_snap_speed: f32,
    /// How fast the car settles onto the road surface when grounded (per second).
    pub ground_settle_rate: f32,
    /// The half-length of the player's collision box (m).
    pub half_length: f32,
    /// The half-width of the player's collision box (m).
    pub half_width: f32,
    /// Where the car's mass sits between its wheels.
    ///
    /// Not decoration: the centre of gravity is the point the chassis yaws
    /// about, and its height sets how much load a corner throws onto the
    /// outside wheels — and so how much grip survives the corner. See
    /// [`crate::sim::chassis`].
    pub chassis: ChassisGeometry,
}

impl VehicleTuning {
    /// The shipping car.
    pub const DEFAULT: VehicleTuning = VehicleTuning {
        accel: 38.0,
        accel_falloff_speed: 46.0,
        top_speed: 92.0,
        boost_top_speed_bonus: 22.0,
        boost_accel_bonus: 95.0,
        brake_decel: 52.0,
        reverse_accel: 9.0,
        reverse_top_speed: 9.0,
        coast_drag: 0.012,
        rolling_resistance: 0.7,
        max_yaw_rate: 2.35,
        steer_falloff_speed: 33.0,
        steer_authority_floor: 0.155,
        steer_input_rate: 7.0,
        grip: 17.5,
        handbrake_grip: 1.05,
        offroad_grip: 2.3,
        offroad_drag: 0.45,
        offroad_accel_scale: 0.45,
        handbrake_yaw_gain: 1.9,
        drift_threshold: 5.0,
        drift_release: 2.4,
        drift_recovery: 3.1,
        gravity: 24.0,
        ground_snap_speed: 0.0,
        ground_settle_rate: 11.0,
        half_length: 2.25,
        half_width: 1.0,
        chassis: ChassisGeometry::DEFAULT,
    };
}

impl Default for VehicleTuning {
    fn default() -> Self {
        VehicleTuning::DEFAULT
    }
}

/// **Everything about hitting something.** Classification thresholds, the
/// retained-momentum floors, the contact-episode rules, the separation assist,
/// the recovery assist and the feedback amplitudes — one record, because they
/// are one design.
///
/// The governing idea, and the reason this record exists at all: an ordinary
/// collision in an arcade racer is an *event*, not a *state*. It costs a bounded
/// slice of momentum once, disturbs the car's direction briefly, and hands
/// control straight back. The failure mode this replaced was the opposite —
/// contact was a state the simulation re-entered every fixed step for as long as
/// two boxes overlapped, so one mistake compounded its own speed loss
/// geometrically, retriggered its own sound, and re-armed its own camera shake
/// until the player had stopped moving. See [`crate::sim::contact`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionTuning {
    /// Closing speed along the contact normal (m/s) at or below which a contact
    /// is a [`crate::sim::contact::Severity::Scrape`], whatever its angle.
    pub scrape_normal_speed: f32,
    /// Normal closing speed (m/s) at or above which a *square* contact is a
    /// [`crate::sim::contact::Severity::MajorCrash`].
    pub crash_normal_speed: f32,
    /// Squareness (`0` = parallel, `1` = head-on) at or below which a contact is
    /// shallow enough to be a scrape however fast it was.
    pub scrape_squareness: f32,
    /// Squareness at or above which a fast contact counts as near-perpendicular.
    pub crash_squareness: f32,
    /// Speed (m/s) below which an obstacle counts as "nearly stationary".
    pub stationary_obstacle_speed: f32,
    /// Player speed (m/s) above which hitting a nearly stationary obstacle
    /// square-on is a major crash regardless of the other thresholds.
    pub stationary_crash_speed: f32,
    /// Normal closing speed (m/s) at or above which a square *barrier* contact
    /// is a major crash. Barriers are firmer than traffic, so this is the one
    /// classification threshold that differs between the two.
    pub barrier_crash_normal_speed: f32,
    /// Normal closing speed (m/s) at or above which a square contact with major
    /// scenery — the rock and tunnel walls of a
    /// [`crate::track::SectionKind::walled`] section, which have no guardrail
    /// and no give — is a major crash. The lowest of the three, because these
    /// are the one thing on the course that genuinely does not move.
    pub scenery_crash_normal_speed: f32,

    /// Fraction of the pre-impact forward speed a scrape must leave behind.
    ///
    /// The three floors below are **the** headline promise of the whole system,
    /// and the speed loss for each severity is derived from its floor
    /// (`max_loss = 1 - floor`) rather than authored separately — two numbers
    /// that must agree are one number.
    pub scrape_speed_floor: f32,
    /// Fraction of the pre-impact forward speed an ordinary bump must leave.
    pub bump_speed_floor: f32,
    /// Fraction of the pre-impact forward speed a major crash must leave.
    pub crash_speed_floor: f32,
    /// Normal closing speed (m/s) at which a severity's speed loss reaches its
    /// cap. Below it the loss ramps in proportionally, so a light touch inside a
    /// severity band costs less than a heavy one.
    pub loss_reference_speed: f32,

    /// Lateral separation impulse (m/s) a scrape applies.
    pub scrape_deflect: f32,
    /// Lateral deflection (m/s) an ordinary bump applies.
    pub bump_deflect: f32,
    /// Lateral deflection (m/s) a major crash applies.
    pub crash_deflect: f32,
    /// Yaw disturbance (rad/s) an ordinary bump applies.
    pub bump_yaw_kick: f32,
    /// Yaw disturbance (rad/s) a major crash applies.
    pub crash_yaw_kick: f32,
    /// How fast a collision's yaw disturbance decays on its own (per second).
    pub impact_yaw_decay: f32,

    /// Camera/spark impulse amplitude for a scrape (`0..1`).
    pub scrape_pulse: f32,
    /// Camera/spark impulse amplitude for a bump.
    pub bump_pulse: f32,
    /// Camera/spark impulse amplitude for a major crash.
    pub crash_pulse: f32,

    /// Fixed steps a contact episode with one obstacle suppresses further full
    /// impact responses against **that same obstacle**. 39 steps is 0.65 s.
    pub episode_steps: u32,
    /// Clearance (m) beyond touching at which a pair counts as genuinely
    /// separated, ending the episode early so a fresh collision reads as fresh.
    ///
    /// **Roughly a car's width, and that is the point.** Separation itself opens
    /// a few centimetres of daylight within a step or two, so a small value here
    /// makes "the vehicles separated and collided again" true on almost every
    /// step of a grind — and the cooldown, which exists precisely to stop a
    /// grind re-charging itself, is re-armed by the very assist that is pushing
    /// the pair apart. Measured, that reintroduced the original bug in a milder
    /// form: holding full lock into a car alongside escalated a scrape into a
    /// bump partway through. A gap the player has to genuinely *drive* is what
    /// makes the clause mean what it says.
    pub separation_clearance: f32,
    /// Fixed steps between the rate-limited scrape cues emitted while a contact
    /// episode is still grinding. 12 steps is 0.2 s.
    pub scrape_repeat_steps: u32,

    /// Most a body may be moved out of penetration in one fixed step (m). Deep
    /// overlaps resolve over several steps rather than as a visible teleport.
    pub separation_step: f32,
    /// Velocity bias (m/s) pushing an overlapping pair apart, so separation
    /// continues under the integrator rather than only as position edits.
    pub separation_speed: f32,
    /// Share of a traffic de-penetration the *player* absorbs; the traffic car
    /// takes the rest. Below a half, because traffic yields and concrete does
    /// not.
    pub player_separation_share: f32,

    /// Most a traffic car may be pushed out of its lane by contact (m).
    pub traffic_yield_lateral: f32,
    /// Most forward speed (m/s) a traffic car may be shunted by.
    pub traffic_yield_speed: f32,
    /// How fast a yielded traffic car returns to its lane (per second).
    pub traffic_yield_return: f32,
    /// How fast a shunted traffic car's extra speed bleeds off (per second).
    pub traffic_yield_decay: f32,

    /// Fixed steps of recovery assistance after a bump or a crash. 60 is 1 s.
    pub recovery_steps: u32,
    /// Extra forward acceleration at full assist, as a fraction of the car's own
    /// acceleration. **Not boost**: it neither reads nor writes the meter.
    pub recovery_accel_gain: f32,
    /// Extra lateral bleed at full assist (per second), applied only to the
    /// slide above [`Self::recovery_stable_lateral`].
    pub recovery_lateral_damp: f32,
    /// Extra decay on the collision's yaw disturbance at full assist (per s).
    pub recovery_yaw_damp: f32,
    /// How strongly the heading is biased toward the recovery target (per s).
    pub recovery_heading_pull: f32,
    /// How much of the heading target is the road ahead rather than the car's
    /// own direction of travel (`0..1`).
    pub recovery_road_blend: f32,
    /// Lateral speed (m/s) below which the car counts as stable again.
    pub recovery_stable_lateral: f32,
    /// Yaw disturbance (rad/s) below which the car counts as stable again.
    pub recovery_stable_yaw: f32,

    /// Fraction of the incoming lateral speed reflected off a barrier.
    pub barrier_restitution: f32,
    /// How fast a car pressed against a barrier is turned to run *along* it
    /// (per second). Without this a car that noses into a wall grinds there
    /// forever: with zero yaw authority of its own and no rotation from the
    /// contact, nothing in the model ever points it back down the road.
    pub barrier_align: f32,
}

impl CollisionTuning {
    /// The shipping collision feel.
    pub const DEFAULT: CollisionTuning = CollisionTuning {
        scrape_normal_speed: 9.0,
        crash_normal_speed: 26.0,
        scrape_squareness: 0.30,
        crash_squareness: 0.68,
        stationary_obstacle_speed: 6.0,
        stationary_crash_speed: 55.0,
        barrier_crash_normal_speed: 22.0,
        scenery_crash_normal_speed: 14.0,

        scrape_speed_floor: 0.95,
        bump_speed_floor: 0.85,
        crash_speed_floor: 0.65,
        loss_reference_speed: 40.0,

        scrape_deflect: 2.2,
        bump_deflect: 5.5,
        crash_deflect: 8.0,
        bump_yaw_kick: 0.55,
        crash_yaw_kick: 1.15,
        impact_yaw_decay: 6.0,

        scrape_pulse: 0.10,
        bump_pulse: 0.42,
        crash_pulse: 0.95,

        episode_steps: 39,
        separation_clearance: 1.1,
        scrape_repeat_steps: 12,

        separation_step: 0.35,
        separation_speed: 3.0,
        player_separation_share: 0.35,

        traffic_yield_lateral: 1.4,
        traffic_yield_speed: 6.0,
        traffic_yield_return: 1.6,
        traffic_yield_decay: 2.2,

        recovery_steps: 60,
        recovery_accel_gain: 0.85,
        recovery_lateral_damp: 7.0,
        recovery_yaw_damp: 5.5,
        recovery_heading_pull: 2.2,
        recovery_road_blend: 0.4,
        recovery_stable_lateral: 2.5,
        recovery_stable_yaw: 0.35,

        barrier_restitution: 0.34,
        barrier_align: 7.0,
    };

    /// The most forward speed a `severity` collision may take, as a fraction.
    /// Derived from the floor so the two can never disagree.
    pub fn max_loss(&self, floor: f32) -> f32 {
        (1.0 - floor).clamp(0.0, 1.0)
    }

    /// The episode length in seconds — what the design brief is written in.
    pub fn episode_seconds(&self) -> f32 {
        self.episode_steps as f32 * DT
    }

    /// The recovery length in seconds.
    pub fn recovery_seconds(&self) -> f32 {
        self.recovery_steps as f32 * DT
    }
}

impl Default for CollisionTuning {
    fn default() -> Self {
        CollisionTuning::DEFAULT
    }
}

/// The third-person chase camera.
///
/// Distances are metres, angles degrees (converted once at the boundary), rates
/// per second. The spring constants are angular frequencies for a critically
/// damped spring, so "higher = tighter".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraTuning {
    /// Chase distance at a standstill (m).
    pub distance_low: f32,
    /// Chase distance at natural top speed (m).
    pub distance_high: f32,
    /// Extra chase distance while boosting (m).
    pub distance_boost: f32,
    /// Eye height above the car (m).
    pub height: f32,
    /// Look-ahead ahead of the car at a standstill (m).
    pub look_ahead_low: f32,
    /// Look-ahead at top speed (m).
    pub look_ahead_high: f32,
    /// Vertical field of view at a standstill (degrees).
    pub fov_low: f32,
    /// Vertical field of view at natural top speed (degrees).
    pub fov_high: f32,
    /// Vertical field of view at full boost (degrees).
    pub fov_boost: f32,
    /// How fast the field of view chases its target (per second).
    pub fov_rate: f32,
    /// Position spring frequency (rad/s).
    pub position_spring: f32,
    /// Heading spring frequency (rad/s).
    pub heading_spring: f32,
    /// How much of the heading comes from the velocity direction rather than the
    /// chassis nose (0..1) — this is what keeps a drift readable.
    pub velocity_heading_blend: f32,
    /// How much of the heading is pulled toward the upcoming track direction.
    pub track_anticipation: f32,
    /// How far ahead down the track the anticipation samples (m).
    pub anticipation_distance: f32,
    /// Metres of camera pull-back per m/s² of forward acceleration.
    pub accel_pullback: f32,
    /// Maximum accel/brake pull (m), clamped both ways.
    pub accel_pullback_limit: f32,
    /// Camera roll per rad/s of yaw rate (degrees).
    pub turn_roll: f32,
    /// Hard limit on ordinary camera roll (degrees).
    pub turn_roll_limit: f32,
    /// Vibration amplitude (m) at natural top speed.
    pub speed_shake: f32,
    /// Extra vibration amplitude (m) while boosting.
    pub boost_shake: f32,
    /// Impact shake amplitude (m) at the moment of a full-speed collision.
    pub impact_shake: f32,
    /// Per-second exponential decay of the impact shake.
    ///
    /// This one number sets how long *every* severity of impact is visible for,
    /// because the shake is an impulse that decays rather than a level that is
    /// held: the time an amplitude stays above the eye's floor is
    /// `ln(amplitude / floor) / decay`. At `11.0` the three severities'
    /// amplitudes land at roughly 0.10 s, 0.22 s and 0.30 s of visible kick,
    /// which is the design brief's three bands — from one constant rather than
    /// three that could drift apart.
    pub impact_decay: f32,
    /// Minimum eye height above the road surface (m) — the cheap, bounded
    /// stand-in for camera obstruction, using the track surface we already have.
    pub min_ground_clearance: f32,
}

impl CameraTuning {
    /// The shipping camera.
    ///
    /// The chase distance and the eye height are a *framing* decision, not a
    /// comfort one, and they are two independent decisions that a single "pull
    /// the camera in" instinct keeps conflating. Distance sets how *wide* the
    /// car reads. Height sets how far *down* the rig looks at it, and that is
    /// what decides whether the shot is a car seen from behind or a car seen
    /// from above.
    ///
    /// Those two knobs are also not independent of each other, which is the
    /// thing the last two passes each got half-right. Measured against the art
    /// target, the shot the previous rig produces is already the *right shot* —
    /// the horizon sits within 3% of frame height of the target's, the road
    /// converges on the same line, the roadside posts run past at the same
    /// cadence, and the car's rear bumper sits at 0.69 of frame height where the
    /// target has it at 0.72. Everything about the world matches. The one thing
    /// that does not is the **subject**: the car spans 22% of frame width where
    /// the target's spans 59%.
    ///
    /// That gap cannot be closed by pulling the camera in, and the arithmetic is
    /// worth writing down because it is counter-intuitive. For anything standing
    /// on the road, the ratio of its on-screen width to its distance *below the
    /// horizon* is `width / eye_height` — the chase distance cancels out
    /// entirely. Halving the distance makes the car twice as big and puts it
    /// twice as far down the frame, straight through the touch controls and off
    /// the bottom edge; it never makes the car bigger *where it is*. Field of
    /// view does not help either, because it magnifies the road and the posts by
    /// exactly the same factor, and those already match.
    ///
    /// The only lever that changes how much frame the subject owns is therefore
    /// the **eye height**, with the distance scaled by the same factor so the
    /// car keeps its place in the frame and every angle in the rig is preserved.
    /// So the whole rig scales by 0.815: the eye drops from 1.35 m to 1.10 m,
    /// and 5.9 m becomes 4.81 m behind the car.
    ///
    /// What that buys is two things at once. The car grows by a quarter without
    /// moving down the frame. And the eye is now 0.12 m above the car's own
    /// [`crate::render::car_model::ROOF_HEIGHT`] of 0.98 m rather than 0.37 m
    /// above it, so the roof and the shallow-raked backlight — which at 12° from
    /// horizontal present as a wide flat slab to anything looking down at them —
    /// go nearly edge-on. That is the target's read: a wide, low car seen from
    /// its own roofline, taillights across the middle of the silhouette, not a
    /// tall box seen from above.
    ///
    /// `min_ground_clearance` scales with the rig rather than being left behind.
    /// It is a safety floor, not a framing knob, but a floor that does not move
    /// when the eye above it does stops being a floor and becomes the framing:
    /// held at 0.9 m it would sit 0.2 m under a 1.10 m eye and take the shot over
    /// on every crest. Scaled to 0.73 m it keeps the same 0.37 m of headroom it
    /// has always had, and goes on doing only its own job.
    pub const DEFAULT: CameraTuning = CameraTuning {
        distance_low: 4.81,
        distance_high: 6.28,
        distance_boost: 0.9,
        height: 1.1,
        look_ahead_low: 5.0,
        look_ahead_high: 14.0,
        fov_low: 65.0,
        fov_high: 88.0,
        fov_boost: 96.0,
        fov_rate: 3.4,
        position_spring: 11.0,
        heading_spring: 6.2,
        velocity_heading_blend: 0.62,
        track_anticipation: 0.3,
        anticipation_distance: 34.0,
        accel_pullback: 0.055,
        accel_pullback_limit: 1.6,
        turn_roll: 1.9,
        turn_roll_limit: 4.0,
        speed_shake: 0.035,
        boost_shake: 0.05,
        impact_shake: 0.55,
        impact_decay: 11.0,
        min_ground_clearance: 0.73,
    };
}

impl Default for CameraTuning {
    fn default() -> Self {
        CameraTuning::DEFAULT
    }
}

/// The course generator's shape and its hard constraints.
///
/// The constraints are not suggestions: [`crate::track::generate`] runs a
/// *bounded* correction pass and then the generated table is asserted against
/// them by the test suite, so a course that violates one is a build failure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CourseTuning {
    /// Arc-length spacing of the sampled centreline (m).
    pub sample_spacing: f32,
    /// Distance between generated control points (m).
    pub control_spacing: f32,
    /// Maximum heading change between adjacent control points (rad). With
    /// `control_spacing` this sets the minimum turn radius.
    pub max_yaw_step: f32,
    /// Maximum change in heading *rate* between adjacent control points (rad) —
    /// the curvature-continuity bound that stops instant reversals.
    pub max_yaw_step_delta: f32,
    /// Maximum road grade (rise/run) on ordinary road.
    pub max_grade: f32,
    /// Maximum change in grade between adjacent control points.
    pub max_grade_delta: f32,
    /// Maximum banking angle (rad).
    pub max_bank: f32,
    /// Banking per unit curvature (rad per rad/m), before clamping.
    pub bank_per_curvature: f32,
    /// Narrowest the road is ever allowed to get, half-width (m).
    pub min_half_width: f32,
    /// Widest half-width the generator may author (m).
    pub max_half_width: f32,
    /// How many bounded smoothing iterations the correction pass runs.
    pub correction_passes: u32,
    /// Paved shoulder beyond the lane edge (m) — rumble strips and a little
    /// drag, still recoverable at speed.
    pub shoulder: f32,
    /// Dirt verge beyond the shoulder before the barrier (m), on open sections.
    /// Walled sections ([`crate::track::SectionKind::walled`]) have none: their
    /// barrier sits right at the shoulder, which is what makes them feel tight.
    pub verge: f32,
    /// Lane width (m) — traffic lanes and the painted dividers share it.
    ///
    /// **Constant for the whole course.** A lane is a fixed physical thing, not
    /// a fraction of however wide the road happens to be here; see
    /// [`crate::track::Track::lane_lateral`].
    pub lane_width: f32,
    /// Paved margin (m) between the outermost lane and the edge of the tarmac,
    /// each side. The hard shoulder: the road is its lanes plus this.
    pub lane_shoulder: f32,
    /// Spacing of a lane dash plus its gap (m).
    pub dash_period: f32,
    /// Length of the painted part of a dash (m).
    pub dash_length: f32,
    /// Spacing of roadside reflector posts (m).
    pub post_spacing: f32,
}

impl CourseTuning {
    /// The tarmac half-width (m) that carries `lanes` lanes plus its shoulder.
    ///
    /// **This is the direction the dependency runs**: a section authors how many
    /// lanes it has and the road is however wide that needs to be. It used to run
    /// the other way — a section authored a width and the lane count fell out of
    /// a division — which meant every lane centre was a fraction of the local
    /// road width, so lanes slid sideways as the road breathed and were
    /// renumbered wholesale whenever the division crossed an integer. A car
    /// holding a lane got shunted across the road by geometry it never asked to
    /// change. See [`crate::track::Track::lane_lateral`].
    pub fn half_width_for_lanes(&self, lanes: usize) -> f32 {
        lanes as f32 * self.lane_width * 0.5 + self.lane_shoulder
    }

    /// The shipping course shape.
    pub const DEFAULT: CourseTuning = CourseTuning {
        sample_spacing: 2.0,
        control_spacing: 40.0,
        // 40 m of arc through 0.115 rad is a ~348 m radius: fast and sweeping,
        // never a hairpin.
        max_yaw_step: 0.115,
        max_yaw_step_delta: 0.032,
        max_grade: 0.10,
        max_grade_delta: 0.018,
        max_bank: 0.14,
        bank_per_curvature: 26.0,
        // The road is authored in LANES (see `SectionProfile::lanes`), so these
        // bounds are the three-lane and five-lane widths plus the jitter band
        // either side — not free parameters. Widening them without widening the
        // lane ladder just adds unpainted tarmac.
        min_half_width: 5.6,
        max_half_width: 9.9,
        correction_passes: 6,
        shoulder: 1.6,
        verge: 5.0,
        lane_width: 3.5,
        lane_shoulder: 0.75,
        dash_period: 12.0,
        dash_length: 5.0,
        post_spacing: 8.0,
    };
}

impl Default for CourseTuning {
    fn default() -> Self {
        CourseTuning::DEFAULT
    }
}

/// Traffic, boost and the near-miss rules — the "drive dangerously" economy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaceTuning {
    /// Traffic cars considered by the simulation at once.
    pub traffic_active: usize,
    /// How far ahead of the car traffic is simulated (m).
    pub traffic_ahead: f32,
    /// How far behind the car traffic is kept before recycling (m).
    pub traffic_behind: f32,
    /// Gap between consecutive traffic spawn slots along the course (m). With
    /// [`Self::traffic_ahead`] this sets how dense the traffic feels.
    pub traffic_spacing: f32,
    /// How far past the start line traffic begins (m) — the countdown and the
    /// first acceleration happen on clear road.
    pub traffic_clear_start: f32,
    /// How far ahead of the player a traffic car may never *appear* (m).
    ///
    /// Recycled traffic normally spawns [`Self::traffic_ahead`] away, but a jump
    /// — a capture, a reset, the finish teleport — refills the pool around
    /// wherever the player now is, and without this the next slot can land on
    /// top of the car. Sized so that even at the boosted top speed a newly
    /// spawned car is more than a second of warning away.
    pub traffic_safe_ahead: f32,
    /// How far behind the player a traffic car may never appear (m). Shorter
    /// than the window ahead: a car materialising in the mirror is startling,
    /// one materialising in the windscreen is unfair.
    pub traffic_safe_behind: f32,
    /// Slowest traffic speed (m/s).
    pub traffic_speed_min: f32,
    /// Fastest traffic speed (m/s).
    pub traffic_speed_max: f32,
    /// Traffic collision half-length (m).
    pub traffic_half_length: f32,
    /// Traffic collision half-width (m).
    pub traffic_half_width: f32,
    /// Lateral gap (m) inside which passing traffic counts as a near miss.
    pub near_miss_gap: f32,
    /// Minimum closing speed (m/s) for a pass to count as a near miss.
    pub near_miss_closing_speed: f32,
    /// Boost awarded by one near miss (fraction of the meter).
    pub near_miss_boost: f32,
    /// Boost awarded per second of sustained drift (fraction of the meter).
    pub drift_boost_rate: f32,
    /// Boost awarded per second above [`Self::high_speed_threshold`].
    pub high_speed_boost_rate: f32,
    /// The speed (m/s) above which simply holding it earns boost.
    pub high_speed_threshold: f32,
    /// Boost drained per second while held (fraction of the meter).
    pub boost_drain_rate: f32,
    /// Minimum meter needed to start a boost (stops a stuttering tap).
    pub boost_min_to_start: f32,
    /// Simulation steps a near-miss notification stays on the HUD.
    pub notify_steps: u32,
    /// Simulation steps of the pre-race countdown per number.
    pub countdown_steps: u32,
    /// Off-road time (s) after which the auto-reset offers itself.
    pub stuck_seconds: f32,
    /// Speed (m/s) below which the car counts as stuck.
    pub stuck_speed: f32,
}

impl RaceTuning {
    /// The shipping race rules.
    pub const DEFAULT: RaceTuning = RaceTuning {
        traffic_active: 9,
        traffic_ahead: 620.0,
        traffic_behind: 90.0,
        traffic_spacing: 85.0,
        traffic_clear_start: 300.0,
        traffic_safe_ahead: 140.0,
        traffic_safe_behind: 20.0,
        traffic_speed_min: 22.0,
        traffic_speed_max: 38.0,
        traffic_half_length: 2.3,
        traffic_half_width: 1.05,
        near_miss_gap: 3.1,
        near_miss_closing_speed: 16.0,
        near_miss_boost: 0.13,
        drift_boost_rate: 0.22,
        high_speed_boost_rate: 0.075,
        high_speed_threshold: 74.0,
        boost_drain_rate: 0.36,
        boost_min_to_start: 0.06,
        notify_steps: 75,
        countdown_steps: 45,
        stuck_seconds: 2.5,
        stuck_speed: 4.0,
    };
}

impl Default for RaceTuning {
    fn default() -> Self {
        RaceTuning::DEFAULT
    }
}

/// The whole tuning surface, carried as one value so a test can vary a single
/// number without reaching into a global.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Tuning {
    pub vehicle: VehicleTuning,
    pub collision: CollisionTuning,
    pub camera: CameraTuning,
    pub course: CourseTuning,
    pub race: RaceTuning,
}

impl Tuning {
    /// The shipping tuning.
    pub const DEFAULT: Tuning = Tuning {
        vehicle: VehicleTuning::DEFAULT,
        collision: CollisionTuning::DEFAULT,
        camera: CameraTuning::DEFAULT,
        course: CourseTuning::DEFAULT,
        race: RaceTuning::DEFAULT,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_tuning_is_the_shipping_tuning() {
        assert_eq!(Tuning::default(), Tuning::DEFAULT);
        assert_eq!(VehicleTuning::default(), VehicleTuning::DEFAULT);
        assert_eq!(CollisionTuning::default(), CollisionTuning::DEFAULT);
        assert_eq!(CameraTuning::default(), CameraTuning::DEFAULT);
        assert_eq!(CourseTuning::default(), CourseTuning::DEFAULT);
        assert_eq!(RaceTuning::default(), RaceTuning::DEFAULT);
    }

    /// The camera targets the specification named, so a later "tuning drift"
    /// that quietly walks the field of view out of its band is caught here.
    #[test]
    fn the_camera_field_of_view_band_is_ordered_and_bounded() {
        let c = CameraTuning::DEFAULT;
        assert!(c.fov_low < c.fov_high, "fov rises with speed");
        assert!(c.fov_high < c.fov_boost, "boost widens further");
        assert!(c.fov_boost <= 100.0, "and stays readable");
        assert!(c.distance_low < c.distance_high, "and so does chase distance");
        assert!(c.look_ahead_low < c.look_ahead_high);
        assert!(c.turn_roll_limit <= 6.0, "ordinary roll stays subtle");
    }

    /// The framing decision behind [`CameraTuning::DEFAULT`], as an assertion
    /// rather than a memory: the eye sits just *above* the car's roofline, and
    /// the clearance floor stays below the eye by a real margin.
    ///
    /// Both bounds are one-sided for a reason. Drop the eye under the roof and
    /// the car becomes a wall across the road ahead. Raise it and the roof and
    /// the shallow backlight turn back into the flat slab that made the car read
    /// as a box seen from above instead of a wide, low car seen from behind.
    #[test]
    fn the_eye_sits_just_above_the_roofline_and_clear_of_its_own_floor() {
        let c = CameraTuning::DEFAULT;
        let roof = crate::render::car_model::ROOF_HEIGHT;
        assert!(c.height > roof, "the eye is above the roof: {} vs {roof}", c.height);
        assert!(
            c.height - roof < 0.3,
            "but only just — {} m of it is a look-down angle",
            c.height - roof
        );
        assert!(
            c.height - c.min_ground_clearance > 0.3,
            "and the clearance floor is a floor, not the framing: {} vs {}",
            c.min_ground_clearance,
            c.height
        );
    }

    /// The car's headline behaviour is an ordering between numbers, and the
    /// ordering is the design: braking beats acceleration, reverse is a crawl,
    /// boost genuinely raises the ceiling.
    #[test]
    fn the_vehicle_numbers_encode_the_intended_arcade_feel() {
        let v = VehicleTuning::DEFAULT;
        assert!(v.brake_decel > v.accel, "braking is more forceful than throttle");
        assert!(v.reverse_top_speed < v.top_speed * 0.2, "reverse is limited");
        assert!(v.boost_top_speed_bonus > 0.0 && v.boost_accel_bonus > 0.0);
        assert!(
            v.boost_accel_bonus > v.accel * 2.0,
            "boost is a different order of thing from the throttle, not a bonus on it"
        );
        assert!(v.handbrake_grip < v.grip, "the handbrake breaks traction");
        assert!(v.offroad_grip < v.grip, "and so does the dirt");
        // The dirt must not out-brake the brakes. `offroad_drag` is a rate, so
        // its effect scales with speed; unchecked, running wide at top speed
        // sheds more than a full-force stop and a small mistake reads as a
        // crash.
        let dirt_at_top = v.offroad_drag * v.top_speed;
        assert!(
            dirt_at_top < v.brake_decel,
            "the dirt decelerates at {dirt_at_top} m/s^2, harder than the {} m/s^2 brakes",
            v.brake_decel
        );
        assert!(dirt_at_top > v.brake_decel * 0.4, "but it still genuinely costs you");
        assert!(v.drift_release < v.drift_threshold, "drift state has hysteresis");
        // A hard turn on tarmac at top speed must stay on the grippy side of the
        // drift threshold: `v·ω/grip` is where the slide settles.
        let authority_at_top = v.max_yaw_rate
            * (1.0f32 / (1.0 + v.top_speed / v.steer_falloff_speed)).max(v.steer_authority_floor);
        let gripping_slide = v.top_speed * authority_at_top / v.grip;
        assert!(
            gripping_slide < v.drift_threshold,
            "a plain hard turn slides {gripping_slide} m/s, past the {} m/s drift threshold",
            v.drift_threshold
        );
        // And the handbrake must comfortably clear it, or it does nothing.
        let handbrake_slide =
            v.top_speed * authority_at_top * v.handbrake_yaw_gain / v.handbrake_grip;
        assert!(
            handbrake_slide > v.drift_threshold * 3.0,
            "the handbrake only slides {handbrake_slide} m/s"
        );
        // Coasting is gentle: braking must dominate it by a wide margin, or
        // lifting off feels like braking.
        let coast_at_top = v.coast_drag * v.top_speed + v.rolling_resistance;
        assert!(
            v.brake_decel > coast_at_top * 10.0,
            "coasting sheds {coast_at_top} m/s^2, too close to the {} m/s^2 brakes",
            v.brake_decel
        );
        assert!(v.steer_authority_floor > 0.0, "steering never dies completely");
    }

    /// The collision numbers encode the design brief as an *ordering*, and the
    /// ordering is what makes a scrape a scrape and a crash a crash. Every claim
    /// here is one sentence of that brief turned into an assertion.
    #[test]
    fn the_collision_numbers_encode_the_intended_severity_ladder() {
        let c = CollisionTuning::DEFAULT;
        // The three severities are genuinely ordered on every axis they share.
        assert!(c.scrape_normal_speed < c.crash_normal_speed, "the bands are ordered");
        // Firmness ladder: traffic yields, a guardrail does not, rock does not
        // even pretend to — so each takes less closing speed to be a crash.
        assert!(c.scenery_crash_normal_speed < c.barrier_crash_normal_speed);
        assert!(c.barrier_crash_normal_speed < c.crash_normal_speed);
        assert!(c.scrape_normal_speed < c.scenery_crash_normal_speed);
        assert!(c.scrape_squareness < c.crash_squareness);
        assert!(c.crash_speed_floor < c.bump_speed_floor);
        assert!(c.bump_speed_floor < c.scrape_speed_floor);
        assert!(c.scrape_speed_floor < 1.0, "even a scrape costs something");
        assert!(c.scrape_deflect < c.bump_deflect && c.bump_deflect < c.crash_deflect);
        assert!(c.bump_yaw_kick < c.crash_yaw_kick);
        assert!(c.scrape_pulse < c.bump_pulse && c.bump_pulse < c.crash_pulse);

        // The brief's headline floors, verbatim.
        assert!((c.scrape_speed_floor - 0.95).abs() < 1.0e-6);
        assert!((c.bump_speed_floor - 0.85).abs() < 1.0e-6);
        assert!((c.crash_speed_floor - 0.65).abs() < 1.0e-6);
        // And the losses are the floors' complements, by construction.
        assert!((c.max_loss(c.bump_speed_floor) - 0.15).abs() < 1.0e-6);
        assert!((c.max_loss(c.crash_speed_floor) - 0.35).abs() < 1.0e-6);
        assert_eq!(c.max_loss(2.0), 0.0, "a floor above one costs nothing");

        // "suppress another full impact response ... for 0.65 seconds".
        assert!(
            (c.episode_seconds() - 0.65).abs() < 0.01,
            "the episode is {} s",
            c.episode_seconds()
        );
        // "for approximately one second following the impact".
        assert!((c.recovery_seconds() - 1.0).abs() < 0.02);
        // A scrape cue repeats several times inside one episode, so a grind
        // sounds continuous — but far less often than every step.
        assert!(c.scrape_repeat_steps > 1 && c.scrape_repeat_steps < c.episode_steps);

        // Separation is bounded on every axis, and traffic yields more than the
        // player does.
        assert!(c.player_separation_share < 0.5, "traffic is the lighter body");
        assert!(c.player_separation_share > 0.0, "but the player still moves");
        assert!(c.separation_step > 0.0 && c.separation_step < 1.0, "no teleports");
        assert!(c.traffic_yield_lateral > 0.0 && c.traffic_yield_lateral < 2.0);
        assert!(c.traffic_yield_speed > 0.0 && c.traffic_yield_speed < 10.0);
        assert!(c.traffic_yield_return > 0.0, "a yielded car comes back to its lane");
        assert!(c.traffic_yield_decay > 0.0);

        // Recovery is an assist, not an autopilot: the extra acceleration is a
        // fraction of the car's own, never a multiple of it.
        assert!(c.recovery_accel_gain > 0.0 && c.recovery_accel_gain < 1.0);
        assert!(c.recovery_heading_pull < VehicleTuning::DEFAULT.max_yaw_rate);
        assert!((0.0..1.0).contains(&c.recovery_road_blend));
        assert!(c.recovery_stable_lateral > 0.0 && c.recovery_stable_yaw > 0.0);
        assert!(
            c.recovery_stable_lateral < VehicleTuning::DEFAULT.drift_threshold,
            "recovery settles the car well before it counts as drifting"
        );

        // A barrier is firmer than traffic but still not a brick wall.
        assert!(c.barrier_align > 0.0, "a wall always turns you back along itself");
        assert!((0.0..1.0).contains(&c.barrier_restitution), "walls do not launch you");
    }

    /// The course constraints have to be self-consistent or the generator's
    /// bounded correction pass cannot converge.
    #[test]
    fn the_course_constraints_are_self_consistent() {
        let c = CourseTuning::DEFAULT;
        assert!(c.sample_spacing < c.control_spacing);
        assert!(c.max_yaw_step_delta < c.max_yaw_step);
        assert!(c.max_grade_delta < c.max_grade);
        assert!(c.min_half_width < c.max_half_width);
        assert!(c.correction_passes > 0);
        assert!(c.dash_length < c.dash_period);
        assert!(c.lane_width * 2.0 <= c.min_half_width * 2.0);
        assert!(c.shoulder > 0.0 && c.verge > c.shoulder, "the verge is the recoverable margin");
    }

    /// Boost must be earnable faster than it drains under aggressive driving, or
    /// the reward loop never closes.
    #[test]
    fn the_boost_economy_can_actually_be_sustained() {
        let r = RaceTuning::DEFAULT;
        assert!(r.near_miss_boost > 0.0);
        assert!(r.drift_boost_rate > 0.0);
        assert!(r.high_speed_boost_rate > 0.0);
        assert!(r.boost_drain_rate > r.high_speed_boost_rate, "boost is spent faster than it trickles in");
        assert!(r.traffic_speed_min < r.traffic_speed_max);
        assert!(r.traffic_speed_max < VehicleTuning::DEFAULT.top_speed, "the player always closes on traffic");
    }

    /// Traffic has to stay dense enough to be worth avoiding and fair enough to
    /// be avoidable, and those are two numbers pulling opposite ways.
    #[test]
    fn the_traffic_layout_is_dense_but_navigable() {
        let r = RaceTuning::DEFAULT;
        let v = VehicleTuning::DEFAULT;
        // Consecutive spawn slots are far enough apart that two cars can never
        // form a wall across the road: a blocked cross-section needs cars within
        // a car length of each other along the course, and the slot pitch is an
        // order of magnitude past that.
        let car_length = (v.half_length + r.traffic_half_length) * 2.0;
        assert!(
            r.traffic_spacing > car_length * 5.0,
            "slots {} m apart cannot block a {car_length} m cross-section",
            r.traffic_spacing
        );
        // But close enough that traffic is a constant presence: at the top speed
        // the player meets one every couple of seconds.
        let closing = v.top_speed - r.traffic_speed_max;
        assert!(
            r.traffic_spacing / closing < 2.5,
            "traffic arrives every {} s at top speed, which is scenery",
            r.traffic_spacing / closing
        );
        // A newly spawned car is always more than a second of warning away, even
        // at the boosted top speed and even against the slowest traffic.
        let worst_closing = v.top_speed + v.boost_top_speed_bonus - r.traffic_speed_min;
        assert!(
            r.traffic_safe_ahead / worst_closing > 1.0,
            "only {} s of warning at the worst closing speed",
            r.traffic_safe_ahead / worst_closing
        );
        assert!(r.traffic_safe_behind > 0.0 && r.traffic_safe_behind < r.traffic_safe_ahead);
        assert!(
            r.traffic_safe_ahead < r.traffic_ahead,
            "the safety window is a floor on the spawn horizon, not a replacement for it"
        );
    }

    #[test]
    fn the_fixed_step_is_sixty_hertz() {
        assert_eq!(FIXED_STEP_NANOS, 16_666_667);
        assert!((DT - FIXED_STEP_NANOS as f32 / 1.0e9).abs() < 1.0e-6);
    }
}
