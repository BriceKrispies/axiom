# `axiom-field` — the typed pointwise field IR

> A **field** is a deterministic, hashable, canonically-serializable pure
> function from an explicitly supplied typed evaluation context to a typed
> value, represented as a closed-algebra, id-ordered, acyclic typed expression
> graph.

This crate owns the **representation**, the **type rules**, the **canonical
form**, and the **semantic reference evaluator**. `FieldGraph::evaluate` is not a
testing convenience: what its operator functions compute **is** what the language
means, and every other realisation of a field — a shader emitted for a GPU
backend, a per-triangle CPU shading path on a backend that cannot execute a
shader at all — is a mirror checked against it.

## Placement

`Layer: field` — `crates/axiom-field`, `depends_on = ["kernel", "math",
"recipe", "noise"]`.

It is a **layer, not a module**, and that is forced rather than preferred: three
engine *layers* (`mesh-ops`, `proc-texture`, `proc-mesh`) must be able to name a
field, and a layer may never depend on a module. This is the `axiom-mesh`
precedent verbatim — the same argument that made the neutral triangle mesh a
layer makes the neutral field one.

The engine stated the need three times in the negative before this crate existed
(`crates/axiom-mesh-ops/src/implicit_surface.rs`,
`crates/axiom-proc-mesh/src/implicit.rs`, and a reverted attempt to solve it one
layer too high). A field is the missing *function-as-a-value*, and it has
nothing to do with rendering: a height for a displacement, a mask for a
placement rule, a density for an implicit surface and a colour for a material
are all the same value here.

## What each dependency is for

| Layer | What is genuinely adapted |
|---|---|
| `recipe` | The container. `RecipeGraph` supplies the append-only acyclic-by-construction DAG, `NodeId`/`RecipeId` its identity, `Param` its raw 32-bit words, `Scalar` the quantity newtype that keeps naked floats off the boundary, and `RecipeError` the diagnostics `FieldError` lifts one-for-one. |
| `math` | `Vec2`/`Vec3`/`Vec4` are the lanes a `FieldValue` is built from and read back as; `Mat4` fixes `Transform`'s parameter arity — one parameter slot per matrix column, derived from the matrix type itself — and `Mat4::transform_point` is what that operator *means*; `Epsilon::DEFAULT` is the length floor below which `Normalize` falls back to `+Y`. |
| `kernel` | `BinaryWriter`/`BinaryReader` and `SchemaVersion` are the canonical byte format; `StableHash` mints `FieldId` and parameter-slot identity from a name and labels the structural bytes; `Seconds` is the evaluation context's time input. |
| `noise` | `FbmConfig`'s knob set fixes the `Fbm` operator's parameter arity, pinned by an exhaustive destructuring so the operator and the noise layer can never drift apart; `value_noise` and `Fbm::sample` are what the `Noise` and `Fbm` operators *mean*. |

## The shape of the thing

```text
FieldBuilder  ──push/declare──>  FieldBuilder  ──build(output)──>  FieldGraph
                                                                     │
                        ┌────────────────────────────────────────────┤
                        │                                            │
                  RecipeGraph (the container)                  FieldParams
                  + output: NodeId                             (dense slot -> FieldValue)
```

* **`FieldType`** — exactly four types: `Scalar`, `Vec2`, `Vec3`, `Vec4`. There
  is no `Color` (a colour is a linear-RGBA `Vec4`), no `Mask`/`Bool` (a mask is
  a `Scalar` in `0..=1`; selection is `Mix`), and no `Coordinate` (a coordinate
  is a `Vec3`; its *space* is a property of the caller's `EvalContext`).
* **`FieldValue`** — a **tagged struct**, never a data-carrying enum: a
  `FieldType` tag plus four `Scalar` lanes, where every lane past the type's
  width holds a fixed documented default. Reading a wider accessor on a narrower
  value is *defined*, which is what makes the accessors branchless.
* **`FieldOp`** — a closed 23-operator algebra, `#[repr(u16)]`, discriminant ==
  table index. There is no registry, no runtime-extensible verb and no dynamic
  dispatch: **a new visual effect is a new graph, never a new Rust function.**
* **`FieldSignature` / `SIGNATURES`** — one `const` row per operator, in
  discriminant order, giving its arity, its parameter-word count, and the rule
  by which its output type is derived. `SignatureKind` is fieldless; the
  concrete type a fixed-output row yields rides in a separate field, again the
  tagged-struct discipline.
* **`FieldGraph`** — *wraps* a `RecipeGraph`. Acyclicity, the node budget, dense
  ids and the canonical node encoding come from the container for free. What
  this layer adds is the declared `output` node (a container has no notion of a
  *result*), the parameter table, and the meaning of the operator codes.

## Validation: one forward fold, and no second cycle check

`FieldGraph::validate` is a **single forward fold in node id order**. Because a
node's inputs may reference only strictly-earlier nodes, every input's derived
type is already known when the fold reaches a node, so one pass accumulating a
`Vec<FieldType>` indexed by node id is the whole type checker — no recursion
(`engine_no_recursion` is at 0), no worklist, no second pass.
`FieldGraph::type_of` is the same fold, read at one index.

**Cycles are not re-checked here.** `RecipeGraph::validate` already proves every
input id is strictly smaller than its node's index, and for an id-ordered append
graph that *is* the complete cycle argument. `validate` calls the container's
check first and lifts its diagnostic.

**Scalar-broadcasts-to-vector is the language's only implicit conversion.**
`Add(Vec3, Scalar)` is legal and yields `Vec3`; `Add(Vec3, Vec2)` is a
`TypeMismatch`.

Every rejection names the offending `NodeId`:

| Code | Condition |
|---|---|
| `NodeLimitExceeded` | lifted from the container |
| `CyclicInput` | lifted from the container |
| `MalformedData` | lifted from the container / decode |
| `UnknownType` | a `Const` or `Param` node, or a parameter slot, declares a type code that names no `FieldType` |
| `OutputNodeMissing` | a node id names no node — the declared output, or the id `type_of` was asked about |
| `UnknownOperator` | the operator code names no `FieldOp` |
| `WrongInputCount` | arity disagrees with the signature row |
| `WrongParamCount` | parameter-word count disagrees with the signature row |
| `TypeMismatch` | a width-generic operator whose non-scalar inputs disagree in width, **or** a `Param` node whose declared type is not the type its slot holds |
| `ComponentOutOfRange` | a `Component` lane index ≥ its input's width |
| `ComposeWidthInvalid` | a `Compose` width outside `2..=4`, or an input count ≠ that width |
| `UnknownParamSlot` | a `Param` node reads a slot the table does not have |
| `NonFiniteConstant` | a `Const` parameter word decodes to NaN or ±∞ |

`NonFiniteConstant` matters more than it looks: a NaN that enters a graph
propagates silently to every consumer, and `ScalarField::new` already refuses
such a value downstream. Reject it at the door.

`OutputNodeMissing` is deliberately **one** code covering both the decode-time
and the validation-time form of "this id names no node". Two codes for one
condition would make the stable numeric discriminant useless for the thing it
exists for.

## Evaluation: a flat fold over a fixed-size register file

`FieldGraph::evaluate(&ctx)` is **one forward fold in node id order** over a
`[FieldValue; MAX_NODES]` register array on the stack. Node ids are dense and
every input names a strictly-earlier node, so one pass has each input already
computed when it reaches a node, and `evaluate_at(ctx, n)` stops at `n` because
nothing later can contribute.

Four properties are load-bearing, and none is negotiable:

1. **Zero allocation per call.** `MAX_NODES` is 256 and a `FieldValue` is five
   words, so the register file is ~5 KB of stack and allocates nothing. This runs
   once per texel, per lattice node, per vertex: a 128×128 texture is 16,384
   evaluations. **This is the reason `field` does not build on `proc-core`**,
   whose evaluator allocates a `Vec` and mints an entropy stream per node per
   call — fine once per artifact, catastrophic once per texel.
2. **Inputs are read by index, never cloned.** The per-call cost is `O(nodes)`,
   not `O(nodes × inputs)`. (`ProcCore` clones a cached value per edge; that is
   the specific defect not inherited here.)
3. **A `const [fn; 23]` dispatch table**, indexed by the operator code — not a
   `match` (the Branchless Law) and not a generic closure parameter
   (the Axiom State Law bans an `F: Fn(..)` bound).
4. **Every operator is total.** A `FieldValue` in, a `FieldValue` out: no
   `Option`, no error. All rejection happened in `validate`; an evaluator that
   could fail at a point would put an error path in the innermost loop of every
   bake. Every remaining out-of-range read falls back to a documented default
   (`FieldValue::ZERO`), so even an unvalidated graph yields a value rather than
   a panic — the value is simply only *meaningful* for a graph that type-checks.

`evaluate` therefore does **not** re-validate. Validation is `O(nodes)` and
allocates a `Vec<FieldType>`; re-proving it per sample would be the whole cost of
a bake. The contract is: **validate once at preparation time, evaluate many
times.** Two `O(1)` preconditions are still checked per call, because they are
what keeps the register indexing in range: the queried node must name a node
(`OutputNodeMissing`), and the graph must fit the register file
(`NodeLimitExceeded` — the same number as the container's own budget, so only a
graph `validate` would already reject can trip it).

The evaluator needs **no type table**: every operator re-derives its own output
type from the same data the type checker used (the widest input, a declared type
word, a width word, or its signature row). That is what keeps the call
allocation-free, and it is why `FieldType::of_width` is stated once and read by
both the checker and `Compose`.

### Operator semantics — the contract every mirror is written against

| Op | Semantics |
|---|---|
| `Const` | the parameter words, typed by the declared `FieldType` |
| `Point` / `Uv` / `Normal` / `Time` | the corresponding `EvalContext` field |
| `Param` | `FieldParams[slot]`, the slot named by word 0 |
| `Add`/`Sub`/`Mul`/`Min`/`Max` | component-wise `+`, `-`, `*`, `f32::min`, `f32::max`; a `Scalar` input broadcasts |
| `Abs` | component-wise `f32::abs` |
| `Clamp(x, lo, hi)` | component-wise `max(min(x, hi), lo)` — `lo > hi` yields **`lo`**, documented, not undefined |
| `Mix(a, b, t)` | `a + (b - a) * t`, component-wise, **`t` unclamped** |
| `Smoothstep(e0, e1, x)` | `t = clamp((x - e0) / (e1 - e0), 0, 1); t*t*(3 - 2*t)`; `e0 == e1` yields `0` |
| `Dot` | scalar dot product over the inputs' common width |
| `Length` | `sqrt(dot(v, v))` |
| `Normalize` | `v * (1.0 / length(v))`, a `Vec3`; a length below `Epsilon::DEFAULT` yields **`+Y`** |
| `Compose(width)` | a vector of `width` lanes, the first lane of each input in slot order |
| `Component(i)` | lane `i` of the input, as a `Scalar` |
| `Noise(seed)` | `axiom_noise::value_noise(seed, point)` |
| `Fbm(seed, cfg…)` | `axiom_noise::Fbm::new(seed, cfg).sample(point)` |
| `Transform` | the `Mat4` whose four columns the parameter table holds, applied to the input as a **point** (`w = 1`) via `Mat4::transform_point` |

Two spellings are exact and not interchangeable with the algebraically equal ones
a mirror might reach for. `Mix` is `a + (b - a) * t`, **not** `a*(1-t) + b*t` —
they differ in the last `f32` bit. `Clamp` is `max(min(x, hi), lo)`, **not**
`min(max(x, lo), hi)` — they agree whenever `lo <= hi` and disagree exactly on the
degenerate node, and the documented answer there is `lo`.

`Normalize`'s order is likewise fixed: one reciprocal then three multiplies. It
is the algebra's only reciprocal, which is what makes CPU-to-CPU evaluation
bit-exact everywhere. Its `+Y` fallback is not invented here — it is the
deterministic default `crates/axiom-mesh-ops/src/implicit_surface.rs` already
uses where a sampled field's gradient vanishes.

`Noise` and `Fbm` read their words through `noise_words`, the **single**
statement of that wire format, which the authoring surface
(`FieldBuilder::push_noise` / `push_fbm`) writes and the evaluator reads. The
`Fbm` knob count is pinned there by an encoder that destructures an `FbmConfig`
**exhaustively**, so adding a knob to the config fails to *compile* — deliberately
replacing the earlier `size_of::<FbmConfig>() / size_of::<u32>()`, which was a
memory-layout coincidence standing in for a semantic parameter count.

### Floating-point determinism, stated precisely

* **CPU-to-CPU determinism is exact and required.** Same graph, same context →
  bit-identical `f32` on every target including `wasm32`. The algebra excludes
  transcendentals and division, `sqrt` is IEEE-754 exact, and the one reciprocal
  has its order fixed. `crates/axiom-field/tests/eval_golden.rs` commits a golden
  row per operator plus the composed reference case, asserted bit-exactly.
* **CPU-to-GPU parity is a tolerance, not an equality.** GPUs are permitted wider
  intermediates and a lower-precision `inversesqrt`. A backend pins parity with a
  sampled-grid test at a documented tolerance — never with byte equality.

### Capability annotations — not yet, and here is the line

Every one of the 23 operators is implementable on the CPU and in WGSL. **No
operator needs a `RenderCapability` annotation today**, which is the strongest
argument that the algebra is correctly sized. The moment an operator is proposed
that one backend cannot express (a texture sample, a screen-space derivative), it
would need a capability bit and a validation step — and that is the signal to
**reject the operator**, not to add the annotation.

## The canonical form

`FieldGraph::canonicalize` runs four passes, in this order:

1. **Constant folding** — a node all of whose inputs are known constants becomes
   a `Const` carrying **the value the evaluator would compute for it**. The
   folder holds no arithmetic of its own: it answers "may this operator be
   evaluated ahead of time?" and then calls the evaluator's own dispatch table. A
   second implementation of `Mix` or `Clamp` there would be a second definition of
   what the language means, and the two would eventually differ in a last bit and
   make `canonicalize` silently change what a graph computes. The unfoldable set
   is exactly the operators whose value is not a function of the node alone:
   `Point`/`Uv`/`Normal`/`Time` have no value until evaluation, and `Param` and
   `Transform` read the *parameter table*, so folding them would move a value into
   structure and start moving the digest. `Noise` and `Fbm` **are** folded — they
   are pure in `(seed, config, point)`, and the CPU evaluator is now the semantic
   reference for what every backend must compute. A fold whose result is not
   finite is refused, so a node that overflows stays a node and overflows
   identically wherever it is evaluated; a *documented degenerate* (equal
   `Smoothstep` edges, a zero-length `Normalize`, an inverted `Clamp`) is not
   degenerate at all — it folds to the total value its rule gives it.
2. **Common-subexpression elimination** — nodes are keyed by
   `(op, params, canonical input ids)` and the first node with a key is reused.
   `Add`, `Mul`, `Min` and `Max` sort their input ids first, so `a+b` and `b+a`
   are one node.
3. **Dead-node elimination** — nodes the output cannot reach are dropped,
   computed as a **reverse fold over the id-ordered node list**. Every input id is
   strictly smaller than its node's id, so one descending pass is complete. The
   obvious recursive walk is banned and would be worse anyway.
4. **Deterministic relabelling** — the survivors are emitted in ascending
   original id order into a fresh dense `0..n`. That order is already a valid
   topological order, so no sort is involved and no tie-break rule can drift.

Passes 1 and 2 are one forward walk. `canonicalize` is a **pure function** —
nothing is memoised, because a cache is retained state — and it is idempotent.

**What canonicalisation deliberately does not do:** algebraic rewriting
(`x*1 -> x`, `x+0 -> x`), reassociation, strength reduction, or any transform
whose result differs in the last `f32` bit. Those would break the CPU/GPU parity
contract the backend lowering depends on. `mul_add` is likewise not used: a fused
multiply-add rounds once where a shader rounds twice.

**Where the normalisation stops.** Pass 4 normalises *within* the authoring's
topological order; it does not re-sort independent nodes into a canonical one. So
two graphs that differ only by dead nodes, duplicated subexpressions, foldable
constants and commuted operands canonicalise to identical bytes — the case that
matters — but two graphs that genuinely interleave independent subtrees in
different orders still can differ. Fixing that would need a content-keyed
topological sort, which is a deliberate future decision, not an accident here.

**CSE's key map is a `BTreeMap`,** not a hash map: ordering is by the key's own
bytes, so nothing depends on a hasher. **Canonicalisation is a preparation-time
operation. Never call it from a frame path.**

**The parameter table is never touched.** Dead-node elimination may drop the last
`Param` node reading a slot; the slot stays. Shrinking the table would move the
digest for a reason that is not structural, which is exactly what the table
exists to prevent.

## Sharing is free

A `NodeId` may appear in any number of later nodes' inputs — that **is** the
DAG's sharing, inherited from the container. There is no separate "reuse"
concept, no reference counting and no subgraph type. A reusable material
function is a `FieldGraph` a builder inlines.

## Parameters, and the one performance property to protect

`FieldOp::Param` reads slot *n*. **Changing a parameter's value does not change
the graph's structure, therefore does not change `FieldGraph::digest`, therefore
cannot cause a downstream program recompile.**

That is why `digest()` folds the schema stamp, the whole recipe, the output id,
and each slot's *declared type* — but **not** the slot values. `serialize()`
carries the whole state (values included) so a field round-trips exactly;
`digest()` is the *structural* label a program cache keys on. Retyping a slot
does move the digest, because a type change really is a different program.

Two properties must survive every future change here, because losing either
makes the obvious optimisations impossible:

1. **Node ids are dense and id-ordered**, so the evaluator *is* a flat fold over
   an indexed register file — never a recursive descent.
2. **Parameters are separate from structure**, so a value change never touches
   the digest.

## Determinism

Same graph → same bytes → same digest, on every target including `wasm32`.
Nothing depends on an address, an iteration order, or a `TypeId`. There is no
`f64` (the kernel's `BinaryWriter` has no `write_f64`, which enforces it), no
ambient time (the context's `Seconds` is handed in), and **no randomness**: the
only stochastic-looking operators, `Noise` and `Fbm`, are pure functions of
`(seed, point)` where the seed is a graph parameter.

**The bytes are the determinism proof; the digest is the label.** That is the
kernel's stated stance for `StableHash` and it is repeated here so nobody treats
a hash match as the verdict — byte equality is.

Decode is bounds-checked and never panics: a buffer truncated at *any* prefix
length fails cleanly, an unrecognised parameter type code fails, a container
that is over budget or cyclic fails with the container's own diagnostic, and an
output id naming no node fails naming that id.

## Deliberate exclusions

**Not operators**, and why:

| Excluded | Reason |
|---|---|
| `Div` | Division by zero is a determinism hazard and a NaN source. Multiply by a constant reciprocal, or add a guarded op later with an explicit fallback. |
| `Pow`, `Exp`, `Log`, `Sin`, `Cos` | Transcendentals differ between CPU and GPU `f32` by more than the parity tolerance, and nothing needs one yet. |
| `Step` | `Smoothstep` with equal edges. |
| `Cross` | Expressible with `Compose` plus arithmetic. |
| `dpdx` / `dpdy` | Screen-space derivatives are backend-specific, absent on the CPU, and the cause of a real past defect. |
| `If` / `Select` / `Compare` | Selection is `Mix`. A comparison operator is the seed of control flow in a language that must stay branchless end to end. |
| `Texture` / `Sample` | A texture is a rendering resource; sampling one from a field is a later, separate decision with real capability consequences. |
| marble, wood, rust, dirt, asphalt | Library graphs built from the 23 operators, not engine primitives. |

**Not dependencies:** `entropy` and `space`, deliberately — there is no
randomness to seed and no spatial index to consult.

## Growth budget

`FieldOp` has 23 variants against the `engine_no_large_enums` cap of 24, so
there is exactly **one** spare slot. Do not spend it casually. A 25th operator
means moving the discriminant to a bare `u16` code with a `const` catalog — the
`axiom-recipe` shape.
