//! # Axiom Runtime — Layer 01
//!
//! The runtime is the deterministic engine substrate that adapts the kernel
//! (Layer 00) into lifecycle, fixed-timestep stepping, ordered scheduling,
//! FIFO command/event queues, and replay-ready step records. It is the
//! foundation every later engine layer will build on.
//!
//! ## What this layer owns
//! - [`Runtime`] — owns runtime state and drives deterministic stepping.
//! - [`RuntimeTimeline`] — wraps the kernel [`axiom_kernel::SimulationClock`]
//!   into a frame/tick/sequence identity.
//! - [`RuntimeScheduler`] — registers ordered systems with stable IDs.
//! - [`RuntimeCommandQueue`] / [`RuntimeEventQueue`] — deterministic FIFO queues.
//! - [`RuntimeContext`] — the per-step surface a [`RuntimeSystem`] sees.
//! - [`RuntimeStepRecord`] / [`RuntimeDiagnostics`] — audit/replay data.
//!
//! ## What this layer consumes from the kernel
//! `KernelApi`, `SimulationClock`, `FixedStep`, `Tick`, `FrameIndex`,
//! `KernelResult`, `KernelError`, `HandleId`, `LogRecord` / `LogLevel` /
//! `LogSink` / `InMemoryLogSink`, `TelemetryMetric` / `TelemetrySink` /
//! `InMemoryTelemetrySink`. The kernel's other primitives stay unused.
//!
//! ## What this layer intentionally does not know about
//! Rendering, WebGPU, DOM, browser APIs, assets, physics, input, ECS,
//! plugins, scripting, networking, editor concepts, async host integration,
//! or any game-specific concept. Those belong to higher layers built on this
//! one.

mod runtime;
mod runtime_command;
mod runtime_command_queue;
mod runtime_config;
mod runtime_context;
mod runtime_diagnostics;
mod runtime_error;
mod runtime_error_code;
mod runtime_event;
mod runtime_event_queue;
mod runtime_result;
mod runtime_scheduler;
mod runtime_state;
mod runtime_step;
mod runtime_step_record;
mod runtime_system;
mod runtime_timeline;
mod system_outcome;

pub use runtime::Runtime;
pub use runtime_command::RuntimeCommand;
pub use runtime_command_queue::RuntimeCommandQueue;
pub use runtime_config::RuntimeConfig;
pub use runtime_context::RuntimeContext;
pub use runtime_diagnostics::RuntimeDiagnostics;
pub use runtime_error::RuntimeError;
pub use runtime_error_code::RuntimeErrorCode;
pub use runtime_event::RuntimeEvent;
pub use runtime_event_queue::RuntimeEventQueue;
pub use runtime_result::RuntimeResult;
pub use runtime_scheduler::RuntimeScheduler;
pub use runtime_state::RuntimeState;
pub use runtime_step::RuntimeStep;
pub use runtime_step_record::RuntimeStepRecord;
pub use runtime_system::RuntimeSystem;
pub use runtime_timeline::RuntimeTimeline;
pub use system_outcome::SystemOutcome;
