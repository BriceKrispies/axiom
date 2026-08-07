//! **The course system**: authored road and traffic in, an immutable runtime
//! plan out.
//!
//! Burnt Rubber's course used to be a procedure — a bespoke control-point walk
//! with the pacing plan baked into an enum, and traffic that was an arithmetic
//! function of a slot index. Both were deterministic and both worked, and
//! neither could be *authored*: there was nowhere to say "put a zipper here",
//! nowhere to ask "is this passable", and no way to write a course down.
//!
//! This module is the replacement, and it is a **compiler**:
//!
//! ```text
//!   course source (.brc)                    programmatic builder
//!            │                                        │
//!            └──────────────┬─────────────────────────┘
//!                           ▼
//!                      CourseSpec                 (specification/)
//!                           │  expand motifs and groups
//!                           ▼
//!                     ExpandedCourse              (compiler/)
//!                    ┌──────┴───────┐
//!            geometry/              traffic/
//!         Track + sections      vehicles + encounters + windows
//!                    └──────┬───────┘
//!                           ▼
//!                       validation/               (grid, budget, ghost)
//!                           ▼
//!                      CoursePlan                 (runtime/)
//!                           │
//!                  RaceSim, every frame
//! ```
//!
//! Three rules hold the shape together, and every one of them is load-bearing:
//!
//! * **Distance is the coordinate.** Everything authored is stated in metres
//!   along the course, and everything compiled is addressed by them. There is no
//!   authoring interface in world space at all — a world-space control point is
//!   a *result*, and an author editing results cannot be told their corner is
//!   too tight, because "too tight" is a property of a radius.
//! * **Compilation happens once.** The runtime reads a sorted array through a
//!   bucket index. It never parses, never re-expands, never re-validates, and
//!   cannot: by the time it has a [`CoursePlan`](runtime::CoursePlan) the spec
//!   is gone.
//! * **Nothing here is gameplay-neutral.** Motifs, encounters, near-miss
//!   opportunities, the boost budget and the difficulty weights are all *this
//!   game's opinions*, which is exactly why they live in the app and not in an
//!   engine layer.

pub mod authoring;
pub mod compiler;
pub mod error;
pub mod geometry;
pub mod motifs;
pub mod procedural;
pub mod runtime;
pub mod specification;
pub mod traffic;
pub mod validation;

pub use compiler::{compile, compile_valid, expand, ExpandedCourse, ExpandedSection};
pub use error::{CourseError, CourseErrorCode, CourseResult, SourceLocation};
pub use runtime::CoursePlan;
pub use specification::CourseSpec;
pub use validation::{BoostStatus, ValidationReport};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    /// The pipeline, end to end, on the course the game actually ships.
    #[test]
    fn the_shipping_course_goes_all_the_way_through_the_pipeline() {
        let spec = procedural::shipping_spec(crate::DEFAULT_SEED, &Tuning::DEFAULT);
        assert!(spec.validate().is_ok());

        let expanded = expand(&spec).expect("expands");
        assert!(expanded.sections.len() > spec.items.len(), "motifs expanded");
        assert!(!expanded.zones.is_empty(), "and the course has traffic");

        let plan = compile(&spec, &Tuning::DEFAULT).expect("compiles");
        assert!(plan.length() > 8_000.0);
        assert!(!plan.traffic().is_empty());
        assert!(!plan.near_miss_windows().is_empty());
        assert!(!plan.report().sections.is_empty());
        assert!(
            !plan.report().has_errors(),
            "the shipping course must validate:\n{}",
            plan.report().dump()
        );
    }

    /// A course written in the DSL goes through the identical pipeline.
    #[test]
    fn a_text_course_goes_through_the_same_pipeline() {
        let plan = authoring::burning_coast_plan(&Tuning::DEFAULT).expect("compiles");
        assert!(plan.length() > 2_000.0);
        assert!(!plan.encounters().is_empty());
        assert_eq!(plan.seed(), 84_192);
        // Nothing in the compiled plan records that a motif or a group existed.
        assert!(plan
            .sections()
            .iter()
            .all(|s| !s.primitive.is_empty()));
    }

    #[test]
    fn the_strict_door_reports_every_error_rather_than_the_first() {
        let source = r#"
            course "walled" {
                seed = 1
                straight {
                    id = "run"
                    length = 2000m
                    lanes = 3
                    traffic {
                        flow {
                            vehicles_per_km = 90
                            min_headway = 8m
                            preferred_headway = 9m
                            max_headway = 10m
                            speed = 12mps..13mps
                        }
                    }
                }
            }
        "#;
        let spec = authoring::parse("walled.brc", source).expect("parses");
        let err = compile_valid(&spec, &Tuning::DEFAULT).unwrap_err();
        assert_eq!(err.code, CourseErrorCode::UntraversableEncounter);
        assert!(
            err.message.lines().count() > 2,
            "the message lists every error, not the first: {}",
            err.message
        );
    }
}
