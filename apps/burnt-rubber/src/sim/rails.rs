//! The lane-locked lateral model — the phone game's car, on rails.
//!
//! In the wheel game the car's position across the road is *emergent*: you
//! rotate the chassis, the velocity keeps pointing where it was, the difference
//! is a slide, and where you end up is the consequence. That model is the whole
//! point of the desktop game and it is unplayable with a thumb.
//!
//! Here the relationship is inverted. Lateral position is *driven*: you name a
//! lane, and the car goes to it. The skill moves from "hold a line" to "pick the
//! right gap, at speed" — which is a game a single button can express.
//!
//! # What this deliberately does NOT do
//!
//! It does not teleport the car, and it does not move `position` directly. It
//! sets [`CarState::lateral_speed`] — the same channel the wheel game's grip
//! model writes — and lets the ordinary integrator carry the car there. That is
//! the entire reason this module is small: barrier collisions, traffic
//! collisions, the surface classifier, distance accumulation, the boost economy
//! and the near-miss detector are all downstream of the integrator and none of
//! them need to know the car is on rails. A solver that assigned `position`
//! would have had to reimplement every one of them.
//!
//! The lane vocabulary is the track's own ([`Track::lane_count`],
//! [`Track::lane_lateral`]) — the same lanes the traffic already drives in, so
//! "the gap in the middle lane" means the same thing to the player and to the
//! traffic scheduler.

use crate::command::DriveCommand;
use crate::sim::car::CarState;
use crate::track::Track;
use crate::tuning::DT;

/// How quickly the car crosses to a newly chosen lane (m/s of lateral travel).
/// A lane is ~3.5 m, so this is a little under a third of a second per lane —
/// fast enough to dodge, slow enough that the move is legible and committing.
const LANE_CROSS_SPEED: f32 = 12.0;

/// Lateral acceleration (m/s²) toward the crossing speed. Finite so a lane
/// change eases in rather than snapping the car sideways on the first frame.
const LANE_ACCEL: f32 = 90.0;

/// Proportional gain on the remaining lateral error. Above this distance the car
/// crosses at the full [`LANE_CROSS_SPEED`]; inside it, it eases to a stop on the
/// lane centre instead of oscillating around it.
const LANE_SETTLE: f32 = 6.0;

/// Peak visual lean (radians) at full crossing speed. The chassis is not
/// steering — it is on rails — but a car that changes lane with its nose rigidly
/// straight reads as a sprite sliding sideways, so the nose is turned into the
/// move by this much and no more.
const LANE_LEAN: f32 = 0.16;

/// Which lane the phone game's car is heading for.
///
/// Just the target index: everything else — where that lane *is* — is asked of
/// the [`Track`] each step, so nothing here can go stale. The index is **signed
/// and centre-anchored** ([`Track::lane_lateral`]): `0` is the centreline lane,
/// which exists everywhere on the course, so the default is a car in the middle
/// of the road rather than a car in "lane zero of however many there are here".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RailsState {
    lane: i32,
}

impl RailsState {
    /// A car starting in `lane`, numbered out from the centreline.
    pub const fn in_lane(lane: i32) -> RailsState {
        RailsState { lane }
    }

    /// The lane currently being driven toward.
    pub const fn lane(self) -> i32 {
        self.lane
    }
}

/// Apply one step of the lane model, in place of the wheel game's
/// steer/rotate/grip trio.
///
/// Consumes [`DriveCommand::lane_step`] (already clamped to `-1..=1` and
/// edge-triggered by the caller) and leaves the car with a lateral velocity that
/// carries it toward the chosen lane, a heading that follows the road, and a
/// visual lean proportional to how fast it is crossing.
pub fn guide(car: &mut CarState, command: DriveCommand, track: &Track, state: &mut RailsState) {
    let sample = track.sample_at(car.distance);
    let reach = track.lane_reach(&sample);

    // Retarget.
    //
    // `lane_step` is a SCREEN direction (`+1` = the player pressed the right
    // button and expects the car to go right), and the lane index runs the other
    // way, so it is negated here. Two facts make that so, and neither is
    // guessable from this file alone:
    //
    //   * `Track::lane_lateral` puts lane `n` at `n * lane_width`, so a rising
    //     lane index moves toward `+lateral`.
    //   * `+lateral` is the *simulation's* right, and this engine renders world
    //     `+X` to SCREEN-LEFT. That is why `rotate_chassis` negates the steering
    //     input into a yaw rate (`-(car.steer * ...)`) — the wheel game makes
    //     exactly this correction, one line, for exactly this reason.
    //
    // Skipping it is not subtly wrong, it is precisely backwards: the left
    // button moves the car right. Doing it here rather than in the input layer
    // keeps `lane_step` meaning "the direction the player pointed", which is the
    // only meaning a button can honestly have.
    //
    // The clamp is against *this* sample's reach, so a road that drops its outer
    // lane pulls the target in rather than aiming it off the edge — and a hop
    // into a wall is simply refused at the outermost lane. Because the numbering
    // is centre-anchored, that is now the ONLY way a held lane can move: a road
    // that merely widens appends lanes further out and leaves this one alone.
    let requested = state.lane - command.lane_step as i32;
    state.lane = requested.clamp(-reach, reach);

    let target = track.lane_lateral(&sample, state.lane);
    let error = target - car.lateral;

    // Cross at a fixed speed while far away, easing to zero over the last few
    // metres. A pure proportional term would crawl the last stretch forever; a
    // pure fixed speed would buzz around the centre.
    let desired = (error * (LANE_CROSS_SPEED / LANE_SETTLE))
        .clamp(-LANE_CROSS_SPEED, LANE_CROSS_SPEED);
    let step = LANE_ACCEL * DT;
    car.lateral_speed += (desired - car.lateral_speed).clamp(-step, step);

    // The nose follows the road, turned into the lane change by the lean.
    //
    // `+lateral` is the driver's right and `right()` is `(cos yaw, 0, -sin yaw)`
    // against a `forward()` of `(sin yaw, 0, cos yaw)`, so a *larger* yaw turns
    // the nose toward `+right`. Moving right therefore adds lean, and the car
    // banks into the direction it is actually travelling.
    let lean = (car.lateral_speed / LANE_CROSS_SPEED).clamp(-1.0, 1.0) * LANE_LEAN;
    // The road's heading AS A CAR YAW — derived exactly the way
    // `controller::place_on_track` derives it, and deliberately not from
    // `TrackSample::heading`. Those are two different conventions: `heading` is
    // the generator's own angle, while a chassis yaw is measured from the
    // flattened tangent as `atan2(x, z)`. Using the former here pointed the car
    // across the road instead of along it, which read as the chassis being
    // rotated and the chase camera swinging out beside it.
    let road = sample.flat_forward();
    let heading = road.x.atan2(road.z) + lean;
    // The rate is the ROAD's, not the nose's.
    //
    // Differencing successive yaws looks right and is wrong, because `heading`
    // carries the cosmetic lean: a lane change swings the nose by `LANE_LEAN` in
    // a step or two, which differences out to ~10 rad/s and reads to the chase
    // camera as a violent corner. The camera then swings out beside a car that
    // is travelling in a straight line. What the car is *actually* rotating at
    // is the rate the road turns under it — curvature times forward speed —
    // which is zero on a straight however hard the player is dodging.
    car.yaw_rate = sample.curvature * car.forward_speed;
    car.yaw = heading.rem_euclid(std::f32::consts::TAU);

    // The front wheels are drawn from `steer`, so give them the lean too —
    // normalised, because `steer` is a `-1..1` input channel and not an angle.
    car.steer = (lean / LANE_LEAN).clamp(-1.0, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{shortest_angle, Track};
    

    fn track() -> Track {
        Track::fixture(7)
    }

    /// A car parked at distance 40 m, sitting in `lane`'s centre. The lane must
    /// match the `RailsState` the test starts from, or the car is already
    /// crossing before the test presses anything.
    fn car_on(track: &Track, lane: i32) -> CarState {
        let sample = track.sample_at(40.0);
        let mut car = CarState::parked(sample.position, road_yaw(track, 40.0));
        car.distance = 40.0;
        car.lateral = track.lane_lateral(&sample, lane);
        car
    }

    /// The road's heading expressed as a chassis yaw — the same derivation the
    /// solver and `place_on_track` use.
    fn road_yaw(track: &Track, distance: f32) -> f32 {
        let f = track.sample_at(distance).flat_forward();
        f.x.atan2(f.z)
    }

    /// A press of the on-screen lane button. `+1` is the RIGHT button — a
    /// screen direction, which the solver turns into a lane index the other way
    /// round. Every assertion below is written in the player's terms.
    fn hop(step: i8) -> DriveCommand {
        DriveCommand {
            lane_step: step,
            ..DriveCommand::IDLE
        }
    }

    /// The lane whose centre is furthest SCREEN-RIGHT is the most negative
    /// lateral, i.e. the lowest (most negative) index — `-reach`.

    #[test]
    fn a_hop_retargets_one_lane_and_only_one() {
        let track = track();
        let mut car = car_on(&track, 1);
        let mut state = RailsState::in_lane(1);
        // RIGHT button -> a lane nearer screen-right -> a LOWER index.
        guide(&mut car, hop(1), &track, &mut state);
        assert_eq!(state.lane(), 0, "and lane 0 is the centreline lane");
        // Holding the button is not a second hop: the caller edge-triggers, so a
        // held finger arrives here as `lane_step: 0`.
        guide(&mut car, hop(0), &track, &mut state);
        assert_eq!(state.lane(), 0);
    }

    #[test]
    fn a_hop_off_the_road_is_refused_rather_than_clamped_late() {
        let track = track();
        let mut car = car_on(&track, 0);
        let sample = track.sample_at(car.distance);
        let reach = track.lane_reach(&sample);
        let mut state = RailsState::in_lane(-reach);
        guide(&mut car, hop(1), &track, &mut state);
        assert_eq!(
            state.lane(),
            -reach,
            "cannot hop right out of the rightmost lane"
        );

        let mut outer = RailsState::in_lane(reach);
        guide(&mut car, hop(-1), &track, &mut outer);
        assert_eq!(outer.lane(), reach, "cannot hop left out of the leftmost lane");
    }

    #[test]
    fn the_car_accelerates_toward_the_chosen_lane_and_settles_on_it() {
        let track = track();
        let mut car = car_on(&track, 1);
        let mut state = RailsState::in_lane(1);
        // Aim one lane right and integrate the lateral channel by hand (the real
        // integrator is the controller's; here we only prove the solver drives
        // the car to the lane centre and stops there).
        guide(&mut car, hop(1), &track, &mut state);
        assert!(
            car.lateral_speed < 0.0,
            "the RIGHT button moves the car toward screen-right, which is              DECREASING lateral — the engine renders world +X to screen-left"
        );
        let target = track.lane_lateral(&track.sample_at(car.distance), state.lane());
        (0..240).for_each(|_| {
            guide(&mut car, hop(0), &track, &mut state);
            car.lateral += car.lateral_speed * DT;
        });
        assert!(
            (car.lateral - target).abs() < 0.05,
            "settled on the lane centre: lateral {} vs target {target}",
            car.lateral
        );
        assert!(
            car.lateral_speed.abs() < 0.5,
            "and stopped there rather than oscillating: {}",
            car.lateral_speed
        );
    }

    #[test]
    fn the_nose_leans_into_the_move_and_returns_to_the_road_heading() {
        let track = track();
        let mut car = car_on(&track, 2);
        let mut state = RailsState::in_lane(2);
        let heading = road_yaw(&track, car.distance);
        guide(&mut car, hop(1), &track, &mut state);
        (0..6).for_each(|_| {
            guide(&mut car, hop(0), &track, &mut state);
            car.lateral += car.lateral_speed * DT;
        });
        let leaning = car.yaw;
        // Compared as a shortest arc: `yaw` is stored wrapped into `[0, TAU)`,
        // so a small negative lean reads as ~6.2 against a heading of 0.
        assert!(
            shortest_angle(leaning - heading) < 0.0,
            "crossing toward screen-right lowers the yaw, the same way              `rotate_chassis` negates a right steering input: {leaning} vs {heading}"
        );
        assert!(car.steer < 0.0, "and the front wheels turn with it");
        // Once settled the nose is back on the road's own heading.
        (0..240).for_each(|_| {
            guide(&mut car, hop(0), &track, &mut state);
            car.lateral += car.lateral_speed * DT;
        });
        assert!(
            shortest_angle(car.yaw - heading).abs() < 0.01,
            "settled back to the road heading: {} vs {heading}",
            car.yaw
        );
    }

    #[test]
    fn a_heading_across_the_angle_wrap_does_not_explode_the_yaw_rate() {
        // The bug this guards: `yaw` is stored in `[0, TAU)` and a track heading
        // is in `(-PI, PI]`, so a car pointing a hair past zero sitting on a
        // road heading a hair below it used to read as ~TAU of rotation in one
        // fixed step. Nothing on rails integrates yaw_rate, but the chase camera
        // reads it, and the symptom was the camera whipping around a car that
        // had not turned.
        let track = track();
        let mut car = car_on(&track, 0);
        let mut state = RailsState::in_lane(0);
        // Put the stored yaw on the far side of the wrap from the road heading.
        let heading = road_yaw(&track, car.distance);
        car.yaw = (heading - 0.01).rem_euclid(std::f32::consts::TAU);
        guide(&mut car, hop(0), &track, &mut state);
        assert!(
            car.yaw_rate.abs() < 10.0,
            "a hair of heading error is not a spin: {}",
            car.yaw_rate
        );
        // And a lane change on a straight produces no yaw rate at all: the nose
        // leans, but the car is not cornering, and the chase camera must not be
        // told that it is.
        let mut straight = car_on(&track, 2);
        let mut lane = RailsState::in_lane(2);
        guide(&mut straight, hop(1), &track, &mut lane);
        (0..8).for_each(|_| {
            guide(&mut straight, hop(0), &track, &mut lane);
            straight.lateral += straight.lateral_speed * DT;
        });
        assert!(straight.lateral_speed.abs() > 0.5, "it is changing lane");
        assert!(
            straight.yaw_rate.abs() < 1.0e-3,
            "a stationary car dodging on a straight is not cornering: {}",
            straight.yaw_rate
        );
    }

    #[test]
    fn the_lean_never_exceeds_its_cap() {
        let track = track();
        let mut car = car_on(&track, 0);
        let mut state = RailsState::in_lane(0);
        // Slam the lateral speed far past the crossing speed and confirm the
        // lean saturates rather than spinning the chassis.
        car.lateral_speed = LANE_CROSS_SPEED * 20.0;
        guide(&mut car, hop(0), &track, &mut state);
        assert!(car.steer <= 1.0 && car.steer >= -1.0);
        let heading = road_yaw(&track, car.distance);
        assert!(shortest_angle(car.yaw - heading).abs() <= LANE_LEAN + 1.0e-4);
    }
}
