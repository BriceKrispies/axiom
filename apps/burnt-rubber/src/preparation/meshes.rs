//! **Mesh preparation** — cutting the road, its paint and the scenery props
//! into geometry once, at startup.
//!
//! This is the last task in the schedule ([`super::RacePreparation::tasks`])
//! because it is the one with a real input dependency: the road is cut from the
//! track the course task compiled.
//!
//! # Why this task holds the course *cell*
//!
//! The schedule is assembled **before** `Runtime::prepare` runs, so at the
//! moment a `MeshTask` is constructed the compiled course does not exist — it
//! comes into being only when the course task executes, inside that same
//! `prepare()` call. A `MeshTask` that took a `Track` at construction would
//! therefore be unbuildable. It takes the shared cell instead and reads it from
//! inside its own [`PreparationTask::prepare`], where the read is guaranteed to
//! happen after the course task wrote. If it is somehow still `None`, the right
//! answer is an `Err` that fails the phase — never a panic through
//! `Runtime::prepare`, and never a plausible empty default.
//!
//! The task body is currently an inert placeholder — the scaffold is in place
//! and the generation still runs where it always did, so the game's behaviour
//! is unchanged.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_runtime::{PreparationTask, RuntimeResult};

use crate::preparation::course;
use crate::tuning::CourseTuning;

/// The generated-geometry product of the preparation phase.
#[derive(Debug, Clone, Default)]
pub struct PreparedMeshes {}

/// Builds the road, paint and prop geometry into [`PreparedMeshes`].
#[derive(Debug)]
pub struct MeshTask {
    /// The **read** cell: the course task, pushed earlier, fills this.
    pub course: Rc<RefCell<Option<course::PreparedCourse>>>,
    /// The course tuning the road cross-section is cut to.
    pub tuning: CourseTuning,
    /// The cell this task writes its product into.
    pub out: Rc<RefCell<Option<PreparedMeshes>>>,
}

impl PreparationTask for MeshTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        *self.out.borrow_mut() = Some(PreparedMeshes::default());
        Ok(())
    }
}
