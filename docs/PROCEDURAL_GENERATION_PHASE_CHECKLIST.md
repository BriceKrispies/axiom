# Procedural Generation — Phase Checklist

> Terse, execution-ready checklist for future Claude Code sessions. One section
> per phase. Read the [roadmap](PROCEDURAL_GENERATION_ROADMAP.md) for rationale and
> the [test plan](PROCEDURAL_GENERATION_TEST_PLAN.md) for the test-class vocabulary
> before executing a phase. **Hard rules every phase:** no shortcuts; new
> layers/modules land at 100% coverage + zero branches with their tests in the same
> change; never weaken a law; no browser APIs outside `host`/`windowing`/allowlisted
> platform modules; no `utils`/`helpers`/`common`/`misc`; no module→module deps; no
> earlier-layer→later-layer deps.

**Standard gate commands (run before declaring any phase done):**
```
cargo fmt                         # only if Rust changed
cargo test --workspace
cargo xtask check-architecture
scripts/coverage.ps1              # 100% on new layer/module code
cargo dylint --all -- --all-targets
```

---

## Phase 0 — Audit & boundary discovery

- **Inspect:** `Cargo.toml`, `CLAUDE.md`, `crates/*/layer.toml`, `modules/*/module.toml`,
  `apps/*/app.toml`, `crates/axiom-kernel/ARCHITECTURE.md`,
  `apps/axiom-demo-rotating-cube/src/vertical_slice.rs`,
  `modules/axiom-recording/src/{hash,frame_capture,determinism_report}.rs`,
  `crates/axiom-introspect/src/frame_report.rs`, `apps/axiom-growth/src/*`.
- **Create:** the three planning docs (done) + this checklist.
- **Modify:** none (docs only).
- **Tests to add:** none.
- **Commands:** `cargo test --workspace`, `cargo xtask check-architecture`.
- **Expected artifacts:** confirmed list of byte-stable boundaries.
- **Stop conditions:** stop if any existing determinism test is red at head
  (record as pre-existing before any change); stop before adding *any* engine code.

---

## Phase 1 — Golden capture for existing apps

- **Inspect:** `apps/axiom-demo-rotating-cube/src/vertical_slice.rs` +
  `tests/vertical_slice.rs`; `apps/axiom-retro-fps-browser/tests/replay_determinism.rs`;
  `apps/axiom-quintet/src/*`; `apps/axiom-roomed-puzzle/src/*`.
- **Create:** golden corpus dir + capture test under each target app's `tests/`
  (start with rotating-cube).
- **Modify:** app `tests/` only; do **not** touch `crates/` or `modules/`.
- **Tests to add:** exact-golden byte-equality per boundary; documented
  regeneration path.
- **Commands:** standard gates (coverage unaffected — apps are exempt).
- **Expected artifacts:** committed golden bytes for the six rotating-cube
  boundaries + retro_fps state + quintet/roomed-puzzle outputs.
- **Stop conditions:** stop if any captured artifact carries raw `f32` ordering or
  GPU handles (not platform-stable — capture only owned, no-`f32` data); do not
  invent the `GoldenRun` format yet (Phase 2).

---

## Phase 2 — Stable hashing & GoldenRun format

- **Inspect:** `crates/axiom-kernel/src/{binary_writer,binary_reader,schema_version,
  reflect}.rs`; `modules/axiom-recording/src/{hash,frame_capture,determinism_report}.rs`;
  `crates/axiom-kernel/ARCHITECTURE.md` (kernel "no exciting code" rule).
- **Create:** stable hash primitive (kernel extension *or* `crates/axiom-hash` —
  decide by genuine use; lean kernel); `GoldenRun` writer/reader in a harness/tool.
- **Modify:** `crates/axiom-kernel` or new layer + root `Cargo.toml`;
  re-express Phase 1 goldens through `GoldenRun`.
- **Tests to add:** hash stability, hash sensitivity (1-bit flip), `GoldenRun`
  round-trip, golden-of-the-hash for a fixed byte vector.
- **Commands:** standard gates (100% + branchless on new spine code; if new layer,
  `check-architecture` must list it).
- **Expected artifacts:** versioned `GoldenRun` format; all Phase 1 goldens
  re-expressed.
- **Stop conditions:** stop if the hash depends on `HashMap` order, host endianness,
  or uncanonicalized `f32`; stop if tempted to make the hash the equality *proof*
  (bytes prove); no third-party hash crate without a recorded justification.

---

## Phase 3 — `axiom-space` layer (addresses)

- **Inspect:** `crates/axiom-crypto/layer.toml` + `crates/axiom-interface/layer.toml`
  (root-adjacent precedent); `apps/axiom-growth/src/ids.rs`
  (`ChunkCoord`/`RegionId`/`PlateId` to generalize); `crates/xtask/src/manifest.rs`.
- **Create:** `crates/axiom-space/` (`Cargo.toml`, `layer.toml`,
  `src/lib.rs` with `SpaceApi` + `Address`), root `Cargo.toml` members entry.
- **Modify:** root `Cargo.toml`.
- **Tests to add:** address equality/ordering determinism; byte round-trip; hash
  stability; hierarchical parent/child; **invariant** collision-free over a swept
  domain.
- **Commands:** standard gates; `check-architecture` must show `space → kernel`.
- **Expected artifacts:** green `space` layer in the DAG.
- **Stop conditions:** stop if `space` needs geometry (that's `math`) or domain
  meaning; stop if `depends_on` lists anything but `kernel`; no web APIs.

---

## Phase 4 — `axiom-entropy` layer (entropy streams)

- **Inspect:** `crates/axiom-kernel/src/deterministic_rng.rs`;
  `apps/axiom-growth/src/rng.rs` (`fork(salt)` to generalize); `crates/axiom-space`
  (Phase 3 `Address`).
- **Create:** `crates/axiom-entropy/` (`EntropyApi`, stream type keyed by
  `(seed, Address, version)`), `layer.toml` (`depends_on = ["kernel", "space"]`),
  members entry.
- **Modify:** root `Cargo.toml`.
- **Tests to add:** exact-golden first-N stream values; **invariant** distinct
  addresses → independent non-overlapping streams; **metamorphic** version bump
  changes/restores stream; cross-platform byte stability.
- **Commands:** standard gates; `check-architecture` shows `entropy → {kernel, space}`.
- **Expected artifacts:** green `entropy` layer.
- **Stop conditions:** stop if tempted to add a new RNG algorithm (reuse kernel
  `DeterministicRng`) or any noise function (Phase 9); no ambient entropy / wall
  clock; entropy must not depend on `proc`.

---

## Phase 5 — `axiom-proc` layer (recipes, graph, artifacts, traces)

- **Inspect:** `apps/axiom-growth/src/pipeline.rs` (`Stage`/`StageRegistry`/
  `Pipeline` to generalize); `modules/axiom-sim-core/src/*` (process/effect model
  for contrast — it's a module, can't be the shared substrate);
  `crates/axiom-kernel/src/{replay_timeline.rs, schema_version.rs}` (budget cadence).
- **Create:** `crates/axiom-proc/` (`ProcApi`, recipe/graph model, neutral
  `Artifact`, `ProcTrace`, budgeted/incremental eval), `layer.toml`
  (`depends_on = ["kernel", "space", "entropy"]`; add `math` **only if genuinely
  used**), members entry.
- **Modify:** root `Cargo.toml`.
- **Tests to add:** exact-golden artifact + trace bytes; **invariant** DAG acyclic
  (cycle rejected as data); **metamorphic** budget/chunking-invariant
  (incremental == whole) and version keying; trace ↔ artifact consistency.
- **Commands:** standard gates; **branchless graph eval** (table/iterator node
  dispatch, never `match` over node kinds in spine).
- **Expected artifacts:** green `proc` layer producing a hashable artifact+trace
  for a trivial recipe.
- **Stop conditions:** stop if any domain content (terrain/biome/noise/mesh) appears
  — it belongs in Phase 9 modules; stop if `math` is declared without genuine use;
  no web APIs; no junk-drawer node grab-bag.

---

## Phase 6 — `axiom-proc-validate` layer (constraints/scoring/repair)

- **Inspect:** `crates/axiom-proc` (Phase 5 `Artifact`/`ProcTrace`);
  `crates/axiom-kernel/src` (result/error model).
- **Create:** `crates/axiom-proc-validate/` (`ProcValidateApi`, constraint/score
  model, `ValidationReport`, repair-hook), `layer.toml`
  (`depends_on = ["kernel", "proc"]`), members entry.
- **Modify:** root `Cargo.toml`.
- **Tests to add:** exact-golden `ValidationReport` bytes; **invariant** identical
  artifacts → identical reports + validated artifact satisfies constraints;
  **metamorphic** known-good passes / perturbed fails at the expected constraint;
  repair determinism + re-validation.
- **Commands:** standard gates.
- **Expected artifacts:** green `proc-validate` layer.
- **Stop conditions:** stop if domain rules ("rivers reach the sea") appear (that's
  a terrain module); repair must be budgeted (no unbounded loop); no web APIs.

---

## Phase 7 — Procedural playground app

- **Inspect:** `apps/axiom-demo-rotating-cube-browser/` and
  `apps/axiom-stress-cubes-browser/` (browser app pattern, `App::run`);
  `scripts/playwright_controller.py`.
- **Create:** `apps/axiom-proc-playground/` (`app.toml`, composes the 4 new layers
  + `engine`/`windowing` if visual), trivial recipe (colored grid / cube field),
  members entry.
- **Modify:** root `Cargo.toml`.
- **Tests to add:** golden-run (artifact + proc-graph + trace hashes, + frame hashes
  if visual); replay byte-equality; visual sanity via Playwright if rendered.
- **Commands:** standard gates; if visual: build wasm, serve, then
  `uv run scripts/playwright_controller.py goto … / wait / console / screenshot`.
- **Expected artifacts:** committed `GoldenRun`; browser smoke screenshot.
- **Stop conditions:** stop if domain generation logic accretes in the app (push it
  down to a layer/module); do not rely on the final-frame hash as the only golden.

---

## Phase 8 — Migrate one hardcoded demo to a recipe

- **Inspect:** `apps/axiom-stress-cubes-browser/src/*` (first target);
  `apps/axiom-quintet/src/*` (headless-pure alternative); the app's existing
  determinism/golden tests.
- **Create:** recipe glue in the chosen app (or a feature module only if composing
  domain modules).
- **Modify:** the chosen app; remove its hardcoded content path.
- **Tests to add:** pre-existing golden still passes (or intentional re-golden +
  version bump); **seed-sweep** (N seeds reproducible); **metamorphic** (seed+1
  changes, seed restores).
- **Commands:** standard gates.
- **Expected artifacts:** recipe-driven, byte-reproducible, golden-tested app.
- **Stop conditions:** **do not** migrate `axiom-growth` here (too large — it's the
  eventual proving ground); do not weaken the app's existing golden to make the
  recipe pass; no new module→module edge.

---

## Phase 9 — terrain / biome / placement modules

- **Inspect:** `apps/axiom-growth/src/{noise,topology,gameworld,model_planet,
  atlas,sampler}.rs` (graduation source for noise + icosphere + sampling);
  `modules/axiom-render/module.toml` + `modules/axiom-scene/module.toml`
  (module manifest pattern); `modules/axiom-render-pipeline/module.toml`
  (feature-module composition pattern for `levelgen`).
- **Create:** `modules/axiom-terrain/`, `modules/axiom-biome/`,
  `modules/axiom-placement/` (engine modules, `allowed_modules = []`, one facade
  each); optionally `modules/axiom-levelgen/` (`kind = "feature-module"`); members
  entries.
- **Modify:** root `Cargo.toml`.
- **Tests to add:** exact-golden artifacts; **invariant** seam-coherence
  (`shared_edge_seam_is_zero` discipline) / biome single-classification;
  **metamorphic** ε-stability; **fuzz** random valid inputs never panic; validation
  integration.
- **Commands:** standard gates (each module 100% + branchless; platform-API ban).
- **Expected artifacts:** noise + icosphere now spine-tested (not app-local); green
  domain modules producing golden-stable artifacts.
- **Stop conditions:** stop if a domain module needs another domain module (extract
  the shared primitive **down** into a layer); no browser APIs in these modules;
  meshgen emits neutral mesh data, not GPU calls.

---

## Phase 10 — Procedural inspection tooling

- **Inspect:** `tools/axiom-shot/` (native renderer/CLI pattern);
  `modules/axiom-debug-overlay/` + `crates/axiom-interface/` (browser overlay
  pattern); `crates/xtask/src/hygiene.rs` (`PLATFORM_FACING_MODULES`).
- **Create:** `tools/axiom-proc-inspect/` (native CLI); optionally
  `modules/axiom-proc-overlay/` (platform-facing module composing `interface`).
- **Modify:** `crates/xtask/src/hygiene.rs` only if adding a platform-facing module
  (deliberate allowlist amendment); root `Cargo.toml`.
- **Tests to add:** fixed trace/artifact → golden inspection output; overlay core
  branchless + covered (module); CLI tested (tool, gate-exempt).
- **Commands:** standard gates.
- **Expected artifacts:** an agent can dump any artifact's provenance from
  `(seed, address, version)`.
- **Stop conditions:** stop if inspection/IO logic creeps into `proc`/`proc-validate`;
  no web APIs in the proc layers; allowlist amendment must be deliberate + documented.

---

## Phase 11 — Determinism, fuzz, performance gates

- **Inspect:** `tools/axiom-profile-runner/`; `scripts/coverage.ps1`/`.sh`;
  `scripts/playwright_controller.py`; `.github/workflows/ci.yml`;
  `docs/growth-port/terrain-streaming-stutter.md`.
- **Create:** seed-sweep + fuzz/property harnesses across new layers/modules; a
  browser-budget test; CI wiring.
- **Modify:** `tools/axiom-profile-runner`; CI config.
- **Tests to add:** seed-sweep at scale (log any cap); fuzz/property
  (never-panic/always-reproduce); long-run replay (generate→serialize→regenerate
  byte-equal); browser-budget (per-frame gen under documented ms; re-center
  regenerates only exposed sites); provenance reproduction.
- **Commands:** standard gates + sweep/fuzz/perf harnesses; Playwright budget run.
- **Expected artifacts:** CI fails on determinism drift / unbudgeted frame /
  provenance mismatch.
- **Stop conditions:** **never** widen a budget to silence a regression (fix the
  generator); **never** silently sample-cap a sweep (log the cap).

---

## Phase 12 — Save/delta model & server/browser parity

- **Inspect:** `apps/axiom-growth/src/model_world.rs` (`Chunk.edited` + `Diff`
  precedent); `modules/axiom-netcode/` (lockstep + state-hash reconciliation);
  `modules/axiom-net-protocol/` + `modules/axiom-client-core/`;
  `apps/axiom-netcode-demo/` (determinism proof to extend).
- **Create:** save/delta feature module or app glue; parity harness.
- **Modify:** netcode/net-protocol integration points (no new stack — reuse).
- **Tests to add:** save round-trip (generate→save→load+regenerate+apply
  deltas→byte-equal); **invariant** native hash == wasm hash for fixed inputs;
  **metamorphic** deltas+regenerate == live world; long-run lockstep determinism
  over a generated world.
- **Commands:** standard gates; native + wasm parity run; lockstep determinism run.
- **Expected artifacts:** a world reproduces from `{seed, versions, deltas}`
  byte-for-byte; server and browser agree.
- **Stop conditions:** **never** store full generated worlds "to be safe"; **never**
  ship full state over the wire; enforce server/client generator-version match.
