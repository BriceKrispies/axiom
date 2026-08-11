//! Determinism and ownership properties of the startup preparation phase.
//!
//! Three facts this file pins, none of which the type system alone can state:
//!
//! 1. **Replay** — equivalent seeds and configuration produce byte-equal
//!    prepared products.
//! 2. **Ownership** — the runtime owns the *fact* that preparation completed;
//!    the caller owns the *data*. Scratch state dies at the barrier because the
//!    schedule is moved into `prepare` by value and dropped there.
//! 3. **The empty-cell hazard** — a task that reads a product an earlier task
//!    should have written, but finds it absent, must return `Err` and fail the
//!    phase through the normal protocol. **Never a panic**: a panic would go
//!    straight through `Runtime::prepare`, bypass the failure protocol, leave
//!    the lifecycle un-settled, and abort the process on `wasm32`.
//!
//! Like the lifecycle proof, this file names no engine-domain concept.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use axiom_kernel::DeterministicRng;
use axiom_runtime::{
    PreparationSchedule, PreparationTask, Runtime, RuntimeConfig, RuntimeError, RuntimeErrorCode,
    RuntimeResult, RuntimeState,
};

const TABLE_LEN: usize = 256;

/// A product cell. `Option` — never a defaultable bare `T` — so that a
/// premature read is an unmistakable `None` rather than a plausible empty value.
type TableCell = Rc<RefCell<Option<Vec<u64>>>>;

/// A cell for a value *derived* from another task's product.
type SumCell = Rc<RefCell<Option<u64>>>;

fn empty_table() -> TableCell {
    Rc::new(RefCell::new(None))
}

/// The producer: generates a deterministic table from its seed.
struct Producer {
    seed: u64,
    out: TableCell,
}

impl PreparationTask for Producer {
    fn prepare(&mut self) -> RuntimeResult<()> {
        let mut rng = DeterministicRng::seeded(self.seed);
        *self.out.borrow_mut() = Some((0..TABLE_LEN).map(|_| rng.next_u64()).collect());
        Ok(())
    }
}

/// The consumer: folds the producer's table into a single derived value.
///
/// This is the shape every dependent task must take. The `ok_or_else` is the
/// point of the whole struct — `.expect()` here would panic through
/// `Runtime::prepare` instead of failing the phase.
struct Consumer {
    source: TableCell,
    derived: SumCell,
}

impl PreparationTask for Consumer {
    fn prepare(&mut self) -> RuntimeResult<()> {
        self.source
            .borrow()
            .as_ref()
            .ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorCode::PreparationFailed,
                    "the derived value requires the produced table",
                )
            })
            .map(|table| {
                let folded = table.iter().fold(0u64, |acc, v| acc.wrapping_add(*v));
                *self.derived.borrow_mut() = Some(folded);
            })
    }
}

/// Working state a task needs only while it runs. Counts its own destruction so
/// a test can observe that the barrier really disposed of it.
struct Scratch {
    drops: Rc<Cell<u32>>,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

/// A task that allocates scratch state, writes a product, and keeps the scratch
/// alive for exactly as long as it itself is alive.
struct ProducerWithScratch {
    seed: u64,
    out: TableCell,
    scratch: Scratch,
}

impl PreparationTask for ProducerWithScratch {
    fn prepare(&mut self) -> RuntimeResult<()> {
        let mut rng = DeterministicRng::seeded(self.seed);
        // The scratch is genuinely used, not merely held: it perturbs nothing
        // observable, but reading it here proves it is live during the phase.
        let live = self.scratch.drops.get();
        *self.out.borrow_mut() = Some(
            (0..TABLE_LEN)
                .map(|_| rng.next_u64().wrapping_add(u64::from(live)))
                .collect(),
        );
        Ok(())
    }
}

fn initialized() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig::new(16_666_667)).expect("a 60 Hz step is valid");
    runtime.initialize().expect("a fresh runtime initializes");
    runtime
}

#[test]
fn equivalent_inputs_produce_equivalent_prepared_output() {
    // Two independent runtimes, two independently-constructed schedules holding
    // the same tasks with the same seeds. Nothing is shared between the arms
    // except the seed values themselves.
    let build = |seed: u64| -> (Vec<u64>, u64) {
        let table = empty_table();
        let derived: SumCell = Rc::new(RefCell::new(None));
        let mut runtime = initialized();

        let mut schedule = PreparationSchedule::new();
        schedule.push(
            "producer",
            Box::new(Producer {
                seed,
                out: Rc::clone(&table),
            }),
        );
        schedule.push(
            "consumer",
            Box::new(Consumer {
                source: Rc::clone(&table),
                derived: Rc::clone(&derived),
            }),
        );

        runtime.prepare(schedule).expect("every task succeeds");
        assert_eq!(runtime.state(), RuntimeState::Prepared);

        let produced = table.borrow().as_ref().expect("the table exists").clone();
        let folded = derived.borrow().expect("the derived value exists");
        (produced, folded)
    };

    let first = build(0x0B17_4E7A_5C09_1D33);
    let second = build(0x0B17_4E7A_5C09_1D33);
    assert_eq!(first.0.len(), TABLE_LEN);
    assert_eq!(first, second, "equivalent inputs prepared equal products");

    let different = build(0x0B17_4E7A_5C09_1D34);
    assert_ne!(
        first, different,
        "a different seed is a different application, not the same one"
    );
}

#[test]
fn temporary_preparation_data_is_discarded_at_the_barrier() {
    let drops = Rc::new(Cell::new(0u32));
    let table = empty_table();
    let mut runtime = initialized();

    let mut schedule = PreparationSchedule::new();
    schedule.push(
        "producer",
        Box::new(ProducerWithScratch {
            seed: 11,
            out: Rc::clone(&table),
            scratch: Scratch {
                drops: Rc::clone(&drops),
            },
        }),
    );

    assert_eq!(drops.get(), 0, "the scratch is alive before the phase");

    runtime.prepare(schedule).expect("every task succeeds");

    assert_eq!(
        drops.get(),
        1,
        "the schedule moved into prepare by value and died there, taking every \
         task's scratch state with it"
    );
    assert_eq!(
        table.borrow().as_ref().expect("the product survives").len(),
        TABLE_LEN,
        "while the product the task wrote outlives the barrier"
    );
    assert_eq!(runtime.state(), RuntimeState::Prepared);
}

#[test]
fn products_reach_the_caller_without_passing_through_the_runtime() {
    let table = empty_table();
    let mut runtime = initialized();

    let mut schedule = PreparationSchedule::new();
    schedule.push(
        "caller-owned-product",
        Box::new(Producer {
            seed: 5,
            out: Rc::clone(&table),
        }),
    );

    assert_eq!(
        runtime.prepare(schedule),
        Ok(()),
        "prepare returns the fact that the phase completed and nothing else — \
         there is no report and no product channel through the runtime"
    );
    assert_eq!(
        table.borrow().as_ref().expect("the product exists").len(),
        TABLE_LEN,
        "the caller's own cell holds it"
    );
    assert!(
        !format!("{runtime:?}").contains("caller-owned-product"),
        "the runtime retained no trace of the schedule or its products; its \
         complete self-description is lifecycle, timeline, scheduler and queues"
    );
}

#[test]
fn a_task_that_reads_an_unwritten_product_fails_the_phase() {
    let table = empty_table();
    let derived: SumCell = Rc::new(RefCell::new(None));
    let mut runtime = initialized();

    // Deliberately inverted: the consumer is pushed *before* its producer, so
    // it reads a cell nobody has written yet.
    let mut schedule = PreparationSchedule::new();
    schedule.push(
        "consumer",
        Box::new(Consumer {
            source: Rc::clone(&table),
            derived: Rc::clone(&derived),
        }),
    );
    schedule.push(
        "producer",
        Box::new(Producer {
            seed: 5,
            out: Rc::clone(&table),
        }),
    );

    // Reaching this line at all is half the assertion: a `.expect()` inside the
    // consumer would have unwound through `Runtime::prepare` instead.
    let failure = runtime
        .prepare(schedule)
        .expect_err("the consumer found an empty cell");

    assert_eq!(
        failure.code(),
        RuntimeErrorCode::PreparationFailed,
        "the task's own diagnosis is preserved"
    );
    assert_eq!(
        failure.message(),
        "consumer",
        "and the phase names the task that failed"
    );
    assert_eq!(
        runtime.state(),
        RuntimeState::Failed,
        "an out-of-order schedule settles the lifecycle deterministically, \
         it does not panic"
    );
    assert!(
        derived.borrow().is_none(),
        "the consumer wrote nothing from a product it never had"
    );
    assert!(
        table.borrow().is_none(),
        "and the producer after it never ran"
    );
    assert_eq!(
        runtime.start().unwrap_err().code(),
        RuntimeErrorCode::InvalidLifecycleTransition,
        "so the simulation is unreachable"
    );
}
