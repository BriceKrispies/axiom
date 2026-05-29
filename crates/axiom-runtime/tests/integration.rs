//! Integration tests for `axiom-runtime`.
//!
//! These compile as a separate crate and therefore see only the public Layer 01
//! surface, mirroring how a future Layer 02 will consume the runtime. They
//! prove the runtime's deterministic guarantees end-to-end.

use std::sync::{Arc, Mutex};

use axiom_kernel::{HandleId, Tick};
use axiom_runtime::{
    Runtime, RuntimeCommand, RuntimeConfig, RuntimeContext, RuntimeError, RuntimeErrorCode,
    RuntimeEvent, RuntimeResult, RuntimeState, RuntimeSystem,
};

/// A trace system that records (step sequence, name) into a shared buffer and
/// pushes one command per run, proving both system ordering and command
/// production are deterministic across steps.
struct Trace {
    name: &'static str,
    trace: Arc<Mutex<Vec<(u64, &'static str)>>>,
}

impl RuntimeSystem for Trace {
    fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
        // Take an immutable snapshot of the step first, so the borrow on `ctx`
        // is released before the subsequent mutable borrows of its queues.
        let step = ctx.step();
        self.trace
            .lock()
            .unwrap()
            .push((step.sequence(), self.name));
        ctx.commands_mut().push(RuntimeCommand::new(
            0xC0DE,
            step.tick(),
            vec![self.name.len() as u8],
        ));
        ctx.events_mut()
            .push(RuntimeEvent::new(0xE0DE, step.tick(), vec![]));
        Ok(())
    }
}

fn started(config: RuntimeConfig) -> Runtime {
    let mut rt = Runtime::new(config).unwrap();
    rt.initialize().unwrap();
    rt.start().unwrap();
    rt
}

#[test]
fn full_lifecycle_and_deterministic_replay() {
    let trace_a = Arc::new(Mutex::new(Vec::new()));
    let trace_b = Arc::new(Mutex::new(Vec::new()));

    let build = |trace: Arc<Mutex<Vec<(u64, &'static str)>>>| -> Runtime {
        let mut rt = started(RuntimeConfig::new(1_000));
        rt.scheduler_mut()
            .register(
                HandleId::from_raw(20),
                "b",
                20,
                Box::new(Trace {
                    name: "b",
                    trace: trace.clone(),
                }),
            )
            .unwrap();
        rt.scheduler_mut()
            .register(
                HandleId::from_raw(10),
                "a",
                10,
                Box::new(Trace { name: "a", trace }),
            )
            .unwrap();
        rt
    };

    let mut a = build(trace_a.clone());
    let mut b = build(trace_b.clone());

    let mut last_a = None;
    let mut last_b = None;
    for _ in 0..8 {
        last_a = Some(a.step().unwrap());
        last_b = Some(b.step().unwrap());
    }
    let last_a = last_a.unwrap();
    let last_b = last_b.unwrap();

    // Step identity matches byte-for-byte.
    assert_eq!(last_a.step(), last_b.step());
    assert_eq!(last_a.step().tick(), Tick::new(8));
    assert_eq!(last_a.step().sequence(), 8);

    // The two trace logs are identical and follow scheduled order, repeated.
    let trace_a = trace_a.lock().unwrap().clone();
    let trace_b = trace_b.lock().unwrap().clone();
    assert_eq!(trace_a, trace_b);
    assert_eq!(trace_a[0], (1, "a"));
    assert_eq!(trace_a[1], (1, "b"));
    assert_eq!(trace_a[14], (8, "a"));

    // Pause -> Stop is a legal terminal transition.
    a.pause().unwrap();
    assert_eq!(a.state(), RuntimeState::Paused);
    a.stop().unwrap();
    assert_eq!(a.state(), RuntimeState::Stopped);
}

#[test]
fn queue_ordering_is_fifo_across_systems() {
    struct Pusher {
        kind: u32,
    }
    impl RuntimeSystem for Pusher {
        fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            let tick = ctx.step().tick();
            ctx.commands_mut()
                .push(RuntimeCommand::new(self.kind, tick, vec![]));
            ctx.events_mut()
                .push(RuntimeEvent::new(self.kind, tick, vec![]));
            Ok(())
        }
    }

    let mut rt = started(RuntimeConfig::new(1_000));
    rt.scheduler_mut()
        .register(
            HandleId::from_raw(1),
            "first",
            1,
            Box::new(Pusher { kind: 100 }),
        )
        .unwrap();
    rt.scheduler_mut()
        .register(
            HandleId::from_raw(2),
            "second",
            2,
            Box::new(Pusher { kind: 200 }),
        )
        .unwrap();
    rt.scheduler_mut()
        .register(
            HandleId::from_raw(3),
            "third",
            3,
            Box::new(Pusher { kind: 300 }),
        )
        .unwrap();

    let record = rt.step().unwrap();
    assert_eq!(record.diagnostics().commands_pushed(), 3);
    assert_eq!(record.diagnostics().commands_drained(), 3);
    assert_eq!(record.diagnostics().events_drained(), 3);
    // After drain, queues are empty.
    assert!(rt.commands().is_empty());
    assert!(rt.events().is_empty());
}

#[test]
fn system_failure_propagates_into_step_record_and_state() {
    struct F;
    impl RuntimeSystem for F {
        fn run(&mut self, _: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "x"))
        }
    }

    let mut rt = started(RuntimeConfig::new(1_000));
    rt.scheduler_mut()
        .register(HandleId::from_raw(1), "boom", 1, Box::new(F))
        .unwrap();
    let record = rt.step().unwrap();
    assert!(!record.succeeded());
    assert_eq!(record.state_after(), RuntimeState::Failed);
    assert_eq!(record.diagnostics().errors().len(), 1);
    // Subsequent step calls are rejected because we are no longer Running.
    assert_eq!(
        rt.step().unwrap_err().code(),
        RuntimeErrorCode::StepWhileNotRunning
    );
}

#[test]
fn diagnostics_log_and_metric_per_step_via_kernel_sinks() {
    let mut rt = started(RuntimeConfig::new(1_000));
    rt.step().unwrap();
    rt.step().unwrap();
    rt.step().unwrap();
    assert_eq!(rt.log_sink().len(), 3);
    assert_eq!(rt.telemetry_sink().len(), 3);
    assert_eq!(rt.telemetry_sink().counter_total("runtime.steps"), 3);
}
