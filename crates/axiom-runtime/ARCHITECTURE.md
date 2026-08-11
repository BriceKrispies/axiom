# Axiom Runtime — Architecture

## What the runtime is

The runtime is the **deterministic engine execution substrate**. It depends on
the kernel and adapts the kernel's primitive types into:

- a strict **lifecycle** state machine (`Created → Initialized → Prepared → Running ↔ Paused → Stopped` and `→ Failed`),
- a **startup preparation phase** (`PreparationTask`, `PreparationSchedule`,
  `Runtime::prepare`) that must run to completion before stepping may begin,
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

## The startup preparation phase

Between `Initialized` and `Running` sits **preparation**. Expensive,
startup-only work runs to completion, and only then may the simulation begin
stepping. `Runtime::prepare(schedule)` executes every task the caller pushed
onto a `PreparationSchedule`, in push order; all-`Ok` moves the runtime to
`RuntimeState::Prepared` — which, with `Paused`, is the only state `start()`
accepts — and the first `Err` moves it to `Failed`, which is terminal. There is
no partial readiness.

**What preparation is not.** It is not offline asset baking, not a persistent
cache, not build-time generation, not asset packaging, and not runtime
streaming. Nothing is written to disk or to any store, ever; on the next launch
the work simply runs again. Nor is it ordinary per-frame procedural work —
preparation is only for work that needs no simulation state at all. (The full
statement of the scope lives in
`docs/work-manifests/startup-preparation/README.md` §1.)

**The runtime owns the phase, never a product.** `PreparationTask::prepare`
takes `&mut self`, no arguments, and returns `RuntimeResult<()>`. A task is
handed no `Runtime`, no tick, no clock, and no queue, and nothing it produces
flows back through this layer — no mesh buffer, no typed handle, no
`Box<dyn Any>`, nothing from a higher tier that the runtime would otherwise
have to name. A task writes into storage its own constructor captured, and the
caller above owns that storage. That is precisely what keeps rendering, assets
and gameplay out of the exclusion list below. The zero-argument signature is
also load-bearing in the other direction: it is structurally incompatible with
`RuntimeSystem::run(&mut self, &mut RuntimeContext<'_>)`, so startup work
cannot be registered as frame work, or the reverse, without a compile error.

**The schedule is taken by value and dropped at the barrier.** `prepare`
consumes the `PreparationSchedule`, so every task — and whatever scratch state
it still holds — is destroyed when the phase ends. Temporary startup work
*cannot* leak into the frame loop; that is a property of the ownership model,
not a convention anyone has to honour. It also makes the phase un-repeatable:
running it again would require constructing a fresh schedule, and the lifecycle
rejects a second `prepare` regardless.

**Execution is sequential and single-pass**, in push order, stopping at the
first failing task. This is not a simplification to be revisited: the crate
contains zero `async fn`, `.await`, `Future` or executor, and the primary
target `wasm32-unknown-unknown` has no threads in this build, so a sequential
executor is the only model consistent with the spine as it exists. Push order
is a total order by construction — no ids, no order keys, no tie-breaker, and
no dependency solver for what is a straight line. There is deliberately no
`Preparing` state: `prepare(&mut self)` holds the exclusive borrow for the
whole phase, so no task, host or test could ever observe one.

**Scope limit — the barrier gates simulation stepping, not presentation.**
`Prepared` is a precondition of `Runtime::start` and therefore of
`Runtime::step`. A host that owns its own loop and calls a higher layer's
render entry point directly is not gated by it; if presentation gating is ever
wanted, it is a second consumer of `RuntimeState::Prepared`, not a relocation
of the phase. Equally, `Prepared` asserts only that *a* preparation phase ran
to completion — an empty schedule satisfies it. Deciding *what* preparation
must contain belongs to the composition root above, never to this layer.

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

1. Declares a `layer.toml` listing `runtime` in its `depends_on`.
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
- Preparation tasks run sequentially in push order and stop at the first
  failure, so a given schedule always produces the same outcome; the schedule
  is dropped at the barrier, so no startup scratch state reaches stepping.
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
