# 05 — Lifecycle Call-Site Sweep

## Mission

Repair the 14 lifecycle sequences that `02`'s `start()` change breaks, across 9
files in four different package classes. Every repair is the same mechanical
insertion — `initialize(); prepare(PreparationSchedule::new())?; start();` — and
this manifest exists so that a cross-cutting, low-judgement edit has exactly one
owner instead of leaking into four other manifests.

## Architectural owner

- **Packages:** `crates/axiom-runtime`, `crates/axiom-frame`, `crates/axiom-host`,
  `crates/axiom-introspect`, `apps/axiom-demo-rotating-cube`,
  `tools/axiom-profile-runner`
- **Classification:** cross-cutting maintenance across layers, an app and a tool
- **Why here:** the sites are scattered and individually trivial. Bundling them
  keeps `02` focused on semantics and stops three other manifests each
  half-owning a sweep.

## Depends on

**`02-runtime-preparation-barrier.md`** — and you must **branch from `02`'s
branch, not from `main`**. You are part of the atomic landing group; your work
does not compile against `main`.

## Parallel safety

Concurrent with `02` (head of your group), `06` (the other group member) and
`03`. You own a disjoint file set from all three.

## Files owned

| Path | Sites | Class |
|---|---|---|
| `crates/axiom-runtime/src/runtime_scheduler.rs` | `:404` | test |
| `crates/axiom-runtime/tests/integration.rs` | `:45` | test |
| `crates/axiom-frame/src/frame_step_summary.rs` | `:110, :184, :243` | test |
| `crates/axiom-host/src/host_api.rs` | `:571` | test |
| `crates/axiom-host/src/host_step_driver.rs` | `:147, :257, :272` | test |
| `crates/axiom-introspect/src/fixtures.rs` | `:32, :70` | test |
| `apps/axiom-demo-rotating-cube/src/demo_api.rs` | `:94` | **production** |
| `apps/axiom-demo-rotating-cube/examples/introspection_evidence.rs` | `:236` | **production** |
| `tools/axiom-profile-runner/src/scenario.rs` | `:155` | **production** |

Line numbers are pre-change; re-locate with the search below rather than
trusting them after `02` lands.

## Files allowed to modify

Only the nine above.

## Files forbidden to modify

- **`crates/axiom-runtime/src/runtime.rs`** — owned by `02`. It holds 8 further
  call sites which `02` fixes itself. Do not touch it.
- **`modules/axiom/src/app.rs`** — owned by `06`. It holds the 23rd site.
- `crates/axiom-runtime/src/lib.rs`, `preparation_*.rs`, `runtime_state.rs` — `01`
- `crates/axiom-runtime/tests/{preparation,preparation_lifecycle,architecture}.rs` — `04`
- Any `layer.toml`, `module.toml`, `app.toml`, or any `Cargo.toml`

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `crates/axiom-introspect/src/fixtures.rs:14-17` | **The hazard.** This crate names `axiom_runtime` while its `layer.toml` declares `["kernel","frame","ecs"]` — it survives via a dev-dependency and a `#[cfg(test)] mod fixtures;` at `crates/axiom-introspect/src/lib.rs:50`. Adding a `PreparationSchedule` reference here may trip `DisallowedLayerImport`. **Run the checker; do not assume.** |
| `crates/axiom-runtime/tests/integration.rs:44-46` | The canonical sequence you are transforming |

## Contract consumed

From `01`/`02`, verbatim:

```rust
runtime.initialize()?;
runtime.prepare(PreparationSchedule::new())?;   // NEW — the empty-schedule migration
runtime.start()?;
```

An empty schedule is legal and reaches `Prepared` immediately. That is the
sanctioned migration for every site here: **none of these call sites is supposed
to gain real preparation work.**

## Contract produced

None. The workspace compiles and its tests pass.

## Implementation instructions

1. Re-locate every site — do not trust the pre-change line numbers:

```sh
rg -n '\.initialize\(\)' --glob '!target' --glob '!.claude'
```

2. For each, insert `prepare(PreparationSchedule::new())` between `initialize()`
   and `start()`, matching the surrounding error idiom exactly: `.unwrap()` in
   tests that already `.unwrap()`, `.expect("…")` where the file uses `expect`.
   Add the `PreparationSchedule` import to each file's existing `axiom_runtime`
   `use` list.

3. **Do not** add real preparation tasks anywhere. **Do not** refactor these
   sequences. **Do not** rename tests. The smallest correct edit is the goal.

4. For the three **production** sites (`demo_api.rs:94`,
   `introspection_evidence.rs:236`, `scenario.rs:155`), read the surrounding
   function first. Each constructs a `Runtime` and immediately drives it; the
   empty-schedule insertion is still correct, because none of them authors a
   scene through `RunningApp::realize` (that path is `06`'s).

5. After `crates/axiom-introspect/src/fixtures.rs`, run
   `cargo run -p xtask -- check-architecture` **specifically** and report the
   result. If it reports `DisallowedLayerImport` for `introspect`, **stop and
   report** — the fix would be a `layer.toml` change, which you do not own.

## Required behavior

- The workspace compiles (once merged with `02` and `06`).
- Every previously-passing test still passes, with unchanged assertions.
- No test's *meaning* changes. If a test's premise genuinely no longer holds,
  that is `02`'s business (it owns the two rejection probes) — report rather
  than rewrite.

## Error behavior

Match each site's existing idiom. `prepare` on an empty schedule cannot fail from
`Initialized`, so `.unwrap()`/`.expect()` is honest here.

## Determinism requirements

None beyond preserving existing behaviour. An empty schedule performs no work and
cannot change any observable outcome.

## Tests

You add none. You keep existing ones passing. Do not weaken an assertion to make
a test pass — if one fails, report it.

## Architecture validation

- `cargo run -p xtask -- check-architecture` must pass, in particular for
  `introspect` (see the hazard above).
- `crates/axiom-frame`, `crates/axiom-host` and `crates/axiom-introspect` already
  declare `runtime` in `depends_on`, so referencing `PreparationSchedule` there is
  legal — **verify, don't assume**.
- `apps/axiom-demo-rotating-cube/app.toml` and `tools/axiom-profile-runner` must
  already permit `runtime`; check before editing.

## Performance considerations

None.

## Documentation changes

None.

## Completion criteria

- [ ] All 14 non-`runtime.rs`, non-`app.rs` sites migrated
- [ ] `rg -n '\.initialize\(\)'` shows every site followed by a `prepare` call
- [ ] `cargo build --workspace` succeeds **when combined with `02` and `06`**
- [ ] `cargo test --workspace` green on the merged group
- [ ] `cargo run -p xtask -- check-architecture` passes
- [ ] Zero assertions weakened, zero tests renamed, zero real tasks added

## Validation commands

```sh
cargo build --workspace
cargo test --workspace
cargo run -p xtask -- check-architecture
rg -n '\.initialize\(\)' --glob '!target' --glob '!.claude'
```

**On your own branch `cargo test --workspace` will be RED**, because the other
member of the landing group has not merged yet. That is correct. Do **not** reach
into files you do not own to make it green — that is exactly the write conflict
the group exists to avoid. `cargo test --workspace` is an orchestrator-only,
post-merge gate.

Note the breakage is **behavioural, not a compile error**: `Runtime::start` keeps
its signature and only its accepted states change, so failures appear as
`.unwrap()` panics in tests, never as build errors. Never enumerate the remaining
work with `cargo build`.


## Deliverable to orchestrator

Report: commit hash; the nine file paths with the sites changed in each; the
checker result **specifically for `introspect`**; confirmation that `runtime.rs`
and `modules/axiom/src/app.rs` are untouched; any site whose test premise looked
wrong (report, do not fix); deviations.
