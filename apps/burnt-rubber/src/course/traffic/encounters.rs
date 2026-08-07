//! **Encounter compilation**: authored figures become concrete vehicles.
//!
//! Each template places exactly the cars it describes and hands back a
//! [`CompiledEncounter`] recording what it demanded of the player, so validation
//! can check the figure against its own claims rather than against a guess. A
//! zipper that asks for 0.75 s of reaction time is checked against 0.75 s.
//!
//! The seeded stream here ([`SeedDomain::TrafficEncounter`]) is deliberately
//! separate from the ambient one: an encounter is *authored*, and re-tuning the
//! ambient density around it must not move the figure.

use crate::course::compiler::seeds::{section_draw, SeedDomain};
use crate::course::error::{CourseError, CourseErrorCode, CourseResult};
use crate::course::specification::{
    EncounterId, EncounterSpec, PassingSide, RollingWallSpec, ScalarRange, SectionId, SlalomSpec,
    VehicleArchetype, VehicleId, ZipperSpec,
};
use crate::draw::Draw;
use crate::track::Track;

use super::{CompiledEncounter, NearMissWindow, TrafficPlan, PLAN_LIFETIME_M};

/// The most vehicles one encounter may place — a bound on an authored figure,
/// the same way [`super::flow::MAX_VEHICLES_PER_ZONE`] bounds a density.
pub const MAX_VEHICLES_PER_ENCOUNTER: usize = 192;

/// What one compiled encounter produced.
#[derive(Debug, Clone, PartialEq)]
pub struct EncounterOutput {
    /// The encounter record.
    pub encounter: CompiledEncounter,
    /// The vehicles it placed.
    pub plans: Vec<TrafficPlan>,
    /// The opportunity window it offers.
    pub window: NearMissWindow,
}

/// Compile one encounter into concrete vehicles.
#[allow(clippy::too_many_arguments)]
pub fn compile(
    course_seed: u64,
    zone_id: &SectionId,
    spec: &EncounterSpec,
    track: &Track,
    zone_start_m: f32,
    id: EncounterId,
    section_of: &dyn Fn(f32) -> u16,
    next_id: &mut u32,
) -> CourseResult<EncounterOutput> {
    let start_m = zone_start_m + spec.start_offset_m();
    let mut draw = section_draw(course_seed, &zone_id.child(id.0), SeedDomain::TrafficEncounter);
    let rows = match spec {
        EncounterSpec::Zipper(z) => zipper_rows(z, track, start_m),
        EncounterSpec::RollingWall(w) => rolling_wall_rows(w, track, start_m),
        EncounterSpec::Slalom(s) => slalom_rows(s, track, start_m),
    };
    (rows.len() <= MAX_VEHICLES_PER_ENCOUNTER)
        .then_some(())
        .ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::RepeatLimitExceeded,
                format!(
                    "a {} encounter asked for {} vehicles, above the per-encounter limit of \
                     {MAX_VEHICLES_PER_ENCOUNTER}",
                    spec.token(),
                    rows.len()
                ),
            )
            .in_section(zone_id.as_str())
        })?;
    (!rows.is_empty()).then_some(()).ok_or_else(|| {
        CourseError::new(
            CourseErrorCode::UntraversableEncounter,
            format!("a {} encounter placed no vehicles at all", spec.token()),
        )
        .in_section(zone_id.as_str())
    })?;

    // A vehicle placed off the end of the course is a figure that does not fit,
    // not a figure that silently loses its tail.
    let overrun = rows.iter().any(|row| row.distance_m > track.length());
    (!overrun).then_some(()).ok_or_else(|| {
        CourseError::new(
            CourseErrorCode::UntraversableEncounter,
            format!(
                "a {} encounter starting at {start_m:.0} m runs off the end of a {:.0} m course",
                spec.token(),
                track.length()
            ),
        )
        .in_section(zone_id.as_str())
    })?;

    let plans: Vec<TrafficPlan> = rows
        .iter()
        .map(|row| {
            let vehicle = VehicleId(*next_id);
            *next_id += 1;
            TrafficPlan {
                id: vehicle,
                spawn_m: row.distance_m,
                despawn_m: (row.distance_m + PLAN_LIFETIME_M).min(track.length()),
                lane: row.lane,
                speed_mps: spec.speed_mps().max(1.0),
                archetype: archetype_for(&mut draw),
                lane_changes: row.lane_changes.clone(),
                speed_changes: Vec::new(),
                encounter: Some(id),
                section: section_of(row.distance_m),
                variation_seed: draw.fork(u64::from(vehicle.0)).seed(),
            }
        })
        .collect();

    let end_m = start_m + spec.length_m();
    let encounter = CompiledEncounter {
        id,
        kind: spec.token(),
        section: section_of(start_m),
        start_m,
        end_m,
        vehicles: plans.iter().map(|p| p.id).collect(),
        requires_route: requires_route(spec),
        minimum_reaction_time_s: minimum_reaction_time(spec),
        lateral_clearance_m: lateral_clearance(spec),
        target_near_misses: target_near_misses(spec),
    };
    let window = NearMissWindow {
        encounter: Some(id),
        start_m,
        end_m,
        vehicles: encounter.vehicles.clone(),
        clearance_m: ScalarRange::new(
            encounter.lateral_clearance_m,
            encounter.lateral_clearance_m * 3.0,
        ),
        side: PassingSide::Either,
        minimum_relative_speed_mps: ENCOUNTER_MIN_RELATIVE_SPEED_MPS,
        intended_opportunities: encounter.target_near_misses,
        difficulty_weight: difficulty_weight(spec),
        section: encounter.section,
    };
    Ok(EncounterOutput {
        encounter,
        plans,
        window,
    })
}

/// The least closing speed a pass inside an encounter has to have to count.
///
/// Deliberately non-zero: crawling past a blocker is not the manoeuvre the
/// figure was built to reward.
pub const ENCOUNTER_MIN_RELATIVE_SPEED_MPS: f32 = 8.0;

/// One vehicle an encounter wants placed.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    distance_m: f32,
    lane: i32,
    lane_changes: Vec<super::LaneChange>,
}

/// **Zipper.** Each row blocks every lane but one, and the open lane walks back
/// and forth, so the only route through is a weave.
fn zipper_rows(spec: &ZipperSpec, track: &Track, start_m: f32) -> Vec<Row> {
    let rows = (spec.length_m / spec.spacing_m).floor().max(1.0) as u32;
    (0..rows)
        .flat_map(|k| {
            let distance_m = start_m + k as f32 * spec.spacing_m;
            let reach = track.lane_reach(&track.sample_at(distance_m));
            let open = open_lane(spec, reach, k);
            (-reach..=reach)
                .filter(move |lane| *lane != open)
                .map(move |lane| Row {
                    distance_m,
                    lane,
                    lane_changes: Vec::new(),
                })
                .collect::<Vec<Row>>()
        })
        .collect()
}

/// Which lane row `k` of a zipper leaves open.
///
/// The pair of lanes the opening bounces between is resolved once against the
/// road that actually exists: if stepping in the authored direction would leave
/// the tarmac, it steps the other way instead, so a zipper on a narrow road
/// still alternates rather than collapsing to a fixed opening.
fn open_lane(spec: &ZipperSpec, reach: i32, row: u32) -> i32 {
    let first = spec.first_open_lane.clamp(-reach, reach);
    let step = spec.alternation.sign() as i32;
    let forward = (first + step).clamp(-reach, reach);
    let second = (forward != first)
        .then_some(forward)
        .unwrap_or_else(|| (first - step).clamp(-reach, reach));
    [first, second][(row % 2) as usize]
}

/// **Rolling wall.** A block of vehicles occupies the lanes nearest the opening,
/// and the opening moves between phases.
fn rolling_wall_rows(spec: &RollingWallSpec, track: &Track, start_m: f32) -> Vec<Row> {
    let rows_per_phase = (spec.phase_length_m / spec.group_spacing_m).floor().max(1.0) as u32;
    (0..spec.phases)
        .flat_map(|phase| {
            let phase_start = start_m + phase as f32 * spec.phase_length_m;
            (0..rows_per_phase).flat_map(move |row| {
                let distance_m = phase_start + row as f32 * spec.group_spacing_m;
                let reach = track.lane_reach(&track.sample_at(distance_m));
                let open = spec.open_lane_for(phase).clamp(-reach, reach);
                // The wall is the lanes nearest the opening, so the gap is a
                // slot in a block rather than a hole at one edge.
                let mut lanes: Vec<i32> = (-reach..=reach).filter(|l| *l != open).collect();
                lanes.sort_by_key(|l| ((l - open).abs(), *l));
                lanes.truncate(spec.wall_width_lanes as usize);
                lanes.sort_unstable();
                lanes
                    .into_iter()
                    .map(move |lane| Row {
                        distance_m,
                        lane,
                        lane_changes: Vec::new(),
                    })
                    .collect::<Vec<Row>>()
            })
        })
        .collect()
}

/// **Slalom.** Single blockers stepping through the authored lane sequence.
fn slalom_rows(spec: &SlalomSpec, track: &Track, start_m: f32) -> Vec<Row> {
    (0..spec.blockers)
        .map(|k| {
            let distance_m = start_m + k as f32 * spec.spacing_m;
            let reach = track.lane_reach(&track.sample_at(distance_m));
            let lane = spec.lane_sequence[(k as usize) % spec.lane_sequence.len()]
                .clamp(-reach, reach);
            Row {
                distance_m,
                lane,
                lane_changes: Vec::new(),
            }
        })
        .collect()
}

fn requires_route(spec: &EncounterSpec) -> bool {
    match spec {
        EncounterSpec::Zipper(z) => z.require_continuous_route,
        // A wall with a moving opening is only an encounter if the opening can
        // be reached; a slalom is a rhythm and can always be taken wide.
        EncounterSpec::RollingWall(_) => true,
        EncounterSpec::Slalom(_) => false,
    }
}

fn minimum_reaction_time(spec: &EncounterSpec) -> f32 {
    match spec {
        EncounterSpec::Zipper(z) => z.minimum_reaction_time_s,
        EncounterSpec::RollingWall(w) => w.reaction_distance_m / w.speed_mps.max(1.0),
        EncounterSpec::Slalom(s) => s.spacing_m / s.speed_mps.max(1.0),
    }
}

fn lateral_clearance(spec: &EncounterSpec) -> f32 {
    match spec {
        EncounterSpec::Zipper(z) => z.lateral_clearance_m,
        EncounterSpec::RollingWall(_) => DEFAULT_WALL_CLEARANCE_M,
        EncounterSpec::Slalom(s) => s.clearance_m,
    }
}

/// The lateral gap a rolling wall's opening is meant to leave. The wall states
/// its width in lanes rather than in metres, so the clearance is the lane
/// lattice's own margin.
const DEFAULT_WALL_CLEARANCE_M: f32 = 0.6;

fn target_near_misses(spec: &EncounterSpec) -> u32 {
    match spec {
        EncounterSpec::Zipper(z) => z.target_near_misses,
        // Two chances per phase: one either side of the opening.
        EncounterSpec::RollingWall(w) => w.phases * 2,
        EncounterSpec::Slalom(s) => s.blockers,
    }
}

/// How much of an encounter's offered chances a skilled route converts.
///
/// Higher is easier. A slalom is a rhythm and a good driver takes nearly all of
/// it; a zipper's chances are the hardest on the course.
fn difficulty_weight(spec: &EncounterSpec) -> f32 {
    match spec {
        EncounterSpec::Zipper(_) => 0.7,
        EncounterSpec::RollingWall(_) => 0.8,
        EncounterSpec::Slalom(_) => 0.95,
    }
}

/// Encounter vehicles are cosmetically varied but never *behaviourally*: the
/// figure is the point, and a van that handled differently from a saloon would
/// make the same authored figure a different puzzle each time.
fn archetype_for(draw: &mut Draw) -> VehicleArchetype {
    VehicleArchetype::ALL[draw.index(VehicleArchetype::ALL.len())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::TurnDirection;

    fn track() -> Track {
        crate::course::procedural::shipping_plan(crate::DEFAULT_SEED)
            .expect("the shipping course compiles")
            .track()
            .clone()
    }

    fn build(spec: &EncounterSpec, track: &Track, start_m: f32) -> EncounterOutput {
        let mut next = 0u32;
        compile(
            5,
            &SectionId::new("zone"),
            spec,
            track,
            start_m,
            EncounterId(1),
            &|_| 0,
            &mut next,
        )
        .expect("compiles")
    }

    /// Where the road is three lanes wide, so a zipper leaves two blockers a
    /// row rather than four.
    fn narrow_start(track: &Track) -> f32 {
        track
            .samples()
            .iter()
            .find(|s| (s.distance > 3_000.0) & (track.lane_reach(s) == 1))
            .map(|s| s.distance)
            .expect("the shipping course has a three-lane stretch")
    }

    #[test]
    fn a_zipper_alternates_its_opening_and_blocks_everything_else() {
        let track = track();
        let start = narrow_start(&track);
        let spec = EncounterSpec::Zipper(ZipperSpec {
            spacing_m: 60.0,
            length_m: 300.0,
            first_open_lane: 0,
            alternation: TurnDirection::Right,
            ..ZipperSpec::of_length(300.0)
        });
        let out = build(&spec, &track, start);
        assert_eq!(out.encounter.kind, "zipper");
        assert!(out.encounter.requires_route);

        // Group the placed vehicles into rows and read the open lane of each.
        let mut rows: Vec<(f32, Vec<i32>)> = Vec::new();
        for p in &out.plans {
            match rows.iter_mut().find(|(d, _)| (*d - p.spawn_m).abs() < 1.0) {
                Some((_, lanes)) => lanes.push(p.lane),
                None => rows.push((p.spawn_m, vec![p.lane])),
            }
        }
        assert_eq!(rows.len(), 5, "300 m at 60 m spacing is five rows");
        let opens: Vec<i32> = rows
            .iter()
            .map(|(d, lanes)| {
                let reach = track.lane_reach(&track.sample_at(*d));
                (-reach..=reach)
                    .find(|l| !lanes.contains(l))
                    .expect("every row leaves a lane open")
            })
            .collect();
        for pair in opens.windows(2) {
            assert_ne!(pair[0], pair[1], "the opening did not move: {opens:?}");
            assert_eq!(
                (pair[1] - pair[0]).abs(),
                1,
                "the opening jumped more than a lane: {opens:?}"
            );
        }
        // Every vehicle belongs to the encounter and to its window.
        assert!(out.plans.iter().all(|p| p.encounter == Some(EncounterId(1))));
        assert_eq!(out.window.vehicles, out.encounter.vehicles);
        assert_eq!(out.window.intended_opportunities, 6);
    }

    #[test]
    fn a_zipper_on_a_narrow_road_still_alternates_from_an_edge_lane() {
        let track = track();
        let start = narrow_start(&track);
        // The opening starts against the right edge and is told to step right,
        // which would leave the road — so it has to bounce the other way.
        let spec = EncounterSpec::Zipper(ZipperSpec {
            first_open_lane: 1,
            alternation: TurnDirection::Right,
            spacing_m: 60.0,
            length_m: 240.0,
            ..ZipperSpec::of_length(240.0)
        });
        let out = build(&spec, &track, start);
        let reach = track.lane_reach(&track.sample_at(start));
        assert_eq!(reach, 1);
        let first_row: Vec<i32> = out
            .plans
            .iter()
            .filter(|p| (p.spawn_m - start).abs() < 1.0)
            .map(|p| p.lane)
            .collect();
        assert!(!first_row.contains(&1), "the first row opens lane 1");
        let second_row: Vec<i32> = out
            .plans
            .iter()
            .filter(|p| (p.spawn_m - start - 60.0).abs() < 1.0)
            .map(|p| p.lane)
            .collect();
        assert!(!second_row.contains(&0), "and the second opens lane 0");
    }

    #[test]
    fn a_rolling_wall_produces_the_authored_number_of_phases_and_moves_its_gap() {
        let track = track();
        let start = narrow_start(&track);
        let spec = EncounterSpec::RollingWall(RollingWallSpec {
            phases: 3,
            phase_length_m: 150.0,
            group_spacing_m: 150.0,
            wall_width_lanes: 2,
            open_lane: 1,
            opening_step_lanes: -1,
            ..RollingWallSpec::of_phases(3)
        });
        let out = build(&spec, &track, start);
        assert_eq!(out.encounter.kind, "rolling_wall");
        assert_eq!(out.plans.len(), 3 * 2, "one row of two per phase");
        let phase_lanes = |phase: usize| -> Vec<i32> {
            let at = start + phase as f32 * 150.0;
            let mut lanes: Vec<i32> = out
                .plans
                .iter()
                .filter(|p| (p.spawn_m - at).abs() < 1.0)
                .map(|p| p.lane)
                .collect();
            lanes.sort_unstable();
            lanes
        };
        assert_eq!(phase_lanes(0), vec![-1, 0], "phase 0 leaves lane 1 open");
        assert_eq!(phase_lanes(1), vec![-1, 1], "phase 1 leaves lane 0 open");
        assert_eq!(phase_lanes(2), vec![0, 1], "phase 2 leaves lane -1 open");
        assert_eq!(out.encounter.target_near_misses, 6);
        assert!(out.encounter.requires_route);
    }

    #[test]
    fn a_slalom_walks_its_lane_sequence_and_leaves_a_recovery_gap() {
        let track = track();
        let start = narrow_start(&track);
        let spec = EncounterSpec::Slalom(SlalomSpec {
            blockers: 6,
            spacing_m: 70.0,
            lane_sequence: vec![-1, 1],
            recovery_gap_m: 200.0,
            ..SlalomSpec::of_blockers(6)
        });
        let out = build(&spec, &track, start);
        assert_eq!(out.plans.len(), 6);
        let lanes: Vec<i32> = out.plans.iter().map(|p| p.lane).collect();
        assert_eq!(lanes, vec![-1, 1, -1, 1, -1, 1]);
        for pair in out.plans.windows(2) {
            assert!((pair[1].spawn_m - pair[0].spawn_m - 70.0).abs() < 1.0e-3);
        }
        // The recovery gap is part of the encounter's extent, so nothing else
        // is authored into it.
        assert!(
            (out.encounter.end_m - out.encounter.start_m - (6.0 * 70.0 + 200.0)).abs() < 1.0e-2
        );
        assert!(!out.encounter.requires_route, "a slalom can be taken wide");
    }

    #[test]
    fn every_encounter_compiles_deterministically() {
        let track = track();
        let start = narrow_start(&track);
        for spec in [
            EncounterSpec::Zipper(ZipperSpec::of_length(240.0)),
            EncounterSpec::RollingWall(RollingWallSpec::of_phases(3)),
            EncounterSpec::Slalom(SlalomSpec::of_blockers(5)),
        ] {
            assert_eq!(build(&spec, &track, start), build(&spec, &track, start));
        }
    }

    #[test]
    fn an_encounter_that_runs_off_the_course_is_rejected() {
        let track = track();
        let spec = EncounterSpec::Slalom(SlalomSpec::of_blockers(20));
        let mut next = 0u32;
        let err = compile(
            5,
            &SectionId::new("zone"),
            &spec,
            &track,
            track.length() - 100.0,
            EncounterId(1),
            &|_| 0,
            &mut next,
        )
        .unwrap_err();
        assert_eq!(err.code, CourseErrorCode::UntraversableEncounter);
    }

    #[test]
    fn an_encounter_too_large_to_place_is_rejected() {
        let track = track();
        let spec = EncounterSpec::Zipper(ZipperSpec {
            length_m: 6_000.0,
            spacing_m: 8.0,
            ..ZipperSpec::of_length(6_000.0)
        });
        let mut next = 0u32;
        let err = compile(
            5,
            &SectionId::new("zone"),
            &spec,
            &track,
            300.0,
            EncounterId(1),
            &|_| 0,
            &mut next,
        )
        .unwrap_err();
        assert_eq!(err.code, CourseErrorCode::RepeatLimitExceeded);
    }

    #[test]
    fn the_reaction_time_and_clearance_an_encounter_demands_are_carried_through() {
        let track = track();
        let start = narrow_start(&track);
        let zipper = build(
            &EncounterSpec::Zipper(ZipperSpec {
                minimum_reaction_time_s: 1.25,
                lateral_clearance_m: 0.8,
                ..ZipperSpec::of_length(240.0)
            }),
            &track,
            start,
        );
        assert_eq!(zipper.encounter.minimum_reaction_time_s, 1.25);
        assert_eq!(zipper.encounter.lateral_clearance_m, 0.8);

        let wall = build(
            &EncounterSpec::RollingWall(RollingWallSpec {
                reaction_distance_m: 150.0,
                speed_mps: 30.0,
                phases: 2,
                ..RollingWallSpec::of_phases(2)
            }),
            &track,
            start,
        );
        assert!((wall.encounter.minimum_reaction_time_s - 5.0).abs() < 1.0e-4);

        let slalom = build(
            &EncounterSpec::Slalom(SlalomSpec {
                spacing_m: 60.0,
                speed_mps: 30.0,
                clearance_m: 0.9,
                ..SlalomSpec::of_blockers(4)
            }),
            &track,
            start,
        );
        assert!((slalom.encounter.minimum_reaction_time_s - 2.0).abs() < 1.0e-4);
        assert_eq!(slalom.encounter.lateral_clearance_m, 0.9);
        // Higher is easier: a slalom converts better than a zipper.
        assert!(slalom.window.difficulty_weight > zipper.window.difficulty_weight);
        assert!(wall.window.difficulty_weight > zipper.window.difficulty_weight);
    }
}
