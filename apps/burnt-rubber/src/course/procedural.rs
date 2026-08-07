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
    BankingMode, CountRange, CourseBuilder, CourseDefaults, MotifInvocation, MotifKind,
    MotifParams, RoadModifierSpec, RoadPrimitiveSpec, ScalarRange, SectionId, SectionKind,
    SectionSpec, SlalomSpec, TrafficFlowSpec, TrafficZoneSpec, ValidationThresholds,
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
pub fn amplitude_for_hilliness(
    hilliness: f32,
    wavelength_m: f32,
    thresholds: &ValidationThresholds,
) -> f32 {
    hilliness.clamp(0.0, 1.0) * thresholds.max_grade * wavelength_m / std::f32::consts::TAU
}

/// What a section's road actually is.
///
/// The dispatch used to be a pair of thresholds on `curviness` — under 0.15 was
/// a straight, over 0.8 was a slalom — which meant a section could not be given
/// a shape without also being given a curviness that implied it. Naming the
/// shape is both clearer and the only way a figure like the corkscrew, whose
/// character is not a point on a curviness scale, can be asked for at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Level, straight road.
    Straight,
    /// Long alternating banked bends: [`MotifKind::HighSpeedSweeps`].
    Sweepers,
    /// Tight alternating S-bends: [`MotifKind::AlternatingSlalom`].
    Slalom,
    /// One continuous banked turn that descends under itself:
    /// [`MotifKind::Corkscrew`].
    Corkscrew,
}

/// One entry of the pacing plan: the character the old generator drew inside.
struct Pacing {
    environment: SectionKind,
    shape: Shape,
    length_m: f32,
    /// Peak bend severity, as the old `SectionProfile::curviness` — the radius a
    /// [`Shape::Sweepers`] or [`Shape::Slalom`] section turns on. Unread by the
    /// other shapes, which derive their geometry differently.
    curviness: f32,
    /// Peak hill severity, as the old `SectionProfile::hilliness`. Unread by
    /// [`Shape::Corkscrew`], whose descent is a stated drop rather than a wave.
    hilliness: f32,
    /// The total drop of a [`Shape::Corkscrew`] (m). Unread by the others.
    drop_m: f32,
    lanes: u32,
    /// How many figures the section is broken into — revolutions, for a
    /// corkscrew.
    pieces: u32,
    /// Vehicles per kilometre of ambient traffic, or zero for clear road.
    vehicles_per_km: f32,
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
        shape: Shape::Straight,
        length_m: 620.0,
        curviness: 0.10,
        hilliness: 0.0,
        drop_m: 0.0,
        lanes: 5,
        pieces: 1,
        // The opening is clear road: the countdown and the first acceleration
        // happen on empty tarmac, exactly as `traffic_clear_start` used to
        // guarantee.
        vehicles_per_km: 0.0,
    },
    Pacing {
        environment: SectionKind::SweepingBends,
        shape: Shape::Sweepers,
        length_m: 1_700.0,
        curviness: 0.78,
        hilliness: 0.28,
        drop_m: 0.0,
        lanes: 5,
        pieces: 5,
        vehicles_per_km: 11.8,
    },
    // **The corkscrew.** The road leaves the ridge by screwing its way down it
    // — one continuous banked turn, a full revolution, seventy metres of
    // descent, passing under its own entry on the way out. It replaces a run of
    // rolling crests, and it is the one section of the shipping course that
    // spends the full grade and bank envelope the course authors for itself.
    Pacing {
        environment: SectionKind::RollingHills,
        shape: Shape::Corkscrew,
        length_m: 1_250.0,
        curviness: 0.0,
        hilliness: 0.0,
        drop_m: 70.0,
        // Five lanes, where the crests it replaced had three. A sustained
        // maximum-lean descent held for nine hundred metres needs somewhere to
        // run wide *to*: on a three-lane road the usable width is barely four
        // metres once the car and its edge margin are taken out, and a driver
        // holding the cornering limit that long finds the guardrail rather than
        // the exit. Measured, the three-lane version cost the ghost five barrier
        // grinds inside the coil alone.
        lanes: 5,
        // Revolutions, for this shape. One is what makes the exit pass under
        // the entry; the radius is whatever that needs (about 112 m here).
        pieces: 1,
        vehicles_per_km: 6.0,
    },
    Pacing {
        environment: SectionKind::TechnicalBends,
        shape: Shape::Slalom,
        length_m: 1_150.0,
        curviness: 1.0,
        hilliness: 0.35,
        drop_m: 0.0,
        lanes: 3,
        pieces: 6,
        vehicles_per_km: 10.5,
    },
    Pacing {
        environment: SectionKind::Tunnel,
        shape: Shape::Sweepers,
        length_m: 780.0,
        curviness: 0.34,
        hilliness: 0.12,
        drop_m: 0.0,
        lanes: 3,
        pieces: 3,
        vehicles_per_km: 12.5,
    },
    Pacing {
        environment: SectionKind::HighSpeedStraight,
        shape: Shape::Sweepers,
        length_m: 1_500.0,
        curviness: 0.22,
        hilliness: 0.15,
        drop_m: 0.0,
        lanes: 5,
        pieces: 5,
        // "Wide, flat, flat-out — and full of traffic to thread."
        vehicles_per_km: 15.5,
    },
    Pacing {
        environment: SectionKind::Canyon,
        shape: Shape::Slalom,
        length_m: 1_100.0,
        curviness: 0.86,
        hilliness: 0.45,
        drop_m: 0.0,
        lanes: 3,
        pieces: 5,
        vehicles_per_km: 11.0,
    },
    Pacing {
        environment: SectionKind::FinalSweeps,
        shape: Shape::Sweepers,
        length_m: 850.0,
        curviness: 0.70,
        hilliness: 0.30,
        drop_m: 0.0,
        lanes: 5,
        pieces: 3,
        vehicles_per_km: 12.0,
    },
    Pacing {
        environment: SectionKind::Finish,
        shape: Shape::Straight,
        length_m: 320.0,
        curviness: 0.0,
        hilliness: 0.0,
        drop_m: 0.0,
        lanes: 5,
        pieces: 1,
        vehicles_per_km: 0.0,
    },
];

/// How far the shipping course's ordinary sweepers are allowed to lean (rad).
///
/// Named here rather than derived from the course's grade/bank envelope: the
/// envelope is what an *authored figure* may reach, and the sweepers are
/// deliberately kept to the gentler lean the road has always had.
const SWEEPER_BANK_MIN_RAD: f32 = 0.063;
/// See [`SWEEPER_BANK_MIN_RAD`].
const SWEEPER_BANK_MAX_RAD: f32 = 0.14;

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

        if pacing.shape == Shape::Straight {
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
            builder = builder.push_section(section);
        } else {
            // Everything else is a motif, named by the section rather than
            // inferred from a number.
            builder = builder.motif(MotifInvocation {
                id: SectionId::new(id),
                kind: match pacing.shape {
                    Shape::Slalom => MotifKind::AlternatingSlalom,
                    Shape::Corkscrew => MotifKind::Corkscrew,
                    Shape::Straight | Shape::Sweepers => MotifKind::HighSpeedSweeps,
                },
                params: MotifParams {
                    count: pacing.pieces,
                    length_m: pacing.length_m,
                    // A corkscrew *derives* its radius from the road it has
                    // and the revolutions it was asked for, and reads this only
                    // as the floor it may not go under. Handing it a curviness
                    // band would floor it at a seven-kilometre radius and
                    // quietly straighten the figure out.
                    radius_m: (pacing.shape == Shape::Corkscrew)
                        .then_some(ScalarRange::exact(
                            ValidationThresholds::DEFAULT.min_turn_radius_m,
                        ))
                        .unwrap_or(ScalarRange::new(
                            radius_for_curviness(pacing.curviness, course),
                            radius_for_curviness(pacing.curviness * 0.62, course),
                        )),
                    // A corkscrew is the one figure allowed the full envelope:
                    // a banked descent that is not leaning is just a long
                    // corner you happen to be falling down.
                    bank_rad: (pacing.shape == Shape::Corkscrew)
                        .then_some(ScalarRange::new(
                            SWEEPER_BANK_MIN_RAD,
                            ValidationThresholds::DEFAULT.max_bank_rad,
                        ))
                        .unwrap_or(ScalarRange::new(
                            SWEEPER_BANK_MIN_RAD,
                            SWEEPER_BANK_MAX_RAD,
                        )),
                    elevation_amplitude_m: amplitude_for_hilliness(
                        pacing.hilliness,
                        WAVELENGTH_M,
                        &ValidationThresholds::DEFAULT,
                    ),
                    lateral_amplitude_m: 0.0,
                    wavelength_m: WAVELENGTH_M,
                    height_m: pacing.drop_m,
                    lanes: CountRange::exact(pacing.lanes),
                    narrow_lanes: pacing.lanes,
                },
                environment: Some(pacing.environment),
                expected_speed_mps: Some(EXPECTED_SPEED_MPS),
                traffic,
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
        // The character ordering the old profiles had, still true where the
        // shape still reads a curviness.
        assert!(PACING[0].curviness < PACING[3].curviness, "the esses are curvier");
        assert!(
            PACING[5].lanes > PACING[6].lanes,
            "the long haul opens up and the canyon squeezes"
        );
        assert!(PACING.iter().all(|p| p.lanes % 2 == 1));
        // The third section is the corkscrew, and it is the only one.
        assert_eq!(PACING[2].shape, Shape::Corkscrew);
        assert_eq!(
            PACING.iter().filter(|p| p.shape == Shape::Corkscrew).count(),
            1
        );
        assert!(PACING[2].drop_m > 0.0, "and it actually descends");
        // Only the corkscrew states a drop; the rest roll on waves.
        PACING
            .iter()
            .filter(|p| p.shape != Shape::Corkscrew)
            .for_each(|p| assert_eq!(p.drop_m, 0.0));
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
        let thresholds = ValidationThresholds::DEFAULT;
        let amplitude = amplitude_for_hilliness(1.0, 220.0, &thresholds);
        let peak_grade = amplitude * std::f32::consts::TAU / 220.0;
        assert!(
            (peak_grade - thresholds.max_grade).abs() < 1.0e-4,
            "peaked at {peak_grade}, wanted {}",
            thresholds.max_grade
        );
        assert_eq!(amplitude_for_hilliness(0.0, 220.0, &thresholds), 0.0);
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
    }

    /// **The corkscrew, on the road the game ships.** One continuous turn in
    /// the third section, banked past what an ordinary sweeper is allowed,
    /// dropping far enough that the road passes under its own entry.
    #[test]
    fn the_third_section_screws_its_way_down_the_ridge() {
        let plan = shipping_plan(crate::DEFAULT_SEED).expect("compiles");
        let coil = plan
            .sections()
            .iter()
            .find(|s| s.id.as_str().ends_with("/coil"))
            .expect("the shipping course has a corkscrew");
        assert_eq!(coil.environment, SectionKind::RollingHills, "the third section");
        assert_eq!(coil.primitive, "turn");

        let track = plan.track();
        let samples: Vec<&crate::track::TrackSample> = track
            .samples()
            .iter()
            .filter(|s| (s.distance >= coil.start_m) & (s.distance < coil.end_m))
            .collect();

        // One revolution, one way round the whole time.
        let turned: f32 = samples.windows(2).map(|w| w[1].heading - w[0].heading).sum();
        assert!(
            turned.abs() > 5.6,
            "the coil turns {:.2} rad, which is not a revolution",
            turned.abs()
        );
        assert!(
            samples.iter().all(|s| s.curvature * turned >= -1.0e-4),
            "the coil reverses — that is a slalom, not a screw"
        );

        // It descends the whole way, by about what it was asked for.
        let drop = samples[0].position.y - samples.last().unwrap().position.y;
        assert!((55.0..=75.0).contains(&drop), "the coil dropped {drop:.1} m");
        assert!(
            samples.windows(2).all(|w| w[1].position.y <= w[0].position.y + 0.05),
            "the coil climbs somewhere"
        );

        // It leans harder than the course's ordinary sweepers are allowed to.
        let lean = samples.iter().map(|s| s.bank.abs()).fold(0.0f32, f32::max);
        assert!(
            lean > SWEEPER_BANK_MAX_RAD,
            "the corkscrew leans {:.1} deg, no more than a sweeper",
            lean.to_degrees()
        );
        assert!(lean <= ValidationThresholds::DEFAULT.max_bank_rad + 1.0e-3);

        // And the exit passes under the entry: somewhere in the figure the road
        // is close to itself horizontally and far from itself vertically.
        let under = samples.iter().enumerate().any(|(i, a)| {
            samples.iter().skip(i + 200).any(|b| {
                let flat = ((a.position.x - b.position.x).powi(2)
                    + (a.position.z - b.position.z).powi(2))
                .sqrt();
                (flat < 30.0) & ((a.position.y - b.position.y).abs() > 25.0)
            })
        });
        assert!(under, "the corkscrew never passes under itself");
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
