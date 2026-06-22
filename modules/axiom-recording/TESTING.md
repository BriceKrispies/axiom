# `axiom-recording` — testing

The module ships at **100% coverage** (regions, lines, functions, branches) and
proves its determinism and architectural boundaries with tests. New code lands
with the tests that cover all of it — there is no "later".

## Running

```sh
# Unit + boundary + replay tests
cargo test -p axiom-recording

# 100% coverage gate (nightly populates the branch column)
cargo +nightly llvm-cov --no-cfg-coverage -p axiom-recording --branch --summary-only

# Branchless / no-unwrap / module-doc dylints
cargo dylint --all -- -p axiom-recording --all-targets

# Whole-workspace architecture + module law
cargo xtask check-architecture
```

The repo-wide gate is `scripts/coverage.ps1` (Windows) / `bash scripts/coverage.sh`
(CI), which runs the instrumented workspace and fails below 100% on the engine
spine.

## What is covered

### Unit tests (in each `src/*.rs` `#[cfg(test)]` module)

* **`hash.rs`** — empty input hashes to the FNV offset basis; the hash is
  deterministic for identical bytes, differs for different bytes, and is
  order-sensitive (both the byte and the word fold).
* **`error.rs`** — every error constructor carries the `(Memory, OutOfBounds)`
  identity and a non-empty diagnostic message.
* **`frame_capture.rs`** — the constructor stores identity + every payload;
  `byte_len` accounts for all four byte arrays; `is_empty_payload` detection;
  identical payloads hash identically; changing one artifact changes only that
  artifact's hash (plus `final_hash`); `final_hash` is identity-sensitive.
* **`frame_timeline.rs`** — zero `max_frames`/`max_bytes` rejected; insertion
  order preserved; `max_frames` eviction; `max_bytes` eviction; a single
  over-budget capture rejected without mutation; `current_bytes` tracks
  push + eviction; fetch present vs. evicted vs. never-recorded; latest/oldest
  (incl. empty errors); previous/next navigation and its edges/missing-selection;
  `clear`; the bound getters.
* **`timeline_mode.rs`** — `Live` is live with no selection; `Scrubbing` carries
  and compares its selection; `Copy`/`Eq`/`Debug`.
* **`artifact_kind.rs`** — the five kinds are distinct, `Copy`, and `Debug`.
* **`determinism_report.rs`** — the matched report is empty; identical timelines
  match; `first_byte_diff` locates a byte or a length difference; input / runtime
  / state / render divergences are each located with the right index; a `tick`
  divergence reports `Final` with no byte index; a later-frame divergence is
  found; length and aligned-frame-index mismatches are structural errors.
* **`recording_api.rs`** — the documented default budgets; zero-bound rejection;
  a fresh recorder is empty + live; recording grows the timeline and tracks
  bytes; an over-budget capture is rejected; fetching exposes the opaque payload;
  `enter_scrub` selects a present frame / fails on a missing one; **scrubbing does
  not mutate the timeline**; `step_back` from live walks back from the latest and
  stops at the oldest edge; `step_forward` walks toward the latest and stops at
  its edge; stepping an empty timeline errors; `resume`; `clear`; comparison of
  identical / divergent / different-length recordings; `Debug`/`Clone`.

### Boundary tests (`tests/architecture.rs`)

Scans `src/` (comments and string literals stripped first) to fail at
`cargo test` time on any regression:

* `module.toml` is an isolated engine module (`allowed_modules = []`).
* `lib.rs` publicly exports **exactly one** facade (`RecordingApi`) and declares
  no `pub mod`.
* imports only the kernel layer; no other module, app, or tool is referenced.
* no browser/DOM/GPU tokens; no wall-clock/randomness; no console/placeholder
  macros; no global mutable state or file IO; no render/scene/input/pixel/
  screenshot/compression concepts; no `utils`/`helpers`/`common`/`misc` modules.

### Replay determinism (`tests/replay_determinism.rs`)

The engine's replay-determinism proof at the recorder's own boundary — the
lowest deterministic boundary that owns canonical artifacts, independent of any
app/renderer/GPU:

* A **fixed initial state** and a **fixed scenario** of synthetic input packets
  are run through a tiny deterministic simulation, recording opaque canonical
  bytes (input / runtime / state / render) for every frame into timeline A.
* The same scenario is replayed from the same initial state into timeline B.
* The two recordings are compared **through `RecordingApi`** and asserted
  byte-identical; if they ever diverge, the `DeterminismReport` localizes the
  first mismatch (frame / artifact / byte index / hashes) instead of an opaque
  failure — no ad-hoc debug printing.
* A perturbed replay (different initial state) is caught and localized to the
  first frame, proving the comparison detects divergence as well as confirming
  identity.

## Empty artifacts

Each artifact is just opaque bytes; an empty `Vec<u8>` means "this artifact was
unavailable this frame". The module records and compares empty artifacts exactly
like non-empty ones, so a host that cannot yet produce (say) canonical render
bytes still gets a deterministic, comparable timeline for the artifacts it does
produce. Which artifacts an app actually populates is the app's decision and is
documented there, not here.
