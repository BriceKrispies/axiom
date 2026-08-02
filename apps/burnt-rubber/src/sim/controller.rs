//! The arcade car controller: one fixed step of driving.
//!
//! The model, in order, is: smooth the steering input → rotate the chassis →
//! decompose the (unchanged) velocity into the new chassis frame → apply
//! longitudinal forces → bleed the lateral component according to grip →
//! integrate position → re-seat the car on the road surface.
//!
//! The important consequence of that ordering is that **rotating the chassis is
//! what creates a slide**. Nothing anywhere models a tyre. When you flick the
//! wheel, the nose swings; the velocity does not; the difference is the lateral
//! component; grip decides how quickly it goes away. Turn the grip down (the
//! handbrake, or dirt) and the difference survives longer, which is a drift.
//! This gives an authored, always-stable arcade car with a handful of numbers a
//! human can actually tune, and it is why a bad contact can slow the car or
//! shove it sideways but can never blow up the integrator: every quantity is a
//! bounded velocity, never an accumulated force.

use axiom_math::Vec3;

use crate::command::DriveCommand;
use crate::track::{shortest_angle, Track, TrackSample};
use crate::tuning::{CollisionTuning, Tuning, VehicleTuning, DT};

use super::car::{CarState, Surface};
use super::contact::ContactState;

/// Position is integrated in this many equal sub-moves per fixed step, each with
/// its own boundary check. Two sub-moves at the boosted top speed is under a
/// metre of travel per check — far shorter than the car, the traffic, or the
/// barrier's thickness, so nothing can pass through anything.
///
/// It is a **constant**, not a speed- or frame-rate-derived count: the number of
/// substeps is part of the simulation's definition, so replay is exact.
pub const POSITION_SUBSTEPS: u32 = 2;

/// How far either side of the previous progress the re-localisation searches.
/// One boosted step covers under 2 m; 80 m is a fifty-fold margin, which is what
/// makes the bounded search safe rather than merely fast.
pub const LOCALISE_WINDOW: f32 = 80.0;

/// Speed (m/s) below which steering authority is scaled down so a stationary car
/// cannot pirouette on the spot.
const PIVOT_SPEED: f32 = 6.0;

/// The outcome of one controller step that the rest of the simulation needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepReport {
    /// Forward acceleration over the step (m/s²) — the camera's pull-back.
    pub forward_accel: f32,
    /// Whether the car crossed from gripping to sliding this step.
    pub drift_started: bool,
    /// Distance travelled along the course this step (m), signed.
    pub distance_delta: f32,
    /// The strongest barrier contact resolved during this step's sub-moves.
    ///
    /// Barriers are resolved *inside* the position integration, so the
    /// controller is the only code that knows one happened. Reporting it here —
    /// rather than leaving the simulation to infer it from the car's impact
    /// counter — is what keeps "a wall was hit" a fact rather than a deduction.
    pub barrier_impact: Option<super::contact::Impact>,
}

/// Advance the car one fixed step.
///
/// `boost_available` gates the boost force: the meter is owned by
/// [`super::boost`], and the controller only asks whether it may pull.
///
/// `contact` is the live collision state, and it is threaded in here rather than
/// resolved above because **barriers are resolved inside the position
/// integration** (see [`integrate`]): the sub-move loop is the only code that
/// knows a wall was touched, so it is the only code that can consult the episode
/// ledger about whether that touch is a new collision or the same one still in
/// progress. It is also where the recovery assist has to act, because the assist
/// is a modification of ordinary driving — extra acceleration, extra lateral
/// bleed, a gentle heading pull — not a separate motion applied afterwards.
///
/// `rails` selects the lateral model, and is the only thing about this function
/// that differs between the two games. `None` is the wheel game: the chassis is
/// rotated by the steering input and the grip model decides how much of the
/// velocity survives. `Some(state)` is the phone game: [`super::rails::guide`]
/// drives the car to a chosen lane instead, and the grip model is skipped
/// because there is no slide to bleed. Everything after the lateral model —
/// longitudinal forces, the integrator, collisions, the surface classifier — is
/// shared, which is what keeps the two games one game.
///
/// A railed car takes the collision's *deflection* (its lane solver then carries
/// it back, which is exactly the phone game's version of correcting) but not its
/// yaw disturbance: a car on rails cannot be spun, and pretending otherwise
/// would be a rotation the lane solver immediately overwrote.
pub fn step(
    car: &mut CarState,
    command: DriveCommand,
    track: &Track,
    tuning: &Tuning,
    boost_available: bool,
    contact: &mut ContactState,
    rails: Option<&mut super::rails::RailsState>,
) -> StepReport {
    let vehicle = &tuning.vehicle;
    let collision = &tuning.collision;
    let command = command.sanitised();
    let speed_before = car.forward_speed;
    let distance_before = car.distance;
    let was_drifting = car.drifting;
    // Two assists, two lifetimes: the throttle help runs its full second, the
    // stabilisation stops the moment the car is steady. See [`super::contact`].
    let assist = contact.recovery_assist();
    let stabilise = contact.stabilise_assist();

    // `>= 0` rather than `> 0`: boost applies its own throttle, so it has to be
    // able to launch a stopped car. It still refuses while reversing.
    car.boosting = command.boost & boost_available & (car.forward_speed >= 0.0);

    let on_rails = match rails {
        Some(state) => {
            super::rails::guide(car, command, track, state);
            true
        }
        None => {
            steer(car, command, vehicle);
            let road_heading = track.sample_at(car.distance).heading;
            rotate_chassis(car, command, vehicle, collision, road_heading, stabilise);
            false
        }
    };
    longitudinal(car, command, vehicle, collision, assist);
    // The grip model exists to bleed a slide. A railed car has no slide: its
    // lateral velocity is the lane solver's output, and bleeding it would fight
    // the solver for control of the same channel.
    if !on_rails {
        lateral_grip(car, command, vehicle);
        // Recovery trims the *excess* slide only, so the collision's readable
        // deflection survives and the spin it would otherwise become does not.
        super::contact::recovery_damp_lateral(car, stabilise, collision);
    }
    let barrier_impact = integrate(car, track, vehicle, collision, contact);
    settle_onto_the_road(car, track, vehicle);
    classify_surface(car, track);
    update_drift(car, vehicle);
    decay_impact(car);

    car.wheel_spin = (car.wheel_spin + car.forward_speed * DT / WHEEL_RADIUS)
        .rem_euclid(std::f32::consts::TAU);

    StepReport {
        forward_accel: (car.forward_speed - speed_before) / DT,
        drift_started: car.drifting & !was_drifting,
        distance_delta: car.distance - distance_before,
        barrier_impact,
    }
}

/// The visual wheel radius (m) the spin rate is derived from.
pub const WHEEL_RADIUS: f32 = 0.36;

/// Ramp the applied steering toward the commanded steering, and do nothing else.
///
/// This is the whole of what a **held** car does: the wheel is live so the
/// player can settle it before the flag drops, but nothing moves. It is a
/// separate entry point on purpose — the obvious way to hold a car is to stand
/// on the brake, and in this model that is precisely wrong, because at a
/// standstill the brake *is* reverse.
pub fn settle_steering(car: &mut CarState, command: DriveCommand, tuning: &VehicleTuning) {
    steer(car, command.sanitised(), tuning);
    car.yaw_rate = 0.0;
    car.forward_speed = 0.0;
    car.lateral_speed = 0.0;
    car.vertical_speed = 0.0;
}

/// Ramp the applied steering toward the commanded steering.
fn steer(car: &mut CarState, command: DriveCommand, tuning: &VehicleTuning) {
    let delta = command.steer - car.steer;
    let step = tuning.steer_input_rate * DT;
    car.steer += delta.clamp(-step, step);
}

/// The peak yaw rate (rad/s) available at `speed`.
///
/// This is *the* handling curve, so it is a named function rather than three
/// lines buried in the integrator: it can be plotted, it can be tested for its
/// shape directly, and a tuning pass can reason about it without also reasoning
/// about drift recovery and the handbrake, which are separate multipliers
/// applied on top of it.
///
/// The shape is a hyperbola with a floor: full authority at a standstill, half
/// of it at `steer_falloff_speed`, and never below `steer_authority_floor` of
/// the peak — because a car with literally no steering at top speed is a rail,
/// not a racing game.
pub fn steering_authority(speed: f32, tuning: &VehicleTuning) -> f32 {
    let falloff = 1.0 / (1.0 + speed.abs() / tuning.steer_falloff_speed.max(1.0e-3));
    tuning.max_yaw_rate * falloff.max(tuning.steer_authority_floor)
}

/// Turn the chassis. This is the step that manufactures a slide.
fn rotate_chassis(
    car: &mut CarState,
    command: DriveCommand,
    tuning: &VehicleTuning,
    collision: &CollisionTuning,
    road_heading: f32,
    assist: f32,
) {
    // The front tyres are what point the car, so a front-biased mass bites
    // harder on entry. A 50/50 car scales by exactly 1.0 — this is a bias, not
    // free authority.
    let authority = steering_authority(car.speed(), tuning) * tuning.chassis.turn_in_scale();
    // Below walking pace the car pivots less and less, reaching zero at rest.
    let pivot = (car.forward_speed.abs() / PIVOT_SPEED).clamp(0.0, 1.0);
    // Reversing steers the other way, exactly as a real car does.
    let direction = if car.forward_speed < -0.2 { -1.0 } else { 1.0 };
    // The handbrake grants extra rotation, which is how a drift is *initiated*
    // deliberately rather than stumbled into.
    let handbrake = if command.handbrake {
        tuning.handbrake_yaw_gain
    } else {
        1.0
    };
    // Airborne, the car keeps rotating at a fraction of its ground authority —
    // enough to line up a landing, not enough to fly.
    let airborne = if car.grounded { 1.0 } else { AIR_STEER_SCALE };

    // The sign is the renderer's, not a preference. `Mat4::look_at` builds its
    // screen-right axis as `forward x up`, which places world `+X` on the LEFT
    // of the screen; increasing `yaw` swings the nose from `+Z` toward `+X`, so
    // an *increasing* yaw is a turn the player sees as going left. Steering
    // right is therefore a decreasing yaw. Every world-space test passes either
    // way — this is exactly the sign a game ships inverted.
    let mut yaw_rate = -(car.steer * authority * pivot * direction * handbrake * airborne);

    // Counter-steer assist: while sliding, the chassis is pulled back toward the
    // direction of travel. This is the forgiving drift window — without it every
    // slide ends as a spin, and the car stops being fun in about four seconds.
    if car.drifting {
        let travel = car.heading_of_travel();
        let travel_yaw = travel.x.atan2(travel.z);
        let error = shortest_angle(travel_yaw - car.yaw);
        yaw_rate += error * tuning.drift_recovery;
    }

    // The collision's own rotation, **added to** the player's rather than
    // replacing it. This is the "brief directional disturbance" of the design —
    // the car is knocked off line and the player corrects it — and because it is
    // decaying state rather than a one-shot rotation, the recovery assist has
    // something it can damp. See [`super::contact`].
    yaw_rate += car.impact_yaw_rate;

    // And the recovery assist's gentle pull back toward the line, which fades
    // out over the second after an impact and is zero the rest of the time.
    yaw_rate += super::contact::recovery_heading_pull(car, road_heading, assist, collision);

    // Capture the world velocity BEFORE the chassis turns.
    let velocity = car
        .forward()
        .mul_scalar(car.forward_speed)
        .add(car.right().mul_scalar(car.lateral_speed));

    // A rigid body rotates about its centre of mass, not about the middle of
    // its bodywork. Hold the CoG fixed across the rotation and let the chassis
    // centre swing around it: with the mass ahead of the wheelbase midpoint the
    // nose scribes the tighter arc and the tail steps out, which is exactly how
    // a front-biased car rotates. `forward_offset` is zero for a balanced car,
    // so this is a no-op unless the geometry actually says otherwise.
    let offset = tuning.chassis.forward_offset();
    let pivot_point = car.position.add(car.forward().mul_scalar(offset));

    car.yaw_rate = yaw_rate;
    car.yaw = (car.yaw + yaw_rate * DT).rem_euclid(std::f32::consts::TAU);

    car.position = pivot_point.subtract(car.forward().mul_scalar(offset));

    // Re-express that same velocity in the NEW chassis frame.
    //
    // This is the single line the whole vehicle model rests on, and it is worth
    // being explicit about why. The velocity is stored as a forward component
    // and a lateral component; if the chassis rotates and those two numbers are
    // left alone, the velocity has silently rotated *with* the car — the nose
    // and the direction of travel can never disagree, and there is no such
    // thing as a slide. Re-projecting the unchanged world velocity onto the new
    // axes is what makes the disagreement exist: whatever no longer lines up
    // with the nose is the lateral component, and that component *is* the drift.
    //
    // It also makes cornering cost speed for free, because the lateral part is
    // then bled away by grip. Note the rotation itself conserves speed exactly
    // (it is a rotation), so this can never add energy.
    let forward = car.forward();
    let right = car.right();
    car.forward_speed = velocity.dot(forward);
    car.lateral_speed = velocity.dot(right);
}

/// The fraction of ground steering authority available in the air.
const AIR_STEER_SCALE: f32 = 0.35;

/// The surface the car's **handling** sees, as opposed to the one it is standing
/// on.
///
/// Boost overrides it: a boosting car behaves as though it were on tarmac
/// wherever it actually is. That is deliberately a physics cheat — it is the
/// whole point of the power-up — and it is expressed once, here, rather than as
/// three separate "unless boosting" clauses scattered through the throttle, the
/// drag and the grip, which is how the three quietly drift out of agreement.
///
/// `car.surface` keeps reporting the truth, so the HUD still says OFF ROAD and
/// the tyres still throw dirt; it is only the *penalty* that is suspended.
fn handling_surface(car: &CarState) -> Surface {
    if car.boosting {
        Surface::Tarmac
    } else {
        car.surface
    }
}

/// Throttle, brake, reverse, boost, drag.
fn longitudinal(
    car: &mut CarState,
    command: DriveCommand,
    tuning: &VehicleTuning,
    collision: &CollisionTuning,
    assist: f32,
) {
    let off_road = handling_surface(car).is_off_road();
    let surface_scale = if off_road {
        tuning.offroad_accel_scale
    } else {
        1.0
    };
    // Airborne wheels drive nothing.
    let traction = if car.grounded { 1.0 } else { 0.0 };

    let ceiling = tuning.top_speed
        + if car.boosting {
            tuning.boost_top_speed_bonus
        } else {
            0.0
        };
    let accel = tuning.accel
        + if car.boosting {
            tuning.boost_accel_bonus
        } else {
            0.0
        };

    // Acceleration tapers as the speed approaches its ceiling: violent off the
    // line, still pulling at 250 km/h, asymptotic at the top.
    // Boost presses the throttle for you. Holding a button that means "go
    // faster" and *also* having to hold the accelerator is a control scheme, not
    // a power-up.
    let throttle = if car.boosting {
        1.0f32.max(command.throttle)
    } else {
        command.throttle
    };
    // The recovery assist's share of the throttle.
    //
    // A bounded *fraction* of the car's own acceleration, fading out over the
    // second after an impact, and applied only while the player is asking for
    // throttle: this is forgiving handling, not a rescue. It deliberately reads
    // nothing from the boost meter and writes nothing to it — being knocked into
    // a car must never be a way to earn or spend boost, and it must never look
    // like one either (no widened field of view, no boost cue, no streaks).
    let recovered = 1.0 + collision.recovery_accel_gain * assist;
    let headroom = (1.0 - (car.forward_speed.max(0.0) / ceiling).clamp(0.0, 1.0)).powf(ACCEL_CURVE);
    car.forward_speed += accel * throttle * headroom * surface_scale * traction * recovered * DT;

    // Braking bleeds forward motion; once stopped, the same input reverses.
    let braking = command.brake * traction;
    let moving_forward = car.forward_speed > REVERSE_THRESHOLD;
    if moving_forward {
        // Longitudinal load transfer: braking pitches the car onto its nose,
        // and the front axle does most of the stopping, so the weight arriving
        // there is worth real stopping power. `pitch_leverage` is `h /
        // wheelbase` — the same free-body ratio as the roll term, taken about
        // the axles — so a taller mass transfers more. Normalised against a
        // 50/50 car with no transfer, so this is a bias rather than a bonus.
        let decel_g = tuning.brake_decel * braking / tuning.gravity.max(1.0e-3);
        let dynamic_front =
            (tuning.chassis.front_load() + tuning.chassis.pitch_leverage() * decel_g).clamp(0.0, 1.0);
        let brake_scale = (0.5 + dynamic_front).clamp(BRAKE_SCALE_FLOOR, BRAKE_SCALE_CEILING);
        car.forward_speed =
            (car.forward_speed - tuning.brake_decel * braking * brake_scale * DT).max(0.0);
    } else {
        car.forward_speed = (car.forward_speed - tuning.reverse_accel * braking * DT)
            .max(-tuning.reverse_top_speed);
    }

    // Drag and rolling resistance, always.
    let drag = tuning.coast_drag + if off_road { tuning.offroad_drag } else { 0.0 };
    car.forward_speed *= (-drag * DT).exp();
    let resistance = tuning.rolling_resistance * DT * traction;
    car.forward_speed -= car.forward_speed.signum() * resistance.min(car.forward_speed.abs());

    // A hard ceiling above the boosted top speed catches anything a collision or
    // a downhill could otherwise add without bound.
    let hard_ceiling = ceiling * SPEED_HEADROOM;
    car.forward_speed = car
        .forward_speed
        .clamp(-tuning.reverse_top_speed, hard_ceiling);
}

/// Exponent on the acceleration taper. Below 1 keeps real pull at high speed.
const ACCEL_CURVE: f32 = 0.6;

/// Forward speed below which the brake input becomes reverse (m/s).
const REVERSE_THRESHOLD: f32 = 0.6;

/// How far above the boosted top speed the hard clamp sits.
const SPEED_HEADROOM: f32 = 1.12;

/// Bleed the lateral velocity according to the current grip.
fn lateral_grip(car: &mut CarState, command: DriveCommand, tuning: &VehicleTuning) {
    let base = match handling_surface(car) {
        Surface::Tarmac => tuning.grip,
        Surface::Shoulder => (tuning.grip + tuning.offroad_grip) * 0.5,
        Surface::OffRoad => tuning.offroad_grip,
    };
    let grip = if command.handbrake {
        tuning.handbrake_grip
    } else {
        base
    };
    // Load transfer. Cornering throws weight onto the outside wheels, and a
    // tyre's grip grows less than linearly with the load on it, so an unevenly
    // loaded pair grips less than an evenly loaded one. How much weight moves is
    // set by the height of the centre of gravity against the half-track — the
    // rollover ratio — which is why a low car corners better than a tall one,
    // here for the same reason it does in the world.
    //
    // The lateral acceleration is the centripetal term `v * omega`, measured in
    // the model's own gravity so the ratio stays consistent with the rest of the
    // simulation (`tuning.gravity` is the arcade g the car falls under, not
    // 9.81).
    let lateral_g = (car.forward_speed * car.yaw_rate).abs() / tuning.gravity.max(1.0e-3);
    let transfer = tuning.chassis.lateral_transfer(lateral_g);
    car.load_transfer = transfer;

    // In the air there is nothing to grip against; the slide is preserved.
    let effective = if car.grounded {
        grip * tuning.chassis.grip_scale(transfer)
    } else {
        0.0
    };
    car.lateral_speed *= (-effective * DT).exp();
    // Below a millimetre a second the slide is over; snapping it to zero keeps
    // the drift flag from flickering on floating-point dust.
    if car.lateral_speed.abs() < LATERAL_EPSILON {
        car.lateral_speed = 0.0;
    }
}

/// Floor and ceiling on the braking scale from longitudinal load transfer. A
/// car whose mass is right over the front axle does not stop twice as fast as a
/// balanced one, and a rear-biased one still has brakes.
///
/// The ceiling has to sit clear of what the shipping car actually reaches
/// (about `1.30`), for the same reason the rollover threshold has to sit clear
/// of the car's real cornering load: a bound the normal case is pinned against
/// is a bound that quietly deletes the effect it was meant to limit. A clamp
/// should catch the absurd, not the ordinary.
const BRAKE_SCALE_FLOOR: f32 = 0.85;
/// See [`BRAKE_SCALE_FLOOR`].
const BRAKE_SCALE_CEILING: f32 = 1.45;

/// Lateral speed below which the slide is considered finished (m/s).
const LATERAL_EPSILON: f32 = 1.0e-3;

/// Move the car, in bounded sub-moves, and re-localise it onto the course.
/// Returns the strongest barrier contact resolved along the way.
fn integrate(
    car: &mut CarState,
    track: &Track,
    tuning: &VehicleTuning,
    collision: &CollisionTuning,
    contact: &mut ContactState,
) -> Option<super::contact::Impact> {
    let sub_dt = DT / POSITION_SUBSTEPS as f32;
    let mut strongest: Option<super::contact::Impact> = None;
    for _ in 0..POSITION_SUBSTEPS {
        // The planar velocity is re-read each sub-move, so a barrier resolved in
        // the first one actually changes where the second one goes — which is
        // what stops a fast car from being pushed out of a wall and straight
        // back into it within a single step.
        let planar = car
            .forward()
            .mul_scalar(car.forward_speed)
            .add(car.right().mul_scalar(car.lateral_speed));
        car.position = car.position.add(planar.mul_scalar(sub_dt));
        car.vertical_speed -= tuning.gravity * sub_dt;
        car.position.y += car.vertical_speed * sub_dt;
        let (distance, lateral) = track.localise(car.position, car.distance, LOCALISE_WINDOW);
        car.distance = distance;
        car.lateral = lateral;
        if let Some(impact) =
            super::collision::resolve_barrier(car, track, tuning, collision, contact)
        {
            let stronger = strongest.map_or(true, |best| impact.strength > best.strength);
            if stronger {
                strongest = Some(impact);
            }
        }
    }
    strongest
}

/// Hold the car on the road surface, and let it leave the ground over a crest.
///
/// The rule is one line: gravity always applies, and the road is a floor. Over a
/// crest the floor falls away faster than gravity pulls, so the car is briefly
/// airborne with no special "jump" case anywhere; on the ground the vertical
/// velocity is set to the rate the road itself is climbing, so a hill is
/// followed rather than bounced along.
fn settle_onto_the_road(car: &mut CarState, track: &Track, tuning: &VehicleTuning) {
    let sample = track.interpolated_at(car.distance);
    let surface_y = road_height(&sample, car.lateral);
    if car.position.y <= surface_y {
        car.position.y = surface_y;
        // The rate the road climbs at the car's current speed. Following it is
        // what makes a hill feel like a hill instead of a series of landings.
        let climb = sample.grade * car.forward_speed;
        car.vertical_speed = climb.max(0.0).max(car.vertical_speed.max(climb));
        car.grounded = true;
        car.airborne_steps = 0;
    } else {
        // A shallow gap is closed smoothly rather than snapped, which is the
        // visual "suspension settle" without any suspension.
        let gap = car.position.y - surface_y;
        if gap < SETTLE_GAP && car.vertical_speed <= 0.0 {
            car.position.y -= gap * (tuning.ground_settle_rate * DT).min(1.0);
            car.grounded = true;
            car.airborne_steps = 0;
        } else {
            car.grounded = false;
            car.airborne_steps = car.airborne_steps.saturating_add(1);
        }
    }
    if car.grounded {
        car.vertical_speed = car.vertical_speed.max(tuning.ground_snap_speed);
    }
}

/// Height gap (m) below which the car is settled onto the road rather than
/// treated as airborne.
const SETTLE_GAP: f32 = 0.45;

/// The road surface height at `lateral` metres from the centre of `sample`,
/// including the banking.
pub fn road_height(sample: &TrackSample, lateral: f32) -> f32 {
    sample.at_lateral(lateral).y
}

/// Decide which surface the car is on.
fn classify_surface(car: &mut CarState, track: &Track) {
    let sample = track.sample_at(car.distance);
    let offset = car.lateral.abs();
    car.surface = if offset <= sample.half_width {
        Surface::Tarmac
    } else if offset <= sample.half_width + track.shoulder() {
        Surface::Shoulder
    } else {
        Surface::OffRoad
    };
}

/// Maintain the drift flag with hysteresis.
fn update_drift(car: &mut CarState, tuning: &VehicleTuning) {
    let sliding = car.lateral_speed.abs();
    // Hysteresis: entering a drift takes a real slide, leaving it takes the
    // slide genuinely stopping. Without the gap the flag chatters and the boost
    // reward becomes a strobe.
    car.drifting = if car.drifting {
        sliding > tuning.drift_release
    } else {
        sliding > tuning.drift_threshold
    };
    car.drift_steps = if car.drifting {
        car.drift_steps.saturating_add(1)
    } else {
        0
    };
}

/// Age out the impact state.
fn decay_impact(car: &mut CarState) {
    car.impact_steps = car.impact_steps.saturating_sub(1);
    if car.impact_steps == 0 {
        car.impact_strength = 0.0;
    }
}

/// Place the car at a track sample, at rest and facing down the road. Used by
/// the start line, the reset, and the restart.
pub fn place_on_track(car: &mut CarState, sample: &TrackSample, lateral: f32) {
    let forward = sample.flat_forward();
    car.position = sample.at_lateral(lateral).add(Vec3::new(0.0, RESET_LIFT, 0.0));
    car.yaw = forward.x.atan2(forward.z);
    car.yaw_rate = 0.0;
    car.forward_speed = 0.0;
    car.lateral_speed = 0.0;
    car.vertical_speed = 0.0;
    car.grounded = true;
    car.airborne_steps = 0;
    car.steer = 0.0;
    car.distance = sample.distance;
    car.lateral = lateral;
    car.surface = Surface::Tarmac;
    car.drifting = false;
    car.drift_steps = 0;
    car.impact_steps = 0;
    car.impact_strength = 0.0;
    car.impact_yaw_rate = 0.0;
    car.boosting = false;
    car.stuck_steps = 0;
}

/// How far above the road a reset places the car (m), so it settles down onto
/// the surface rather than starting inside it.
const RESET_LIFT: f32 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::chassis::ChassisGeometry;
    use crate::tuning::{CourseTuning, Tuning};

    fn fixture() -> (Track, CarState, VehicleTuning) {
        let track = Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT);
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        place_on_track(&mut car, &track.sample_at(0.0), 0.0);
        (track, car, VehicleTuning::DEFAULT)
    }

    /// The full tuning surface around a test's chosen vehicle. These tests vary
    /// the *car*, so everything else stays shipping.
    fn tuned(vehicle: &VehicleTuning) -> Tuning {
        Tuning {
            vehicle: *vehicle,
            ..Tuning::DEFAULT
        }
    }

    /// One controller step with a throwaway contact state.
    ///
    /// Fine for every test in this module: these are handling tests, and a car
    /// that never touches anything never opens a contact episode. The tests that
    /// are genuinely about contact carry a persistent state and live in
    /// [`crate::sim::collision`] and [`crate::sim::contact`].
    fn once(
        car: &mut CarState,
        command: DriveCommand,
        track: &Track,
        vehicle: &VehicleTuning,
        boost: bool,
    ) -> StepReport {
        let tuning = tuned(vehicle);
        let mut contact = ContactState::new();
        let report = step(car, command, track, &tuning, boost, &mut contact, None);
        contact.advance(car, &tuning.collision);
        report
    }

    fn drive(car: &mut CarState, track: &Track, tuning: &VehicleTuning, command: DriveCommand, steps: u32) {
        for _ in 0..steps {
            once(car, command, track, tuning, true);
        }
    }

    #[test]
    fn throttle_accelerates_the_car() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 30);
        assert!(car.forward_speed > 10.0, "half a second of throttle: {}", car.forward_speed);
        let after_half = car.forward_speed;
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 30);
        assert!(car.forward_speed > after_half, "and it keeps building");
    }

    /// "Responsive within the first few seconds" is the headline requirement, so
    /// it is a test rather than a hope.
    #[test]
    fn the_car_is_genuinely_quick_off_the_line() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 60);
        assert!(
            car.forward_speed > 25.0,
            "one second gets past 90 km/h, got {} m/s",
            car.forward_speed
        );
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 240);
        assert!(
            car.forward_speed > 70.0,
            "five seconds gets past 250 km/h, got {} m/s",
            car.forward_speed
        );
    }

    /// Driven on the racing line (so the measurement is of the car, not of its
    /// argument with a guardrail), the speed climbs to just under the top speed
    /// and stops there.
    #[test]
    fn speed_settles_below_the_top_speed_and_never_runs_away() {
        let (track, mut car, t) = fixture();
        let mut best = 0.0f32;
        for _ in 0..6_000 {
            let command = crate::script::autopilot(&car, &track);
            once(&mut car, command, &track, &t, false);
            best = best.max(car.forward_speed);
            assert!(car.forward_speed <= t.top_speed * SPEED_HEADROOM);
        }
        assert!(best > t.top_speed * 0.85, "and it does get near it: {best}");
        assert!(car.is_finite());
    }

    #[test]
    fn braking_reduces_forward_speed_faster_than_coasting() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 180);
        let entry = car.forward_speed;

        let mut braked = car;
        drive(&mut braked, &track, &t, DriveCommand { brake: 1.0, ..DriveCommand::IDLE }, 30);
        let mut coasted = car;
        drive(&mut coasted, &track, &t, DriveCommand::IDLE, 30);

        assert!(braked.forward_speed < coasted.forward_speed, "braking beats coasting");
        assert!(braked.forward_speed < entry * 0.7, "and it is forceful");
    }

    #[test]
    fn zero_input_coasts_down_deterministically_without_reversing() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 120);
        let entry = car.forward_speed;
        drive(&mut car, &track, &t, DriveCommand::IDLE, 600);
        assert!(car.forward_speed < entry, "coasting sheds speed");
        assert!(car.forward_speed >= 0.0, "and never rolls backwards on its own");
    }

    #[test]
    fn reverse_engages_from_rest_and_is_bounded() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand { brake: 1.0, ..DriveCommand::IDLE }, 600);
        assert!(car.forward_speed < -1.0, "the car reverses: {}", car.forward_speed);
        assert!(
            car.forward_speed >= -t.reverse_top_speed - 1.0e-3,
            "reverse is capped at {}, got {}",
            t.reverse_top_speed,
            car.forward_speed
        );
    }

    /// Screen-right, derived the way the engine's own view matrix derives it.
    ///
    /// `Mat4::look_at` sets its screen-X axis to `forward.cross(up)`. Checking
    /// steering against world `+X` instead would pass with the controls
    /// inverted, because world `+X` is on the *left* of the screen — so this is
    /// the only axis worth asserting against.
    fn screen_right(view_forward: Vec3) -> Vec3 {
        view_forward
            .cross(Vec3::UNIT_Y)
            .normalize()
            .expect("a horizontal view direction")
    }

    /// The test the inverted-controls bug would have failed: steering right must
    /// move the car toward the **right of the screen**.
    #[test]
    fn steering_turns_the_car_in_the_direction_the_player_sees() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 90);
        let start = car.position;
        let right_on_screen = screen_right(car.forward());

        let mut right = car;
        drive(&mut right, &track, &t, DriveCommand::turning(1.0), 60);
        let mut left = car;
        drive(&mut left, &track, &t, DriveCommand::turning(-1.0), 60);

        let drift_of = |c: &CarState| c.position.subtract(start).dot(right_on_screen);
        assert!(
            drift_of(&right) > 1.0,
            "steering right goes right on screen, got {}",
            drift_of(&right)
        );
        assert!(
            drift_of(&left) < -1.0,
            "steering left goes left on screen, got {}",
            drift_of(&left)
        );
        // And the two are genuinely opposite, not merely both drifting.
        assert!(drift_of(&right) > drift_of(&left) + 2.0);
    }

    /// The same claim one level down: full right lock produces a negative yaw
    /// rate, because increasing yaw is a left turn on screen.
    #[test]
    fn full_right_lock_decreases_yaw() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 90);
        drive(&mut car, &track, &t, DriveCommand::turning(1.0), 20);
        assert!(car.yaw_rate < 0.0, "yaw rate {} should be negative", car.yaw_rate);
        drive(&mut car, &track, &t, DriveCommand::turning(-1.0), 40);
        assert!(car.yaw_rate > 0.0, "and positive the other way");
    }

    /// The steering-authority curve is the single most important handling
    /// number, so its shape is pinned directly.
    #[test]
    fn steering_authority_falls_with_speed_but_never_to_zero() {
        let t = VehicleTuning::DEFAULT;
        let at_rest = steering_authority(0.0, &t);
        let cruising = steering_authority(t.steer_falloff_speed, &t);
        let flat_out = steering_authority(t.top_speed, &t);

        assert!((at_rest - t.max_yaw_rate).abs() < 1.0e-5, "full authority at rest");
        assert!(
            (cruising - t.max_yaw_rate * 0.5).abs() < 1.0e-4,
            "half at the falloff speed: {cruising}"
        );
        assert!(flat_out < cruising, "and it keeps falling: {flat_out}");
        assert!(
            flat_out >= t.max_yaw_rate * t.steer_authority_floor,
            "but never below the floor: {flat_out}"
        );
        assert!(flat_out > 0.25, "which is still real steering at top speed");
        // Monotone, and symmetric in reverse.
        let mut previous = f32::INFINITY;
        for i in 0..200 {
            let a = steering_authority(i as f32, &t);
            assert!(a <= previous + 1.0e-6, "monotone at {i} m/s");
            assert!(a <= t.max_yaw_rate + 1.0e-6, "never above the ceiling");
            assert_eq!(a, steering_authority(-(i as f32), &t));
            previous = a;
        }
    }

    /// And its *consequence* is pinned separately, through the integrator: a
    /// gentle input (no drift, no handbrake) turns the car more per second at
    /// low speed than at high speed.
    #[test]
    fn a_gentle_input_turns_the_car_more_at_low_speed() {
        let (track, _, t) = fixture();
        let yaw_change_at = |target: f32| {
            let mut car = CarState::parked(Vec3::ZERO, 0.0);
            place_on_track(&mut car, &track.sample_at(200.0), 0.0);
            for _ in 0..4_000 {
                if car.forward_speed >= target {
                    break;
                }
                once(&mut car, DriveCommand::FLAT_OUT, &track, &t, false);
            }
            let gentle = DriveCommand { steer: 0.3, ..DriveCommand::IDLE };
            // Let the steering ramp reach its held value first.
            for _ in 0..30 {
                once(&mut car, gentle, &track, &t, false);
            }
            let before = car.yaw;
            for _ in 0..30 {
                once(&mut car, gentle, &track, &t, false);
            }
            assert!(!car.drifting, "a gentle input does not slide the car");
            shortest_angle(car.yaw - before).abs()
        };
        let slow = yaw_change_at(18.0);
        let fast = yaw_change_at(78.0);
        assert!(
            slow > fast * 1.5,
            "the car turns more sharply at low speed: {slow} vs {fast}"
        );
        assert!(fast > 0.02, "and still turns at high speed: {fast}");
    }

    /// The trap this function exists to avoid: braking a stationary car is
    /// reverse, so "hold it on the line with the brake" drives it backwards.
    #[test]
    fn braking_a_stationary_car_is_reverse_not_a_hold() {
        let (track, mut car, t) = fixture();
        let brake = DriveCommand { brake: 1.0, ..DriveCommand::IDLE };
        for _ in 0..135 {
            once(&mut car, brake, &track, &t, false);
        }
        assert!(
            car.forward_speed < -5.0,
            "the brake reverses a stopped car ({} m/s) — this is why holding is a \
             separate operation",
            car.forward_speed
        );
    }

    /// And what holding actually does: nothing moves, but the wheel still works.
    #[test]
    fn settling_the_steering_holds_the_car_completely_still() {
        let (track, mut car, t) = fixture();
        let start = car.position;
        let turning = DriveCommand { steer: 1.0, ..DriveCommand::FLAT_OUT };
        for _ in 0..135 {
            settle_steering(&mut car, turning, &t);
        }
        assert_eq!(car.position, start, "the car has not moved at all");
        assert_eq!(car.forward_speed, 0.0);
        assert_eq!(car.lateral_speed, 0.0);
        assert!(car.steer > 0.9, "but the wheel has settled: {}", car.steer);
        let _ = track;
    }

    // ---------------------------------------------------------------------
    // The recovery assist, as the controller actually applies it.
    // ---------------------------------------------------------------------

    /// A contact state with recovery armed by a genuine bump, so these tests
    /// exercise the same path the game does rather than poking the state.
    fn after_a_bump(car: &mut CarState) -> ContactState {
        use crate::sim::contact::{ContactFacts, Obstacle};
        let mut contact = ContactState::new();
        let facts = ContactFacts {
            obstacle: Obstacle::Traffic { slot: 1 },
            normal: car.right(),
            bias: car.right(),
            normal_speed: 18.0,
            player_speed: car.forward_speed,
            obstacle_speed: 30.0,
            squareness: 0.6,
            rear_hit: false,
        };
        contact
            .respond(car, &facts, &CollisionTuning::DEFAULT)
            .expect("a bump");
        assert!(contact.is_recovering());
        contact
    }

    /// The assist adds throttle, and only while the player is asking for it.
    #[test]
    fn recovery_acceleration_helps_under_throttle_and_fades_away() {
        let (track, car, t) = fixture();
        let tuning = tuned(&t);
        let run = |assisted: bool| {
            let mut c = car;
            c.forward_speed = 55.0;
            let mut contact = after_a_bump(&mut c);
            (!assisted).then(|| contact.clear());
            let start = c.forward_speed;
            let mut samples = Vec::new();
            for _ in 0..tuning.collision.recovery_steps {
                step(&mut c, DriveCommand::FLAT_OUT, &track, &tuning, false, &mut contact, None);
                contact.advance(&mut c, &tuning.collision);
                samples.push(contact.recovery_assist());
            }
            (c.forward_speed - start, samples)
        };
        let (assisted, fade) = run(true);
        let (plain, _) = run(false);
        assert!(
            assisted > plain,
            "the assist added nothing: {assisted} vs {plain} m/s gained"
        );
        // A bounded fraction of the car's own acceleration, never a boost.
        assert!(
            assisted < plain * (1.0 + tuning.collision.recovery_accel_gain) + 1.0,
            "the assist is a fraction of the throttle, not a power-up: {assisted} vs {plain}"
        );
        // And it genuinely fades rather than switching off.
        assert!(fade.first().is_some_and(|a| *a > 0.5), "starts strong: {fade:?}");
        assert_eq!(fade.last().copied(), Some(0.0), "and finishes at nothing");
        assert!(
            fade.windows(2).all(|w| w[1] <= w[0] + 1.0e-6),
            "the fade never rises"
        );
    }

    /// **The assist is not an autopilot.** Full lock still turns the car, and it
    /// still turns it the way the player asked.
    #[test]
    fn recovery_never_overrides_the_players_steering() {
        let (track, car, t) = fixture();
        let tuning = tuned(&t);
        let turn = |steer: f32, recovering: bool| {
            let mut c = car;
            c.forward_speed = 55.0;
            let mut contact = after_a_bump(&mut c);
            (!recovering).then(|| contact.clear());
            // The disturbance the bump left is not part of what is being
            // measured — the question is only whether steering still works.
            c.impact_yaw_rate = 0.0;
            c.lateral_speed = 0.0;
            let before = c.yaw;
            for _ in 0..30 {
                let command = DriveCommand {
                    steer,
                    ..DriveCommand::FLAT_OUT
                };
                step(&mut c, command, &track, &tuning, false, &mut contact, None);
                contact.advance(&mut c, &tuning.collision);
            }
            shortest_angle(c.yaw - before)
        };
        let right = turn(1.0, true);
        let left = turn(-1.0, true);
        assert!(
            right.signum() != left.signum(),
            "steering still points the car both ways under the assist: {right} vs {left}"
        );
        assert!(right.abs() > 0.2 && left.abs() > 0.2, "and with real authority");

        // The assist costs the player some authority — that is what a bias
        // toward the road *is* — but nowhere near all of it.
        let free = turn(1.0, false);
        assert!(
            right.abs() > free.abs() * 0.5,
            "the assist ate {:.0}% of the steering",
            (1.0 - right.abs() / free.abs()) * 100.0
        );
    }

    /// The assist damps the collision's yaw disturbance, not the player's.
    #[test]
    fn recovery_damps_the_collisions_yaw_disturbance_faster_than_it_decays_alone() {
        let (track, car, t) = fixture();
        let tuning = tuned(&t);
        let settle = |recovering: bool| {
            let mut c = car;
            c.forward_speed = 55.0;
            let mut contact = after_a_bump(&mut c);
            (!recovering).then(|| contact.clear());
            c.impact_yaw_rate = 1.0;
            // Held sliding, so the "already stable" early exit cannot fire and
            // shorten the assisted run.
            for _ in 0..30 {
                c.lateral_speed = 8.0;
                step(&mut c, DriveCommand::FLAT_OUT, &track, &tuning, false, &mut contact, None);
                contact.advance(&mut c, &tuning.collision);
            }
            c.impact_yaw_rate.abs()
        };
        let assisted = settle(true);
        let alone = settle(false);
        assert!(
            assisted < alone,
            "the assist did not damp the kick: {assisted} vs {alone} rad/s"
        );
        assert!(alone > 0.0, "and it would still be ringing without it");
    }

    /// The disturbance is a rotation the player can drive out of, not a
    /// rotation applied to them once and forgotten.
    #[test]
    fn the_collisions_yaw_disturbance_turns_the_car_and_then_lets_go() {
        let (track, car, t) = fixture();
        let tuning = tuned(&t);
        let mut c = car;
        c.forward_speed = 55.0;
        let mut contact = ContactState::new();
        c.impact_yaw_rate = 1.2;
        let before = c.yaw;
        for _ in 0..10 {
            step(&mut c, DriveCommand::FLAT_OUT, &track, &tuning, false, &mut contact, None);
            contact.advance(&mut c, &tuning.collision);
        }
        assert!(
            shortest_angle(c.yaw - before).abs() > 0.05,
            "the kick actually swung the nose"
        );
        for _ in 0..180 {
            step(&mut c, DriveCommand::FLAT_OUT, &track, &tuning, false, &mut contact, None);
            contact.advance(&mut c, &tuning.collision);
        }
        assert_eq!(c.impact_yaw_rate, 0.0, "and then it is completely gone");
        assert!(c.is_finite());
    }

    /// The centre of gravity is *causal*, not decoration: raise it and the same
    /// corner, taken identically, throws more load onto the outside wheels and
    /// the car slides more. This is the whole claim of [`super::chassis`].
    #[test]
    fn a_higher_centre_of_gravity_slides_more_through_the_same_corner() {
        let (track, car, base) = fixture();
        // A *sustained, moderate* corner, on the tarmac, and the mean slide once
        // it has settled.
        //
        // Neither the endpoint nor the peak of a full-lock turn measures grip. At
        // the endpoint a tall car reads lower, because sliding earlier drops its
        // yaw rate. At full lock both cars end up fully sideways against a
        // barrier, and the peak is the impact rather than the tyres. A held
        // part-lock corner reaches a steady state in which the only thing setting
        // the lateral speed is how much grip survived the load transfer.
        //
        // **The car has to still be on the road when that is measured**, and this
        // is the part an earlier version of this test got wrong: accelerating in
        // a straight line down a road that curves, then holding lock for two
        // seconds, put the car fifteen metres from the centreline — five metres
        // past the tarmac, on dirt, where `offroad_grip` swamps the effect being
        // measured. It compared two dirt slides and happened to get the right
        // answer. So the warm-up follows the racing line and the corner is held
        // only as long as the car genuinely stays on the road, which the test
        // asserts rather than assumes.
        let corner = |height: f32| {
            let mut t = base;
            t.chassis = ChassisGeometry { cog_height: height, ..t.chassis };
            let mut c = car;
            for _ in 0..300 {
                let line = crate::script::autopilot(&c, &track);
                once(&mut c, line, &track, &t, true);
            }
            let turn = DriveCommand { steer: 0.45, ..DriveCommand::FLAT_OUT };
            drive(&mut c, &track, &t, turn, 20);
            let (mut slide, mut transfer) = (0.0f32, 0.0f32);
            for _ in 0..20 {
                once(&mut c, turn, &track, &t, true);
                assert!(
                    !c.surface.is_off_road(),
                    "the measurement left the tarmac at {} m, where the dirt decides the slide",
                    c.lateral
                );
                slide += c.lateral_speed.abs() / 20.0;
                transfer = transfer.max(c.load_transfer);
            }
            (slide, transfer)
        };
        let (low_slide, low_transfer) = corner(0.30);
        let (tall_slide, tall_transfer) = corner(0.40);
        assert!(
            tall_transfer > low_transfer,
            "the tall car transfers more load: {tall_transfer} vs {low_transfer}"
        );
        assert!(
            tall_slide > low_slide,
            "and so slides more: {tall_slide} m/s vs {low_slide} m/s"
        );
    }

    /// A straight line is untouched by the geometry — load transfer is a
    /// cornering phenomenon, so this must never become a general speed tax.
    #[test]
    fn the_centre_of_gravity_costs_nothing_in_a_straight_line() {
        let (track, car, base) = fixture();
        let straight = |height: f32| {
            let mut t = base;
            t.chassis = ChassisGeometry { cog_height: height, ..t.chassis };
            let mut c = car;
            drive(&mut c, &track, &t, DriveCommand::FLAT_OUT, 120);
            c.forward_speed
        };
        assert!(
            (straight(0.30) - straight(0.95)).abs() < 0.5,
            "a tall car accelerates the same down a straight"
        );
    }

    /// The chassis rotates about its mass, not about the middle of its
    /// bodywork, so a front-biased car's tail steps out as it turns.
    #[test]
    fn the_chassis_yaws_about_the_centre_of_gravity() {
        let (_track, car, base) = fixture();
        let swing = |bias: f32| {
            let mut t = base;
            t.chassis = ChassisGeometry { cog_from_front: bias, ..t.chassis };
            let mut c = car;
            c.forward_speed = 30.0;
            c.steer = 1.0;
            let before = c.position;
            let yaw_before = c.yaw;
            rotate_chassis(&mut c, DriveCommand::turning(1.0), &t, &CollisionTuning::DEFAULT, 0.0, 0.0);
            assert!((c.yaw - yaw_before).abs() > 0.0, "the car did turn");
            c.position.distance(before)
        };
        // A balanced car pivots in place; a front-biased one sweeps its centre.
        assert!(swing(0.5) < 1.0e-6, "a 50/50 car rotates about its own centre");
        assert!(
            swing(0.40) > 1.0e-4,
            "a front-biased car's body swings around the mass ahead of it"
        );
    }

    /// Front weight bites: the same input turns a front-biased car harder.
    #[test]
    fn a_front_biased_car_turns_in_harder() {
        let (track, car, base) = fixture();
        let turn = |bias: f32| {
            let mut t = base;
            t.chassis = ChassisGeometry { cog_from_front: bias, ..t.chassis };
            let mut c = car;
            drive(&mut c, &track, &t, DriveCommand::FLAT_OUT, 120);
            let yaw_before = c.yaw;
            drive(&mut c, &track, &t, DriveCommand::turning(1.0), 30);
            shortest_angle(c.yaw - yaw_before).abs()
        };
        assert!(
            turn(0.40) > turn(0.60),
            "front weight turns more than rear weight: {} vs {}",
            turn(0.40),
            turn(0.60)
        );
    }

    /// Weight over the front axle is weight over the brakes, so a front-biased
    /// car stops shorter — and neither bias is pinned against the clamp.
    #[test]
    fn a_front_biased_car_brakes_harder() {
        let (track, car, base) = fixture();
        let stop = |bias: f32| {
            let mut t = base;
            t.chassis = ChassisGeometry { cog_from_front: bias, ..t.chassis };
            let mut c = car;
            drive(&mut c, &track, &t, DriveCommand::FLAT_OUT, 180);
            let entry = c.forward_speed;
            drive(
                &mut c,
                &track,
                &t,
                DriveCommand { brake: 1.0, ..DriveCommand::IDLE },
                30,
            );
            entry - c.forward_speed
        };
        let front = stop(0.40);
        let rear = stop(0.60);
        assert!(
            front > rear,
            "front weight brakes harder: {front} m/s shed vs {rear} m/s"
        );
        assert!(
            (front - rear).abs() > 0.5,
            "and the difference survives the clamp rather than being flattened by it"
        );
    }

    #[test]
    fn a_stationary_car_cannot_pirouette() {
        let (track, mut car, t) = fixture();
        let yaw = car.yaw;
        drive(&mut car, &track, &t, DriveCommand { steer: 1.0, ..DriveCommand::IDLE }, 60);
        assert!(
            shortest_angle(car.yaw - yaw).abs() < 0.05,
            "a parked car does not spin: {}",
            car.yaw - yaw
        );
    }

    /// The mechanism the whole model rests on: turning the chassis must leave
    /// the velocity where it was, so the two disagree. Without this there is no
    /// such thing as a slide — the car simply pivots, and every "drift" you see
    /// is something else (a barrier bounce) wearing its name.
    #[test]
    fn turning_the_chassis_leaves_the_velocity_behind() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 150);
        let before = car.velocity();
        let speed_before = car.speed();
        assert_eq!(car.lateral_speed, 0.0, "straight-line driving has no slide");

        // One step of hard lock, with grip and drag removed from the picture by
        // looking at the frame conversion alone.
        let mut turned = car;
        turned.steer = 1.0;
        rotate_chassis(&mut turned, DriveCommand::turning(1.0), &t, &CollisionTuning::DEFAULT, 0.0, 0.0);

        assert!(turned.yaw != car.yaw, "the chassis turned");
        assert!(
            turned.lateral_speed.abs() > 0.0,
            "and the velocity did not turn with it"
        );
        // The rotation is a rotation: it moves speed between the two components
        // and creates none.
        assert!(
            (turned.speed() - speed_before).abs() < 1.0e-3,
            "speed is conserved: {speed_before} -> {}",
            turned.speed()
        );
        // And the world velocity is genuinely unchanged.
        let after = turned
            .forward()
            .mul_scalar(turned.forward_speed)
            .add(turned.right().mul_scalar(turned.lateral_speed));
        assert!(
            after.subtract(before).length() < 1.0e-3,
            "the world velocity is untouched: {before:?} -> {after:?}"
        );
    }

    /// And the consequence, at the level the player feels: a hard turn on tarmac
    /// scrubs speed, because the lateral part it creates is then bled away.
    #[test]
    fn cornering_costs_speed() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 240);
        let mut straight = car;
        let mut turning = car;
        for _ in 0..60 {
            once(&mut straight, DriveCommand::FLAT_OUT, &track, &t, false);
            once(&mut turning, DriveCommand::turning(0.8), &track, &t, false);
        }
        assert!(
            turning.speed() < straight.speed(),
            "the corner cost speed: {} vs {}",
            turning.speed(),
            straight.speed()
        );
    }

    #[test]
    fn the_handbrake_breaks_traction_and_starts_a_drift() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 150);

        let mut gripped = car;
        let mut slid = car;
        let turn = DriveCommand::turning(1.0);
        let flick = DriveCommand { handbrake: true, ..turn };
        for _ in 0..40 {
            once(&mut gripped, turn, &track, &t, false);
            once(&mut slid, flick, &track, &t, false);
        }
        assert!(
            slid.lateral_speed.abs() > gripped.lateral_speed.abs() * 2.0,
            "the handbrake slides: {} vs {}",
            slid.lateral_speed,
            gripped.lateral_speed
        );
        assert!(slid.drifting, "and it counts as a drift");
        assert!(!gripped.drifting, "an ordinary turn does not");
    }

    #[test]
    fn a_drift_recovers_and_converges_rather_than_spinning() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 150);
        drive(
            &mut car,
            &track,
            &t,
            DriveCommand { handbrake: true, ..DriveCommand::turning(1.0) },
            45,
        );
        assert!(car.drifting, "the drift is established");

        // Let go of everything: the counter-steer assist should recover it.
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 180);
        assert!(!car.drifting, "the drift ends");
        assert!(
            car.lateral_speed.abs() < t.drift_release,
            "the slide converged to {}",
            car.lateral_speed
        );
        assert!(car.is_finite());
    }

    #[test]
    fn drift_state_has_hysteresis_and_does_not_chatter() {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        let t = VehicleTuning::DEFAULT;
        // Between the two thresholds: not drifting yet.
        car.lateral_speed = (t.drift_threshold + t.drift_release) * 0.5;
        update_drift(&mut car, &t);
        assert!(!car.drifting);
        // Past the entry threshold: drifting.
        car.lateral_speed = t.drift_threshold + 1.0;
        update_drift(&mut car, &t);
        assert!(car.drifting);
        assert_eq!(car.drift_steps, 1);
        // Back between the thresholds: still drifting.
        car.lateral_speed = (t.drift_threshold + t.drift_release) * 0.5;
        update_drift(&mut car, &t);
        assert!(car.drifting);
        assert_eq!(car.drift_steps, 2);
        // Below the release threshold: over.
        car.lateral_speed = t.drift_release - 0.1;
        update_drift(&mut car, &t);
        assert!(!car.drifting);
        assert_eq!(car.drift_steps, 0);
    }

    #[test]
    fn boost_accelerates_harder_and_raises_the_ceiling() {
        let (track, car, t) = fixture();
        // Straight-line pull, off the line.
        let launch = |boost: bool| {
            let mut c = car;
            for _ in 0..120 {
                once(
                    &mut c,
                    DriveCommand { boost, ..DriveCommand::FLAT_OUT },
                    &track,
                    &t,
                    boost,
                );
            }
            c.forward_speed
        };
        assert!(launch(true) > launch(false) + 2.0, "boost pulls harder");

        // And on the racing line it carries the car past the natural top speed.
        let mut c = car;
        let mut best = 0.0f32;
        for _ in 0..6_000 {
            let command = DriveCommand {
                boost: true,
                ..crate::script::autopilot(&c, &track)
            };
            once(&mut c, command, &track, &t, true);
            best = best.max(c.forward_speed);
        }
        assert!(
            best > t.top_speed,
            "boost exceeds the natural top speed: {best} vs {}",
            t.top_speed
        );
    }

    /// Boost presses the throttle itself.
    #[test]
    fn boost_drives_the_car_without_the_throttle_held() {
        let (track, car, t) = fixture();
        let mut coasting = car;
        let mut boosting = car;
        let boost_only = DriveCommand { boost: true, ..DriveCommand::IDLE };
        for _ in 0..120 {
            once(&mut coasting, DriveCommand::IDLE, &track, &t, false);
            once(&mut boosting, boost_only, &track, &t, true);
        }
        assert!(
            boosting.forward_speed > 30.0,
            "boost alone launched the car: {} m/s",
            boosting.forward_speed
        );
        assert!(coasting.forward_speed < 1.0, "and nothing else did");
    }

    /// And it is a violent shove, not a nudge.
    #[test]
    fn boost_accelerates_far_harder_than_the_throttle_alone() {
        let (track, car, t) = fixture();
        let reach = |boost: bool, steps: u32| {
            let mut c = car;
            for _ in 0..steps {
                once(
                    &mut c,
                    DriveCommand { boost, ..DriveCommand::FLAT_OUT },
                    &track,
                    &t,
                    boost,
                );
            }
            c.forward_speed
        };
        let plain = reach(false, 45);
        let boosted = reach(true, 45);
        assert!(
            boosted > plain * 2.0,
            "three quarters of a second: {plain} m/s on the throttle, {boosted} m/s on boost"
        );
    }

    /// Boost ignores the dirt entirely — the whole point of the power-up.
    #[test]
    fn boost_ignores_the_off_road_penalty() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 240);

        // Put the car in the dirt without disturbing its velocity.
        let sample = track.sample_at(car.distance);
        car.position = sample
            .at_lateral(sample.half_width + track.shoulder() + 2.0)
            .add(Vec3::new(0.0, car.position.y - sample.position.y, 0.0));
        let (d, l) = track.localise(car.position, car.distance, LOCALISE_WINDOW);
        car.distance = d;
        car.lateral = l;
        classify_surface(&mut car, &track);
        assert_eq!(car.surface, Surface::OffRoad);

        let mut struggling = car;
        let mut boosting = car;
        for _ in 0..90 {
            once(&mut struggling, DriveCommand::FLAT_OUT, &track, &t, false);
            once(
                &mut boosting,
                DriveCommand { boost: true, ..DriveCommand::FLAT_OUT },
                &track,
                &t,
                true,
            );
        }
        assert!(
            boosting.forward_speed > struggling.forward_speed + 15.0,
            "boost shrugs off the dirt: {} vs {} m/s",
            boosting.forward_speed,
            struggling.forward_speed
        );
        assert_eq!(
            boosting.surface,
            Surface::OffRoad,
            "the car still KNOWS it is off road — only the penalty is suspended"
        );
    }

    #[test]
    fn the_handling_surface_is_the_real_one_unless_boosting() {
        let (_, mut car, _) = fixture();
        car.surface = Surface::OffRoad;
        car.boosting = false;
        assert_eq!(handling_surface(&car), Surface::OffRoad);
        car.boosting = true;
        assert_eq!(handling_surface(&car), Surface::Tarmac);
    }

    #[test]
    fn boost_does_nothing_when_the_meter_refuses() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 60);
        let mut refused = car;
        for _ in 0..120 {
            once(
                &mut refused,
                DriveCommand { boost: true, ..DriveCommand::FLAT_OUT },
                &track,
                &t,
                false,
            );
        }
        assert!(!refused.boosting, "an empty meter does not boost");
    }

    #[test]
    fn off_road_costs_speed() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 240);
        let entry = car.forward_speed;

        let sample = track.sample_at(car.distance);
        let mut dirt = car;
        // Displace the car onto the dirt without changing its velocity.
        dirt.position = sample
            .at_lateral(sample.half_width + track.shoulder() + 2.0)
            .add(Vec3::new(0.0, dirt.position.y - sample.position.y, 0.0));
        let (d, l) = track.localise(dirt.position, dirt.distance, LOCALISE_WINDOW);
        dirt.distance = d;
        dirt.lateral = l;
        classify_surface(&mut dirt, &track);
        assert_eq!(dirt.surface, Surface::OffRoad, "the car is on the dirt");

        let mut tarmac = car;
        for _ in 0..60 {
            once(&mut dirt, DriveCommand::FLAT_OUT, &track, &t, false);
            once(&mut tarmac, DriveCommand::FLAT_OUT, &track, &t, false);
        }
        assert!(
            dirt.forward_speed < tarmac.forward_speed - 2.0,
            "the dirt is slower: {} vs {} (entry {entry})",
            dirt.forward_speed,
            tarmac.forward_speed
        );
    }

    #[test]
    fn placing_the_car_on_the_track_faces_it_down_the_road() {
        let (track, mut car, _) = fixture();
        let sample = track.sample_at(2_500.0);
        place_on_track(&mut car, &sample, 2.0);
        assert!((car.distance - sample.distance).abs() < 1.0e-3);
        assert_eq!(car.lateral, 2.0);
        assert_eq!(car.forward_speed, 0.0);
        let facing = car.forward();
        assert!(
            facing.dot(sample.flat_forward()) > 0.999,
            "the car points down the road"
        );
    }

    #[test]
    fn the_car_follows_the_road_uphill_and_downhill() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 1_800);
        // Wherever it ends up, it is sitting on (not through, not far above) the
        // road surface.
        let sample = track.interpolated_at(car.distance);
        let surface = road_height(&sample, car.lateral);
        assert!(
            (car.position.y - surface).abs() < 3.0,
            "the car is on the road: car {} vs road {}",
            car.position.y,
            surface
        );
        assert!(car.is_finite());
    }

    #[test]
    fn wheel_spin_tracks_distance_travelled_and_stays_bounded() {
        let (track, mut car, t) = fixture();
        drive(&mut car, &track, &t, DriveCommand::FLAT_OUT, 600);
        assert!(car.wheel_spin >= 0.0 && car.wheel_spin < std::f32::consts::TAU);
        let before = car.wheel_spin;
        once(&mut car, DriveCommand::FLAT_OUT, &track, &t, false);
        assert_ne!(car.wheel_spin, before, "the wheels keep turning");
    }

    #[test]
    fn the_step_report_describes_what_happened() {
        let (track, mut car, t) = fixture();
        let report = once(&mut car, DriveCommand::FLAT_OUT, &track, &t, false);
        assert!(report.forward_accel > 0.0, "throttle is acceleration");
        assert!(report.distance_delta >= 0.0);
        assert!(!report.drift_started);
    }

    #[test]
    fn a_non_finite_command_cannot_poison_the_car() {
        let (track, mut car, t) = fixture();
        let poison = DriveCommand {
            throttle: f32::NAN,
            steer: f32::INFINITY,
            brake: f32::NAN,
            ..DriveCommand::IDLE
        };
        for _ in 0..120 {
            once(&mut car, poison, &track, &t, true);
        }
        assert!(car.is_finite(), "the sanitiser held: {car:?}");
    }

    #[test]
    fn the_impact_state_ages_out() {
        let (track, mut car, t) = fixture();
        car.impact_steps = 3;
        car.impact_strength = 1.0;
        for _ in 0..2 {
            once(&mut car, DriveCommand::IDLE, &track, &t, false);
        }
        assert!(car.impact_steps > 0 && car.impact_strength > 0.0);
        once(&mut car, DriveCommand::IDLE, &track, &t, false);
        assert_eq!(car.impact_steps, 0);
        assert_eq!(car.impact_strength, 0.0);
    }
}
