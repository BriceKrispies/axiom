//! The trait a single unit of startup-only preparation work implements.

use crate::runtime_result::RuntimeResult;

/// A unit of startup-only work, supplied from above and opaque to the runtime.
///
/// The runtime knows nothing about what a task prepares. It knows only that the
/// task runs once, to completion, before the simulation may begin stepping, and
/// that it either succeeded or failed.
///
/// # The zero-argument signature is load-bearing
///
/// `prepare` takes nothing but `&mut self`, while
/// [`crate::runtime_system::RuntimeSystem::run`] takes
/// `&mut RuntimeContext<'_>`. The two traits are therefore structurally
/// incompatible: a `PreparationTask` can never be handed to
/// [`crate::runtime_scheduler::RuntimeScheduler::register`], and a
/// `RuntimeSystem` can never be pushed onto a
/// [`crate::preparation_schedule::PreparationSchedule`]. Startup work leaking
/// into the frame loop — or frame work leaking into startup — is a compile
/// error, enforced by the type system rather than by developer discipline.
///
/// The signature also denies a task any tick, command queue, event queue,
/// clock or telemetry sink. Nothing about the running simulation is observable
/// from inside preparation, so a task cannot depend on gameplay state that does
/// not exist yet.
///
/// # Products never flow through the runtime
///
/// `prepare` returns only success or failure. A task writes what it produced
/// into storage its own constructor captured, so the runtime owns the *fact*
/// that preparation completed while the caller owns the *data*.
pub trait PreparationTask {
    /// Run this task's startup work to completion.
    ///
    /// Returning `Err` aborts the whole preparation phase: the tasks pushed
    /// after this one do not run, and the runtime becomes terminally failed.
    /// An application cannot survive a world that was never built, so there is
    /// deliberately no "continue on error" mode here — unlike
    /// [`crate::runtime_config::RuntimeConfig::fail_on_system_error`], which
    /// governs *per-step* systems.
    fn prepare(&mut self) -> RuntimeResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_error::RuntimeError;
    use crate::runtime_error_code::RuntimeErrorCode;

    /// A task that records how many times it ran into state it owns itself —
    /// the storage-captured-by-the-constructor shape the trait documents.
    struct Counting {
        runs: u32,
    }

    impl PreparationTask for Counting {
        fn prepare(&mut self) -> RuntimeResult<()> {
            self.runs += 1;
            Ok(())
        }
    }

    struct Failing;

    impl PreparationTask for Failing {
        fn prepare(&mut self) -> RuntimeResult<()> {
            Err(RuntimeError::new(
                RuntimeErrorCode::SystemFailed,
                "intentional",
            ))
        }
    }

    #[test]
    fn a_task_mutates_only_the_state_it_owns() {
        let mut task = Counting { runs: 0 };
        task.prepare().unwrap();
        task.prepare().unwrap();
        assert_eq!(task.runs, 2);
    }

    #[test]
    fn a_failing_task_returns_a_typed_runtime_error() {
        let mut task = Failing;
        let err = task.prepare().unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::SystemFailed);
    }

    #[test]
    fn a_task_is_usable_behind_a_box() {
        let mut boxed: Box<dyn PreparationTask> = Box::new(Counting { runs: 0 });
        assert!(boxed.prepare().is_ok());
    }
}
