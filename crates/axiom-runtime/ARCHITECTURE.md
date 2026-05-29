# Axiom Runtime — Architecture (Layer 01)

## What the runtime is

The runtime is the **deterministic engine execution substrate**. It sits one
layer above the kernel and adapts the kernel's primitive types into:

- a strict **lifecycle** state machine (`Created → Initialized → Running ↔ Paused → Stopped` and `→ Failed`),
- **deterministic fixed-timestep stepping** built on the kernel `SimulationClock`,
- per-step **frame / tick / sequence** identity (`RuntimeStep`),
- an **ordered system scheduler** with stable kernel-typed `HandleId`s and explicit `i32` order values,
- **FIFO command and event queues** drained at explicit step boundaries,
- **structured per-step diagnostics** and replay-ready `RuntimeStepRecord`s,
- **logging and telemetry hooks** routed through the kernel facade, never printed.

It is the substrate every later engine layer (rendering, ECS, assets, physics,
scenes, input, scripting, plugins, host integration) will build on.

## What this layer depends on from the kernel

The runtime consumes — and only consumes — the kernel's public primitives:

| From kernel | Used for |
|-------------|----------|
| `KernelApi` | the facade; logging and telemetry emission |
| `SimulationClock`, `FixedStep` | the ground-truth deterministic clock |
| `Tick`, `FrameIndex` | step identity carried on `RuntimeStep` |
| `KernelResult`, `KernelError` | wrapped inside `RuntimeError::with_kernel` |
| `HandleId` | stable system identity in the scheduler |
| `LogRecord`, `LogLevel`, `LogField`, `LogSink`, `InMemoryLogSink` | structured logging |
| `TelemetryMetric`, `MetricValue`, `TelemetrySink`, `InMemoryTelemetrySink` | structured telemetry |

The Axiom Layer Law and `cargo xtask check-architecture` mechanically enforce
that the runtime imports *only* the kernel and *only* through its public root
exports — never through private module paths.

## What the runtime intentionally does not know about

These belong to higher layers and must never appear here:

- rendering, WebGPU, WebGL, shaders, scenes, cameras,
- DOM / browser APIs of any kind,
- assets, asset loaders, codecs,
- physics, animation, audio, particle systems,
- ECS, world, archetypes,
- input devices,
- networking, scripting, plugins, editor surfaces,
- async host integration / event loops / `requestAnimationFrame`,
- any game-specific concept.

The runtime is a small, headless deterministic execution kernel for engine
systems. Host integration (driving the runtime from a browser frame loop or a
native main loop) lives one or more layers above.

## What future layers are expected to build on top of it

A future layer typically:

1. Declares a `layer.toml` with `previous = "runtime"`.
2. Imports `Runtime`, `RuntimeConfig`, `RuntimeSystem`, `RuntimeContext`, and
   `HandleId` from the runtime/kernel crates.
3. Implements `RuntimeSystem` for whatever it owns (e.g. an ECS world tick,
   a render pass schedule, an asset hot-reload scan).
4. Registers those systems with stable kernel-typed `HandleId`s and explicit
   order values via `Runtime::scheduler_mut()`.
5. Reads `RuntimeStepRecord`s after each `Runtime::step()` to drive any
   per-step audit, snapshot, or recording behavior.

## Determinism guarantees

The runtime preserves every determinism guarantee the kernel makes and adds its
own:

- No wall-clock time, no randomness, no global state, no I/O.
- Time advances only through `Runtime::step` (which calls `SimulationClock::advance`).
- Scheduler execution order is fully determined by the `(order, id)` pairs
  configured at registration — duplicate `id`s and duplicate `order`s are
  rejected, so there is **no implicit tie-breaker**.
- Command and event queues are strict FIFO (`VecDeque`); no hashing or priority.
- Two `Runtime`s constructed from the same `RuntimeConfig` and driven through
  the same sequence of `step()` calls produce byte-identical `RuntimeStep`s,
  byte-identical `RuntimeStepRecord` outcomes, and identical log / telemetry
  traces.

## One-public-thing-per-file convention

Following the kernel's structural convention, each source file owns exactly one
primary public type or trait, and `lib.rs` re-exports them. Adding a public
type means: a new file under `src/`, a private `mod` line in `lib.rs`, and a
matching `pub use` re-export. There is no `utils` or grab-bag module.
