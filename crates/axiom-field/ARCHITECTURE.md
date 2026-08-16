# `axiom-field` — the typed pointwise field IR

> A **field** is a deterministic, hashable, canonically-serializable pure
> function from an explicitly supplied typed evaluation context to a typed
> value, represented as a closed-algebra, id-ordered, acyclic typed expression
> graph.

This crate owns the **representation**. It carries no evaluator, no
canonicalisation pass and no type checker; those land separately, on top of the
vocabulary and the bytes fixed here.

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
| `math` | `Vec2`/`Vec3`/`Vec4` are the lanes a `FieldValue` is built from and read back as, and `Mat4` fixes `Transform`'s parameter arity — one parameter slot per matrix column, derived from the matrix type itself. |
| `kernel` | `BinaryWriter`/`BinaryReader` and `SchemaVersion` are the canonical byte format; `StableHash` mints `FieldId` and parameter-slot identity from a name and labels the structural bytes; `Seconds` is the evaluation context's time input. |
| `noise` | `FbmConfig`'s knob set fixes the `Fbm` operator's parameter arity in the signature table, derived from the config type so the operator and the noise layer it will lower to can never drift apart. |

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

1. **Node ids are dense and id-ordered**, so an evaluator can be a flat fold
   over an indexed register file — never a recursive descent.
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
