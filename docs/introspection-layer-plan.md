# Design Plan — `axiom-introspect` (Layer 05): the engine introspection surface

**Status:** proposed (not yet implemented)
**Owner concept:** the agent-facing "interrogate the running engine" capability
**North star:** Axiom is a 3D engine *built for agentic engineering* — an agent
can interrogate a running engine and understand what it is and what it just did.
This layer is the first concrete step toward that, and the property that makes
it differ from bolting a logger onto Unity/Unreal/Godot is **determinism +
replay**: every answer this layer gives is exact and reproducible.

---

## 1. The problem this solves

Axiom already **records**. It does not yet **remember** or **answer**.

Grounded in the code as it stands today:

- **The kernel has the right primitives.** `LogRecord`, `TelemetryMetric`
  (counter/gauge + name + value + optional `Tick`), and in-memory sinks that
  capture in deterministic arrival order. Plus `BinaryWriter`/`BinaryReader`
  and `SchemaVersion` for stable serialization. Good foundation; leave it
  alone.
- **The runtime produces rich per-step diagnostics.** `RuntimeDiagnostics`
  carries per-system outcomes (`HandleId` + name + priority + pass/fail),
  queue push/drain counts, and `step_duration_nanos: Option<u64>`.
- **The frame layer discards almost all of it.** `FrameStepSummary` keeps only
  `{frame_index, tick, sequence, succeeded}`. By the time you reach the
  canonical per-frame contract (`EngineFrame`), you can ask "did the frame
  succeed?" but **not** "which systems ran, in what order, and which one failed
  and why." This is a real defect in the frame contract independent of this
  plan: the contract is lossy.
- **There is no time source.** `step_duration_nanos` is always `None` by
  deliberate kernel design (the kernel never reads a wall clock).
- **The sinks are flat `Vec`s** with no schema, no retained history window, no
  query surface, and no stable serialized form an external agent can read.

So the gap is three distinct capabilities — a queryable, versioned snapshot
model; a retained frame history you can diff; and a stable serialized channel
an agent reads — plus one determinism trap to avoid (timing).

## 2. Architectural placement — a new layer

**Decision: a new Layer 05, crate `axiom-introspect`, `previous = "frame"`.**

Rejected alternatives:

- **Kernel.** Querying and aggregating engine *frames* is not "what must always
  be true." The kernel owns the recording vocabulary; growing a frame-history
  query engine inside it pulls engine concepts inward, which the kernel rules
  forbid.
- **Module.** Modules are *isolated capabilities* (scene, render) that no other
  module or layer may import. Introspection is the opposite — cross-cutting
  spine infrastructure every future app, layer, and agent builds on. If it were
  a module, no future layer could ever consume it. Foundational ⇒ layer.

`introspect` is an honest semantic adapter over Layer 04: it consumes
`EngineFrame` / `FrameContext` (its N-1, the canonical per-frame facts) and,
legally, the kernel telemetry/log records (the emitted observability stream),
and produces a higher-level, *answerable* model of engine state and history.
Broad and shallow; satisfies the Layer Law.

`layer.toml` sketch:

```toml
[layer]
name = "introspect"
index = 5
previous = "frame"
crate_name = "axiom-introspect"
allowed_dependencies = ["kernel", "runtime", "math", "host", "frame"]
introduced_capabilities = ["IntrospectApi", "FrameReport", "FrameHistory", "FrameDiff"]
consumed_capabilities = ["EngineFrame", "FrameContext", "TelemetryMetric", "LogRecord", "SchemaVersion"]

[[proof_exports]]
export = "FrameReport"
must_reference = ["EngineFrame"]
# ...one per introduced capability, each referencing a frame/kernel symbol.
```

### Prerequisite that touches Layer 04

The frame contract must stop discarding per-system diagnostics. Either enrich
`FrameStepSummary` or add a sibling `FrameStepReport` carrying the system
outcomes the runtime already produces (`SystemOutcome`: id, name, priority,
result). This is **Phase 0** below. It is a legitimate frame-layer fix — the
contract is currently lossy regardless of introspection.

## 3. The model

Three plain-data, **kernel-serializable**, **versioned** types. Serializing
through the kernel's `BinaryWriter`/`BinaryReader` + `SchemaVersion` makes the
agent channel a stable wire format that is itself replay-diffable.

- **`FrameReport`** — the full answerable picture of one frame: identity
  (engine index, host sequence, tick), lifecycle, viewport, skip status, and
  the per-system list (name, id, priority, succeeded, error identity) recovered
  from the de-lossied frame contract. → "describe frame N."
- **`FrameHistory`** — a fixed-capacity ring buffer of the last *K*
  `FrameReport`s. Deterministic, bounded memory, no wall clock. → "what just
  happened."
- **`FrameDiff`** — a structured delta between two `FrameReport`s: which fields
  changed, which systems newly failed, the step-count delta. The payoff of
  determinism: "what changed between tick N and N+60, and why" — exact, not
  approximate.

## 4. The agent-facing query API

A single facade, `IntrospectApi`, exposing pure read verbs over the history:

- `describe_frame(index) -> FrameReport`
- `recent(n) -> &[FrameReport]`
- `systems(index) -> &[SystemReport]` — what ran, in order, with status
- `diff(a, b) -> FrameDiff`
- `failures(window) -> Vec<…>` — where errors occurred
- `to_bytes()` / `from_bytes()` via the kernel binary primitives — the
  serialized snapshot an external agent reads

Everything deterministic, assertable, and held to the 100% coverage invariant
like the rest of the spine.

## 5. The timing trap (do not break replay)

An agent will want "how long did this take," and `step_duration_nanos` is
sitting there `None`. **Do not** fill it with `std::time::Instant` inside a
layer. That smuggles wall-clock nondeterminism into the spine and destroys
replay — the single property that makes Axiom's introspection worth more than a
logger.

Correct shape: wall-clock measurement is an **explicit input supplied at the
host/app boundary as data** (the host already receives `elapsed_nanos` this
way). The introspect layer reports the deterministic timing it is *given*; it
never reads a clock. Simulation time (deterministic, already present) is always
available; perf timing is an optional, app-injected measurement behind an
explicit boundary.

## 6. Transport is an app concern, not the layer

The layer produces the serializable model; it does not open a channel.

- **Browser app:** a `wasm-bindgen` query bridge — JS calls `describe_frame`,
  gets bytes/JSON back.
- **Headless harness:** reads the same model directly in Rust.
- **Future native app:** could expose it over a socket.

Same model, different transports — the app-composition pattern the repo already
enforces. No browser API ever enters the layer.

## 7. Phasing — each phase lands green at 100% coverage

| Phase | Deliverable |
|-------|-------------|
| **0** | De-lossy the frame contract: carry per-system outcomes into the frame layer. (Pure Layer-04 fix.) |
| **1** | `axiom-introspect` layer: `FrameReport` + `FrameHistory` ring + `IntrospectApi` read verbs, kernel-serializable & versioned. |
| **2** | `FrameDiff` + failure queries — the "explain what changed" capability. |
| **3** | Browser query bridge in the browser app: a live agent can interrogate the running cube. |
| **4** | Inject deterministic perf-timing at the host boundary so reports carry real durations without breaking replay. |

## 8. First change to cut

**Phase 0 + the Phase 1 skeleton** in one change: fix the lossy frame summary,
scaffold `crates/axiom-introspect` with its `layer.toml`, and implement
`FrameReport` + `FrameHistory` + a minimal `IntrospectApi::describe_frame` /
`recent`, fully tested, passing `cargo xtask check-architecture` and the
coverage gate. The smallest structurally-correct slice that proves the layer is
real and gives an agent its first genuine answer.

## 9. Open questions for later

- Exact retention size *K* for `FrameHistory` (config primitive vs. fixed).
- Whether `FrameDiff` belongs in the layer or is an app-level convenience over
  two `FrameReport`s (lean: keep it in the layer — diffs are a core
  introspection verb, and they must be deterministic).
- Whether telemetry/log streams get folded into `FrameReport` per-frame, or
  remain a parallel queryable stream keyed by `Tick`.
