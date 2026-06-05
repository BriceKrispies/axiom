//! Registers ordered runtime systems and executes them deterministically.

use axiom_kernel::HandleId;

use crate::runtime_context::RuntimeContext;
use crate::runtime_error::RuntimeError;
use crate::runtime_error_code::RuntimeErrorCode;
use crate::runtime_result::RuntimeResult;
use crate::runtime_system::RuntimeSystem;
use crate::system_outcome::SystemOutcome;

/// A registered system together with the metadata the scheduler needs.
///
/// Held privately — callers register through [`RuntimeScheduler::register`]
/// and observe through [`SystemOutcome`].
struct Registered {
    id: HandleId,
    name: &'static str,
    order: i32,
    system: Box<dyn RuntimeSystem>,
}

/// A deterministic, order-driven scheduler.
///
/// Systems are registered with a stable [`HandleId`] (re-using the kernel's
/// identity primitive rather than inventing one), a static name, and an
/// explicit `i32` order. The scheduler stores them sorted by order at all
/// times, so execution is determined by configuration alone — never by
/// registration order. Duplicate `id`s and duplicate `order` values are
/// rejected at registration: the design has no implicit tie-breaker, so
/// ambiguity is a hard error.
pub struct RuntimeScheduler {
    entries: Vec<Registered>,
}

impl std::fmt::Debug for RuntimeScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeScheduler")
            .field(
                "systems",
                &self
                    .entries
                    .iter()
                    .map(|e| (e.id, e.name, e.order))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Default for RuntimeScheduler {
    fn default() -> Self {
        RuntimeScheduler::new()
    }
}

impl RuntimeScheduler {
    /// An empty scheduler.
    pub fn new() -> Self {
        RuntimeScheduler {
            entries: Vec::new(),
        }
    }

    /// Register a system at a stable `(id, name, order)`.
    ///
    /// Returns [`RuntimeErrorCode::DuplicateSystemId`] if `id` is already
    /// registered, or [`RuntimeErrorCode::DuplicateSystemOrder`] if another
    /// system already holds the same `order` value.
    pub fn register(
        &mut self,
        id: HandleId,
        name: &'static str,
        order: i32,
        system: Box<dyn RuntimeSystem>,
    ) -> RuntimeResult<()> {
        if self.entries.iter().any(|e| e.id == id) {
            return Err(RuntimeError::new(
                RuntimeErrorCode::DuplicateSystemId,
                "system id is already registered",
            ));
        }
        if self.entries.iter().any(|e| e.order == order) {
            return Err(RuntimeError::new(
                RuntimeErrorCode::DuplicateSystemOrder,
                "another system already uses this order value",
            ));
        }
        self.entries.push(Registered {
            id,
            name,
            order,
            system,
        });
        // Keep `entries` sorted by order so execution order is determined by
        // configuration only, not by insertion order.
        self.entries.sort_by_key(|e| e.order);
        Ok(())
    }

    /// Number of registered systems.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no systems are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The system ids in execution order — useful for assertions and
    /// diagnostics.
    pub fn system_ids(&self) -> Vec<HandleId> {
        self.entries.iter().map(|e| e.id).collect()
    }

    /// Execute every registered system in order.
    ///
    /// - If `stop_on_error` is `true`, the iteration stops at the first
    ///   failing system; remaining systems do not run.
    /// - If `stop_on_error` is `false`, every system runs regardless.
    ///
    /// Either way, an outcome is appended for every system that actually
    /// executed, in execution order.
    pub fn execute(
        &mut self,
        ctx: &mut RuntimeContext<'_>,
        stop_on_error: bool,
    ) -> Vec<SystemOutcome> {
        let mut outcomes = Vec::with_capacity(self.entries.len());
        for entry in &mut self.entries {
            let result = entry.system.run(ctx);
            let failed = result.is_err();
            outcomes.push(SystemOutcome::new(
                entry.id,
                entry.name,
                entry.order,
                result,
            ));
            if failed && stop_on_error {
                break;
            }
        }
        outcomes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_command_queue::RuntimeCommandQueue;
    use crate::runtime_event_queue::RuntimeEventQueue;
    use crate::runtime_step::RuntimeStep;
    use axiom_kernel::{FrameIndex, KernelApi, Tick};
    use std::sync::{Arc, Mutex};

    /// A system that pushes its name into a shared trace so we can prove
    /// execution order is decided by `order`, not registration order.
    struct TraceSystem {
        name: &'static str,
        trace: Arc<Mutex<Vec<&'static str>>>,
    }

    impl RuntimeSystem for TraceSystem {
        fn run(&mut self, _ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            self.trace.lock().unwrap().push(self.name);
            Ok(())
        }
    }

    /// A system that returns a typed error.
    struct FailSystem;
    impl RuntimeSystem for FailSystem {
        fn run(&mut self, _ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            Err(RuntimeError::new(
                RuntimeErrorCode::SystemFailed,
                "intentional",
            ))
        }
    }

    fn ctx<'a>(
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
    fn execution_order_follows_order_value_not_insertion_order() {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let mut sched = RuntimeScheduler::new();
        sched
            .register(
                HandleId::from_raw(2),
                "b",
                20,
                Box::new(TraceSystem {
                    name: "b",
                    trace: trace.clone(),
                }),
            )
            .unwrap();
        sched
            .register(
                HandleId::from_raw(1),
                "a",
                10,
                Box::new(TraceSystem {
                    name: "a",
                    trace: trace.clone(),
                }),
            )
            .unwrap();
        sched
            .register(
                HandleId::from_raw(3),
                "c",
                30,
                Box::new(TraceSystem {
                    name: "c",
                    trace: trace.clone(),
                }),
            )
            .unwrap();

        assert_eq!(
            sched.system_ids(),
            vec![
                HandleId::from_raw(1),
                HandleId::from_raw(2),
                HandleId::from_raw(3),
            ],
            "stored in ascending order"
        );

        let kernel = KernelApi::new();
        let (mut cmds, mut evts) = (RuntimeCommandQueue::new(), RuntimeEventQueue::new());
        let (mut logs, mut tel) = (kernel.log_sink(), kernel.telemetry_sink());
        let mut c = ctx(&mut cmds, &mut evts, &kernel, &mut logs, &mut tel);
        let outcomes = sched.execute(&mut c, true);

        assert_eq!(outcomes.len(), 3);
        let order: Vec<&'static str> = outcomes.iter().map(SystemOutcome::name).collect();
        assert_eq!(order, vec!["a", "b", "c"]);
        assert_eq!(*trace.lock().unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let mut sched = RuntimeScheduler::new();
        sched
            .register(HandleId::from_raw(1), "a", 1, Box::new(FailSystem))
            .unwrap();
        let err = sched
            .register(HandleId::from_raw(1), "a-twin", 2, Box::new(FailSystem))
            .unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::DuplicateSystemId);
        assert_eq!(sched.len(), 1);
    }

    #[test]
    fn duplicate_order_is_rejected() {
        let mut sched = RuntimeScheduler::new();
        sched
            .register(HandleId::from_raw(1), "a", 10, Box::new(FailSystem))
            .unwrap();
        let err = sched
            .register(HandleId::from_raw(2), "b", 10, Box::new(FailSystem))
            .unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::DuplicateSystemOrder);
    }

    #[test]
    fn stop_on_error_truncates_execution() {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let mut sched = RuntimeScheduler::new();
        sched
            .register(
                HandleId::from_raw(1),
                "a",
                1,
                Box::new(TraceSystem {
                    name: "a",
                    trace: trace.clone(),
                }),
            )
            .unwrap();
        sched
            .register(HandleId::from_raw(2), "boom", 2, Box::new(FailSystem))
            .unwrap();
        sched
            .register(
                HandleId::from_raw(3),
                "c",
                3,
                Box::new(TraceSystem {
                    name: "c",
                    trace: trace.clone(),
                }),
            )
            .unwrap();

        let kernel = KernelApi::new();
        let (mut cmds, mut evts) = (RuntimeCommandQueue::new(), RuntimeEventQueue::new());
        let (mut logs, mut tel) = (kernel.log_sink(), kernel.telemetry_sink());
        let mut c = ctx(&mut cmds, &mut evts, &kernel, &mut logs, &mut tel);
        let outcomes = sched.execute(&mut c, true);

        assert_eq!(outcomes.len(), 2, "stops at the failing system");
        assert!(outcomes[0].succeeded());
        assert!(!outcomes[1].succeeded());
        assert_eq!(
            *trace.lock().unwrap(),
            vec!["a"],
            "system after failure did not run"
        );
    }

    #[test]
    fn continue_on_error_runs_every_system() {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let mut sched = RuntimeScheduler::new();
        sched
            .register(
                HandleId::from_raw(1),
                "a",
                1,
                Box::new(TraceSystem {
                    name: "a",
                    trace: trace.clone(),
                }),
            )
            .unwrap();
        sched
            .register(HandleId::from_raw(2), "boom", 2, Box::new(FailSystem))
            .unwrap();
        sched
            .register(
                HandleId::from_raw(3),
                "c",
                3,
                Box::new(TraceSystem {
                    name: "c",
                    trace: trace.clone(),
                }),
            )
            .unwrap();

        let kernel = KernelApi::new();
        let (mut cmds, mut evts) = (RuntimeCommandQueue::new(), RuntimeEventQueue::new());
        let (mut logs, mut tel) = (kernel.log_sink(), kernel.telemetry_sink());
        let mut c = ctx(&mut cmds, &mut evts, &kernel, &mut logs, &mut tel);
        let outcomes = sched.execute(&mut c, false);

        assert_eq!(outcomes.len(), 3);
        assert_eq!(*trace.lock().unwrap(), vec!["a", "c"]);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::runtime::Runtime;
    use crate::runtime_config::RuntimeConfig;
    use axiom_kernel::HandleId;

    struct Noop;
    impl RuntimeSystem for Noop {
        fn run(&mut self, _: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            Ok(())
        }
    }

    #[test]
    fn default_matches_new() {
        assert_eq!(
            RuntimeScheduler::default().len(),
            RuntimeScheduler::new().len()
        );
    }

    #[test]
    fn registered_system_runs_through_a_runtime() {
        let mut rt = Runtime::new(RuntimeConfig::new(1_000)).unwrap();
        rt.initialize().unwrap();
        rt.start().unwrap();
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "noop", 1, Box::new(Noop))
            .unwrap();
        rt.step().unwrap(); // executes Noop::run
        // The registered system survived the step through the runtime.
        assert_eq!(rt.scheduler().len(), 1);
    }

    #[test]
    fn duplicates_rejected_and_accessors_work() {
        let mut s = RuntimeScheduler::new();
        assert!(s.is_empty());
        s.register(HandleId::from_raw(1), "a", 1, Box::new(Noop)).unwrap();
        assert!(s
            .register(HandleId::from_raw(1), "b", 2, Box::new(Noop))
            .is_err()); // duplicate id
        assert!(s
            .register(HandleId::from_raw(2), "c", 1, Box::new(Noop))
            .is_err()); // duplicate order
        s.register(HandleId::from_raw(2), "c", 2, Box::new(Noop)).unwrap();
        assert_eq!(s.len(), 2);
        assert!(!s.is_empty());
        assert_eq!(
            s.system_ids(),
            vec![HandleId::from_raw(1), HandleId::from_raw(2)]
        );
        assert!(format!("{:?}", s).contains("RuntimeScheduler"));
    }
}
