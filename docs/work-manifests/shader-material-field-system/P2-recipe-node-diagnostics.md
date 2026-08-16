# P2 — Node-pointing diagnostics in `axiom-recipe`

## Objective

Make a recipe/graph validation failure name **which node** failed. Today
`RecipeError` is a fieldless enum, so `CyclicInput` cannot say where. Every
agentic requirement in the brief — "receive structured diagnostics", "diagnostics
pointing to semantic graph nodes rather than generated WGSL lines" — bottoms out
here, and the fix belongs at the lowest layer that owns the node id.

## Architectural placement

**Layer: `recipe`** (`crates/axiom-recipe`). Not a new package. This is the
lowest correct layer: `NodeId` is defined here, `validate()` is here, and every
higher graph language (`proc-texture`, `proc-mesh`, and the future `field`)
inherits the error type from here.

## Existing code involved

| Path | Current shape |
|---|---|
| `crates/axiom-recipe/src/recipe_error.rs` | fieldless `enum RecipeError { NodeLimitExceeded, CyclicInput, MalformedData }`, `code() -> u16` via the table `[1,2,3][self as usize]` |
| `crates/axiom-recipe/src/recipe_graph.rs:77-89` | `validate()` — the two rules that need to report a location |
| `crates/axiom-recipe/src/ids.rs` | `NodeId(u32)` |
| `crates/axiom-state/src/state_error.rs` | **the pattern to copy** |

## The pattern to copy — do not invent a new one

`crates/axiom-state/src/state_error.rs`:

```rust
pub struct StateError { code: StateErrorCode, state: StateId, message: &'static str, cause: Option<KernelError> }
pub const fn new(code, message) -> Self
pub const fn at(code: StateErrorCode, state: StateId, message: &'static str) -> Self
pub const fn caused_by(self, cause: KernelError) -> Self
pub const fn about(self, state: StateId) -> Self   // stamp the location on the way out
pub const fn state(self) -> StateId                // or StateId::NULL
```

Its doc on `about` states the idiom exactly: *"A decode helper does not know which
state it was decoding; the caller does, and stamps it on the way out so the
diagnostic names the slot."*

**Critically: the id is a struct field, not an enum payload.** A data-carrying
enum variant forces a `match` on read and violates the Branchless Law. Note also
that `KernelError`'s identity is the `(scope, code)` pair and its `&'static str`
message deliberately does not participate in `PartialEq` — mirror that so
existing equality assertions keep working.

## Files likely to change

| Path | Action |
|---|---|
| `crates/axiom-recipe/src/recipe_error.rs` | rewrite: fieldless `RecipeErrorCode` + a `Copy` `RecipeError { code, node, message }` struct |
| `crates/axiom-recipe/src/recipe_graph.rs` | `validate()` stamps the offending `NodeId`; `deserialize` stamps `NodeId::NULL` |
| `crates/axiom-recipe/src/lib.rs` | export `RecipeErrorCode` alongside `RecipeError` |
| `crates/axiom-recipe/layer.toml` | add `RecipeErrorCode` to `introduced_capabilities` |
| `crates/axiom-proc-core/src/proc_error.rs` | may carry the node id through `ProcError::OpFailed` — **optional**, and only if it does not widen `proc-core`'s surface |
| call sites asserting `RecipeError::CyclicInput` | update to compare on `code()` |

## Dependencies on earlier manifests

**None. May begin immediately.** Fully parallel with `P1`.

## Public API / data contracts

Add `NodeId::NULL` (raw `u32::MAX`, or `0` with an explicit `is_valid`) so an
error that has no node can say so without an `Option`, matching the kernel's
`define_id!` convention where raw `0` is reserved. **Choose `u32::MAX`**: node
ids are dense insertion indices starting at `0`, so `0` is a real node here and
the kernel's convention does not transfer.

Keep `RecipeError: Copy + PartialEq`. Keep `code() -> u16` stable — existing
callers and any serialized verdict depend on the numeric codes `1/2/3`.

## Explicitly excluded

* No type checking. That is `02`'s job, in `axiom-field`.
* No new error *kinds* beyond the three that exist.
* No `String` in the error. `&'static str` only — the kernel has no string
  serialization primitive and an allocation in an error path is a smell here.
* Do not touch `crates/axiom-proc-validate` (that is `P1`).

## Determinism requirements

Error identity must remain `(code, node)` and must be reproducible. Two runs over
the same malformed graph must report the same node.

## Serialization requirements

`RecipeError` is not serialized today and must not become serialized here. If a
future manifest needs a wire error, it uses `code(): u16` plus `node().raw(): u32`.

## Testing requirements

* `forward_reference_is_cyclic` and `self_reference_is_cyclic` (existing tests in
  `recipe_graph.rs`) extended to assert the **reported node id**.
* A graph whose 5th node is the offender reports node 4, not node 0 — proving the
  stamp is the real index, not a constant.
* `NodeLimitExceeded` reports `NodeId::NULL` (it is a whole-graph property).
* `MalformedData` from truncated bytes reports `NodeId::NULL`.
* `code()` still returns `1/2/3`.
* 100% coverage on the new accessors and the `about`/`caused_by` builders — note
  the Coverage Law counts every builder even if only one call site exists, so do
  not add a builder you do not use.

## Architecture tests

`cargo xtask check-architecture` — `CapabilityNotExported` fires if
`RecipeErrorCode` is added to `introduced_capabilities` without being a public
export.

## Performance risks

None. `RecipeError` grows from 1 byte to ~16; it is returned only on failure.

## Migration considerations

Every existing `assert_eq!(…, Err(RecipeError::CyclicInput))` across the repo
breaks at compile time — that is the desired failure mode. Sweep
`crates/axiom-proc-texture`, `crates/axiom-proc-mesh`, `crates/axiom-proc-core`
and their tests.

## Completion criteria

1. `RecipeError` is a `Copy` struct carrying `(RecipeErrorCode, NodeId, &'static str)`.
2. `validate()` names the offending node for `CyclicInput`.
3. `code()` values are unchanged.
4. `cargo test --workspace` passes; `crates/axiom-recipe` is at 100% coverage.
5. `cargo xtask check-architecture` exits 0.
6. No dylint count rises.

## Validation commands

```sh
cargo test -p axiom-recipe
cargo test --workspace
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```
