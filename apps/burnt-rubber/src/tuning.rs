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
    /// Fraction of forward speed kept after a barrier impact.
    pub barrier_speed_keep: f32,
    /// Fraction of the incoming lateral speed reflected off a barrier.
    pub barrier_restitution: f32,
    /// How fast a car pressed against a barrier is turned to run *along* it
    /// (per second). Without this a car that noses into a wall grinds there
    /// forever: with zero yaw authority of its own and no rotation from the
    /// contact, nothing in the model ever points it back down the road.
    pub barrier_align: f32,
    /// Fraction of forward speed kept after hitting a traffic car.
    pub traffic_speed_keep: f32,
    /// Sideways shove (m/s) applied when the player hits traffic.
    pub traffic_deflect: f32,
    /// Simulation steps a collision keeps the impact state raised.
    pub impact_steps: u32,
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
        barrier_speed_keep: 0.66,
        barrier_restitution: 0.34,
        barrier_align: 7.0,
        traffic_speed_keep: 0.58,
        traffic_deflect: 7.0,
        impact_steps: 26,
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
    /// The previous pass tightened the distance to fix a car that read too
    /// small, and left the height alone at 2.0 m — twice the car's own
    /// [`crate::render::car_model::ROOF_HEIGHT`] of 0.98 m, only 5.5 m back.
    /// That is a 20° depression angle onto a subject four metres away, and the
    /// near-field perspective at that range turns the roof into a wide flat
    /// slab: measured against the art target the car's *projected* silhouette
    /// came out 1.98 tall per unit wide where the target reads 1.19, its rear
    /// bumper ran to 82% of frame height and collided with the on-screen touch
    /// controls, and the road ahead was squeezed into the top quarter.
    ///
    /// So the correction is almost entirely vertical. Dropping to 1.35 m puts
    /// the eye 0.37 m above the roof rather than a full metre above it and
    /// nearly halves the depression angle (20.0° → 12.9°), which is what
    /// restores the target's read: a rear-three-quarter silhouette — taillights
    /// and rear glass, a sliver of roof — sitting on open tarmac with the road
    /// running away underneath it. Distance moves only enough to hold the car's
    /// on-screen *width* where the target has it (the one thing the previous
    /// pass got right), and `distance_high` moves by the same factor so the rig
    /// keeps its speed character.
    ///
    /// The eye now sits 0.45 m above the `min_ground_clearance` floor rather
    /// than 1.1 m, so that clamp does more work over undulating terrain — it is
    /// a safety floor, not a framing knob, and it is deliberately left alone.
    pub const DEFAULT: CameraTuning = CameraTuning {
        distance_low: 5.9,
        distance_high: 7.7,
        distance_boost: 1.1,
        height: 1.35,
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
        impact_decay: 7.5,
        min_ground_clearance: 0.9,
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
    pub lane_width: f32,
    /// Spacing of a lane dash plus its gap (m).
    pub dash_period: f32,
    /// Length of the painted part of a dash (m).
    pub dash_length: f32,
    /// Spacing of roadside reflector posts (m).
    pub post_spacing: f32,
}

impl CourseTuning {
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
        min_half_width: 6.0,
        max_half_width: 11.0,
        correction_passes: 6,
        shoulder: 1.6,
        verge: 5.0,
        lane_width: 3.5,
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
    pub camera: CameraTuning,
    pub course: CourseTuning,
    pub race: RaceTuning,
}

impl Tuning {
    /// The shipping tuning.
    pub const DEFAULT: Tuning = Tuning {
        vehicle: VehicleTuning::DEFAULT,
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
        assert!(v.barrier_align > 0.0, "a wall always turns you back along itself");
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

    #[test]
    fn the_fixed_step_is_sixty_hertz() {
        assert_eq!(FIXED_STEP_NANOS, 16_666_667);
        assert!((DT - FIXED_STEP_NANOS as f32 / 1.0e9).abs() < 1.0e-6);
    }
}
