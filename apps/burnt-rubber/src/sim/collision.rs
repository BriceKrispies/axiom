//! Racing collision **geometry**: where two things overlap, how far, and along
//! which axis — plus the bounded separation that pushes them apart again.
//!
//! What this module deliberately does *not* decide is how bad a contact was or
//! what it should cost. That is [`super::contact`]'s job, and the split is the
//! point: geometry is about the world and has no opinions, response is about the
//! game and is nothing but opinions. Before the split the two were one function,
//! and the consequence was that "the boxes still overlap" and "you have been hit
//! again" were the same statement — so a single mistake was re-charged at 60 Hz
//! until the car had stopped. See [`super::contact`] for the full diagnosis.
//!
//! Both cases are resolved the same way — **positionally, then in velocity** —
//! and both are expressed entirely in bounded velocities, never in accumulated
//! forces. There is no penetration spring to tune, no impulse to integrate, and
//! therefore no way for one bad frame to hand the next frame a number that grows.

use axiom_math::Vec3;

use crate::track::{Track, TrackSample};
use crate::tuning::{CollisionTuning, RaceTuning, VehicleTuning};

use super::car::CarState;
use super::contact::{ContactFacts, ContactState, Impact, Obstacle};
use super::traffic::TrafficCar;

/// Push the car back inside the barriers if it has crossed them, and resolve the
/// contact against `contact`'s episode ledger. Returns the impact if one was
/// worth reporting.
///
/// Called from inside the position integration's sub-moves, so the car is
/// checked against the barrier several times per fixed step and can never end a
/// step outside the road no matter how fast it is going. That is also exactly
/// why the episode ledger has to reach in here: two sub-moves against one wall
/// are one collision, and only the ledger knows that.
pub fn resolve_barrier(
    car: &mut CarState,
    track: &Track,
    vehicle: &VehicleTuning,
    tuning: &CollisionTuning,
    contact: &mut ContactState,
) -> Option<Impact> {
    let sample = track.sample_at(car.distance);
    // A walled section has no guardrail — its barrier is the tunnel lining or
    // the canyon wall, which is a different kind of thing to hit and is
    // classified as one.
    let obstacle = if sample.section.walled() {
        Obstacle::Scenery
    } else {
        Obstacle::Barrier
    };
    let limit = (track.barrier_offset(&sample) - vehicle.half_width).max(1.0);
    let overshoot = car.lateral.abs() - limit;
    // Report the clearance even when there is no contact, so an episode against
    // this wall ends the moment the car has genuinely driven away from it.
    contact.note_gap(obstacle, -overshoot, tuning);
    if overshoot <= 0.0 {
        return None;
    }
    let side = if car.lateral >= 0.0 { 1.0 } else { -1.0 };

    // Positional correction along the *track's* lateral axis, which preserves
    // the car's progress along the course exactly. Unlike traffic, this is a
    // full correction rather than a bounded one: the barrier is the edge of the
    // playable world and a car may not be outside it for even one frame.
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

    // The wall's inward normal — the direction the car has to go to get out.
    let normal = sample.right.mul_scalar(-side);
    let planar = planar_velocity(car);
    let into_wall = planar.dot(normal);
    // Already travelling away from the obstacle: the positional correction was
    // enough, and reflecting again would fire the car back across the road.
    if into_wall >= 0.0 {
        return None;
    }

    // Remove the closing component and give back a fraction of it. This is the
    // barrier's *physical* response and happens whether or not an episode is
    // already running — a wall does not stop being solid because you hit it a
    // moment ago. Only the momentum cost and the feedback are episode-gated.
    let reflected = planar.subtract(normal.mul_scalar(into_wall * (1.0 + tuning.barrier_restitution)));
    write_planar(car, reflected);

    let facts = ContactFacts {
        obstacle,
        normal,
        bias: normal,
        normal_speed: (-into_wall).max(0.0),
        player_speed: car.forward_speed,
        obstacle_speed: 0.0,
        squareness: squareness(planar, normal),
        rear_hit: false,
    };
    contact.respond(car, &facts, tuning)
}

/// The geometry of a player/traffic overlap, in track space.
///
/// The test is done in track space — along-course distance against lateral
/// offset — rather than in world space. On a road that is at most gently curved
/// over a car's length the two agree; and doing it in track space means the test
/// is two scalar comparisons instead of an oriented-box intersection, which is
/// what keeps a pool of traffic free.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrafficOverlap {
    /// How far the boxes interpenetrate across the road (m).
    pub across_penetration: f32,
    /// How far the boxes interpenetrate along the course (m).
    pub along_penetration: f32,
    /// `+1` if the player is to the simulation's right of the traffic car.
    pub side: f32,
    /// `+1` if the player is further along the course than the traffic car.
    pub ahead: f32,
    /// Whether the shallower axis is the lateral one — a side-swipe rather than
    /// a nose-to-tail shunt.
    pub sideways: bool,
}

impl TrafficOverlap {
    /// The penetration along the axis this contact resolves on (m).
    pub fn penetration(&self) -> f32 {
        if self.sideways {
            self.across_penetration
        } else {
            self.along_penetration
        }
    }
}

/// How far apart the player and a traffic car are, negative while overlapping.
///
/// The larger of the two axis clearances, because two boxes are apart as soon as
/// they are apart on *either* axis. This is what ends a contact episode, so it
/// has to be the honest separation rather than a distance between centres.
pub fn traffic_gap(
    car: &CarState,
    traffic_distance: f32,
    traffic_lateral: f32,
    race: &RaceTuning,
    vehicle: &VehicleTuning,
) -> f32 {
    let along = (car.distance - traffic_distance).abs() - (vehicle.half_length + race.traffic_half_length);
    let across = (car.lateral - traffic_lateral).abs() - (vehicle.half_width + race.traffic_half_width);
    along.max(across)
}

/// Test the player against a traffic car occupying `(distance, lateral)`.
pub fn traffic_overlap(
    car: &CarState,
    traffic_distance: f32,
    traffic_lateral: f32,
    race: &RaceTuning,
    vehicle: &VehicleTuning,
) -> Option<TrafficOverlap> {
    let along = (car.distance - traffic_distance).abs();
    let across = (car.lateral - traffic_lateral).abs();
    let along_limit = vehicle.half_length + race.traffic_half_length;
    let across_limit = vehicle.half_width + race.traffic_half_width;
    if along >= along_limit || across >= across_limit {
        return None;
    }
    let across_penetration = across_limit - across;
    let along_penetration = along_limit - along;
    Some(TrafficOverlap {
        across_penetration,
        along_penetration,
        side: if car.lateral >= traffic_lateral { 1.0 } else { -1.0 },
        ahead: if car.distance >= traffic_distance { 1.0 } else { -1.0 },
        // Push out along whichever axis is least penetrated — a nose-to-tail
        // shunt resolves along the road, a side-swipe resolves across it.
        sideways: across_penetration <= along_penetration,
    })
}

/// Build the deterministic facts a traffic contact is classified from.
///
/// Note the closing speed is **relative**: rear-ending a car doing 30 m/s at
/// 35 m/s is a 5 m/s contact, not a 35 m/s one. Getting that wrong is how a
/// gentle catch-up in dense traffic ends up classified as a crash.
pub fn traffic_facts(
    car: &CarState,
    overlap: &TrafficOverlap,
    traffic_speed: f32,
    traffic_slot: u32,
    sample: &TrackSample,
    escape: f32,
) -> ContactFacts {
    let course = sample.flat_forward();
    let across = sample.right.mul_scalar(overlap.side);
    // The push-out direction: across the road for a side-swipe, along it for a
    // shunt.
    let normal = if overlap.sideways {
        across
    } else {
        course.mul_scalar(overlap.ahead)
    };
    // Relative velocity: the traffic car is a lane-follower, so all of its
    // motion is along the course.
    let relative = planar_velocity(car).subtract(course.mul_scalar(traffic_speed));
    let into = relative.dot(normal);
    ContactFacts {
        obstacle: Obstacle::Traffic { slot: traffic_slot },
        normal,
        // Sideways contacts push you off the way you came in; a shunt has no
        // natural side, so the caller names the way round with more room.
        bias: if overlap.sideways {
            across
        } else {
            sample.right.mul_scalar(escape)
        },
        normal_speed: (-into).max(0.0),
        player_speed: car.forward_speed,
        obstacle_speed: traffic_speed,
        squareness: squareness(relative, normal),
        rear_hit: !overlap.sideways && overlap.ahead < 0.0,
    }
}

/// Push an overlapping player and traffic car apart, over as many fixed steps as
/// it takes, without teleporting either of them.
///
/// This runs on **every** overlapping step, including the suppressed ones — the
/// episode ledger stops the player being charged twice for one mistake, it does
/// not make them intangible. Three rules make it safe:
///
/// * every move is clamped to [`CollisionTuning::separation_step`], so a deep
///   overlap resolves over several steps rather than as a jump;
/// * the traffic car takes the larger share (it is the lighter body) but only
///   within its yield budget, so it can never be pushed off the road, and any
///   remainder falls back to the player;
/// * only the lateral and along-course axes are touched — nothing here can put
///   a vertical impulse into a car, which is the classic way a de-penetration
///   pass launches something into the sky.
#[allow(clippy::too_many_arguments)]
pub fn separate_from_traffic(
    car: &mut CarState,
    traffic: &mut TrafficCar,
    overlap: &TrafficOverlap,
    sample: &TrackSample,
    escape: f32,
    course_length: f32,
    tuning: &CollisionTuning,
) {
    let total = overlap.penetration().max(0.0);
    let player_share = (total * tuning.player_separation_share).min(tuning.separation_step);
    let traffic_share = (total - player_share).min(tuning.separation_step);

    if overlap.sideways {
        // The player moves out across the road; the traffic car yields the rest
        // sideways, up to its bounded lane offset.
        let yielded = traffic.yield_lateral(-overlap.side * traffic_share, tuning);
        let player_move = player_share + (traffic_share - yielded.abs()).max(0.0);
        let moved = player_move.min(tuning.separation_step);
        car.position = car
            .position
            .add(sample.right.mul_scalar(overlap.side * moved));
        car.lateral += overlap.side * moved;
        car.lateral_speed += overlap.side * separation_bias(total, tuning);
    } else {
        // A shunt: the traffic car is knocked along the road, the player takes
        // the rest of the correction, and — the part that stops a rear-end
        // becoming a grind — the player is biased sideways so they slide round
        // rather than bulldozing.
        let shunted = traffic.yield_forward(-overlap.ahead * traffic_share, tuning);
        let player_move = player_share + (traffic_share - shunted.abs()).max(0.0);
        let moved = player_move.min(tuning.separation_step);
        let course = sample.flat_forward();
        car.position = car
            .position
            .add(course.mul_scalar(overlap.ahead * moved));
        // Clamped to the course. `distance` is a course coordinate and every
        // other writer of it goes through `Track::localise`, which clamps; a
        // shunt right on the finish line would otherwise nudge it past the end.
        car.distance = (car.distance + overlap.ahead * moved).clamp(0.0, course_length);
        car.lateral_speed += escape * separation_bias(total, tuning) * SHUNT_ESCAPE_SHARE;
    }
}

/// How much of the extra along-course separation bias a shunt puts sideways.
/// Under a half: enough to slide the player round the obstacle, not enough to
/// throw them into the next lane.
const SHUNT_ESCAPE_SHARE: f32 = 0.5;

/// The bounded velocity bias that keeps a pair coming apart under the
/// integrator, rather than only as position edits that the next step undoes.
fn separation_bias(penetration: f32, tuning: &CollisionTuning) -> f32 {
    let depth = (penetration / tuning.separation_step.max(1.0e-3)).clamp(0.0, 1.0);
    tuning.separation_speed * depth
}

/// The planar (horizontal) velocity, rebuilt from the chassis-frame components.
fn planar_velocity(car: &CarState) -> Vec3 {
    car.forward()
        .mul_scalar(car.forward_speed)
        .add(car.right().mul_scalar(car.lateral_speed))
}

/// Write a world-space planar velocity back into the chassis frame.
fn write_planar(car: &mut CarState, planar: Vec3) {
    let forward = car.forward();
    let right = car.right();
    car.forward_speed = planar.dot(forward);
    car.lateral_speed = planar.dot(right);
}

/// How square a contact was: `0` sliding along the obstacle, `1` straight into
/// it. Degenerate inputs read as a graze rather than as a crash, which is the
/// safe direction to be wrong in.
fn squareness(velocity: Vec3, normal: Vec3) -> f32 {
    velocity
        .normalize()
        .map(|v| v.dot(normal).abs().clamp(0.0, 1.0))
        .unwrap_or(0.0)
}

/// Whether the player is passing a traffic car closely enough to count as a
/// near miss.
///
/// **The rule is the one a player can state without being told it:** you are in
/// the lane next to a car, and you go past it. Nothing else. No history of where
/// you were before, no timer, and — deliberately — no tuned thresholds.
///
/// What that replaced is worth writing down, because the old rule was three
/// numbers pretending to be a rule. It asked for a lateral gap under a
/// `near_miss_gap` of 3.1 m *and* a closing speed over a `near_miss_closing_speed`
/// of 16 m/s. Both are invisible from the driver's seat. A lane is 3.5 m, so the
/// 3.1 m gap meant "adjacent lane, but only if you were also drifting toward the
/// inside of yours" — two identical-looking passes would score differently
/// because of half a metre of lane wander nobody can see. And the closing-speed
/// floor silently switched the whole mechanic off in the exact situation it looks
/// most earned: easing past a car you have nearly matched speed with. The player
/// was left to infer a rule from rewards that fired about half the time.
///
/// Lanes are the right unit precisely because they are the unit the game already
/// speaks in — the traffic holds a lane, the on-rails car picks a lane, and the
/// road is painted in them. "The lane next to it" is a thing you can see.
///
/// The one condition kept beyond adjacency is that **you** are the one passing
/// (`car.forward_speed > traffic_speed`). That is not a threshold to tune, it is
/// the sign of the relative motion, and without it the mechanic pays out for
/// being overtaken while parked — traffic streams past a stationary player in the
/// next lane and every one of them is a near miss.
///
/// Not touching is not tested here because it cannot be false: this is only ever
/// reached on the no-overlap branch of the caller, which handles contact itself
/// and marks the car as spent either way.
pub fn is_near_miss(
    car: &CarState,
    player_lane: i32,
    traffic_distance: f32,
    traffic_lane: i32,
    traffic_speed: f32,
    race: &RaceTuning,
    tuning: &VehicleTuning,
) -> bool {
    let along = (car.distance - traffic_distance).abs();
    let alongside = along < tuning.half_length + race.traffic_half_length + NEAR_MISS_ALONG;
    let adjacent = (player_lane - traffic_lane).abs() == 1;
    let passing = car.forward_speed > traffic_speed;
    alongside & adjacent & passing
}

/// Extra along-course window (m) either side of contact in which a pass counts.
const NEAR_MISS_ALONG: f32 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;
    use crate::sim::contact::Severity;
    use crate::sim::controller::{place_on_track, step, LOCALISE_WINDOW};
    use crate::tuning::Tuning;

    fn fixture() -> (Track, CarState, Tuning, ContactState) {
        let tuning = Tuning::DEFAULT;
        let track = Track::fixture(crate::DEFAULT_SEED);
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        place_on_track(&mut car, &track.sample_at(120.0), 0.0);
        (track, car, tuning, ContactState::new())
    }

    /// Drive one step through the real controller, the way the simulation does.
    fn drive(
        car: &mut CarState,
        command: DriveCommand,
        track: &Track,
        tuning: &Tuning,
        contact: &mut ContactState,
        boost: bool,
    ) {
        step(car, command, track, tuning, boost, contact, None);
        contact.advance(car, &tuning.collision);
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

    fn barrier(
        car: &mut CarState,
        track: &Track,
        tuning: &Tuning,
        contact: &mut ContactState,
    ) -> Option<Impact> {
        resolve_barrier(car, track, &tuning.vehicle, &tuning.collision, contact)
    }

    #[test]
    fn a_car_inside_the_barriers_is_left_alone() {
        let (track, mut car, t, mut contact) = fixture();
        car.forward_speed = 60.0;
        let before = car;
        assert!(barrier(&mut car, &track, &t, &mut contact).is_none());
        assert_eq!(car, before);
    }

    #[test]
    fn crossing_the_barrier_puts_the_car_back_on_the_road() {
        let (track, mut car, t, mut contact) = fixture();
        car.forward_speed = 70.0;
        let sample = track.sample_at(car.distance);
        let limit = track.barrier_offset(&sample);
        displace_to(&mut car, &track, limit + 4.0);
        // Aim it further into the wall.
        car.lateral_speed = 20.0;

        let impact = barrier(&mut car, &track, &t, &mut contact).expect("that is a collision");
        assert!(impact.strength > 0.0 && impact.strength <= 1.0);
        assert!(!impact.traffic);
        assert!(
            car.lateral.abs() <= limit - t.vehicle.half_width + 1.0e-3,
            "the car is inside the barrier: {} vs {limit}",
            car.lateral
        );
        assert!(car.is_finite());
    }

    #[test]
    fn a_barrier_impact_costs_speed_without_stopping_the_car() {
        let (track, mut car, t, mut contact) = fixture();
        car.forward_speed = 80.0;
        let sample = track.sample_at(car.distance);
        displace_to(&mut car, &track, track.barrier_offset(&sample) + 1.0);
        car.lateral_speed = 40.0;
        barrier(&mut car, &track, &t, &mut contact);
        assert!(car.forward_speed < 80.0, "the hit hurt");
        assert!(
            car.forward_speed >= 80.0 * t.collision.crash_speed_floor,
            "but the demo continues: {}",
            car.forward_speed
        );
    }

    /// Grinding a wall must not be re-charged every step. This is the barrier
    /// half of the bug the contact episodes exist to fix, and it is measured
    /// through the *real* controller so the sub-move loop is in the picture.
    #[test]
    fn grinding_a_barrier_does_not_take_speed_every_step() {
        let (track, mut car, t, mut contact) = fixture();
        for _ in 0..240 {
            drive(&mut car, DriveCommand::FLAT_OUT, &track, &t, &mut contact, false);
        }
        // Bury the car in the wall and hold it there at full throttle.
        for _ in 0..90 {
            drive(&mut car, DriveCommand::turning(1.0), &track, &t, &mut contact, false);
        }
        let ground_down = car.forward_speed;
        assert!(
            ground_down > 20.0,
            "ninety steps of wall took the car to {ground_down} m/s"
        );
        // The car is still genuinely against the wall.
        let sample = track.sample_at(car.distance);
        assert!(car.lateral.abs() > sample.half_width, "still on the barrier");
    }

    /// Scrape alignment must not fight a car that is steering off the wall.
    ///
    /// The failure mode that ruins an arcade racer is being *pinned* against a
    /// wall. Grinding one at full lock for four seconds is allowed to cost
    /// speed — but the moment you straighten up you must be able to drive away,
    /// which is what the scrape alignment guarantees.
    #[test]
    fn barrier_alignment_fades_as_the_nose_turns_away_from_the_wall() {
        let (track, mut car, t, mut contact) = fixture();
        for _ in 0..240 {
            drive(&mut car, DriveCommand::FLAT_OUT, &track, &t, &mut contact, false);
        }
        for _ in 0..240 {
            drive(&mut car, DriveCommand::turning(1.0), &track, &t, &mut contact, false);
        }
        let pinned = car.lateral;
        assert!(
            pinned.abs() > track.sample_at(car.distance).half_width,
            "it is on the wall"
        );

        // The car must come off the wall immediately and keep coming off it for
        // as long as it is still out there. Once it is back on the tarmac it is
        // free to move around — that is a racing line, not a trap — so the
        // monotonic claim applies only while it is still off the road.
        let mut previous = pinned.abs();
        for second in 0..3 {
            let was_off_road = previous > track.sample_at(car.distance).half_width;
            for _ in 0..60 {
                let command = crate::script::autopilot(&car, &track);
                drive(&mut car, command, &track, &t, &mut contact, false);
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

    /// The scrape alignment, isolated: a car nosed into a barrier is rotated
    /// toward the road's direction, not away from it.
    #[test]
    fn a_barrier_turns_the_car_back_along_itself() {
        let (track, mut car, t, mut contact) = fixture();
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
            barrier(&mut car, &track, &t, &mut contact);
            car.lateral = track.barrier_offset(&sample) + 1.0;
        }
        let after = crate::track::shortest_angle(car.yaw - road_yaw).abs();
        assert!(after < before * 0.5, "the wall straightened it: {before} -> {after}");
    }

    #[test]
    fn repeated_wall_contact_stays_stable_over_a_long_run() {
        let (track, mut car, t, mut contact) = fixture();
        for i in 0..6_000 {
            let steer = if (i / 40) % 2 == 0 { 1.0 } else { -1.0 };
            drive(&mut car, DriveCommand::turning(steer), &track, &t, &mut contact, true);
            assert!(car.is_finite(), "step {i} produced {car:?}");
        }
        let sample = track.sample_at(car.distance);
        assert!(car.lateral.abs() <= track.barrier_offset(&sample) + 1.0e-2);
    }

    #[test]
    fn a_car_already_leaving_the_wall_is_not_reflected_again() {
        let (track, mut car, t, mut contact) = fixture();
        car.forward_speed = 40.0;
        let sample = track.sample_at(car.distance);
        displace_to(&mut car, &track, track.barrier_offset(&sample) + 0.5);
        // Moving back toward the middle of the road already.
        car.lateral_speed = -25.0;
        let lateral_before = car.lateral_speed;
        let impact = barrier(&mut car, &track, &t, &mut contact);
        assert!(impact.is_none(), "no second reflection");
        assert_eq!(car.lateral_speed, lateral_before);
    }

    /// A tunnel wall is not a guardrail, and the classifier knows it.
    #[test]
    fn a_walled_section_is_scenery_rather_than_a_barrier() {
        let tuning = Tuning::DEFAULT;
        let track = Track::fixture(crate::DEFAULT_SEED);
        let walled = track
            .samples()
            .iter()
            .find(|s| s.section.walled())
            .copied()
            .expect("the course has a tunnel");
        let open = track
            .samples()
            .iter()
            .find(|s| !s.section.walled())
            .copied()
            .expect("and open road");

        // The *identical* contact — same speeds, same angle — against each. All
        // that differs is what is being hit.
        let hit = |sample: &crate::track::TrackSample| {
            let mut car = CarState::parked(Vec3::ZERO, 0.0);
            place_on_track(&mut car, sample, 0.0);
            car.forward_speed = 12.0;
            displace_to(&mut car, &track, track.barrier_offset(sample) + 0.6);
            car.lateral_speed = 18.0;
            let mut contact = ContactState::new();
            let impact =
                barrier(&mut car, &track, &tuning, &mut contact).expect("a wall contact");
            (impact.severity, contact)
        };

        let (severity, contact) = hit(&walled);
        assert_eq!(
            severity,
            Severity::MajorCrash,
            "square into rock at 18 m/s of closing is a crash"
        );
        assert!(contact.is_suppressed(Obstacle::Scenery), "and it opens a scenery episode");
        assert!(!contact.is_suppressed(Obstacle::Barrier), "not a guardrail episode");

        let (guardrail, contact) = hit(&open);
        assert_eq!(
            guardrail,
            Severity::Bump,
            "the same hit against a guardrail is an ordinary bump — a rail gives, rock does not"
        );
        assert!(contact.is_suppressed(Obstacle::Barrier));
    }

    #[test]
    fn walled_sections_have_their_barriers_at_the_shoulder() {
        let track = Track::fixture(crate::DEFAULT_SEED);
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
    fn traffic_far_away_is_not_an_overlap() {
        let (_, car, t, _) = fixture();
        let here = car.distance;
        assert!(traffic_overlap(&car, here + 50.0, 0.0, &t.race, &t.vehicle).is_none());
        assert!(traffic_overlap(&car, here, 20.0, &t.race, &t.vehicle).is_none());
        assert!(traffic_gap(&car, here + 50.0, 0.0, &t.race, &t.vehicle) > 0.0);
    }

    #[test]
    fn the_overlap_resolves_along_the_shallower_axis() {
        let (_, car, t, _) = fixture();
        let here = car.distance;
        // Nose to tail: deeply overlapped across, barely along.
        let shunt = traffic_overlap(&car, here + 4.4, 0.0, &t.race, &t.vehicle).expect("a shunt");
        assert!(!shunt.sideways, "a nose-to-tail contact resolves along the road");
        assert!(shunt.ahead < 0.0, "and the player is behind");
        assert!((shunt.penetration() - shunt.along_penetration).abs() < 1.0e-6);

        // Side by side: deeply overlapped along, barely across.
        let swipe = traffic_overlap(&car, here, 2.0, &t.race, &t.vehicle).expect("a swipe");
        assert!(swipe.sideways, "an alongside contact resolves across the road");
        assert!(swipe.side < 0.0, "the player is to the traffic car's left");
        assert!((swipe.penetration() - swipe.across_penetration).abs() < 1.0e-6);
    }

    #[test]
    fn the_gap_is_negative_while_overlapping_and_grows_once_apart() {
        let (_, car, t, _) = fixture();
        let here = car.distance;
        assert!(traffic_gap(&car, here, 0.0, &t.race, &t.vehicle) < 0.0, "on top of it");
        let apart = traffic_gap(&car, here, 4.0, &t.race, &t.vehicle);
        let further = traffic_gap(&car, here, 6.0, &t.race, &t.vehicle);
        assert!(apart > 0.0 && further > apart, "{apart} then {further}");
    }

    #[test]
    fn traffic_facts_measure_the_relative_closing_speed_not_the_raw_speed() {
        let (track, mut car, t, _) = fixture();
        car.forward_speed = 35.0;
        let sample = track.sample_at(car.distance);
        let here = car.distance;
        let overlap = traffic_overlap(&car, here + 4.4, 0.0, &t.race, &t.vehicle).expect("a shunt");
        let facts = traffic_facts(&car, &overlap, 30.0, 12, &sample, 1.0);
        assert!(
            (facts.normal_speed - 5.0).abs() < 0.5,
            "catching a 30 m/s car at 35 is a 5 m/s contact, not 35: {}",
            facts.normal_speed
        );
        assert!(facts.rear_hit, "and it is a rear-end");
        assert_eq!(facts.obstacle, Obstacle::Traffic { slot: 12 });
        assert!(facts.is_finite());
        // The same geometry against a stationary car really is a 35 m/s hit.
        let stopped = traffic_facts(&car, &overlap, 0.0, 12, &sample, 1.0);
        assert!(stopped.normal_speed > 30.0, "{}", stopped.normal_speed);
    }

    /// The whole rule, stated as a player would: the lane next to you, and you
    /// go past it. Each assertion below is one half of that sentence failing.
    #[test]
    fn a_near_miss_is_the_next_lane_over_and_you_going_past_it() {
        let (_, mut car, t, _) = fixture();
        let r = t.race;
        let v = t.vehicle;
        car.forward_speed = 80.0;
        car.distance = 500.0;

        assert!(
            is_near_miss(&car, 0, 500.0, 1, 30.0, &r, &v),
            "the next lane over, alongside, and going past it"
        );
        assert!(
            is_near_miss(&car, 1, 500.0, 0, 30.0, &r, &v),
            "and it reads the same from the other side"
        );
        assert!(
            !is_near_miss(&car, 1, 500.0, 1, 30.0, &r, &v),
            "the same lane is a car you are about to hit, not one you threaded"
        );
        assert!(
            !is_near_miss(&car, -1, 500.0, 1, 30.0, &r, &v),
            "two lanes away is just traffic on the far side of the road"
        );
        assert!(
            !is_near_miss(&car, 0, 560.0, 1, 30.0, &r, &v),
            "60 m up the road is not a pass"
        );
        assert!(
            !is_near_miss(&car, 0, 500.0, 1, 95.0, &r, &v),
            "being overtaken is not passing — otherwise parking pays out"
        );
    }

    /// The closing-speed floor is gone on purpose, and this is the case that
    /// argued for removing it: easing past a car you have nearly matched speed
    /// with is the pass that *looks* most deliberate, and the old rule scored it
    /// zero. One metre per second of advantage is a pass.
    #[test]
    fn crawling_past_a_car_still_counts() {
        let (_, mut car, t, _) = fixture();
        let (r, v) = (t.race, t.vehicle);
        car.forward_speed = 31.0;
        car.distance = 500.0;
        assert!(is_near_miss(&car, 0, 500.0, 1, 30.0, &r, &v));
    }

    /// Build a traffic car overlapping the player by construction.
    fn planted(car: &CarState, along: f32, across: f32, speed: f32) -> TrafficCar {
        let course = crate::course::procedural::shipping_plan(1).expect("compiles");
        let mut planted = crate::sim::traffic::activate(&course.traffic()[3], 3, course.track());
        planted.active = true;
        planted.distance = car.distance + along;
        planted.lateral = car.lateral + across;
        planted.speed = speed;
        planted.yield_offset = 0.0;
        planted.yield_speed = 0.0;
        planted
    }

    #[test]
    fn separation_reduces_penetration_step_after_step_until_the_pair_is_clear() {
        let (track, mut car, t, _) = fixture();
        car.forward_speed = 60.0;
        // Deeply interpenetrated, side by side.
        let mut traffic = planted(&car, 0.0, 0.4, 30.0);
        let mut penetration = f32::INFINITY;
        let mut steps = 0;
        while let Some(overlap) =
            traffic_overlap(&car, traffic.distance, traffic.lateral, &t.race, &t.vehicle)
        {
            assert!(
                overlap.penetration() < penetration,
                "step {steps}: penetration stopped falling at {}",
                overlap.penetration()
            );
            penetration = overlap.penetration();
            let sample = track.sample_at(car.distance);
            separate_from_traffic(&mut car, &mut traffic, &overlap, &sample, 1.0, track.length(), &t.collision);
            // The traffic car's own step is what turns its yield into position.
            traffic.lateral += traffic.yield_offset;
            traffic.yield_offset = 0.0;
            steps += 1;
            assert!(steps < 60, "the pair never came apart");
        }
        assert!(steps > 1, "a deep overlap takes several steps, not a teleport");
        assert!(
            traffic_gap(&car, traffic.distance, traffic.lateral, &t.race, &t.vehicle) >= 0.0,
            "and they end up genuinely apart"
        );
    }

    #[test]
    fn separation_never_teleports_either_body_or_lifts_them_off_the_road() {
        let (track, mut car, t, _) = fixture();
        car.forward_speed = 80.0;
        // A pathologically deep overlap — far deeper than the game can produce.
        let mut traffic = planted(&car, 0.05, 0.05, 30.0);
        let sample = track.sample_at(car.distance);
        for step in 0..40 {
            let Some(overlap) =
                traffic_overlap(&car, traffic.distance, traffic.lateral, &t.race, &t.vehicle)
            else {
                break;
            };
            let before = car.position;
            let lateral_before = car.lateral_speed;
            separate_from_traffic(&mut car, &mut traffic, &overlap, &sample, 1.0, track.length(), &t.collision);
            let moved = car.position.distance(before);
            assert!(
                moved <= t.collision.separation_step + 1.0e-4,
                "step {step} moved the player {moved} m in one step"
            );
            assert!(
                (car.position.y - before.y).abs() < 1.0e-6,
                "separation put a vertical impulse into the car"
            );
            assert!(
                (car.lateral_speed - lateral_before).abs()
                    <= t.collision.separation_speed + 1.0e-4,
                "and the velocity bias is bounded"
            );
            assert!(car.is_finite());
            assert!(traffic.yield_offset.abs() <= t.collision.traffic_yield_lateral + 1.0e-4);
            assert!(traffic.yield_speed.abs() <= t.collision.traffic_yield_speed + 1.0e-4);
            traffic.lateral += traffic.yield_offset;
            traffic.yield_offset = 0.0;
        }
    }

    /// A shunt must push the player *round* the obstacle as well as back from
    /// it, or "stuck behind a car" is the whole experience of dense traffic.
    #[test]
    fn a_rear_end_biases_the_player_sideways_as_well_as_back() {
        let (track, mut car, t, _) = fixture();
        car.forward_speed = 80.0;
        let mut traffic = planted(&car, 3.5, 0.0, 30.0);
        let overlap = traffic_overlap(&car, traffic.distance, traffic.lateral, &t.race, &t.vehicle)
            .expect("a shunt");
        assert!(!overlap.sideways);
        let sample = track.sample_at(car.distance);
        let before = car.lateral_speed;
        separate_from_traffic(&mut car, &mut traffic, &overlap, &sample, 1.0, track.length(), &t.collision);
        assert!(
            car.lateral_speed > before,
            "the player is nudged toward the escape side"
        );
        assert!(traffic.yield_speed > 0.0, "and the traffic car is knocked along");
        assert!(car.distance < car.distance + overlap.along_penetration);
    }

    /// A shunt on the finish line cannot push the car off the end of the course.
    ///
    /// The clamp this pins was lost once already, in the rewrite that split the
    /// resolver into geometry and response: `distance` is a course coordinate,
    /// every other writer of it goes through `Track::localise` (which clamps),
    /// and separation is the one place that moves it directly.
    #[test]
    fn a_shunt_cannot_push_the_car_past_either_end_of_the_course() {
        let t = Tuning::DEFAULT;
        let track = Track::fixture(crate::DEFAULT_SEED);
        for (place, along) in [(track.length(), -3.0f32), (0.0, 3.0)] {
            let mut car = CarState::parked(Vec3::ZERO, 0.0);
            place_on_track(&mut car, &track.sample_at(place), 0.0);
            car.forward_speed = 90.0;
            let mut traffic = planted(&car, along, 0.0, 25.0);
            let overlap =
                traffic_overlap(&car, traffic.distance, traffic.lateral, &t.race, &t.vehicle)
                    .expect("a shunt");
            let sample = track.sample_at(car.distance);
            for _ in 0..20 {
                separate_from_traffic(
                    &mut car,
                    &mut traffic,
                    &overlap,
                    &sample,
                    1.0,
                    track.length(),
                    &t.collision,
                );
                assert!(
                    (0.0..=track.length()).contains(&car.distance),
                    "a shunt at {place} pushed the car to {} on a {} m course",
                    car.distance,
                    track.length()
                );
            }
        }
    }

    #[test]
    fn squareness_reads_one_head_on_and_zero_along() {
        let normal = Vec3::UNIT_X;
        assert!((squareness(Vec3::new(-10.0, 0.0, 0.0), normal) - 1.0).abs() < 1.0e-5);
        assert!(squareness(Vec3::new(0.0, 0.0, 30.0), normal).abs() < 1.0e-5);
        // A 45° hit reads as half its energy going into the obstacle.
        let diagonal = squareness(Vec3::new(-10.0, 0.0, 10.0), normal);
        assert!((diagonal - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01, "{diagonal}");
        assert_eq!(squareness(Vec3::ZERO, normal), 0.0, "a degenerate hit is a graze");
    }
}
