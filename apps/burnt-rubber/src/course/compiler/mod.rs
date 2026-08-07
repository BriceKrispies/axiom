//! **The compilation pipeline**: authored source to immutable runtime plan.
//!
//! ```text
//! CourseSpec  ──expand──▶  ExpandedCourse  ──geometry──▶  Track + sections
//!                                │                              │
//!                                └──────────traffic─────────────┤
//!                                                               ▼
//!                                                    validate ──▶ CoursePlan
//! ```
//!
//! Each arrow is a total function with no hidden state. Expansion turns motifs
//! and groups into a flat list of ordinary sections and the traffic zones that
//! span them; geometry turns that into one sample table; traffic turns the zones
//! into concrete vehicles against that table; validation measures the result;
//! and the plan is assembled from all of it and never changed again.
//!
//! The pipeline runs **once**, at course construction. Nothing here is on a
//! frame path, and nothing downstream of it can reach back into it.

pub mod seeds;

use crate::course::error::{CourseError, CourseErrorCode, CourseResult};
use crate::course::geometry;
use crate::course::motifs;
use crate::course::runtime::CoursePlan;
use crate::course::specification::{
    CourseItem, CourseSpec, RoadModifierSpec, RoadPrimitiveSpec, SectionId, SectionKind,
    SectionSpec, TrafficZoneSpec,
};
use crate::course::traffic::{
    encounters, flow, CompiledEncounter, NearMissWindow, TrafficPlan,
};
use crate::course::validation::{self, ValidationInput};
use crate::track::Track;
use crate::tuning::Tuning;

use crate::course::specification::{EncounterId, PassingSide, ScalarRange};

/// One section after motifs and groups have been expanded away.
///
/// Everything is resolved: the lane count, the expected speed and the
/// environment are concrete values rather than "or the course default", and the
/// id is the final stable name. The geometry compiler sees nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedSection {
    /// The stable name.
    pub id: SectionId,
    /// The road.
    pub primitive: RoadPrimitiveSpec,
    /// What is layered on it.
    pub modifiers: Vec<RoadModifierSpec>,
    /// Resolved lane count.
    pub lanes: u32,
    /// Resolved expected player speed (m/s).
    pub expected_speed_mps: f32,
    /// Resolved environment/scenery profile.
    pub environment: SectionKind,
}

/// A traffic zone after expansion: which sections it covers, and what it asks
/// for.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedTrafficZone {
    /// The stable name the zone's seed streams are anchored on.
    pub id: SectionId,
    /// The index of the first section it covers.
    pub first_section: usize,
    /// The index one past the last section it covers.
    pub last_section: usize,
    /// What it asks for.
    pub spec: TrafficZoneSpec,
}

/// The whole course, expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedCourse {
    /// The sections, in course order.
    pub sections: Vec<ExpandedSection>,
    /// The traffic zones, in course order.
    pub zones: Vec<ExpandedTrafficZone>,
}

/// Expand motifs and groups into ordinary sections and traffic zones.
///
/// This is the last point at which a motif exists. Everything after it works on
/// [`ExpandedSection`]s, which is what makes "the runtime must not know a motif
/// existed" a structural fact rather than a rule somebody has to remember.
pub fn expand(spec: &CourseSpec) -> CourseResult<ExpandedCourse> {
    spec.validate()?;
    let mut sections: Vec<ExpandedSection> = Vec::new();
    let mut zones: Vec<ExpandedTrafficZone> = Vec::new();

    for item in &spec.items {
        let first_section = sections.len();
        let (produced, id, traffic): (Vec<SectionSpec>, SectionId, Option<TrafficZoneSpec>) =
            match item {
                CourseItem::Section(section) => (
                    vec![section.clone()],
                    section.id.clone(),
                    section.traffic.clone(),
                ),
                CourseItem::Group(group) => (
                    group
                        .parts
                        .iter()
                        .enumerate()
                        .map(|(i, part)| SectionSpec {
                            id: group.id.child(i),
                            lanes: part.lanes.or(group.lanes),
                            expected_speed_mps: part
                                .expected_speed_mps
                                .or(group.expected_speed_mps),
                            environment: part.environment.or(group.environment),
                            ..part.clone()
                        })
                        .collect(),
                    group.id.clone(),
                    group.traffic.clone(),
                ),
                CourseItem::Motif(motif) => (
                    motifs::expand(spec.seed, motif)?,
                    motif.id.clone(),
                    motif.traffic.clone(),
                ),
            };

        produced.into_iter().for_each(|section| {
            sections.push(ExpandedSection {
                lanes: section.lanes.unwrap_or(spec.defaults.lanes),
                expected_speed_mps: section
                    .expected_speed_mps
                    .unwrap_or(spec.defaults.expected_speed_mps),
                environment: section.environment.unwrap_or(spec.defaults.environment),
                id: section.id,
                primitive: section.primitive,
                modifiers: section.modifiers,
            });
        });
        traffic.filter(|t| !t.is_empty()).map(|spec| {
            zones.push(ExpandedTrafficZone {
                id,
                first_section,
                last_section: sections.len(),
                spec,
            })
        });
    }

    let ids: Vec<SectionId> = sections.iter().map(|s| s.id.clone()).collect();
    crate::course::specification::ids::reject_duplicates(&ids)?;
    Ok(ExpandedCourse { sections, zones })
}

/// Compile a specification into the immutable plan the game runs on.
///
/// Returns `Err` only where the course cannot be *built* at all — a bad number,
/// an unknown motif, a lane count the tarmac cannot carry. A course that builds
/// but is unplayable comes back as a plan whose
/// [`report`](CoursePlan::report) has errors, which is deliberate: the caller
/// can then show the author *where*, on the road they actually got.
pub fn compile(spec: &CourseSpec, tuning: &Tuning) -> CourseResult<CoursePlan> {
    let expanded = expand(spec)?;
    // The authored defaults own the lane lattice; the course tuning owns the
    // limits. Resolving them into one record here is what keeps the geometry
    // compiler from having to know about `CourseDefaults` at all.
    let course_tuning = crate::tuning::CourseTuning {
        lane_width: spec.defaults.lane_width_m,
        lane_shoulder: spec.defaults.shoulder_width_m,
        ..tuning.course
    };
    let geometry = geometry::compile(&expanded.sections, &course_tuning, &spec.thresholds)?;
    let track = Track::from_samples(
        spec.seed,
        geometry.samples.clone(),
        &course_tuning,
    );

    let section_of = |distance_m: f32| -> u16 {
        geometry
            .sections
            .iter()
            .rposition(|s| s.start_m <= distance_m)
            .unwrap_or(0) as u16
    };

    let mut plans: Vec<TrafficPlan> = Vec::new();
    let mut compiled_encounters: Vec<CompiledEncounter> = Vec::new();
    let mut windows: Vec<NearMissWindow> = Vec::new();
    let mut next_vehicle = 0u32;
    let mut next_encounter = 0u32;

    for zone in &expanded.zones {
        let start_m = geometry.sections[zone.first_section].start_m;
        let end_m = geometry.sections[zone.last_section - 1].end_m;
        let expected = geometry.sections[zone.first_section].expected_speed_mps;

        // Where the zone's authored figures are. Ambient traffic keeps out of
        // them: an encounter is a *composition*, and a random car dropped into
        // the gap a zipper deliberately left is both the wrong shape and the
        // fastest way to turn a designed figure into a wall.
        let figures: Vec<(f32, f32)> = zone
            .spec
            .encounters
            .iter()
            .map(|e| {
                let at = start_m + e.start_offset_m();
                (at - ENCOUNTER_KEEP_OUT_M, at + e.length_m() + ENCOUNTER_KEEP_OUT_M)
            })
            .collect();

        if let Some(flow_spec) = &zone.spec.flow {
            let ambient = flow::compile(
                spec.seed,
                &zone.id,
                flow_spec,
                &track,
                start_m,
                end_m,
                expected,
                &section_of,
                &mut next_vehicle,
            )?;
            // Dropped *after* generation rather than skipped during it, so the
            // flow's own stream is untouched: moving an encounter must not
            // re-roll the ambient traffic around it.
            let ambient: Vec<TrafficPlan> = ambient
                .into_iter()
                .filter(|plan| {
                    !figures
                        .iter()
                        .any(|(from, to)| (plan.spawn_m >= *from) & (plan.spawn_m <= *to))
                })
                .collect();
            // Every ambient car is an opportunity where the player actually
            // meets it, which is not where it spawns.
            ambient.iter().for_each(|plan| {
                let meet = crate::course::traffic::meeting_distance(
                    plan.spawn_m,
                    plan.speed_mps,
                    tuning.race.traffic_ahead,
                    expected,
                    track.length(),
                );
                windows.push(NearMissWindow {
                    encounter: None,
                    start_m: (meet - crate::course::traffic::MEETING_WINDOW_M).max(0.0),
                    end_m: meet + crate::course::traffic::MEETING_WINDOW_M,
                    vehicles: vec![plan.id],
                    clearance_m: ScalarRange::new(
                        AMBIENT_CLEARANCE_MIN_M,
                        AMBIENT_CLEARANCE_MAX_M,
                    ),
                    side: PassingSide::Either,
                    minimum_relative_speed_mps: (expected - plan.speed_mps).max(0.0)
                        * AMBIENT_RELATIVE_SPEED_SHARE,
                    intended_opportunities: 1,
                    difficulty_weight: AMBIENT_DIFFICULTY_WEIGHT,
                    section: plan.section,
                });
            });
            plans.extend(ambient);
        }

        for encounter in &zone.spec.encounters {
            let id = EncounterId(next_encounter);
            next_encounter += 1;
            let output = encounters::compile(
                spec.seed,
                &zone.id,
                encounter,
                &track,
                start_m,
                id,
                &section_of,
                &mut next_vehicle,
            )?;
            plans.extend(output.plans);
            windows.push(output.window);
            compiled_encounters.push(output.encounter);
        }

        for window in &zone.spec.near_miss_windows {
            let at = start_m + window.start_offset_m;
            let end = at + window.length_m;
            windows.push(NearMissWindow {
                encounter: None,
                start_m: at,
                end_m: end,
                // An explicitly-authored window is against whatever traffic the
                // zone put in its span; it does not conjure vehicles.
                vehicles: plans
                    .iter()
                    .filter(|p| (p.spawn_m >= at) & (p.spawn_m <= end))
                    .map(|p| p.id)
                    .collect(),
                clearance_m: window.clearance_m,
                side: window.side,
                minimum_relative_speed_mps: window.minimum_relative_speed_mps,
                intended_opportunities: window.intended_opportunities,
                difficulty_weight: window.difficulty_weight,
                section: section_of(at),
            });
        }
    }

    // The runtime's indexes assume ascending order, and identities are minted
    // in generation order rather than in course order, so both are sorted here —
    // once, at compile time, rather than being maintained per frame.
    plans.sort_by(|a, b| a.spawn_m.total_cmp(&b.spawn_m).then(a.id.cmp(&b.id)));
    windows.sort_by(|a, b| {
        a.start_m
            .total_cmp(&b.start_m)
            .then(a.end_m.total_cmp(&b.end_m))
    });
    compiled_encounters.sort_by(|a, b| a.start_m.total_cmp(&b.start_m).then(a.id.cmp(&b.id)));

    let report = validation::validate(ValidationInput {
        track: &track,
        sections: &geometry.sections,
        clamps: &geometry.clamps,
        plans: &plans,
        encounters: &compiled_encounters,
        windows: &windows,
        thresholds: &spec.thresholds,
        vehicle: &tuning.vehicle,
        race: &tuning.race,
    });

    Ok(CoursePlan::assemble(
        spec.name.clone(),
        spec.seed,
        track,
        geometry.sections,
        plans,
        compiled_encounters,
        windows,
        report,
    ))
}

/// Compile, and refuse a course whose validation found errors.
///
/// The strict door: a test fixture or a tool that wants "this course is
/// playable or tell me why not" calls this, and the message carries every error
/// rather than the first.
pub fn compile_valid(spec: &CourseSpec, tuning: &Tuning) -> CourseResult<CoursePlan> {
    let plan = compile(spec, tuning)?;
    (!plan.report().has_errors())
        .then_some(())
        .ok_or_else(|| {
            let detail = plan
                .report()
                .errors()
                .map(|f| f.line())
                .collect::<Vec<String>>()
                .join("\n");
            CourseError::new(
                CourseErrorCode::UntraversableEncounter,
                format!(
                    "course `{}` compiled but did not validate:\n{detail}",
                    spec.name
                ),
            )
        })?;
    Ok(plan)
}

/// How much clear road an authored figure is given either side of itself (m).
const ENCOUNTER_KEEP_OUT_M: f32 = 140.0;

/// The clearance band an ambient pass is meant to happen in (m).
///
/// The near-miss rule pays for a pass one lane over, so the band runs from
/// "very close" up to a shade under a lane width.
const AMBIENT_CLEARANCE_MIN_M: f32 = 0.35;
/// See [`AMBIENT_CLEARANCE_MIN_M`].
const AMBIENT_CLEARANCE_MAX_M: f32 = 3.1;
/// How much of the projected closing speed an ambient pass has to keep to
/// count. Below one, because a player who has lifted slightly is still passing.
const AMBIENT_RELATIVE_SPEED_SHARE: f32 = 0.35;
/// How much of an ambient window a skilled route actually converts. Ordinary
/// traffic on open road is the reliable case; an authored figure asks more.
const AMBIENT_DIFFICULTY_WEIGHT: f32 = 0.9;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{
        MotifInvocation, MotifKind, SectionGroupSpec, TrafficFlowSpec, ZipperSpec,
    };

    fn straight(id: &str, length_m: f32) -> SectionSpec {
        SectionSpec::new(
            SectionId::new(id),
            RoadPrimitiveSpec::Straight { length_m },
        )
    }

    #[test]
    fn expansion_flattens_groups_and_motifs_into_ordinary_sections() {
        let mut spec = CourseSpec::new("mixed", 3);
        spec.items.push(CourseItem::Section(straight("opening", 400.0)));
        let mut group = SectionGroupSpec::new(SectionId::new("squeeze"));
        group.parts.push(straight("ignored", 300.0));
        group.parts.push(straight("also-ignored", 200.0));
        group.lanes = Some(3);
        spec.items.push(CourseItem::Group(group));
        spec.items.push(CourseItem::Motif(MotifInvocation::new(
            SectionId::new("sweeps"),
            MotifKind::AlternatingSlalom,
        )));

        let expanded = expand(&spec).expect("expands");
        let names: Vec<String> = expanded.sections.iter().map(|s| s.id.to_string()).collect();
        assert_eq!(names[0], "opening");
        assert_eq!(names[1], "squeeze/0", "a group's parts are re-minted");
        assert_eq!(names[2], "squeeze/1");
        assert!(names[3].starts_with("sweeps/"), "{names:?}");
        assert_eq!(expanded.sections[1].lanes, 3, "the group's lanes are inherited");
        assert_eq!(
            expanded.sections[0].lanes,
            spec.defaults.lanes,
            "and the course default otherwise"
        );
        // Nothing downstream can tell a motif existed: they are all sections.
        assert!(expanded.sections.iter().all(|s| s.primitive.length_m() > 0.0));
    }

    #[test]
    fn a_traffic_zone_spans_every_section_its_item_produced() {
        let mut spec = CourseSpec::new("zoned", 3);
        spec.items.push(CourseItem::Section(straight("clear", 400.0)));
        let mut motif = MotifInvocation::new(SectionId::new("run"), MotifKind::HighSpeedSweeps);
        motif.params.count = 3;
        motif.traffic = Some(TrafficZoneSpec {
            flow: Some(TrafficFlowSpec::at_density(12.0)),
            ..TrafficZoneSpec::default()
        });
        spec.items.push(CourseItem::Motif(motif));
        let expanded = expand(&spec).expect("expands");
        assert_eq!(expanded.zones.len(), 1);
        let zone = &expanded.zones[0];
        assert_eq!(zone.first_section, 1);
        assert_eq!(zone.last_section, expanded.sections.len());
        assert_eq!(zone.id.as_str(), "run");
    }

    #[test]
    fn a_duplicate_section_id_is_rejected() {
        let mut spec = CourseSpec::new("clash", 1);
        spec.items.push(CourseItem::Section(straight("same", 200.0)));
        spec.items.push(CourseItem::Section(straight("same", 200.0)));
        let err = expand(&spec).unwrap_err();
        assert_eq!(err.code, CourseErrorCode::DuplicateIdentifier);
        assert_eq!(err.section.as_deref(), Some("same"));
    }

    #[test]
    fn an_empty_traffic_zone_is_not_a_zone() {
        let mut spec = CourseSpec::new("empty", 1);
        spec.items.push(CourseItem::Section(
            straight("a", 400.0).with_traffic(TrafficZoneSpec::default()),
        ));
        assert!(expand(&spec).unwrap().zones.is_empty());
    }

    #[test]
    fn a_compiled_course_carries_road_traffic_and_a_report() {
        let mut spec = CourseSpec::new("small", 21);
        spec.items.push(CourseItem::Section(
            straight("run", 2_000.0).with_traffic(TrafficZoneSpec {
                flow: Some(TrafficFlowSpec::at_density(12.0)),
                ..TrafficZoneSpec::default()
            }),
        ));
        let plan = compile(&spec, &Tuning::DEFAULT).expect("compiles");
        assert_eq!(plan.name(), "small");
        assert_eq!(plan.seed(), 21);
        assert!((plan.length() - 2_000.0).abs() < 5.0);
        assert!(!plan.traffic().is_empty());
        assert_eq!(
            plan.near_miss_windows().len(),
            plan.traffic().len(),
            "one ambient opportunity per ambient car"
        );
        assert!(plan.report().metrics.vehicles > 0);
        // Traffic and windows are in ascending order, which the indexes assume.
        assert!(plan.traffic().windows(2).all(|w| w[1].spawn_m >= w[0].spawn_m));
        assert!(plan
            .near_miss_windows()
            .windows(2)
            .all(|w| w[1].start_m >= w[0].start_m));
    }

    #[test]
    fn compiling_the_same_specification_twice_produces_the_same_plan() {
        let mut spec = CourseSpec::new("stable", 88);
        spec.items.push(CourseItem::Section(
            straight("run", 3_000.0).with_traffic(TrafficZoneSpec {
                flow: Some(TrafficFlowSpec::at_density(16.0)),
                ..TrafficZoneSpec::default()
            }),
        ));
        let a = compile(&spec, &Tuning::DEFAULT).unwrap();
        let b = compile(&spec, &Tuning::DEFAULT).unwrap();
        assert_eq!(a.dump(), b.dump());
        assert_eq!(a.track().samples(), b.track().samples());
        // A different seed is a different course.
        let mut other = spec.clone();
        other.seed = 89;
        assert_ne!(compile(&other, &Tuning::DEFAULT).unwrap().dump(), a.dump());
    }

    #[test]
    fn an_encounter_compiles_into_ordinary_traffic_owned_by_it() {
        let mut spec = CourseSpec::new("figure", 5);
        spec.items.push(CourseItem::Section(straight("lead-in", 800.0).with_lanes(3)));
        spec.items.push(CourseItem::Section(
            straight("figure", 1_200.0)
                .with_lanes(3)
                .with_traffic(TrafficZoneSpec {
                    encounters: vec![crate::course::specification::EncounterSpec::Zipper(
                        ZipperSpec {
                            start_offset_m: 100.0,
                            length_m: 300.0,
                            spacing_m: 80.0,
                            ..ZipperSpec::of_length(300.0)
                        },
                    )],
                    ..TrafficZoneSpec::default()
                }),
        ));
        let plan = compile(&spec, &Tuning::DEFAULT).expect("compiles");
        assert_eq!(plan.encounters().len(), 1);
        let encounter = &plan.encounters()[0];
        assert_eq!(encounter.kind, "zipper");
        assert!(!encounter.vehicles.is_empty());
        assert!(plan
            .traffic()
            .iter()
            .filter(|p| p.encounter == Some(encounter.id))
            .count()
            == encounter.vehicles.len());
        assert!((encounter.start_m - 900.0).abs() < 5.0, "at {}", encounter.start_m);
    }

    #[test]
    fn a_course_that_cannot_be_built_fails_rather_than_compiling_badly() {
        let mut spec = CourseSpec::new("broken", 1);
        spec.items.push(CourseItem::Section(SectionSpec::new(
            SectionId::new("hairpin"),
            RoadPrimitiveSpec::Turn {
                length_m: 200.0,
                radius_m: 12.0,
                direction: crate::course::specification::TurnDirection::Right,
            },
        )));
        assert_eq!(
            compile(&spec, &Tuning::DEFAULT).unwrap_err().code,
            CourseErrorCode::InvalidRadius
        );
    }

    #[test]
    fn the_strict_door_refuses_a_course_whose_report_has_errors() {
        // A course whose road is fine but whose traffic walls it off.
        let mut spec = CourseSpec::new("walled", 4);
        spec.items.push(CourseItem::Section(
            straight("run", 2_000.0)
                .with_lanes(3)
                .with_traffic(TrafficZoneSpec {
                    encounters: vec![crate::course::specification::EncounterSpec::RollingWall(
                        crate::course::specification::RollingWallSpec {
                            start_offset_m: 400.0,
                            wall_width_lanes: 2,
                            open_lane: 1,
                            // The opening never moves, and the wall is two
                            // lanes of a three-lane road: passable.
                            opening_step_lanes: 0,
                            phase_length_m: 200.0,
                            phases: 3,
                            speed_mps: 30.0,
                            group_spacing_m: 100.0,
                            reaction_distance_m: 120.0,
                        },
                    )],
                    ..TrafficZoneSpec::default()
                }),
        ));
        let plan = compile(&spec, &Tuning::DEFAULT).expect("compiles");
        assert!(!plan.report().has_errors(), "{}", plan.report().dump());
        assert!(compile_valid(&spec, &Tuning::DEFAULT).is_ok());
    }
}
