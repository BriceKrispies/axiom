//! Holds the startup preparation tasks a launch must run, in push order.

use crate::preparation_task::PreparationTask;
use crate::runtime_error::RuntimeError;

/// A pushed task together with the static name reported if it fails.
///
/// Held privately — callers add work through [`PreparationSchedule::push`] and
/// learn about failure through the name returned by the crate-private executor.
struct Registered {
    name: &'static str,
    task: Box<dyn PreparationTask>,
}

/// A deterministic schedule of startup preparation work, ordered by push order.
///
/// # Why this is not shaped like [`crate::runtime_scheduler::RuntimeScheduler`]
///
/// The scheduler is a **long-lived, multi-writer registry**: systems arrive
/// from independent layers at arbitrary moments across the runtime's whole
/// life, nobody controls the interleaving, and ids are read back afterwards.
/// A stable id and an explicit order value are what make its execution order a
/// function of *configuration* rather than of who happened to call first.
///
/// None of that holds here. A schedule is built at exactly one site, populated
/// in a straight line, moved into the runtime by value and dropped. Push order
/// therefore already *is* a deterministic total order — with no id that could
/// collide, no order key that could tie, and no sort to apply. Push order is in
/// fact the *stronger* guarantee: with an ordering key a later caller could
/// legally schedule itself in front of the engine's own work, whereas here it
/// simply cannot.
///
/// The named struct is kept rather than a bare `Vec` because it carries the
/// `&'static str` names the failure protocol reports, makes the by-value move
/// into the runtime meaningful, and hosts the hand-written [`std::fmt::Debug`]
/// that `Box<dyn PreparationTask>` cannot derive.
pub struct PreparationSchedule {
    entries: Vec<Registered>,
}

impl std::fmt::Debug for PreparationSchedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparationSchedule")
            .field(
                "tasks",
                &self.entries.iter().map(|e| e.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Default for PreparationSchedule {
    fn default() -> Self {
        PreparationSchedule::new()
    }
}

impl PreparationSchedule {
    /// An empty schedule.
    pub fn new() -> Self {
        PreparationSchedule {
            entries: Vec::new(),
        }
    }

    /// Append `task` under `name`, to run after everything already pushed.
    ///
    /// Infallible by construction: there is exactly one writer, so there is no
    /// duplicate to detect, and push order is the execution order, so there is
    /// nothing to sort.
    pub fn push(&mut self, name: &'static str, task: Box<dyn PreparationTask>) {
        self.entries.push(Registered { name, task });
    }

    /// Run every task in push order, stopping at the first failure.
    ///
    /// Returns the failing task's name **and its own [`RuntimeError`]** — so
    /// the caller can report both *which* task failed and *why* — or `None`
    /// when every task succeeded, including for an empty schedule. Tasks after
    /// the failing one do not run.
    ///
    /// Crate-private deliberately: only the runtime's own lifecycle may drive a
    /// schedule. A public executor would let a caller run preparation without
    /// ever touching the lifecycle it exists to gate.
    pub(crate) fn execute(&mut self) -> Option<(&'static str, RuntimeError)> {
        self.entries
            .iter_mut()
            .find_map(|entry| entry.task.prepare().err().map(|cause| (entry.name, cause)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_error_code::RuntimeErrorCode;
    use crate::runtime_result::RuntimeResult;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Trace = Rc<RefCell<Vec<&'static str>>>;

    /// Appends its name to a shared trace, so execution order is observable.
    struct TraceTask {
        name: &'static str,
        trace: Trace,
    }

    impl PreparationTask for TraceTask {
        fn prepare(&mut self) -> RuntimeResult<()> {
            self.trace.borrow_mut().push(self.name);
            Ok(())
        }
    }

    /// Records that it ran, then fails with its own distinguishable code.
    struct FailTask {
        name: &'static str,
        trace: Trace,
        code: RuntimeErrorCode,
    }

    impl PreparationTask for FailTask {
        fn prepare(&mut self) -> RuntimeResult<()> {
            self.trace.borrow_mut().push(self.name);
            Err(RuntimeError::new(self.code, "intentional"))
        }
    }

    fn tracer(name: &'static str, trace: &Trace) -> Box<dyn PreparationTask> {
        Box::new(TraceTask {
            name,
            trace: trace.clone(),
        })
    }

    #[test]
    fn tasks_run_in_push_order() {
        let trace: Trace = Rc::new(RefCell::new(Vec::new()));
        let mut schedule = PreparationSchedule::new();
        schedule.push("a", tracer("a", &trace));
        schedule.push("b", tracer("b", &trace));
        schedule.push("c", tracer("c", &trace));

        assert_eq!(schedule.execute(), None, "every task succeeded");
        assert_eq!(*trace.borrow(), vec!["a", "b", "c"]);
    }

    #[test]
    fn execute_stops_at_the_first_failure_and_reports_it() {
        let trace: Trace = Rc::new(RefCell::new(Vec::new()));
        let mut schedule = PreparationSchedule::new();
        schedule.push("a", tracer("a", &trace));
        schedule.push(
            "boom",
            Box::new(FailTask {
                name: "boom",
                trace: trace.clone(),
                code: RuntimeErrorCode::KernelFailure,
            }),
        );
        schedule.push("c", tracer("c", &trace));

        let failure = schedule.execute().expect("the middle task fails");
        assert_eq!(failure.0, "boom", "the failing task's name is reported");
        assert_eq!(
            failure.1.code(),
            RuntimeErrorCode::KernelFailure,
            "the task's own error is preserved, not replaced"
        );
        assert_eq!(
            *trace.borrow(),
            vec!["a", "boom"],
            "the task after the failure did not run"
        );
    }

    #[test]
    fn execute_on_an_empty_schedule_succeeds() {
        let mut schedule = PreparationSchedule::new();
        assert_eq!(schedule.execute(), None);
    }

    #[test]
    fn the_schedule_and_its_debug_are_constructible() {
        let trace: Trace = Rc::new(RefCell::new(Vec::new()));
        let mut schedule = PreparationSchedule::default();
        assert_eq!(
            format!("{schedule:?}"),
            "PreparationSchedule { tasks: [] }",
            "an empty default schedule debugs as empty"
        );

        schedule.push("author", tracer("author", &trace));
        schedule.push("course", tracer("course", &trace));
        assert_eq!(
            format!("{schedule:?}"),
            r#"PreparationSchedule { tasks: ["author", "course"] }"#,
            "Debug lists the task names in push order"
        );
    }
}
