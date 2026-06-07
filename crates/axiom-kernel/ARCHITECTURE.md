# Axiom Kernel — Architecture (Layer 00)

## What the kernel is

The kernel is **Layer 00**: the deterministic runtime substrate that every
future Axiom layer is allowed to trust. It defines a small set of primitives —
time, identity, errors, memory addressing, messaging, binary serialization, the
layer contract, structured logging and telemetry — and nothing else.

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

## Public surface: facade + curated primitives

`KernelApi` is the kernel's documented entry point — most callers should reach
capabilities through it. In addition, a **curated set** of primitive types is
re-exported at the crate root so future layers can *name* them (store them in
fields, construct them, pattern-match on them). The original "one public
export" rule was over-strict in practice: Layer 1's `Runtime` must hold a
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
  the strictly-linear Layer Law cannot host a broadly-shared primitive without a
  fake adjacent-layer dependency. They carry **no** unit *algebra* and no
  feature semantics; domain quantities (light intensity, colour channels,
  viewport pixels) deliberately stay out of the kernel and live in the
  layer/module that owns that domain. Each is the only public type in its file.
- The binary-serialization primitives (`BinaryWriter`, `BinaryReader`,
  `SchemaVersion`) and layer-manifest types are nameable, because higher layers
  build and **version** their own wire formats on them — e.g. Layer 5
  (`axiom-introspect`) stamps a `SchemaVersion` header on its serialized
  `FrameReport` and reads it back through a `BinaryReader`. Memory-addressing
  types (offsets, lengths, ranges) remain crate-internal: they are reachable
  only through `KernelApi` methods that return them (usable via inference, just
  not nameable). This keeps the visible surface aligned with what higher layers
  actually need.

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

## The future-layer import rule

Layers are ordinals: the kernel is index `0`. The single rule
`LayerImportRule::validate(importer, target)` permits an import **iff**
`target < importer`. From that one rule:

- a layer cannot import itself (`target == importer` → `SelfImport`);
- a layer cannot import a future/higher layer (`target > importer` →
  `ForwardImport`);
- the kernel (index `0`) can import nothing, since no index is `< 0`.

`LayerManifest` additionally rejects the kernel declaring *any* dependency
(`KernelMustNotImport`) and rejects duplicate dependencies/capabilities. The
canonical kernel manifest is `LayerManifest::kernel()` — index `0`, name
`"axiom-kernel"`, no dependencies.

## Dependency policy

The kernel has **zero** external dependencies, by design. Adding one requires a
strong justification recorded in this file. None is currently justified.

| Dependency | Reason |
|------------|--------|
| _(none)_   | The kernel is pure computation over `core`/`std` primitives. |
