# Procedural Generation — Test Plan

> The testing contract for the procedural pivot described in
> [`PROCEDURAL_GENERATION_ROADMAP.md`](PROCEDURAL_GENERATION_ROADMAP.md), grounded
> in the mechanisms catalogued in
> [`PROCEDURAL_GENERATION_REPOSITORY_AUDIT.md`](PROCEDURAL_GENERATION_REPOSITORY_AUDIT.md).
> No procedural generation is implemented here; this defines how it *will* be
> tested. The guiding stance is the engine's existing one: **byte equality is the
> proof; hashes index and label.**

---

## Why this engine is unusually testable

Axiom already gives every artifact a deterministic byte form (`BinaryWriter`/
`Reflect`, `crates/axiom-kernel`), already proves byte-equal replay at six
boundaries (`apps/axiom-demo-rotating-cube/src/vertical_slice.rs`), and already
has an opaque-byte recorder that localizes the first divergence to (frame,
artifact, byte) (`modules/axiom-recording/`). Procedural generation testing is
that same discipline pointed at *generated* bytes — plus the classes of test
(invariant, metamorphic, seed-sweep) that generation specifically needs.

---

## 1. Golden-master testing

A **golden master** is the committed canonical bytes (and their stable hash) of an
artifact produced from fixed inputs. A test regenerates the artifact and asserts
byte-equality against the committed golden. A mismatch is either a bug (fix the
code) or an intended change (bump the generator version and re-golden
deliberately). This replaces today's *in-memory* "run twice, assert equal" with a
*cross-commit* baseline (audit §6: no stored goldens exist yet).

## 2. Golden-run capture

A **golden run** drives an app/recipe through a fixed input script and captures the
deterministic artifact at *every* boundary into one versioned `GoldenRun` envelope
(see the GoldenRun shape below). Capture reuses the `recording` module's
opaque-byte machinery. Capture is explicit and reviewable; a `GoldenRun` is data,
committed alongside the test that regenerates and diffs it.

## 3. Artifact boundary comparison

Every clean boundary in the flow `Seed → Address → Entropy → Proc Graph →
Validation → Artifact → Hash → Trace` (and, for rendered output, the existing
Scene → Resources → RenderInput → RenderCommandList → GpuSubmission boundaries) is
compared **independently**. When a golden diff fires, the boundary that first
differs localizes the regression — exactly as `DeterminismReport` localizes a
recording divergence to a byte index (`modules/axiom-recording/src/
determinism_report.rs`).

## 4. Deterministic hashing

Each artifact gets a **stable hash** over its canonical bytes (Phase 2 hashing
primitive). The hash must be: stable (same bytes → same hash on every platform,
every run), sensitive (one bit flips it), and free of host endianness, `HashMap`
iteration order, and uncanonicalized `f32` bit patterns. The hash is the **label/
index** for a golden and a fast inequality check — never the equality *proof*.

## 5. Invariant testing

Assert *semantic* properties that must hold for **all** seeds/addresses without
pinning exact bytes: e.g. terrain shared-edge seams are zero
(`apps/axiom-growth` already tests `shared_edge_seam_is_zero`), an address maps to
exactly one region, entropy sub-streams for distinct addresses never coincide, a
validated artifact always satisfies its constraints, a proc DAG is always acyclic.
Invariants catch whole classes of bug that a single golden cannot.

## 6. Metamorphic testing

Assert how outputs *relate* under input transformations, without an oracle for the
absolute output: bumping a generator **version** changes the artifact and
restoring it restores the bytes; `seed+1` changes the world but `seed` reproduces
it; perturbing a biome input by ε keeps classification stable except at
boundaries; evaluating a recipe with a different **budget/chunking** yields the
*same* artifact (incremental == whole). Metamorphic relations are the strongest
test of generation correctness because they need no hand-authored expected output.

## 7. Seed-sweep testing

Run a recipe over a large set of seeds (and addresses) and assert every run is
self-reproducible and satisfies all invariants. Sweeps find seeds that trip rare
branches, overflow a budget, or break an invariant. **If a sweep is sampled/capped
for time, the cap is logged** (the No-Silent-Caps rule) — a silent cap reads as
"all seeds pass" when it didn't.

## 8. Replay testing

Generate → serialize → regenerate from the serialized inputs → assert byte-equal,
including long runs and fork-and-resume (the pattern
`apps/axiom-retro-fps-browser/tests/replay_determinism.rs` already uses via
`write_state`/`read_state`). Replay proves a generator has no hidden state and that
`(seed, address, parameters, version)` fully determines output.

## 9. Serialization round-trip testing

Every artifact, trace, validation report, `GoldenRun`, and save/delta payload must
satisfy `from_bytes(to_bytes(x)) == x` and `to_bytes(from_bytes(b)) == b`, with a
`SchemaVersion` header (the `FrameReport` precedent,
`crates/axiom-introspect/src/frame_report.rs`). Round-trip tests guard the wire
format the goldens and saves depend on.

## 10. Architecture boundary tests

`cargo xtask check-architecture` must stay green: every new layer obeys the Layer
Law (DAG, genuine deps, proof exports), every new module obeys the Module Law (one
facade, isolated, unique capabilities, platform-API ban, no junk drawers). The
`engine_genuine_dependency` and `engine_no_branching` dylints and the 100%-coverage
gate apply to all new spine code. These are tests, run on every commit.

## 11. Browser budget tests

Drive the built wasm app with the Playwright controller
(`scripts/playwright_controller.py`) and assert generation never blocks a frame
beyond a documented budget — the engine-level gate against the
`docs/growth-port/terrain-streaming-stutter.md` 300 ms hitch. Verify chunked,
budgeted, incremental generation: re-centering regenerates only newly-exposed
sites, and per-frame generation stays under the threshold.

## 12. Generated-artifact provenance tests

Every artifact records its provenance — `(seed, address, parameters, version,
generator manifest hash)` — and a provenance test asserts that re-running the
recorded provenance reproduces the artifact byte-for-byte, and that the trace
(Phase 5) explains the artifact (trace ↔ artifact consistency). Provenance is what
makes saves and multiplayer (store/ship seed + deltas, not full state) trustworthy.

---

## How to update goldens intentionally

Goldens are never edited by hand to make a red test green. The only sanctioned
update path:

1. Confirm the output change is **intended** (not a determinism bug).
2. **Bump the generator/recipe version** (and `SchemaVersion` if the byte format
   changed). Versioning is a first-class input — an unbumped behavior change is a
   silent golden-rot bug.
3. Regenerate goldens through the **documented capture path** (a flag/test mode
   that re-emits `GoldenRun` bytes), not by editing files.
4. Review the golden diff as part of the change — the diff is the evidence the
   change is what was intended.
5. Land code + regenerated goldens + version bump in one change.

A golden update with no version bump and no reviewed diff is a red flag, the same
way a coverage drop is.

---

## How to avoid overfitting to visual final-frame hashes only

A single final-frame hash is a coarse oracle: it changes for *any* reason
(generation, rendering, a backend tweak) and tells you only *that* something
changed, never *what*. Relying on it alone:

- hides which boundary regressed (was it generation or rendering?);
- breaks on benign rendering changes, training agents to re-golden reflexively;
- can pass while an intermediate artifact is subtly wrong but happens to render
  identically.

Mitigation: **always** capture and compare the intermediate boundaries too, and
lean on invariant + metamorphic tests (which pin *meaning*, not pixels) so the
suite is not a pile of brittle exact-byte frame hashes.

## Why final-frame hashes are useful but insufficient

Useful: a final-frame hash is the cheapest end-to-end "did anything change at all"
tripwire and the closest proxy for "what the player sees." Insufficient: it cannot
localize a regression, conflates generation with rendering, and is silent about
intermediate correctness. Keep it as the outermost tripwire, never as the only
golden.

## Why intermediate boundary hashes are compared independently

When the boundaries exist, **scene snapshot, render input, render command list,
generated artifact list, and proc trace hashes are compared independently** because:

- a regression localizes to the *first* boundary that differs (generation vs
  rendering vs submission), instead of a single opaque "frame changed";
- a benign change at one boundary doesn't force re-goldening all the others;
- an intermediate artifact can be wrong while the final frame is coincidentally
  identical — only an independent intermediate hash catches it.

This mirrors the existing six-boundary slice
(`apps/axiom-demo-rotating-cube/src/vertical_slice.rs`), where each artifact is its
own `PartialEq` comparison, not one combined blob.

---

## The future `GoldenRun` shape (prose)

A `GoldenRun` is a versioned, byte-serializable envelope (a `SchemaVersion` header,
then little-endian fields via the kernel's `BinaryWriter`). It records everything
needed to reproduce and diff a deterministic run. Its fields:

- **app id** — which app/recipe produced the run (stable string/id).
- **engine version** — the engine build/version the run was captured against, so a
  stale golden is recognizable.
- **generator manifest hash** — a stable hash over the set of generator/recipe
  versions and parameters in effect, so any change to *how* content is generated is
  visible in one field.
- **seed** — the root seed (`u64` or hashed-to-`u64`).
- **input script hash** — a stable hash of the fixed input/intent script that drove
  the run (the retro_fps replay tests already drive a fixed intent script).
- **frame count** — number of frames/ticks captured.
- **runtime hashes** — per-frame hash of the runtime step record(s)
  (`RuntimeStepRecord`).
- **proc graph hashes** — per-generation hash of the evaluated proc graph
  (recipe structure + node results) at each address generated.
- **generated artifact hashes** — per-artifact hash of each generated content
  artifact (terrain chunk, biome map, placement list, …).
- **scene snapshot hashes** — per-frame hash of the `SceneSnapshot` boundary (where
  the run renders).
- **render input hashes** — per-frame hash of the `RenderInput` boundary.
- **render command hashes** — per-frame hash of the `RenderCommandList` boundary.
- **frame hashes** — per-frame final-frame hash (the outermost tripwire).
- **provenance/trace hashes** — per-artifact hash of the proc **trace** and the
  recorded provenance tuple `(seed, address, parameters, version, manifest hash)`.

Each hash list is compared independently (above). Two `GoldenRun`s for the same
inputs must be byte-equal; a diff localizes to the first list/index that differs.
The envelope round-trips (`from_bytes(to_bytes(g)) == g`) and is the unit the
golden-update workflow regenerates.

---

## Exact classes of tests

Every new layer/module ships with the relevant classes from this taxonomy; the
class names are the vocabulary the phase checklists use.

| Class | What it pins | Oracle | Example for the pivot |
|-------|--------------|--------|------------------------|
| **Exact golden tests** | exact canonical bytes / stable hash of an artifact at fixed inputs | committed golden bytes | rotating-cube six-boundary `GoldenRun`; a recipe's artifact bytes for `(seed, address, version)` |
| **Semantic invariant tests** | a property true for *all* inputs | a predicate, no expected output | terrain seam == 0; one address → one region; entropy sub-streams never coincide; proc DAG acyclic; validated artifact satisfies constraints |
| **Metamorphic tests** | how outputs relate under input transforms | a relation, no expected output | `seed+1` differs / `seed` reproduces; budget/chunking-invariant artifact; version bump changes then restores bytes; ε-perturbation keeps biome stable except at edges |
| **Fuzz/property tests** | "never panics, always reproduces" over random valid inputs | generated inputs + invariants | random valid recipes/addresses/seeds → always reproducible, never overflow a budget or panic |
| **Visual sanity tests** | the wasm build actually renders | human/Playwright screenshot + console | playground/migrated app paints; no console errors (`scripts/playwright_controller.py`) |
| **Performance/budget tests** | per-frame generation under a documented ms budget; incremental re-gen reuses neighbors | a threshold + a work-count assertion | re-centering regenerates only exposed sites; no frame exceeds budget (gates the streaming-stutter regression) |
| **Architecture/hygiene tests** | the laws hold | `cargo xtask check-architecture`, dylints, coverage gate | new layers are a genuine-dep DAG; new modules are isolated, branchless, 100% covered, platform-API-clean |

---

## Where each class attaches, by phase (summary)

- **Phase 1–2:** exact golden, serialization round-trip, deterministic hashing,
  architecture/hygiene.
- **Phase 3 (space):** exact golden (address bytes/hash), invariant (collision-free
  addressing), round-trip, architecture.
- **Phase 4 (entropy):** exact golden (stream values), invariant (sub-stream
  independence), metamorphic (version keying), architecture.
- **Phase 5 (proc):** exact golden (artifact + trace), invariant (DAG acyclic),
  metamorphic (budget-invariant, version), architecture.
- **Phase 6 (proc-validate):** exact golden (report), invariant (constraint
  satisfaction), metamorphic (good passes / perturbed fails), architecture.
- **Phase 7 (playground):** golden-run, replay, visual sanity, architecture.
- **Phase 8 (migration):** exact golden (preserved or re-goldened), seed-sweep,
  metamorphic, architecture.
- **Phase 9 (domain modules):** exact golden (artifacts), invariant (seam/
  continuity/classification), metamorphic, fuzz, architecture.
- **Phase 10 (inspection):** exact golden (inspection output), architecture.
- **Phase 11 (gates):** seed-sweep at scale, fuzz/property, performance/budget,
  provenance, long-run replay.
- **Phase 12 (save/parity):** serialization round-trip (save/delta), metamorphic
  (deltas+regenerate == live), invariant (native hash == wasm hash), long-run
  lockstep determinism.
