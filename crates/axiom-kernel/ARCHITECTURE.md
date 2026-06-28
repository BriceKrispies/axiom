# Axiom Kernel — Architecture

## What the kernel is

The kernel is a **root layer**: the deterministic runtime substrate that every
other Axiom layer is allowed to trust. It defines a small set of primitives —
time, identity, errors, memory addressing, messaging, binary serialization,
structured logging and telemetry — and nothing else.

The kernel is a pure-Rust library with **zero external dependencies**. It is
written to compile unchanged for native test runs and for
`wasm32-unknown-unknown` later, because it touches no platform facilities.

## What the kernel is NOT allowed to know

The kernel imports nothing from any higher layer (none exist), and it must never
gain knowledge of:

- browser / DOM / WebGPU / WebGL / JS APIs (`web_sys`, `js_sys`, `wasm-bindgen`);
- wall-clock time (`std::time`, `SystemTime`, `Instant`, `chrono`);
- randomness (`rand`, thread RNGs);
- rendering, ECS, scenes, assets, physics, input, audio, animation, plugins, or
  any game-framework concept;
- global mutable state (`static mut`, `lazy_static`);
- console output (`println!`, `eprintln!`, `dbg!`).

These prohibitions are enforced mechanically by `tests/architecture.rs`, which
scans the kernel's own source on every `cargo test`.

## Deterministic design rules

Determinism is the kernel's reason to exist. Concretely:

- **No ambient inputs.** Nothing reads a clock or an RNG. Time enters the system
  only as *data*: a `FixedStep` magnitude and explicit `advance` calls. Given the
  same inputs, every API produces byte-identical outputs.
- **Integer time.** `FixedStep` is integer nanoseconds, never floating point, so
  stepping is exactly reproducible across platforms.
- **Little-endian everywhere.** `BinaryWriter`/`BinaryReader` serialize in
  little-endian regardless of host, so serialized bytes match on every target
  including wasm. `Endian::KERNEL` names this contract.
- **Errors are identities, not strings.** A `KernelError` is the pair
  `(KernelErrorScope, KernelErrorCode)`. Equality ignores the human message, so
  error handling and replay comparisons are deterministic.
- **Ordered, non-hashing collections.** Queues and record sinks preserve
  insertion order with `Vec`/`VecDeque`; the kernel never iterates a hash map,
  whose order is not guaranteed.
- **Checked arithmetic.** Tick/offset/length math is saturating or checked and
  surfaces overflow as a `KernelError`, never a silent wrap or panic.
- **Replay is a primitive, not an afterthought.** Time has a *data* companion:
  `ReplayTimeline<T>` records a sequence and replays it through a saturating
  cursor (deterministic, rewindable, never panics or runs off the end), and
  `TickDivider` runs work at a whole-number sub-rate of the fixed step (fire
  every N ticks). Together with `Tick`/`SimulationClock`/`FixedStep` they let any
  layer record-and-replay deterministically without re-rolling cursor/cadence
  math by hand.

## Public surface: facade + curated primitives

`KernelApi` is the kernel's documented entry point — most callers should reach
capabilities through it. In addition, a **curated set** of primitive types is
re-exported at the crate root so future layers can *name* them (store them in
fields, construct them, pattern-match on them). The original "one public
export" rule was over-strict in practice: the runtime layer's `Runtime` must hold a
`SimulationClock` field and build `LogRecord` values from kernel data, neither
of which is possible if the types aren't nameable.

The trade-off is enforced rather than left to discipline:

- Each source file still owns **exactly one** primary public thing (one type,
  one trait, or one macro). There is no grab-bag module and, deliberately, **no
  `utils` module**.
- `tests/architecture.rs::lib_exports_are_curated_set` asserts that `lib.rs`'s
  public re-exports match an explicit approved list. Adding to the surface
  requires updating both `lib.rs` and the test in the same change — accidental
  widening still fails the build.
- The **dimensioned scalar quantities** (`Meters`, `Radians`, `Ratio`) are
  nameable primitives for the same reason the time and identity primitives are:
  higher layers must store them in fields and construct them (a camera's
  `fovy: Radians`, a viewport's `aspect: Ratio`, a clip plane's `near: Meters`).
  They live in the kernel — not a separate layer — because they are *core scalars
  required broadly across the engine*, the kernel's sanctioned remit, and because
  the Layer Law's DAG cannot host a broadly-shared primitive without a
  fake adjacent-layer dependency. They carry **no** unit *algebra* and no
  feature semantics; domain quantities (light intensity, colour channels,
  viewport pixels) deliberately stay out of the kernel and live in the
  layer/module that owns that domain. Each is the only public type in its file.
- The **deterministic replay primitives** (`ReplayTimeline<T>`, `TickDivider`)
  are the data half of the clock: higher layers store and replay recorded
  sequences (ghost paths, input timelines, demos) and schedule sub-rate work on
  the fixed step. `ReplayTimeline<T>` is, deliberately, the kernel's **first
  type-generic primitive** — the recorded item belongs to the caller (a move, a
  command, an event), not the kernel, so genericity is essential here and the
  alternative (an opaque byte buffer) would be strictly worse. It carries no
  domain semantics: cadence lives in `TickDivider`, meaning stays with the
  caller, and each remains the only public type in its file.
- The **deterministic tick-scheduling primitives** (`TickSchedule<Id, P>`,
  `TickDelta`) are the *scheduling* half of the clock. A `TickSchedule<Id, P>` is
  a `(Tick, Id)`-ordered schedule holding at most one pending entry per id:
  `schedule` arms (or re-arms) an id to wake at a future tick carrying a
  caller-defined payload, `pop_due` removes and returns every entry due at or
  before a supplied tick in stable ascending `(tick, id)` order (ties break by a
  total order on `Id`, never insertion/hash order), and `cancel`/`pending`/
  `peek_due` round it out. It reads **no clock** — the current tick is always
  supplied as data — and owns **no domain meaning**: it is pure tick + identity
  ordering, the kernel's sanctioned remit of "deterministic time/tick primitives
  over stable IDs." It is curated (not facade-only) and **type-generic** for the
  same reason `ReplayTimeline<T>` is — the id and payload belong to the caller,
  not the kernel. Two engine consumers name it: `axiom-sim-core`'s process wake
  queue (`Id = ProcessId`, `P = WakeReason`) re-bases onto it, and the
  `axiom-tick` timers facade (`Id = TimerId`, `P = (TickDelta, repeating)`)
  projects it; when one primitive is needed by two consumers it belongs in the
  shared root, not a third module. `TickDelta` is its companion — the typed
  non-negative offset between two `Tick`s (`Tick::add(delta)`,
  `Tick::delta_since(earlier)`, both saturating) so a deadline offset or an
  elapsed-ticks count crosses a higher API as a typed integer quantity, never a
  bare `u64` and never a float. Like the other time primitives they carry no
  feature semantics, and each is the only public type in its file.
- The binary-serialization primitives (`BinaryWriter`, `BinaryReader`,
  `SchemaVersion`) are nameable, because higher layers
  build and **version** their own wire formats on them — e.g.
  `axiom-introspect` stamps a `SchemaVersion` header on its serialized
  `FrameReport` and reads it back through a `BinaryReader`. Memory-addressing
  types (offsets, lengths, ranges) remain crate-internal: they are reachable
  only through `KernelApi` methods that return them (usable via inference, just
  not nameable). This keeps the visible surface aligned with what higher layers
  actually need.
- The **identity primitives** (`EntityId`, `HandleId`, `MessageId`, `AssetId`)
  are nameable for the same reason: higher layers and modules store them in
  fields, mint them, and key deterministic collections on them. `AssetId` was
  defined long before it was surfaced; it is now re-exported because the
  `axiom-assets` module (the runtime asset-streaming brain) builds its manifest,
  load-state machine, and dependency graph on it — exactly the "identity
  primitive future asset layers may build on" the type was introduced for. Like
  the others it carries no content or loading semantics; those live in
  `axiom-assets`.
- **`StableHash`** is the deterministic FNV-1a digest over canonical bytes — the
  digest companion to the serialization primitives above. It is curated (not
  facade-only) because higher layers and modules *name* it: the `recording`
  module's determinism reports and the procedural-generation layers' artifact and
  trace **provenance** index serialized bytes with it, and a digest computed in
  one place must equal one computed in another, which only a single shared
  primitive guarantees. It is a *diagnostic index, never the determinism proof* —
  byte equality proves; a digest only labels and locates bytes (the stance
  `modules/axiom-recording` already takes). It carries no domain semantics: it
  hashes opaque bytes and knows nothing about what they encode.

## Logging and telemetry as structured data

The kernel performs **no I/O** for observability. Instead:

- A `LogRecord` is immutable data: a `LogLevel`, a static scope, a machine
  `message_code` (its primary identity), a static human message, optional
  deterministic `Tick`/`FrameIndex`, and typed `LogField`s.
- A `TelemetryMetric` is data: a name, a `MetricKind` (counter or gauge), a
  `MetricValue`, and an optional `Tick`.
- Both are handed to a **sink** (`LogSink` / `TelemetrySink`) through the facade
  (`KernelApi::log`, `KernelApi::record_metric`). The kernel ships in-memory
  sinks that simply retain records in order. Exporting anything externally is a
  higher-layer concern; the kernel only records.

This makes the log/telemetry stream itself a deterministic, assertable artifact.

## The layer dependency graph

The layer DAG is not owned by kernel code. Each layer declares its direct
dependencies in its own `layer.toml`, and `cargo xtask check-architecture`
(repo tooling) verifies that those `depends_on` edges form a DAG, that every
import is declared, and that each declared dependency is genuinely used. The
kernel is simply a root of that graph: it declares no dependencies and imports
nothing.

## Dependency policy

The kernel has **zero** external dependencies, by design. Adding one requires a
strong justification recorded in this file. None is currently justified.

| Dependency | Reason |
|------------|--------|
| _(none)_   | The kernel is pure computation over `core`/`std` primitives. |
