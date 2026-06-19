# axiom-profile-runner

Native **CPU wall-clock profiling** for Axiom's deterministic stress scenario.
Run one command and get a report showing where CPU time is spent across named
engine phases — and, for the two hottest phases, across named **subphases**.

```sh
cargo run --release --bin axiom-profile-runner -- --objects 25000 --frames 600 --warmup-frames 60 --focus-phase full --csv --out target/axiom-profile/latest
```

Always prefer `--release`; debug numbers are dominated by unoptimized
`BTreeMap`/bounds-check overhead and are not representative.

## Commands

Full per-frame profile (JSON + Markdown + CSV):

```sh
cargo run --release --bin axiom-profile-runner -- --objects 25000 --frames 600 --warmup-frames 60 --focus-phase full --csv --out target/axiom-profile/latest
```

Focused `transform_update` profile (only that phase's workload is measured):

```sh
cargo run --release --bin axiom-profile-runner -- --objects 25000 --frames 2000 --warmup-frames 100 --focus-phase transform_update --out target/axiom-profile/transform-update
```

Focused `render_command_build` profile (only that phase's workload is measured):

```sh
cargo run --release --bin axiom-profile-runner -- --objects 25000 --frames 2000 --warmup-frames 100 --focus-phase render_command_build --out target/axiom-profile/render-command-build
```

### Flags

| flag | default | meaning |
|---|---|---|
| `--objects N` | 25000 | number of stress objects |
| `--frames N` | 600 | measured frames / iterations |
| `--warmup-frames N` | 0 | unmeasured warmup iterations run before measuring (excluded from every measured total) |
| `--focus-phase P` | `full` | `full`, `transform_update`, or `render_command_build` |
| `--csv` | off | also write `profile-report.csv` |
| `--out DIR` | `target/axiom-profile/latest` | output directory |

## Output

- `profile-report.json` — machine-readable.
- `profile-report.md` — human-readable, with subphases indented under each phase.
- `profile-report.csv` — flat phase/subphase/churn rows (only with `--csv`).

A summary is printed to the terminal after the run.

The JSON, Markdown, and terminal summary all include: `focus_phase`,
`object_count`, `measured_frame_count`, `warmup_frame_count`, `build_profile`,
the average measured iteration time, the parent phase breakdown, the subphase
breakdown for `transform_update` and `render_command_build`, the placeholder
phases list, and the harness churn counters.

Each `phases[]` entry carries: `name`, `kind`, `total_ns`, `average_ns`,
`sample_count`, `percent_of_measured_phase_time`, and a `subphases[]` array
(each subphase has the same fields, where `percent_of_measured_phase_time` is the
subphase's share of its **parent** phase).

`percent_of_measured_phase_time` for a top-level phase is relative to the sum of
all phase totals (the "measured phase time"), so the phases sum to ~100%. That
sum is intentionally a little less than `total_wall_time_ns`, which also includes
per-iteration loop overhead not attributed to any single phase.

## Phases and subphases

| phase | kind | what it times |
|---|---|---|
| `setup` | harness | building the runtime, scene graph, camera, frustum, mesh (once; full mode only) |
| `runtime_step` | real_engine | `axiom-runtime` fixed stepping with one registered system |
| `transform_update` | real_engine_model | world-transform propagation (see note below) |
| `bounds_update_placeholder` | placeholder | an `axiom-math` `Aabb` per object from its world transform |
| `visibility_or_culling_placeholder` | placeholder | `axiom-math` frustum culling of each placeholder box |
| `render_command_build` | real_engine | `axiom-render` `RenderInput` assembly + `RenderCommandList` compilation |
| `report_write` | harness | serializing this report to JSON + Markdown (+ CSV) |

`transform_update` subphases:
`transform_prepare_inputs`, `transform_parent_lookup`,
`transform_combine_or_matrix_math`, `transform_write_world_state`,
`transform_snapshot_or_output_collection`.

`render_command_build` subphases:
`render_input_create_or_reset`, `render_mesh_data_clone_or_reference`,
`render_object_push`, `render_command_finalize`.

### Why `transform_update` is `real_engine_model`, not `real_engine`

The real engine implementation, `axiom_scene::SceneApi::update_world_transforms`,
is a **single opaque call**: its internal id collection, parent lookup, transform
combine, and world-write steps cannot be timed from outside without modifying
engine code, which this profiling pass forbids. So `transform_update` measures a
**faithful reconstruction** of the same algorithm over the same scene, built
entirely from public APIs:

- `parent_of` / `local_transform` — the engine's own `BTreeMap`-backed reads,
- the exact same `axiom_math::Transform::combine`,
- a fresh per-frame scratch `BTreeMap` mirroring the engine's.

This is valid because the stress scene is depth-2 (one rotating root, N leaf
children), so the per-node work can be staged into separately-timed passes
without breaking the parent→child dependency. The `kind` is `real_engine_model`
and the report's notes say so. Its **absolute** number differs from a direct
engine-call measurement (extra facade hops); the value is the **subphase split**.

`render_command_build`, by contrast, is genuine `axiom-render` work — every
subphase is a real public call — so its subphases sum exactly to the parent.
`render_mesh_data_clone_or_reference` measures the per-frame cube vertex-data
clones; this pass **measures** them and deliberately does not remove them.

### Placeholder phases

`bounds_update_placeholder` and `visibility_or_culling_placeholder` carry the
`_placeholder` suffix because Axiom has no engine layer or module that owns scene
bounds or culling yet. Their work is real `axiom-math` (genuine `Aabb`/`Frustum`
computation) — nothing is faked — but the *capability* is not yet an engine
system. They run only in `full` mode, and are **always disclosed** in the
`placeholder_phases` list, in every mode.

## Focus modes and external (function-level) profilers

This tool times *named phases and subphases*, not individual functions. To get a
**function-level** view, run an external sampling profiler against a **focused**
command — focus mode makes the profiler's output far easier to read, because the
process spends ~all of its time in the one phase you care about instead of
spreading samples across the whole frame.

Examples (Linux; pick whichever sampler you have — this tool does **not**
automate flamegraph):

```sh
# cargo flamegraph (https://github.com/flamegraph-rs/flamegraph)
cargo flamegraph --release --bin axiom-profile-runner -- --objects 25000 --frames 2000 --warmup-frames 100 --focus-phase transform_update --out target/axiom-profile/transform-update

# perf
perf record -g -- target/release/axiom-profile-runner --objects 25000 --frames 2000 --warmup-frames 100 --focus-phase render_command_build --out target/axiom-profile/render-command-build
perf report

# samply (https://github.com/mstange/samply)
samply record target/release/axiom-profile-runner --objects 25000 --frames 2000 --warmup-frames 100 --focus-phase transform_update --out target/axiom-profile/transform-update
```

The `--warmup-frames` value gives caches, branch predictors, and the allocator
time to reach steady state before measurement starts, so both this tool's numbers
and the external sampler's samples reflect the hot path, not cold start.

## What this profiler does NOT measure (and why)

A deliberately small, native, single-threaded slice. Out of scope:

- **Function-level / sampling profiling** — use an external sampler against a
  focused command (above). Keeping it external keeps this runner small and
  dependency-free; flamegraph generation is **not** automated here.
- **GPU timing** — no GPU work at all. No real rendering, no WebGPU/WebGL, no GPU
  timestamp queries. The render phase stops at building a CPU `RenderCommandList`.
- **WASM / browser timing** — native desktop only. No browser APIs, no DOM, no
  `requestAnimationFrame`.
- **Allocation / memory profiling** — no allocation tracking, no allocator
  replacement. (The churn counters *count* allocation-causing events such as mesh
  clones, but do not measure bytes or heap behavior.)
- **Tracy / external tracing integration / dashboards** — none.
- **async / threads** — none.

GPU timing, WASM/browser timing, allocation tracking, and Tracy integration are
intended as **later phases**, built separately, not bolted onto this slice.

## Architecture placement

This is **tooling**, not engine code:

- It lives under `tools/`, so the architecture checker classifies it as a `Tool`.
  It is outside the engine dependency graph and the 100% coverage gate.
- No engine crate depends on it; it depends only on existing public engine
  facades (`axiom-kernel`, `axiom-runtime`, `axiom-math`, `axiom-scene`,
  `axiom-render`).
- All wall-clock timing (`std::time::Instant`) is isolated to this tool
  (`src/scenario.rs` and `src/main.rs`). No timing was added to any deterministic
  engine layer, and no engine system was optimized or rewritten in this pass.
- It adds **no** third-party dependencies. JSON and CSV are serialized by hand.

## Source layout

- `src/main.rs` — CLI parsing, orchestration, file writing, terminal summary.
- `src/scenario.rs` — the deterministic engine-driving stress scenario, focus
  modes, warmup, and per-phase/per-subphase wall-clock timing. The only file
  that touches the engine.
- `src/report.rs` — the pure, engine-free data model: phase/subphase
  accumulation, percentage and roll-up math, and the JSON + Markdown + CSV
  serializers (unit-tested in isolation).
