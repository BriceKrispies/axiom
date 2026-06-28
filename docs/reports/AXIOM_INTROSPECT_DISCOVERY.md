# `axiom-introspect` — Discovery Report

**Scope:** read-only architectural + behavioral discovery of the existing
`axiom-introspect` implementation. No code was modified. Generated from a full
read of the crate (`crates/axiom-introspect/`), its manifests, its tests, its
only consumer (`apps/axiom-demo-rotating-cube`), and the design doc
(`docs/introspection-layer-plan.md`).

> **RESOLUTION (follow-up change):** The risks (§10) and gaps (§9) below were
> subsequently **fixed** inside `crates/axiom-introspect`. Landed: deterministic
> frame timing on `FrameReport` (read from the existing `FrameTiming`);
> `WorldReport` made serializable and wired into the facade
> (`observe_world`/`latest_world`/`world_snapshot_bytes`); a new serializable-by-
> two-reports `FrameDiff` + `IntrospectApi::diff`/`failures`; whole-window
> serialization (`FrameHistory::to_bytes`/`from_bytes`,
> `history_snapshot_bytes`); `describe` resolves newest-first; the stale
> "no floating-point fields" doc and the Layer-05/06 drift were corrected. The
> sections below describe the **pre-fix** state and remain as the discovery
> record. Deferred (unchanged): per-entity world perception, a serialized
> `FrameDiff` wire type, and the browser query bridge (an app concern).

---

## Summary

- **Current verdict:** A small, honest, fully-tested **Phase-1 layer**. It does
  exactly one thing well — projects the canonical per-frame contract
  (`axiom_frame::EngineFrame`) into an owned, versioned, byte-serializable
  `FrameReport`, retains a bounded ring of them, and answers read queries behind
  `IntrospectApi`. It is real, deterministic, and architecturally legal — but it
  is *narrow*: it only introspects **frame execution** (systems + metrics), not
  world/entity perception, and the differentiating "diff / explain what changed"
  verb (Phase 2) does not exist yet.
- **Biggest value:** A **deterministic, replay-stable, schema-versioned snapshot
  of "what the engine just did"** that round-trips through the kernel binary
  primitives — the first concrete step toward an agent interrogating a running
  engine, and genuinely more than a logger because every answer is exact and
  reproducible.
- **Biggest risk:** `WorldReport` is a public export and the layer's *entire*
  proof-of-genuine-use of its `ecs` dependency, yet it is **not reachable
  through the `IntrospectApi` facade**, is **not serializable**, and carries only
  two integer counts. The interrogation surface is silently bifurcated (frames
  via the facade; world via a free `WorldReport::observe`), and the world arm is
  the weakest, most ceremonial-looking part of the layer.
- **Most important missing test:** A **cross-run byte-identical determinism test
  for a `FrameReport` that carries float metrics**, asserted *at the introspect
  layer* (the layer's own determinism test uses only system-free active frames;
  float byte-stability is only proven one level up, in the app's example/slice).
- **Most important missing primitive:** **`FrameDiff`** (the Phase-2
  "what changed between tick N and N+K, and which systems newly failed" delta).
  It is the verb the north-star ("explain what changed, exactly") is built on,
  and it is entirely absent.
- **Should it remain a module/layer/tool/app-helper?** **Layer** — correct as-is.
  It is cross-cutting spine infrastructure every future app/agent consumes; a
  module (isolated, un-importable by other layers) would strand it. The design
  doc's reasoning holds.
- **Ready for agent perception to consume?** **Partially.** Ready as a
  *frame-execution* introspection surface (what ran, in what order, which failed,
  what metrics, serialized + diffable-by-bytes). **Not** ready as a general agent
  *world*-perception surface: no per-entity identity, transforms, bounds,
  relations, capability discovery, `FrameDiff`, failure queries, whole-history
  serialization, or timing.

---

## Command results (run for this report)

### `cargo xtask check-architecture` — **PASS**

```
Layers checked: crypto -> ecs -> entropy -> frame -> host -> interface ->
                introspect -> kernel -> layout -> math -> proc ->
                proc-validate -> runtime -> space
OK: all layers satisfy the Axiom Layer Law.
```

`axiom-introspect` is structurally legal: its declared `depends_on`
(`kernel`, `frame`, `ecs`) is acyclic, genuinely used, and its `proof_exports`
resolve.

### `cargo test --workspace` — **FAIL (unrelated to `axiom-introspect`)**

One deterministic failure, in a **lower layer**, from **uncommitted working-tree
WIP**:

```
axiom-ecs  world::tests::read_snapshot_rejects_wrong_column_count
  assertion `left == right` failed
    left:  OutOfBounds
    right: TruncatedData
test result: FAILED. 110 passed; 1 failed
error: test failed, to rerun pass `-p axiom-ecs --lib`
```

- **Is it related to `axiom-introspect`? No.** The fault is in
  `crates/axiom-ecs/src/world.rs` (shown as `M`/modified in `git status` — an
  in-progress edit from another session): the wrong-column-count guard now
  returns `KernelErrorCode::OutOfBounds` while its test still expects
  `TruncatedData`. `axiom-introspect` only uses `World::entity_count()` /
  `system_count()` (it never touches snapshot deserialization error codes), so it
  is unaffected.
- **`axiom-introspect` in isolation is fully green:** `cargo test -p
  axiom-introspect` → **39 passed** (29 unit + 10 architecture), 0 failed.
- This `axiom-ecs` failure reddens the whole-workspace gate but is an independent
  lower-layer WIP issue. (One earlier `--workspace` invocation reported exit 0;
  the isolated, repeated reproduction of the `axiom-ecs` failure is the
  authoritative result — the green run was a parallel-launch fluke, not a real
  pass.)

---

## 1. What is `axiom-introspect`?

- **Kind:** an **engine layer** (the ordered spine), *not* a module, app helper,
  or tool. It has a `layer.toml`, lives under `crates/`, and is governed by the
  Layer Law.
- **Manifest:** `crates/axiom-introspect/layer.toml`
  (`name = "introspect"`, `crate_name = "axiom-introspect"`,
  `depends_on = ["kernel", "frame", "ecs"]`). Cargo manifest:
  `crates/axiom-introspect/Cargo.toml` (`crate-type = ["rlib"]`,
  `unsafe_code = "forbid"`).
- **Layer numbering is inconsistent in the prose** (a documentation nit, not a
  structural problem): `lib.rs` and the Cargo description say "Layer 05",
  `layer.toml` and `tests/architecture.rs` say "Layer 06". The mechanical checker
  uses the dependency graph, not a number, so this is cosmetic. The memory of
  record places it last in `kernel → runtime → math → host → frame → ecs →
  introspect`.
- **Runtime dependencies (genuinely used in non-test code):**
  - `axiom-kernel` — `BinaryWriter`/`BinaryReader`/`SchemaVersion`,
    `KernelError`/`KernelErrorCode`/`KernelErrorScope`/`KernelResult`,
    `TelemetryMetric`/`MetricValue`/`MetricKind`, `Tick` (in tests).
  - `axiom-frame` — `EngineFrame`, `FrameStepSummary` (via
    `runtime_step_summaries()`), `FrameSystemReport`, `FrameLifecycleState`,
    `FrameViewport`.
  - `axiom-ecs` — `World` (only `WorldReport::observe`).
- **Dev-only dependencies (fixtures, never in runtime surface):** `axiom-host`,
  `axiom-runtime`, `axiom-math` — used by `src/fixtures.rs` to synthesize real
  `EngineFrame`s through the genuine builder path.
- **What depends on it:** exactly **one** crate — `apps/axiom-demo-rotating-cube`
  (its `app.toml` lists `"introspect"`; `Cargo.toml` path-depends on it). No
  other layer, module, app, or tool consumes it. `axiom-agent` does **not** use
  it. The lower layers are verified *not* to import it
  (`tests/architecture.rs::lower_layers_do_not_import_axiom_introspect`).

---

## 2. Public API

`lib.rs` exposes a **curated set of six public items** (locked by
`tests/architecture.rs::lib_exports_are_curated_set`): one facade + five report
types. No public traits, no public macros, no public free functions, no public
enums are *introduced* here (the enums it traffics in — `FrameLifecycleState`,
`MetricValue`, `MetricKind` — belong to lower layers).

### `IntrospectApi` (`src/introspect_api.rs`) — the facade
A `struct` wrapping a single `FrameHistory`. The agent-facing query surface.

| Method | Purpose |
|---|---|
| `new(capacity: usize)` | Create a facade retaining ≤ `capacity` recent frames. |
| `observe(&mut self, frame: &EngineFrame)` | Record one completed frame (projects → `FrameReport`, pushes into the ring). The only mutator. |
| `describe_frame(index: u64) -> Option<&FrameReport>` | "Describe frame N" — linear lookup by engine frame index. |
| `recent(n: usize) -> &[FrameReport]` | The most recent `n` reports, arrival order. |
| `latest(&self) -> Option<&FrameReport>` | The most recent report (`recent(1).last()`). |
| `frame_count(&self) -> usize` | How many reports are retained. |
| `snapshot_bytes(&self) -> Option<Vec<u8>>` | Serialized bytes of the **latest** report — the agent channel. `None` until a frame is observed. |

### `FrameReport` (`src/frame_report.rs`) — the per-frame picture
Owned, `Clone`, `PartialEq` (not `Eq` — it transitively contains a float).
Fields: `engine_frame_index`, `host_frame_sequence`, `runtime_step_count`,
`skipped`, `lifecycle (FrameLifecycleState)`, `viewport_width/height`,
`systems: Vec<SystemReport>`, `metrics: Vec<MetricReport>`.
Public methods: `from_frame(&EngineFrame)` (the projection), `const` accessors
for every field, `systems()`, `metrics()`, `to_bytes()`, `from_bytes()`
(schema-versioned: `SCHEMA = 1.0`; rejects incompatible majors and truncated
buffers). Private `write_to`/`read_from` do a fully branchless `and_then`-chained
binary codec; `lifecycle_to_u8`/`lifecycle_from_u8` use a 4-entry variant table
as the wire codec.

### `SystemReport` (`src/system_report.rs`) — one system's outcome
Owned, `Clone`, `PartialEq`, `Eq`, `Hash`. Fields: `system_id: u64`,
`name: String` (copied off the frame's `'static str`), `order: i32`,
`succeeded: bool`, `error_code: Option<u16>`. Public: `from_frame`, `const`
accessors. `pub(crate) write_to`/`read_from` (length-prefixed name +
presence-tagged optional error code).

### `MetricReport` (`src/metric_report.rs`) — one telemetry sample
Owned, `Clone`, `PartialEq`. Fields: `name: String`, `is_counter: bool` (kind
collapsed to counter-vs-gauge), `value: MetricValue` (kernel int-or-float),
`tick: Option<u64>`. Public: `from_metric(&TelemetryMetric)`, accessors.
`pub(crate) write_to`/`read_from` with a `u8` value tag (`0` int / `1` float) and
an invalid-tag rejection arm.

### `FrameHistory` (`src/frame_history.rs`) — bounded ring
Owned, `Clone`. Fields: `capacity: usize` (clamped ≥ 1), `frames:
Vec<FrameReport>`. Public: `new`, `record` (evict-oldest at capacity via
`frames.remove(0)`), `recent`, `describe` (linear `find` by index), `len`,
`is_empty`, `capacity`.

### `WorldReport` (`src/world_report.rs`) — live world counts
Owned, `Copy`, `Eq`, `Hash`. Fields: `entities: u64`, `systems: u64`. Public:
`observe<S>(&World<S>)`, `entities()`, `systems()`. **Not serializable, not in
the facade, not retained.**

### Facade-rule compliance
The "**one public facade from `lib.rs`**" rule is a **Module Law** constraint
(`ModuleFacadeMustExportOne`), and `axiom-introspect` is a **layer**, not a
module — layers legally export multiple capabilities. So six exports is allowed,
and `layer.toml` declares all six as `introduced_capabilities`. The crate
enforces its *own* curated-export discipline via
`lib_exports_are_curated_set`. **However**, the design intent ("a single facade,
`IntrospectApi`" — design doc §4) is only met for the *frame* arm; `WorldReport`
is a second, parallel entry point (see §10).

---

## 3. Concepts modeled

| Concept | Present? | Where / form |
|---|---|---|
| Stable identity | **Yes** | `engine_frame_index: u64`, `host_frame_sequence: u64`, `system_id: u64` (raw kernel handle), metric `tick: Option<u64>`. |
| Semantic labels / tags | **Partial** | Free-text `name: String` on systems and metrics. No structured tag/label vocabulary, no enums of kinds beyond counter/gauge. |
| Inspectable properties | **Yes** | Frame fields (lifecycle, viewport, skip, step count), system fields (order, succeeded, error code), metric value, world counts. |
| Inspectable relations | **No** | Only flat ordered lists. No parent/child, no entity→component, no system→system ordering graph beyond the `order: i32` scalar. |
| Capability discovery | **No** | Nothing answers "what can this engine do." |
| State digests | **No** | No hashing/checksum/digest. The "diff substrate" is value equality + deterministic byte-serialization, not a digest. |
| Snapshots | **Yes (per-frame only)** | `FrameReport::to_bytes` / `IntrospectApi::snapshot_bytes` (latest frame). No whole-history snapshot. |
| Source / provenance | **Weak** | `host_frame_sequence` (which host frame produced this engine frame) and `tick`. No richer provenance (no system source location, no command provenance). |
| Bounds / transforms | **No** | Only `viewport_width/height: u32`. No spatial transforms or bounds. |
| Debug metadata | **Yes** | `lifecycle`, `skipped`, per-system `succeeded` + `error_code`. |
| Agent-facing perception metadata | **Partial** | The whole layer *is* "what the engine just did" (execution perception). But there is **no world/entity/spatial perception** beyond `WorldReport`'s two counts. |
| Telemetry samples | **Yes** | `MetricReport` (name, counter/gauge, int-or-float value, optional tick). |

---

## 4. Runtime behavior & data flow

**The layer stores state (a bounded ring), computes views (a projection),
serializes records (versioned binary), and defines contracts (the report types).
It emits no logs/telemetry of its own and reads no clock.**

Main flow (frame arm):

1. An owner (today: `DemoRotatingCubeApi`) constructs `IntrospectApi::new(K)` —
   `INTROSPECT_HISTORY` capacity in the demo.
2. Each tick, after the engine produces a completed `EngineFrame`, the owner
   calls `api.observe(&frame)`.
3. `observe` → `FrameReport::from_frame(frame)`: it iterates
   `frame.runtime_step_summaries()`, **flattening** every summary's `systems()`
   into one ordered `Vec<SystemReport>` and every summary's `metrics()` into one
   ordered `Vec<MetricReport>`, and copies the scalar frame facts (index,
   sequence, step count, skip, lifecycle, viewport). This is how an object (a
   frame) "becomes inspectable" — by value-projection into owned data that can
   outlive the borrowed frame.
4. `FrameHistory::record` pushes the report; if at capacity it first evicts the
   oldest (`remove(0)`) — bounded memory, FIFO.
5. **Consumption (all reads):** `describe_frame(index)` (linear `find`),
   `recent(n)` (tail slice), `latest()`, `frame_count()`, and `snapshot_bytes()`
   (serialize the latest report to versioned bytes). The facade holds no engine
   state of its own; its answers are a pure function of the frames it has
   observed.

World arm (disconnected): `WorldReport::observe(&world)` reads
`world.entity_count()` and `world.system_count()` **once, instantaneously**. It
is not retained, not serialized, and not callable through `IntrospectApi`. The
demo does not call it in its facade; only the layer's own unit tests exercise it.

Serialization: `to_bytes` writes `SchemaVersion(1,0)` then each field, with nested
length-prefixed `SystemReport`/`MetricReport` lists; `from_bytes` is a branchless
`and_then` chain that short-circuits on the first error, rejecting incompatible
schema majors (`SchemaVersionMismatch`), unknown lifecycle codes / metric tags
(`InvalidId`), and any truncation (`TruncatedData`/`OutOfBounds` from the reader).

---

## 5. Determinism

**Strongly deterministic by construction and by enforcement.**

- **No nondeterminism sources.** `tests/architecture.rs` token-scans the source
  and forbids `std::time`, `SystemTime`, `Instant::now`, `rand::`, `thread_rng`,
  `getrandom`, all browser/DOM/GPU APIs, and all console/placeholder macros.
  There is no global mutable state (`IntrospectApi` owns its ring; nothing
  static/`thread_local`). No `HashMap`/hashing — all collections are `Vec`,
  preserving insertion order; lookups are linear `find` over that order.
- **Every value enters as explicit data** handed to `observe`; the layer never
  reaches around its inputs (the design's "timing trap" is avoided — durations
  are simply absent, never read from a clock).
- **Byte-stability:** equal frames produce equal `FrameReport`s that serialize to
  identical bytes. The one float in the model (`MetricValue::float` inside
  `MetricReport`) is encoded via `write_f32`'s fixed bit pattern, so it is
  byte-stable for finite values; `PartialEq` (not `Eq`) is used precisely because
  of that float (NaN metrics would be a corner case, not seen in practice).
- **Determinism tests that exist:**
  - `introspect_api::tests::observation_sequence_is_deterministic` — two
    independent observe-sequences produce equal report vectors.
  - `frame_report::tests::round_trips_*` + `truncation_at_every_prefix_is_err` +
    `incompatible_schema_major_is_rejected` + `invalid_lifecycle_code_in_buffer_is_rejected`.
  - `system_report` / `metric_report` round-trip + truncation + invalid-tag tests.
  - App level: `introspection_evidence` example **check 4** ("two independent runs
    yield byte-identical snapshots") and `vertical_slice.rs::introspection_records_each_tick_and_is_queryable`
    (snapshot round-trips; angle metric differs frame-to-frame).
- **Determinism gap:** the *layer's own* determinism test
  (`observation_sequence_is_deterministic`) uses `active_engine_frames` (no
  systems, **no metrics**). Cross-run byte-identical equality for a **float-metric-bearing**
  report is only proven one layer up (the app example/slice). See §7 and §9.

---

## 6. Interaction with the rest of Axiom

| Subsystem | Interaction |
|---|---|
| **kernel** | Heavy, legal. Binary codec (`BinaryWriter`/`Reader`), `SchemaVersion`, error model, `TelemetryMetric`/`MetricValue`/`MetricKind`, `Tick`. The serialization + telemetry vocabulary the layer adapts. |
| **runtime** | **Dev-dependency only** (fixtures build real frames via `Runtime`). Not a runtime dependency, not in `depends_on`. Per-system outcomes reach introspect *through the frame contract*, not by importing runtime. |
| **math** | **Dev-dependency only** (fixtures). No runtime use. |
| **host** | **Dev-dependency only** (fixtures drive `HostStepDriver`). No runtime use. |
| **frame** | Core adapter target. Consumes `EngineFrame` and its per-system/per-metric detail, lifecycle, viewport. This is the layer's reason to exist. |
| **ecs** | Minimal. `WorldReport::observe` reads `World` entity/system counts. This single call is the layer's *entire* genuine-use proof of the `ecs` dependency. |
| **scene / render / resources / assets / physics** | **None.** No dependency, no imports, no concepts. (Correct — introspection must not pull feature concerns inward.) |
| **agent / bot systems** | **None today.** `axiom-agent` does not consume it. The layer is *designed* to be consumed by an agent via an app-owned transport (browser bridge / socket), which does not exist yet (Phase 3). |
| **apps / demos** | `apps/axiom-demo-rotating-cube` only: `demo_api.rs` feeds `observe` each tick and re-exposes `describe_frame`/`recent_frames`/`introspection_snapshot`; `examples/introspection_evidence.rs` produces a CI evidence artifact; `tests/vertical_slice.rs` proves the slice. |
| **tools / editor / debug harnesses** | **None.** No tool consumes introspect; the debug overlay (`modules/axiom-debug-overlay`) does not use it. |

---

## 7. Tests

All introspection tests live inside the crate (unit `#[cfg(test)]` per file +
`tests/architecture.rs`), plus app-level proofs in the demo.

| File | Tests | What they prove |
|---|---|---|
| `src/introspect_api.rs` | `fresh_facade_is_empty`, `observing_records_and_answers_queries`, `snapshot_bytes_round_trip_to_the_latest_report`, `observation_sequence_is_deterministic` | Empty-state answers; observe→query→monotonic indices→describe hit/miss→latest; latest snapshot decodes to an equal report; two runs equal. |
| `src/frame_report.rs` | 8: `from_active_frame_has_no_systems`, `from_failing_frame_carries_the_system`, `accessors_return_distinct_constructed_values`, `round_trips_each_lifecycle_and_skip_flag`, `round_trips_a_frame_with_systems`, `incompatible_schema_major_is_rejected`, `truncation_at_every_prefix_is_err`, `invalid_lifecycle_code_in_buffer_is_rejected`, `lifecycle_codes_round_trip_and_reject_unknown` | Projection of active/failing frames; every accessor; full lifecycle×skip matrix round-trip; system-bearing round-trip; schema-major rejection; truncation at every prefix; corrupt-lifecycle-byte rejection; lifecycle code table. |
| `src/system_report.rs` | 4: `from_frame_copies_every_field`, `failed_system_round_trips_with_error_code`, `succeeded_system_round_trips_without_error_code`, `truncation_at_every_prefix_is_err` | Field copy; both `Some`/`None` error-code arms of the codec (the success arm is hand-built because fixtures only register failing systems); truncation. |
| `src/metric_report.rs` | 5: `counter_with_tick_round_trips`, `gauge_without_tick_round_trips`, `negative_integer_round_trips_through_u64_bits`, `truncation_at_every_prefix_is_err`, `unknown_value_tag_is_rejected` | Counter+tick, gauge+no-tick, negative int via u64 bits, truncation, invalid value tag. |
| `src/frame_history.rs` | 5: `new_clamps_zero_capacity_to_one`, `records_up_to_capacity`, `recording_past_capacity_evicts_oldest`, `recent_clamps_to_available`, `describe_finds_present_and_misses_absent` | Capacity clamp; fill; FIFO eviction; `recent` clamping (incl. n=0); describe hit/miss. |
| `src/world_report.rs` | 2: `observe_captures_entity_and_system_counts`, `empty_world_reports_zero` | Entity/system counts after spawns+register+advance; zero for an empty world. |
| `tests/architecture.rs` | 10 | No browser/JS/DOM/WebGPU; no wall-clock/randomness; no console; no placeholder macros; no `utils`/`helpers`/`common`/`misc`; curated `lib.rs` export set; lower layers don't import introspect; introspect only imports legal lower layers. |
| `apps/.../tests/vertical_slice.rs` | `introspection_records_each_tick_and_is_queryable` (+ index-monotonicity test) | End-to-end: 61 ticks → one report each, queryable, monotonic, `cube-spin` system + `cube.angle_rad` metric captured and *changing*, latest snapshot round-trips, absent index misses. |
| `apps/.../examples/introspection_evidence.rs` | Runnable artifact (5 checks) | Live capture; query-by-index; snapshot round-trip; **two-run byte-identical**; failing-system name+code survive serialization. Exits non-zero on failure (CI-wireable). |

**Untested / weak / missing-coverage behavior:**
- **`WorldReport` is the weakest-tested public type:** two count assertions only.
  No serialization (it isn't serializable), no determinism test, no facade
  integration, no large-world or post-despawn behavior.
- **Float-metric byte determinism is not asserted at the introspect layer** —
  only at the app layer (see §5). The layer's `FrameReport` doc even claims "no
  floating-point fields," which is now false.
- **`describe_frame` duplicate-index behavior is unspecified/untested** — it
  returns the *first* match; nothing proves indices are unique or defines the tie.
- **No whole-history serialization** exists to test (only the latest frame is
  serializable through the facade).
- **No "weak/fake" or assertion-free tests were found.** Every test asserts a
  concrete property; truncation/round-trip tests are genuine. The `succeeded_system`
  and hand-built tests explicitly exist to cover the otherwise-unreachable `None`
  arms — honest coverage work, not theater.

---

## 8. Architecture-law compliance

| Law | Status | Notes |
|---|---|---|
| **Layer Law** | ✅ | `depends_on = [kernel, frame, ecs]` — all strictly lower, acyclic, genuinely used; `proof_exports` (`WorldReport`→`World`, `FrameReport`→`EngineFrame`, `IntrospectApi`→`EngineFrame`) resolve; `check-architecture` passes. |
| **Dependency direction** | ✅ | No future-layer imports; `lower_layers_do_not_import_axiom_introspect` enforces nothing below imports it. Dev-deps (host/runtime/math) are test-only and never in the runtime surface. |
| **Module Law** | ✅ (N/A) | It's a layer, not a module; the single-facade rule doesn't bind it. (But see the *design-intent* gap re `WorldReport` in §10.) |
| **Branchless Law** | ✅ (apparent) | Non-test code is written branchlessly throughout — `and_then`/`map`/`or_else`/`then`/`then_some`/`transpose` chains and a table-indexed lifecycle codec; no `if`/`match`/loops/`&&`/`||`/`?`. (The `engine_no_branching` dylint is the gate; not re-run here, but the source contains no control-flow branches.) |
| **No junk drawers** | ✅ | `no_utils_or_helpers_modules` enforces no `utils`/`helpers`/`common`/`misc`. File names are concept-named. |
| **No browser/platform leakage** | ✅ | Three architecture tests forbid web/JS/DOM/WebGPU/WebGL tokens; the crate is `rlib`, wasm-clean. |
| **No console / placeholder macros** | ✅ | `no_console_printing`, `no_placeholder_macros`. |
| **Coverage Law (100%)** | ✅ (per record) | The layer landed at 100% and its tests deliberately cover both arms of each codec branch. (Not re-measured in this read-only pass; the spine gate would catch a regression.) |

**No violations found.** One cosmetic inconsistency: the "Layer 05" vs "Layer 06"
prose mismatch (§1).

---

## 9. What is missing (contracts & tests, not rewrites)

Missing **primitives/contracts**:
- **`FrameDiff`** — the structured delta between two `FrameReport`s (changed
  fields, newly-failed systems, step-count delta). Design Phase 2; the verb the
  north-star leans on. Absent.
- **Failure query** — `failures(window) -> Vec<…>` ("where did errors occur").
  The data is present (`error_code`), the verb is not.
- **Whole-history serialization** — `FrameHistory::to_bytes`/`from_bytes` (or
  `IntrospectApi::history_snapshot_bytes`). Today only the *latest* frame crosses
  the byte channel; an agent cannot fetch the retained window in one read.
- **`WorldReport` in the channel** — make it reachable through `IntrospectApi`
  and serializable, so world observation uses the same facade + byte contract as
  frames.
- **Deterministic perf-timing-as-data seam** — an optional `duration` on
  `FrameReport`, carried as explicit data from the host boundary (Phase 4),
  *never* read from a clock.

Missing **seams for the stated downstream consumers**:
- **Agent perception:** no per-entity identity / transform / bounds / relation
  surface — only frame execution + two world counts. Agent "what exists in the
  world" is unanswerable here today.
- **Editor selection:** no entity/handle a selection could key on (no entity ids
  surfaced).
- **Debug overlays:** the overlay module doesn't consume introspect; no
  overlay-shaped view (e.g. per-frame system list) is exposed for it.
- **Replay inspection / save-state diffs:** `FrameDiff` (above) is the missing
  contract; the byte format is diff-ready but no diff verb exists.
- **Generated-game debugging:** no failure timeline / "first frame system X
  failed" query.

Missing **tests**:
- Cross-run byte-identical `FrameReport` **with float metrics**, at the introspect
  layer.
- `WorldReport` determinism + (once serializable) round-trip + truncation.
- `describe_frame` behavior on duplicate / absent indices (define and test the
  contract).
- History-wide serialization round-trip + truncation (once the primitive exists).

---

## 10. What is suspicious

1. **`WorldReport` is a second, disconnected interrogation entry point.** The
   design promised "a single facade, `IntrospectApi`." World observation is a
   free `WorldReport::observe`, not a facade method, isn't retained, and isn't
   serializable. `layer.toml` is explicit that `WorldReport` exists largely to be
   the *previous-layer proof* over `ecs` ("the *previous-layer* proof is
   `WorldReport` -> `World`"). It is genuinely used (so the Layer Law is
   satisfied, not violated), but it reads as the **minimum viable adapter to
   justify the `ecs` edge** rather than a designed part of the interrogation
   surface. This is the layer's softest spot.
2. **Stale/misleading doc on `FrameReport`.** Its doc comment says "no
   floating-point fields, so two reports built from equal frames are equal and
   serialize to identical bytes." Since metrics were folded in, `FrameReport`
   *does* transitively carry a float (`MetricValue::float`) — which is exactly why
   it derives `PartialEq` not `Eq`. The determinism claim is still *true* (via the
   fixed `write_f32` bit pattern) but the *reason given* is now wrong.
3. **`snapshot_bytes` only serializes the latest frame.** The method name and the
   "the bytes an external agent reads" framing oversell it; an agent gets one
   frame, not the window. Naming/scope mismatch.
4. **`describe_frame` is an O(n) linear scan** with an implicit "indices are
   unique" assumption that nothing enforces. Bounded today (small ring), but a
   latent foot-gun if capacity grows or indices ever repeat.
5. **Layer-number drift** ("05" vs "06" across `lib.rs`, Cargo, `layer.toml`,
   tests) — cosmetic, but the kind of inconsistency that misleads a future agent
   reading cold.
6. **No introspection of the thing the engine is increasingly about (the ECS
   world).** The sibling memory notes a rich reflection/serialization story landed
   in `axiom-ecs` (`Reflect`, `ColumnSet`, whole-world serialize/describe).
   `axiom-introspect` touches **none** of it — it reads two counts. The
   introspection layer is currently blind to the very data layer designed to be
   introspectable. Not a law violation; a strategic gap that will feel painful as
   soon as agent *world* perception is attempted.
7. **No leakage of scene/render/physics/editor concerns inward** — good; the
   layer is clean on that axis. The risk is the opposite: it is *too* thin to
   carry the perception load the north-star assigns it.

---

## 11. Prioritized follow-up tasks

> Concrete, contract- and test-focused. No implementation code; no "improve
> architecture."

### P1 — Correct the `FrameReport` determinism doc + lock float byte-stability at the layer
- **Files:** `crates/axiom-introspect/src/frame_report.rs` (doc comment +
  one new `#[cfg(test)]` test).
- **Why:** The "no floating-point fields" claim is now false and the
  byte-stability guarantee for float metrics is only proven one layer up. A
  reader trusting the comment could wrongly assume `Eq`/exactness.
- **Validate:** New test builds two reports from equal failing frames (which carry
  a float metric) and asserts `a.to_bytes() == b.to_bytes()`; `cargo test -p
  axiom-introspect`.

### P2 — Bring world observation into the facade + the byte channel
- **Files:** `src/introspect_api.rs` (add an `observe_world`/`world_report`
  read), `src/world_report.rs` (add `to_bytes`/`from_bytes` + schema), `src/lib.rs`
  + `tests/architecture.rs::lib_exports_are_curated_set` (kept in sync),
  `layer.toml`.
- **Why:** Removes the bifurcated interrogation surface (§10.1); makes the `ecs`
  dependency a *designed* part of the facade rather than a minimal proof; lets an
  agent read world state over the same byte contract as frames.
- **Validate:** Round-trip + truncation tests for `WorldReport`; a facade test
  that observes a world and reads it back; `cargo xtask check-architecture`;
  coverage gate stays 100%.

### P3 — Implement `FrameDiff` + `failures(window)` (design Phase 2)
- **Files:** new `src/frame_diff.rs`, `src/introspect_api.rs` (add `diff(a,b)` and
  `failures(window)`), `src/lib.rs` + curated-set test, `layer.toml`
  (`introduced_capabilities`, a `proof_export`).
- **Why:** The "explain what changed, exactly" verb is the differentiator the
  north-star is built on and is entirely absent.
- **Validate:** Deterministic diff tests (equal frames → empty diff;
  changed-field/newly-failed-system/step-delta cases); failure-window tests over a
  history with mixed success/failure; `check-architecture`; 100% coverage.

### P4 — Whole-history serialization
- **Files:** `src/frame_history.rs` (`to_bytes`/`from_bytes`), `src/introspect_api.rs`
  (`history_snapshot_bytes`).
- **Why:** An agent needs the retained window as one stable artifact, not just the
  latest frame; underpins replay inspection and save-state diffs.
- **Validate:** Round-trip + truncation-at-every-prefix tests; determinism across
  two identical observe-sequences.

### P5 — Deterministic perf-timing-as-data seam (design Phase 4)
- **Files:** frame layer (`crates/axiom-frame/` — carry an injected
  `elapsed_nanos` on the step/frame contract) and
  `crates/axiom-introspect/src/frame_report.rs` (surface an optional `duration`).
- **Why:** Agents will ask "how long did this take"; the design forbids a clock in
  a layer, so timing must enter as host-supplied data. Doing it wrong (a clock)
  would break replay.
- **Validate:** A report carries the injected duration; a determinism test proves
  identical injected inputs → identical bytes; the architecture token-scan still
  passes (no `std::time`).

### P6 — (Out of `axiom-introspect` scope, but blocks the shared gate) reconcile the failing `axiom-ecs` snapshot test
- **Files:** `crates/axiom-ecs/src/world.rs` (currently `M`/uncommitted) and its
  `read_snapshot_rejects_wrong_column_count` test.
- **Why:** `cargo test --workspace` is red on `OutOfBounds` vs `TruncatedData`
  from in-progress edits to the column-count guard. It is unrelated to introspect
  but reddens the whole-workspace gate any introspect change must pass through.
- **Validate:** Decide the intended error code for a wrong column count, align
  guard + test, `cargo test -p axiom-ecs`.
