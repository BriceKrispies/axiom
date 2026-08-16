# 05 — Bake-time field consumers

> **This manifest replaces the requested `05-render-shader-ir.md`.** The evidence
> rejects that stratum — see `00-architecture-findings.md` §3C and `README.md` §3.
> This is the work that proves the primitive **without touching the renderer at
> all**, and it is where the field earns its place even if every later manifest
> stalled.

## Objective

Wire `axiom-field` into the three layers that already need it and cannot express
it today:

1. **`crates/axiom-mesh-ops`** — give `ScalarField` a constructor that samples a
   field, closing the gap the layer's own documentation names and the `454707c0`
   revert left open.
2. **`crates/axiom-proc-texture`** — add one operator that bakes a field to an
   RGBA8 buffer, so a texture recipe can carry an expression instead of eleven
   fixed generators.
3. **`crates/axiom-proc-mesh`** — retarget `MeshOp::Displace` at a field, and
   delete the private, non-composable field expression hidden inside
   `MeshOp::MetaSurface`.

## Architectural placement

Three existing **layers**, each gaining `field` in `depends_on`. No new package.

## Existing code involved

| Path | What it is today |
|---|---|
| `crates/axiom-mesh-ops/src/implicit_surface.rs:79` | `ScalarField { values: Vec<f32>, cols, rows, depth }` — validated, finite-checked, **and constructed nowhere but its own tests** |
| `crates/axiom-mesh-ops/src/implicit_surface.rs:10-14` | the doc that states the missing primitive in the negative |
| `crates/axiom-mesh-ops/src/implicit_surface.rs:287` | `implicit_surface_mesh(field, iso, options)` — the consumer waiting for a producer |
| `crates/axiom-proc-texture/src/{texture_op,dispatch,generators,filters}.rs` | 11 ops, `const OPS: [TexOp; 11]`, `TextureBuffer` |
| `crates/axiom-proc-mesh/src/implicit.rs:41-72` | `capsule_sdf`, `smin`, `field`, `field_normal` — **a private field expression with no type to live in** |
| `crates/axiom-proc-mesh/src/implicit.rs:82` | `parse` — the `[iso, res, k, 7×N]` flattened capsule encoding |
| `crates/axiom-proc-mesh/src/transforms.rs` | `MeshOp::Displace` — `value_noise(seed, pos)` along the normal, hardcoded |
| `crates/axiom-proc-mesh/src/mc_tables.rs` (301 lines) | marching-cubes tables **duplicated** from `mesh-ops`' copy |

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-mesh-ops/src/implicit_surface.rs` | modify — add `ScalarField::sample` |
| `crates/axiom-mesh-ops/{Cargo.toml, layer.toml, ARCHITECTURE.md}` | modify — add `field` |
| `crates/axiom-proc-texture/src/texture_op.rs` | modify — add `TextureOp::Field = 11` |
| `crates/axiom-proc-texture/src/dispatch.rs` | modify — table 11 → 12 entries |
| `crates/axiom-proc-texture/src/field_source.rs` | create — the new operator |
| `crates/axiom-proc-texture/{Cargo.toml, layer.toml}` | modify |
| `crates/axiom-proc-mesh/src/transforms.rs` | modify — `Displace` by a field |
| `crates/axiom-proc-mesh/src/implicit.rs` | modify — delete the private field expression |
| `crates/axiom-proc-mesh/{Cargo.toml, layer.toml}` | modify |

## Dependencies on earlier manifests

**`03`** (the evaluator). **`P1`** — do not add a `field` dependency to a layer
while the legacy `proc` stack is still being untangled underneath it.

Parallel with `04`. No file overlap.

## Public API / data contracts

### 1. `ScalarField::sample` — the headline

```rust
impl ScalarField {
    pub fn sample(
        graph: &FieldGraph,
        origin: Vec3,
        spacing: Meters,
        cols: u32, rows: u32, depth: u32,
    ) -> MeshResult<ScalarField>;
}
```

Evaluates the graph at each lattice node with `EvalContext::point = origin + (x,
y, z) * spacing`, keeping the layer's existing X-fastest-then-Y-then-Z layout and
its existing finite-value validation.

**This is the fix the layer asked for in prose.** Its doc says a callback is
banned because *"a callback is an opaque capability that could read a clock or a
global, which would make the operator unreplayable. A sampled lattice is a value
— hashable, diffable, and reproducible."* A `FieldGraph` is **also** a value —
hashable, diffable, reproducible, and with no capability to read a clock, because
`EvalContext::time` is supplied by the caller. It satisfies the layer's stated
requirement exactly, which is why this is the correct fix and a callback is not.

Note the existing gradient convention at `implicit_surface.rs:22-32` — values
rise going outward, so the gradient is the outward normal — and document that a
field intended for this consumer should be signed-distance-like.

### 2. `TextureOp::Field = 11`

Params: `[width, height, graph_byte_count, ...packed graph bytes...]`, or
— preferred — `[width, height, field_id]` where the graph is supplied alongside
the recipe. **Choose the second.** Reason: `MAX_NODES = 256` and a `Param` is one
`u32` word; inlining graph bytes into recipe params would blow the node budget
and make the recipe unreadable. This means `ProcTextureApi::bake` gains an
overload taking a field table:

```rust
impl ProcTextureApi {
    pub fn bake_with_fields(
        &self, recipe: &RecipeGraph, seed: u64, fields: &[FieldGraph],
    ) -> ProcResult<TextureBuffer>;
}
```

The operator evaluates the field once per texel with
`EvalContext::uv = (x + 0.5) / width, (y + 0.5) / height` (texel centres — the
existing generators' convention) and `point = (uv.x, uv.y, 0)`. Output type
mapping: a `Vec4` field writes RGBA directly; a `Scalar` field writes greyscale
with alpha 255. Any other output type is `ProcError::OpFailed`.

**Do not delete the eleven existing operators.** They ship, they are covered, two
apps depend on them, and `Blur` is a neighbourhood operator a pointwise field
genuinely cannot express. `Field` joins them; it does not replace them.

### 3. `MeshOp::Displace` by a field

Today `Displace` is `value_noise(seed, pos)` along the normal, hardcoded. Retarget
it at a field graph supplied through the same field-table mechanism, evaluated
with `EvalContext::point = vertex position`, `normal = vertex normal`. Keep the
existing noise behaviour reachable as the equivalent two-node graph so no
existing recipe changes meaning — and pin that with a test asserting the old and
new paths produce byte-identical geometry.

### 4. Delete the hidden field expression in `implicit.rs`

`capsule_sdf`/`smin`/`field`/`field_normal` and the `[iso, res, k, 7×N]` capsule
encoding are a field expression written in Rust because there was no type for it.
With `ScalarField::sample` and marching cubes both available in `mesh-ops`,
`MeshOp::MetaSurface` becomes: sample a field → `implicit_surface_mesh`.

**Consequence, and it is a good one:** `crates/axiom-proc-mesh/src/mc_tables.rs`
(301 lines, duplicated from `mesh-ops`' 305-line copy, with the duplication
admitted in its own header) becomes deletable.

> **OUTCOME — this paragraph was wrong, and the rewrite was refused.** The
> implementing agent verified the `smin` question empirically first and found it
> is *not* the blocker: a 3-capsule skeleton built entirely from the 23 ops
> matches the reference to `2.4e-7` over a 12³ probe grid in 73 nodes. The
> rewrite fails on four other grounds. (1) **Auto-skinning cannot be a field at
> all** — `implicit.rs:197 skin_of` emits per-vertex `[u16;4]` joints and
> `[f32;4]` weights from `exp(-capsule_sdf/k)` *per capsule*, and a `FieldGraph`
> yields one ≤4-lane value with no notion of which capsule; `exp` is excluded
> from the algebra by design. (2) **The node budget caps the skeleton at 11
> capsules** (7 shared + 22 each against `MAX_NODES = 256`), where MetaSurface
> accepts any count today. (3) **Normals would change** — MetaSurface central-
> differences the *continuous* field at `GRAD_H = 1e-3`, `implicit_surface_mesh`
> interpolates the *lattice* gradient along a cell edge, so every existing
> golden would move. (4) **The output contracts differ** — `implicit_surface_mesh`
> welds and returns `axiom_mesh::Mesh`, MetaSurface must return an unwelded
> skinned `MeshBuffer`, so proc-mesh keeps its own emission path and
> `mc_tables.rs` **does not** become deletable. `MetaSurface` and `mc_tables.rs`
> are retained deliberately. Wirings 1–3 are the complete scope of this manifest. `docs/mesh-convergence-migration.md`
already lists this exact consolidation as item #1 and records that *"nothing in
this document has been done."* Doing it here is the No-Shortcuts fix.

**Scope control:** a smooth-min (`smin`) is not in the 23-op algebra. Express it
as `Mix` + `Clamp` + arithmetic (the polynomial smooth-min is
`mix(b, a, h) - k*h*(1-h)` with `h = clamp(0.5 + 0.5*(b-a)/k, 0, 1)`, which is
exactly `Smoothstep`-free and expressible). Verify this reproduces the existing
metaball output within the parity tolerance **before** deleting anything; if it
does not, stop and report rather than adding an `Smin` operator locally.

## Explicitly excluded

* No renderer changes. No `Surface`. No WGSL. Nothing in `modules/`.
* No new texture operators beyond `Field`.
* No removal of `TextureOp` 0–10 or `MeshOp` 0–11 other than the `MetaSurface`
  internals.
* Do not raise `MAX_NODES`, `MAX_DIM` (512) or `MAX_VERTS`.

## Determinism requirements

* Field-baked output must be byte-identical across runs and targets.
* The `Displace` equivalence test above is a determinism test, not a convenience.
* `tools/axiom-proc-fuzz`'s 2,000-seed byte-identity sweep must still pass.

## Serialization requirements

The field table travels alongside the recipe, not inside it. If a persisted form
is needed, it is `u32` count + each `FieldGraph`'s canonical bytes, length-
prefixed — the `write_byte_slice` shape `examples/recipes/generated_micro_fps/src/pack.rs`
already uses.

## Testing requirements (100%)

* `ScalarField::sample` over a known analytic field (a sphere SDF) reproduces
  hand-computed values at named lattice nodes.
* A sampled sphere field through `implicit_surface_mesh` produces a mesh whose
  vertex count and bounds match a committed golden.
* `TextureOp::Field` baking a gradient×fbm graph matches a committed golden
  `TextureBuffer` digest.
* Wrong output type → `OpFailed`.
* `Displace`-by-field reproduces the old hardcoded `Displace` byte-for-byte.
* `MetaSurface` after the rewrite reproduces its existing golden.
* Every touched layer returns to 100% coverage.

## Architecture tests

* `cargo xtask check-architecture` — three `layer.toml` files gain `field`;
  `UnknownDependency` and `ProofReferenceMissing` fire immediately on a mistake.
* `engine_genuine_dependency` must see a real `axiom_field::` reference in each.
* If `mc_tables.rs` is deleted, `engine_no_large_files` improves; confirm no lint
  count rises.

## Performance risks

* **This is the first place field evaluation runs at scale.** A 512×512 texture
  is 262,144 evaluations; a 64³ lattice is 262,144. The `03` register-file design
  is what makes that viable — verify no allocation per sample by inspection.
* `ProcCore` clones a full `TextureBuffer` per graph edge
  (`proc_core.rs:54`). A `Field` node produces one buffer, so it adds one clone,
  not one per node — but note it, because a graph of many `Field` nodes is the
  pathological case.
* Baking happens in a preparation task, where there is no frame budget. Do not
  optimise speculatively; measure against burnt-rubber's existing
  `PreparedTextures::generate()` cost.

## Migration considerations

`modules/axiom-placement` and the two apps on the legacy stack are `P1`'s
problem, not this manifest's. Confirm `P1` has landed before starting.

## Completion criteria

1. `ScalarField::sample` exists; `implicit_surface_mesh` has a real producer.
2. `TextureOp::Field` bakes a field to RGBA8, with a golden.
3. `MeshOp::Displace` is field-driven and byte-compatible with its predecessor.
4. `implicit.rs`'s private field expression is gone; `mc_tables.rs` is deleted or
   its retention is justified in writing.
5. `cargo test --workspace` passes including `axiom-proc-fuzz`.
6. Coverage 100/100/100; `cargo xtask check-architecture` exits 0; no dylint
   count rises.

## Validation commands

```sh
cargo test -p axiom-mesh-ops -p axiom-proc-texture -p axiom-proc-mesh
cargo test --workspace
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 5.** Parallel with `04`. Owns three layer crates; touches no module, no
app, and no root manifest.
