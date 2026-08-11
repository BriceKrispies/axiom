//! **Course preparation** — compiling the authored course into the immutable
//! runtime plan the whole race reads.
//!
//! This is the first task in the schedule ([`super::RacePreparation::tasks`])
//! because everything else derives from what it produces: the road mesh is cut
//! from the compiled track, and the simulation, ghost and HUD all address the
//! course by distance along that same centreline.
//!
//! The task body is currently an inert placeholder — the scaffold is in place
//! and the generation still runs where it always did, so the game's behaviour
//! is unchanged. Moving `plan_for` in here is a separate, single-owner change.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_runtime::{PreparationTask, RuntimeResult};

use crate::tuning::Tuning;

/// The compiled-course product of the preparation phase.
#[derive(Debug, Clone, Default)]
pub struct PreparedCourse {}

/// Compiles the course once, at startup, into [`PreparedCourse`].
///
/// It carries the `seed` **and** the `tuning` because the course generator
/// needs both, and because `BurntRubber::with_profile` takes an arbitrary
/// caller-supplied `Tuning` — it is not a constant that could be read back
/// from anywhere else.
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
        *self.out.borrow_mut() = Some(PreparedCourse::default());
        Ok(())
    }
}
