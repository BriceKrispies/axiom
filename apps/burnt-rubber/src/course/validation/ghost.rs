//! **Ghost validation**: drive the course with the real agent and measure what
//! happened.
//!
//! Everything else in this module is analysis — a grid, a budget, an estimate.
//! This is the second stage, and it is a *measurement*: the course is compiled,
//! the app's own `axiom-agent` driver ([`crate::agent`]) is put on it, and the
//! run is watched. The agent is exactly the driver the player races as a ghost
//! in the live game, so a course that the ghost cannot get round is a course
//! nobody should be asked to.
//!
//! # What it must not become
//!
//! This never runs during play, and it never touches the player's world. It is
//! a *generation-time* tool: a compiled plan in, a report out. Nothing here can
//! reach into a running race, and in particular **the live game never quietly
//! alters traffic to help the player** — the same plan the ghost was validated
//! against is the plan the player gets.

use crate::agent::{self, DriverTuning};
use crate::course::runtime::CoursePlan;
use crate::course::specification::{EncounterId, SectionId};
use crate::sim::{RaceEvent, RacePhase, RaceSim};
use crate::tuning::{Tuning, DT};
use crate::PlayProfile;

use std::sync::Arc;

use super::report::BoostStatus;

/// What one ghost run measured.
#[derive(Debug, Clone, PartialEq)]
pub struct GhostRunReport {
    /// Whether the ghost crossed the line.
    pub completed: bool,
    /// Race time (s) — a step count, never a clock reading.
    pub elapsed_seconds: f32,
    /// Fixed steps taken.
    pub steps: u32,
    /// How far it got (m).
    pub distance_m: f32,
    /// Things it hit.
    pub collisions: u32,
    /// Traffic it threaded.
    pub near_misses: u32,
    /// Steps spent spending boost.
    pub boost_steps: u32,
    /// The longest unbroken run of boosting steps.
    pub longest_boost_steps: u32,
    /// Sections in which the meter fell below the charge a boost can start on.
    pub boost_lost_sections: Vec<SectionId>,
    /// The tightest lateral gap it ever took past a traffic car (m).
    pub minimum_clearance_m: f32,
    /// Mean ground speed (m/s).
    pub average_speed_mps: f32,
    /// Encounters it collided inside.
    pub encounter_failures: Vec<EncounterId>,
}

impl GhostRunReport {
    /// The fraction of the run spent on boost — what the boost-sustain analysis
    /// folds in.
    pub fn boost_fraction(&self) -> f32 {
        self.boost_steps as f32 / self.steps.max(1) as f32
    }

    /// The stable one-line summary the overlay and the dump show.
    pub fn summary(&self) -> String {
        format!(
            "{} in {:.2}s — {} near misses, {} collisions, boost {:.0}% (longest {:.1}s), \
             min clearance {:.2} m, mean {:.0} km/h{}",
            self.completed
                .then_some("finished")
                .unwrap_or("did not finish"),
            self.elapsed_seconds,
            self.near_misses,
            self.collisions,
            self.boost_fraction() * 100.0,
            self.longest_boost_steps as f32 * DT,
            self.minimum_clearance_m,
            self.average_speed_mps * 3.6,
            (!self.encounter_failures.is_empty())
                .then(|| format!(
                    ", failed {} encounters",
                    self.encounter_failures.len()
                ))
                .unwrap_or_default(),
        )
    }

    /// Fold this run into a boost-sustain verdict.
    pub fn fold_into(
        &self,
        estimate: BoostStatus,
        thresholds: &crate::course::specification::ValidationThresholds,
    ) -> BoostStatus {
        let measured = super::boost::fold_ghost(estimate, self.boost_fraction(), thresholds);
        // A ghost that could not finish, or that had to bulldoze its way round,
        // is evidence about the course rather than about the driver.
        (!self.completed | !self.encounter_failures.is_empty())
            .then_some(measured.worst(BoostStatus::Starved))
            .unwrap_or(measured)
    }
}

/// Drive `plan` with the agent until it finishes or `limit_steps` elapse.
pub fn run(
    plan: Arc<CoursePlan>,
    tuning: Tuning,
    profile: PlayProfile,
    limit_steps: u32,
) -> GhostRunReport {
    let driver = DriverTuning::for_profile(profile);
    let mut sim = RaceSim::from_plan(plan.clone(), tuning, profile);
    let mut collisions = 0u32;
    let mut boost_steps = 0u32;
    let mut boost_run = 0u32;
    let mut longest_boost = 0u32;
    let mut speed_sum = 0f64;
    let mut steps = 0u32;
    let mut minimum_clearance = f32::INFINITY;
    let mut lost: Vec<SectionId> = Vec::new();
    let mut failures: Vec<EncounterId> = Vec::new();
    let mut was_charged = true;

    while (sim.phase() != RacePhase::Finished) & (steps < limit_steps) {
        let (command, _) = agent::drive_one_step(&sim, &driver, u64::from(steps));
        sim.step(command);
        steps += 1;
        speed_sum += f64::from(sim.car().speed());

        let boosting = sim.boost().active();
        boost_steps += u32::from(boosting);
        boost_run = boosting.then_some(boost_run + 1).unwrap_or(0);
        longest_boost = longest_boost.max(boost_run);

        // Where the meter fell below the charge a fresh boost needs — the
        // sections a route runs dry in.
        let charged = sim.boost().charge() >= tuning.race.boost_min_to_start;
        let section = plan.section_at(sim.car().distance);
        (was_charged & !charged & !lost.contains(&section.id)).then(|| {
            lost.push(section.id.clone());
        });
        was_charged = charged;

        minimum_clearance = minimum_clearance.min(clearance(&sim, &tuning));

        let inside = plan.encounter_at(sim.car().distance).map(|e| e.id);
        sim.events().iter().for_each(|event| {
            matches!(event, RaceEvent::Impact { fresh: true, .. }).then(|| collisions += 1);
            matches!(event, RaceEvent::Impact { fresh: true, traffic: true, .. })
                .then(|| {
                    inside
                        .filter(|id| !failures.contains(id))
                        .map(|id| failures.push(id));
                });
        });
    }

    GhostRunReport {
        completed: sim.phase() == RacePhase::Finished,
        elapsed_seconds: sim.elapsed_seconds(),
        steps,
        distance_m: sim.car().distance,
        collisions,
        near_misses: sim.near_miss_count(),
        boost_steps,
        longest_boost_steps: longest_boost,
        boost_lost_sections: lost,
        minimum_clearance_m: minimum_clearance,
        average_speed_mps: (speed_sum / f64::from(steps.max(1))) as f32,
        encounter_failures: failures,
    }
}

/// How much lateral room the car has past the nearest traffic it is abreast of
/// (m), or infinity where it is abreast of nothing.
fn clearance(sim: &RaceSim, tuning: &Tuning) -> f32 {
    let car = sim.car();
    let along = tuning.vehicle.half_length + tuning.race.traffic_half_length;
    let across = tuning.vehicle.half_width + tuning.race.traffic_half_width;
    sim.traffic()
        .active()
        .filter(|other| (other.distance - car.distance).abs() < along * 1.5)
        .map(|other| (other.lateral - car.lateral).abs() - across)
        .fold(f32::INFINITY, f32::min)
}

/// How many steps a validation run is allowed before it is called a failure.
///
/// Three minutes of race at 60 Hz — comfortably past the ghost's ninety-second
/// pace, and a hard bound so a course that traps the driver ends the run rather
/// than the test.
pub const VALIDATION_STEP_LIMIT: u32 = 60 * 60 * 3;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{
        CourseItem, CourseSpec, RoadPrimitiveSpec, SectionSpec, TrafficFlowSpec, TrafficZoneSpec,
    };
    use crate::course::{compiler, procedural};

    fn shipping() -> Arc<CoursePlan> {
        Arc::new(procedural::shipping_plan(crate::DEFAULT_SEED).expect("compiles"))
    }

    #[test]
    fn the_ghost_completes_the_shipping_course_and_reports_its_run() {
        let report = run(
            shipping(),
            Tuning::DEFAULT,
            PlayProfile::Wheel,
            VALIDATION_STEP_LIMIT,
        );
        assert!(report.completed, "the ghost got {:.0} m", report.distance_m);
        assert!(report.near_misses > 20, "{} near misses", report.near_misses);
        assert!(report.average_speed_mps > 40.0);
        assert!(report.boost_steps > 0, "the ghost never used boost");
        assert!(report.longest_boost_steps > 0);
        assert!(report.boost_fraction() > 0.0);
        assert!(report.minimum_clearance_m.is_finite(), "it passed something");
        assert!(!report.summary().is_empty());
    }

    /// Repeated runs must produce identical numbers, or the metric is not a
    /// measurement.
    #[test]
    fn repeated_runs_produce_identical_metrics() {
        let plan = shipping();
        let a = run(plan.clone(), Tuning::DEFAULT, PlayProfile::Wheel, 4_000);
        let b = run(plan, Tuning::DEFAULT, PlayProfile::Wheel, 4_000);
        assert_eq!(a, b);
    }

    #[test]
    fn a_blocked_fixture_stops_the_ghost() {
        // A short course whose traffic is so dense the road cannot be threaded.
        let mut spec = CourseSpec::new("wall", 3);
        spec.items.push(CourseItem::Section(
            SectionSpec::new(
                crate::course::specification::SectionId::new("run"),
                RoadPrimitiveSpec::Straight { length_m: 2_500.0 },
            )
            .with_lanes(3)
            .with_traffic(TrafficZoneSpec {
                flow: Some(TrafficFlowSpec {
                    min_headway_m: 12.0,
                    preferred_headway_m: 12.0,
                    max_headway_m: 14.0,
                    speed_mps: crate::course::specification::ScalarRange::exact(14.0),
                    ..TrafficFlowSpec::at_density(80.0)
                }),
                ..TrafficZoneSpec::default()
            }),
        ));
        let plan = Arc::new(compiler::compile(&spec, &Tuning::DEFAULT).expect("compiles"));
        assert!(
            plan.report().has_errors(),
            "a wall of traffic should not validate:\n{}",
            plan.report().dump()
        );
        let report = run(plan, Tuning::DEFAULT, PlayProfile::Wheel, 3_600);
        assert!(
            report.collisions > 0,
            "the ghost got through a wall of traffic untouched"
        );
    }

    #[test]
    fn a_clear_fixture_is_completed_cleanly() {
        let mut spec = CourseSpec::new("clear", 3);
        spec.items.push(CourseItem::Section(
            SectionSpec::new(
                crate::course::specification::SectionId::new("run"),
                RoadPrimitiveSpec::Straight { length_m: 1_500.0 },
            )
            .with_lanes(5),
        ));
        let plan = Arc::new(compiler::compile(&spec, &Tuning::DEFAULT).expect("compiles"));
        let report = run(plan, Tuning::DEFAULT, PlayProfile::Wheel, 6_000);
        assert!(report.completed, "the ghost got {:.0} m", report.distance_m);
        assert_eq!(report.collisions, 0, "on an empty road");
        assert!(report.encounter_failures.is_empty());
        assert_eq!(report.minimum_clearance_m, f32::INFINITY, "nothing to pass");
    }

    #[test]
    fn the_boost_fold_demotes_a_run_that_could_not_finish() {
        let thresholds = crate::course::specification::ValidationThresholds::DEFAULT;
        let good = GhostRunReport {
            completed: true,
            elapsed_seconds: 90.0,
            steps: 5_400,
            distance_m: 9_000.0,
            collisions: 1,
            near_misses: 80,
            boost_steps: 3_000,
            longest_boost_steps: 400,
            boost_lost_sections: Vec::new(),
            minimum_clearance_m: 0.4,
            average_speed_mps: 80.0,
            encounter_failures: Vec::new(),
        };
        assert!((good.boost_fraction() - 3_000.0 / 5_400.0).abs() < 1.0e-5);
        assert_eq!(
            good.fold_into(BoostStatus::Excellent, &thresholds),
            BoostStatus::Excellent
        );

        let stranded = GhostRunReport {
            completed: false,
            ..good.clone()
        };
        assert_eq!(
            stranded.fold_into(BoostStatus::Excellent, &thresholds),
            BoostStatus::Starved
        );

        let crashed = GhostRunReport {
            encounter_failures: vec![EncounterId(0)],
            ..good.clone()
        };
        assert_eq!(
            crashed.fold_into(BoostStatus::Excellent, &thresholds),
            BoostStatus::Starved
        );

        let dry = GhostRunReport {
            boost_steps: 10,
            ..good
        };
        assert_eq!(
            dry.fold_into(BoostStatus::Excellent, &thresholds),
            BoostStatus::Starved
        );

        let nothing = GhostRunReport {
            steps: 0,
            boost_steps: 0,
            ..dry
        };
        assert_eq!(nothing.boost_fraction(), 0.0);
    }
}
