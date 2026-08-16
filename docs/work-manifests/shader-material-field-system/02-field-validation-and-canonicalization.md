# 02 — Field validation and canonicalisation

## Objective

Make an invalid graph unrepresentable and an equivalent graph indistinguishable.
Land type checking against the signature table, and the canonicalisation pass —
constant folding, common-subexpression elimination, dead-node elimination, and
deterministic relabelling — so that **two graphs that compute the same thing
produce the same bytes and therefore the same digest**.

## Architectural placement

**Layer: `field`** (`crates/axiom-field`). No new package.

This is not an optimiser bolted on for speed. Canonicalisation is what makes the
digest usable as a **program cache key** (`09`) and what lets an agent diff two
graphs meaningfully (`12`). It is backend-independent by construction, which is
exactly why it belongs here and not in a backend.

## Existing code involved

| Path | Why |
|---|---|
| `crates/axiom-field/src/{signature,field_op,field_type,field_graph}.rs` | from `01` |
| `crates/axiom-recipe/src/recipe_graph.rs:77-89` | `validate()` — structural validation already done for you; do not duplicate it |
| `crates/axiom-state/src/state_shape_id.rs:24-34` | `schema_word` — the repo's only structural fold-hash, the shape to copy for a node key |
| `crates/axiom-kernel/src/stable_hash.rs` | `of_bytes`, `of_words` |

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-field/src/type_check.rs` | create |
| `crates/axiom-field/src/canonical.rs` | create |
| `crates/axiom-field/src/field_graph.rs` | modify — add `type_of`, `validate`, `canonicalize` |
| `crates/axiom-field/src/field_error.rs` | modify — add the type-error codes |
| `crates/axiom-field/src/lib.rs` | modify — export what is new |
| `crates/axiom-field/layer.toml` | modify — extend `introduced_capabilities` |
| `crates/axiom-field/ARCHITECTURE.md` | modify — document the canonical form |

## Dependencies on earlier manifests

**`01`, strictly.** Same-crate, same `lib.rs` — must not run concurrently with
`01` or `03`.

## Public API / data contracts

```rust
impl FieldGraph {
    pub fn type_of(&self, node: NodeId) -> FieldResult<FieldType>;
    pub fn validate(&self) -> FieldResult<()>;
    pub fn canonicalize(&self) -> FieldResult<FieldGraph>;
    pub fn is_canonical(&self) -> bool;
}
```

### How invalid graphs are rejected

Validation is a **single forward fold in id order** — no recursion
(`engine_no_recursion`), no second pass. Because inputs reference only
strictly-earlier nodes, every input's type is already known when a node is
reached. Accumulate `Vec<FieldType>` indexed by node id; each step looks up
`SIGNATURES[op]`, checks arity, checks the input types against the rule, and
pushes the derived output type.

Rejected, each with its own `FieldErrorCode` and the offending `NodeId` (the
`P2` pattern):

| Code | Condition |
|---|---|
| `UnknownOperator` | `op >= 23` |
| `WrongInputCount` | arity mismatch against the signature |
| `WrongParamCount` | parameter-word count mismatch |
| `TypeMismatch` | a `WidthGeneric` op whose non-scalar inputs disagree in width |
| `ComponentOutOfRange` | `Component` index ≥ the input's width |
| `ComposeWidthInvalid` | `Compose` width not in `2..=4`, or input count ≠ width |
| `UnknownParamSlot` | `Param` references a slot absent from `FieldParams` |
| `OutputNodeMissing` | the declared output id is out of range |
| `NonFiniteConstant` | a `Const` parameter word decodes to NaN or ±∞ |

**Cycles are not checked here.** They are structurally impossible —
`RecipeGraph::validate` already proves every input id is strictly smaller than
its node's index, and that is the complete cycle argument for an id-ordered
append graph. Re-checking would be duplicated logic with no new guarantee. Call
`recipe.validate()` first and map its error.

`NonFiniteConstant` matters more than it looks: `ScalarField::new` already
rejects non-finite values with `MeshErrorCode::NonFiniteAttribute`, and a NaN
that enters a graph propagates silently to every consumer. Reject at the door.

### The canonical form — four passes, in this order

1. **Constant folding.** A node all of whose inputs are `Const` and whose op is
   pure and total is replaced by a `Const`. `Noise`/`Fbm`/`Param` are foldable
   only if their seed/slot inputs are constant *and* the op is genuinely
   deterministic across targets — **fold `Noise` and `Fbm` only if the CPU
   evaluator from `03` is already the semantic reference**, so do this in `03` or
   later, not here. In this manifest fold only arithmetic and shaping ops.
2. **Common-subexpression elimination.** Key each node by
   `(op, params[], canonical_input_ids[])` and reuse the first node with that
   key. Because ids are dense and processed in order, one forward pass with a
   key→id map suffices. **Commutative normalisation:** for `Add`, `Mul`, `Min`,
   `Max` sort the input ids ascending before keying, so `a+b` and `b+a` collapse.
   Do **not** attempt associativity or distributivity — it changes floating-point
   results, and the CPU/GPU parity budget cannot absorb that.
3. **Dead-node elimination.** Mark from the output node backwards over the
   already-built dependency lists; drop unmarked nodes.
4. **Deterministic relabelling.** Emit surviving nodes in ascending original id
   order into a fresh dense `0..n`. This is already a valid topological order, so
   no sort is needed and no tie-break rule can drift.

The result is a `FieldGraph` for which `is_canonical()` is true and
`canonicalize()` is idempotent.

**What canonicalisation deliberately does not do:** algebraic rewriting
(`x*1 → x`, `x+0 → x`) beyond exact constant folding, strength reduction,
reassociation, or any transform whose result differs in the last float bit.
Those belong nowhere — they would break the CPU/GPU parity contract that `08`
depends on.

## Explicitly excluded

* No optimisation aimed at *shader* size or *shader* cost. That is `08`, and it
  operates on the emitted WGSL, not on the semantic graph.
* No graph rewriting API for agents here — inspection and rewriting is `12`.
* No caching of canonical results. `canonicalize` is a pure function; memoising
  it would be retained state.

## Determinism requirements

* `canonicalize` is a pure function of the input graph. Same input → identical
  output bytes, on every target.
* `canonicalize(canonicalize(g)) == canonicalize(g)` — idempotence, tested.
* Two structurally equivalent graphs authored in different orders canonicalise to
  **byte-identical** bytes and therefore the same digest. This is the headline
  property and it needs its own named test.

## Serialization requirements

Unchanged from `01` — canonicalisation produces a `FieldGraph`, which serializes
by the existing rules. Add one committed golden: a deliberately messy graph
(duplicate subexpressions, a dead branch, a foldable constant chain) plus its
canonical bytes and digest.

## Testing requirements (100%)

* One rejection test per `FieldErrorCode`, each asserting **both** the code and
  the reported `NodeId`.
* Scalar-broadcast acceptance: `Add(Vec3, Scalar)` is legal and yields `Vec3`.
* Width disagreement rejection: `Add(Vec3, Vec2)` is `TypeMismatch`.
* Fold: a chain of `Const`-only arithmetic collapses to one node.
* CSE: a graph using the same subexpression twice loses one node; the
  commutative case (`a+b` vs `b+a`) collapses too.
* DCE: a graph with a branch unreachable from the output loses it.
* Idempotence.
* Order independence: two hand-built graphs, same semantics, different authoring
  order → equal digests.
* A canonical graph still validates and still type-checks.

## Architecture tests

`cargo xtask check-architecture` — `CapabilityNotExported` for each new export.
`engine_no_recursion` must stay at 0: the DCE mark phase is a **reverse fold over
the id-ordered node list**, not a graph walk. Write it that way deliberately and
say so in a comment, because the recursive version is the obvious one.

## Performance risks

* CSE's key map is the one place a `HashMap` is tempting. The renderer has
  already been burned by per-frame `hashbrown` (~10% of a throttled frame,
  `frame_packet_adapter.rs:32-49`). **Canonicalisation is a preparation-time
  operation, not a per-frame one**, so a map is acceptable here — but say so in
  the code, and never call `canonicalize` from a frame path.
* `MAX_NODES = 256` bounds every pass at 256 iterations. Do not raise it in this
  manifest; if a real graph exceeds it, that is a signal to inline less
  aggressively in `12`, not to raise the cap.

## Migration considerations

None; additive within a crate nothing depends on yet.

## Completion criteria

1. Every `FieldErrorCode` is reachable and tested, and names its node.
2. `canonicalize` is idempotent and order-independent, with a committed golden.
3. `cargo xtask check-architecture` exits 0.
4. `scripts/coverage.sh` reports 100/100/100.
5. Zero new dylint findings, `engine_no_recursion` still 0.

## Validation commands

```sh
cargo test -p axiom-field
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 3, width 1.** Owns `crates/axiom-field/src/lib.rs`. Sequential after `01`,
before `03`.
