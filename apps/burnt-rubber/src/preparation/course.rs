//! **Course preparation** — compiling the authored course into the immutable
//! runtime plan the whole race reads.
//!
//! This is the first task in the schedule ([`super::RacePreparation::tasks`])
//! because everything else derives from what it produces: the road mesh is cut
//! from the compiled track, and the simulation, ghost and HUD all address the
//! course by distance along that same centreline.
//!
//! # Why this is the largest thing the phase does
//!
//! Compiling the shipping course is the single most expensive piece of startup
//! work in the game: nine kilometres of road integrated from an authored
//! specification, then traffic flow, authored encounters, boost pickups,
//! near-miss windows, a traversability analysis and a full validation pass.
//! Before this task existed the app paid for it **four times** per
//! construction-plus-restart cycle — once for the player's simulation, once for
//! the ghost's, and once more for each on a restart — because every `RaceSim`
//! constructor compiled its own copy.
//!
//! The plan is shared as an `Arc` rather than recompiled, which is safe for a
//! reason the type system already guarantees: [`CoursePlan`] exposes no
//! `&mut self` method and holds no interior mutability, so two simulations
//! reading one plan cannot observe each other.
//!
//! # The `tuning` field, and a trap worth naming
//!
//! The task carries the seed **and** the tuning because
//! `BurntRubber::with_profile` takes an arbitrary caller-supplied [`Tuning`] —
//! it is not a constant that could be read back from somewhere else.
//!
//! What makes preparing the course *before* the window has been sized safe is
//! that the generator reads `tuning.course`, `tuning.race` and `tuning.vehicle`
//! and never `tuning.camera` — and `tuning.camera` is the only field
//! `with_profile` rewrites (via `CameraTuning::framed_for_aspect`). If a future
//! change moves an aspect-derived or device-derived value into any of the three
//! fields the generator *does* read, a prepared course would silently stop
//! matching the one the old inline path produced.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use axiom_runtime::{PreparationTask, RuntimeError, RuntimeErrorCode, RuntimeResult};

use crate::course::runtime::CoursePlan;
use crate::tuning::Tuning;

/// The compiled-course product of the preparation phase.
///
/// Deliberately not `Default`: a course that was never compiled is not an empty
/// course, it is the absence of one, and that absence is already modelled by the
/// `Option` in [`super::RacePreparation::course`].
#[derive(Debug, Clone)]
pub struct PreparedCourse {
    plan: Arc<CoursePlan>,
}

impl PreparedCourse {
    /// The compiled course. Cloning the returned handle is an `Arc` bump, never
    /// a recompile — which is the whole point of preparing it once.
    pub fn plan(&self) -> Arc<CoursePlan> {
        Arc::clone(&self.plan)
    }
}

/// Compiles the course once, at startup, into [`PreparedCourse`].
#[derive(Debug)]
pub struct CourseTask {
    /// The seed the course is generated from.
    pub seed: u64,
    /// The full tuning surface the generator reads its constraints from.
    pub tuning: Tuning,
    /// The cell this task writes its product into.
    pub out: Rc<RefCell<Option<PreparedCourse>>>,
}

impl PreparationTask for CourseTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        // A course that will not compile fails the phase. It must not panic:
        // the old inline path did (`the shipping course must compile`), and a
        // panic here would unwind straight through `Runtime::prepare`, skip the
        // `Failed` transition entirely, and abort the process on `wasm32`.
        // Failing properly is what lets the barrier keep its promise that a
        // failed preparation can never reach `Running`.
        crate::course::procedural::plan_for(self.seed, &self.tuning)
            .map_err(|_| {
                RuntimeError::new(
                    RuntimeErrorCode::PreparationFailed,
                    "burnt-rubber/course failed to compile",
                )
            })
            .map(|plan| {
                *self.out.borrow_mut() = Some(PreparedCourse {
                    plan: Arc::new(plan),
                });
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEFAULT_SEED;

    fn prepared() -> PreparedCourse {
        let out = Rc::new(RefCell::new(None));
        let mut task = CourseTask {
            seed: DEFAULT_SEED,
            tuning: Tuning::DEFAULT,
            out: Rc::clone(&out),
        };
        task.prepare().expect("the shipping course compiles");
        let product = out.borrow_mut().take();
        product.expect("the task wrote its product")
    }

    /// The task produces the real shipping course, not a placeholder.
    #[test]
    fn preparing_produces_the_shipping_course() {
        let plan = prepared().plan();
        assert_eq!(plan.seed(), DEFAULT_SEED);
        assert!(
            (8_000.0..=10_500.0).contains(&plan.length()),
            "the demo course is 8-10 km: {} m",
            plan.length()
        );
        assert!(plan.track().samples().len() > 4_000);
        assert!(!plan.report().has_errors());
    }

    /// The determinism the whole baseline rests on: same seed, same course.
    #[test]
    fn two_preparations_from_the_same_seed_are_identical() {
        let a = prepared().plan();
        let b = prepared().plan();
        assert_eq!(a.length(), b.length());
        assert_eq!(a.track().samples().len(), b.track().samples().len());
        assert_eq!(a.track().samples(), b.track().samples());
    }

    /// The prepared plan is shared, not copied. This is what removes three of
    /// the four compiles: the ghost and a restart take an `Arc` bump.
    #[test]
    fn the_prepared_plan_is_shared_not_copied() {
        let course = prepared();
        assert!(
            Arc::ptr_eq(&course.plan(), &course.plan()),
            "two reads of one prepared course are the same allocation"
        );
    }

    /// A prepared plan builds a simulation indistinguishable from the one the
    /// inline constructor produced — the property that lets manifest 11 swap
    /// them without moving a golden byte.
    #[test]
    fn a_prepared_plan_builds_the_same_sim_as_with_profile() {
        let prepared_sim = crate::sim::RaceSim::from_plan(
            prepared().plan(),
            Tuning::DEFAULT,
            crate::PlayProfile::Wheel,
        );
        let inline_sim = crate::sim::RaceSim::with_profile(
            DEFAULT_SEED,
            Tuning::DEFAULT,
            crate::PlayProfile::Wheel,
        );
        assert_eq!(prepared_sim.car(), inline_sim.car());
        assert_eq!(prepared_sim.track().length(), inline_sim.track().length());
        assert_eq!(
            prepared_sim.camera_pose(0.0),
            inline_sim.camera_pose(0.0),
            "the chase camera starts in the same place"
        );
    }
}
