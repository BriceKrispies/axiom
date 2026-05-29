//! The trait every deterministic runtime system implements.

use crate::runtime_context::RuntimeContext;
use crate::runtime_result::RuntimeResult;

/// A deterministic, scheduler-driven unit of work.
///
/// Implementations are pure transformations over their own state and the
/// [`RuntimeContext`]: same input state and same context → same outcome. The
/// trait deliberately exposes no host, rendering, or world surface — only the
/// runtime substrate.
pub trait RuntimeSystem {
    /// Execute this system for one runtime step.
    ///
    /// Returning `Err` is recorded in the step's diagnostics. Whether the
    /// runtime stops on the first error or continues with the next system is
    /// governed by the [`crate::runtime_config::RuntimeConfig::fail_on_system_error`] flag.
    fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_command_queue::RuntimeCommandQueue;
    use crate::runtime_error::RuntimeError;
    use crate::runtime_error_code::RuntimeErrorCode;
    use crate::runtime_event_queue::RuntimeEventQueue;
    use crate::runtime_step::RuntimeStep;
    use axiom_kernel::{FrameIndex, KernelApi, Tick};

    /// Trivial system that counts how many times it ran.
    struct Counter {
        runs: u32,
    }

    impl RuntimeSystem for Counter {
        fn run(&mut self, _ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            self.runs += 1;
            Ok(())
        }
    }

    /// Trivial system that always fails — proves the trait carries `RuntimeResult`.
    struct Failing;

    impl RuntimeSystem for Failing {
        fn run(&mut self, _ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            Err(RuntimeError::new(
                RuntimeErrorCode::SystemFailed,
                "intentional",
            ))
        }
    }

    fn fresh_ctx<'a>(
        commands: &'a mut RuntimeCommandQueue,
        events: &'a mut RuntimeEventQueue,
        kernel: &'a KernelApi,
        logs: &'a mut axiom_kernel::InMemoryLogSink,
        tel: &'a mut axiom_kernel::InMemoryTelemetrySink,
    ) -> RuntimeContext<'a> {
        RuntimeContext::new(
            RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 1_000, 0),
            commands,
            events,
            kernel,
            logs,
            tel,
        )
    }

    #[test]
    fn a_passing_system_increments_its_own_state() {
        let kernel = KernelApi::new();
        let (mut cmds, mut evts) = (RuntimeCommandQueue::new(), RuntimeEventQueue::new());
        let (mut logs, mut tel) = (kernel.log_sink(), kernel.telemetry_sink());
        let mut sys = Counter { runs: 0 };
        for _ in 0..3 {
            let mut ctx = fresh_ctx(&mut cmds, &mut evts, &kernel, &mut logs, &mut tel);
            sys.run(&mut ctx).unwrap();
        }
        assert_eq!(sys.runs, 3);
    }

    #[test]
    fn a_failing_system_returns_typed_error() {
        let kernel = KernelApi::new();
        let (mut cmds, mut evts) = (RuntimeCommandQueue::new(), RuntimeEventQueue::new());
        let (mut logs, mut tel) = (kernel.log_sink(), kernel.telemetry_sink());
        let mut sys = Failing;
        let mut ctx = fresh_ctx(&mut cmds, &mut evts, &kernel, &mut logs, &mut tel);
        let err = sys.run(&mut ctx).unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::SystemFailed);
    }
}
