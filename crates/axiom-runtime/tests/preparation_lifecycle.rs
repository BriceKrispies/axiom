//! The generic-application proof of the startup preparation barrier.
//!
//! This file deliberately names **no** engine-domain concept — no mesh, no
//! texture, no scene, no course, no game. The runtime layer knows only that
//! *some* application had *some* expensive startup work to do; a deterministic
//! table generated from a seeded kernel RNG stands in for whatever that work
//! actually produces. If a future reader can tell from this file what kind of
//! application Axiom builds, the abstraction has leaked.
//!
//! Compiled as its own crate, so it sees only the public runtime surface.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_kernel::DeterministicRng;
use axiom_runtime::{
    PreparationSchedule, PreparationTask, Runtime, RuntimeConfig, RuntimeError, RuntimeErrorCode,
    RuntimeResult, RuntimeState,
};

/// How many entries the stand-in product holds.
const TABLE_LEN: usize = 1024;

/// The product cell shape every task in this file writes through.
///
/// `Option` is load-bearing, not decoration: a bare `Vec<u64>` cell would read
/// as an empty-but-plausible table before its producer ran, and an application
/// would build something from it and never notice. `None` is unmistakable.
type TableCell = Rc<RefCell<Option<Vec<u64>>>>;

/// Expensive startup work, standing in for any application's own.
struct BuildTable {
    seed: u64,
    out: TableCell,
}

impl PreparationTask for BuildTable {
    fn prepare(&mut self) -> RuntimeResult<()> {
        let mut rng = DeterministicRng::seeded(self.seed);
        *self.out.borrow_mut() = Some((0..TABLE_LEN).map(|_| rng.next_u64()).collect());
        Ok(())
    }
}

/// Startup work that cannot complete — the application's world is unbuildable.
struct UnbuildableTable;

impl PreparationTask for UnbuildableTable {
    fn prepare(&mut self) -> RuntimeResult<()> {
        Err(RuntimeError::new(
            RuntimeErrorCode::PreparationFailed,
            "the input this task needs does not exist",
        ))
    }
}

fn empty_cell() -> TableCell {
    Rc::new(RefCell::new(None))
}

/// An `Initialized` runtime on a 60 Hz fixed step.
fn initialized() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig::new(16_666_667)).expect("a 60 Hz step is valid");
    runtime.initialize().expect("a fresh runtime initializes");
    runtime
}

#[test]
fn a_generic_application_prepares_then_runs() {
    let table = empty_cell();
    let mut runtime = initialized();

    let mut schedule = PreparationSchedule::new();
    schedule.push(
        "table",
        Box::new(BuildTable {
            seed: 42,
            out: Rc::clone(&table),
        }),
    );

    assert!(
        table.borrow().is_none(),
        "nothing is built before prepare runs"
    );
    assert_eq!(
        runtime.step().unwrap_err().code(),
        RuntimeErrorCode::StepWhileNotRunning,
        "and the frame loop is closed while it is unbuilt"
    );

    runtime.prepare(schedule).expect("every task succeeds");
    assert_eq!(runtime.state(), RuntimeState::Prepared);
    assert_eq!(
        table.borrow().as_ref().expect("the product exists").len(),
        TABLE_LEN,
        "the product is complete at the barrier, not partially built"
    );

    runtime.start().expect("a prepared runtime starts");
    assert_eq!(runtime.state(), RuntimeState::Running);

    let sequences: Vec<u64> = (0..10)
        .map(|_| {
            runtime
                .step()
                .expect("a running runtime steps")
                .step()
                .sequence()
        })
        .collect();
    assert_eq!(
        sequences,
        (1..=10).collect::<Vec<u64>>(),
        "ten steps advanced the simulation deterministically"
    );
    assert_eq!(
        table.borrow().as_ref().expect("the product survives").len(),
        TABLE_LEN,
        "stepping did not rebuild the product"
    );
}

#[test]
fn the_frame_loop_is_closed_until_prepared() {
    let table = empty_cell();
    let mut runtime = Runtime::new(RuntimeConfig::new(16_666_667)).expect("a 60 Hz step is valid");

    assert_eq!(runtime.state(), RuntimeState::Created);
    assert_eq!(
        runtime.step().unwrap_err().code(),
        RuntimeErrorCode::StepWhileNotRunning,
        "Created cannot step"
    );

    runtime.initialize().expect("a fresh runtime initializes");
    assert_eq!(runtime.state(), RuntimeState::Initialized);
    assert_eq!(
        runtime.step().unwrap_err().code(),
        RuntimeErrorCode::StepWhileNotRunning,
        "Initialized cannot step — preparation has not run"
    );

    let mut schedule = PreparationSchedule::new();
    schedule.push(
        "table",
        Box::new(BuildTable {
            seed: 7,
            out: Rc::clone(&table),
        }),
    );
    runtime.prepare(schedule).expect("every task succeeds");
    assert_eq!(runtime.state(), RuntimeState::Prepared);
    assert_eq!(
        runtime.step().unwrap_err().code(),
        RuntimeErrorCode::StepWhileNotRunning,
        "Prepared still cannot step — the barrier is crossed by start(), not by prepare()"
    );

    runtime.start().expect("a prepared runtime starts");
    assert_eq!(
        runtime.step().expect("Running steps").step().sequence(),
        1,
        "Running is the only state that steps"
    );

    runtime.pause().expect("a running runtime pauses");
    assert_eq!(
        runtime.step().unwrap_err().code(),
        RuntimeErrorCode::StepWhileNotRunning,
        "Paused cannot step"
    );
}

#[test]
fn a_generic_application_with_a_failing_task_never_runs() {
    let table = empty_cell();
    let mut runtime = initialized();

    let mut schedule = PreparationSchedule::new();
    schedule.push("unbuildable", Box::new(UnbuildableTable));
    schedule.push(
        "table",
        Box::new(BuildTable {
            seed: 42,
            out: Rc::clone(&table),
        }),
    );

    let failure = runtime
        .prepare(schedule)
        .expect_err("the first task cannot complete");
    assert_eq!(failure.code(), RuntimeErrorCode::PreparationFailed);
    assert_eq!(
        failure.message(),
        "unbuildable",
        "the error names the task that failed"
    );

    assert_eq!(runtime.state(), RuntimeState::Failed);
    assert!(
        table.borrow().is_none(),
        "the task after the failure never ran, so its product is absent"
    );
    assert_eq!(
        runtime.start().unwrap_err().code(),
        RuntimeErrorCode::InvalidLifecycleTransition,
        "a failed preparation phase can never be started"
    );
    assert_eq!(
        runtime.step().unwrap_err().code(),
        RuntimeErrorCode::StepWhileNotRunning,
        "and it can never step"
    );
}
