# Axiom Frame — Architecture

`axiom-frame` is the canonical engine frame boundary of the Axiom engine.
It depends on kernel, runtime, and host:

```
axiom-frame depends on:
  axiom-kernel   (time, identity, errors, binary, logging, telemetry)
  axiom-runtime  (lifecycle, fixed-step scheduling, queues, context)
  axiom-host     (deterministic host boundary, runtime step driver)
```

## What axiom-frame is

The single place that answers the one question every future engine system
will ask:

> Given explicit host input and deterministic runtime stepping, what is
> the authoritative engine frame object for this update?

The answer is an [`EngineFrame`]. Each frame carries:

- an engine frame index (this layer's monotonic counter),
- the host frame sequence the engine frame was adapted from,
- an ordered list of [`FrameStepSummary`]s (one per `Runtime::step`),
- a frame-stable [`FrameViewport`] snapshot,
- a [`FrameLifecycleState`] projection of the host's lifecycle,
- a [`FrameTiming`] summary derived from explicit host/runtime data,
- a [`FrameDiagnostics`] summary, and
- an ordered list of [`FrameCommand`]s the builder attached.

Future systems read this through [`FrameContext`] (a borrow-side lens on
an `EngineFrame`) and never reach back to the host or runtime directly.

The crate ships exactly fourteen public items, all curated through
`lib.rs`:

- `FrameApi` — the facade.
- `EngineFrame` — the authoritative per-frame result.
- `FrameBuilder` — the deterministic constructor.
- `FrameContext` — the read-only borrow surface.
- `FrameCommand` / `FrameCommandQueue` — the deterministic command queue.
- `FrameDiagnostics` — the per-frame diagnostic summary.
- `FrameLifecycleState` — the four-state lifecycle projection.
- `FrameStepSummary` — the stable runtime-step summary.
- `FrameTiming` — the timing summary.
- `FrameViewport` — the frame-stable viewport snapshot.
- `FrameError` / `FrameErrorCode` / `FrameResult` — the error model.

## Why it exists after `axiom-host`

`axiom-host` answers a different question:

> Given external host input, how do I drive the runtime deterministically?

That layer owns the per-frame *driver*: it validates host data, plans
runtime stepping, and calls `Runtime::step`. What it does not own is the
*canonical, immutable per-frame value future systems consume*. Putting
that value in the host layer would be a layering inversion: a renderer
or debug overlay would have to know about `HostFrameInput`, accumulators,
and the runtime driver, when all it actually needs is "the engine frame
to read this update."

Frame sits above the host boundary specifically so:

- the engine's per-frame contract has one shape (`EngineFrame`),
- the contract is **immutable** (no setters, no mutation, no rebuilds),
- the host layer can evolve its driver/queue/lifecycle policy without
  forcing every consumer to track those changes,
- replay tests can compare engine frames as plain data.

## Why it is the canonical engine frame boundary

Every future engine system — renderer, scene graph, picking, animation,
audio, debug overlay, replay sink, test harness — will read frame data.
If each one reaches into the host layer (or worse, the runtime layer)
directly, the engine ends up with N different per-frame contracts. The
contract this layer publishes is the only one those systems get to use.

The contract is:

- a function of explicit depended-layer inputs (`HostFrameReport`,
  `RuntimeStepRecord`, and a fixed step value),
- byte-identical for byte-identical inputs,
- expressed in terms of value types with stable equality.

## What depended-layer data it consumes

- **From `axiom-host`:** [`HostFrameReport`] (which carries the host
  frame sequence, the host step plan, the ordered runtime step records,
  the host viewport, and the lifecycle state after the frame),
  [`HostViewport`], [`HostLifecycleState`], and [`HostSkipReason`].
- **From `axiom-runtime`:** [`RuntimeStepRecord`] (read through the host
  report; frame does not call `Runtime::step` itself), and the
  underlying [`RuntimeStep`] / `FrameIndex` / `Tick` identities.
- **From `axiom-kernel`:** the deterministic primitives that flow
  through the layers above (FrameIndex, Tick), reached transitively
  through `RuntimeStepRecord`. The [`axiom_kernel::Ratio`] type carried
  by [`HostViewport`] also guarantees that the viewport's scale factor
  and aspect ratio are finite by construction — no separate finiteness
  validation is needed at this layer.

## What higher-level contract it creates

A future engine system that reads frame data builds:

```rust
use axiom_frame::{FrameApi, FrameContext};

let api = FrameApi::new();
let mut builder = api.frame_builder(fixed_step_nanos);
// On each host frame:
let engine_frame = builder.build(&host_report, frame_commands)?;
let ctx = api.frame_context(&engine_frame);
// Read whatever it needs through the context:
let viewport = ctx.viewport();
let timing   = ctx.timing();
let lifecycle = ctx.lifecycle();
```

That contract is what every higher layer gets, and nothing else.

## Why this layer does not call browser APIs

The layer compiles to a pure Rust `rlib`. It has no `wasm-bindgen`
dependency, no `web_sys`/`js_sys`, no `wgpu`/`webgl`, no DOM/canvas
interop, no `requestAnimationFrame`, no `performance.now`, no
`std::time`, no `Instant`/`SystemTime`/`chrono`, no `rand`/`thread_rng`.
The architecture tests (`tests/architecture.rs`) scan the source tree
for all of those and fail the build if any of them appears.

The reasoning is the same as in the host layer's `ARCHITECTURE.md`:
keeping the engine frame contract free of browser/OS APIs means it
compiles and runs identically under a future browser adapter, a future
native adapter, a future headless harness, and the replay tests. Any
browser-specific concern lives in the future adapter crate, not here.

## Why this layer does not render

Rendering needs a backend (`wgpu` or `webgl2`), a swapchain, a shader
compiler, a frame-graph, and a material/mesh model. None of that
belongs in the engine frame boundary — the boundary's job is to deliver
the *frame contract*, not to act on it. A future renderer crate
will accept a `FrameContext`, build its own draw stream from the
viewport, lifecycle, step summaries, and commands carried on the frame,
and submit work to the backend. Letting any of that leak in here would
re-couple the boundary to a backend it must stay agnostic of.

## Why this layer does not own scenes / world / assets / input / physics / animation

Same reasoning, generalised: those are systems that read from the engine
frame contract, not parts of it. Each one needs to evolve under its own
constraints (asset streaming has IO, input mapping has user-facing
semantics, physics has solver tuning, animation has skeleton/clip
models). Letting any of them live here would either pull constraints
into the frame boundary that do not belong, or freeze the boundary
around an arbitrary choice in those systems. The boundary stays small,
boring, deterministic, and the systems that depend on it can ship
independently.

## How future systems should consume `FrameApi` and `FrameContext`

A future engine system (renderer, debug overlay, replay sink, picking
layer) will:

1. Hold a `FrameApi` and (where it owns the per-update flow) a
   `FrameBuilder` constructed from the same fixed step the host
   boundary uses.
2. On every host frame, hand the `HostFrameReport` (produced by the
   host boundary) to its builder and receive an `EngineFrame`.
3. Borrow that frame as a `FrameContext` and read whatever it needs:
   - `ctx.viewport()` for the frame-stable viewport,
   - `ctx.runtime_step_summaries()` for ordered runtime tick/frame
     identities,
   - `ctx.timing()` for consumed / retained / fixed-step nanoseconds,
   - `ctx.lifecycle()` for `Active` / `Hidden` / `Suspended` /
     `ShutdownRequested`,
   - `ctx.commands()` for any frame-local commands the builder
     attached,
   - `ctx.diagnostics()` for the small per-frame fact summary.
4. Never reach back into the host or runtime crates for per-frame data
   the engine frame boundary already exposes. If a fact is missing,
   the correct response is to add it to this layer (and to its tests),
   not to bypass it.

The frame contract is intentionally small. Future layers may extend it
in backwards-compatible ways; they may not invent parallel contracts.
