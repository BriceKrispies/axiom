# 13 — Integration, Proof and Landing

## Mission

Pull the landed parallel work together, prove the invariant actually holds
end-to-end, run the **complete** gate suite (which no other manifest is permitted
to run), and leave the repository in a state where the documentation matches the
implementation. This manifest **implements no feature work**. If you find
yourself writing preparation logic, an earlier manifest is incomplete — stop and
report it.

## Architectural owner

- **Packages:** `apps/burnt-rubber` (tests + docs), `docs/`, `.github/`
- **Classification:** Integration and validation
- **Why here:** the shared files reserved from every parallel agent — the app's
  new test file, `TESTING.md`, the architecture reference, CI — are collected
  here deliberately so no parallel worker ever contends for them.

## Depends on

**Every manifest `01`–`11`.** `12` is optional; proceed without it if skipped.

## Parallel safety

None. Runs alone, last.

## Files owned

| Path | Action |
|---|---|
| `apps/burnt-rubber/tests/preparation.rs` | **create** |
| `apps/burnt-rubber/TESTING.md` | modify |
| `docs/architecture/startup-preparation-plan.md` | modify |
| `docs/work-manifests/startup-preparation/README.md` | modify (record outcomes) |
| `.github/workflows/ci.yml` | modify (one line) |

## Files allowed to modify

The five above, plus this **closed list** of integration-seam repairs, each
reported individually:

- removal of a now-dead inline generator path in
  `apps/burnt-rubber/src/render/{palette,chunks,scenery_pool,prop_meshes}.rs` —
  the originals `09` and `10` kept alive under the additive rule, once `11` has
  switched every call site to the `_prepared` variant;
- removal of a duplicated or unused `use` left by two streams touching one file.

**Nothing else.** An earlier draft allowed "any file with a genuine integration
seam". On the one manifest that runs last and alone, that clause would silently
absorb exactly the unowned-file breakages this plan exists to prevent, and the
report would say "integration seam" rather than naming the manifest that broke
it. If a file outside this list needs editing, **escalate — do not edit**.

## Files forbidden to modify

- `apps/burnt-rubber/tests/golden/**` (15 files), `apps/burnt-rubber/slice.toml`,
  `apps/burnt-rubber/tests/agent_golden.rs` — **the committed baseline. Never
  re-bless. Never widen a tolerance. Never edit an assertion.**
- `crates/xtask/**`, `tools/lints/**`, `tools/lints/dylint-baseline.txt` — no
  checker or lint change is part of this programme
- `CLAUDE.md` — out of scope even though it is stale (see Findings below)
- Workspace `Cargo.toml` — untouched throughout

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `apps/burnt-rubber/tests/agent_golden.rs` | The baseline's shape. Read it; do not edit it |
| `apps/burnt-rubber/TESTING.md` §0 | The golden run's documentation, which you extend |
| `docs/architecture/startup-preparation-plan.md` | The architecture reference; its §20 task graph is now superseded by this directory |
| `.github/workflows/ci.yml` | Add `check-slices`; note CI is `workflow_dispatch`-only |

## Contract consumed

Everything. You add no new contract.

## Contract produced

A green repository and a documented, proven invariant.

## Implementation instructions

### 1. Write the end-to-end proof — `apps/burnt-rubber/tests/preparation.rs`

These are the tests the goldens **cannot** express, because a golden proves
nothing broke and can never prove the migration *worked*:

- `the_race_is_playable_only_after_preparation` — the course, road meshes and
  textures all exist at the instant the app first becomes steppable
- `the_course_is_compiled_exactly_once_per_launch` — reads the **counter `11`
  provides**; shows one, not the four of the current state. **Would have failed
  before the programme; it is the measurable win**
- `preparation_failure_is_surfaced_not_swallowed` — uses the **failure-injection
  seam `11` provides**; the runtime lands in `Failed` and no frame is presented
- `two_preparations_from_the_same_seed_produce_identical_products` — run
  `prepare()` twice from scratch and compare the `CoursePlan` and the three
  texture buffers. **This is the only mechanical determinism check the app tier
  gets**, because `apps/` is outside dylint, coverage and the branchless gate
- `prepared_work_does_not_rerun_during_gameplay` — a counting task, 100 frames,
  count still 1

**Do not re-derive `11`'s tests.** `a_restart_does_not_recompile_the_course` and
`the_ghost_shares_the_prepared_course` are `11`'s and already landed; confirm
they still pass, do not copy them. And both seams the first two tests need are
**`11`'s deliverables** — if they are missing, that is an incomplete manifest to
report, not instrumentation for you to add to `src/`, which you do not own.

### 2. Verify the barrier structurally

Confirm by inspection and record in your report:
- `PreparationTask::prepare(&mut self)` and
  `RuntimeSystem::run(&mut self, ctx)` remain **incompatible**, so a preparation
  task cannot be registered in `RuntimeScheduler` — a compile error, not a
  convention.
- `crates/axiom-runtime` exposes **exactly two** new public symbols, locked by
  `tests/architecture.rs` from `04`.
- `Runtime::start` accepts exactly `{Prepared, Paused}`.

### 3. Run the full gate suite — **one at a time**

You are the **only** manifest permitted to run coverage, dylint and ts-gate.
Never run two concurrently: dylint fakes a `cargo metadata` error that masks real
findings, and `link.exe 0xc0000142` is the out-of-memory signature.

### 4. Prove the pixels

Capture all five golden checkpoints on both backends and compare against the
hashes recorded in `docs/architecture/startup-preparation-plan.md` §4.8.
Canvas 2D must be **byte-identical**. GPU must be byte-identical **on the
development machine** — that is machine-local evidence, not a portable pin, and
must be reported as such (no tool in the repo can apply
`Tolerance::GPU_DEFAULT` to these captures, and building one is a declared
non-goal).

### 5. Browser verification

Serve the app and confirm it still renders — a green build and a painting page
are different facts.

### 6. Documentation

- `TESTING.md` — a section on the preparation phase and the new tests.
- `docs/architecture/startup-preparation-plan.md` — mark §19/§20 as **superseded
  by `docs/work-manifests/startup-preparation/`**, and record the measured
  outcomes: total preparation duration (native and browser), before/after median
  gameplay frame time at the `canyon` checkpoint, the course-compile count
  (4 → 1) and the `Track` copy count (4 → 2). Label everything as measured.
- `README.md` (this directory) — record which manifests landed, in what order,
  and any deviation.
- `.github/workflows/ci.yml` — add
  `cargo run -p xtask -- check-slices`. It is not a CI step today, so the 15
  SHA-256 pins are unenforced by automation.

### 7. Inspect the final diff for architectural leakage

Read `git diff` across the whole programme and confirm:
- no rendering, WebGPU, browser or Burnt Rubber concept appears anywhere in
  `crates/axiom-runtime`
- no `PreparationReport`, `PreparationContext` or `Preparing` state was
  reintroduced
- no `?` was introduced into `modules/axiom` non-test code
- no dead compatibility path survives (e.g. a `with_profile` variant kept "just
  in case")
- the public surface is still exactly two new symbols

## Required behavior

Everything above passes, and the game is behaviourally and visually identical to
the committed baseline.

## Error behavior

**A golden diff is a bug, not a new baseline.** If any of the 15 artifacts moves:
identify which (state → simulation changed; render → scene or look changed;
resources → generated geometry or textures changed), bisect by manifest, and
**report**. Do not run `AXIOM_REGOLD`. Do not edit `slice.toml`.

## Determinism requirements

- All 15 golden artifacts byte-identical.
- Canvas 2D pixels byte-identical.
- GPU pixels byte-identical on the development machine, reported as machine-local.

## Tests

`apps/burnt-rubber/tests/preparation.rs` (7 tests above) plus the full existing
suite unchanged.

## Architecture validation

Full suite, below.

## Performance considerations

Record, do not chase. Use `--profile-compare` (interleaved, one process) for any
A/B; cross-process comparison on this machine is worthless (3.29 ms vs 13.52 ms
for the same slice).

## Documentation changes

The five owned files.

## Completion criteria

- [ ] `apps/burnt-rubber/tests/preparation.rs` exists with all 7 tests passing
- [ ] `the_course_is_compiled_exactly_once_per_launch` passes (was 4)
- [ ] All 15 golden artifacts byte-unchanged; `AXIOM_REGOLD` never set
- [ ] Canvas 2D pixels byte-identical; GPU byte-identical on this machine
- [ ] `cargo test --workspace` green
- [ ] `cargo run -p xtask -- check-architecture` green
- [ ] `cargo run -p xtask -- check-slices` green **and wired into CI**
- [ ] `bash scripts/coverage.sh` reports **100.00%** regions/lines/functions
- [ ] `bash scripts/dylint-gate.sh` at or under baseline, `engine_no_branching`
      still **0**
- [ ] `bash scripts/ts-gate.sh` green
- [ ] Browser renders; console error-free; screenshot read
- [ ] `crates/axiom-runtime` exposes exactly two new public symbols
- [ ] Final diff inspected for leakage
- [ ] Documentation matches implementation

## Validation commands

Run **one at a time**.

```sh
# tests
cargo test --workspace
cargo test -p axiom-burnt-rubber --test preparation
cargo test -p axiom-burnt-rubber --test agent_golden -- --nocapture

# structure
cargo run -p xtask -- check-architecture
cargo run -p xtask -- check-slices
cargo run -p xtask -- check-slice-placement   # 2 PRE-EXISTING findings in apps/end-zone; not yours

# gates — never concurrent
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
bash scripts/ts-gate.sh

# pixels
cargo build --release -p axiom-shot --features offscreen
for cp in grid opening esses canyon finish; do
  ./target/release/axiom-shot --app burnt-rubber-golden-$cp --backend gpu      --out screenshots/after/$cp.gpu.png
  ./target/release/axiom-shot --app burnt-rubber-golden-$cp --backend canvas2d --out screenshots/after/$cp.canvas2d.png
done
sha256sum screenshots/after/*.png

# frame cost — interleaved, never across processes
./target/release/axiom-shot --profile-compare \
  burnt-rubber-golden-opening,burnt-rubber-golden-canyon \
  --profile-frames 60 --profile-trials 5

# browser
cargo build --target wasm32-unknown-unknown -p axiom-kernel
uv run scripts/localhost_servers.py start-app burnt-rubber --port 8085
uv run scripts/localhost_servers.py logs burnt-rubber -n 20
uv run scripts/playwright_controller.py goto http://localhost:8085/
uv run scripts/playwright_controller.py wait 2500
uv run scripts/playwright_controller.py console
uv run scripts/playwright_controller.py screenshot burnt-rubber-after
uv run scripts/localhost_servers.py stop burnt-rubber
```

## Findings to report, not fix

- **`CLAUDE.md` is stale**: it names `windowing` as the sole platform-facing
  module; `crates/xtask/src/hygiene.rs:65-70` allows five (`windowing`,
  `gpu-backend`, `canvas2d-backend`, `debug-overlay`, `audio`).
- **`crates/axiom-host/ARCHITECTURE.md:304-321` is stale**: it says there is "no
  async device-init path in any layer" and that `HostPresentationStatus::Ready`
  awaits a live pass. The live backend exists (`modules/axiom-gpu-backend`) and
  bypassed the host seam entirely; `Ready` is still unreachable and
  `evaluate_presentation` has no production caller.
- **CI is `workflow_dispatch`-only** since 2026-07-14 — nothing runs
  automatically, so every gate above is a local obligation.
- **`check-slice-placement` reports 2 pre-existing violations** in
  `apps/end-zone`. Not caused by this programme.

## Deliverable to orchestrator

Report: commit hash; every file changed across the whole programme; the full
output of each gate; **per-artifact confirmation that all 15 goldens are
byte-unchanged**; both pixel-arm hashes; the measured numbers for the plan doc;
the leakage-inspection result; every stale-documentation finding; and any
manifest that landed incomplete.
