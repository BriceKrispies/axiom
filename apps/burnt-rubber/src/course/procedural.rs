//! **The shipping course**, authored through the same pipeline as everything
//! else.
//!
//! This is the nine-kilometre road Burnt Rubber has always shipped — opening
//! straight, coastal sweepers, ridge crests, the esses, the tunnel, the long
//! haul, the canyon, the final sweep, the finish — re-expressed as a
//! [`CourseSpec`]. Nothing about the road's *character* changed: the section
//! order, the lengths, how curvy and how hilly each one is, and how many lanes
//! it carries are all carried across from the pacing plan the old generator
//! walked. What changed is that they are now **authored values** compiled by the
//! ordinary pipeline rather than parameters fed to a bespoke control-point
//! generator.
//!
//! It is deliberately a *programmatic* course rather than a text one: it is
//! generated from a seed, so it has to be a function of that seed, and the DSL
//! is for hand-authored courses (see `courses/burning_coast.brc`). Both produce
//! the same [`CourseSpec`] type and go through the identical compiler, which is
//! the whole point of having one authored representation.
//!
//! ## How the old envelope maps onto the new vocabulary
//!
//! The old generator drew a *heading step per control point*, bounded by
//! `max_yaw_step` over `control_spacing` — 0.115 rad per 40 m, a 348 m radius at
//! full severity. A section's `curviness` scaled that, so a section of curviness
//! `c` bottomed out at `348 / c` metres of radius. That is exactly what
//! [`radius_for_curviness`] computes, which is why the new sweepers are the same
//! sweepers.
//!
//! Hills carried the same way: `hilliness` scaled `max_grade`, and an elevation
//! wave of amplitude `A` and wavelength `λ` peaks at a grade of `2πA/λ`, so
//! [`amplitude_for_hilliness`] inverts it.

use crate::course::compiler;
use crate::course::error::CourseResult;
use crate::course::runtime::CoursePlan;
use crate::course::specification::{
    BankingMode, BoostPickupSpec, BoostTier, CountRange, CourseBuilder, CourseDefaults,
    MotifInvocation, MotifKind, MotifParams, RoadModifierSpec, RoadPrimitiveSpec, ScalarRange,
    SectionId, SectionKind, SectionSpec, SlalomSpec, TrafficFlowSpec, TrafficZoneSpec,
    ValidationThresholds,
};
use crate::course::specification::{EncounterSpec, RollingWallSpec};
use crate::tuning::{CourseTuning, Tuning};

/// The old generator's heading-step bound, expressed as a radius (m).
///
/// `max_yaw_step` radians of turn over `control_spacing` metres of arc is a
/// circle of `control_spacing / max_yaw_step` metres — 348 m under the shipping
/// tuning. A section of curviness `c` used a fraction `c` of that bound, so it
/// turned on a radius of `TIGHTEST / c`.
pub fn radius_for_curviness(curviness: f32, tuning: &CourseTuning) -> f32 {
    let tightest = tuning.control_spacing / tuning.max_yaw_step.max(1.0e-4);
    tightest / curviness.clamp(0.05, 1.0)
}

/// The elevation-wave amplitude (m) that peaks at `hilliness` of the course's
/// maximum grade, over `wavelength_m`.
///
/// A wave `A·sin(2πs/λ)` has a maximum slope of `2πA/λ`, so the amplitude that
/// reaches a given grade is `g·λ/2π`.
pub fn amplitude_for_hilliness(hilliness: f32, wavelength_m: f32, tuning: &CourseTuning) -> f32 {
    hilliness.clamp(0.0, 1.0) * tuning.max_grade * wavelength_m
        / std::f32::consts::TAU
}

/// One entry of the pacing plan: the character the old generator drew inside.
struct Pacing {
    environment: SectionKind,
    length_m: f32,
    curviness: f32,
    hilliness: f32,
    lanes: u32,
    /// How many figures the section is broken into.
    pieces: u32,
    /// Vehicles per kilometre of ambient traffic, or zero for clear road.
    vehicles_per_km: f32,
    /// The boost pickups this section offers, at offsets from its own start.
    ///
    /// Authored per section rather than scattered by a rule, because *where* a
    /// pickup is is the whole of its difficulty. A rule that placed one every
    /// 400 m would produce charge nobody had to drive for; every row below is
    /// on a line that costs something — the outside of a banked sweeper, the
    /// far side of a crest you cannot see over, a committed lane in the tunnel's
    /// traffic. See [`crate::sim::boost`] for why that constraint is the whole
    /// point of the feature.
    pickups: &'static [BoostPickupSpec],
}

/// The shipping pacing plan, in course order.
///
/// The lengths, lane counts, curviness and hilliness are the old
/// `SectionProfile` values, unchanged. `pieces` replaces the old "how many bends
/// does this section emit" draw with an authored count, and `vehicles_per_km`
/// replaces the old fixed 85 m traffic slot pitch (1000/85 ≈ 11.8) with a
/// per-section density.
const PACING: [Pacing; 9] = [
    Pacing {
        environment: SectionKind::StartStraight,
        length_m: 620.0,
        curviness: 0.10,
        hilliness: 0.0,
        lanes: 5,
        pieces: 1,
        // The opening is clear road: the countdown and the first acceleration
        // happen on empty tarmac, exactly as `traffic_clear_start` used to
        // guarantee.
        vehicles_per_km: 0.0,
        // The teaching row: three green pickups down the centre of an empty
        // straight, where the only thing to learn is that driving over one
        // fills the bar.
        pickups: &[BoostPickupSpec::row(360.0, 0, BoostTier::Small, 3, 45.0)],
    },
    Pacing {
        environment: SectionKind::SweepingBends,
        length_m: 1_700.0,
        curviness: 0.78,
        hilliness: 0.28,
        lanes: 5,
        pieces: 5,
        vehicles_per_km: 11.8,
        // The sweepers alternate direction, so a fixed outer lane is the
        // *wide* line through half of them and the apex through the other
        // half. That is the intended cost: taking every one of these means
        // giving up the tightest line twice.
        pickups: &[
            BoostPickupSpec::row(300.0, 2, BoostTier::Medium, 2, 60.0),
            BoostPickupSpec::row(1_000.0, -2, BoostTier::Medium, 2, 60.0),
        ],
    },
    Pacing {
        environment: SectionKind::RollingHills,
        length_m: 1_250.0,
        curviness: 0.42,
        hilliness: 1.0,
        lanes: 3,
        pieces: 5,
        vehicles_per_km: 11.0,
        // Over the back of a crest, in the middle of a three-lane road: the
        // one large pickup on the course you cannot see before you commit to
        // it.
        pickups: &[BoostPickupSpec::single(620.0, 0, BoostTier::Large)],
    },
    Pacing {
        environment: SectionKind::TechnicalBends,
        length_m: 1_150.0,
        curviness: 1.0,
        hilliness: 0.35,
        lanes: 3,
        pieces: 6,
        vehicles_per_km: 10.5,
        pickups: &[
            BoostPickupSpec::row(240.0, -1, BoostTier::Small, 2, 50.0),
            BoostPickupSpec::row(760.0, 1, BoostTier::Medium, 2, 50.0),
        ],
    },
    Pacing {
        environment: SectionKind::Tunnel,
        length_m: 780.0,
        curviness: 0.34,
        hilliness: 0.12,
        lanes: 3,
        pieces: 3,
        vehicles_per_km: 12.5,
        // The densest traffic on the course. Holding an outer lane long
        // enough to take these is a commitment made among cars.
        pickups: &[
            BoostPickupSpec::single(300.0, 1, BoostTier::Large),
            BoostPickupSpec::row(560.0, -1, BoostTier::Small, 2, 40.0),
        ],
    },
    Pacing {
        environment: SectionKind::HighSpeedStraight,
        length_m: 1_500.0,
        curviness: 0.22,
        hilliness: 0.15,
        lanes: 5,
        pieces: 5,
        // "Wide, flat, flat-out — and full of traffic to thread."
        vehicles_per_km: 15.5,
        // Before the rolling wall and after it, never inside it: the wall's
        // opening walks across the road and a pickup standing in it would be
        // bait that is sometimes a car.
        pickups: &[
            BoostPickupSpec::row(180.0, 0, BoostTier::Medium, 3, 55.0),
            BoostPickupSpec::single(1_260.0, -2, BoostTier::Large),
        ],
    },
    Pacing {
        environment: SectionKind::Canyon,
        length_m: 1_100.0,
        curviness: 0.86,
        hilliness: 0.45,
        lanes: 3,
        pieces: 5,
        vehicles_per_km: 11.0,
        pickups: &[
            BoostPickupSpec::row(120.0, 1, BoostTier::Small, 2, 45.0),
            BoostPickupSpec::single(900.0, -1, BoostTier::Large),
        ],
    },
    Pacing {
        environment: SectionKind::FinalSweeps,
        length_m: 850.0,
        curviness: 0.70,
        hilliness: 0.30,
        lanes: 5,
        pieces: 3,
        vehicles_per_km: 12.0,
        pickups: &[
            BoostPickupSpec::row(200.0, 2, BoostTier::Medium, 2, 60.0),
            BoostPickupSpec::single(640.0, -2, BoostTier::Large),
        ],
    },
    Pacing {
        environment: SectionKind::Finish,
        length_m: 320.0,
        curviness: 0.0,
        hilliness: 0.0,
        lanes: 5,
        pieces: 1,
        vehicles_per_km: 0.0,
        // Nothing on the run to the line. The last three hundred metres are
        // about whatever the player has left, not about topping it up.
        pickups: &[],
    },
];

/// The wavelength the shipping course's elevation wave uses (m).
///
/// Long, and that is the whole point. A wave of amplitude `A` and wavelength `λ`
/// pulls the car down over its crest at `A·k²·v²`, which for a wave that reaches
/// the course's maximum grade is `2π·max_grade·v²/λ` — so at 105 m/s a 220 m
/// wave is nearly 3 g and the car is airborne over every crest, unable to steer,
/// in a section full of traffic. Six hundred metres is a swell the car stays
/// planted through and still visibly rolls over, which is what the old
/// generator's `max_grade_delta` was quietly buying: it capped how fast the
/// grade could change and therefore how hard a crest could throw the car.
const WAVELENGTH_M: f32 = 260.0;

/// The expected player speed the shipping course is paced for (m/s).
///
/// The car's natural top speed is a shade under 90 m/s and the ghost averages
/// about this, so it is what the traffic closing speeds and the boost budget are
/// measured against.
pub const EXPECTED_SPEED_MPS: f32 = 78.0;

/// Build the shipping course's specification for `seed`.
pub fn shipping_spec(seed: u64, tuning: &Tuning) -> crate::course::specification::CourseSpec {
    let course = &tuning.course;
    let flow = |vehicles_per_km: f32| -> Option<TrafficZoneSpec> {
        (vehicles_per_km > 0.0).then(|| TrafficZoneSpec {
            flow: Some(TrafficFlowSpec {
                // The old traffic was a fixed 85 m slot pitch with no variation
                // at all. The band around it is what the new model buys: the
                // same average density, with gaps worth aiming for.
                min_headway_m: 58.0,
                preferred_headway_m: 1_000.0 / vehicles_per_km,
                max_headway_m: 1_000.0 / vehicles_per_km * 1.6,
                speed_mps: ScalarRange::new(
                    tuning.race.traffic_speed_min,
                    tuning.race.traffic_speed_max,
                ),
                // The shipping course keeps the old road's *even* traffic: the
                // platoon, burst and corridor machinery is real and exercised
                // by `courses/burning_coast.brc`, but this course is a port of
                // a road whose cars were evenly spaced, and knots of three are
                // a difficulty it never had.
                platoon_probability: 0.0,
                platoon_size: CountRange::new(2, 3),
                platoon_gap_m: 52.0,
                open_corridor_every_m: ScalarRange::new(700.0, 1_100.0),
                open_corridor_length_m: 190.0,
                ..TrafficFlowSpec::at_density(vehicles_per_km)
            }),
            ..TrafficZoneSpec::default()
        })
    };

    let mut builder = CourseBuilder::new("burnt_rubber_default", seed)
        .defaults(CourseDefaults {
            lanes: 5,
            lane_width_m: course.lane_width,
            shoulder_width_m: course.lane_shoulder,
            expected_speed_mps: EXPECTED_SPEED_MPS,
            environment: SectionKind::StartStraight,
        })
        .thresholds(ValidationThresholds::DEFAULT);

    for pacing in &PACING {
        let id = pacing.environment.token();
        let mut traffic = flow(pacing.vehicles_per_km);
        // Two authored figures on the shipping course, both placed where the
        // road already has room for them. Everything else is ambient.
        if let (SectionKind::HighSpeedStraight, Some(zone)) =
            (pacing.environment, traffic.as_mut())
        {
            zone.encounters.push(EncounterSpec::RollingWall(RollingWallSpec {
                start_offset_m: 520.0,
                // Two of five lanes, so the wall is a thing to get round rather
                // than a thing to survive.
                wall_width_lanes: 2,
                open_lane: -1,
                opening_step_lanes: 1,
                phase_length_m: 190.0,
                phases: 3,
                speed_mps: 34.0,
                group_spacing_m: 190.0,
                reaction_distance_m: 150.0,
            }));
        }
        if let (SectionKind::Canyon, Some(zone)) = (pacing.environment, traffic.as_mut()) {
            zone.encounters.push(EncounterSpec::Slalom(SlalomSpec {
                start_offset_m: 380.0,
                blockers: 5,
                spacing_m: 95.0,
                lane_sequence: vec![-1, 1],
                speed_mps: 32.0,
                clearance_m: 0.85,
                recovery_gap_m: 140.0,
            }));
        }

        if pacing.curviness < 0.15 {
            // A straight section: the old generator's near-zero curviness
            // produced a road that reads as straight, so it is authored as one.
            let mut section = SectionSpec::new(
                SectionId::new(id),
                RoadPrimitiveSpec::Straight {
                    length_m: pacing.length_m,
                },
            )
            .with_lanes(pacing.lanes)
            .with_environment(pacing.environment)
            .with_expected_speed(EXPECTED_SPEED_MPS)
            .with_modifier(RoadModifierSpec::Banking {
                mode: BankingMode::Flat,
                strength: 0.0,
                maximum_rad: 0.0,
            });
            if let Some(zone) = traffic {
                section = section.with_traffic(zone);
            }
            section.pickups.extend_from_slice(pacing.pickups);
            builder = builder.push_section(section);
        } else {
            // Everything else is a motif: sweepers where the old profile wanted
            // long bends, a slalom of S-bends where it wanted tight alternating
            // ones.
            let tight = pacing.curviness >= 0.8;
            builder = builder.motif(MotifInvocation {
                id: SectionId::new(id),
                kind: tight
                    .then_some(MotifKind::AlternatingSlalom)
                    .unwrap_or(MotifKind::HighSpeedSweeps),
                params: MotifParams {
                    count: pacing.pieces,
                    length_m: pacing.length_m,
                    radius_m: ScalarRange::new(
                        radius_for_curviness(pacing.curviness, course),
                        radius_for_curviness(pacing.curviness * 0.62, course),
                    ),
                    bank_rad: ScalarRange::new(course.max_bank * 0.45, course.max_bank),
                    elevation_amplitude_m: amplitude_for_hilliness(
                        pacing.hilliness,
                        WAVELENGTH_M,
                        course,
                    ),
                    lateral_amplitude_m: 0.0,
                    wavelength_m: WAVELENGTH_M,
                    height_m: 0.0,
                    lanes: CountRange::exact(pacing.lanes),
                    narrow_lanes: pacing.lanes,
                },
                environment: Some(pacing.environment),
                expected_speed_mps: Some(EXPECTED_SPEED_MPS),
                traffic,
                pickups: pacing.pickups.to_vec(),
            });
        }
    }

    builder.build()
}

/// Compile the shipping course for `seed` under the shipping tuning.
pub fn shipping_plan(seed: u64) -> CourseResult<CoursePlan> {
    plan_for(seed, &Tuning::DEFAULT)
}

/// Compile the shipping course for `seed` under `tuning`.
pub fn plan_for(seed: u64, tuning: &Tuning) -> CourseResult<CoursePlan> {
    compiler::compile(&shipping_spec(seed, tuning), tuning)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::validation::BoostStatus;

    #[test]
    fn the_pacing_plan_is_the_one_the_course_has_always_had() {
        let environments: Vec<SectionKind> = PACING.iter().map(|p| p.environment).collect();
        assert_eq!(environments, SectionKind::ALL.to_vec());
        let total: f32 = PACING.iter().map(|p| p.length_m).sum();
        assert!(
            (8_000.0..=10_000.0).contains(&total),
            "the course is 8-10 km, got {total} m"
        );
        // The character ordering the old profiles had, still true.
        assert!(PACING[0].curviness < PACING[3].curviness, "the esses are curvier");
        assert!(PACING[2].hilliness > PACING[3].hilliness, "the ridge is hillier");
        assert!(
            PACING[5].lanes > PACING[6].lanes,
            "the long haul opens up and the canyon squeezes"
        );
        assert!(PACING.iter().all(|p| p.lanes % 2 == 1));
    }

    #[test]
    fn the_curviness_mapping_reproduces_the_old_generators_tightest_corner() {
        let course = CourseTuning::DEFAULT;
        // 40 m of arc through 0.115 rad is a 348 m radius.
        let tightest = radius_for_curviness(1.0, &course);
        assert!((tightest - 347.8).abs() < 1.0, "tightest radius {tightest} m");
        // A gentler section turns on a wider one.
        assert!(radius_for_curviness(0.5, &course) > tightest);
        assert!(radius_for_curviness(0.0, &course) > tightest);
    }

    #[test]
    fn the_hilliness_mapping_reaches_the_courses_maximum_grade() {
        let course = CourseTuning::DEFAULT;
        let amplitude = amplitude_for_hilliness(1.0, 220.0, &course);
        let peak_grade = amplitude * std::f32::consts::TAU / 220.0;
        assert!(
            (peak_grade - course.max_grade).abs() < 1.0e-4,
            "peaked at {peak_grade}, wanted {}",
            course.max_grade
        );
        assert_eq!(amplitude_for_hilliness(0.0, 220.0, &course), 0.0);
    }

    #[test]
    fn the_shipping_specification_validates_and_names_every_environment() {
        let spec = shipping_spec(crate::DEFAULT_SEED, &Tuning::DEFAULT);
        assert!(spec.validate().is_ok(), "{:?}", spec.validate());
        assert_eq!(spec.seed, crate::DEFAULT_SEED);
        assert_eq!(spec.items.len(), PACING.len());
    }

    #[test]
    fn the_shipping_course_compiles_to_the_advertised_road() {
        let plan = shipping_plan(crate::DEFAULT_SEED).expect("compiles");
        assert!(
            (8_000.0..=10_500.0).contains(&plan.length()),
            "the course is 8-10.5 km: {} m",
            plan.length()
        );
        assert!(plan.track().samples().len() > 4_000);
        assert_eq!(plan.track().seed(), crate::DEFAULT_SEED);
        // Every environment appears, in order.
        let mut seen: Vec<SectionKind> = Vec::new();
        plan.track().samples().iter().for_each(|s| {
            (seen.last() != Some(&s.section)).then(|| seen.push(s.section));
        });
        assert_eq!(seen, SectionKind::ALL.to_vec());
    }

    #[test]
    fn the_shipping_course_validates_without_errors() {
        let plan = shipping_plan(crate::DEFAULT_SEED).expect("compiles");
        assert!(
            !plan.report().has_errors(),
            "the shipping course does not validate:\n{}",
            plan.report().dump()
        );
        assert_ne!(plan.report().status, BoostStatus::Invalid);
        assert!(plan.report().metrics.vehicles > 60, "the road has traffic");
        assert!(!plan.encounters().is_empty(), "and authored figures on it");
        assert!(!plan.pickups().is_empty(), "and boost to pick up on it");
    }

    /// Every section that authored pickups got them, on the road, in a lane the
    /// road has — and the whole course still validates.
    #[test]
    fn the_shipping_course_places_its_pickups_on_the_road() {
        let plan = shipping_plan(crate::DEFAULT_SEED).expect("compiles");
        let authored: usize = PACING
            .iter()
            .flat_map(|p| p.pickups.iter())
            .map(|row| row.count.max(1) as usize)
            .sum();
        assert_eq!(
            plan.pickups().len(),
            authored,
            "every authored pickup compiles into exactly one"
        );
        assert_eq!(plan.report().metrics.pickups, authored);

        let track = plan.track();
        for pickup in plan.pickups() {
            let sample = track.sample_at(pickup.at_m);
            assert!(
                pickup.lane.abs() <= track.lane_reach(&sample),
                "{} is in lane {} where the road reaches {}",
                pickup.id,
                pickup.lane,
                track.lane_reach(&sample)
            );
            // On the tarmac, not on the verge.
            let lateral = track.lane_lateral(&sample, pickup.lane);
            assert!(
                lateral.abs() < sample.half_width,
                "{} is {:.1} m off centre on a {:.1} m half-width road",
                pickup.id,
                lateral,
                sample.half_width
            );
        }
        // Ascending, which the runtime's index and the collector both assume.
        assert!(plan
            .pickups()
            .windows(2)
            .all(|w| w[0].at_m <= w[1].at_m));
    }

    /// **Why an unreachable pickup is a warning and not an error.** A pickup is
    /// authored against the *road*, and the road's ambient traffic is drawn per
    /// seed. The compiler clears the cars that would sit on top of one
    /// (`PICKUP_KEEP_OUT_M`), which is what it can do without moving what the
    /// author wrote; whether every remaining lane is reachable at the expected
    /// speed then depends on the draw, and varies from seed to seed.
    ///
    /// So: **never an error, at any seed**. That is the property worth pinning —
    /// a warning count is a property of one traffic draw and would be a test
    /// that fails whenever the flow generator is touched.
    #[test]
    fn no_seed_produces_an_unplaceable_pickup() {
        for seed in [crate::DEFAULT_SEED, 1, 2, 7, 99, 12_345] {
            let plan = shipping_plan(seed).expect("compiles");
            assert!(!plan.pickups().is_empty(), "seed {seed} placed no pickups");
            let bad: Vec<String> = plan
                .report()
                .errors()
                .filter(|f| {
                    matches!(
                        f.error.code,
                        crate::course::CourseErrorCode::InvalidPickupLane
                            | crate::course::CourseErrorCode::OverlappingPickups
                    )
                })
                .map(|f| f.line())
                .collect();
            assert!(bad.is_empty(), "seed {seed}: {}", bad.join("\n"));
        }
    }

    /// The keep-out earns its place: an ambient car may not be parked in a
    /// pickup's own lane at the point the player meets it, because taking the
    /// pickup would then mean driving through the car.
    #[test]
    fn ambient_traffic_keeps_out_of_a_pickups_own_lane() {
        let plan = shipping_plan(crate::DEFAULT_SEED).expect("compiles");
        let tuning = Tuning::DEFAULT;
        let length = plan.length();
        for pickup in plan.pickups() {
            for vehicle in plan.traffic().iter().filter(|v| v.encounter.is_none()) {
                let meet = crate::course::traffic::meeting_distance(
                    vehicle.spawn_m,
                    vehicle.speed_mps,
                    tuning.race.traffic_ahead,
                    plan.track().sample_at(vehicle.spawn_m).expected_speed,
                    length,
                );
                assert!(
                    (vehicle.lane != pickup.lane) | ((meet - pickup.at_m).abs() >= 30.0),
                    "vehicle {} meets the player {:.0} m from {} in the same lane {}",
                    vehicle.id,
                    (meet - pickup.at_m).abs(),
                    pickup.id,
                    pickup.lane
                );
            }
        }
    }

    #[test]
    fn a_different_seed_is_a_different_road_with_the_same_shape() {
        let a = shipping_plan(11).expect("compiles");
        let b = shipping_plan(12).expect("compiles");
        assert_ne!(a.track().samples(), b.track().samples());
        assert_eq!(
            a.sections().len(),
            b.sections().len(),
            "the pacing plan fixes the section count"
        );
        let drift = a
            .track()
            .samples()
            .iter()
            .zip(b.track().samples())
            .map(|(p, q)| p.position.distance(q.position))
            .fold(0.0f32, f32::max);
        assert!(drift > 100.0, "the two courses diverge substantially: {drift} m");
    }

    #[test]
    fn the_shipping_course_is_a_pure_function_of_its_seed() {
        assert_eq!(
            shipping_plan(crate::DEFAULT_SEED).unwrap().dump(),
            shipping_plan(crate::DEFAULT_SEED).unwrap().dump()
        );
    }
}

