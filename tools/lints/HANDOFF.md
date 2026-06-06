# Lint rulebook — handoff / resume doc

Point a fresh session at this file to continue building Axiom's dylint rulebook.
It captures what exists, how it works, the gotchas already paid, and the exact
next steps.

---

## TL;DR — what to do next

The **zone foundation is built and proven end-to-end**. The remaining work is
two parallel tracks:

1. **Fan out the lint backlog** (`tools/lints/lints_to_add.md`) — ~14 clean
   Tier-1 lints, then ~35 zone-dependent Tier-2 lints, via sub-agents (one per
   lint). Protocol + templates below.
2. **Label engine zones** — apply `#[axiom_zones::sim]` / `#[hot_path]` /
   `#[strict]` to the rest of the engine so the Tier-2 lints actually fire.
   Incremental: each zone you mark activates its lints.

**Recommended first action:** lift the shared helpers (see "Shared helper" below)
into a crate, then fan out Tier 1. Start with `engine_no_transmute`,
`engine_no_static_mut`, `engine_no_wildcard_imports`,
`engine_no_runtime_type_branch`.

---

## What exists now (all committed, all gates green)

Run from repo root: `cargo dylint --all -- --all-targets` (currently reports 0).

### Working lints — `tools/lints/`
- **`test_without_assertion`** — flags `#[test]` fns with no assertion. Strict
  (bare `unwrap`/`expect`/`?` don't count), resolves helper calls semantically.
- **`no_unwrap_in_engine`** — bans `.unwrap()`/`unwrap_err`/`unwrap_unchecked` in
  non-test engine `src`. **This is the Tier-1 template.**
- **`engine_no_time_in_sim`** — the pilot zone lint: bans `Instant::now`/
  `SystemTime::now` only inside a `#[sim]` zone. **This is the Tier-2 template**
  (zone detection via `in_zone`/`item_has_marker`).

### Zone foundation — `crates/axiom-zones/` (a proc-macro `Support` crate)
Attributes `#[sim]`, `#[hot_path]`, `#[strict]`, `#[supervisor]`,
`#[escape_hatch(reason="...")]`. Custom attrs don't exist on stable Rust, so each
re-emits the item with a **greppable zero-sized marker** prepended:
`const __engine_zone_sim: () = ();` (and `__engine_escape_hatch_reason: &str` for
escape hatches). The raw attribute is consumed at expansion (like `#[test]`); the
marker survives into HIR for lints to detect by name. Works on free fns, inline
`mod`s, AND impl methods. `tests/markers.rs` proves injection.

### Law amendments (mechanically enforced)
- **Module Law:** new `PackageClass::Support` (`crates/xtask/src/classification.rs`,
  `class_check.rs`). `axiom-zones` classifies as Support: no `layer.toml`, every
  layer/module/app may depend on it (exempt from layer ordering), depends on
  nothing engine. Documented as CLAUDE.md Module Law #13 + a "Zone markers" section.
- **Coverage Law:** `axiom-zones` excluded from the 100% gate
  (`crates/xtask/src/coverage_scope.rs` `SANCTIONED_IGNORE_REGEX` =
  `[/\\](xtask|apps|axiom-zones)[/\\]`, mirrored in both `scripts/coverage.*`).

### First labeling pass (proves it on real code)
- `#[sim]`: `Runtime::step`, `SceneApi::advance`, `FrameBuilder::build`
- `#[strict]`: `Mat4::multiply`
- Labeled crates (runtime/frame/math/scene) gained the `axiom-zones` dep AND
  `axiom_zones` in their per-crate `tests/architecture.rs` allowed-import lists.

---

## Backlog triage (`lints_to_add.md` has the full 68)

- **Tier 1 — clean now (~14):** `engine_no_transmute`, `engine_no_uninit_memory`,
  `engine_no_static_mut`, `engine_no_thread_spawn`, `engine_no_wildcard_imports`,
  `engine_no_runtime_type_branch`, `engine_no_recursion`,
  `engine_no_unitless_float_public_api`, `engine_no_large_files`,
  `engine_no_large_functions`, `engine_no_large_structs`, `engine_no_large_enums`,
  `engine_no_large_impl_blocks`, `engine_require_module_docs`. Mirror
  `no_unwrap_in_engine`: ban symbols/items in engine `src`, exempt tests + macros.
- **Tier 2 — need a labeled zone (~35):** everything "in sim / hot-path / strict /
  runtime zone", budgets, escape hatches, bounded collections. Mirror
  `engine_no_time_in_sim`: detect the zone marker, then ban. Inert until the zone
  is labeled.
- **Tier 3 — DROP (~12):** `engine_no_todo_unimplemented`,
  `engine_no_forbidden_imports`, `engine_no_layer_skipping` (already enforced by
  `xtask`); `engine_no_unwrap_expect`/`engine_no_panic_paths` (overlap
  `no_unwrap_in_engine`); `engine_no_renderer_gameplay_coupling`,
  `engine_no_gameplay_in_platform` (no gameplay layer exists yet).

---

## Fan-out protocol (avoids the collisions that break naive parallelism)

Every new lint touches two SHARED files: `tools/lints/Cargo.toml` (members) and
the root `Cargo.toml` `[workspace.metadata.dylint]`. If N agents edit those in
parallel they collide. So:

> **Orchestrator owns the manifests.** For a wave: (1) create skeleton crates
> (copy an existing lint's `Cargo.toml` + `rust-toolchain` + `.cargo/config.toml`
> + `.gitignore`, rename the package, stub `src/lib.rs`), (2) add all of them to
> both manifests in one edit each, (3) THEN spawn one agent per lint that fills
> ONLY its own `src/lib.rs` + `ui/` + README and never touches a shared file.
> Agents write in parallel; builds serialize on the shared `tools/lints/target`
> (clippy_utils compiles once, not N times).

### Each agent brief must include
- The template source: `no_unwrap_in_engine/src/lib.rs` (Tier 1) or
  `engine_no_time_in_sim/src/lib.rs` (Tier 2).
- The conventions: `declare_late_lint!`; `is_engine_file` scoping (path has a
  `crates`/`modules` component AND a `src` component, NOT `xtask`/`axiom-zones`);
  `clippy_utils::is_in_test` exemption; `expr.span.from_expansion()` skip;
  `ui/` fixtures in path-meaningful dirs (`ui/modules/m/src/...`,
  `ui/apps/a/src/...`); the bless step (below).
- Its one spec from `lints_to_add.md`.

### Shared helper (do this FIRST for Tier 2)
`is_engine_file`, `in_zone`, `item_has_marker`, `def_named` are currently copy-
pasted in `engine_no_time_in_sim`. Before the Tier-2 fan-out, lift them into a
`tools/lints/engine_lint_helpers` lib crate (normal lib, `#![feature(rustc_private)]`,
same nightly) that the lint cdylibs depend on. Saves 35× duplication.

---

## Gotchas already solved (don't re-discover these)

- **Install:** `cargo +nightly-x86_64-pc-windows-msvc install cargo-dylint dylint-link --locked`
  (stable rustc was too old for `cargo-platform`).
- **`cargo dylint new <name>`** scaffolds a lint; pins `nightly-2026-04-16` +
  `clippy_utils` rev. New crate must be a workspace member or it errors
  ("believes it's in a workspace"). `tools/lints/` is its own workspace.
- **`#[cfg(test)]` invisibility:** a plain `cargo check` doesn't compile test
  code, so lints see no tests — always run `-- --all-targets`. In `ui_test`, add
  `// compile-flags: --test` when the fixture needs `#[test]`/`#[cfg(test)]`.
- **`#[test]`/`#[should_panic]` are consumed under `--test`** — detect via
  `clippy_utils::is_in_test_function` / `is_in_test` and
  `AttributeKind::ShouldPanic`, NOT the raw attribute name.
- **`dylint_testing` 6.0.1 has no bless.** To snapshot: run `cargo test`, find the
  "Actual stderr saved to <tmp path>" line, copy that file over `ui/<name>.stderr`.
  Compiletest recurses into `ui/` subdirs and normalizes the test dir to `$DIR`
  (it replaces the literal dir-name substring everywhere — avoid fixture/fn names
  containing the dir name, e.g. don't put `ui` inside an identifier).
- **rustc_private API churn (this toolchain):** `ItemKind::Mod(_, m)` (2 fields);
  `cx.tcx.sess` not `cx.sess()`; `RealFileName::local_path()` returns
  `Option<&Path>`; `has_name` is inherent on the HIR `Attribute`.
- **Per-crate `tests/architecture.rs` allowlists:** these hand-rolled tests scan
  source for `axiom_*` imports against a hardcoded list. Labeling a crate (adding
  `#[axiom_zones::sim]`) makes `axiom_zones` appear → add `&& chunk != "axiom_zones"`
  to that crate's allowlist. The Layer Law checker (`xtask`) does NOT need this —
  it only scans known layer prefixes, and `axiom_zones` isn't a layer.
- **Coverage:** injected marker consts are coverage-neutral (verified — region
  count unchanged). Re-run `scripts/coverage.ps1` after any labeling.

---

## Proposed engine zone map (for labeling; refine as needed)

| Zone | Targets | Unlocks |
|---|---|---|
| `#[sim]` | scene SpinSystem/TransformPropagation/`advance`; runtime `step`/scheduler; frame `build`; kernel `SimulationClock::advance` | no time/random/hashmap/float/alloc-in-sim |
| `#[strict]` | `axiom-math` primitive ops (vec/mat/quat/transform/aabb/sphere/plane/frustum/ray) | no branching/`?`/indexing in strict |
| `#[hot_path]` | umbrella `RunningApp::tick`; `scene.advance`; `pipeline.submit` (often also `#[sim]` — markers stack) | no alloc/log/lock/format/dyn-dispatch in hot path |
| `#[supervisor]` | the run loop — currently in apps (outside engine scope), so no engine targets yet | bounded-loop / supervisor-only rules |
| `#[escape_hatch]` | none up front — case-by-case with a reason | reason / scope-limit lints |

Note: `#[strict]` on `Mat4::multiply` already contains a `for` loop, so once
`engine_no_raw_loops` / `engine_no_source_branching` land they WILL flag it — by
design; that's the signal to refactor or escape-hatch.

---

## Session memory pointer

See `~/.claude/projects/C--dev-axiom/memory/dylint-lint-platform.md` for the
condensed version of the above.
