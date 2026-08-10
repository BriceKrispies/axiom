//! **Authoring**: the textual course DSL, and the demo course written in it.
//!
//! The language is small on purpose (see [`parser`] for what it deliberately
//! cannot do). Its whole job is to say the things a course says — metadata,
//! defaults, sections, primitives, modifiers, traffic zones, encounters,
//! near-miss windows, motifs, bounded repetition — and to say them somewhere a
//! person can read and diff.
//!
//! It produces exactly the [`CourseSpec`] a programmatic builder produces, and
//! goes through the identical compiler. There is no "parsed course" type and no
//! second pipeline.
//!
//! ```text
//! source text ──lex──▶ tokens ──parse──▶ CourseSpec ──compile──▶ CoursePlan
//! ```

pub mod lexer;
pub mod parser;

use crate::course::compiler;
use crate::course::error::CourseResult;
use crate::course::runtime::CoursePlan;
use crate::course::specification::CourseSpec;
use crate::tuning::Tuning;

pub use lexer::{tokenise, Token, TokenKind};
pub use parser::{parse, MAX_REPEAT};

/// The demo course, compiled into the binary.
///
/// It is `include_str!`'d rather than read from disk because the app has to run
/// in a browser, where there is no filesystem — and because a course the game
/// ships with is part of the game, not a data file a player could be missing.
pub const BURNING_COAST_SOURCE: &str = include_str!("../../../courses/burning_coast.brc");

/// The demo course's source name, used in its diagnostics.
pub const BURNING_COAST_NAME: &str = "burning_coast.brc";

/// Parse the demo course.
pub fn burning_coast_spec() -> CourseResult<CourseSpec> {
    parse(BURNING_COAST_NAME, BURNING_COAST_SOURCE)
}

/// Parse and compile the demo course under `tuning`.
pub fn burning_coast_plan(tuning: &Tuning) -> CourseResult<CoursePlan> {
    compiler::compile(&burning_coast_spec()?, tuning)
}

/// Parse and compile an arbitrary course source.
pub fn compile_source(name: &str, source: &str, tuning: &Tuning) -> CourseResult<CoursePlan> {
    compiler::compile(&parse(name, source)?, tuning)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{
        CourseBuilder, CourseDefaults, CourseItem, MotifInvocation, MotifKind, MotifParams,
        RoadPrimitiveSpec, ScalarRange, SectionId, SectionKind, SectionSpec,
    };

    #[test]
    fn the_example_course_parses() {
        let spec = burning_coast_spec().expect("the demo course parses");
        assert_eq!(spec.name, "burning_coast");
        assert_eq!(spec.seed, 84_192);
        assert!(spec.items.len() >= 5, "{} items", spec.items.len());
        assert_eq!(spec.defaults.lanes, 3);
        assert!((spec.defaults.lane_width_m - 3.8).abs() < 1.0e-5);
        assert!((spec.defaults.expected_speed_mps - 80.467_2).abs() < 1.0e-2);
    }

    #[test]
    fn the_example_course_compiles_into_a_real_plan() {
        let plan = burning_coast_plan(&Tuning::DEFAULT).expect("the demo course compiles");
        assert!(plan.length() > 2_500.0, "{} m", plan.length());
        assert!(plan.track().samples().len() > 1_000);
        assert!(!plan.traffic().is_empty(), "it has traffic");
        assert!(!plan.encounters().is_empty(), "and an authored figure");
        assert!(
            !plan.near_miss_windows().is_empty(),
            "and compiled opportunities"
        );
        // The worked example uses every construct the grammar has, and `pickups`
        // is one of them. All three tiers appear, so a change that broke one
        // tier's parsing could not pass this.
        let tiers: Vec<crate::course::specification::BoostTier> = {
            let mut t: Vec<_> = plan.pickups().iter().map(|p| p.tier).collect();
            t.sort_unstable();
            t.dedup();
            t
        };
        assert_eq!(
            tiers,
            crate::course::specification::BoostTier::ALL.to_vec(),
            "the example course does not author every tier"
        );
        assert!(
            plan.pickups().len() > tiers.len(),
            "and at least one of them is a row rather than a single"
        );
        // Compiling it twice is byte-identical.
        assert_eq!(
            plan.dump(),
            burning_coast_plan(&Tuning::DEFAULT).unwrap().dump()
        );
    }

    /// **The equivalence the whole authoring layer rests on.** The parser is
    /// not a second way to build a course; it is another front end onto the one
    /// authored representation.
    #[test]
    fn parsing_and_the_programmatic_builder_produce_the_same_specification() {
        let parsed = parse(
            "equivalence.brc",
            r#"
            course "equivalence" {
                seed = 4242
                defaults {
                    lanes = 3
                    lane_width = 3.8m
                    shoulder_width = 1.5m
                    expected_speed = 180mph
                    environment = coast_placeholder_replaced_below
                }
            }
            "#,
        );
        // (The above deliberately names an environment that does not exist, to
        // prove the equivalence test below is not accidentally passing on a
        // course that failed to parse.)
        assert!(parsed.is_err());

        let source = r#"
            course "equivalence" {
                seed = 4242
                defaults {
                    lanes = 3
                    lane_width = 3.8m
                    shoulder_width = 1.5m
                    expected_speed = 180mph
                    environment = sweeping_bends
                }
                straight { id = "opening" length = 500m }
                motif high_speed_sweeps {
                    id = "sweeps"
                    count = 4
                    length = 1200m
                    radius = 200m..320m
                }
                crest { id = "blind_crest" length = 180m height = 6m }
            }
        "#;
        let parsed = parse("equivalence.brc", source).expect("parses");

        let built = CourseBuilder::new("equivalence", 4_242)
            .defaults(CourseDefaults {
                lanes: 3,
                lane_width_m: 3.8,
                shoulder_width_m: 1.5,
                expected_speed_mps: 180.0 * crate::course::specification::units::MPH_TO_MPS,
                environment: SectionKind::SweepingBends,
            })
            .push_section(SectionSpec::new(
                SectionId::new("opening"),
                RoadPrimitiveSpec::Straight { length_m: 500.0 },
            ))
            .motif(MotifInvocation {
                id: SectionId::new("sweeps"),
                kind: MotifKind::HighSpeedSweeps,
                params: MotifParams {
                    count: 4,
                    length_m: 1_200.0,
                    radius_m: ScalarRange::new(200.0, 320.0),
                    ..MotifParams::DEFAULT
                },
                environment: None,
                expected_speed_mps: None,
                traffic: None,
                pickups: Vec::new(),
            })
            .push_section(SectionSpec::new(
                SectionId::new("blind_crest"),
                RoadPrimitiveSpec::Crest {
                    length_m: 180.0,
                    height_m: 6.0,
                },
            ))
            .build();

        assert_eq!(parsed.name, built.name);
        assert_eq!(parsed.seed, built.seed);
        assert_eq!(parsed.defaults.lanes, built.defaults.lanes);
        assert_eq!(parsed.defaults.environment, built.defaults.environment);
        assert!(
            (parsed.defaults.expected_speed_mps - built.defaults.expected_speed_mps).abs() < 1.0e-3
        );
        assert_eq!(parsed.items.len(), built.items.len());
        // Compare item by item, tolerating the float conversion on lane width.
        parsed
            .items
            .iter()
            .zip(&built.items)
            .for_each(|(a, b)| match (a, b) {
                (CourseItem::Section(x), CourseItem::Section(y)) => {
                    assert_eq!(x.id, y.id);
                    assert_eq!(x.primitive, y.primitive);
                    assert_eq!(x.modifiers, y.modifiers);
                }
                (CourseItem::Motif(x), CourseItem::Motif(y)) => {
                    assert_eq!(x.id, y.id);
                    assert_eq!(x.kind, y.kind);
                    assert_eq!(x.params, y.params);
                }
                (x, y) => panic!("item kinds differ: {x:?} vs {y:?}"),
            });

        // And, the point of all of it: both compile to the same plan.
        let from_source = compile_source("equivalence.brc", source, &Tuning::DEFAULT).unwrap();
        let from_builder = compiler::compile(&built, &Tuning::DEFAULT).unwrap();
        assert_eq!(from_source.dump(), from_builder.dump());
    }

    #[test]
    fn a_bad_source_fails_compilation_rather_than_producing_a_broken_course() {
        assert!(compile_source("bad.brc", "not a course at all", &Tuning::DEFAULT).is_err());
        assert!(compile_source(
            "bad.brc",
            "course \"x\" { seed = 1 straight { length = -5m } }",
            &Tuning::DEFAULT
        )
        .is_err());
    }
}
