# `axiom-surface` — the neutral appearance artifact

> A **surface** is a closed record of seven named shading channels, each bound to
> a constant value or to a field expression evaluated in object space, plus a
> lighting-model discriminant and bounded mask-driven layering that flattens into
> one field graph per channel.

This crate owns the **channel vocabulary**, the **layering algebra**, the
**lighting discriminant**, the **capability contract** a backend checks before it
lowers anything, and the **canonical bytes and structural digest** that give a
surface a stable program identity. It is where "roughness" is allowed to exist
for the first time.

## Placement

`Layer: surface` — `crates/axiom-surface`, `depends_on = ["kernel", "math",
"field", "host"]`.

It is a **layer, not a module**, and that is forced rather than preferred: seven
engine *modules* must be able to name a material description — `resources`,
`render`, `render-pipeline`, `gpu-backend`, `canvas2d-backend`, `assets` and the
`axiom` facade — and a module may never depend on another module. This is the
`axiom-mesh` precedent verbatim: *seven engine modules need to name triangle
geometry*, so the neutral triangle mesh is a layer.

**Why not in `axiom-host`.** `host` is the flattened presentation boundary —
`frame_packet.rs` states it carries *"only primitives — no GPU, browser, DOM,
render-module, or scene types"* — consumed by backends after all authoring is
resolved. A graph-bearing authoring type inverts that role, and `host` is
additionally the one platform-facing layer whose own `ARCHITECTURE.md` rules a
shader compiler out of it. `Surface` is upstream of the flattening.

**Why not in `axiom-field`.** Channel names *are* rendering semantics. Putting
`roughness` in the generic expression primitive would make `mesh-ops` — which
needs fields for implicit surfaces and has nothing to do with rendering — depend
on a crate that knows about emission.

## What each dependency is for

| Layer | What is genuinely adapted |
|---|---|
| `field` | The expression language. `FieldGraph` is what a channel binds to, `FieldValue`/`FieldType` are the values and the type lattice a channel declares, `FieldBuilder`/`FieldOp` are how layer flattening *composes* graphs, `EvalContext` is what a constant-folded composition is read against, and `FieldGraph::digest` is the half of a surface's digest that already excludes parameter values. |
| `math` | `Vec3`/`Vec4` are the lanes a channel constant carries, and `Vec3` is what the four finite-difference offsets of the height-to-normal derivation are expressed in. |
| `kernel` | `BinaryWriter`/`BinaryReader` and `SchemaVersion` are the canonical byte format; `StableHash` is the structural digest; `Meters` and `Ratio` are the quantities `normal_from_height` is authored in, so no naked float reaches the boundary. |
| `host` | `BackendCapabilityProfile` and `RenderCapability` are what `supported_by` checks a surface's derived requirements against — the pure "will this render there?" query. |

### `recipe` was a dependency, and is no longer — because `field` fixed it

This layer used to declare `recipe`, and the reason was honest: **`field`'s public
API traffics in `recipe`'s value types.** `FieldValue::scalar` takes a `Scalar`,
`FieldBuilder::push` takes `Vec<Param>` and `Vec<NodeId>`, and `FieldGraph::output`
hands a `NodeId` back. A layer that composes field graphs — which is exactly what
layer flattening is — cannot avoid naming them, and naming them only through type
inference to dodge the text scan would have been a dependency satisfied by
coincidence, which `CLAUDE.md` names as the thing not to do.

The edge was real. It was also a symptom: **it existed because `field` leaked its
substrate.** The fix belonged at the lowest correct layer, and it landed there —
`axiom-field` now re-exports `NodeId`, `Param`, `Scalar` and `MAX_NODES`, so this
layer names `axiom_field::NodeId` and the edge simply stopped being real. Nothing
changed about the types: `axiom_field::NodeId` **is** `axiom_recipe::NodeId`.

The same fix removed the `kernel` edge from `axiom-proc-texture` and
`axiom-proc-mesh`: both declared it solely to write `Seconds::finite_or_zero(0.0)`
for a bake, and `EvalContext::at` now states that convention once, in `field`.
This layer keeps `kernel`, which it genuinely uses for the byte format, the
digest and the `Meters`/`Ratio` quantities.

### `host` is a dependency, and the plan did not list it either

`supported_by(reqs, profile)` answers *"will a backend with this capability
profile render this surface, or fall back to its constants?"* — as a pure query,
with no device, no program and no frame. The profile type is
`axiom_host::BackendCapabilityProfile`, so the edge is real and non-ceremonial.

It is acyclic and precedented: `host` depends only on `kernel`, `runtime` and
`math`, and `frame` and `layout` already build on it. The alternative — restating
which capability bit a procedural surface occupies — would have been a second,
drifting definition of a backend contract this layer does not own.

**What the query is not.** It answers the *capability* half and only that. A
backend's own gate additionally checks ceilings that are properties of that
backend: how many parameters its shared uniform region holds, which interstage
lanes its main pass carries, its shader node budget, and which vertex stage a
particular draw uses. None of those are derivable from a backend-neutral
requirements summary. So a `false` is final, and a `true` means the surface
clears the capability gate with the backend's own ceilings still to come.

## The shape of the thing

```text
SurfaceBuilder  ──bind/layer──>  SurfaceBuilder  ──build──>  Surface
                                                                │
                        ┌───────────────────────────────────────┼─────────────┐
                        │                                       │             │
                  [ChannelBinding; 7]                   LightingModel    Vec<SurfaceLayer>
                  (constant | FieldGraph)                                (surface, mask, blend)
```

* **`SurfaceChannel`** — exactly seven, `#[repr(u16)]`, discriminant == table
  index: `BaseColor`, `Roughness`, `Metallic`, `Normal`, `Emission`, `Opacity`,
  `Displacement`. Each declares the `FieldType` its values must carry and the
  default it holds when nobody binds it. **This closedness is the whole point**:
  the model is fixed in Rust and parameterised by data, never extended at
  runtime.
* **`ChannelBinding`** — a **tagged struct**, never a data-carrying enum: a
  `kind` code plus a `FieldValue` and a `FieldGraph`, of which the kind selects
  one. The `RenderCommand` precedent in `modules/axiom-render`, for the same
  reason: a data-carrying variant would force a `match` on read.
* **`Surface`** — the artifact. Every `Surface` value is legal, because both
  constructors (`SurfaceBuilder::build` and `Surface::deserialize`) validate.
* **`SurfaceLayer` / `LayerBlend`** — one whole surface, a scalar mask, and one
  of three blends. Bounded at `MAX_LAYERS = 4` over the *whole tree*.
* **`LightingModel`** — `Unlit` / `Lambert` / `LambertSpecular`, with
  `LambertSpecular` the default because it is what the engine's one lit shader
  already computes. This type changes no pixel on its own.

## The coordinate convention — fixed here, once

**A surface's channel graphs are evaluated with `EvalContext::point` in OBJECT
space.**

A world-space pattern swims when the object moves; object space is what makes a
boulder's noise ride with the boulder. Every downstream consumer — the backend
lowering and the WGSL emitter both — depends on this being stated in exactly one
place, and this is that place.

Two consequences follow for free:

* **Triplanar projection is authorable, not a primitive.** Three samples of the
  same pattern blended by `Abs(Normal)` weights is an ordinary graph over the
  existing operators. No new operator, no new channel, no capability bit.
* **`normal_from_height` differences in object space too.** A height authored
  over `Uv` alone has no object-space gradient, so it derives a *flat* normal
  — correctly, and the test `a_height_with_no_object_space_gradient_yields_a_flat_normal`
  pins that rather than leaving it folklore. A height meant to bump a surface is
  authored over `Point`.

## Layering: an algebra, and one graph per channel

Each blend is stated once, on `LayerBlend`, as the exact field expression the
flattener builds — `under` is the accumulated value, `over` is the layer's value,
`mask` is the layer's scalar mask:

| Blend | Expression |
|---|---|
| `Over` | `Mix(under, over, mask)` |
| `Add` | `Add(under, Mul(over, mask))` |
| `Multiply` | `Mix(under, Mul(under, over), mask)` |

The spellings are exact and **not** interchangeable with the algebraically equal
ones a mirror might reach for: `field`'s `Mix` is `a + (b - a) * t`, so a masked
multiply written `under * (1 + (over - 1) * mask)` would differ in the last `f32`
bit and break CPU/GPU parity.

That is what makes `painted_metal = metal_base + paint(mask) + scratches(mask) +
dirt(mask)` expressible **without any new primitive**: each layer is a `Surface`,
each mask is a scalar field, and the flattening is `Mix` nodes.

### Flattening is iterative, and the bound is the budget

`Surface` is a recursive value type — it holds layers, and every layer holds a
surface. **That does not license a recursive walk**, and `engine_no_recursion`
sits at zero. Everything that has to see the whole tree — validation, the
requirements summary, the canonical bytes, the flattener — reads it through one
bounded, iterative linearisation in `layer_tree.rs`:

* The walk is a **fold over `0..=MAX_LAYERS`**, expanding one already-discovered
  surface's children per step. That is iterative *and* terminating without a
  `while`, and the bound **is** the layer budget: a tree within budget is fully
  discovered, and a tree over budget comes back longer than the bound, which is
  exactly how `Surface::validate` detects it and reports
  `SurfaceErrorCode::LayerBudgetExceeded`. Never a silent truncation.
* The order is **breadth-first**, so a parent's index is always strictly smaller
  than its children's. The flattener then folds the list **in reverse** and finds
  every child already resolved — one pass, no worklist that can grow, no
  tie-break rule that can drift. The byte reader rebuilds the tree the same way.

That strictly-earlier rule is the same one `axiom-recipe` proves for node inputs,
and it is reused here for the same reason: it turns a graph walk into a fold.

The three blend expressions are all **emitted**, and the blend selects the output
node by table index; the two unused ones are dropped by `field`'s own dead-node
elimination rather than by a branch. A composition whose inputs are all constants
folds back to a **constant** binding, so flattening an all-constant surface does
not manufacture graphs a backend would then have to lower.

`MAX_LAYERS = 4` is not arbitrary: a layered surface flattens into one graph per
channel and `axiom_recipe::MAX_NODES = 256` is the real budget that graph must
fit. An over-eager author hits the cap, which is the correct loud failure. **Do
not raise either cap to make a scene fit.**

## The digest, and the property it exists to protect

`Surface::digest()` folds the schema stamp, the tree shape, every blend and
lighting model, every binding's kind and constant, and every bound graph's own
`FieldGraph::digest` — which deliberately **excludes that graph's parameter
values** and **includes each slot's declared type**.

So:

* **Retuning a parameter does not move a surface's digest.** A material tweak
  cannot invalidate a compiled program, and animating a parameter cannot explode
  into variants. This is the property that makes the digest a safe program-cache
  key, and `retuning_a_parameter_leaves_the_layered_digest_alone` is the
  load-bearing test.
* **Retyping a slot does move it**, because a type change really is a different
  program.
* **A channel bound to a *constant* is structure**, exactly as a `Const` node is
  in `field` — changing it moves the digest. To retune a channel without moving
  the digest, bind it to a one-node `Param` field. This is stated on
  `Surface::digest` so nobody has to infer it.

**The bytes are the determinism proof; the digest is the label.** That is the
kernel's stated stance for `StableHash` and it holds here.

## `SurfaceRequirements` — the backend-neutral half of the rejected shader IR

```rust
SurfaceRequirements {
    inputs: SurfaceInput,   // Point | Uv | Normal | Time
    varying_channels: u16,  // bitset over SurfaceChannel
    has_displacement: bool,
    param_count: u16,
    node_count: u16,
}
```

Derived by walking the bound graphs — never authored, and never stale, because
there is nowhere to store a stale copy. It is what a backend checks against its
capability profile **before attempting to lower anything**.

`inputs` is **exact**: it is read off the four context-source operator codes, so
a surface reading only `Uv` does not claim `Point`. `varying_channels` follows
one rule, stated once: a channel varies when some surface in the tree binds it to
a field, **or** when some layer's mask is a field — because every blend rule
makes each channel a function of the mask. That second clause is exact for `Add`
and conservative for `Over`/`Multiply` in the one case where the composed
constants happen to agree; a backend is never told a channel is constant when it
is not.

There is deliberately **no third IR stratum** between this and the backend. The
repository has one lit shader, no variant machinery, and a second backend that
cannot execute a program at all. The backend-shaped half of a shader IR — stage
assignment, varyings, bind-group indices, uniform packing — is inherently
backend-shaped and belongs beside the emitter.

## Displacement is a general field consumer, not a material concept

Read this before concluding that a GPU executing something makes it an appearance
feature. **A displacement is a `Vec3` field of position and time; it has no more
to do with materials than a heightfield does**, and the engine already proves it:
`axiom_proc_mesh`'s `MeshOp::Displace` is bake-time deformation with no material
anywhere near it, and `axiom_mesh_ops` transforms geometry with no material
either.

`Surface` carries a `Displacement` channel **only because that is the binding
site for the vertex stage of the program its fragment channels already compile
into**. It is a wiring convenience — one authored artifact, one digest, one
pipeline — and *not* a claim that deformation is appearance.

This note was written where the vertex emitter lives, because this layer was out
of that manifest's scope. It belongs here, where the channel is declared, and
this is now its home.

## Reading a surface: `inspect`, and the diagnostics that name a node

`Surface::inspect()` is the agent-facing read: per channel, what it is bound to,
the type it produces, how large the bound graph is, how many knobs it carries,
and that graph's own structural digest — plus the lighting model, the whole
tree's layer count, the derived requirements and the surface's digest. From a
channel an agent drills into the `FieldGraph` itself, where `axiom_field`'s own
`describe`/`explain`/`dependents_of`/`diff` take over. This layer adds the
channel vocabulary and nothing else.

Only the **root** surface's channels are reported. A layer's own channels are
read by inspecting that layer's surface; the resolved single binding per channel
is `flatten()` followed by `inspect()`.

**Every failure names both the channel and the node.** `SurfaceError` carries a
`SurfaceChannel`, a layer index within the linearised tree, and the `NodeId` of
the field graph the failure concerns — lifted one-for-one from the field layer's
own diagnostic, wording and stable code included. That is the brief's
*"diagnostics pointing to semantic graph nodes rather than generated WGSL lines"*,
satisfied end to end: an agent is told *"opacity, node 0, `Dot` has no inputs"*,
never *"line 214 of a shader you did not write."*

## Normal from height — a derivation, not a derivative operator

```rust
SurfaceBuilder::normal_from_height(self, height: FieldGraph, offset: Meters, strength: Ratio)
```

The height graph is inlined **four times**, each read against a sample point
displaced by `offset` along `+x`, `-x`, `+y`, `-y`, and the four samples are
composed into `normalize(vec3(-dx * strength, -dy * strength, 2 * offset))`.
Scaling the `z` lane by `2 * offset` is what divides the differences by their own
step **without a division** — the field algebra deliberately has none.

The substitution is the mechanism: inlining rewrites what the inlined graph reads
as its `Point`, so "the same height at a different place" is the same graph read
against a different node, not a new operator.

**There is no screen-space derivative operator in the algebra, deliberately.**
`dpdx`/`dpdy` are backend-specific, absent on the CPU and on a software
rasterizer, and already the cause of a real mobile-GPU NaN defect in this engine.
A finite difference at an offset the author chose is expressible everywhere and
reproducible bit-for-bit. A zero offset leaves a degenerate vector, which
`field`'s `Normalize` resolves to its documented `+Y` fallback rather than a NaN.

The cost is honest and bounded: four copies of the height graph. A height wide
enough that four copies overflow `MAX_NODES` is rejected with the field layer's
own `NodeLimitExceeded`, not truncated.

## Lifetime: preparation-time data

A `Surface` holds field graphs and a `Vec`. It is **preparation-time data**,
addressed by identity afterwards, and it must **never be cloned per frame** —
the same rule that made per-frame mesh geometry churn a real measured defect in
this engine. Flattening, canonicalisation and digesting all belong at the
preparation barrier.

## Deliberate exclusions

| Excluded | Reason |
|---|---|
| PBR | `Metallic` is a **channel**, not a BRDF: carried, digested, reported, and read by no lighting model. `SPEC-11`'s *"Resist PBR scope creep"* still binds. |
| transmission, subsurface, clear-coat, anisotropy | A channel nothing can render is debt, not capability — the failure that got a previous attempt reverted. Opacity is here because alpha is real; transmission is not. |
| a texture-sampling channel | A surface binds **fields**, not images. One of the engine's two backends cannot sample at all, so this is a later, separate decision with real capability consequences. |
| WGSL, stages, bindings, varyings, pipelines, program caches | Backend concepts. They live beside the emitter, in the module that already owns every shader string in the engine. |
| decals, environment mapping, post-processing graphs | Scene- and frame-level concerns, not surface-level ones. |
| a per-layer lighting model | Flattening keeps the **root's** model: a layer contributes channel values, and one draw has one lighting model. |

## Growth budget

`SurfaceChannel` has 7 variants, `LayerBlend` 3, `LightingModel` 3,
`SurfaceErrorCode` 10 — all far under the 24-variant cap. The budget that is
actually tight is `MAX_LAYERS × MAX_NODES`: every channel of every layer is
inlined into one graph. Adding an eighth channel costs a `const` row in four
tables and a wire-format bump; adding a fifth layer costs 256 nodes of headroom
that do not exist. Spend the first cautiously and the second not at all.
