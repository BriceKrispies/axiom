# Procedural Generation — Roadmap

> **This is a plan, not an implementation.** Nothing in the change that produced
> this document generates terrain, adds noise, adds a random API, or weakens any
> architecture rule. It defines Axiom's procedural-generation identity and the
> phased build that future agents execute. Read the
> [repository audit](PROCEDURAL_GENERATION_REPOSITORY_AUDIT.md) for the grounding
> facts and the [test plan](PROCEDURAL_GENERATION_TEST_PLAN.md) for the testing
> contract every phase must satisfy.

---

## North star

> **Seed + Address + Entropy + Proc Graph + Validation + Artifact + Hash + Trace.**

Axiom becomes a **deterministic procedural world/content machine**: generated
content is reproducible, inspectable, hashable, validated, golden-testable, and
runnable in the browser/WASM. Given the same inputs, every generator produces
byte-identical artifacts on every platform, forever — and every artifact can
explain how it was made.

---

## Why procedural generation fits Axiom

Axiom already is a deterministic machine; procedural generation is the natural
*product* of that machine, not a bolt-on.

- The **kernel** is built on "no ambient inputs": integer time, little-endian
  serialization, errors as identities, seeded `DeterministicRng`, replay as a
  primitive (`crates/axiom-kernel/ARCHITECTURE.md`). A generator that is a pure
  function of explicit inputs is exactly what this substrate is *for*.
- The engine already proves **byte-equal replay** at six boundaries
  (`apps/axiom-demo-rotating-cube/src/vertical_slice.rs`) and has an opaque-byte
  recorder with first-divergence localization
  (`modules/axiom-recording/`). Golden procedural artifacts are the same
  discipline applied to *generated* bytes.
- The **branchless** spine forces logic to be expressed as data transforms —
  the same shape a proc graph wants (recipes are data; evaluation is a fold over
  a DAG, not hand-rolled control flow).
- An existing app, `axiom-growth`, has already built a deterministic worldgen
  prototype (seeds, forking RNG, noise, icosphere, a 19-stage pipeline, an FNV-1a
  `world_hash`) **inside an app on purpose**, so proven primitives can graduate
  into layers. The pivot turns that ad-hoc proof into first-class engine spine.

Procedural generation is therefore not a new direction so much as naming what
Axiom's determinism was always heading toward.

---

## Random generation vs deterministic procedural generation

| | Random generation | Deterministic procedural generation |
|---|---|---|
| Source of variety | ambient entropy (OS RNG, wall clock, thread RNG) | an explicit **seed** + **address** + **version** |
| Reproducibility | none — re-running differs | byte-identical on every platform, every run |
| Inspectable | no | yes — a **trace** records every decision |
| Hashable / golden-testable | meaningless (changes each run) | yes — a stable **hash** indexes stored golden bytes |
| Storable as save | must store the whole output | store seed + versions + deltas (§ save/delta) |
| Networkable | must ship full state | ship seed + command stream + deltas |

Axiom does **only** the right-hand column. Randomness never enters a generator
ambiently; it enters as a seed at the edge and is expanded by an explicit,
address-keyed entropy stream.

---

## The core content function

```
content = generator(seed, address, parameters, version)
```

- **seed** — the root entropy for a world/content set (a `u64`, or a value hashed
  into one). One seed, one world.
- **address** — *where/what* is being generated: a stable, hashable coordinate
  or path (chunk coord, hierarchical region id, content node path). Owned by the
  `space` layer (Phase 3).
- **parameters** — the recipe's tunable inputs (knobs, presets, budgets).
- **version** — the generator manifest version. Changing a generator's behavior
  **must** change its version, or goldens silently rot. Versioning is a
  first-class input, not metadata.

The same `(seed, address, parameters, version)` always yields the same
`content`. No hidden state, no clock, no ambient RNG.

---

## The core artifact flow

```
Seed
  → Address              (which site/content node — axiom-space)
  → Entropy              (deterministic stream keyed by seed+address+version — axiom-entropy)
  → Proc Graph           (recipe DAG evaluated over entropy+params — axiom-proc)
  → Validation           (constraints/scoring/repair — axiom-proc-validate)
  → Artifact             (the generated content, as canonical bytes)
  → Hash                 (stable digest indexing the artifact — stable hashing phase)
  → Trace                (the recorded decision log explaining the artifact)
```

Each arrow is a clean boundary with a deterministic byte representation, so each
can be golden-tested independently (see the test plan's "compare boundaries
independently" rule).

---

## How generated content relates to golden tests

A **golden test** pins an artifact's canonical bytes (and their hash) at a known
`(app/recipe id, engine version, generator manifest hash, seed, parameters,
version)`. A future change that alters output is *caught* by a golden diff and is
either:

1. a **bug** — the change was unintended; fix the code, goldens unchanged; or
2. an **intended behavior change** — bump the generator **version** and
   regenerate the goldens deliberately (the test plan's golden-update workflow).

Goldens are stored bytes, not in-memory `assert_eq!`. They survive across
commits, which is exactly what today's in-memory determinism tests cannot do
(audit §6). Hashes index and label goldens; **byte equality remains the proof**
(the stance already established in `modules/axiom-recording`).

---

## How existing app outputs become golden baselines

The engine already emits deterministic, `PartialEq`, byte-serializable artifacts
at real boundaries — they are *latent goldens*:

- `apps/axiom-demo-rotating-cube` — the six vertical-slice artifacts (Scene →
  Resources → RenderInput → RenderCommandList → GpuSubmission →
  GpuSubmissionReport).
- `crates/axiom-introspect` — `FrameReport` (`to_bytes`/`from_bytes`,
  `SchemaVersion`-stamped).
- `apps/axiom-retro-fps-browser` — `write_state`/`read_state` game state.
- `apps/axiom-growth` — `world_hash` over generated fields.

Phase 1 captures these *as they are today* into stored goldens, giving the pivot
a regression net before any generation code is written. No generation, no new
engine code — just serialize-and-store what is already byte-stable.

---

## How save files should eventually store seed + versions + deltas

A naive save stores the entire generated world. A deterministic procedural
engine should store only what cannot be regenerated:

```
save = { seed, generator_manifest_versions, parameters, player_deltas }
```

The world is *regenerated* from `(seed, parameters, versions)`; only the
**player's deviations** from the generated baseline (edited cells, placed
objects, consumed resources) are stored as deltas. `axiom-growth` already hints
at this: chunks carry an `edited` flag and a `Diff` enum
(`apps/axiom-growth/src/model_world.rs`) — edits are the delta, terrain is
regenerated. Loading replays generation then applies deltas. This is a Phase 12
deliverable; the layers built in Phases 3–6 make it *possible* (versioned,
addressable, hashable generation is the precondition for trusting regeneration).

---

## How multiplayer should eventually use seed + command stream + deltas

The same principle networked: peers share a `seed` + `versions` and a
**command/intent stream**; the world is generated identically on each peer, and
only commands and authoritative deltas cross the wire — never full world state.
Axiom already has both halves:

- deterministic-lockstep (`modules/axiom-netcode`) — only **inputs** cross the
  wire over a replayable simulation; state-hash reconciliation detects desync.
- server-authoritative intent/snapshot (`modules/axiom-net-protocol` +
  `axiom-client-core`).

Deterministic generation slots directly under lockstep: identical seed + identical
generation + identical commands ⇒ identical worlds, with the existing state-hash
reconciliation catching any generation-determinism break. This is Phase 12.

---

## How browser/WASM generation must be chunked, budgeted, and incremental

Generation must never block a frame. `axiom-growth` already learned this the hard
way — `docs/growth-port/terrain-streaming-stutter.md` documents a **300 ms hitch**
from regenerating an entire window in one frame. The engine-level contract the
proc layers must encode:

- **Chunked** — generation is addressed at chunk/node granularity; you generate a
  site, not a world, per call.
- **Budgeted** — a generation step takes an explicit work/time budget and yields;
  it never runs unbounded inside a frame. (`#[supervisor]`/cooperative stepping,
  built on the kernel's integer-time + `TickDivider` cadence.)
- **Incremental** — re-centering or refining regenerates only the newly-exposed
  sites, reusing already-generated neighbors (the audit's "stop the 19× redundancy"
  finding).

Budgeting and incrementality are part of the `proc` layer's evaluation contract
(Phase 5) and are gated by browser-budget tests (Phase 11).

---

## Layers vs modules vs apps vs tools vs tests for this pivot

| Tier | What goes here for the pivot | Why |
|------|------------------------------|-----|
| **Layers** (`crates/`) | `space`, `entropy`, `proc`, `proc-validate` — the **generic, shared** substrate. | Many sibling domain modules must share addressing/entropy/graph-eval; engine modules can't depend on each other, so the shared substrate *must* be layers (Module Law). |
| **Engine modules** (`modules/`) | `terrain`, `biome`, `vegetation`, `structures`, `meshgen` — **isolated** domain generators, each one facade, `allowed_modules = []`. | Each is an isolated capability consuming the proc *layers*; none imports another. |
| **Feature modules** (`modules/`, `kind = "feature-module"`) | `levelgen` / a composed "world recipe" that pulls several domain modules together. | Composition of modules is only legal in a feature module or an app (the `render-pipeline` precedent). |
| **Apps** (`apps/`) | the procedural playground, the migrated demo, app-owned glue between generators. | Apps are the only leaves and the only place module contracts are translated; exempt from branchless/coverage gates, so experiments live here first. |
| **Tools** (`tools/`) | a native procedural inspector CLI (beside `axiom-shot`); fuzz/perf harnesses. | Outside the engine graph; not held to the spine gates. |
| **Tests / harnesses** | golden corpora, invariant/metamorphic/seed-sweep/replay/round-trip suites, architecture-boundary tests. | Prove determinism, boundaries, budgets, and provenance. |

The generic substrate is layers; the domain semantics are modules; the
experiments and glue are apps; the inspectors are tools.

---

## Phase-by-phase roadmap

Each phase below is self-contained: goal, placement, crates touched, capabilities
in/consumed, non-goals, tests, architecture checks, golden/invariant/metamorphic
tests, completion criteria, risks, and "what future agents must not do." Phases
0–2 build the **golden/hash safety net** before any generation. Phases 3–6 add
the **substrate layers**. Phases 7–9 build **on** it. Phases 10–12 mature it.

A phase is "done" only when `cargo test --workspace`,
`cargo xtask check-architecture`, the coverage gate (`scripts/coverage.ps1`), and
the dylint gates all pass — for new layer/module code at 100% coverage and zero
branches.

---

### Phase 0 — Repository audit and procedural boundary discovery

- **Goal:** Establish the factual baseline and the exact deterministic boundaries
  the pivot will hash and golden. (Largely satisfied by
  [`PROCEDURAL_GENERATION_REPOSITORY_AUDIT.md`](PROCEDURAL_GENERATION_REPOSITORY_AUDIT.md);
  this phase's remaining work is confirming each named boundary still serializes
  deterministically at the head commit.)
- **Architectural placement:** `Test/Harness` + docs. No engine code.
- **Crates/modules/apps/tools touched:** none created. Read-only across
  `crates/`, `modules/`, `apps/`, `tools/`.
- **Public capabilities introduced:** none.
- **Consumed lower-layer capabilities:** none.
- **Non-goals:** no new layer/module/app; no generation; no hashing format yet.
- **Tests required:** none added; *run* the existing determinism tests and record
  which boundaries are byte-stable.
- **Architecture checks required:** `cargo xtask check-architecture` green at head.
- **Golden/invariant/metamorphic:** inventory only — list every existing artifact
  with a deterministic byte form (audit §5–§7).
- **Completion criteria:** audit doc matches the repo; every claimed boundary is
  confirmed byte-stable by an existing or one-off local check.
- **Risks:** mistaking an in-memory `PartialEq` for a *stored* golden (there are
  none yet — audit §6).
- **Future agents must not:** add generation, noise, RNG, or new crates "while
  they're in there." This phase is observation only.

---

### Phase 1 — Golden artifact capture for existing deterministic apps

- **Goal:** Capture the *already byte-stable* outputs of existing apps as the
  first **stored** golden baselines, creating a cross-commit regression net before
  any generation code exists.
- **Architectural placement:** `Test/Harness` (golden capture lives in app
  `tests/` + a golden corpus directory). No spine code.
- **Crates/modules/apps/tools touched:** `apps/axiom-demo-rotating-cube` (first),
  then `apps/axiom-retro-fps-browser`, `apps/axiom-quintet`,
  `apps/axiom-roomed-puzzle`. New golden corpus dir under each app's `tests/`.
- **Public capabilities introduced:** none (tests + data only).
- **Consumed lower-layer capabilities:** existing artifact serialization
  (`vertical_slice.rs`, `FrameReport::to_bytes`, `write_state`).
- **Non-goals:** no `GoldenRun` *format* yet (Phase 2); no hashing convention
  yet; no generation; do not touch the spine.
- **Tests required:** per app, a test that regenerates each boundary artifact and
  asserts byte-equality against the committed golden file; a documented,
  intentional regeneration path.
- **Architecture checks required:** `check-architecture` unaffected (apps);
  workspace tests green.
- **Golden tests required:** the captured baselines themselves — at minimum the
  six rotating-cube boundaries for a fixed tick set, the retro_fps state for a fixed
  intent script, and the kernel-only apps' deterministic outputs.
- **Completion criteria:** committed goldens; tests fail loudly on any byte drift;
  golden-update workflow documented.
- **Risks:** capturing a boundary that is *not actually* platform-stable (e.g.
  anything carrying raw `f32` ordering or GPU handles) — capture only the
  no-`f32`, owned-data artifacts the audit confirmed.
- **Future agents must not:** "fix" a failing golden by editing the golden without
  a documented intentional reason; capture non-deterministic data.

---

### Phase 2 — Stable artifact hashing and golden-run format

- **Goal:** Define a single canonical-bytes → stable-hash convention and a
  versioned `GoldenRun` envelope, so any artifact (now and future generated ones)
  can be hashed, stored, and diffed uniformly.
- **Architectural placement:** the hashing primitive is **spine** — a stable
  digest over canonical bytes. Candidate placements, to decide by genuine use:
  (a) extend the kernel (a `reflect`-adjacent stable digest, since the kernel
  already owns `BinaryWriter`/`Reflect`), or (b) a small root-adjacent layer.
  Lean kernel — hashing canonical kernel bytes is a kernel-shaped concern and many
  layers need it — but confirm against the kernel's "no exciting code" rule. The
  `GoldenRun` *envelope* (app id, versions, seed, the hash lists) is `Test/Harness`
  data built on that primitive; the `recording` module's opaque-byte +
  `DeterminismReport` machinery is the storage/diff substrate.
- **Crates/modules/apps/tools touched:** `crates/axiom-kernel` *or* a new
  `crates/axiom-hash` layer (decide by §placement); `modules/axiom-recording`
  (reuse). The `GoldenRun` writer/reader lives in a test harness or tool.
- **Public capabilities introduced:** a stable artifact hash (`StableHash` /
  `artifact_digest`) and a `SchemaVersion`-stamped `GoldenRun` byte format.
- **Consumed lower-layer capabilities:** `BinaryWriter`/`BinaryReader`,
  `SchemaVersion`, `Reflect`, and `recording`'s FNV-1a / opaque-byte capture.
- **Non-goals:** no generation; do not replace byte-equality with hashing as the
  *proof* (hash indexes/labels; bytes prove); do not invent a new serialization
  substrate.
- **Tests required:** hash stability (same bytes → same hash across runs/targets),
  hash sensitivity (one bit flips the hash), `GoldenRun` round-trip
  (`to_bytes`/`from_bytes`), and re-expression of Phase 1 goldens through the new
  format.
- **Architecture checks required:** if a new layer is added, full Layer-Law
  compliance (`layer.toml`, proof exports, DAG, dylint genuine-dep); 100% coverage
  + zero branches on the new spine code.
- **Golden/invariant tests:** the hashing primitive must reproduce a committed
  digest for a committed byte vector (a golden of the hash itself).
- **Completion criteria:** every Phase 1 golden re-expressed as a `GoldenRun`;
  hashing primitive covered 100%, branchless; format versioned.
- **Risks:** picking a non-stable hash (anything using `HashMap` iteration, host
  endianness, or `f32` bit patterns without canonicalization); over-building the
  kernel.
- **Future agents must not:** add hashing to a layer that doesn't genuinely use it
  (ceremonial dep); use the hash as the equality proof; reach for a third-party
  crypto-hash crate without a recorded justification (the kernel has zero deps by
  design).

---

### Phase 3 — `axiom-space` layer for deterministic addresses

- **Goal:** A stable, hashable, serializable **address** primitive naming *what/
  where* is generated (chunk coords, hierarchical region/content paths), shared by
  every future generator.
- **Architectural placement:** `Layer: space`, root-adjacent
  (`depends_on = ["kernel"]`), beside `crypto`/`interface`.
- **Crates/modules/apps/tools touched:** new `crates/axiom-space` (+ `layer.toml`,
  + root `Cargo.toml` members).
- **Public capabilities introduced:** `SpaceApi`, `Address` (and the address id
  newtypes it traffics in), address hashing/serialization. (Generalizes
  `axiom-growth`'s app-local `ChunkCoord`/`RegionId`/`PlateId` from
  `apps/axiom-growth/src/ids.rs`.)
- **Consumed lower-layer capabilities:** kernel `HandleId`/ids, `BinaryWriter`/
  `BinaryReader`, `SchemaVersion`, the Phase-2 stable hash.
- **Non-goals:** no coordinates *of* geometry semantics (no world transforms — that
  is `math`); no generation; no entropy; no terrain concepts. An address names a
  site; it does not generate one.
- **Tests required:** address equality/ordering determinism, byte round-trip,
  hash stability, hierarchical composition (parent/child addresses).
- **Architecture checks required:** Layer Law (`layer.toml`, ≥1 proof export
  referencing a kernel symbol, DAG acyclic, genuine-dep dylint); 100% coverage;
  zero branches.
- **Golden/invariant tests:** an address's serialized bytes + hash are golden;
  invariant: equal addresses ⇒ equal hash, distinct addresses ⇒ distinct hash
  (collision-free over a swept domain).
- **Completion criteria:** `space` is a green layer; `check-architecture` reports
  it in the graph; covered and branchless.
- **Risks:** smuggling geometry or domain meaning into `space` (it must stay a
  naming primitive); arguing it into the kernel (it's a capability, not a bare
  scalar — keep the kernel small).
- **Future agents must not:** make `space` depend on `math`/`entropy`/`proc`
  (it is below them); add web APIs; add generation.

---

### Phase 4 — `axiom-entropy` layer for explicit deterministic entropy streams

- **Goal:** Address- and version-keyed **entropy streams** — expand one seed into
  independent, reproducible sub-streams per address, so two sites never share
  state and a site's stream is stable across runs.
- **Architectural placement:** `Layer: entropy`, `depends_on = ["kernel", "space"]`.
- **Crates/modules/apps/tools touched:** new `crates/axiom-entropy`.
- **Public capabilities introduced:** `EntropyApi`, an entropy-stream type keyed
  by `(seed, Address, version)`, sub-stream derivation. (Generalizes
  `axiom-growth`'s forking `Rng::fork(salt)` from `apps/axiom-growth/src/rng.rs`.)
- **Consumed lower-layer capabilities:** kernel `DeterministicRng`; `space`
  `Address`; Phase-2 stable hash for keying.
- **Non-goals:** **no new RNG algorithm** beyond the kernel's `DeterministicRng`;
  no noise functions (noise graduates later, in a domain module, Phase 9); no
  ambient entropy. This layer *routes and keys* the kernel's existing determinism;
  it does not invent randomness.
- **Tests required:** same `(seed, address, version)` ⇒ identical stream;
  different address or version ⇒ statistically independent, non-overlapping
  streams; cross-platform byte stability; sub-stream isolation.
- **Architecture checks required:** Layer Law (proof export must reference a
  `space` *and/or* kernel symbol genuinely); 100% coverage; zero branches.
- **Golden/invariant tests:** golden first-N values for fixed `(seed, address,
  version)`; invariant: stream(addr A) and stream(addr B) never coincide on a
  swept address set; metamorphic: bumping `version` changes the stream,
  re-using it restores it.
- **Completion criteria:** green `entropy` layer; goldened streams; covered,
  branchless.
- **Risks:** re-deriving streams in a way that correlates neighboring addresses
  (defeats independence); reaching for wall-clock/OS entropy.
- **Future agents must not:** add noise/terrain here; introduce a second RNG; let
  entropy depend on `proc` (it is below it).

---

### Phase 5 — `axiom-proc` layer for recipes, graph evaluation, artifacts, and traces

- **Goal:** The generic **proc graph**: declare a recipe as a DAG of nodes, evaluate
  it deterministically over entropy + parameters at an address, emit a canonical
  **artifact** (bytes) and a **trace** (recorded decisions), within an explicit
  **budget** (chunked/incremental). This is the engine's generation core — *domain-
  free*.
- **Architectural placement:** `Layer: proc`,
  `depends_on = ["kernel", "space", "entropy"]` (+ `math` **only if** evaluation
  genuinely references geometry in non-test code — decide by use, never declare
  ceremonially; see audit §11 open question).
- **Crates/modules/apps/tools touched:** new `crates/axiom-proc`.
- **Public capabilities introduced:** `ProcApi`, recipe/graph node model, a
  generic `Artifact` (canonical bytes + `SchemaVersion` + generator version), a
  `ProcTrace`, a **budgeted/incremental evaluation** contract. (Generalizes
  `axiom-growth`'s `Stage`/`StageRegistry`/`Pipeline` from
  `apps/axiom-growth/src/pipeline.rs`, and is informed by — but distinct from —
  `sim-core`'s process/effect model, which is a module and cannot be a shared
  substrate.)
- **Consumed lower-layer capabilities:** `space` addresses, `entropy` streams,
  kernel serialization + Phase-2 hash + replay primitives + `TickDivider` cadence
  for budgeting.
- **Non-goals:** **no domain content** — no terrain, biome, mesh, level meaning.
  The artifact is opaque/neutral data; what it *means* is a domain module's job
  (Phase 9). No browser APIs. No noise functions.
- **Tests required:** deterministic evaluation (same inputs ⇒ identical artifact +
  trace bytes); DAG validity (acyclic recipes; cycle rejected as data, not a
  panic); budget honored (an evaluation step yields and resumes producing the same
  result as an unbudgeted run); incremental re-eval reproduces full re-eval;
  trace ↔ artifact consistency.
- **Architecture checks required:** Layer Law; the proof export must reference an
  `entropy`/`space` symbol genuinely; 100% coverage; zero branches (a DAG fold is
  naturally branchless — express node dispatch as table/iterator selection, not
  `match` over node kinds in spine code).
- **Golden/invariant/metamorphic tests:** golden artifact + trace bytes for a
  fixed trivial recipe; invariant: artifact hash is independent of evaluation
  *budget/chunking* (incremental == whole); metamorphic: bumping recipe version
  changes the artifact, restoring it restores the bytes.
- **Completion criteria:** green `proc` layer producing a hashable artifact +
  trace for a trivial recipe; budgeted + incremental contract proven equal to
  whole-evaluation; covered, branchless.
- **Risks:** the branchless requirement on a graph evaluator (resolve by
  data-driven node dispatch, the same way the spine already avoids `match`);
  smuggling domain meaning in; an under-specified budget contract that lets a step
  run unbounded.
- **Future agents must not:** add terrain/biome/noise here; make `proc` a junk
  drawer of generation helpers; declare `math` without genuine use.

---

### Phase 6 — `axiom-proc-validate` layer for constraints, scoring, validation, repair hooks

- **Goal:** Make generated artifacts **trustworthy**: declarative constraints,
  scoring, validation verdicts, and *repair hooks* (a constraint can request a
  bounded regeneration/adjustment) over `proc` artifacts/graphs.
- **Architectural placement:** `Layer: proc-validate`,
  `depends_on = ["kernel", "proc"]`.
- **Crates/modules/apps/tools touched:** new `crates/axiom-proc-validate`.
- **Public capabilities introduced:** `ProcValidateApi`, constraint/score model,
  `ValidationReport`, repair-hook contract.
- **Consumed lower-layer capabilities:** `proc` artifacts/traces; kernel
  result/error model + serialization.
- **Non-goals:** no domain constraints (no "rivers must reach the sea" — that's a
  terrain module); no generation of new content beyond bounded repair invoked
  through `proc`; no browser APIs.
- **Tests required:** deterministic validation verdicts; a constraint's pass/fail
  is a pure function of artifact bytes; repair hook produces a deterministic,
  re-validatable artifact; scoring is stable and ordered.
- **Architecture checks required:** Layer Law; proof export references a `proc`
  symbol; 100% coverage; zero branches.
- **Golden/invariant/metamorphic tests:** golden `ValidationReport` bytes for a
  fixed artifact; invariant: validating identical artifacts yields identical
  reports; metamorphic: a known-good artifact passes, a perturbed one fails at the
  expected constraint.
- **Completion criteria:** green `proc-validate` layer; goldened reports; covered,
  branchless.
- **Risks:** validation that is itself nondeterministic (ordering of constraint
  evaluation must be stable); repair that loops unbounded (must be budgeted like
  `proc` evaluation).
- **Future agents must not:** put domain rules in this generic layer; let repair
  bypass the `proc` budget contract.

---

### Phase 7 — First procedural playground app

- **Goal:** Prove the `space → entropy → proc → proc-validate` stack end-to-end
  with a tiny, fully-deterministic, golden-tested generated artifact in an app
  (and, where it renders, in the browser).
- **Architectural placement:** `App` — a new `apps/axiom-proc-playground` (leaf;
  exempt from spine gates but ships its own tests).
- **Crates/modules/apps/tools touched:** new app composing the four new layers
  (and `engine`/`windowing` if it renders).
- **Public capabilities introduced:** none (app).
- **Consumed lower-layer capabilities:** `SpaceApi`, `EntropyApi`, `ProcApi`,
  `ProcValidateApi`, plus presentation modules if visual.
- **Non-goals:** no terrain/biome (Phase 9); no production content; keep the recipe
  trivial (e.g. a deterministic colored-grid or cube-field artifact) — the point is
  the *pipeline*, not the content.
- **Tests required:** golden artifact + hash + trace for fixed `(seed, address,
  parameters, version)`; replay byte-equality; if visual, a browser smoke via the
  Playwright controller.
- **Architecture checks required:** `check-architecture` classifies the new app;
  app composes only declared layers/modules.
- **Golden/invariant tests:** the playground's artifact list, proc-graph hashes,
  and (if rendered) frame hashes captured as a `GoldenRun`.
- **Completion criteria:** the app regenerates byte-identical content across runs
  and commits; `GoldenRun` committed; browser smoke (if visual) green.
- **Risks:** letting the playground accrete real generation logic that belongs in a
  module/layer; visual nondeterminism leaking through a backend.
- **Future agents must not:** grow domain generation in the app; treat the
  playground's final-frame hash as the *only* golden (compare boundaries
  independently — test plan).

---

### Phase 8 — Migrate one existing hardcoded demo into a procedural recipe

- **Goal:** Replace one app's hand-authored content with a `proc` recipe driven by
  `(seed, address, parameters, version)`, proving the substrate can express *real*
  existing content and that the migrated output is golden-stable.
- **Architectural placement:** `App` change (recipe glue in the app, or in a
  feature module if it composes domain modules later). First target:
  `apps/axiom-stress-cubes-browser` (smallest deterministic scene; cube field
  becomes `recipe(seed, address) → placements`). Headless-pure alternative:
  `apps/axiom-quintet`'s seeded piece generation.
- **Crates/modules/apps/tools touched:** the chosen app; the `proc` layer (recipe
  authoring only — no new layer capability unless a genuine gap appears).
- **Public capabilities introduced:** none new (consumes Phase 5/6).
- **Consumed lower-layer capabilities:** `ProcApi` (+ `space`/`entropy`).
- **Non-goals:** **do not** migrate `axiom-growth` here — it is the eventual large
  proving ground (audit §13), not the first migration; no terrain modules yet; do
  not change the app's *observable* output if it can be preserved (so the existing
  golden still passes), or, if it must change, bump version and regolden
  deliberately.
- **Tests required:** the migrated app's pre-existing determinism/golden tests
  still pass (or are intentionally re-goldened with a version bump); a new test that
  the same seed reproduces the same scene and a different seed changes it.
- **Architecture checks required:** unchanged app classification; no new
  cross-module dependency introduced.
- **Golden/invariant/metamorphic tests:** seed-sweep (N seeds each reproducible);
  metamorphic (seed+1 changes layout, same seed restores it); the app's boundary
  goldens.
- **Completion criteria:** the app's content is recipe-driven, byte-reproducible,
  and golden-tested; old hardcoded path removed.
- **Risks:** the recipe being less deterministic than the hardcoded path (catch via
  the pre-existing golden); scope-creeping into `growth`.
- **Future agents must not:** migrate `growth` as "one demo"; weaken the app's
  existing golden to make the recipe pass.

---

### Phase 9 — terrain / biome / placement modules

- **Goal:** Build the first **domain** generators as engine modules on top of the
  `proc` substrate: `terrain` (heightfield/voxel artifact), `biome`
  (classification artifact), `placement`/`vegetation`/`structures` (object
  placement artifact). This is where **noise functions and the icosphere graduate
  from `axiom-growth`** — with their own tests — since the hard constraints forbade
  adding them during planning.
- **Architectural placement:** `Engine modules` (`allowed_modules = []`), each
  consuming the `proc`/`proc-validate`/`space`/`entropy`/`math` *layers*. A
  composed "world recipe" (terrain+biome+placement together) is a **feature
  module** (`levelgen`) or app glue — never an engine module depending on another.
- **Crates/modules/apps/tools touched:** new `modules/axiom-terrain`,
  `modules/axiom-biome`, `modules/axiom-placement` (and later `vegetation`,
  `structures`, `meshgen`); optionally `modules/axiom-levelgen`
  (`kind = "feature-module"`). Graduation source: `apps/axiom-growth/src/{noise,
  topology,gameworld}.rs`.
- **Public capabilities introduced:** per module, one facade producing a
  domain artifact contract (e.g. `TerrainApi` → heightfield artifact).
- **Consumed lower-layer capabilities:** `ProcApi`, `EntropyApi`, `SpaceApi`,
  `MathApi`, `ProcValidateApi`.
- **Non-goals:** no module imports another module; no browser APIs in these
  modules; meshgen produces neutral mesh *data*, not GPU calls.
- **Tests required:** per module, deterministic artifact goldens; seam/continuity
  invariants (the audit's `shared_edge_seam_is_zero` discipline); validation
  integration.
- **Architecture checks required:** Module Law (one facade, isolated, unique
  capabilities); 100% coverage; zero branches per module; platform-API ban.
- **Golden/invariant/metamorphic tests:** terrain seam-coherence invariant;
  biome-classification metamorphic (shifting input by ε keeps classification
  stable except at boundaries); placement determinism golden.
- **Completion criteria:** each module is green, covered, branchless, and produces
  golden-stable artifacts; the noise/icosphere primitives are now spine-tested, not
  app-local.
- **Risks:** a domain module needing another domain module (resolve by extracting
  the shared primitive *down* into a layer, never a module→module edge); branchless
  noise/icosphere (express as table/iterator math).
- **Future agents must not:** create a module→module dependency; leave noise in an
  app; smuggle browser APIs into a generation module.

---

### Phase 10 — Procedural debug / inspection tooling

- **Goal:** Make generation **inspectable**: visualize/inspect proc graphs,
  artifacts, traces, validation reports, and the seed→address→artifact provenance.
- **Architectural placement:** split by surface — a **native inspector CLI** in
  `tools/` (beside `axiom-shot`), and/or a **browser proc-trace overlay** as a
  platform-facing *module* composing the `interface` layer (the `debug-overlay`
  precedent, allowlisted in `PLATFORM_FACING_MODULES`).
- **Crates/modules/apps/tools touched:** new `tools/axiom-proc-inspect`; optionally
  a new `modules/axiom-proc-overlay`.
- **Public capabilities introduced:** tool surface (outside the engine graph) +,
  if a module, one overlay facade.
- **Consumed lower-layer capabilities:** `ProcApi`/`ProcValidateApi` (read-only),
  `interface` (overlay).
- **Non-goals:** the *generic* proc layers must never gain inspection/IO code; web
  APIs stay out of the proc layers; the CLI is not part of the engine graph.
- **Tests required:** the tool/overlay renders a fixed trace/artifact
  deterministically; overlay core branchless + covered (module), CLI tested but
  gate-exempt (tool).
- **Architecture checks required:** tool excluded from the engine graph; overlay
  module obeys Module Law + platform allowlist.
- **Golden tests:** a fixed proc trace renders to a golden inspection output.
- **Completion criteria:** an agent can dump/inspect any generated artifact's
  provenance from `(seed, address, version)`.
- **Risks:** inspection logic leaking into the proc spine; overlay touching web
  APIs outside the allowlisted arm.
- **Future agents must not:** add inspection/IO to `proc`/`proc-validate`; add a
  new platform-facing module without amending the allowlist deliberately.

---

### Phase 11 — Long-run determinism, fuzz, and performance gates

- **Goal:** Turn determinism, budget, and provenance from per-test checks into
  *gates*: large seed sweeps, fuzz/property tests, long-run replay, and
  browser-budget tests that fail CI on a hitch.
- **Architectural placement:** `Test/Harness` + `Tools` (fuzz/perf harnesses,
  e.g. extend `tools/axiom-profile-runner`); browser checks via the Playwright
  controller.
- **Crates/modules/apps/tools touched:** test harnesses across the new layers/
  modules; `tools/axiom-profile-runner`; CI wiring.
- **Public capabilities introduced:** none.
- **Consumed lower-layer capabilities:** all proc layers/modules.
- **Non-goals:** no new generation; no relaxing of the budget contract to pass a
  perf test (fix the generator, not the gate).
- **Tests required:** seed-sweep determinism over thousands of seeds; property/fuzz
  tests (random valid recipes/addresses never panic, always reproduce); long-run
  replay (generate → serialize → regenerate, byte-equal); browser-budget test
  (per-frame generation stays under a documented ms budget; the
  `terrain-streaming-stutter` regression is gated).
- **Architecture checks required:** unchanged; gates wired into CI like
  `scripts/coverage.ps1`.
- **Golden/invariant/metamorphic tests:** the full test-plan taxonomy applied at
  scale; provenance tests (every artifact's recorded `(seed, address, version,
  manifest hash)` reproduces it).
- **Completion criteria:** CI fails on determinism drift, an unbudgeted frame, or a
  provenance mismatch; sweeps run in reasonable time.
- **Risks:** flaky perf gates (budget must be a stable, documented threshold);
  fuzz finding genuine nondeterminism late (good — that's the point).
- **Future agents must not:** widen the budget to silence a regression; sample-cap a
  sweep silently (the test plan requires logging any dropped coverage).

---

### Phase 12 — Save/delta model and server/browser parity

- **Goal:** Realize the save and multiplayer payoff: a save is `{seed, versions,
  parameters, player_deltas}`; a multiplayer session is `{seed, versions, command
  stream, authoritative deltas}` — never full world state — with byte-identical
  regeneration on server and browser.
- **Architectural placement:** `Feature module`/`App` for the save/delta model
  built on the proc layers; integration with `modules/axiom-netcode` (lockstep) and
  `modules/axiom-net-protocol`/`axiom-client-core` (server-authoritative).
- **Crates/modules/apps/tools touched:** a save/delta feature module or app glue;
  `netcode`/`net-protocol` integration; a parity harness.
- **Public capabilities introduced:** save/delta format (versioned), regeneration-
  from-save, parity verification.
- **Consumed lower-layer capabilities:** all proc layers; `netcode` state-hash
  reconciliation; kernel serialization/replay.
- **Non-goals:** no storing of full generated worlds; no new netcode stack (reuse
  the two existing ones); no gameplay-specific delta semantics in the generic
  layers.
- **Tests required:** save round-trip (generate → save → load+regenerate+apply
  deltas → byte-equal world); server/browser parity (same seed+versions+commands ⇒
  identical state hash on native and wasm); delta application determinism.
- **Architecture checks required:** save/delta module obeys Module/Feature-Module
  Law; no new cross-module edge except via a feature module.
- **Golden/invariant/metamorphic tests:** parity invariant (native hash == wasm
  hash for fixed inputs); save metamorphic (deltas applied then regenerated equal
  the live world); long-run lockstep determinism (extends the existing
  `axiom-netcode-demo` proof to generated worlds).
- **Completion criteria:** a world reproduces from `{seed, versions, deltas}`
  byte-for-byte; native and browser agree; lockstep over a generated world holds.
- **Risks:** the smallest generation-determinism break breaking parity (the netcode
  state-hash reconciliation is the safety net — it surfaces it precisely); version
  drift between server and client (versions are explicit inputs — enforce match).
- **Future agents must not:** store full worlds "to be safe"; ship full state over
  the wire; let server and browser diverge on generator version.

---

## Cross-cutting rules every phase obeys

- **Never weaken a law.** Layer Law, Module Law, Branchless Law, Coverage Law, the
  platform-API ban, and the no-junk-drawer rule apply to every new layer/module
  from its first commit. Apps and tools are the only gate-exempt tiers.
- **Genuine dependencies only.** A new layer declares a lower layer in `depends_on`
  only if it *genuinely uses* it in non-test code. Anticipated use is not use.
- **Hashes index; bytes prove.** Adopt the engine's existing stance
  (`modules/axiom-recording`): never substitute a hash comparison for the byte
  comparison as the proof of determinism.
- **Compare boundaries independently.** A final-frame hash is necessary but not
  sufficient; hash scene snapshot, render input, command list, artifact list, and
  proc trace separately (test plan).
- **Versioning is an input, not metadata.** Any behavior change to a generator
  bumps its version and regolden is deliberate.
- **No shortcuts.** If generation logic cannot be expressed cleanly in the correct
  tier, that is a design signal — reshape the boundary, do not route around it.
