# 03 — Runtime Layer Manifest and Architecture Doc

## Mission

Declare the two new capabilities in `crates/axiom-runtime/layer.toml` and record
the preparation phase in `crates/axiom-runtime/ARCHITECTURE.md`. This is a
documentation-and-manifest task; it touches **no Rust**, which is exactly why it
can run alongside the code streams without any conflict risk.

## Architectural owner

- **Package:** `crates/axiom-runtime`
- **Classification:** Layer (`runtime`) manifest + architecture record
- **Why here:** `layer.toml` is the layer's declaration of what it introduces;
  `ARCHITECTURE.md:8` currently lists the lifecycle state machine as the layer's
  first responsibility and must now name the new phase.

## Depends on

**`01-runtime-preparation-primitive.md`** — you need the final symbol names.
You do **not** need `02` to have landed; you are documenting a contract that is
frozen in `01` and `02`.

## Parallel safety

Concurrent with **everything**. This manifest owns two non-`.rs` files that no
other manifest may touch.

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-runtime/layer.toml` | modify (64 lines) |
| `crates/axiom-runtime/ARCHITECTURE.md` | modify (90 lines) |

## Files allowed to modify

Only the two above.

## Files forbidden to modify

- **Every `.rs` file in the repository.** If a documentation change appears to
  require a code change, stop and report it.
- `docs/architecture/startup-preparation-plan.md` — owned by `13`
- Any other `layer.toml`, `module.toml` or `app.toml`

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `crates/axiom-runtime/layer.toml` (all 64 lines) | 18 `introduced_capabilities`, 15 `consumed_capabilities`, 5 `[[proof_exports]]` |
| `crates/axiom-runtime/ARCHITECTURE.md:8-14` | The sentence enumerating the lifecycle — `Created → Initialized → Running ↔ Paused → Stopped` and `→ Failed` |
| `crates/axiom-runtime/ARCHITECTURE.md:41-53` | The "intentionally does not implement" list — you must not contradict it |
| `crates/xtask/src/check.rs:313-326` | The capability check, so you know exactly what your edit must satisfy |

## Contract consumed

The two new public symbols from `01`: `PreparationTask`, `PreparationSchedule`.
Plus, for prose only, `Runtime::prepare` and `RuntimeErrorCode::PreparationFailed`
from `02`.

## Contract produced

None in code. A declared manifest that `cargo run -p xtask -- check-architecture`
accepts.

## Implementation instructions

1. **`layer.toml` — `introduced_capabilities`.** Append exactly two entries:
   `"PreparationTask"` and `"PreparationSchedule"`.

   The checker's rule (`check.rs:313`) is **one-directional**: it iterates the
   declared list and asserts each is publicly exported. It never asserts the
   reverse. So these two additions are strictly optional *mechanically* — but the
   crate's convention is a 1:1 correspondence (18 capabilities ↔ 18 `pub use`
   lines) and this manifest exists to keep that true. Do **not** add
   `Runtime::prepare` or `PreparationFailed`: they are methods/variants, not
   root exports, and `locate_public_export` would not find them.

2. **`layer.toml` — `[[proof_exports]]`: add NOTHING.**

   An earlier draft told you to add a block for `PreparationSchedule` with
   `must_reference = ["HandleId"]`. That is now **wrong and would fail the
   checker**: the schedule no longer takes a `HandleId` (README §7 removed it),
   so `references_symbol` would not find it and `check.rs:331-403` would raise
   `ProofReferenceMissing`.

   Nothing needs adding. `MissingProofExport` fires only when a non-root layer
   has **zero** proof exports (`check.rs:342`), and `runtime` already has five.
   A proof export you cannot satisfy is worse than none.

3. **`layer.toml` — `consumed_capabilities`.** `HandleId` is already listed. No
   change. (Note: this field is parsed and **never checked** — it is
   documentation.)

4. **`meaningful_dependency`.** Extend the existing sentence to mention that the
   layer now also provides a startup preparation phase gating the transition to
   `Running`. This field is **required** by the schema but never checked; write
   it for a human.

5. **`ARCHITECTURE.md`.** Update the lifecycle sentence at `:8` to
   `Created → Initialized → Prepared → Running ↔ Paused → Stopped` and
   `→ Failed`. Add a short section covering:
   - what preparation is and what it is not (point at
     `docs/work-manifests/startup-preparation/README.md` §1);
   - that the runtime owns the *phase* and never a *product* — no `MeshBuffer`,
     no `Handle<T>`, no `Box<dyn Any>`, nothing it could name from a higher tier;
   - that the schedule is taken by value and dropped, so scratch state dies at
     the barrier;
   - that execution is sequential and single-pass, and **why**: the crate
     contains zero `async`, and `wasm32-unknown-unknown` has no threads here;
   - the scope limit — the barrier gates `Runtime::step`, not
     `RunningApp::render` (README §2, "Scope limit").

   Do **not** delete or weaken the "intentionally does not implement" list at
   `:41-53`; preparation adds no async, no host integration and no event loop,
   so that list remains true as written.

## Required behavior

Documentation only. The manifest must parse (`#[serde(deny_unknown_fields)]` on
every struct — a typo'd key is a `ManifestInvalid` violation) and every declared
capability must resolve to a real public export.

## Error behavior

N/A.

## Determinism requirements

N/A.

## Tests

None owned. Your correctness is proved by the architecture checker.

## Architecture validation

- `CapabilityNotExported` fires if either new capability name does not match a
  `pub`/`pub use` line in `crates/axiom-runtime/src/**`. Match the **exact**
  identifier.
- `MissingProofExport` fires only on a non-root layer with **zero** proof
  exports; `runtime` has five, so it cannot fire.
- `ProofReferenceMissing` **would** fire if you added a proof export whose
  `must_reference` symbol is absent — which is exactly why you add none.
- `ManifestInvalid` fires on any unknown key.

## Performance considerations

None.

## Documentation changes

The two files above are the deliverable.

## Completion criteria

- [ ] `introduced_capabilities` contains `PreparationTask` and
      `PreparationSchedule`
- [ ] **No** new `[[proof_exports]]` block (adding one would fail the checker)
- [ ] `meaningful_dependency` mentions the preparation phase
- [ ] `ARCHITECTURE.md` lifecycle line includes `Prepared`
- [ ] `ARCHITECTURE.md` states the no-product rule, the drop-at-barrier rule,
      the sequential-execution rationale, and the scope limit
- [ ] `cargo run -p xtask -- check-architecture` passes
- [ ] Zero `.rs` files changed

## Validation commands

```sh
cargo run -p xtask -- check-architecture
cargo test -p xtask
git diff --name-only        # must list exactly two paths
```

## Deliverable to orchestrator

Report: commit hash; both file paths; checker output; confirmation that
`git diff --name-only` lists exactly two files and no `.rs`; deviations.
