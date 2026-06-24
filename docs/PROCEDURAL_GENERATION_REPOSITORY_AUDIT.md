# Procedural Generation — Repository Audit

> Status: **audit only**. No procedural generation, terrain, noise, or random
> APIs are added by the work that produced this document. This file records what
> the repository *actually contains today* (inspected 2026-06-23) so the
> [roadmap](PROCEDURAL_GENERATION_ROADMAP.md) and
> [test plan](PROCEDURAL_GENERATION_TEST_PLAN.md) rest on facts, not guesses.

Every claim below is grounded in a concrete path. Where a symbol is named, it
exists in the repo at the stated location.

---

## 1. Existing layers (`crates/*/layer.toml`)

Axiom layers form a **DAG**, not a linear chain (the checker prints them
alphabetically: `crypto → ecs → frame → host → interface → introspect → kernel →
math → runtime`, which is display order, not dependency order). The real
`depends_on` edges are:

| Layer | Crate | `depends_on` | Role (from manifest) |
|-------|-------|--------------|----------------------|
| `kernel` | `crates/axiom-kernel` | *(root)* `[]` | Deterministic substrate: time, identity, errors, binary IO, logging/telemetry, RNG, replay. |
| `runtime` | `crates/axiom-runtime` | `["kernel"]` | Lifecycle, fixed-step stepping, scheduling, command/event queues, replay-ready step records. |
| `crypto` | `crates/axiom-crypto` | `["kernel"]` | ed25519 sign/verify + wire-serializable keys/signatures. Root-adjacent. |
| `interface` | `crates/axiom-interface` | `["kernel"]` | Renderer-neutral panels/layout/console/command-table/draw-list. Root-adjacent. |
| `math` | `crates/axiom-math` | `["kernel", "runtime"]` | Deterministic scalar/vector/quaternion/matrix/transform/geometry, checked, serializable. |
| `host` | `crates/axiom-host` | `["kernel", "runtime"]` | Deterministic platform boundary (viewport, lifecycle, presentation request/surface). |
| `frame` | `crates/axiom-frame` | `["kernel", "runtime", "host"]` | Canonical per-frame contract (`EngineFrame` / `FrameContext`). |
| `ecs` | `crates/axiom-ecs` | `["kernel", "frame"]` | Generic deterministic entity/component world, frame-gated advance. |
| `introspect` | `crates/axiom-introspect` | `["kernel", "frame", "ecs"]` | Agent-facing observability: byte-serializable per-frame `FrameReport`, `FrameHistory`, `WorldReport`. |

Root-adjacent layers (`depends_on = ["kernel"]`: `crypto`, `interface`, plus
`runtime`) are the precedent for adding **new low-level capability layers that
sit beside the kernel rather than on top of the render spine** — directly
relevant to the procedural pivot (see §11).

Kernel rules live in `crates/axiom-kernel/ARCHITECTURE.md`; the kernel's own
intra-crate checks live in `crates/axiom-kernel/tests/architecture.rs`.

---

## 2. Existing modules (`modules/*/module.toml`)

Engine modules (`kind = "engine-module"`, `allowed_modules = []`, isolated):

| Module | Crate | `allowed_layers` | Capability |
|--------|-------|------------------|------------|
| `scene` | `axiom-scene` | kernel, runtime, math, frame, ecs | Deterministic ECS-native scene graph + `SceneSnapshot`. |
| `resources` | `axiom-resources` | kernel, runtime, math, frame | CPU resource table + `ResolvedResources`. |
| `render` | `axiom-render` | kernel, runtime, math, frame, host | `RenderInput` → `RenderCommandList` + backend-neutral `FramePacket`. |
| `webgpu` | `axiom-webgpu` | kernel, runtime, math, host, frame | `GpuSubmission` / `GpuSubmissionReport`, recording + live backend boundary. |
| `gpu-backend` | `axiom-gpu-backend` | host | Real wgpu presentation (wasm32 arm). Platform-facing. |
| `canvas2d-backend` | `axiom-canvas2d-backend` | host | Software Canvas 2D fallback. Platform-facing. |
| `recording` | `axiom-recording` | kernel | Opaque-byte frame recorder/scrubber + determinism reporting. |
| `sim-core` | `axiom-sim-core` | kernel, ecs | Generic facts/relations/processes/effects/causal-journal (`SimCoreApi`). |
| `netcode` | `axiom-netcode` | kernel, crypto | Deterministic-lockstep session, input timeline, state-hash reconciliation. |
| `net-protocol` | `axiom-net-protocol` | kernel | Multiplayer wire/message contract (binary codec). |
| `client-core` | `axiom-client-core` | kernel | Portable client connection state machine. |
| `debug-overlay` | `axiom-debug-overlay` | interface | Browser debug overlay + command console. Platform-facing. |
| `input` | `axiom-input` | kernel, math | Virtual touch controls + pointer-intent synthesis. |

Feature modules (`kind = "feature-module"`, may compose listed modules):

| Module | Crate | `allowed_modules` | Capability |
|--------|-------|-------------------|------------|
| `render-pipeline` | `axiom-render-pipeline` | scene, render, webgpu | Scene → render-input → command-list → GPU pipeline. |
| `windowing` | `axiom-windowing` | gpu-backend, canvas2d-backend, recording | Presentation-request assembly + fixed-tick loop. |
| `engine` | `axiom` | scene, resources, render-pipeline, webgpu, windowing | The umbrella `App`/`prelude` apps import. |

**Module Law constraint that shapes the pivot:** an engine module may **never**
depend on another module. Anything several future generation modules
(terrain, biome, vegetation, …) must share has to live in a **layer**, not a
module. This is the decisive reason the generation substrate must be layers
(see §11).

---

## 3. Existing apps (`apps/*/app.toml`)

| App | Crate | Composes | Notes |
|-----|-------|----------|-------|
| `rotating-cube-demo` | `axiom-demo-rotating-cube` | scene, resources, render, webgpu (+ layers) | **Owns the 6-boundary deterministic vertical slice + artifact types.** |
| `rotating-cube-browser-demo` | `axiom-demo-rotating-cube-browser` | engine | Browser cube on `App::run`. |
| `retro_fps-browser-demo` | `axiom-retro-fps-browser` | engine, windowing | FPS gameplay; has `write_state`/`read_state` + replay tests. |
| `stress-cubes-browser-demo` | `axiom-stress-cubes-browser` | engine | Field of N spinning cubes, pure scene description. |
| `growth` | `axiom-growth` | engine, windowing (+ kernel, math) | **Procedural-planet survival game; the existing worldgen prototype.** |
| `roomed-puzzle` | `axiom-roomed-puzzle` | input (+ kernel, math) | Deterministic 2D grid puzzle + ghost replay. |
| `quintet` | `axiom-quintet` | *(none)* (+ kernel) | Seeded block-placement game; generation is a pure fn of (board, score, move-count). |
| `sim-crucible` | `axiom-sim-crucible` | sim-core (+ ecs) | Headless DF-like causal-chain proof. |
| `netcode-demo` | `axiom-netcode-demo` | engine, netcode (+ crypto) | Headless lockstep determinism proof. |
| `netcode-sim` | `axiom-netcode-sim` | engine, netcode (+ kernel, crypto) | N-peer lockstep harness. |
| `netplay-browser` | `axiom-netplay-browser` | engine, windowing | Server-authoritative multiplayer renderer. |
| `netplay-ffi` | `axiom-netplay-ffi` | engine, net-protocol | C-ABI engine embed. |
| `browser-dev-harness` | `axiom-browser-dev-harness` | debug-overlay | Mounts the overlay over a bare canvas. |

Apps are leaves; nothing depends on them. Apps are **exempt** from the
branchless and 100%-coverage gates (but still ship with their own tests). This
exemption is why `growth`'s worldgen lives in an app today.

---

## 4. Existing tools (`tools/*`, plus `xtask`)

`xtask` (the architecture checker), `tools/lints` (the dylint rulebook),
`tools/axiom-shot` (headless screenshot renderer — native GPU or canvas2d),
`tools/axiom-profile-runner` (native per-phase CPU profiler),
`tools/axiom-netcode-relay`, `tools/axiom-netplay-server`,
`tools/axiom-dev-reload`. Tools are outside the engine dependency graph and the
coverage gate. A native **procedural inspector CLI** (Phase 10) belongs here,
beside `axiom-shot`.

---

## 5. Existing deterministic boundaries discovered

The engine already has a strong determinism story; the pivot **extends** it
rather than inventing it.

- **Kernel determinism primitives** (`crates/axiom-kernel/src/`):
  - `deterministic_rng.rs` — `DeterministicRng::seeded(u64)`, `next_u64`
    (splitmix64, branchless), `next_bounded` (Lemire), `next_bool_in_thousand`.
    *No entropy, no clock, no global state.*
  - `replay_timeline.rs` — `ReplayTimeline<T>` (record → saturating-cursor
    replay; the kernel's one generic primitive).
  - `binary_writer.rs` / `binary_reader.rs` — little-endian, length-prefixed,
    bounds-checked (`Endian::KERNEL`).
  - `schema_version.rs` — `SchemaVersion { major, minor }` + `is_compatible_with`.
  - `reflect.rs` — `Reflect` trait (`reflect_write`/`reflect_read`,
    `const SCHEMA: TypeSchema`); impls for scalars, `EntityId`, and (in
    `axiom-math`) `Vec*`/`Quat`/`Mat4`/`Transform`, plus `Meters`/`Radians`/`Ratio`.
  - Integer time: `Tick`, `FixedStep` (integer ns), `SimulationClock`,
    `FrameIndex`; checked arithmetic everywhere.
- **The 6-boundary vertical slice** (`apps/axiom-demo-rotating-cube/src/vertical_slice.rs`):
  plain-data artifact types — `SceneSnapshotArtifact`, `ResolvedResourcesArtifact`,
  `RenderInputArtifact`, `RenderCommandListArtifact`, `GpuSubmissionArtifact`,
  `GpuSubmissionReportArtifact` (all `Debug + Clone + PartialEq + Eq`), plus
  `CubeIdentityArtifact`, `CubeTransformArtifact`, `VerticalSliceArtifact`.
  Tests (`tests/vertical_slice.rs`): `tick_zero_replay_is_byte_for_byte_equal`,
  `render_command_list_is_deterministic_for_the_same_tick`,
  `driven_sequence_replays_identically`,
  `cube_world_transform_changes_as_the_simulation_advances` (tick N vs N+60 differ).
- **Introspection serialization** (`crates/axiom-introspect/src/frame_report.rs`):
  `FrameReport::to_bytes`/`from_bytes`, stamped `SchemaVersion::new(1, 0)`; owned,
  no-`f32` data, so equal reports serialize byte-equal and round-trip.

---

## 6. Existing frame/app/output hashing mechanisms discovered

- **`modules/axiom-recording/src/hash.rs`** — `hash_bytes` / `hash_words`, a
  64-bit **FNV-1a** branchless fold (`FNV_OFFSET = 0xcbf29ce484222325`,
  `FNV_PRIME = 0x100000001b3`). This is the only hash in the engine spine.
- **`modules/axiom-recording/src/frame_capture.rs`** — `FrameCapture` carries
  `input_hash`, `runtime_hash`, `state_hash`, `render_hash`, `final_hash`
  alongside opaque `Vec<u8>` payloads; derives `PartialEq, Eq`.
- **`modules/axiom-recording/src/determinism_report.rs`** — `DeterminismReport` +
  `compare_timelines`, which localizes the **first** divergence to
  (frame, artifact kind, byte index). Replay tests:
  `modules/axiom-recording/tests/replay_determinism.rs`,
  `apps/axiom-retro-fps-browser/tests/replay_determinism.rs` (incl. fork-and-resume
  via `write_state`/`read_state`).
- **`apps/axiom-growth/src/determinism.rs`** — `world_hash`, an FNV-1a digest
  over the generated elevation + moisture fields, used today as a worldgen QA
  gate.

**Crucial framing already established in the codebase:** *hashes are
diagnostics; byte equality is the source of truth.* The procedural pivot adopts
this exact stance (the roadmap's hashing phase makes hashes the **index/label**
into stored golden bytes, never the proof).

**No golden baseline files exist yet.** All determinism today is in-memory
("run twice, assert equal"). There is no stored golden corpus, no `GoldenRun`
format, and no golden-update workflow. That gap is Phases 1–2.

---

## 7. Existing serialization mechanisms discovered

The kernel owns the canonical wire format (§5): `BinaryWriter`/`BinaryReader`
(little-endian, length-prefixed), `SchemaVersion`, and the composable `Reflect`
trait. Higher layers stamp their own `SchemaVersion` and compose `Reflect`
(`axiom-math` types, `FrameReport`). The recording module stores opaque
canonical bytes indexed by `FrameIndex`/`Tick`. **Everything a `GoldenRun` needs
to serialize already has a deterministic byte encoding or can compose one from
`Reflect` — no new serialization substrate is required.**

---

## 8. Architecture checker behaviour relevant to new layers/modules/apps

`cargo xtask check-architecture` **exists** and passes today (verified:
`OK: all layers satisfy the Axiom Layer Law.`). Checker sources:
`crates/xtask/src/{check.rs, class_check.rs, classification.rs, manifest.rs,
module_manifest.rs, app_manifest.rs, cargo_metadata.rs, coverage_scope.rs,
hygiene.rs, rust_source.rs, violation.rs}`.

What this means for new procedural layers/modules:

- **A new layer** needs `crates/axiom-<name>/layer.toml` with `depends_on` listing
  *only genuinely-used lower layers*, ≥1 `[[proof_exports]]` whose declaring file
  references a `must_reference` symbol, and an entry in root `Cargo.toml`
  `members`. The `engine_genuine_dependency` dylint then verifies each declared
  dep is referenced by a resolved `DefId` in non-test code.
- **Layers must stay a DAG.** New layers `space → kernel`, `entropy → {kernel,
  space}`, `proc → {kernel, space, entropy}`, `proc-validate → {kernel, proc}`
  introduce no cycle and no earlier-imports-later edge (verified by hand against
  the existing graph; see §11).
- **A new module** needs `modules/axiom-<name>/module.toml`
  (`kind`, `allowed_layers`, `allowed_modules`), one public facade in `lib.rs`
  (Module Law #8), globally-unique `introduced_capabilities` (#7) and name (#6).
- **Branchless Law + Coverage Law** apply to every new layer/module from the
  first commit (baseline 0 branches; 100% coverage). Apps and tools are exempt.
- **Platform-API ban (#9):** no `web_sys`/`js_sys`/`wasm_bindgen`/`canvas`/
  `document.`/`window.` in any new generation layer or module. A browser proc
  inspector must be a platform-facing *module* (allowlisted like `debug-overlay`)
  or app-local, never in the generic proc layers.
- **No junk drawers (#11):** no `utils`/`helpers`/`common`/`misc`.

---

## 9. Current gaps blocking procedural generation

1. **No stored golden corpus / `GoldenRun` format.** Determinism is proven only
   in-memory. There is nothing to diff a future generator's output *against
   across commits*. (Phases 1–2.)
2. **No stable cross-commit artifact hash convention.** FNV-1a exists in
   `recording`, but there is no agreed canonical-bytes → hash pipeline for
   arbitrary engine artifacts, no `SchemaVersion`-stamped golden envelope, and no
   golden-update workflow. (Phase 2.)
3. **No deterministic *address* primitive.** Nothing names a generation site
   (chunk coord, hierarchical region id, content path) as a stable, hashable,
   layerable type. `growth` has app-local `ChunkCoord`/`RegionId`/`PlateId`
   newtypes (`apps/axiom-growth/src/ids.rs`) but they are not a shared layer.
   (Phase 3 — `axiom-space`.)
4. **No explicit entropy-stream primitive.** The kernel has `DeterministicRng`
   (one stream from one `u64` seed) but no *address-keyed, version-keyed,
   sub-streamable* entropy. `growth` re-implements its own forking `Rng`
   (`apps/axiom-growth/src/rng.rs`, `fork(salt)`) precisely because the kernel
   does not offer it. (Phase 4 — `axiom-entropy`.)
5. **No generic proc-graph / recipe / artifact / trace substrate.** `growth`'s
   `Stage`/`StageRegistry`/`Pipeline` (`apps/axiom-growth/src/pipeline.rs`) is a
   bespoke app-local pipeline; `sim-core` has generic processes/effects but is a
   module (so other modules can't build on it). (Phase 5 — `axiom-proc`.)
6. **No validation/constraint/scoring/repair substrate** for generated content.
   (Phase 6 — `axiom-proc-validate`.)
7. **No noise / RNG-float / icosphere primitives in the spine.** They live only
   in `growth` (`noise.rs`, `topology.rs`, `rng.rs`). Per the hard constraints,
   the planning task does **not** add them; they graduate in Phases 4/9 with
   their own tests. The roadmap records where, not how.
8. **No browser generation-budget contract.** `growth` already discovered the
   problem (`docs/growth-port/terrain-streaming-stutter.md`: a 300 ms hitch from
   regenerating an entire window in one frame) but there is no engine-level
   chunked/budgeted/incremental generation contract. (Phases 5 & 11.)

---

## 10. Where golden artifact capture should attach first

**`apps/axiom-demo-rotating-cube`** is the first attachment point. It already
produces all six boundary artifacts as `PartialEq` plain data
(`src/vertical_slice.rs`) and already has the determinism tests
(`tests/vertical_slice.rs`). Phase 1 captures those existing artifacts as the
first stored goldens — *no new engine code, just serialize-and-store what is
already byte-stable*. Phase 2 then reuses the **`recording` module's** opaque-byte
+ `DeterminismReport` machinery as the storage/diff substrate for a `GoldenRun`.

Secondary early targets: `apps/axiom-retro-fps-browser` (already has
`write_state`/`read_state` + replay), and the kernel-only deterministic apps
`quintet` / `roomed-puzzle`.

---

## 11. Should `axiom-space`, `axiom-entropy`, `axiom-proc`, `axiom-proc-validate` be layers?

**Yes — all four are layers, per the current Layer Law.** The decisive argument
is the Module Law: *engine modules may not depend on other modules.* Terrain,
biome, vegetation, structures, meshgen, and levelgen will **all** need
addressing, entropy, and proc-graph evaluation. A capability that many sibling
modules must share cannot be a module — it must be a lower **layer** (exactly the
reason `math` is a layer, not a module). Each proposed layer also genuinely
adapts the layer(s) beneath it, satisfying the proof-export requirement:

| New layer | `depends_on` | Genuine adaptation (proof) | Acyclic? |
|-----------|--------------|----------------------------|----------|
| `space` | `["kernel"]` | Mints stable hashable addresses from kernel `HandleId`/ids + `BinaryWriter`; root-adjacent, beside `crypto`/`interface`. | ✅ |
| `entropy` | `["kernel", "space"]` | Derives deterministic entropy streams from kernel `DeterministicRng` keyed by a `space` `Address` + version. | ✅ |
| `proc` | `["kernel", "space", "entropy"]` | Generic recipe/graph evaluator emitting versioned artifacts + traces; consumes `space` addresses, `entropy` streams, kernel serialization. | ✅ |
| `proc-validate` | `["kernel", "proc"]` | Constraints/scoring/validation/repair hooks over `proc` artifacts/graphs. | ✅ |

None introduces a cycle and none makes an earlier layer import a later one.
`space` *could* be argued into the kernel as a bare scalar primitive (like
`Meters`), but it is a **capability with validation and hierarchy**, not a naked
scalar — so a root-adjacent layer (the `crypto`/`interface` precedent) is the
correct placement, keeping the kernel small. This is a recommendation for the
implementing agent to confirm at Phase 3, not a fait accompli.

**Open question to resolve at Phase 5 (flagged, not pre-decided):** whether
`proc` needs `math` in `depends_on`. If recipe evaluation references geometry
(`Vec3`, `Transform`) in non-test code it must declare `math`; if artifacts stay
neutral byte/scalar data it must **not** (a ceremonial dep is banned). Decide by
genuine use, not anticipation.

---

## 12. Should terrain/biome/vegetation/structures/meshgen/levelgen/proc-debug be modules?

Per the current Module Law:

- **terrain, biome, vegetation, structures, meshgen** — **engine modules**
  (`allowed_modules = []`), each consuming the `proc` / `proc-validate` /
  `space` / `entropy` / `math` *layers* and exposing one facade producing a
  generated-artifact contract. They must **not** import one another.
- **levelgen / "world recipe"** — a composition of several domain modules is a
  **feature module** (`kind = "feature-module"`, the `render-pipeline`
  precedent) listing the domain modules in `allowed_modules`, **or** app-owned
  glue. It is *not* an engine module.
- **proc-debug** — split by surface:
  - a **native inspector CLI** belongs in `tools/` (beside `axiom-shot`);
  - a **browser proc-trace/artifact overlay** belongs in a platform-facing
    *module* that composes the `interface` layer (the `debug-overlay`
    precedent), allowlisted in `PLATFORM_FACING_MODULES` — never web APIs in the
    generic proc layers.

---

## 13. Best candidate apps

- **Phase 1 golden capture:** `apps/axiom-demo-rotating-cube` (already has all
  six artifact boundaries + determinism tests). Fallbacks: `axiom-retro-fps-browser`,
  `quintet`, `roomed-puzzle`.
- **Phase 8 procedural migration (first):** `apps/axiom-stress-cubes-browser` —
  a field of N spinning cubes is the smallest, fully-deterministic scene whose
  layout can become `recipe(seed, address) → cube placements` with a directly
  visible golden. `quintet`'s seeded piece generation is the purest *pure
  function* example if a headless target is preferred.
- **Phase 8 procedural migration (eventual, large):** `apps/axiom-growth` — the
  existing procedural-planet prototype. It is already ~85% the shape
  `content = generator(seed, address, parameters, version)`
  (`generate_chunk(coord, atlas, localmap, seed)`,
  `sample_height_m(seed, world_pos, lod)`, forking RNG, FNV-1a `world_hash`), but
  it is far too large to be the *first* migration. It is the proving ground the
  Phase 3–9 layers/modules must ultimately support, and the place graduated
  primitives (RNG-float, noise, icosphere, addressing) come from.

---

## 14. Concrete file paths referenced

```
Cargo.toml                                              (workspace members)
CLAUDE.md                                               (the laws)
crates/axiom-kernel/ARCHITECTURE.md
crates/axiom-kernel/tests/architecture.rs
crates/axiom-kernel/src/deterministic_rng.rs
crates/axiom-kernel/src/replay_timeline.rs
crates/axiom-kernel/src/binary_writer.rs
crates/axiom-kernel/src/binary_reader.rs
crates/axiom-kernel/src/schema_version.rs
crates/axiom-kernel/src/reflect.rs
crates/axiom-*/layer.toml                               (9 layer manifests)
crates/xtask/src/check.rs, class_check.rs, classification.rs, hygiene.rs,
            manifest.rs, module_manifest.rs, app_manifest.rs, coverage_scope.rs
modules/axiom-recording/src/hash.rs
modules/axiom-recording/src/frame_capture.rs
modules/axiom-recording/src/determinism_report.rs
modules/axiom-recording/tests/replay_determinism.rs
modules/axiom-sim-core/src/                             (generic process/effect substrate)
modules/axiom-introspect/.. -> crates/axiom-introspect/src/frame_report.rs
apps/axiom-demo-rotating-cube/src/vertical_slice.rs
apps/axiom-demo-rotating-cube/tests/vertical_slice.rs
apps/axiom-retro-fps-browser/tests/replay_determinism.rs
apps/axiom-growth/src/{seed,rng,noise,topology,pipeline,model_planet,atlas,
            sampler,gameworld,model_world,chunkstore,determinism,ids}.rs
apps/axiom-growth/src/stages/*.rs                       (19-stage DEFAULT_GLOBE)
docs/growth-port/{roadmap,target-product,gap-analysis,
            terrain-streaming-stutter,worldgen_simulator_requirements_audit}.md
```
