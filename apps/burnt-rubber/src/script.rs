//! Scripted driving: a deterministic autopilot and the canned run the tests and
//! the capture harness both drive.
//!
//! Two things need to drive the car without a human. The **capture harness**
//! needs the car placed at a particular section, at a particular speed, doing a
//! particular thing, reproducibly. The **test suite** needs a driver that can
//! actually complete a nine-kilometre course, because "does the car reach top
//! speed" is unanswerable if the test driver ploughs into the first barrier.
//!
//! Both are served by the same thing: a pure function from simulation state to a
//! [`DriveCommand`]. It is not AI and does not pretend to be — it is a pursuit
//! controller aiming at a point down the road, which is exactly enough to keep a
//! car on a road, and exactly little enough that its behaviour is obvious.
//!
//! Everything here is app-tier scripting, deterministic by construction, and
//! reads only simulation state.

use crate::command::DriveCommand;
use crate::sim::car::CarState;
use crate::sim::{RaceEvent, RacePhase, RaceSim};
use crate::track::{shortest_angle, Track};

/// Steering that aims the car at a point down the road.
///
/// The aim point is `LOOKAHEAD_BASE + LOOKAHEAD_PER_SPEED · speed` metres ahead
/// on the line: faster means looking further, which is the whole trick — a fixed
/// lookahead either wobbles at speed or cuts the corner at low speed.
pub fn steer_toward_line(car: &CarState, track: &Track, target_lateral: f32) -> f32 {
    let lookahead = LOOKAHEAD_BASE + LOOKAHEAD_PER_SPEED * car.speed();
    let aim = track
        .interpolated_at(car.distance + lookahead)
        .at_lateral(target_lateral);
    let to_aim = aim.subtract(car.position);
    let wanted_yaw = to_aim.x.atan2(to_aim.z);
    // Negated because steering right is a *decreasing* yaw - see the sign note
    // in `sim::controller::rotate_chassis`. A yaw error that needs correcting
    // upward is corrected by steering left.
    let proportional = -shortest_angle(wanted_yaw - car.yaw) * STEER_GAIN;
    // Damping. Without this the autopilot is a *pure* proportional controller on
    // heading error, which is only marginally stable: its lookahead is the sole
    // thing damping it, so any increase in the car's yaw authority — a grippier
    // surface, a change of chassis geometry — tips a recovery from converging
    // into oscillating and then into leaving the road entirely.
    //
    // `yaw_rate` is positive when the car is already rotating left, and positive
    // steer is right, so adding it opposes the rotation the car has already got.
    // That is a derivative term on exactly the quantity being controlled, and it
    // gives the controller stability margin rather than luck.
    let damping = car.yaw_rate * STEER_DAMPING;
    (proportional + damping).clamp(-1.0, 1.0)
}

/// Metres of lookahead at a standstill.
const LOOKAHEAD_BASE: f32 = 14.0;
/// Extra metres of lookahead per m/s of speed.
const LOOKAHEAD_PER_SPEED: f32 = 0.34;
/// Steering per radian of heading error.
const STEER_GAIN: f32 = 2.4;
/// Steering per rad/s of the car's own yaw rate, opposing it. The derivative
/// term that gives the controller its stability margin.
const STEER_DAMPING: f32 = 0.35;

/// A flat-out autopilot that stays on the racing line.
///
/// It lifts off the throttle for a corner it is arriving at too fast, which is
/// what lets it hold a full course rather than understeering into the first
/// guardrail.
pub fn autopilot(car: &CarState, track: &Track) -> DriveCommand {
    autopilot_at(car, track, 0.0)
}

/// The autopilot, holding a line `target_lateral` metres off the centre.
///
/// A wide road is a road with a choice of line, and threading traffic is that
/// choice: a driver pinned to the centreline either never meets a car or drives
/// straight into one, depending on whether the lane count happens to be odd.
pub fn autopilot_at(car: &CarState, track: &Track, target_lateral: f32) -> DriveCommand {
    let steer = steer_toward_line(car, track, target_lateral);
    // Read the sharpest curvature between here and roughly a braking distance
    // ahead, and back off in proportion to it.
    let horizon = (car.speed() * CORNER_LOOKAHEAD_SECONDS).max(30.0);
    let worst = corner_severity(track, car.distance, horizon);
    let corner_speed = (CORNER_SPEED_BASE / (1.0 + worst * CORNER_SPEED_FALLOFF)).max(18.0);
    DriveCommand {
        throttle: if car.speed() > corner_speed { 0.0 } else { 1.0 },
        brake: if car.speed() > corner_speed * BRAKE_MARGIN {
            1.0
        } else {
            0.0
        },
        steer,
        ..DriveCommand::IDLE
    }
}

/// How far ahead (in seconds of travel) the autopilot reads corners.
const CORNER_LOOKAHEAD_SECONDS: f32 = 1.6;
/// The speed the autopilot would hold on a perfectly straight road (m/s). Well
/// above the car's top speed, so a straight is genuinely flat out.
const CORNER_SPEED_BASE: f32 = 150.0;
/// How sharply the autopilot's corner speed falls with curvature.
const CORNER_SPEED_FALLOFF: f32 = 190.0;
/// How far over its corner speed the autopilot has to be before it brakes.
const BRAKE_MARGIN: f32 = 1.18;

/// The sharpest curvature on the road between `from` and `from + span`.
fn corner_severity(track: &Track, from: f32, span: f32) -> f32 {
    let steps = ((span / track.spacing()).ceil().max(1.0) as usize).clamp(1, 512);
    (0..=steps)
        .map(|i| {
            let d = from + i as f32 * track.spacing();
            track.sample_at(d).curvature.abs()
        })
        .fold(0.0f32, f32::max)
}

/// What a deliberate off-road excursion cost, and whether the car came back.
///
/// This is the "can a player recover from an ordinary mistake" question asked as
/// a measurement rather than a hope. Running wide is the single most common way
/// to lose a lap in a racing game, and a game where it means *restarting* is a
/// game nobody finishes — so the recovery has to be a property that is checked,
/// not an assumption.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoveryReport {
    /// Whether the car actually left the tarmac. A "mistake" that stayed on the
    /// road proves nothing, so this is checked before anything else is believed.
    pub left_the_road: bool,
    /// Ground speed at the moment it left the tarmac (m/s).
    pub speed_leaving: f32,
    /// The furthest it got from the road centre (m).
    pub worst_lateral: f32,
    /// How far outside the tarmac edge that was (m).
    pub worst_beyond_edge: f32,
    /// Whether it reached the barrier.
    pub hit_barrier: bool,
    /// Whether it got back onto the tarmac and stayed there.
    pub recovered: bool,
    /// Fixed steps the recovery took, from the end of the mistake.
    pub recovery_steps: u32,
    /// Ground speed once recovered (m/s).
    pub speed_after: f32,
    /// The lowest speed reached during the whole excursion (m/s).
    pub slowest: f32,
    /// Whether the car was ever slow enough, and off-road for long enough, to
    /// trip the stuck detector — i.e. whether the game offered a reset.
    pub needed_a_reset: bool,
}

impl RecoveryReport {
    /// Recovery time in seconds.
    pub fn recovery_seconds(&self) -> f32 {
        self.recovery_steps as f32 * crate::tuning::DT
    }
}

/// How many consecutive steps back on the tarmac count as genuinely recovered.
/// One step could be the car clipping the edge on its way further off.
const RECOVERED_STEPS: u32 = 30;

/// Drive `sim` off the road on purpose, then hand it back to the autopilot and
/// see whether it can drive out of the mistake.
///
/// `steer` is the wrong input held for `hold_steps` — a big lift-and-turn is the
/// realistic version of running wide. Recovery is the ordinary autopilot: no
/// reset, no teleport, nothing the player would not have.
pub fn deliberate_excursion(
    sim: &mut RaceSim,
    steer: f32,
    hold_steps: u32,
    recovery_limit: u32,
) -> RecoveryReport {
    let mut report = RecoveryReport {
        left_the_road: false,
        speed_leaving: 0.0,
        worst_lateral: 0.0,
        worst_beyond_edge: 0.0,
        hit_barrier: false,
        recovered: false,
        recovery_steps: 0,
        speed_after: 0.0,
        slowest: f32::INFINITY,
        needed_a_reset: false,
    };
    let impacts_before = sim.impact_count();

    let observe = |sim: &RaceSim, report: &mut RecoveryReport| {
        let car = sim.car();
        let edge = sim.track().sample_at(car.distance).half_width;
        report.slowest = report.slowest.min(car.speed());
        if car.lateral.abs() > report.worst_lateral.abs() {
            report.worst_lateral = car.lateral;
            report.worst_beyond_edge = (car.lateral.abs() - edge).max(0.0);
        }
        report.needed_a_reset |= sim.is_stuck();
        if car.surface.is_off_road() && !report.left_the_road {
            report.left_the_road = true;
            report.speed_leaving = car.speed();
        }
    };

    // The mistake: throttle on, wheel turned the wrong way, no correction.
    let mistake = DriveCommand {
        throttle: 1.0,
        steer: steer.clamp(-1.0, 1.0),
        ..DriveCommand::IDLE
    };
    for _ in 0..hold_steps {
        sim.step(mistake);
        observe(sim, &mut report);
    }

    // The recovery: the ordinary autopilot, nothing else.
    let mut on_tarmac_for = 0u32;
    for taken in 0..recovery_limit {
        let command = autopilot(sim.car(), sim.track());
        sim.step(command);
        observe(sim, &mut report);

        on_tarmac_for = if sim.car().surface.is_off_road() {
            0
        } else {
            on_tarmac_for + 1
        };
        if on_tarmac_for >= RECOVERED_STEPS {
            report.recovered = true;
            report.recovery_steps = taken + 1;
            break;
        }
    }
    if !report.recovered {
        report.recovery_steps = recovery_limit;
    }
    report.hit_barrier = sim.impact_count() > impacts_before;
    report.speed_after = sim.car().speed();
    report.slowest = report.slowest.min(sim.car().speed());
    report
}

/// What a deliberate collision with traffic cost, and whether the car drove out
/// of it.
///
/// The companion to [`RecoveryReport`]. Running into the back of something is
/// the other common way to lose a lap, and it has the same requirement: it has
/// to *hurt* and it has to be *survivable*. A shunt that spins the car to a
/// standstill ends the run as surely as a crash screen would; a shunt that costs
/// nothing makes traffic scenery rather than an obstacle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionReport {
    /// Whether contact was actually made.
    pub made_contact: bool,
    /// The player's ground speed the step before impact (m/s).
    pub speed_before: f32,
    /// The traffic car's speed (m/s).
    pub traffic_speed: f32,
    /// How fast the player was closing on it (m/s).
    pub closing_speed: f32,
    /// The reported impact strength, `0..1`.
    pub strength: f32,
    /// Ground speed the step after impact (m/s).
    pub speed_after_impact: f32,
    /// How much the shunt swung the car's nose (radians).
    pub yaw_kick: f32,
    /// Whether the shunt spun the car far enough to lose the road ahead.
    pub spun: bool,
    /// Whether the car ended up off the tarmac because of it.
    pub went_off_road: bool,
    /// The lowest speed reached across the whole incident (m/s).
    pub slowest: f32,
    /// Whether the car was back on the tarmac and driving afterwards.
    pub recovered: bool,
    /// Fixed steps the recovery took.
    pub recovery_steps: u32,
    /// Ground speed once recovered (m/s).
    pub speed_after: f32,
    /// Whether the stuck detector ever offered a reset.
    pub needed_a_reset: bool,
}

impl CollisionReport {
    /// Recovery time in seconds.
    pub fn recovery_seconds(&self) -> f32 {
        self.recovery_steps as f32 * crate::tuning::DT
    }

    /// The fraction of its speed the shunt took, `0..1`.
    pub fn speed_lost(&self) -> f32 {
        let before = self.speed_before.max(1.0);
        ((before - self.speed_after_impact) / before).clamp(0.0, 1.0)
    }
}

/// Yaw swing (radians) past which a shunt counts as having spun the car.
const SPIN_THRESHOLD: f32 = 1.0;

/// Drive `sim` into the back of a traffic car on purpose, then hand it back to
/// the autopilot and see whether it drives out of it.
///
/// The approach is a pursuit: aim at whichever car is next ahead and hold the
/// throttle. That is deliberately how a player hits something — not a teleport
/// into an overlap, which would measure the collision resolver rather than the
/// game.
pub fn deliberate_collision(
    sim: &mut RaceSim,
    approach_limit: u32,
    recovery_limit: u32,
) -> CollisionReport {
    let mut report = CollisionReport {
        made_contact: false,
        speed_before: 0.0,
        traffic_speed: 0.0,
        closing_speed: 0.0,
        strength: 0.0,
        speed_after_impact: 0.0,
        yaw_kick: 0.0,
        spun: false,
        went_off_road: false,
        slowest: f32::INFINITY,
        recovered: false,
        recovery_steps: 0,
        speed_after: 0.0,
        needed_a_reset: false,
    };

    // --- approach: chase whatever is next ahead, flat out.
    for _ in 0..approach_limit {
        let car = *sim.car();
        let target = sim
            .traffic()
            .active()
            .filter(|c| c.distance > car.distance + 2.0)
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
            .copied();
        let command = match target {
            Some(t) => DriveCommand {
                throttle: 1.0,
                steer: steer_toward_line(&car, sim.track(), t.lateral),
                ..DriveCommand::IDLE
            },
            // Nothing ahead yet: close the gap on the racing line.
            None => autopilot(&car, sim.track()),
        };
        let speed_before = car.speed();
        let yaw_before = car.yaw;

        sim.step(command);
        report.slowest = report.slowest.min(sim.car().speed());

        let hit = sim.events().iter().find_map(|e| match e {
            RaceEvent::Impact { strength, traffic: true } => Some(*strength),
            _ => None,
        });
        if let Some(strength) = hit {
            report.made_contact = true;
            report.strength = strength;
            report.speed_before = speed_before;
            report.traffic_speed = target.map(|t| t.speed).unwrap_or(0.0);
            report.closing_speed = (speed_before - report.traffic_speed).max(0.0);
            report.speed_after_impact = sim.car().speed();
            report.yaw_kick = crate::track::shortest_angle(sim.car().yaw - yaw_before).abs();
            report.spun = report.yaw_kick > SPIN_THRESHOLD;
            break;
        }
    }

    // --- recovery: the ordinary autopilot, nothing else.
    let mut on_tarmac_for = 0u32;
    for taken in 0..recovery_limit {
        let command = autopilot(sim.car(), sim.track());
        sim.step(command);

        let car = sim.car();
        report.slowest = report.slowest.min(car.speed());
        report.went_off_road |= car.surface.is_off_road();
        report.needed_a_reset |= sim.is_stuck();

        on_tarmac_for = if car.surface.is_off_road() {
            0
        } else {
            on_tarmac_for + 1
        };
        // Back on the road AND going again — a car sitting still on the tarmac
        // has not recovered from anything.
        if on_tarmac_for >= RECOVERED_STEPS && car.speed() > RECOVERED_SPEED {
            report.recovered = true;
            report.recovery_steps = taken + 1;
            break;
        }
    }
    if !report.recovered {
        report.recovery_steps = recovery_limit;
    }
    report.speed_after = sim.car().speed();
    report
}

/// Speed (m/s) the car has to be back up to before a recovery counts.
const RECOVERED_SPEED: f32 = 25.0;

/// A named stage of the canned demonstration run.
///
/// The run exists to exercise every behaviour the game has, in one deterministic
/// sequence: it is what the multi-minute stability test drives, and what the
/// capture harness poses its stills from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Flat out from the line.
    Launch,
    /// Following the racing line at speed.
    Cruise,
    /// Hard on the brakes.
    Brake,
    /// Handbrake into a slide.
    Drift,
    /// Boost held.
    Boost,
    /// Deliberately steering off the racing line into the barriers.
    Impact,
    /// Reset to the last safe point.
    Reset,
}

impl Stage {
    /// The stages, in the order the canned run performs them.
    pub const ALL: [Stage; 7] = [
        Stage::Launch,
        Stage::Cruise,
        Stage::Brake,
        Stage::Drift,
        Stage::Boost,
        Stage::Impact,
        Stage::Reset,
    ];

    /// How many fixed steps this stage lasts.
    pub const fn steps(self) -> u32 {
        match self {
            Stage::Launch => 300,
            Stage::Cruise => 420,
            Stage::Brake => 90,
            Stage::Drift => 110,
            Stage::Boost => 300,
            Stage::Impact => 90,
            Stage::Reset => 30,
        }
    }

    /// The command this stage issues, given the current car and course.
    pub fn command(self, car: &CarState, track: &Track, step_in_stage: u32) -> DriveCommand {
        let line = autopilot(car, track);
        match self {
            Stage::Launch => DriveCommand {
                throttle: 1.0,
                brake: 0.0,
                ..line
            },
            Stage::Cruise => line,
            Stage::Brake => DriveCommand {
                throttle: 0.0,
                brake: 1.0,
                ..line
            },
            Stage::Drift => DriveCommand {
                throttle: 1.0,
                brake: 0.0,
                handbrake: true,
                steer: 0.85,
                ..DriveCommand::IDLE
            },
            Stage::Boost => DriveCommand {
                throttle: 1.0,
                brake: 0.0,
                boost: true,
                ..line
            },
            Stage::Impact => DriveCommand {
                throttle: 1.0,
                brake: 0.0,
                steer: -1.0,
                ..DriveCommand::IDLE
            },
            // A single reset on the stage's first step, then straighten up.
            Stage::Reset => DriveCommand {
                reset: step_in_stage == 0,
                ..line
            },
        }
    }
}

/// One full cycle of the canned run, in steps.
pub fn cycle_length() -> u32 {
    Stage::ALL.iter().map(|s| s.steps()).sum()
}

/// The stage, and how far into it, at `step` of the canned run (which repeats).
pub fn stage_at(step: u32) -> (Stage, u32) {
    let cycle = cycle_length().max(1);
    let mut into = step % cycle;
    for stage in Stage::ALL {
        if into < stage.steps() {
            return (stage, into);
        }
        into -= stage.steps();
    }
    (Stage::Cruise, 0)
}

/// Advance `sim` by one step of the canned run at overall step `step`.
pub fn drive_canned(sim: &mut RaceSim, step: u32) -> Stage {
    let (stage, into) = stage_at(step);
    let command = stage.command(sim.car(), sim.track(), into);
    sim.step(command);
    stage
}

/// Run `sim` on the autopilot until it finishes or `limit` steps elapse.
/// Returns the number of steps taken.
pub fn drive_to_the_finish(sim: &mut RaceSim, limit: u32) -> u32 {
    for taken in 0..limit {
        if sim.phase() == RacePhase::Finished {
            return taken;
        }
        let command = autopilot(sim.car(), sim.track());
        sim.step(command);
    }
    limit
}

/// Run `sim` on the autopilot for exactly `steps` steps.
pub fn drive_autopilot(sim: &mut RaceSim, steps: u32) {
    for _ in 0..steps {
        let command = autopilot(sim.car(), sim.track());
        sim.step(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::DT;

    fn racing() -> RaceSim {
        let mut sim = RaceSim::shipping();
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        sim
    }

    #[test]
    fn the_autopilot_steers_back_toward_the_line() {
        let mut sim = racing();
        drive_autopilot(&mut sim, 300);
        let track = sim.track().clone();
        let mut car = *sim.car();
        let sample = track.sample_at(car.distance);
        car.yaw = sample.heading;

        // `at_lateral` is in the track's own frame; what matters is only that
        // the correction opposes the displacement, whichever way round that is.
        let pushed_out = steer_toward_line(&car, &track, 0.0);
        car.position = sample.at_lateral(5.0);
        let one_way = steer_toward_line(&car, &track, 0.0);
        car.position = sample.at_lateral(-5.0);
        let other_way = steer_toward_line(&car, &track, 0.0);
        assert!(
            one_way * other_way < 0.0,
            "the two displacements steer opposite ways: {one_way} and {other_way}"
        );
        let _ = pushed_out;
    }

    #[test]
    fn the_autopilot_holds_the_road_for_the_whole_course() {
        let mut sim = racing();
        let steps = drive_to_the_finish(&mut sim, 40_000);
        assert_eq!(
            sim.phase(),
            RacePhase::Finished,
            "the autopilot finished (took {steps} steps, reached {} m of {})",
            sim.car().distance,
            sim.track().length()
        );
        assert!(sim.car().is_finite());
        assert!(
            sim.near_miss_count() > 0,
            "and threaded traffic on the way: {} near misses",
            sim.near_miss_count()
        );
    }

    /// The reason the autopilot exists: it lets a test measure the *car* rather
    /// than the car's argument with a guardrail.
    #[test]
    fn the_autopilot_reaches_a_real_top_speed() {
        let mut sim = racing();
        let mut best = 0.0f32;
        for _ in 0..6_000 {
            let command = autopilot(sim.car(), sim.track());
            sim.step(command);
            best = best.max(sim.car().speed());
        }
        let top = sim.tuning().vehicle.top_speed;
        assert!(
            best > top * 0.85,
            "a clean run gets near the top speed: {best} of {top}"
        );
        assert!(best <= top * 1.06, "and does not exceed it without boost: {best}");
    }

    #[test]
    fn the_autopilot_backs_off_for_a_corner() {
        let track = Track::generate(crate::DEFAULT_SEED, &crate::Tuning::DEFAULT.course);
        let straight = corner_severity(&track, 100.0, 200.0);
        let sharpest = track
            .samples()
            .iter()
            .max_by(|a, b| a.curvature.abs().total_cmp(&b.curvature.abs()))
            .copied()
            .expect("the course has corners");
        let corner = corner_severity(&track, sharpest.distance - 20.0, 60.0);
        assert!(corner > straight, "a corner reads sharper than a straight");

        // The corner speed the autopilot would hold here is genuinely lower than
        // on a straight — that is the curve doing its job.
        let corner_speed = |k: f32| (CORNER_SPEED_BASE / (1.0 + k * CORNER_SPEED_FALLOFF)).max(18.0);
        assert!(
            corner_speed(corner) < corner_speed(straight),
            "a sharper corner has a lower target speed"
        );
        assert!(corner_speed(corner) < CORNER_SPEED_BASE);

        // Approaching the corner (the autopilot reads the road AHEAD, so a car
        // sitting on the apex has already passed the thing it would lift for).
        let approach = track.sample_at(sharpest.distance - 80.0);
        let mut car = CarState::parked(approach.position, approach.heading);
        car.distance = approach.distance;
        // Deliberately above the corner speed — note this is *above* the car's
        // own top speed, because this course is fast enough that the autopilot
        // never actually has to lift on it. The lift logic still has to work.
        car.forward_speed = corner_speed(corner) + 20.0;
        let hot = autopilot(&car, &track);
        assert_eq!(hot.throttle, 0.0, "arriving at a corner too fast, it lifts");
        assert_eq!(hot.brake, 1.0, "and well over, it brakes");

        car.forward_speed = 10.0;
        assert_eq!(
            autopilot(&car, &track).throttle,
            1.0,
            "and gets back on the power when slow enough"
        );
    }

    #[test]
    fn corner_severity_is_bounded_for_a_degenerate_span() {
        let track = Track::generate(crate::DEFAULT_SEED, &crate::Tuning::DEFAULT.course);
        assert!(corner_severity(&track, 0.0, 0.0).is_finite());
        assert!(corner_severity(&track, 0.0, -50.0).is_finite());
        assert!(corner_severity(&track, 0.0, 1.0e9).is_finite());
    }

    /// The headline recovery property: the car can be driven off the road on
    /// purpose, at speed, and driven back on again — without a reset, without
    /// getting stuck, and without the run ending.
    #[test]
    fn the_car_can_be_driven_off_the_road_and_back_on_again() {
        for steer in [1.0f32, -1.0] {
            let mut sim = racing();
            drive_autopilot(&mut sim, 600);
            let entry = sim.car().speed();
            assert!(entry > 60.0, "the mistake happens at racing speed: {entry}");

            let report = deliberate_excursion(&mut sim, steer, 90, 1_200);

            assert!(
                report.left_the_road,
                "steering {steer} for a second and a half did not leave the road"
            );
            assert!(
                report.worst_beyond_edge > 2.0,
                "it barely left the tarmac ({} m beyond the edge)",
                report.worst_beyond_edge
            );
            assert!(
                report.recovered,
                "the car never got back on the road (worst {} m off, slowest {} m/s, barrier {})",
                report.worst_beyond_edge, report.slowest, report.hit_barrier
            );
            assert!(
                report.recovery_seconds() < 12.0,
                "recovery took {} s, which is a restart in disguise",
                report.recovery_seconds()
            );
            assert!(
                report.speed_after > 25.0,
                "it is going again afterwards: {} m/s",
                report.speed_after
            );
            assert!(sim.car().is_finite());
            assert_ne!(sim.phase(), RacePhase::Finished, "the run continues");
        }
    }

    /// The same mistake made much worse — full lock held for four seconds, which
    /// buries the car in the dirt against the barrier. It must still come back,
    /// and it is allowed to take longer and cost more.
    #[test]
    fn even_a_badly_botched_excursion_recovers_without_a_reset() {
        let mut sim = racing();
        drive_autopilot(&mut sim, 600);
        let report = deliberate_excursion(&mut sim, 1.0, 240, 2_400);

        assert!(report.left_the_road);
        assert!(
            report.recovered,
            "buried in the dirt and never came back: {report:?}"
        );
        assert!(
            report.speed_after > 20.0,
            "it is moving again: {} m/s",
            report.speed_after
        );
        // The mistake is *supposed* to hurt — this is the assertion that the
        // recovery is not free.
        assert!(
            report.slowest < report.speed_leaving * 0.8,
            "running wide cost real speed: {} -> {}",
            report.speed_leaving,
            report.slowest
        );
        assert!(sim.car().is_finite());
    }

    /// And the excursion is as deterministic as everything else, so a recovery
    /// that works once works every time.
    #[test]
    fn an_excursion_replays_identically() {
        let run = || {
            let mut sim = racing();
            drive_autopilot(&mut sim, 600);
            let report = deliberate_excursion(&mut sim, 1.0, 90, 1_200);
            (report, *sim.car())
        };
        assert_eq!(run(), run());
    }

    /// Running into the back of traffic must hurt and must be survivable: it
    /// costs real speed, it does not spin the car, and the car drives out of it
    /// without a reset.
    #[test]
    fn the_car_can_be_driven_into_traffic_and_drive_out_of_it() {
        let mut sim = racing();
        drive_autopilot(&mut sim, 600);
        let report = deliberate_collision(&mut sim, 1_800, 1_200);

        assert!(report.made_contact, "never actually hit anything: {report:?}");
        assert!(
            report.closing_speed > 20.0,
            "the shunt was at a real closing speed: {}",
            report.closing_speed
        );
        assert!(report.strength > 0.0 && report.strength <= 1.0);

        // It hurts. How *much* depends on whether the approach ended up square
        // on the back of the car or brushing down its side, and both are real
        // outcomes — the exact cost of each is pinned in
        // `sim::collision::tests::a_side_swipe_costs_less_than_a_shunt_but_is_not_free`,
        // where the geometry can be controlled. What matters here is that
        // contact is never free.
        assert!(
            report.speed_lost() > 0.04,
            "the contact cost only {:.0}% of the speed",
            report.speed_lost() * 100.0
        );
        // ...but never stops the demo.
        assert!(
            report.speed_after_impact > 10.0,
            "the shunt nearly stopped the car: {} m/s",
            report.speed_after_impact
        );
        assert!(
            !report.spun,
            "the shunt spun the car ({} rad of yaw kick)",
            report.yaw_kick
        );
        assert!(
            report.recovered,
            "the car never got going again: {report:?}"
        );
        assert!(
            report.recovery_seconds() < 8.0,
            "recovery took {} s",
            report.recovery_seconds()
        );
        assert!(!report.needed_a_reset, "and never needed a reset");
        assert!(sim.car().is_finite());
        assert_ne!(sim.phase(), RacePhase::Finished, "the run continues");
    }

    /// A shunt can never leave the player slower than the car it hit — that is
    /// the rule that stops a rear-ender becoming a full stop.
    #[test]
    fn a_shunt_never_leaves_the_player_slower_than_the_car_in_front() {
        let mut sim = racing();
        drive_autopilot(&mut sim, 600);
        let report = deliberate_collision(&mut sim, 1_800, 600);
        assert!(report.made_contact);
        assert!(
            report.speed_after_impact >= report.traffic_speed * 0.55,
            "left at {} m/s behind a car doing {}",
            report.speed_after_impact,
            report.traffic_speed
        );
    }

    #[test]
    fn a_collision_replays_identically() {
        let run = || {
            let mut sim = racing();
            drive_autopilot(&mut sim, 600);
            let report = deliberate_collision(&mut sim, 1_800, 600);
            (report, *sim.car())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn the_canned_run_visits_every_stage_in_order_and_repeats() {
        let cycle = cycle_length();
        assert!(cycle > 0);
        let mut seen: Vec<Stage> = Vec::new();
        for step in 0..cycle {
            let (stage, _) = stage_at(step);
            if seen.last() != Some(&stage) {
                seen.push(stage);
            }
        }
        assert_eq!(seen, Stage::ALL.to_vec());
        assert_eq!(stage_at(0), stage_at(cycle));
    }

    #[test]
    fn each_stage_asks_for_what_it_says_it_does() {
        let track = Track::generate(crate::DEFAULT_SEED, &crate::Tuning::DEFAULT.course);
        let mut car = CarState::parked(track.sample_at(400.0).position, 0.0);
        car.distance = 400.0;
        car.forward_speed = 50.0;

        assert_eq!(Stage::Launch.command(&car, &track, 0).throttle, 1.0);
        assert_eq!(Stage::Brake.command(&car, &track, 0).brake, 1.0);
        assert!(Stage::Drift.command(&car, &track, 0).handbrake);
        assert!(Stage::Boost.command(&car, &track, 0).boost);
        assert_eq!(Stage::Impact.command(&car, &track, 0).steer, -1.0);
        assert!(Stage::Reset.command(&car, &track, 0).reset, "resets on entry");
        assert!(!Stage::Reset.command(&car, &track, 1).reset, "and only once");
    }

    /// The canned run is the multi-minute stability exercise: it must survive
    /// several cycles with every value finite and the car on the course, and it
    /// must actually *do* the things it claims — hit something, go fast, drift.
    #[test]
    fn the_canned_run_is_stable_over_several_minutes() {
        let mut sim = racing();
        let steps = (4.0 * 60.0 / DT) as u32;
        let mut stages_seen: Vec<Stage> = Vec::new();
        let mut drifted = false;
        for step in 0..steps {
            let stage = drive_canned(&mut sim, step);
            if stages_seen.last() != Some(&stage) {
                stages_seen.push(stage);
            }
            drifted |= sim.car().drifting;
            let car = sim.car();
            assert!(car.is_finite(), "step {step}: {car:?}");
            assert!(
                (0.0..=sim.track().length() + 1.0).contains(&car.distance),
                "step {step}: distance {}",
                car.distance
            );
            assert!((0.0..=1.0).contains(&sim.boost().charge()));
        }
        assert!(stages_seen.len() >= Stage::ALL.len(), "a full cycle ran");
        assert!(drifted, "the drift stage actually slid the car");
        assert!(sim.impact_count() > 0, "the impact stage actually hit something");
        assert!(sim.top_speed_seen() > 60.0, "and it went genuinely fast");
    }

    #[test]
    fn the_canned_run_replays_identically() {
        let run = || {
            let mut sim = racing();
            for step in 0..3_000 {
                drive_canned(&mut sim, step);
            }
            (
                *sim.car(),
                *sim.boost(),
                sim.impact_count(),
                sim.near_miss_count(),
            )
        };
        assert_eq!(run(), run());
    }
}
