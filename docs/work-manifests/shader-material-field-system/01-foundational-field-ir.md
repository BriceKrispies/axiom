# 01 — The foundational field IR

## Objective

Create the layer `crates/axiom-field` and land the **representation only**: the
typed value union, the closed 23-op algebra, the per-op signature table, the
typed graph built on `RecipeGraph`, the explicit evaluation-context description,
the parameter table, canonical serialization and the content digest.

**No evaluator in this manifest** (that is `03`) and **no canonicalisation** (that
is `02`). This manifest lands the vocabulary and the bytes.

## Architectural placement

**Layer: `field`** — a new crate `crates/axiom-field`.

```toml
[layer]
name = "field"
crate_name = "axiom-field"
depends_on = ["kernel", "math", "recipe", "noise"]
```

Justification in full is `00-architecture-findings.md` §2.1. Summary: three
**layers** (`mesh-ops`, `proc-texture`, `proc-mesh`) must name a field, and a
layer may not depend on a module — so a module placement is structurally
impossible, not merely awkward. This is the `axiom-mesh` precedent verbatim
(`crates/axiom-mesh/src/lib.rs:26`).

### `layer.toml` — fill these in honestly or it is not a layer

```toml
meaningful_dependency = """
Field adapts recipe's domain-free operator DAG (RecipeGraph, NodeId, Param, the
append-only acyclic-by-construction invariant, canonical little-endian bytes and
StableHash digest) into a *typed* pointwise expression language: it assigns
meaning to the opaque u16 operator code through a const signature table, gives
every node a FieldType inferred from its inputs, and rejects a graph whose types
do not compose. It builds math's Vec2/Vec3/Vec4 into the value union those
expressions carry and Mat4 into the coordinate-transform operator, and noise's
value_noise/Fbm into the two spatial source operators the algebra could not
otherwise express. Recipe owns the container and proves it acyclic; this layer
adds the type system, the operator algebra, and the evaluation contract that make
the container a language.
"""

introduced_capabilities = [
  "FieldGraph", "FieldBuilder", "FieldId", "FieldType", "FieldValue",
  "FieldOp", "FieldSignature", "FieldParams", "FieldParamSlot",
  "EvalContext", "FieldError", "FieldErrorCode", "FieldResult",
  "FIELD_SCHEMA_VERSION",
]

consumed_capabilities = [
  "RecipeGraph", "Node", "NodeId", "Param", "Scalar", "RecipeError",
  "Vec2", "Vec3", "Vec4", "Mat4",
  "value_noise", "Fbm", "FbmConfig", "NoiseValue",
  "StableHash", "BinaryWriter", "BinaryReader", "SchemaVersion", "KernelError",
  "Ratio", "Seconds",
]

[[proof_exports]]
export = "FieldGraph"
must_reference = ["RecipeGraph", "NodeId", "StableHash"]

[[proof_exports]]
export = "FieldValue"
must_reference = ["Vec2", "Vec3", "Vec4"]

[[proof_exports]]
export = "EvalContext"
must_reference = ["Vec3", "Seconds"]
```

`engine_genuine_dependency` will verify each `depends_on` is referenced by a
resolved `DefId` in non-test code. `noise` is genuinely referenced **only once
`03` lands the evaluator** — so in this manifest the `Noise`/`Fbm` signature
entries must reference `axiom_noise::FbmConfig` in the *signature table*
(parameter arity is derived from it), or `noise` must be omitted from
`depends_on` until `03`. **Take the first option**: the signature table's noise
entries genuinely name `FbmConfig`'s parameter count. Do not declare a dependency
you do not yet use.

## Existing code involved (study before writing)

| Path | Why |
|---|---|
| `crates/axiom-recipe/src/{recipe_graph,node,value,ids}.rs` | the container you build on, in full |
| `crates/axiom-proc-texture/src/{texture_op,dispatch}.rs` | **the template.** `#[repr(u16)]` enum whose discriminant is the table index + `const OPS: [TexOp; 11]` |
| `crates/axiom-mesh/src/lib.rs` | the "neutral value type as a layer" doc voice to match |
| `crates/axiom-noise/src/lib.rs` | *"no naked scalar reaches the public API"* — the newtype discipline |
| `crates/axiom-kernel/src/binary_reader.rs` | `read_tagged` — the sanctioned branchless tagged decode |
| `crates/axiom-state/src/state_id.rs` | `StateId::of_path` — deterministic id minting from a name |
| `docs/unbranching.md` | the branchless recipe catalog |

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-field/Cargo.toml` | create |
| `crates/axiom-field/layer.toml` | create |
| `crates/axiom-field/ARCHITECTURE.md` | create |
| `crates/axiom-field/src/lib.rs` | create (facade + module docs) |
| `crates/axiom-field/src/field_type.rs` | create |
| `crates/axiom-field/src/field_value.rs` | create |
| `crates/axiom-field/src/field_op.rs` | create |
| `crates/axiom-field/src/signature.rs` | create |
| `crates/axiom-field/src/field_graph.rs` | create |
| `crates/axiom-field/src/field_builder.rs` | create |
| `crates/axiom-field/src/field_params.rs` | create |
| `crates/axiom-field/src/eval_context.rs` | create |
| `crates/axiom-field/src/field_error.rs` | create |
| `crates/axiom-field/src/ids.rs` | create (`FieldId`, `FieldParamSlot`) |
| root `Cargo.toml` | add `"crates/axiom-field"` to `members` |

**Forbidden to modify:** anything outside `crates/axiom-field/` except the one
`members` line. In particular do not touch `crates/axiom-recipe` (that is `P2`).

## Dependencies on earlier manifests

**`P2`** — `FieldError` wraps `RecipeError`, and it must already carry a node id.
`P1` is not required for this manifest but must land before `05`.

## Public API / data contracts to introduce

### The type lattice — exactly four types

```rust
#[repr(u16)]
pub enum FieldType { Scalar = 0, Vec2 = 1, Vec3 = 2, Vec4 = 3 }
```

**Decisions, recorded so they are not relitigated:**

* **There is no `Color` type.** A colour is a `Vec4` in linear RGBA, documented.
  Adding `Color` doubles every signature-table row that already accepts `Vec4`
  and buys nothing the documentation does not.
* **There is no `Mask`/`Bool` type.** A mask is a `Scalar` in `0..=1`; clamping
  is a `Clamp` node. A boolean type would require comparison and selection
  operators, and selection is `Mix`, which is already in the algebra and is
  branchless by construction.
* **There is no `Coordinate` type.** A coordinate is a `Vec3`. The *space* it is
  in is a property of the `EvalContext`, not of the value — see below.

`FieldValue` is the runtime carrier and is a **tagged struct, not a data-carrying
enum** (Branchless Law; the `RenderCommand` precedent at
`modules/axiom-render/src/render_command.rs:12`):

```rust
pub struct FieldValue { ty: FieldType, x: Scalar, y: Scalar, z: Scalar, w: Scalar }
```

Unused lanes hold a fixed default that is never read for the wrong type.
Construction via `FieldValue::scalar/vec2/vec3/vec4`; inspection via
`as_scalar`/`as_vec3`/… `Scalar` here is `axiom_recipe::Scalar`, reused rather
than redefined — it is already the sanctioned quantity newtype for a raw `f32`
in a graph.

### The algebra — 23 operators, closed

```rust
#[repr(u16)]
pub enum FieldOp {
    // sources (read from the graph or the context)
    Const = 0, Point = 1, Uv = 2, Normal = 3, Time = 4, Param = 5,
    // arithmetic — width-generic
    Add = 6, Sub = 7, Mul = 8, Min = 9, Max = 10, Abs = 11,
    // shaping — width-generic
    Clamp = 12, Mix = 13, Smoothstep = 14,
    // vector
    Dot = 15, Length = 16, Normalize = 17, Compose = 18, Component = 19,
    // spatial
    Noise = 20, Fbm = 21, Transform = 22,
}
```

**23 variants, under the `engine_no_large_enums` cap of 24, with one slot spare.
Do not spend it casually.** If a 25th operator is ever needed the discriminant
must move to a bare `u16` code with a `const` catalog — the `axiom-recipe` shape.

**What is deliberately *not* an operator, and why:**

| Excluded | Reason |
|---|---|
| `Div` | Division by zero is a determinism hazard and a NaN source. Multiply by a constant reciprocal, or add a guarded op later with an explicit fallback value. |
| `Pow`, `Exp`, `Log`, `Sin`, `Cos` | Transcendentals differ between CPU `f32` and GPU `f32` by more than the parity tolerance, and none is needed by any duplication in `00-architecture-findings.md` §1.8. Add one only when a real consumer presents the wall. |
| `Step` | `Smoothstep` with equal edges. |
| `Cross` | Expressible via `Compose` + arithmetic. Promote it only if normal/tangent work in `10` proves the node-count cost real. |
| `dpdx`/`dpdy` screen-space derivatives | Backend-specific, absent on the CPU and on Canvas2D, and the cause of a real past defect (mobile-GPU derivative NaN from zero-UV quads). Height→normal is finite differences at a **caller-supplied** offset — see `04`. |
| `If`/`Select`/`Compare` | Selection is `Mix`. A comparison operator is the seed of control flow in a language that must stay branchless end-to-end. |
| `Texture` / `Sample` | A texture is a rendering resource. A field that samples an image is a *later, separate* decision with real capability consequences; see `07`. |
| Anything named marble/wood/rust/dirt/asphalt | Library graphs, not primitives. See `12`. |

### Signatures — a `const` table, the type system's whole implementation

```rust
pub struct FieldSignature { inputs: u8, params: u8, kind: SignatureKind }
const SIGNATURES: [FieldSignature; 23] = [ /* one row per FieldOp, in discriminant order */ ];
```

`SignatureKind` is a small fieldless enum describing how the output type is
derived — the four rules the algebra needs:

* `Fixed(FieldType)` — `Point`/`Normal` → `Vec3`, `Uv` → `Vec2`, `Time` → `Scalar`.
* `FromParams` — `Const`, `Param`: the declared type rides in a parameter word.
* `WidthGeneric` — `Add`/`Sub`/`Mul`/`Min`/`Max`/`Abs`/`Clamp`/`Mix`/`Smoothstep`:
  output type equals the widest input; all non-scalar inputs must agree.
  (Scalar-broadcasts-to-vector is permitted and is the only implicit conversion
  in the language.)
* `ScalarOut` — `Dot`, `Length`, `Noise`, `Fbm`.
* `Vec3Out` — `Normalize`, `Transform`.
* `Explicit` — `Compose` (output width from param), `Component` (always `Scalar`).

Type *checking* is `02`. This manifest lands the table and the accessor.

### The graph

```rust
pub struct FieldGraph { recipe: RecipeGraph, output: NodeId, params: FieldParams }
```

`FieldGraph` **wraps** `RecipeGraph`; it does not reimplement it. Acyclicity,
node budget, ids, and the canonical node encoding all come from `recipe` for
free. What `field` adds: the declared `output` node (a `RecipeGraph` has no
notion of a result), the parameter table, and the type discipline.

`FieldBuilder` is the append-only authoring surface. Every `push_*` returns a
`NodeId`; inputs must be ids already returned. This is the same shape as
`RecipeGraph::add`, and it is what makes the graph acyclic by construction rather
than by search.

### Sharing, and why it is free

A `NodeId` may appear in any number of later nodes' `inputs`. That **is** the
DAG's sharing, inherited from `recipe`. There is no separate "reuse" concept, no
reference counting, and no subgraph type. A reusable material function is a
`FieldGraph` that a builder *inlines* — see `12`.

### Parameters — the mechanism that prevents variant explosion

```rust
pub struct FieldParamSlot(u16);
pub struct FieldParams { /* dense slot -> (FieldType, FieldValue) */ }
```

`FieldOp::Param` reads slot *n*. **Changing a parameter's value does not change
the graph, therefore does not change the digest, therefore cannot cause a shader
recompile.** This is the single most important performance property in the whole
design (see `09`), and it must be true from the first commit.

Slot names are minted deterministically from a string with
`StableHash::of_bytes(name.as_bytes())`, the `StateId::of_path` pattern — so an
agent can address a parameter by name without the name entering the wire format.
The name→slot map is authoring-side; the graph carries only the slot index.

### The evaluation context

```rust
pub struct EvalContext { point: Vec3, uv: Vec2, normal: Vec3, time: Seconds }
```

Every external input is **explicitly supplied**. There is no ambient anything.
`Time` is a kernel `Seconds` handed in by the caller — never a wall clock, which
`engine_no_time_in_sim` would catch anyway inside a `#[sim]` zone and which the
Determinism Rules forbid everywhere.

**Coordinate spaces are not typed; they are contextual.** `EvalContext::point` is
whatever space the *caller* supplies, and the caller documents it. `04` fixes the
convention for surfaces (object space, so a moving object's pattern does not
swim). `FieldOp::Transform` applies a `Mat4` from the parameter table, which is
how a graph moves between spaces explicitly. Adding a space *type* would put
scene semantics into the primitive — exactly the contamination
`00-architecture-findings.md` §2.2 forbids.

### Randomness

**There is none.** The only stochastic-looking operators are `Noise` and `Fbm`,
which are pure functions of `(seed, point)` where `seed` is a graph parameter.
`axiom-entropy` and `axiom-space` are deliberately not dependencies.

## Serialization requirements

* Canonical little-endian, `SchemaVersion`-stamped, built on
  `RecipeGraph::write_to` — do not invent a second byte format.
* `FIELD_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0)`.
* Layout: field schema stamp → the embedded `RecipeGraph` bytes → `output: u32` →
  the parameter table (`u32` count, then per slot: `u16` type, four `u32` words).
* `FieldGraph::digest() -> StableHash` over those bytes.
* **The bytes are the determinism proof; the digest is the label.** That is the
  kernel's stated stance (`stable_hash.rs`) and it must be repeated in this
  crate's docs so nobody treats the hash as the verdict.
* Decode is bounds-checked and never panics; a truncated buffer at *every* prefix
  length must fail cleanly (the `world_tag.rs:117-149` test shape).

## Determinism requirements

* Same graph → same bytes → same digest, on every target including wasm32.
* Node ids are dense insertion indices; nothing depends on address, iteration
  order, or `TypeId`.
* No `f64` anywhere — `BinaryWriter` has no `write_f64`, which enforces it.

## Testing requirements (100% or it does not land)

* One test per `FieldType` constructor and accessor, including reading a lane
  that is not part of the value's type (it must return the documented default,
  not garbage).
* `codes_are_their_dispatch_indices` — assert every `FieldOp` discriminant equals
  its index, the `texture_op.rs` test verbatim in spirit. This is what makes the
  `const` table safe.
* `SIGNATURES.len() == 23` and every row's `kind` matches its documented rule.
* Round-trip: build → serialize → deserialize → assert equal, for a graph
  exercising every op at least once.
* Truncation: `deserialize(&bytes[..n])` fails for every `n < bytes.len()`.
* Digest stability: a committed golden byte vector and its digest for one fixed
  graph, so a format change can never be silent.
* Parameter independence: two graphs identical but for parameter *values* have
  the **same digest**; two graphs differing in structure do not.

## Architecture tests

* `cargo xtask check-architecture` — new layer must satisfy `UnknownDependency`,
  `DependencyCycle`, `DisallowedLayerImport`, `PrivatePathImport`,
  `CapabilityNotExported`, `MissingProofExport`, `ProofReferenceMissing`.
* `engine_genuine_dependency` must find a resolved reference to each of
  `axiom_recipe`, `axiom_math`, `axiom_kernel` (and `axiom_noise` — see the
  warning in the `layer.toml` section above) in non-test code.
* `unused_crate_dependencies` must be clean.

## Performance risks

* **`Vec` per node.** `RecipeGraph::Node` holds `Vec<Param>` and `Vec<NodeId>`.
  With `MAX_NODES = 256` this is bounded and acceptable for a *representation*.
  Do not "optimise" it into a flat arena here — that would change `recipe`'s wire
  format. `03` avoids the cost at evaluation time with a fixed register array.
* **Do not add interning in this manifest.** Structural dedup is `02`'s CSE pass,
  which is the correct place because it must run before hashing.
* **Protect these two properties**, because losing either makes the obvious
  optimisations impossible later: (a) node ids are dense and id-ordered, so an
  evaluator can be a flat fold with an indexed register file; (b) parameters are
  separate from structure, so a value change never touches the digest.

## Migration considerations

None — the crate is new and nothing depends on it yet. The `members` line is the
only shared edit, which is why nothing else may run concurrently with this
manifest.

## Completion criteria

1. `crates/axiom-field` exists, builds, and is a legal layer.
2. All 23 ops, 4 types, and the signature table are present and documented.
3. `FieldGraph` round-trips through canonical bytes with a committed golden.
4. `cargo xtask check-architecture` exits 0 with the new layer counted.
5. `scripts/coverage.sh` reports 100/100/100 including the new crate.
6. The new crate contributes **zero** findings to every dylint.

## Validation commands

```sh
cargo build -p axiom-field
cargo test -p axiom-field
cargo xtask check-architecture
cargo test --workspace
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 2, width 1.** Nothing may run concurrently: this manifest owns
`crates/axiom-field/src/lib.rs` and the root `Cargo.toml` members list.
