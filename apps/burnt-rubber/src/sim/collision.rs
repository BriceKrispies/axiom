//! Racing collision response: barriers and traffic.
//!
//! Both cases are resolved the same way — **positionally, then in velocity** —
//! and both are expressed entirely in bounded velocities, never in accumulated
//! forces. A contact removes the component of motion heading into the obstacle,
//! reflects a fraction of it, and scrubs some forward speed. There is no
//! penetration spring to tune, no impulse to integrate, and therefore no way for
//! one bad frame to hand the next frame a number that grows.
//!
//! The design goal is specific and is asserted by the tests below: a collision
//! must **hurt momentum without stopping the demo**. Losing a third of your
//! speed and being pushed straight is a mistake you drive out of in a second;
//! being pinned against a wall or spun to a standstill is a mistake you restart
//! from, and a demo nobody finishes.

use axiom_math::Vec3;

use crate::track::Track;
use crate::tuning::{RaceTuning, VehicleTuning};

use super::car::CarState;

/// Impact strength below which a contact is a graze: the car is still nudged
/// straight, but no camera kick, sound or HUD flash fires. Without this, running
/// a wall produces a strobe of "impacts" every step.
pub const GRAZE_THRESHOLD: f32 = 0.06;

/// What a resolved contact did, for the camera, the audio and the HUD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Impact {
    /// World direction the car was shoved, unit.
    pub direction: Vec3,
    /// `0..1`, where `1` is a head-on hit at the boosted top speed.
    pub strength: f32,
    /// Whether the obstacle was traffic (rather than a barrier).
    pub traffic: bool,
}

/// Push the car back inside the barriers if it has crossed them, and resolve the
/// contact. Returns the impact if one was worth reporting.
///
/// Called from inside the position integration's sub-moves, so the car is
/// checked against the barrier several times per fixed step and can never end a
/// step outside the road no matter how fast it is going.
pub fn resolve_barrier(car: &mut CarState, track: &Track, tuning: &VehicleTuning) -> Option<Impact> {
    let sample = track.sample_at(car.distance);
    let limit = (track.barrier_offset(&sample) - tuning.half_width).max(1.0);
    let overshoot = car.lateral.abs() - limit;
    if overshoot <= 0.0 {
        return None;
    }
    let side = if car.lateral >= 0.0 { 1.0 } else { -1.0 };

    // Positional correction along the *track's* lateral axis, which preserves
    // the car's progress along the course exactly.
    car.position = car
        .position
        .subtract(sample.right.mul_scalar(side * overshoot));
    car.lateral = side * limit;

    // Scrape alignment: pressing on a barrier turns the car to run along it.
    //
    // This is not decoration, it is what makes a wall *recoverable*. The
    // chassis has no yaw authority of its own at a standstill and the contact
    // response only touches velocity, so a car that has nosed into a barrier
    // would otherwise sit there grinding its speed away with nothing in the
    // model ever pointing it back down the road. A real arcade racer scrapes you
    // straight; so does this — but only while the nose is actually pointing
    // *into* the wall.
    //
    // Alignment exists to rescue a car that has buried its nose in a barrier. A
    // car that has already turned back toward the road is recovering under its
    // own steering, and dragging it parallel again fights the driver. Left
    // unconditional, that tug-of-war has a stable fixed point: the driver's yaw
    // and the alignment's pull cancel exactly, and the car grinds along the wall
    // forever at a constant offset — the precise failure this code was written
    // to prevent. Whether a given car escaped then came down to whether its yaw
    // authority happened to land on the lucky side of that balance, which is not
    // a property anything should depend on.
    //
    // Fading the pull out with the nose's angle into the wall removes the fixed
    // point entirely: a car aimed at the barrier is straightened hard, a car
    // running parallel is barely touched, and a car aimed back at the road is
    // left alone.
    let outward = sample.right.mul_scalar(side);
    let nosed_in = car.forward().dot(outward).clamp(0.0, 1.0);
    let along = sample.flat_forward();
    let wall_yaw = along.x.atan2(along.z);
    let pull = (tuning.barrier_align * crate::tuning::DT * nosed_in).clamp(0.0, 1.0);
    car.yaw += crate::track::shortest_angle(wall_yaw - car.yaw) * pull;

    // The wall's inward normal.
    let normal = sample.right.mul_scalar(-side);
    resolve_against(
        car,
        normal,
        tuning.barrier_restitution,
        tuning.barrier_speed_keep,
        tuning,
        false,
    )
}

/// Resolve a contact with a traffic car occupying `(distance, lateral)` with the
/// given half extents. Returns the impact if the boxes actually overlap.
///
/// The overlap test is done in track space — along-course distance against
/// lateral offset — rather than in world space. On a road that is at most gently
/// curved over a car's length, the two agree; and doing it in track space means
/// the test is two scalar comparisons instead of an oriented-box intersection,
/// which is what keeps twenty-eight traffic cars free.
#[allow(clippy::too_many_arguments)]
pub fn resolve_traffic(
    car: &mut CarState,
    track: &Track,
    traffic_distance: f32,
    traffic_lateral: f32,
    traffic_speed: f32,
    race: &RaceTuning,
    tuning: &VehicleTuning,
) -> Option<Impact> {
    let along = (car.distance - traffic_distance).abs();
    let across = (car.lateral - traffic_lateral).abs();
    let along_limit = tuning.half_length + race.traffic_half_length;
    let across_limit = tuning.half_width + race.traffic_half_width;
    if along >= along_limit || across >= across_limit {
        return None;
    }

    let sample = track.sample_at(car.distance);
    // Push out along whichever axis is least penetrated — a nose-to-tail shunt
    // resolves along the road, a side-swipe resolves across it.
    let along_penetration = along_limit - along;
    let across_penetration = across_limit - across;
    let side = if car.lateral >= traffic_lateral { 1.0 } else { -1.0 };

    if across_penetration <= along_penetration {
        // A side-swipe. Unlike a wall, this always shoves — you are being pushed
        // out of a space something else is occupying, whether or not you were
        // still closing on it — so the deflection is applied unconditionally and
        // *after* any closing velocity is removed, rather than through the
        // barrier path (which would cancel its own shove).
        car.position = car
            .position
            .add(sample.right.mul_scalar(side * across_penetration));
        car.lateral += side * across_penetration;

        let away = sample.right.mul_scalar(side);
        let planar = car
            .forward()
            .mul_scalar(car.forward_speed)
            .add(car.right().mul_scalar(car.lateral_speed));
        // Whatever motion was heading into the traffic car is cancelled.
        let closing = (-planar.dot(away)).max(0.0);
        let separated = planar.add(away.mul_scalar(closing));
        let forward = car.forward();
        let right = car.right();
        car.forward_speed = separated.dot(forward);
        car.lateral_speed = separated.dot(right);
        // Then the shove, which is what makes contact read as contact.
        car.lateral_speed += side * tuning.traffic_deflect;

        // A graze costs far less speed than a rear-ender: the strength comes
        // from the closing rate plus a share of the raw speed difference, so
        // brushing a car at 300 km/h still registers as *something*.
        let strength = impact_strength(
            closing.max((car.forward_speed - traffic_speed) * SIDE_SWIPE_SEVERITY),
            tuning,
        );
        // Note `strength` ALREADY carries the side-swipe discount, so the scrub
        // scales by it once and not twice. Applying the severity again here
        // squares it (0.35 becomes 0.12), and a graze at a 60 m/s closing speed
        // ends up costing two percent — contact the player sees and does not
        // feel.
        // A boosting car ploughs through. It is still shoved sideways and the
        // hit is still reported — you feel it, it just costs no speed.
        let keep = (!car.boosting)
            .then(|| 1.0 - (1.0 - tuning.traffic_speed_keep) * strength)
            .unwrap_or(1.0);
        car.forward_speed *= keep;
        register(car, away, strength, true)
    } else {
        // A rear-ender: the player cannot end up slower than the car in front,
        // which is what stops a shunt from becoming a full stop.
        let forward = sample.flat_forward();
        let push = if car.distance >= traffic_distance { 1.0 } else { -1.0 };
        car.position = car
            .position
            .add(forward.mul_scalar(push * along_penetration));
        // Clamped to the course: `distance` is a course coordinate, and every
        // other writer of it goes through `Track::localise`, which clamps. A
        // shunt right on the finish line would otherwise nudge it past the end.
        car.distance = (car.distance + push * along_penetration).clamp(0.0, track.length());
        let strength = impact_strength(car.forward_speed - traffic_speed, tuning);
        // Boosting, the shunt costs nothing: the car goes through rather than
        // into. Everything else about the contact still happens.
        car.forward_speed = (!car.boosting)
            .then(|| (car.forward_speed * tuning.traffic_speed_keep).max(traffic_speed * 0.6))
            .unwrap_or(car.forward_speed);
        // A shunt kicks the nose slightly off line — enough to feel, not enough
        // to spin.
        car.lateral_speed += side * tuning.traffic_deflect * 0.5;
        register(car, forward.mul_scalar(-1.0), strength, true)
    }
}

/// The shared velocity half of both responses.
fn resolve_against(
    car: &mut CarState,
    normal: Vec3,
    restitution: f32,
    speed_keep: f32,
    tuning: &VehicleTuning,
    traffic: bool,
) -> Option<Impact> {
    let planar = car
        .forward()
        .mul_scalar(car.forward_speed)
        .add(car.right().mul_scalar(car.lateral_speed));
    let into_wall = planar.dot(normal);
    // Already travelling away from the obstacle: the positional correction was
    // enough, and reflecting again would fire the car back across the road.
    if into_wall >= 0.0 {
        return None;
    }
    let strength = impact_strength(into_wall, tuning);
    // Remove the closing component and give back a fraction of it.
    let reflected = planar.subtract(normal.mul_scalar(into_wall * (1.0 + restitution)));
    // Scrubbing scales with how square the hit was: a graze along a wall barely
    // slows you, a head-on hit costs a lot.
    let keep = 1.0 - (1.0 - speed_keep) * strength.clamp(0.0, 1.0);
    let scrubbed = reflected.mul_scalar(keep);

    let forward = car.forward();
    let right = car.right();
    car.forward_speed = scrubbed.dot(forward);
    car.lateral_speed = scrubbed.dot(right);

    register(car, normal.mul_scalar(-1.0), strength, traffic)
}

/// How hard a closing speed counts as, `0..1`.
fn impact_strength(closing_speed: f32, tuning: &VehicleTuning) -> f32 {
    let ceiling = tuning.top_speed + tuning.boost_top_speed_bonus;
    (closing_speed.abs() / ceiling.max(1.0)).clamp(0.0, 1.0)
}

/// Record the impact on the car and hand it back, unless it was only a graze.
fn register(car: &mut CarState, direction: Vec3, strength: f32, traffic: bool) -> Option<Impact> {
    if strength < GRAZE_THRESHOLD {
        return None;
    }
    let direction = direction.normalize().unwrap_or(Vec3::UNIT_Z);
    // A bigger hit overrides a smaller one still ringing; a smaller one does not
    // cut a bigger one short.
    if strength >= car.impact_strength {
        car.impact_direction = direction;
        car.impact_strength = strength;
    }
    car.impact_steps = car
        .impact_steps
        .max((strength * IMPACT_STEP_SCALE) as u32 + IMPACT_STEP_FLOOR);
    Some(Impact {
        direction,
        strength,
        traffic,
    })
}

/// Steps of impact state per unit strength.
const IMPACT_STEP_SCALE: f32 = 34.0;
/// Minimum steps any registered impact lasts.
const IMPACT_STEP_FLOOR: u32 = 8;

/// Whether a traffic car at `(distance, lateral)` is close enough to the player,
/// and being passed fast enough, to count as a near miss.
///
/// Near misses are decided from **relative geometry and velocity**, never a
/// timer: the player must actually be alongside the traffic car, within
/// `near_miss_gap` laterally, closing at `near_miss_closing_speed` or more, and
/// not touching it. That is what makes the reward feel earned — you get boost
/// for threading a gap, not for existing near a car.
pub fn is_near_miss(
    car: &CarState,
    traffic_distance: f32,
    traffic_lateral: f32,
    traffic_speed: f32,
    race: &RaceTuning,
    tuning: &VehicleTuning,
) -> bool {
    let along = (car.distance - traffic_distance).abs();
    let across = (car.lateral - traffic_lateral).abs();
    let alongside = along < tuning.half_length + race.traffic_half_length + NEAR_MISS_ALONG;
    let touching = across < tuning.half_width + race.traffic_half_width;
    let close = across < race.near_miss_gap;
    let closing = car.forward_speed - traffic_speed >= race.near_miss_closing_speed;
    alongside & close & closing & !touching
}

/// Extra along-course window (m) either side of contact in which a pass counts.
const NEAR_MISS_ALONG: f32 = 2.0;

/// How much of a raw speed difference a side-swipe counts as a hit. Well under
/// one, because brushing a car at a closing 60 m/s is not a 60 m/s crash.
const SIDE_SWIPE_SEVERITY: f32 = 0.35;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;
    use crate::sim::controller::{place_on_track, step, LOCALISE_WINDOW};
    use crate::tuning::CourseTuning;

    fn fixture() -> (Track, CarState, VehicleTuning) {
        let track = Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT);
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        place_on_track(&mut car, &track.sample_at(120.0), 0.0);
        (track, car, VehicleTuning::DEFAULT)
    }

    /// Displace a car sideways without disturbing its velocity, so a test can
    /// place it exactly where it wants a contact to happen.
    fn displace_to(car: &mut CarState, track: &Track, lateral: f32) {
        let sample = track.sample_at(car.distance);
        let lift = car.position.y - sample.position.y;
        car.position = sample.at_lateral(lateral).add(Vec3::new(0.0, lift, 0.0));
        let (d, l) = track.localise(car.position, car.distance, LOCALISE_WINDOW);
        car.distance = d;
        car.lateral = l;
    }

    #[test]
    fn a_car_inside_the_barriers_is_left_alone() {
        let (track, mut car, t) = fixture();
        car.forward_speed = 60.0;
        let before = car;
        assert!(resolve_barrier(&mut car, &track, &t).is_none());
        assert_eq!(car, before);
    }

    #[test]
    fn crossing_the_barrier_puts_the_car_back_on_the_road() {
        let (track, mut car, t) = fixture();
        car.forward_speed = 70.0;
        let sample = track.sample_at(car.distance);
        let limit = track.barrier_offset(&sample);
        displace_to(&mut car, &track, limit + 4.0);
        // Aim it further into the wall.
        car.lateral_speed = 20.0;

        let impact = resolve_barrier(&mut car, &track, &t).expect("that is a collision");
        assert!(impact.strength > 0.0 && impact.strength <= 1.0);
        assert!(!impact.traffic);
        assert!(
            car.lateral.abs() <= limit - t.half_width + 1.0e-3,
            "the car is inside the barrier: {} vs {limit}",
            car.lateral
        );
        assert!(car.is_finite());
    }

    #[test]
    fn a_barrier_impact_costs_speed_without_stopping_the_car() {
        let (track, mut car, t) = fixture();
        car.forward_speed = 80.0;
        let sample = track.sample_at(car.distance);
        displace_to(&mut car, &track, track.barrier_offset(&sample) + 1.0);
        car.lateral_speed = 40.0;
        resolve_barrier(&mut car, &track, &t);
        assert!(car.forward_speed < 80.0, "the hit hurt");
        assert!(
            car.forward_speed > 20.0,
            "but the demo continues: {}",
            car.forward_speed
        );
    }

    /// The failure mode that ruins an arcade racer is being *pinned* against a
    /// wall. Grinding one at full lock for four seconds is allowed to cost most
    /// of your speed — but the moment you straighten up you must be able to
    /// drive away, which is what the scrape alignment guarantees.
    #[test]
    /// Scrape alignment must not fight a car that is steering off the wall.
    ///
    /// Unconditional alignment gave the system a stable fixed point — the pull
    /// and the driver's steering cancelling exactly — in which the car ground
    /// along the barrier at a constant offset forever. Whether a particular car
    /// escaped depended on which side of that balance its yaw authority landed,
    /// which is luck, not design.
    #[test]
    fn barrier_alignment_fades_as_the_nose_turns_away_from_the_wall() {
        let (track, mut car, t) = fixture();
        for _ in 0..240 {
            step(&mut car, DriveCommand::FLAT_OUT, &track, &t, false, None);
        }
        for _ in 0..240 {
            step(&mut car, DriveCommand::turning(1.0), &track, &t, false, None);
        }
        let pinned = car.lateral;
        assert!(pinned.abs() > track.sample_at(car.distance).half_width, "it is on the wall");

        // The car must come off the wall immediately and keep coming off it for
        // as long as it is still out there. Once it is back on the tarmac it is
        // free to move around — that is a racing line, not a trap — so the
        // monotonic claim applies only while it is still off the road.
        let mut previous = pinned.abs();
        for second in 0..3 {
            let was_off_road = previous > track.sample_at(car.distance).half_width;
            for _ in 0..60 {
                let command = crate::script::autopilot(&car, &track);
                step(&mut car, command, &track, &t, false, None);
            }
            let now = car.lateral.abs();
            assert!(
                !was_off_road || now < previous - 0.5,
                "second {second}: still off the road and not escaping — {now} m (was {previous} m)"
            );
            previous = now;
        }
        assert!(
            car.lateral.abs() < track.sample_at(car.distance).half_width,
            "and it finishes on the tarmac, not alongside it: {}",
            car.lateral
        );
    }

    fn the_car_cannot_be_trapped_against_a_barrier() {
        let (track, mut car, t) = fixture();
        for _ in 0..240 {
            step(&mut car, DriveCommand::FLAT_OUT, &track, &t, false, None);
        }
        // Steer hard into the wall and hold it there.
        for _ in 0..240 {
            step(&mut car, DriveCommand::turning(1.0), &track, &t, false, None);
        }
        let sample = track.sample_at(car.distance);
        assert!(
            car.lateral.abs() <= track.barrier_offset(&sample),
            "still inside the barriers"
        );

        // Now drive away from it, as a player would. Within three seconds the
        // car is back on the road and genuinely going again — that is the whole
        // "recoverable" claim, and it is the thing a pinned car cannot do. (The
        // first second of that is spent crossing the dirt verge at the reduced
        // off-road acceleration, which is the intended cost of the mistake.)
        for _ in 0..240 {
            let command = crate::script::autopilot(&car, &track);
            step(&mut car, command, &track, &t, false, None);
        }
        let sample = track.sample_at(car.distance);
        assert!(
            car.lateral.abs() < sample.half_width,
            "it is back on the tarmac: {} of {}",
            car.lateral,
            sample.half_width
        );
        assert!(
            car.forward_speed > 30.0,
            "and going again: {}",
            car.forward_speed
        );
        assert!(car.is_finite());
    }

    /// The scrape alignment, isolated: a car nosed into a barrier is rotated
    /// toward the road's direction, not away from it.
    #[test]
    fn a_barrier_turns_the_car_back_along_itself() {
        let (track, mut car, t) = fixture();
        let sample = track.sample_at(car.distance);
        let road_yaw = sample.heading;
        // Point the car 60 degrees into the wall and push it through.
        car.yaw = road_yaw + 1.05;
        car.forward_speed = 40.0;
        displace_to(&mut car, &track, track.barrier_offset(&sample) + 1.0);
        let before = crate::track::shortest_angle(car.yaw - road_yaw).abs();
        // Each contact aligns a little; the car is re-pressed into the wall
        // between contacts, exactly as holding full lock against it does.
        for _ in 0..30 {
            resolve_barrier(&mut car, &track, &t);
            car.lateral = track.barrier_offset(&sample) + 1.0;
        }
        let after = crate::track::shortest_angle(car.yaw - road_yaw).abs();
        assert!(after < before * 0.5, "the wall straightened it: {before} -> {after}");
    }

    #[test]
    fn repeated_wall_contact_stays_stable_over_a_long_run() {
        let (track, mut car, t) = fixture();
        for i in 0..6_000 {
            let steer = if (i / 40) % 2 == 0 { 1.0 } else { -1.0 };
            step(&mut car, DriveCommand::turning(steer), &track, &t, true, None);
            assert!(car.is_finite(), "step {i} produced {car:?}");
        }
        let sample = track.sample_at(car.distance);
        assert!(car.lateral.abs() <= track.barrier_offset(&sample) + 1.0e-2);
    }

    #[test]
    fn a_car_already_leaving_the_wall_is_not_reflected_again() {
        let (track, mut car, t) = fixture();
        car.forward_speed = 40.0;
        let sample = track.sample_at(car.distance);
        displace_to(&mut car, &track, track.barrier_offset(&sample) + 0.5);
        // Moving back toward the middle of the road already.
        car.lateral_speed = -25.0;
        let lateral_before = car.lateral_speed;
        let impact = resolve_barrier(&mut car, &track, &t);
        assert!(impact.is_none(), "no second reflection");
        assert_eq!(car.lateral_speed, lateral_before);
    }

    #[test]
    fn walled_sections_have_their_barriers_at_the_shoulder() {
        let track = Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT);
        let tunnel = track
            .samples()
            .iter()
            .find(|s| s.section.walled())
            .expect("the course has a tunnel");
        let open = track
            .samples()
            .iter()
            .find(|s| !s.section.walled())
            .expect("and open road");
        assert!(
            (track.barrier_offset(tunnel) - tunnel.half_width - track.shoulder()).abs() < 1.0e-4,
            "a walled section has no verge"
        );
        assert!(
            track.barrier_offset(open) > open.half_width + track.shoulder(),
            "an open one does"
        );
    }

    #[test]
    fn traffic_far_away_is_not_a_collision() {
        let (track, mut car, t) = fixture();
        let r = RaceTuning::DEFAULT;
        let here = car.distance;
        assert!(resolve_traffic(&mut car, &track, here + 50.0, 0.0, 30.0, &r, &t).is_none());
        assert!(resolve_traffic(&mut car, &track, here, 20.0, 30.0, &r, &t).is_none());
    }

    #[test]
    fn rear_ending_traffic_scrubs_speed_but_leaves_the_player_rolling() {
        let (track, mut car, t) = fixture();
        let r = RaceTuning::DEFAULT;
        car.forward_speed = 85.0;
        let ahead = car.distance + 3.0;
        let impact = resolve_traffic(&mut car, &track, ahead, 0.0, 28.0, &r, &t)
            .expect("that is a shunt");
        assert!(impact.traffic);
        assert!(car.forward_speed < 85.0, "the shunt cost speed");
        assert!(
            car.forward_speed >= 28.0 * 0.6,
            "but never below the car in front: {}",
            car.forward_speed
        );
        assert!(car.is_finite());
    }

    /// A graze must cost less than a rear-ender but more than nothing. The
    /// severity discount belongs in the strength, once.
    /// Boosting through traffic costs no speed at all — the power-up's third
    /// cheat. The contact still happens and is still reported.
    #[test]
    fn a_boosting_car_loses_no_speed_to_traffic() {
        let (track, car, t) = fixture();
        let r = RaceTuning::DEFAULT;

        for boosting in [false, true] {
            // Rear-ender.
            let mut shunt = car;
            shunt.forward_speed = 90.0;
            shunt.boosting = boosting;
            let ahead = shunt.distance + 3.0;
            let hit = resolve_traffic(&mut shunt, &track, ahead, 0.0, 28.0, &r, &t);
            assert!(hit.is_some(), "the contact still registers when boosting");
            if boosting {
                assert!(
                    (shunt.forward_speed - 90.0).abs() < 1.0e-3,
                    "boosting through a car cost {} m/s",
                    90.0 - shunt.forward_speed
                );
            } else {
                assert!(shunt.forward_speed < 80.0, "and it costs plenty when not");
            }

            // Side-swipe.
            let mut graze = car;
            graze.forward_speed = 90.0;
            graze.lateral = 1.0;
            graze.boosting = boosting;
            let here = graze.distance;
            resolve_traffic(&mut graze, &track, here, 0.2, 28.0, &r, &t).expect("a graze");
            if boosting {
                assert!(
                    (graze.forward_speed - 90.0).abs() < 1.0e-3,
                    "boosting past a car cost {} m/s",
                    90.0 - graze.forward_speed
                );
                assert!(graze.lateral_speed != 0.0, "but it is still shoved sideways");
            }
        }
    }

    #[test]
    fn a_side_swipe_costs_less_than_a_shunt_but_is_not_free() {
        let (track, car, t) = fixture();
        let r = RaceTuning::DEFAULT;

        let mut swiped = car;
        swiped.forward_speed = 90.0;
        swiped.lateral = 1.0;
        let here = swiped.distance;
        resolve_traffic(&mut swiped, &track, here, 0.2, 28.0, &r, &t).expect("a graze");
        let swipe_loss = (90.0 - swiped.forward_speed) / 90.0;

        let mut shunted = car;
        shunted.forward_speed = 90.0;
        let ahead = shunted.distance + 3.0;
        resolve_traffic(&mut shunted, &track, ahead, 0.0, 28.0, &r, &t).expect("a shunt");
        let shunt_loss = (90.0 - shunted.forward_speed) / 90.0;

        assert!(
            swipe_loss < shunt_loss,
            "a graze costs less than a shunt: {swipe_loss} vs {shunt_loss}"
        );
        assert!(
            swipe_loss > 0.04,
            "but a graze at 62 m/s of closing speed is not free: {swipe_loss}"
        );
    }

    /// A shunt on the finish line cannot push the car off the end of the course.
    #[test]
    fn a_shunt_cannot_push_the_car_past_the_end_of_the_course() {
        let track = Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT);
        let t = VehicleTuning::DEFAULT;
        let r = RaceTuning::DEFAULT;
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        place_on_track(&mut car, &track.sample_at(track.length()), 0.0);
        car.forward_speed = 90.0;
        // Traffic just behind it, so the shunt pushes the player forward.
        resolve_traffic(&mut car, &track, track.length() - 3.0, 0.0, 25.0, &r, &t);
        assert!(
            car.distance <= track.length(),
            "the shunt pushed the car to {} on a {} m course",
            car.distance,
            track.length()
        );
        assert!(car.distance >= 0.0);
    }

    #[test]
    fn side_swiping_traffic_deflects_the_player_sideways() {
        let (track, mut car, t) = fixture();
        let r = RaceTuning::DEFAULT;
        car.forward_speed = 70.0;
        car.lateral = 1.0;
        let here = car.distance;
        let impact = resolve_traffic(&mut car, &track, here, 0.2, 30.0, &r, &t)
            .expect("that is a side-swipe");
        assert!(impact.traffic);
        assert!(car.lateral > 1.0, "pushed away from the traffic car");
        assert!(car.lateral_speed > 0.0, "and given lateral velocity");
        assert!(
            car.forward_speed > 40.0,
            "a graze keeps most of the speed: {}",
            car.forward_speed
        );
    }

    #[test]
    fn a_side_swipe_from_the_left_pushes_the_other_way() {
        let (track, mut car, t) = fixture();
        let r = RaceTuning::DEFAULT;
        car.forward_speed = 70.0;
        car.lateral = -1.0;
        let here = car.distance;
        resolve_traffic(&mut car, &track, here, -0.2, 30.0, &r, &t)
            .expect("that is a side-swipe");
        assert!(car.lateral < -1.0);
        assert!(car.lateral_speed < 0.0);
    }

    #[test]
    fn a_graze_is_not_reported_as_an_impact() {
        let (track, mut car, t) = fixture();
        car.forward_speed = 2.0;
        let sample = track.sample_at(car.distance);
        displace_to(&mut car, &track, track.barrier_offset(&sample) + 0.05);
        car.lateral_speed = 0.4;
        assert!(
            resolve_barrier(&mut car, &track, &t).is_none(),
            "too gentle to report"
        );
        // But the car is still pushed back inside.
        assert!(car.lateral.abs() <= track.barrier_offset(&sample) - t.half_width + 1.0e-3);
    }

    #[test]
    fn a_bigger_impact_overrides_a_smaller_one_still_ringing() {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        register(&mut car, Vec3::UNIT_X, 0.3, false);
        assert!((car.impact_strength - 0.3).abs() < 1.0e-6);
        register(&mut car, Vec3::UNIT_Z, 0.1, false);
        assert!((car.impact_strength - 0.3).abs() < 1.0e-6, "the small one does not win");
        register(&mut car, Vec3::UNIT_Z, 0.8, false);
        assert!((car.impact_strength - 0.8).abs() < 1.0e-6, "the big one does");
        assert_eq!(car.impact_direction, Vec3::UNIT_Z);
    }

    #[test]
    fn a_degenerate_impact_direction_falls_back_instead_of_producing_a_nan() {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        let impact = register(&mut car, Vec3::ZERO, 0.5, false).expect("reported");
        assert_eq!(impact.direction, Vec3::UNIT_Z);
        assert!(car.is_finite());
    }

    #[test]
    fn a_near_miss_needs_closeness_and_closing_speed_and_no_contact() {
        let (_, mut car, t) = fixture();
        let r = RaceTuning::DEFAULT;
        car.forward_speed = 80.0;
        car.lateral = 0.0;
        car.distance = 500.0;
        let gap = t.half_width + r.traffic_half_width + 0.4;

        assert!(
            is_near_miss(&car, 500.0, gap, 30.0, &r, &t),
            "alongside, close, and closing fast"
        );
        assert!(
            !is_near_miss(&car, 500.0, gap, 79.0, &r, &t),
            "not closing fast enough"
        );
        assert!(
            !is_near_miss(&car, 500.0, r.near_miss_gap + 1.0, 30.0, &r, &t),
            "too far across"
        );
        assert!(
            !is_near_miss(&car, 560.0, gap, 30.0, &r, &t),
            "not alongside"
        );
        assert!(
            !is_near_miss(&car, 500.0, 0.1, 30.0, &r, &t),
            "that is a collision, not a near miss"
        );
    }

    #[test]
    fn impact_strength_is_bounded_and_scales_with_closing_speed() {
        let t = VehicleTuning::DEFAULT;
        assert_eq!(impact_strength(0.0, &t), 0.0);
        assert!(impact_strength(-1.0e6, &t) <= 1.0);
        assert!(impact_strength(50.0, &t) > impact_strength(10.0, &t));
        assert_eq!(impact_strength(-30.0, &t), impact_strength(30.0, &t));
    }
}
