//! **Validation**: everything the compiler can be told about a course before
//! anybody drives it.
//!
//! The pass is deterministic and total — it does not stop at the first problem,
//! because a validator that reports one failure per run makes an author fix a
//! course one mistake at a time. It runs every check over the whole compiled
//! course, sorts the findings into a fixed order, and hands back a
//! [`ValidationReport`] carrying errors, warnings **and measurements**.
//!
//! What it checks, in order:
//!
//! 1. the compiled geometry is continuous, and reports where a clamp changed it;
//! 2. every lane width and lane count is real;
//! 3. no vehicle starts off the road, or overlapped with another;
//! 4. every encounter respects its own minimum headway, reaction time and
//!    lateral clearance, and names lanes that exist;
//! 5. every near-miss window refers to real vehicles and a gap the road can
//!    actually offer;
//! 6. a traversable corridor exists wherever one is required
//!    ([`traversal`]);
//! 7. each section's boost economy is classified ([`boost`]).

pub mod boost;
pub mod ghost;
pub mod report;
pub mod traversal;

use crate::course::error::{CourseError, CourseErrorCode};
use crate::course::geometry::{CompiledSection, GeometryClamps};
use crate::course::specification::ValidationThresholds;
use crate::course::traffic::{CompiledEncounter, NearMissWindow, TrafficPlan};
use crate::track::Track;
use crate::tuning::{RaceTuning, VehicleTuning};

pub use report::{
    BoostStatus, CourseMetrics, Finding, SectionVerdict, Severity, ValidationReport,
};
pub use traversal::{OccupancyModel, TraversalGrid};

/// Everything one validation pass looks at.
#[derive(Debug, Clone, Copy)]
pub struct ValidationInput<'a> {
    /// The compiled road.
    pub track: &'a Track,
    /// The compiled sections.
    pub sections: &'a [CompiledSection],
    /// Per-section clamp counts from geometry compilation.
    pub clamps: &'a [GeometryClamps],
    /// The compiled vehicles, in ascending spawn order.
    pub plans: &'a [TrafficPlan],
    /// The compiled encounters.
    pub encounters: &'a [CompiledEncounter],
    /// The compiled near-miss opportunity windows.
    pub windows: &'a [NearMissWindow],
    /// What the course is judged against.
    pub thresholds: &'a ValidationThresholds,
    /// The car's collision box.
    pub vehicle: &'a VehicleTuning,
    /// The traffic rules and the boost economy.
    pub race: &'a RaceTuning,
}

/// Run every check and produce the report.
pub fn validate(input: ValidationInput<'_>) -> ValidationReport {
    let mut findings: Vec<Finding> = Vec::new();
    check_geometry(&input, &mut findings);
    check_lanes(&input, &mut findings);
    check_traffic_placement(&input, &mut findings);
    check_encounters(&input, &mut findings);
    check_windows(&input, &mut findings);

    let model = OccupancyModel::resolve(input.vehicle, input.race, input.thresholds);
    let grid = traversal::analyse(input.track, input.plans, input.thresholds, &model);
    check_traversal(&input, &grid, &mut findings);

    let sections: Vec<SectionVerdict> = input
        .sections
        .iter()
        .map(|section| {
            let corridor = grid.narrowest_corridor(section.start_m, section.end_m);
            boost::classify(
                section,
                input.windows,
                corridor > 0,
                corridor,
                input.thresholds,
                input.race,
            )
        })
        .collect();

    let status = boost::classify_course(&sections, input.thresholds);

    let mut report = ValidationReport {
        findings,
        sections,
        status,
        metrics: CourseMetrics {
            length_m: input.track.length(),
            samples: input.track.samples().len(),
            sections: input.sections.len(),
            vehicles: input.plans.len(),
            encounters: input.encounters.len(),
            near_miss_windows: input.windows.len(),
            traversal_cells: grid.cells(),
            blocked_cells: grid.blocked_cells(),
            tightest_corridor_m: grid.tightest_corridor_m,
            vehicles_per_km: input.plans.len() as f32
                / (input.track.length() / 1_000.0).max(1.0e-3),
        },
    };
    report.sort();
    report
}

/// The road is continuous, and every clamp that changed it is reported.
fn check_geometry(input: &ValidationInput<'_>, findings: &mut Vec<Finding>) {
    let spacing = input.track.spacing();
    let thresholds = input.thresholds;
    input.track.samples().windows(2).for_each(|pair| {
        let (a, b) = (pair[0], pair[1]);
        let step = b.position.distance(a.position);
        let discontinuous = ((step - spacing).abs() > spacing * 0.05)
            | (a.tangent.dot(b.tangent) < TANGENT_CONTINUITY)
            | ((b.curvature - a.curvature).abs() > thresholds.max_curvature_step * 1.5)
            | ((b.grade - a.grade).abs() > thresholds.max_grade_step * 1.5)
            | ((b.bank - a.bank).abs() > thresholds.max_bank_step * 1.5);
        discontinuous.then(|| {
            findings.push(Finding::error(
                a.distance,
                CourseError::new(
                    CourseErrorCode::NonContinuousCourse,
                    format!(
                        "the road is discontinuous at {:.0} m: position stepped {step:.3} m \
                         (spacing {spacing:.1} m), curvature by {:.5}, grade by {:.4}, bank by \
                         {:.4}",
                        a.distance,
                        b.curvature - a.curvature,
                        b.grade - a.grade,
                        b.bank - a.bank
                    ),
                ),
            ));
        });
        let bad = !(a.position.x.is_finite()
            & a.position.y.is_finite()
            & a.position.z.is_finite()
            & a.half_width.is_finite());
        bad.then(|| {
            findings.push(Finding::error(
                a.distance,
                CourseError::new(
                    CourseErrorCode::InvalidFiniteScalar,
                    format!("a non-finite sample at {:.0} m", a.distance),
                ),
            ));
        });
    });

    input
        .sections
        .iter()
        .zip(input.clamps.iter())
        .for_each(|(section, clamps)| {
            clamps.any().then(|| {
                findings.push(Finding::warning(
                    section.start_m,
                    CourseError::new(
                        CourseErrorCode::NonContinuousCourse,
                        format!(
                            "the compiled road is not the authored road: {} curvature, \
                             {} grade, {} bank and {} width samples were clamped to the \
                             course limits",
                            clamps.curvature, clamps.grade, clamps.bank, clamps.width
                        ),
                    )
                    .in_section(section.id.as_str()),
                ));
            });
        });
}

/// How aligned two adjacent tangents must be. `cos(2.6°)`: a road that turns
/// harder than that in two metres is a kink, not a corner.
const TANGENT_CONTINUITY: f32 = 0.999;

/// Lane widths and counts are real everywhere.
fn check_lanes(input: &ValidationInput<'_>, findings: &mut Vec<Finding>) {
    (input.track.lane_width() > 0.0).then_some(()).unwrap_or_else(|| {
        findings.push(Finding::error(
            0.0,
            CourseError::new(
                CourseErrorCode::InvalidLaneWidth,
                format!("the course lane width is {}", input.track.lane_width()),
            ),
        ));
    });
    input.sections.iter().for_each(|section| {
        let sample = input.track.sample_at(section.start_m + section.length_m() * 0.5);
        let lanes = input.track.lane_count(&sample);
        ((lanes >= crate::track::MIN_LANES) & (lanes % 2 == 1))
            .then_some(())
            .unwrap_or_else(|| {
                findings.push(Finding::error(
                    section.start_m,
                    CourseError::new(
                        CourseErrorCode::InvalidLaneCount,
                        format!("the compiled road carries {lanes} lanes here"),
                    )
                    .in_section(section.id.as_str()),
                ));
            });
        (sample.half_width > 0.0).then_some(()).unwrap_or_else(|| {
            findings.push(Finding::error(
                section.start_m,
                CourseError::new(
                    CourseErrorCode::InvalidRoadWidth,
                    format!("the tarmac is {} m wide here", sample.half_width),
                )
                .in_section(section.id.as_str()),
            ));
        });
    });
}

/// No vehicle starts off the road, or on top of another.
fn check_traffic_placement(input: &ValidationInput<'_>, findings: &mut Vec<Finding>) {
    // Two traffic cars have to clear **each other**, which is two traffic
    // half-widths — not a traffic car and the player. Measuring the wrong pair
    // flags a zipper's own row, where two blockers legitimately sit abreast in
    // adjacent lanes.
    let along = input.race.traffic_half_length * 2.0;
    let lateral = input.race.traffic_half_width * 2.0;
    input.plans.iter().for_each(|plan| {
        let sample = input.track.sample_at(plan.spawn_m);
        let reach = input.track.lane_reach(&sample);
        (plan.lane.abs() <= reach).then_some(()).unwrap_or_else(|| {
            findings.push(Finding::error(
                plan.spawn_m,
                CourseError::new(
                    CourseErrorCode::InvalidEncounterLane,
                    format!(
                        "vehicle {} spawns in lane {} on a road that reaches {reach} lanes",
                        plan.id, plan.lane
                    ),
                ),
            ));
        });
        (plan.speed_mps > 0.0)
            .then_some(())
            .unwrap_or_else(|| {
                findings.push(Finding::error(
                    plan.spawn_m,
                    CourseError::new(
                        CourseErrorCode::InvalidSpeedRange,
                        format!("vehicle {} has a speed of {}", plan.id, plan.speed_mps),
                    ),
                ));
            });
    });

    // Overlap at spawn: plans are in ascending spawn order, so a bounded forward
    // scan sees every pair that could possibly touch.
    input.plans.iter().enumerate().for_each(|(i, plan)| {
        input.plans[i + 1..]
            .iter()
            .take_while(|other| other.spawn_m - plan.spawn_m < along)
            .for_each(|other| {
                let sample = input.track.sample_at(plan.spawn_m);
                let gap = (input.track.lane_lateral(&sample, plan.lane)
                    - input.track.lane_lateral(&sample, other.lane))
                .abs();
                (gap < lateral).then(|| {
                    findings.push(Finding::error(
                        plan.spawn_m,
                        CourseError::new(
                            CourseErrorCode::UntraversableEncounter,
                            format!(
                                "vehicles {} and {} start overlapped: {:.1} m apart along the \
                                 course and {gap:.1} m across it",
                                plan.id,
                                other.id,
                                other.spawn_m - plan.spawn_m
                            ),
                        ),
                    ));
                });
            });
    });
}

/// Every encounter respects what it demands of the player.
fn check_encounters(input: &ValidationInput<'_>, findings: &mut Vec<Finding>) {
    let along = input.vehicle.half_length + input.race.traffic_half_length;
    input.encounters.iter().for_each(|encounter| {
        let expected = input
            .track
            .sample_at(encounter.start_m)
            .expected_speed
            .max(1.0);
        let vehicles: Vec<&TrafficPlan> = input
            .plans
            .iter()
            .filter(|p| p.encounter == Some(encounter.id))
            .collect();
        let speed = vehicles.first().map(|p| p.speed_mps).unwrap_or(0.0);
        let closing = (expected - speed).max(0.1);

        // Row spacing: the along-course gap between the distinct distances the
        // encounter placed vehicles at.
        let mut rows: Vec<f32> = vehicles.iter().map(|p| p.spawn_m).collect();
        rows.sort_by(f32::total_cmp);
        rows.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        let tightest = rows
            .windows(2)
            .map(|w| w[1] - w[0])
            .fold(f32::INFINITY, f32::min);

        (tightest >= along * 2.0)
            .then_some(())
            .unwrap_or_else(|| {
                findings.push(Finding::error(
                    encounter.start_m,
                    CourseError::new(
                        CourseErrorCode::InvalidHeadwayRange,
                        format!(
                            "the {} encounter spaces its rows {tightest:.1} m apart, closer \
                             than the {:.1} m two cars need to be clear of each other",
                            encounter.kind,
                            along * 2.0
                        ),
                    ),
                ));
            });

        // Reaction time: how long the player has between one row and the next.
        let available = tightest.is_finite().then_some(tightest / closing).unwrap_or(
            (encounter.end_m - encounter.start_m) / closing,
        );
        (available >= encounter.minimum_reaction_time_s.min(input.thresholds.min_reaction_time_s))
            .then_some(())
            .unwrap_or_else(|| {
                findings.push(Finding::error(
                    encounter.start_m,
                    CourseError::new(
                        CourseErrorCode::ImpossibleReactionTime,
                        format!(
                            "the {} encounter gives {available:.2} s between rows at a \
                             {closing:.0} m/s closing speed, under the {:.2} s it asks for",
                            encounter.kind, encounter.minimum_reaction_time_s
                        ),
                    ),
                ));
            });

        // Lateral clearance: the gap it wants has to fit on the road it is on.
        let sample = input.track.sample_at(encounter.start_m);
        let room = sample.half_width - input.vehicle.half_width - input.race.traffic_half_width;
        (encounter.lateral_clearance_m <= room)
            .then_some(())
            .unwrap_or_else(|| {
                findings.push(Finding::error(
                    encounter.start_m,
                    CourseError::new(
                        CourseErrorCode::ImpossibleLateralClearance,
                        format!(
                            "the {} encounter asks for {:.2} m of clearance where the road \
                             offers {room:.2} m",
                            encounter.kind, encounter.lateral_clearance_m
                        ),
                    ),
                ));
            });

        // Lanes it named have to exist where it placed them.
        vehicles.iter().for_each(|plan| {
            let reach = input
                .track
                .lane_reach(&input.track.sample_at(plan.spawn_m));
            (plan.lane.abs() <= reach).then_some(()).unwrap_or_else(|| {
                findings.push(Finding::error(
                    plan.spawn_m,
                    CourseError::new(
                        CourseErrorCode::InvalidEncounterLane,
                        format!(
                            "the {} encounter placed a vehicle in lane {} where the road \
                             reaches {reach}",
                            encounter.kind, plan.lane
                        ),
                    ),
                ));
            });
        });
    });
}

/// Every near-miss window is geometrically possible and refers to real cars.
fn check_windows(input: &ValidationInput<'_>, findings: &mut Vec<Finding>) {
    let room_needed = input.vehicle.half_width + input.race.traffic_half_width;
    input.windows.iter().for_each(|window| {
        let missing = window
            .vehicles
            .iter()
            .filter(|id| !input.plans.iter().any(|p| p.id == **id))
            .count();
        (missing == 0).then_some(()).unwrap_or_else(|| {
            findings.push(Finding::error(
                window.start_m,
                CourseError::new(
                    CourseErrorCode::UntraversableEncounter,
                    format!("a near-miss window refers to {missing} vehicles that do not exist"),
                ),
            ));
        });
        (!window.vehicles.is_empty())
            .then_some(())
            .unwrap_or_else(|| {
                findings.push(Finding::error(
                    window.start_m,
                    CourseError::new(
                        CourseErrorCode::UntraversableEncounter,
                        "a near-miss window offers no vehicle to pass".to_string(),
                    ),
                ));
            });
        let sample = input.track.sample_at(window.start_m);
        let room = sample.half_width - room_needed;
        (window.clearance_m.lo <= room)
            .then_some(())
            .unwrap_or_else(|| {
                findings.push(Finding::error(
                    window.start_m,
                    CourseError::new(
                        CourseErrorCode::ImpossibleLateralClearance,
                        format!(
                            "a near-miss window wants at least {:.2} m of clearance where the \
                             road offers {room:.2} m",
                            window.clearance_m.lo
                        ),
                    ),
                ));
            });
    });
}

/// A route exists wherever one is required.
fn check_traversal(
    input: &ValidationInput<'_>,
    grid: &TraversalGrid,
    findings: &mut Vec<Finding>,
) {
    (grid.max_lane_shift >= 1).then_some(()).unwrap_or_else(|| {
        findings.push(Finding::error(
            0.0,
            CourseError::new(
                CourseErrorCode::ImpossibleLateralClearance,
                format!(
                    "a {:.0} m traversal step cannot contain a lane change at {:.0} m/s of \
                     lateral speed — the grid cannot express a route at all",
                    grid.step_m, input.thresholds.lateral_speed_mps
                ),
            )
            .in_field("traversal_step_m"),
        ));
    });

    grid.blockages.iter().for_each(|distance| {
        findings.push(Finding::error(
            *distance,
            CourseError::new(
                CourseErrorCode::UntraversableEncounter,
                format!("no lane is reachable at {distance:.0} m — the traffic forms a wall"),
            ),
        ));
    });

    input
        .encounters
        .iter()
        .filter(|e| e.requires_route)
        .for_each(|encounter| {
            grid.is_traversable(encounter.start_m, encounter.end_m)
                .then_some(())
                .unwrap_or_else(|| {
                    findings.push(Finding::error(
                        encounter.start_m,
                        CourseError::new(
                            CourseErrorCode::UntraversableEncounter,
                            format!(
                                "the {} encounter requires a continuous route and does not \
                                 leave one",
                                encounter.kind
                            ),
                        ),
                    ));
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{
        EncounterId, PassingSide, ScalarRange, SectionId, SectionKind, VehicleArchetype, VehicleId,
    };
    use crate::course::traffic::PLAN_LIFETIME_M;
    use crate::track::MAX_LANE_REACH;

    fn track() -> Track {
        crate::course::procedural::shipping_plan(crate::DEFAULT_SEED)
            .expect("the shipping course compiles")
            .track()
            .clone()
    }

    fn sections(track: &Track) -> (Vec<CompiledSection>, Vec<GeometryClamps>) {
        (
            vec![CompiledSection {
                id: SectionId::new("whole"),
                index: 0,
                start_m: 0.0,
                end_m: track.length(),
                environment: SectionKind::StartStraight,
                expected_speed_mps: 80.0,
                lanes: 5,
                primitive: "straight",
            }],
            vec![GeometryClamps::default()],
        )
    }

    fn blocker(id: u32, at_m: f32, lane: i32) -> TrafficPlan {
        TrafficPlan {
            id: VehicleId(id),
            spawn_m: at_m,
            despawn_m: at_m + PLAN_LIFETIME_M,
            lane,
            speed_mps: 0.01,
            archetype: VehicleArchetype::Saloon,
            lane_changes: Vec::new(),
            speed_changes: Vec::new(),
            encounter: None,
            section: 0,
            variation_seed: 1,
        }
    }

    fn run<'a>(
        track: &'a Track,
        sections: &'a [CompiledSection],
        clamps: &'a [GeometryClamps],
        plans: &'a [TrafficPlan],
        encounters: &'a [CompiledEncounter],
        windows: &'a [NearMissWindow],
    ) -> ValidationReport {
        validate(ValidationInput {
            track,
            sections,
            clamps,
            plans,
            encounters,
            windows,
            thresholds: &ValidationThresholds::DEFAULT,
            vehicle: &VehicleTuning::DEFAULT,
            race: &RaceTuning::DEFAULT,
        })
    }

    #[test]
    fn an_open_road_validates_cleanly_and_measures_itself() {
        let track = track();
        let (sections, clamps) = sections(&track);
        let report = run(&track, &sections, &clamps, &[], &[], &[]);
        assert!(
            !report.has_errors(),
            "clean road reported: {}",
            report.dump()
        );
        assert_eq!(report.metrics.samples, track.samples().len());
        assert!(report.metrics.traversal_cells > 0);
        assert!(report.metrics.length_m > 8_000.0);
        // With no traffic there is nothing to earn, so the economy is starved —
        // which is exactly what an empty course should be told.
        assert_eq!(report.status, BoostStatus::Starved);
    }

    #[test]
    fn a_fully_blocked_road_fails_validation() {
        let track = track();
        let (sections, clamps) = sections(&track);
        let plans: Vec<TrafficPlan> = (-MAX_LANE_REACH..=MAX_LANE_REACH)
            .enumerate()
            .map(|(i, lane)| blocker(i as u32, 3_000.0, lane))
            .collect();
        let report = run(&track, &sections, &clamps, &plans, &[], &[]);
        assert!(report.has_errors());
        assert!(report
            .errors()
            .any(|f| f.error.code == CourseErrorCode::UntraversableEncounter));
        assert_eq!(report.status, BoostStatus::Invalid);
    }

    #[test]
    fn overlapped_traffic_is_rejected() {
        let track = track();
        let (sections, clamps) = sections(&track);
        let plans = vec![blocker(0, 3_000.0, 0), blocker(1, 3_000.5, 0)];
        let report = run(&track, &sections, &clamps, &plans, &[], &[]);
        assert!(report
            .errors()
            .any(|f| f.error.message.contains("start overlapped")));
    }

    #[test]
    fn traffic_placed_off_the_road_is_rejected() {
        let track = track();
        let (sections, clamps) = sections(&track);
        // The narrowest stretch cannot carry the outermost lane.
        let narrow = track
            .samples()
            .iter()
            .find(|s| track.lane_reach(s) == 1)
            .map(|s| s.distance)
            .expect("the course narrows somewhere");
        let plans = vec![blocker(0, narrow, MAX_LANE_REACH)];
        let report = run(&track, &sections, &clamps, &plans, &[], &[]);
        assert!(report
            .errors()
            .any(|f| f.error.code == CourseErrorCode::InvalidEncounterLane));
    }

    #[test]
    fn a_stationary_vehicle_is_rejected() {
        let track = track();
        let (sections, clamps) = sections(&track);
        let plans = vec![TrafficPlan {
            speed_mps: 0.0,
            ..blocker(0, 3_000.0, 0)
        }];
        let report = run(&track, &sections, &clamps, &plans, &[], &[]);
        assert!(report
            .errors()
            .any(|f| f.error.code == CourseErrorCode::InvalidSpeedRange));
    }

    #[test]
    fn an_encounter_that_crowds_its_rows_or_demands_the_impossible_is_rejected() {
        let track = track();
        let (sections, clamps) = sections(&track);
        let plans = vec![blocker(0, 3_000.0, 0), blocker(1, 3_002.0, 1)];
        let crowded = CompiledEncounter {
            id: EncounterId(0),
            kind: "zipper",
            section: 0,
            start_m: 3_000.0,
            end_m: 3_100.0,
            vehicles: vec![VehicleId(0), VehicleId(1)],
            requires_route: false,
            minimum_reaction_time_s: 0.75,
            lateral_clearance_m: 0.5,
            target_near_misses: 2,
        };
        let plans: Vec<TrafficPlan> = plans
            .into_iter()
            .map(|p| TrafficPlan {
                encounter: Some(EncounterId(0)),
                ..p
            })
            .collect();
        let report = run(&track, &sections, &clamps, &plans, &[crowded.clone()], &[]);
        assert!(report
            .errors()
            .any(|f| f.error.code == CourseErrorCode::InvalidHeadwayRange));

        // A clearance the road cannot offer.
        let greedy = CompiledEncounter {
            lateral_clearance_m: 40.0,
            ..crowded
        };
        let report = run(&track, &sections, &clamps, &plans, &[greedy], &[]);
        assert!(report
            .errors()
            .any(|f| f.error.code == CourseErrorCode::ImpossibleLateralClearance));
    }

    #[test]
    fn an_encounter_that_requires_a_route_and_leaves_none_is_rejected() {
        let track = track();
        let (sections, clamps) = sections(&track);
        let plans: Vec<TrafficPlan> = (-MAX_LANE_REACH..=MAX_LANE_REACH)
            .enumerate()
            .map(|(i, lane)| TrafficPlan {
                encounter: Some(EncounterId(0)),
                ..blocker(i as u32, 3_000.0, lane)
            })
            .collect();
        let encounter = CompiledEncounter {
            id: EncounterId(0),
            kind: "rolling_wall",
            section: 0,
            start_m: 2_950.0,
            end_m: 3_050.0,
            vehicles: plans.iter().map(|p| p.id).collect(),
            requires_route: true,
            minimum_reaction_time_s: 0.2,
            lateral_clearance_m: 0.5,
            target_near_misses: 2,
        };
        let report = run(&track, &sections, &clamps, &plans, &[encounter], &[]);
        assert!(report.errors().any(|f| f
            .error
            .message
            .contains("requires a continuous route")));
    }

    #[test]
    fn a_near_miss_window_pointing_at_nothing_is_rejected() {
        let track = track();
        let (sections, clamps) = sections(&track);
        let phantom = NearMissWindow {
            encounter: None,
            start_m: 1_000.0,
            end_m: 1_100.0,
            vehicles: vec![VehicleId(99)],
            clearance_m: ScalarRange::new(0.4, 1.4),
            side: PassingSide::Either,
            minimum_relative_speed_mps: 8.0,
            intended_opportunities: 1,
            difficulty_weight: 1.0,
            section: 0,
        };
        let report = run(&track, &sections, &clamps, &[], &[], &[phantom.clone()]);
        assert!(report
            .errors()
            .any(|f| f.error.message.contains("do not exist")));

        let empty = NearMissWindow {
            vehicles: Vec::new(),
            ..phantom.clone()
        };
        let report = run(&track, &sections, &clamps, &[], &[], &[empty]);
        assert!(report
            .errors()
            .any(|f| f.error.message.contains("no vehicle to pass")));

        // A gap wider than the road.
        let impossible = NearMissWindow {
            vehicles: vec![VehicleId(0)],
            clearance_m: ScalarRange::new(40.0, 50.0),
            ..phantom
        };
        let report = run(
            &track,
            &sections,
            &clamps,
            &[blocker(0, 1_050.0, 0)],
            &[],
            &[impossible],
        );
        assert!(report
            .errors()
            .any(|f| f.error.code == CourseErrorCode::ImpossibleLateralClearance));
    }

    #[test]
    fn a_clamped_section_is_reported_as_a_warning_not_an_error() {
        let track = track();
        let (sections, _) = sections(&track);
        let clamps = vec![GeometryClamps {
            curvature: 12,
            ..GeometryClamps::default()
        }];
        let report = run(&track, &sections, &clamps, &[], &[], &[]);
        assert!(report
            .warnings()
            .any(|f| f.error.message.contains("not the authored road")));
        assert!(!report
            .errors()
            .any(|f| f.error.code == CourseErrorCode::NonContinuousCourse));
    }

    #[test]
    fn the_report_is_byte_identical_across_two_runs() {
        let track = track();
        let (sections, clamps) = sections(&track);
        let plans = vec![blocker(0, 3_000.0, 0), blocker(1, 4_000.0, 1)];
        let a = run(&track, &sections, &clamps, &plans, &[], &[]);
        let b = run(&track, &sections, &clamps, &plans, &[], &[]);
        assert_eq!(a, b);
        assert_eq!(a.dump(), b.dump());
    }
}
