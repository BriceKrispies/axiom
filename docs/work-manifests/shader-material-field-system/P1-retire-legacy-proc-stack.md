# P1 — Retire the legacy `axiom-proc` stack

## Objective

Converge the engine onto **one** recipe interpreter before a third generation is
built on top of it. Migrate `crates/axiom-proc-validate` and
`modules/axiom-placement` off the v1 `axiom-proc` stack onto
`axiom-recipe` + `axiom-proc-core`, then delete `crates/axiom-proc`.

This is not cosmetic. `00-architecture-findings.md` §1.2 documents two complete,
mutually-unaware recipe interpreters both shipping. Adding `axiom-field` while
both remain turns a two-generation cluster into a three-generation one, and the
repository has an explicit rule against exactly that shape of debt.

## Architectural placement

**Layer** (`proc-validate`) + **Engine module** (`placement`) + deletion of a
**Layer** (`proc`). No new package. No law change.

## Existing code involved

| Path | Role |
|---|---|
| `crates/axiom-proc/` | v1: `Recipe`, `RecipeNode { op: NodeOp, immediate: u64, inputs: [usize; 2] }`, closed ops `Const/Draw/Add/Xor`, `Artifact`, `ProcTrace`, resumable `Evaluation::step(n)` |
| `crates/axiom-proc-validate/` | `ProcValidateApi`, `Constraint::{min_count, max_value, non_zero}` dispatched via `const EVALS: [ConstraintEval; 3]`, `ValidationReport`, `sample_until_valid` |
| `modules/axiom-placement/module.toml` | declares `allowed_layers = ["kernel", "space", "proc"]` |
| `apps/axiom-proc-playground`, `apps/axiom-quintet` | app-tier consumers (outside the gates) |
| `tools/axiom-proc-fuzz`, `tools/axiom-proc-inspect` | tooling consumers |
| `crates/axiom-recipe/`, `crates/axiom-proc-core/` | the v2 target |

## Files likely to change

* `crates/axiom-proc-validate/src/**` — retarget from `Artifact` (a `Vec<u64>` of
  opaque words) to the output of a `ProcCore::execute` run, or to `RecipeGraph`
  itself where the constraint is structural.
* `crates/axiom-proc-validate/layer.toml` — `depends_on` `["kernel", "proc"]` →
  `["kernel", "recipe", "proc-core"]`, with `meaningful_dependency`,
  `consumed_capabilities` and `[[proof_exports]]` rewritten to match.
* `modules/axiom-placement/{module.toml, Cargo.toml, src/**}`.
* `apps/axiom-proc-playground/src/main.rs`, `apps/axiom-quintet/**`.
* `tools/axiom-proc-fuzz/src/main.rs`, `tools/axiom-proc-inspect/src/main.rs`.
* **Delete** `crates/axiom-proc/`; remove from root `Cargo.toml` `members`.

## Dependencies on earlier manifests

**None. May begin immediately.** Fully parallel with `P2`.

## Public API / data contracts

**Decision required from the orchestrator before starting, and recorded in
`docs/engine-datafication.md` §10 "open decisions" (#2A), which already flags it:**
whether v1's `ProcTrace`, resumable `Evaluation::step(n)`, and `Artifact::digest`
are preserved on the v2 stack or dropped.

Recommendation, on the evidence: **preserve `digest`, drop `ProcTrace` and
resumability.** `RecipeGraph::digest()` already covers recipe identity;
`ProcTrace`'s only consumer is `tools/axiom-proc-inspect`, which can be
re-expressed as a per-node output dump from `ProcCore`; nothing in the repo calls
`Evaluation::step` with a real budget. Dropping them is ≈ −1,087 LOC per
`docs/engine-datafication.md` §7 #2A.

## Explicitly excluded

* Do **not** add types to `axiom-recipe` or `axiom-proc-core` to ease migration —
  their shape is settled and `01` depends on it.
* Do **not** introduce a compatibility shim crate. A shim is a third generation.
* Do **not** change `crates/axiom-proc-texture` or `crates/axiom-proc-mesh`; they
  are already on v2.

## Determinism requirements

`tools/axiom-proc-fuzz` sweeps 2,000 seeds asserting byte-identical regeneration
across proc/terrain/biome/placement/levelgen and runs under `cargo test
--workspace`. **It must still pass, unmodified in intent, after the migration.**
Where a digest legitimately changes because the underlying representation
changed, the new value is recorded once with a comment naming this manifest.

## Serialization requirements

Any persisted v1 `Artifact` bytes must either be migrated or proven unused.
Search for committed `.bin` artifacts and for `SchemaVersion` stamps referencing
the v1 format before deleting anything.

## Testing requirements

* Every migrated constraint keeps a test asserting the same verdict on the same
  input.
* `crates/axiom-proc-validate` returns to 100% coverage in the same change.
* A test asserting `axiom-proc` is absent from the workspace members list is
  unnecessary — `cargo xtask check-architecture`'s `UnknownPackageClass` and the
  build itself cover it.

## Architecture tests

`cargo xtask check-architecture` must stay green throughout, including
`real_repo_layers_pass` and `real_repo_class_aware_check_passes`. Removing a
layer changes the DAG; `UnknownDependency` will fire immediately on any stale
`depends_on = ["proc"]`.

## Performance risks

Low. `ProcCore::execute` clones one `Out` per graph edge; for
`axiom-placement`'s small word outputs this is negligible. Note it for the record
so a future agent does not rediscover it.

## Migration considerations

`modules/axiom-placement` is the only **module** in the blast radius. Its
`allowed_layers` change is a `module.toml` edit; the checker reads the real Cargo
graph, so the `Cargo.toml` and the manifest must move together or
`ModuleDependsOnLayerNotAllowed` fires.

## Completion criteria

1. `crates/axiom-proc/` no longer exists and is out of root `Cargo.toml`.
2. Nothing in `crates/`, `modules/`, `apps/` or `tools/` references `axiom_proc::`.
3. `cargo xtask check-architecture` exits 0 with 23 layers.
4. `cargo test --workspace` passes, including `axiom-proc-fuzz`'s property tests.
5. `scripts/coverage.sh` reports 100/100/100.
6. Every dylint count is at or below its baseline.

## Validation commands

```sh
cargo xtask check-architecture
cargo test --workspace
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```
