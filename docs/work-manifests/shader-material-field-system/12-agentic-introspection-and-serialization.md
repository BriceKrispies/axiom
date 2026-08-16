# 12 — Agentic introspection, serialization, and the library tier

## Objective

Make a field graph and a surface **mechanically editable by an agent**: inspect
nodes, read types, walk dependencies, insert, replace and reuse subgraphs,
validate, diff, hash, explain, and receive structured diagnostics that name a
node rather than a WGSL line. Then establish the **library tier** — the rule that
new visual effects are authored graphs, never new Rust.

## Architectural placement

* Graph inspection and rewriting: **Layer `field`** (`crates/axiom-field`).
* Surface inspection: **Layer `surface`** (`crates/axiom-surface`).
* Report shapes: **Layer `introspect`** is the precedent to imitate, **not**
  necessarily to depend on — see below.
* An agent-facing dump tool: **Tooling** (`tools/`).

**Why not put this in `crates/axiom-introspect`.** That layer is narrow by its
own discovery report: it introspects *frame execution* — `FrameReport`,
`SystemReport`, `MetricReport`, and a `WorldReport` that is literally two
integers. Nothing in it is generic or recursive. Making it name a `FieldGraph`
would give `introspect` (`depends_on = ["kernel", "frame", "ecs"]`) a dependency
on `field` for no benefit to its existing consumers. The graph owns its own
introspection; `introspect` remains the *frame* reporter.

**What to copy from it:** `crates/axiom-introspect/src/world_tag.rs:117-149` is
the repo's best schema-stamped, length-prefixed, truncation-tested set codec.
Match that shape.

## Existing code involved

| Path | Role |
|---|---|
| `crates/axiom-kernel/src/{reflect,type_schema}.rs` | `Reflect`, `TypeSchema`, `FieldSchema` — flat, one level deep, `const` |
| `crates/axiom-kernel/src/stable_hash.rs` | `StableHash` — *"a diagnostic index, never the proof"* |
| `crates/axiom-state/src/state_error.rs` | `StateError::at`/`.about` — the node-pointing diagnostic pattern |
| `crates/axiom-state/src/state_id.rs` | `StateId::of_path` — deterministic id from a name |
| `tools/axiom-proc-inspect/src/main.rs` | the existing provenance dump — **text only, no JSON** |
| `tools/axiom-proc-fuzz/src/main.rs` | the 2,000-seed determinism sweep |
| `examples/recipes/generated_micro_fps/` | *"the whole game's art is 1796 bytes of packed recipe that expand into ~0.29 MB of textures"* |
| `apps/arena-forge/EFFECT_LANGUAGE.md` | the repo's only shipped typed declarative authoring language — *"Cards carry data, never code"* |
| `docs/game-vocabulary.md` | the Vocabulary Law, admission test #5: **Introspectable** |

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-field/src/inspect.rs` | create |
| `crates/axiom-field/src/rewrite.rs` | create |
| `crates/axiom-field/src/diff.rs` | create |
| `crates/axiom-surface/src/inspect.rs` | create |
| `tools/axiom-field-inspect/` | create (a Tool — outside the engine graph and the coverage gate) |
| `crates/axiom-field/ARCHITECTURE.md` | modify — the library-tier rule |

## Dependencies on earlier manifests

**`04`.** Parallel with `06`.

## Public API / data contracts

### Inspection

```rust
impl FieldGraph {
    pub fn node_count(&self) -> usize;
    pub fn op_at(&self, node: NodeId) -> FieldResult<FieldOp>;
    pub fn type_at(&self, node: NodeId) -> FieldResult<FieldType>;
    pub fn inputs_at(&self, node: NodeId) -> FieldResult<&[NodeId]>;
    pub fn dependents_of(&self, node: NodeId) -> FieldResult<Vec<NodeId>>;
    pub fn output(&self) -> NodeId;
    pub fn describe(&self) -> FieldDescription;   // schema-stamped, serializable
}
```

`op_at`/`inputs_at` are trivially available because `FieldGraph` wraps
`RecipeGraph`, whose nodes are already a public, indexable, id-ordered list.
`type_at` reuses `02`'s inference. **`dependents_of` is a forward scan**, not a
stored reverse index — storing one would be retained state and would need
invalidation.

### Rewriting — the operations an agent actually needs

```rust
impl FieldGraph {
    pub fn replace_subgraph(&self, at: NodeId, with: &FieldGraph) -> FieldResult<FieldGraph>;
    pub fn insert_before(&self, at: NodeId, node: &FieldGraph) -> FieldResult<FieldGraph>;
    pub fn inline(&self, other: &FieldGraph, bind: &[NodeId]) -> FieldResult<FieldGraph>;
}
```

**Every rewrite returns a new graph.** No `&mut self` on a public boundary
(`engine_no_retained_state`), and immutability is what makes a rewrite diffable
and revertible — the property an agent needs most.

`inline` is how **reusable material functions** work: a library graph is a
`FieldGraph` whose leaf `Point`/`Uv`/`Param` nodes are *bound* to nodes of the
host graph at inline time. There is no separate "function" type, no call node,
and no linker. This is the single most important simplification in the manifest —
it means a library of a hundred effects needs zero engine machinery.

**Watch the budget:** inlining multiplies node count against `MAX_NODES = 256`.
`InlineBudgetExceeded` is a real, tested error, and it is a design signal for the
author to compose fewer layers — not a reason to raise the cap.

Every rewrite result must be re-validated (`02`) before use. State it in the docs
and make the error path a test.

### Diff

```rust
pub struct FieldDiff { added: Vec<NodeId>, removed: Vec<NodeId>, changed: Vec<NodeId> }
pub fn diff(before: &FieldGraph, after: &FieldGraph) -> FieldDiff;
```

**Diff both graphs canonicalised** (`02`), or the result is dominated by
authoring-order noise. Canonicalisation was designed for exactly this and for the
program cache key; this is its second consumer.

### Explanation

```rust
pub fn explain(&self) -> FieldExplanation;   // one line per node, in id order
```

Deterministic text, one line per node: `n7: Mul(Scalar) <- n5, n6`. Not a
pretty-printer, not a DSL, and **not** a parser — there is no textual authoring
format in this manifest. Note that `crates/axiom-recipe`, `axiom-proc` and
`axiom-proc-core` contain zero parsing code and no `toml`/`serde` dependency
today; recipes are authored in Rust. Introducing a text *format* is a separate
decision with a wire-compatibility cost, and it is out of scope. `explain` is
output-only.

### Diagnostics that name a node

Already established by `P2` (`RecipeError` carries a `NodeId`) and extended by
`02` (each `FieldErrorCode` names its node). This manifest adds the surface tier:
a `SurfaceError` names both the `SurfaceChannel` and the `NodeId` within that
channel's graph. That is the brief's *"diagnostics pointing to semantic graph
nodes rather than generated WGSL lines"*, satisfied end to end.

### Determining backend support before rendering

`Surface::requirements()` (`04`) checked against `BackendCapabilityProfile`
(`crates/axiom-host`) — exposed to an agent as a pure query:

```rust
pub fn supported_by(reqs: &SurfaceRequirements, profile: BackendCapabilityProfile) -> bool;
```

An agent can therefore answer *"will this render on Canvas2D?"* without
attempting a render. That closes the last item on the brief's agent checklist.

### The tool

`tools/axiom-field-inspect` — dumps a graph's nodes, types, dependents, digest,
and `explain()` output. **Emit JSON, not only text.** `tools/axiom-proc-inspect`
is text-only, which makes it unusable to a program; do not repeat that. Tools are
outside the engine dependency graph and outside the coverage gate, so this is
cheap.

## The library tier — the rule that keeps the engine small

Record this in `crates/axiom-field/ARCHITECTURE.md` as a standing rule:

> **A visual effect is an authored graph, not an engine primitive.** Marble, wood
> grain, scratches, rust, dirt, skin pores, asphalt, water ripples, brushed metal
> and fabric weave are all compositions of the 23 operators. The engine must never
> gain a Rust function per effect. If an effect cannot be expressed, the question
> is whether the *algebra* is missing something universal — and the answer is
> usually no.

The engine has already run this experiment successfully:
`examples/recipes/generated_micro_fps/` produces ~0.29 MB of textures from
**1,796 bytes** of packed recipe, and its own notes say *"ship the recipe, not the
resources."* `apps/arena-forge/EFFECT_LANGUAGE.md` is the same idea shipped at the
app tier: *"Cards carry data, never code."*

**Where the library lives is deliberately not decided here.** Candidate homes are
a `library/` module of `const fn` builders in `crates/axiom-field` (cheap, but
every entry costs coverage), an app-tier module, or authored `.bin` graph assets.
Pick it when the first three real effects exist, not before — building a library
mechanism before there is a library is the kind of speculative structure this
repo reverts (`a5a9472f`).

The admission test for a **new operator**, to be applied strictly:

1. It cannot be composed from existing operators without unbounded node growth.
2. It is implementable identically on CPU and in WGSL within the parity tolerance.
3. At least two unrelated consumers need it.
4. It fits under the 24-variant `engine_no_large_enums` cap (one slot remains).

`smin`, `triplanar`, `checker`, `bricks`, `gradient` and `voronoi` all fail test
1 or 3 and are therefore **library graphs**. (Voronoi is worth naming explicitly:
it is expressible as a bounded fold over neighbour cells, but not compactly — if
it proves genuinely necessary it is the strongest candidate for the one free
operator slot, and it needs its own decision record.)

## Explicitly excluded

* No textual authoring format, no parser, no `serde`, no TOML.
* No runtime graph mutation. Rewrites are authoring-time and return new values.
* No visual editor.
* No dependency from `axiom-introspect` on `axiom-field`.
* No effect library content in this manifest.

## Determinism requirements

`describe`, `explain`, and `diff` are pure functions with deterministic ordering
(node id order everywhere, no map iteration). `diff` is symmetric-stable: the
same pair always yields the same result.

## Serialization requirements

`FieldDescription` is schema-stamped and byte-serializable through
`BinaryWriter`, following `world_tag.rs`'s codec shape, with truncation tested at
every prefix. `explain()` output is text and is **not** a wire format — do not
golden it as if it were a contract.

## Testing requirements (100%)

* Every accessor, including out-of-range node ids.
* `dependents_of` on a shared node returns every dependent.
* `replace_subgraph` produces a graph that validates and evaluates to the
  expected new value.
* `inline` binds leaves correctly; an over-budget inline is rejected.
* `diff` of a graph with itself is empty; of two canonicalised equivalents is
  empty; of a real edit names exactly the changed nodes.
* `supported_by` agrees with `07`'s `validate` on every case.
* `SurfaceError` names both channel and node.
* Round-trip of `FieldDescription`, with truncation tests.

## Architecture tests

`cargo xtask check-architecture` — `tools/axiom-field-inspect` must classify as a
Tool and must not be depended on by any layer, module or app
(`ToolImportedByEngine`).

## Performance risks

* `dependents_of` is O(nodes × inputs) per call. Bounded by `MAX_NODES = 256` and
  authoring-time only. **Never call it from a frame path**; say so in the doc.
* `inline` and `replace_subgraph` allocate a new graph each. Authoring-time only.
* `diff` canonicalises both inputs, which is the expensive part. Accept it;
  correctness beats speed for a tool an agent runs once per edit.

## Migration considerations

None. Additive.

## Completion criteria

1. Every operation on the brief's agent checklist is a real, tested API:
   identify, inspect type, inspect dependencies, insert, replace, reuse, validate,
   serialize, diff, hash, explain, compile, structured diagnostics, backend
   support query.
2. `tools/axiom-field-inspect` emits JSON.
3. The library-tier rule and the operator-admission test are written into
   `crates/axiom-field/ARCHITECTURE.md`.
4. Coverage 100/100/100 on both layers; `cargo xtask check-architecture` exits 0;
   no dylint count rises.

## Validation commands

```sh
cargo test -p axiom-field -p axiom-surface
cargo run -p axiom-field-inspect -- --help
cargo test --workspace
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 6.** Parallel with `06`. Owns new files in both layers plus a new tool
crate; coordinate the root `Cargo.toml` members edit with whatever else is in
flight.
