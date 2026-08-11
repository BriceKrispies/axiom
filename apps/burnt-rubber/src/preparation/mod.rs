//! **The startup preparation phase** — every expensive, gameplay-independent
//! thing Burnt Rubber builds once, before the race may begin stepping.
//!
//! The engine owns the *barrier*: `Runtime::prepare` runs a
//! [`axiom_runtime::PreparationSchedule`] to completion and only then may
//! `start()` reach `Running`. It owns nothing about *what* is prepared — a
//! compiled course, an asphalt albedo and a road mesh are racing concepts, so
//! they live here, in the app, and the runtime never learns them.
//!
//! # The shape
//!
//! ```text
//!   RacePreparation            one product cell per domain
//!        │                     Rc<RefCell<Option<T>>>
//!        │ tasks(seed, tuning)
//!        ▼
//!   [course, textures, meshes] ── App::prepare_with ──▶ PreparationSchedule
//!                                                              │
//!                                                     Runtime::prepare
//!                                                              ▼
//!   every cell is Some(…)  ══ PREPARATION BARRIER ══   the race may step
//! ```
//!
//! # Why the products are cells, not return values
//!
//! [`axiom_runtime::PreparationTask::prepare`] takes nothing but `&mut self`
//! and returns only success or failure, so a product can never flow *through*
//! the runtime. A task writes into storage its constructor captured. That
//! storage is `Rc<RefCell<Option<T>>>` and the `Option` is load-bearing: a
//! consumer that reads a cell too early sees `None` and fails the phase, where
//! a bare defaultable `T` would hand it a plausible-looking empty value that
//! builds empty geometry and renders without erring.
//!
//! # Push order is the only ordering
//!
//! There is no id, no order key and no dependency graph — the real chain is a
//! straight line and push order expresses it exactly. [`RacePreparation::tasks`]
//! returns the three domain tasks in that line: **course → textures → meshes**.
//! The mesh task reads what the course task wrote, which is why it holds the
//! course *cell* and not a `Track`: the schedule is assembled before
//! `Runtime::prepare` runs, so at construction time the course does not exist
//! yet. It exists only after the course task has executed inside that same
//! `prepare()` call, and the mesh task therefore reads the cell from inside its
//! own `prepare()`.
//!
//! # This module is frozen
//!
//! The three product types and the three task field lists above are a contract
//! three separate streams write against. `mod.rs` declares all three submodules
//! and returns all three tasks; each domain file is filled in by exactly one
//! stream, and none of them may change the push order or a task's fields.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_runtime::PreparationTask;

use crate::tuning::Tuning;

pub mod course;
pub mod meshes;
pub mod textures;

/// The name the course task is scheduled and reported under.
const COURSE_TASK_NAME: &str = "burnt-rubber/course";
/// The name the texture task is scheduled and reported under.
const TEXTURE_TASK_NAME: &str = "burnt-rubber/textures";
/// The name the mesh task is scheduled and reported under.
const MESH_TASK_NAME: &str = "burnt-rubber/meshes";

/// Every product the startup phase yields, shared with the code that consumes
/// it.
///
/// One cell per domain; each is `Option` so a premature read fails the phase
/// rather than yielding a plausible empty value. Cloning a `RacePreparation`
/// clones the *handles*, not the products — the composition root keeps one
/// clone to read after the barrier while the tasks hold the others.
#[derive(Debug, Clone, Default)]
pub struct RacePreparation {
    /// The compiled course, written by [`course::CourseTask`].
    pub course: Rc<RefCell<Option<course::PreparedCourse>>>,
    /// The synthesized albedo textures, written by [`textures::TextureTask`].
    pub textures: Rc<RefCell<Option<textures::PreparedTextures>>>,
    /// The generated geometry, written by [`meshes::MeshTask`].
    pub meshes: Rc<RefCell<Option<meshes::PreparedMeshes>>>,
}

impl RacePreparation {
    /// Three empty product cells.
    pub fn new() -> Self {
        RacePreparation::default()
    }

    /// The three domain tasks in push order: course → textures → meshes.
    ///
    /// The composition root passes each pair to `App::prepare_with`. **Push
    /// order is execution order** — there is no order key, so a caller cannot
    /// reorder them and the mesh task is guaranteed to run after the course
    /// task that fills the cell it reads.
    pub fn tasks(
        &self,
        seed: u64,
        tuning: &Tuning,
    ) -> [(&'static str, Box<dyn PreparationTask>); 3] {
        [
            (
                COURSE_TASK_NAME,
                Box::new(course::CourseTask {
                    seed,
                    tuning: *tuning,
                    out: Rc::clone(&self.course),
                }),
            ),
            (
                TEXTURE_TASK_NAME,
                Box::new(textures::TextureTask {
                    out: Rc::clone(&self.textures),
                }),
            ),
            (
                MESH_TASK_NAME,
                Box::new(meshes::MeshTask {
                    course: Rc::clone(&self.course),
                    tuning: tuning.course,
                    out: Rc::clone(&self.meshes),
                }),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::FIXED_STEP_NANOS;
    use crate::DEFAULT_SEED;
    use axiom_runtime::{PreparationSchedule, Runtime, RuntimeConfig};

    fn schedule_for(preparation: &RacePreparation) -> PreparationSchedule {
        let mut schedule = PreparationSchedule::new();
        preparation
            .tasks(DEFAULT_SEED, &Tuning::DEFAULT)
            .into_iter()
            .for_each(|(name, task)| schedule.push(name, task));
        schedule
    }

    #[test]
    fn a_fresh_preparation_has_no_products() {
        let preparation = RacePreparation::new();
        assert!(preparation.course.borrow().is_none());
        assert!(preparation.textures.borrow().is_none());
        assert!(preparation.meshes.borrow().is_none());
        // `new` and `default` are the same three empty cells.
        let defaulted = RacePreparation::default();
        assert!(defaulted.course.borrow().is_none());
        assert!(defaulted.textures.borrow().is_none());
        assert!(defaulted.meshes.borrow().is_none());
    }

    #[test]
    fn tasks_are_returned_in_push_order() {
        let preparation = RacePreparation::new();
        let names: Vec<&'static str> = preparation
            .tasks(DEFAULT_SEED, &Tuning::DEFAULT)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            names,
            vec![
                "burnt-rubber/course",
                "burnt-rubber/textures",
                "burnt-rubber/meshes",
            ],
            "push order is execution order: the mesh task reads what the \
             course task writes"
        );
    }

    #[test]
    fn preparing_fills_every_product_cell() {
        let preparation = RacePreparation::new();
        let schedule = schedule_for(&preparation);

        let mut runtime =
            Runtime::new(RuntimeConfig::new(FIXED_STEP_NANOS)).expect("the fixed step is valid");
        runtime.initialize().expect("a fresh runtime initializes");
        runtime.prepare(schedule).expect("every task succeeds");

        assert!(preparation.course.borrow().is_some());
        assert!(preparation.textures.borrow().is_some());
        assert!(preparation.meshes.borrow().is_some());

        // And the barrier is cleared: `start` is reachable only from `Prepared`.
        runtime.start().expect("a prepared runtime starts");
    }

    #[test]
    fn the_mesh_task_holds_the_course_cell() {
        let preparation = RacePreparation::new();
        assert_eq!(Rc::strong_count(&preparation.course), 1);

        let tasks = preparation.tasks(DEFAULT_SEED, &Tuning::DEFAULT);
        // Two of the three tasks hold the course cell — the course task that
        // writes it and the mesh task that reads it — so it has three owners
        // while the schedule is alive. Every other cell has exactly two.
        assert_eq!(Rc::strong_count(&preparation.course), 3);
        assert_eq!(Rc::strong_count(&preparation.textures), 2);
        assert_eq!(Rc::strong_count(&preparation.meshes), 2);
        drop(tasks);
        assert_eq!(Rc::strong_count(&preparation.course), 1);

        // The cell is the *same* cell, not a private copy: a mesh task built by
        // hand from the preparation's course handle points at the same storage.
        let task = meshes::MeshTask {
            course: Rc::clone(&preparation.course),
            tuning: Tuning::DEFAULT.course,
            out: Rc::clone(&preparation.meshes),
        };
        assert!(Rc::ptr_eq(&task.course, &preparation.course));
    }
}
