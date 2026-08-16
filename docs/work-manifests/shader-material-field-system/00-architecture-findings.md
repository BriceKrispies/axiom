# 00 — Architecture Findings (the decision record)

> **Status:** investigation complete, **no production code written**. The only
> files this session created are the planning artifacts in this directory.
>
> This document is the decision record. Where it and any later manifest
> disagree, **this document wins on architecture**; the numbered manifests win
> on execution ordering and file ownership.

---

## 1. Existing architecture, as it actually is today

`cargo xtask check-architecture` → exit 0, *"OK: all layers satisfy the Axiom
Layer Law"*, **24 layers**.

### 1.1 The layer DAG

| layer | crate | `depends_on` |
|---|---|---|
| kernel | `axiom-kernel` | *(root)* |
| runtime | `axiom-runtime` | kernel |
| math | `axiom-math` | kernel, runtime |
| host | `axiom-host` | kernel, runtime, math |
| frame | `axiom-frame` | kernel, runtime, host |
| ecs | `axiom-ecs` | kernel, frame |
| introspect | `axiom-introspect` | kernel, frame, ecs |
| crypto | `axiom-crypto` | kernel |
| interface | `axiom-interface` | kernel |
| layout | `axiom-layout` | kernel, host |
| state | `axiom-state` | kernel |
| space | `axiom-space` | kernel |
| entropy | `axiom-entropy` | kernel, space |
| **noise** | `axiom-noise` | kernel, math |
| geosphere | `axiom-geosphere` | math |
| hydrology | `axiom-hydrology` | geosphere, kernel |
| **mesh** | `axiom-mesh` | kernel, math |
| **mesh-ops** | `axiom-mesh-ops` | kernel, math, mesh |
| **recipe** | `axiom-recipe` | kernel |
| proc *(legacy v1)* | `axiom-proc` | kernel, space, entropy |
| proc-core | `axiom-proc-core` | recipe, space, entropy |
| proc-validate *(legacy)* | `axiom-proc-validate` | kernel, proc |
| **proc-texture** | `axiom-proc-texture` | recipe, proc-core, space, noise, math |
| **proc-mesh** | `axiom-proc-mesh` | recipe, proc-core, space, noise, math |

Bold rows are the ones this work touches.

### 1.2 The generation cluster is a *two-generation* stack, and both generations ship

There are **two independent recipe interpreters** in the repo.

| | v1 `axiom-proc` (2026‑06‑24) | v2 `axiom-recipe` + `axiom-proc-core` (2026‑07‑04) |
|---|---|---|
| node | `RecipeNode { op: NodeOp, immediate: u64, inputs: [usize; 2] }` — fixed arity 2, **closed built-in ops** (`Const/Draw/Add/Xor`) | `Node { op: u16, params: Vec<Param>, inputs: Vec<NodeId> }` — variable arity, **open opcode**, domain supplies the table |
| output | `Artifact` (`Vec<u64>`) + `ProcTrace`, both digested | generic `Out: Clone`; **no artifact, no trace, no digest** |
| recipe serialization | none | `serialize`/`deserialize`/`digest` |
| evaluation | resumable `Evaluation::step(n)` | run-to-completion `try_fold` |

v1 has had no functional commit since 2026‑06‑26 but is still load-bearing:
`crates/axiom-proc-validate`, `modules/axiom-placement`, `apps/axiom-proc-player`,
`apps/axiom-quintet`, `tools/axiom-proc-fuzz`, `tools/axiom-proc-inspect` all
depend on it. **Neither generation is a superset of the other.**

### 1.3 What `axiom-recipe` actually is

`crates/axiom-recipe/src/recipe_graph.rs:25`, `node.rs:15`, `value.rs:72`:

```rust
pub struct RecipeGraph { id: RecipeId, version: u32, nodes: Vec<Node> }
pub struct Node        { op: u16, params: Vec<Param>, inputs: Vec<NodeId> }
pub struct Param(u32);   // ONE opaque 32-bit word
```

It already supplies, correctly and fully covered:

* **DAG by construction** — `validate()` (`recipe_graph.rs:77`) requires every
  input id `< index`. A forward reference is the only way to form a cycle in an
  id-ordered append graph, so there is no cycle *search*.
* **Canonical bytes** — `SchemaVersion::new(1, 0)` stamp, little-endian, length-prefixed.
* **Content digest** — `digest() = StableHash::of_bytes(&self.serialize())`.
* **Bounded size** — `MAX_NODES: usize = 256`.
* **Branchlessness** — the reason `Param` is untyped, stated at `value.rs:5`:
  *"deliberately untyped in the graph so the container stays domain-free and
  branchless (no per-variant `match` to read a value)."*

What it deliberately does **not** supply, per its own `lib.rs:16`: *"An operator
code is an opaque `u16` … What a code *means* … and how a node is *evaluated*
belong to a higher generation layer, never here."*

Concretely it lacks: a typed value union, per-op signatures, heterogeneous node
output types (`ProcCore::execute<Out, F>` is monomorphic in `Out`, so a
scalar→vec3→color pipeline cannot exist in one graph), multi-output ports, and
any canonical *form* (the digest is authoring-order sensitive).

### 1.4 What `axiom-proc-texture` / `axiom-proc-mesh` actually are

**Whole-artifact raster/geometry op graphs, not pointwise expressions.** Every
node materialises a complete output:

```rust
// crates/axiom-proc-texture/src/texture_buffer.rs:11
pub struct TextureBuffer { width: u32, height: u32, pixels: Vec<u8> }
pub const MAX_DIM: u32 = 512;
```

11 texture ops (`Solid, Gradient, Noise, Bricks, Blur, Blend, ColorRamp,
HeightToNormal, Checker, Text, Spots`) and 12 mesh ops (`Cube … MetaSurface`),
each dispatched by `const OPS: [TexOp; 11]` indexed by opcode
(`dispatch.rs:16`). Resolution is a *node parameter*; `Blur` is a neighbourhood
operator; every graph edge costs a full `Out::clone()` (`proc_core.rs:54`).

**This is the crux.** A whole-buffer op graph is not lowerable to a fragment
program and cannot express "roughness at this surface point". It is a bake-time
image pipeline, and it is correct as such.

### 1.5 The appearance chain today

Four material types carrying the same five fields:

| tier | type | file |
|---|---|---|
| app | `Material` | `modules/axiom/src/material.rs:31` |
| resource | `MaterialData` | `modules/axiom-resources/src/material_data.rs:39` |
| render | `RenderMaterial` | `modules/axiom-render/src/render_material.rs:29` |
| pipeline (private) | `MaterialAsset` / `MaterialSlot` | `modules/axiom-render-pipeline/src/render_pipeline_api.rs:61,157` |

```rust
pub struct Material {
    base_color: Color, texture: Option<Texture>, emissive: Color,
    roughness: Ratio, opacity: Ratio,
    custom_texture: u64, texture_sampling: TextureSampling,
}
```

* `roughness` **is** live — converted to specular strength at
  `render_pipeline_api.rs:413` (`1.0 - roughness`) and consumed as Blinn-Phong
  with a fixed `SPECULAR_POWER: f32 = 48.0`.
* `opacity` **is** live — `blend_state(false)` is `ALPHA_BLENDING`. The
  "opacity doesn't blend" comments at `material.rs:104` and
  `render_material.rs:26` are **stale**.
* `metallic` **does not exist anywhere in the repo.**
* `Texture` is a 3-variant built-in enum (`Checker | UvGrid | BiomeAtlas`,
  `modules/axiom/src/texture.rs:15`). The only other path is raw bytes through
  `RunningApp::add_texture_data` (`modules/axiom/src/app/authoring.rs:132`).
* **There is no path from `Material` to `ProcTextureApi`.** Only 2 of 32 apps
  depend on the procedural texture layer at all.

### 1.6 The backend-neutral presentation boundary is `crates/axiom-host`

`crates/axiom-host` already owns `FramePacket`, `FrameDrawItem`, `FrameLight`,
`FrameRenderLook`, `FrameSky`, `FrameBloom`, `FramePostProcess`,
`MaterialTexture`, `SdfScene`, `RenderCapability`, `BackendCapabilityProfile`.

`frame_packet.rs:1-14` states its role exactly: *"It carries only primitives — no
GPU, browser, DOM, render-module, or scene types — so it is a stable
presentation-boundary contract any backend can name."*

It is a layer for the `axiom-mesh` reason: `gpu-backend` and `canvas2d-backend`
are engine **modules**, and a module may never depend on another module.

### 1.7 The renderer

* **Shader source lives in exactly two crates**: `modules/axiom-gpu-backend`
  (7 WGSL consts, 8 shader modules, ≤10 pipelines) and `modules/axiom-webgpu`'s
  off-by-default `offscreen` arm. Everything upstream is innocent of shaders.
* **There is no pipeline cache and there are no shader variants.** Every pipeline
  is a named struct field built once in a constructor. Feature selection is a
  runtime `u32` in the lights UBO, gated by `select()` with both arms evaluated.
* `RenderCapability` is 12 bits; two profiles exist (`all()`, `canvas2d()`).
  Bits are pinned as a cross-language contract by
  `capability_bits_are_the_gpu_shader_contract` (`frame_capability.rs:324`).
* `INSTANCE_FLOATS = 40` (`mvp 16 + world 16 + colour 4 + emissive 3 +
  specular 1`) — **zero free lanes**; the specular lane was taken from the last pad.
* The rigid pipeline binds **16 vertex attributes, exactly the WebGL2 downlevel
  `MAX_VERTEX_ATTRIBS` guarantee** (`scene_renderer.rs:245`); the skinned
  pipeline is at that ceiling, which is why skinned draws silently drop emissive
  and specular.
* **No content hashing, no interning, no dedup** in the render path. Every GPU
  cache is keyed on a caller-assigned `u64`.
* `RenderPipelineKind::UNLIT = 2` is emitted by `axiom-render` and **dies at the
  `FramePacket` boundary** — no backend has ever seen it.

### 1.8 Duplicated ad-hoc appearance logic (the pressure)

Ranked by how strongly each argues for a lower primitive:

1. **Per-texel CPU texture authoring, 11 hand-written RGBA8 loops across 4 apps.**
   `hash_unit` is **byte-identical** in `apps/burnt-rubber/src/render/asphalt_texture.rs:385`,
   `verge_texture.rs:207`, `foliage_texture.rs:282`. So are `smoothstep`/`lerp`
   and a hand-rolled sRGB encode (3×, plus an inverse). `apps/axiom-growth/src/visual_target/build.rs:1214`
   `normal_map_from_height()` is a from-scratch reimplementation of
   `TextureOp::HeightToNormal`.
2. **`modules/axiom/src/color.rs:35` has no arithmetic** — no lerp, mix, hsl, or
   sRGB. `smoothstep` is written 6× and `lerp` 7× across apps.
3. **Colour ramps over a field written 3× inside one app** (`axiom-growth`), whose
   own comment admits it *"mirrors the native renderer's `color_for`/elevation ramp"*.
4. **Grade/fog/exposure re-derived in apps** — `build.rs:821-853` hand-rolls the
   chain `axiom_host::FramePostProcess` already implements.
5. **Masks built as geometry** because there is no mask primitive —
   `apps/end-zone/src/field/generator.rs:177` builds mow stripes as alternating
   mesh slabs, which is literally `TextureOp::Checker`.
6. **Vertex colours are hard-wired white** at `modules/axiom/src/app/resources.rs:98`
   and absent from the app-facing `MeshData`, so an app *physically cannot* bake
   a colour field into a mesh — which is why they all produce byte arrays instead.

### 1.9 The engine has already stated, in code, that it is missing this primitive

`crates/axiom-mesh-ops/src/implicit_surface.rs:10-14`:

> **"Why sampled data rather than a field callback.** A public `impl Fn` parameter
> is forbidden across this engine's spine: a callback is an opaque capability
> that could read a clock or a global, which would make the operator
> unreplayable. **A sampled lattice is a value — hashable, diffable, and
> reproducible — which is the whole reason this layer exists.**"

`ScalarField` is therefore a materialised `Vec<f32>` lattice. Commit `454707c0`
(2026‑08‑15) tried to close the resulting gap with a concrete `Solid`/`SolidField`
type — *"nothing could BUILD one … so `ScalarField` was constructed nowhere but
its own tests"* — and was reverted the same day by `a5a9472f`, explicitly **not**
for structural reasons.

Meanwhile `crates/axiom-proc-mesh/src/implicit.rs:41-72` contains a private,
non-composable field expression (`capsule_sdf`, `smin`, `field`, `field_normal`)
flattened into the `MeshOp::MetaSurface` parameter encoding, because there is no
type in which to express it.

**This is the finding.** Axiom independently discovered that it needs *functions
as reproducible values*, and its only available answer is "materialise the
function's output into a buffer or a lattice". The missing primitive is stated in
the negative, three times, in three different layers.

---

## 2. The deepest reusable primitive

> **A *field* is a deterministic, hashable, canonically-serializable pure
> function from an explicitly supplied typed evaluation context to a typed
> value — represented as a closed-algebra, id-ordered, acyclic typed expression
> graph.**

It is the engine's missing **function-as-a-value**. Rendering does not appear in
that sentence, and must not.

**Owner: a new layer, `crates/axiom-field` (layer name `field`).**

```toml
[layer]
name = "field"
crate_name = "axiom-field"
depends_on = ["kernel", "math", "recipe", "noise"]
```

### 2.1 Why a layer, and why those four dependencies

**Why a layer and not a module.** The crates that must *name* a field today are
`crates/axiom-mesh-ops` (§1.9), `crates/axiom-proc-texture`,
`crates/axiom-proc-mesh` — all **layers**. A layer may not depend on a module, so
a module placement is structurally impossible, not merely inconvenient. The
precedent is exact and is written into the repo's own root `Cargo.toml`:

> *"The canonical neutral triangle-mesh representation … Both are layers: seven
> engine modules need to name mesh geometry, and an engine module may never
> depend on another module."*

and `crates/axiom-mesh/src/lib.rs:26`: *"Seven engine modules need to name
triangle geometry … so the shared primitive has to be a layer."*

**Why each dependency is genuine, not ceremonial:**

| dep | genuine use |
|---|---|
| `kernel` | `StableHash` for the graph digest; `BinaryWriter`/`BinaryReader`/`SchemaVersion` for canonical bytes; `KernelError` as the decode cause; `Ratio`/`Seconds` to keep naked floats off the public API (`engine_no_unitless_float_public_api`) |
| `math` | `Vec2`/`Vec3`/`Vec4` **are** the value union's non-scalar members and the sample point; `Mat4` is the `Transform` op's parameter |
| `recipe` | `FieldGraph` is built **on** `RecipeGraph`: it reuses `NodeId`, `Param`, the append-only DAG-by-construction invariant, `validate`, the canonical byte format and `digest`. This is a real adaptation — it turns an untyped opaque-op DAG into a type-checked expression — and it is the same relationship `proc-texture`/`proc-mesh` already have with `recipe` |
| `noise` | the `Noise` and `Fbm` ops' evaluation calls `value_noise` / `Fbm`. Without these ops the algebra cannot express any of the duplication in §1.8 |

**Not dependencies, deliberately:** `space` and `entropy`. A field is a *pure
function of its context*; its only entropy is a `seed` graph parameter fed to
`Noise`. Declaring them would be exactly the ceremonial edge the Layer Law bans.
`proc-core` is also not a dependency — see §2.3.

### 2.2 What must **not** live in `axiom-field`

This section is the load-bearing half of the decision.

| Excluded | Why |
|---|---|
| `base_color`, `roughness`, `metallic`, `emission`, `opacity`, `normal`, surface masks, material layering | These are **appearance semantics**, not expression semantics. A field knows nothing about "roughness". They belong one layer up, in `surface` (§3). |
| Anything named texture, texel, image, resolution, width/height | Resolution is a *baking* parameter. A field is pointwise and resolution-free. `TextureBuffer` stays in `proc-texture`. |
| Anything named shader, WGSL, pipeline, bind group, varying, entry point, stage | Backend concepts. `crates/axiom-host/ARCHITECTURE.md:133` and `crates/axiom-frame/ARCHITECTURE.md:140` already rule that *"a **shader compiler** … None of that belongs"* at this depth. |
| Lights, cameras, view/projection, screen space, `dpdx`/`dpdy` screen-space derivatives | Rendering context. Screen-space derivatives are backend-specific and have already caused a real defect (mobile-GPU derivative-NaN). Normal-from-height is finite differences at a **caller-supplied** offset, not a derivative op. |
| Meshes, entities, scene nodes, resource ids, asset ids | `axiom-mesh` sets the precedent: *"It names no material, texture, shader, GPU buffer, vertex layout, scene node, entity, resource id, or asset."* |
| Wall-clock time, ambient RNG, `impl Fn` callbacks, `TypeId`/`Any` | Determinism rules + `engine_no_time_in_sim` + `engine_no_retained_state` + `engine_no_runtime_type_branch`. Time enters only as an explicit `EvalContext` value. |
| An open/extensible op vocabulary | `docs/engine-datafication.md:310` non-goal. See §5. |

### 2.3 Why `field` does not build on `proc-core`

`ProcCore::execute` evaluates a graph **once**, caching one full `Out` per node
and minting an `EntropyStream` per node. A field is evaluated **once per sample
point** — hundreds of thousands of times per baked texture. Reusing `proc-core`
would allocate a `Vec<Out>` and an entropy stream per texel. Additionally
`ProcCore::execute`'s `F: Fn(NodeEval) -> Option<Out>` generic parameter is
itself an `engine_no_retained_state` violation, and the sanctioned shape is a
`const [fn; N]` table.

`field` therefore owns a tight pointwise evaluator over a fixed-size register
array, dispatched by `const OPS: [FieldOpFn; N]` — the same technique
`proc-texture`/`proc-mesh` use, without the whole-artifact cache.

---

## 3. The strata above the primitive

### A. Generic field/expression system → **Layer: `field`** (new, `crates/axiom-field`)

Typed nodes, constants, parameters, arithmetic, vector math, coordinate
transforms, procedural functions (noise/fbm), remap/clamp/mix/smoothstep, graph
composition, validation, canonicalisation, deterministic serialization, hashing,
and the **CPU reference evaluator**.

Concrete semantic compatibility with future systems is *proven by existing code*,
not speculated: `mesh-ops::ScalarField` (implicit surfaces), `proc-texture`
(per-texel generation), `proc-mesh::Displace`, `modules/axiom-terrain`'s
re-implemented lattice noise, and `apps/burnt-rubber`'s three texture generators
are all pointwise field evaluations written by hand today.

### B. Surface/material semantics → **Layer: `surface`** (new, `crates/axiom-surface`)

```toml
depends_on = ["kernel", "math", "field"]
```

**Why a layer, not a module.** The crates that must name a material description
are `modules/axiom-resources`, `modules/axiom-render`,
`modules/axiom-render-pipeline`, `modules/axiom-gpu-backend`,
`modules/axiom-canvas2d-backend`, `modules/axiom-assets`, and `modules/axiom` —
**seven engine modules**. Modules may not depend on modules. Same law, same
precedent as `mesh` and `host`.

**Why not fold it into `axiom-host` beside `FrameDrawItem`.** `host` is the
*flattened presentation boundary* — "primitive-only … no render-module or scene
types" — consumed by backends after all authoring is resolved. A graph-bearing
authoring type there inverts its role, and `host` is additionally the one
platform-facing layer. `Surface` is upstream of the flattening; `FrameDrawItem`
stays exactly as it is and gains only an opaque `surface_program: u64` identity
lane.

**Why not fold it into `field`.** Channel names *are* rendering semantics. Putting
`roughness` in the generic primitive is precisely the contamination the No-Shortcuts
rule forbids.

`Surface` is a **closed record of channels**, each bound to a constant or a field
id — not an open graph:

```
base_color   : Vec4 field or constant
roughness    : Scalar
metallic     : Scalar          <- does not exist in the engine today
normal       : Vec3, or derived from a height Scalar field
emission     : Vec4
opacity      : Scalar
displacement : Vec3 (vertex stage)
lighting     : LightingModel discriminant (Unlit | Lambert | LambertSpecular)
layers       : [SurfaceLayer { surface, mask: Scalar field, blend }]
```

### C. Render-facing semantic shader IR → **REJECTED as a distinct stratum**

The hypothesis in the brief was that a semantic shader IR sits between material
semantics and backend lowering. **The repository rejects it.**

Evidence: there is exactly **one lit shader**, ≤10 pipelines, **no variant
machinery at all**, an explicit twice-written anti-variant doctrine
(`post_chain.rs:462`, `surface_encode.rs:82` — *"so a device cannot stutter
compiling a second variant mid-session"*), and the one variant seam that does
exist (`RenderPipelineKind::UNLIT`) is unwired and dies at the `FramePacket`
boundary. There is no second consumer for a backend-neutral shader IR: the only
other backend, `canvas2d`, **cannot execute a program at all** — it flat-shades
per triangle and discards uv and normal at upload (`mesh_cache.rs:27-31`).

The two things a shader IR would carry split cleanly and neither justifies a
layer:

* **Backend-neutral requirements** (which context inputs the graph reads, which
  channels are non-constant, the parameter layout) are *derivable from the
  `Surface` graph itself* → a `Surface::requirements()` method in `surface`.
* **The program plan** (stage assignment, varyings, bind-group indices, uniform
  packing, attribute budget) is inherently backend-shaped — it is constrained by
  the WebGL2 16-attribute ceiling and the full 40-float instance stride — and
  belongs beside the emitter in `modules/axiom-gpu-backend`.

A third IR between them would be a ceremonial compiler stage. **Work unit `05` in
the requested list is therefore merged**, its neutral half into `04` and its
backend half into `07`.

### D. Backend lowering → **Existing Module: `gpu-backend`** (+ `canvas2d-backend`)

`modules/axiom-gpu-backend` already owns every WGSL const, every pipeline, every
bind group and the capability→shader bit contract, and is on
`PLATFORM_FACING_MODULES` (`crates/xtask/src/hygiene.rs:64`). WGSL generation,
program caching, binding layout, compilation errors, and backend-specific
optimisation all belong there and nowhere lower.

`modules/axiom-canvas2d-backend` consumes the **same `Surface`** by
CPU-evaluating its channels through `axiom-field`'s reference evaluator at each
triangle's centroid — it already flat-shades per triangle, so a per-triangle
field sample is a *correct degrade rather than a dropped capability*. This is
only possible because the field has a CPU evaluator, and it is the strongest
argument that the CPU evaluator is the semantic reference implementation rather
than a testing convenience.

---

## 4. Higher-level consumers, and how they relate

| Consumer | Relationship to the primitive |
|---|---|
| Procedural textures | `proc-texture` gains one op that samples a field per texel. `TextureBuffer` stays where it is. |
| Implicit surfaces | `mesh-ops::ScalarField` gains a constructor that samples a field onto its lattice, closing the gap its own doc names and the `a5a9472f` revert left open. |
| Vertex deformation | A field consumer. Bake-time: `proc-mesh::Displace` / `mesh-ops`. Runtime: the `Surface::displacement` channel, which is only the *binding site* for the vertex stage. |
| Materials | Compose fields per channel; never duplicate them. |
| Lighting | Lights stay scene/frame data (`FrameLight`). Materials participate through the closed `LightingModel` discriminant, which finally gives `RenderPipelineKind::UNLIT` something behind it. |
| Backends | GPU lowers to WGSL; Canvas2D CPU-evaluates; both validate against `RenderCapability` before rendering. |
| Terrain / biome / worldgen | Replace hand-rolled lattice noise (`modules/axiom-terrain/src/terrain_api.rs:48-62`) with a field. |
| Agents | Inspect, rewrite, diff, hash, and validate graphs as data (§7). |

---

## 5. The documented non-goal, and why this design does not violate it

`docs/engine-datafication.md:310` lists as a non-goal:

> *"A data-described shader-graph VM (parameterizing a closed model is fine; an
> open graph is not)."*

Read with the bullet immediately above it — *"An open/extensible op vocabulary
that data can add verbs to at runtime"* — the ban is on **runtime-extensible
verbs and a frame-time interpreter**, not on expression graphs. `axiom-recipe` +
`axiom-proc-texture` is already a shipped, blessed data-described graph with a
closed op set, and the same document calls it *"the template every other
datafication in Axiom should resemble"*.

The same document's Frontier section (`:234-240`) actively asks for this work:

> *"Materials already carry `roughness`/`emissive` the shader ignores, and an
> `UNLIT` pipeline marker is emitted but unwired. Parameterize the fixed model by
> data and select from a small closed set of variants by discriminant."*

**The design is therefore constrained to, and complies with:**

1. A **closed** op algebra fixed in Rust — 23 ops, `#[repr(u16)]`, no runtime
   extension, no registry, no dynamic dispatch.
2. **No frame-time interpretation on the GPU path.** Lowering and shader
   compilation happen at the **preparation barrier** —
   `RuntimeState::Prepared` / `PreparationTask` in `crates/axiom-runtime`, which
   already exists and is exactly *"expensive, startup-only procedural work runs
   to completion … before the application may begin stepping"*. This structurally
   answers the renderer's anti-variant doctrine: a device can never stutter
   compiling a variant mid-session, because no variant is ever compiled
   mid-session.
3. **Runtime-varying values are parameters, not graph edits** — changing a
   parameter changes a uniform, not the program hash, so it cannot cause a
   recompile.

`docs/engine-datafication.md` should be amended to record this reading as part of
manifest `04`. **Do not land this work silently against a written non-goal.**

---

## 6. Final ownership table

| Concern | Owner | Why |
|---|---|---|
| typed expressions | **Layer `field`** (`crates/axiom-field`) | Three layers (`mesh-ops`, `proc-texture`, `proc-mesh`) must name it; a layer cannot depend on a module, so no module placement is legal. Same law as `axiom-mesh`. |
| procedural fields (noise, fbm, gradients, masks, remap) | **Layer `field`** | They are the op algebra. `noise` supplies the coherent-noise kernel; `field` composes it. |
| baking a field to pixels | **Layer `proc-texture`** | `TextureBuffer` and `MAX_DIM` already live there; resolution is a baking parameter, not a field concept. |
| baking a field to a lattice / implicit surface | **Layer `mesh-ops`** | `ScalarField` + marching cubes already live there and its doc already names the missing constructor. |
| material semantics (base colour, roughness, metallic, normal, emission, opacity, masks, layering) | **Layer `surface`** (`crates/axiom-surface`) | Seven engine modules must name a material description; modules may not depend on modules. Same law as `axiom-host`. |
| shader IR | **No owner — rejected** | One shader, no variants, one non-programmable backend. Neutral requirements fold into `surface`; the program plan folds into `gpu-backend`. |
| CPU evaluation | **Layer `field`** — and it is the semantic reference | The repo already practises this: `FrameSky::radiance`, `FrameDepthFog::mix_fraction`, `FrameBloom::tonemap` are CPU definitions in `axiom-host` that the WGSL mirrors, pinned by parity tests. |
| shader optimisation (const fold, CSE, DCE) | **Layer `field`** (canonicalisation) | Optimising the semantic graph is backend-independent and is what makes the hash stable. Backend-specific optimisation stays in `gpu-backend`. |
| WGSL lowering | **Module `gpu-backend`** | Already owns all 7 WGSL consts and every pipeline; already platform-facing. |
| WebGPU binding / bind groups / uniform packing | **Module `gpu-backend`** | Constrained by the 16-attribute WebGL2 ceiling and the 40-float instance stride, both of which live there. |
| pipeline & program caching | **Module `gpu-backend`**, populated at the preparation barrier | It holds the only pipelines that exist; `axiom-runtime::PreparationTask` supplies the "before play begins" phase. |
| software (Canvas2D) shading | **Module `canvas2d-backend`**, via `field`'s CPU evaluator | It already flat-shades per triangle; a centroid field sample is a correct degrade, not a dropped capability. |
| raw shader escape hatch | **Module `gpu-backend` only** | Keeping it out of `surface` means it cannot be serialized, hashed, CPU-evaluated, or consumed by Canvas2D — so it cannot infect the semantic model. |
| capability validation | **Layer `surface`** declares requirements; **`crates/axiom-host`** owns `RenderCapability`; the **backend** decides | `BackendCapabilityProfile` already lives in `host` for exactly this cross-backend reason. |
| agent introspection | **Layer `field`** (graph walk + schema) and **`crates/axiom-introspect`** (reporting) | `field` owns the data; `introspect` owns the report shape (`WorldTag` is the template). |

---

## 7. Rejected placements

| Rejected | Why it is structurally wrong |
|---|---|
| **Everything in `axiom-render`** | `axiom-render` is an engine module with `allowed_modules = []`. `mesh-ops`, `proc-texture` and `proc-mesh` are **layers** and could never depend on it, so the three consumers that most need a field (§1.9) would be permanently excluded. It also cannot be named by `canvas2d-backend` or `gpu-backend` (module→module). |
| **Everything in the WebGPU backend** | Would make a rendering *backend* foundational to implicit surfaces, terrain and texture baking — none of which render. It also fails the same module→module rule for every other consumer, and would put WGSL on the critical path of a CPU bake. |
| **Making "materials" the foundational abstraction** | A material is a *record of channels*; a field is the *value in a channel*. Making the record foundational leaves implicit surfaces, terrain heightfields, and vertex displacement with nothing to build on — the exact situation today, which produced the duplication in §1.8. It also cannot express the composition the brief asks for, because layering is defined *in terms of* mask fields. |
| **Shader concepts in `axiom-math`** | `math` is the dimensionless linear-algebra layer and is one of only two crates the `engine_no_unitless_float_public_api` lint exempts as a "scalar floor". A graph, a schema version, a digest and an op table are not linear algebra. `math` correctly gains nothing here beyond being depended on. |
| **App-specific shader authoring** | Already the status quo and already measured as the problem: `hash_unit` byte-identical in three files of one app, `smoothstep` 6×, `lerp` 7×, `normal_map_from_height` reimplemented. It is also actively policed — `xtask`'s `SlicePlacementEngineLogicInApp` exists specifically to flag engine logic hiding in `apps/` to dodge the coverage and branchless gates. |
| **Raw WGSL as the primary representation** | Fails every agentic requirement in the brief (no node identity, no typed inspection, no subgraph replacement, no diff, no stable hash, no CPU evaluation, no Canvas2D path, no capability pre-validation), and would make the only non-programmable backend permanently unable to render anything authored. It survives only as a deliberately quarantined backend-module escape hatch (§manifest 07). |
| **A new *module* for fields** | Structurally illegal: `mesh-ops`, `proc-texture` and `proc-mesh` are layers, and a layer may not depend on a module. |
| **Extending `axiom-recipe` in place with types** | Would destroy the property that makes `recipe` reusable — `value.rs:5` states the untyped word exists *"so the container stays domain-free and branchless"*. `recipe` is the correct *container*; `field` is the correct *typed language over it*, exactly as `proc-texture` is the correct *texture language over it*. |
| **Putting `Surface` in `axiom-host` beside `FrameDrawItem`** | `host` is the flattened, primitive-only presentation boundary and the one platform-facing layer. A graph-bearing authoring type inverts its stated role, and `host`'s own `ARCHITECTURE.md:133` rules a shader compiler out of it. |

---

## 8. Dependency and data-flow diagrams

### 8.1 Before — the appearance path today

```text
app authors Material { base_color, texture: enum|u64, emissive, roughness, opacity }
    |                          ^
    |                          |  (apps hand-write RGBA8 byte arrays here:
    |                          |   asphalt_texture.rs, verge_texture.rs,
    |                          |   foliage_texture.rs, growth/build.rs)
    v
modules/axiom-resources   MaterialData / MaterialTexture
    v
modules/axiom-render      RenderMaterial -> RenderInput -> RenderCommandList
    v                                             (draw_order sort: DEAD on the live path)
modules/axiom-render-pipeline   MaterialAsset/MaterialSlot -> RenderReport
    v
crates/axiom-host         FramePacket { FrameDrawItem { color, emissive, specular } }
    |                     RenderCapability (12 bits)  <-- pinned to the WGSL by a test
    +-----------------------------+
    v                             v
modules/axiom-gpu-backend      modules/axiom-canvas2d-backend
  ONE lit shader SCENE_WGSL       flat colour per triangle
  <=10 fixed pipelines            uv + normal DISCARDED at upload
  caps gate via select()          no textures at all
    v
WGSL -> wgpu -> WebGPU or WebGL2

crates/axiom-recipe -> proc-core -> proc-texture (TextureBuffer)   [ISLAND: no path to Material]
crates/axiom-mesh-ops  ScalarField (a Vec<f32> lattice; constructed only in its own tests)
```

### 8.2 After — proposed

```text
                         crates/axiom-kernel   (StableHash, BinaryWriter, SchemaVersion, Ratio)
                                  |
                    +-------------+-------------+
                    v                           v
            crates/axiom-math            crates/axiom-recipe   (RecipeGraph: DAG, canonical bytes, digest)
                    |                           |
                    +------------+--------------+
                                 v
                       crates/axiom-noise
                                 v
        =====================  crates/axiom-field  =====================   <-- THE PRIMITIVE
          FieldGraph  FieldValue  FieldOp  FieldId  EvalContext
          typed  |  validated  |  canonicalised  |  hashed  |  CPU-evaluated
          BACKEND-NEUTRAL. Knows nothing of rendering, GPU, texture, shader.
        ================================================================
             |                 |                  |                 |
   (bake)    |        (bake)   |      (implicit)  |     (semantics) |
             v                 v                  v                 v
   crates/axiom-proc-  crates/axiom-proc-  crates/axiom-mesh-  crates/axiom-surface
       texture              mesh                ops                 (NEW LAYER)
   TextureOp::Field    displacement by     ScalarField::         Surface { base_color,
   -> TextureBuffer    a field             sample(field)         roughness, metallic,
                                                                 normal, emission,
                                                                 opacity, displacement,
                                                                 lighting, layers[] }
                                                                 Surface::requirements()
                                                                 Surface::digest()
                                                                       |
      app authors a Surface (no WGSL, no byte arrays) ------------------+
                                                                       v
                                              modules/axiom-resources  (owns Surface by id)
                                                                       v
                                              modules/axiom-render     RenderInput / RenderCommandList
                                                                       v
                                              modules/axiom-render-pipeline
                                                                       v
                        crates/axiom-host   FramePacket / FrameDrawItem { .., surface_program: u64 }
                                            RenderCapability  (unchanged, primitive-only)
                    +--------------------------------+--------------------------------+
                    v                                                                 v
        modules/axiom-gpu-backend                                    modules/axiom-canvas2d-backend
   ---- BACKEND-SPECIFIC KNOWLEDGE STARTS HERE ----                  CPU-evaluates the Surface's
   SurfaceProgramPlan (stages, varyings, bindings)                   channels per triangle centroid
   WGSL emitter  |  program cache keyed on Surface::digest()         through axiom-field's evaluator
   compiled at the PREPARATION BARRIER, never mid-frame              (a correct degrade, not a drop)
   raw-WGSL escape hatch lives here and ONLY here
                    v
              wgpu -> WebGPU / WebGL2
```

**Backend-neutral boundary:** everything at or below `crates/axiom-surface`, plus
`crates/axiom-host`. **Backend-specific knowledge starts** inside
`modules/axiom-gpu-backend` and `modules/axiom-canvas2d-backend`, and nowhere else.

### 8.3 The field / material / compiler decomposition itself

```text
  AUTHORING                CANONICAL                 CONSUMPTION
  ---------                ---------                 -----------

  FieldGraph               FieldGraph                CPU:  evaluate(ctx) -> FieldValue
  (append-order,     ==>   (const-folded,      ==>         [the SEMANTIC REFERENCE]
   as the agent            CSE'd, DCE'd,
   built it)               topologically             BAKE: sample -> TextureBuffer
       |                   relabelled)                     sample -> ScalarField
       |                        |                          sample -> displaced Mesh
       |                        |
       |                   canonical bytes           GPU:  SurfaceProgramPlan -> WGSL
       |                   (SchemaVersion-                 [a MIRROR, pinned by a
       |                    stamped, LE)                    parity test vs the CPU
       |                        |                           reference]
       |                        v
       |                   digest() = StableHash  ==> program cache key
       v
  Surface { channel -> FieldId | constant, layers[], lighting }
       |
       +--> Surface::requirements() -> which context inputs, which channels vary,
            parameter layout            -> checked against BackendCapabilityProfile
                                           BEFORE any lowering is attempted
```

**Representations, and whether they are distinct** (the brief's §8):

| # | Representation | Distinct? | Boundary |
|---|---|---|---|
| 1 | author-facing semantic graph | **same type** as 2 | `FieldGraph` in append order |
| 2 | validated / canonical graph | **same type**, different *state* | `FieldGraph::canonicalize()` returns a `FieldGraph`; only the canonical one is hashed or lowered. One type, because they are the same language — a separate "validated" type would be a newtype with no new operations. |
| 3 | optimised shader IR | **rejected** | Const-fold/CSE/DCE happen in canonicalisation (2); nothing else is backend-neutral. See §3C. |
| 4 | backend-specific representation | **distinct** | `SurfaceProgramPlan` in `gpu-backend` — stages, varyings, bindings. Cannot be neutral: it is defined by the 16-attribute ceiling and the 40-float instance stride. |
| 5 | generated WGSL | **distinct** | A `String` produced by the emitter, cached by `Surface::digest()`. |

Three representations, not five. Each boundary is a genuine change of
responsibility, and no stage exists only to have a stage.

---

## 9. Prerequisites and conflicts found

Ordered by how hard they block.

1. **`RecipeError` cannot name a node.** `crates/axiom-recipe/src/recipe_error.rs`
   is a fieldless enum, so `CyclicInput` cannot say *which* node. Structured,
   node-pointing diagnostics are a first-class requirement of the brief. Fix at
   the lowest correct layer — `axiom-recipe` — using the established
   `StateError::at(code, id, msg)` / `.about(id)` pattern
   (`crates/axiom-state/src/state_error.rs`), a `Copy` struct with the id as a
   **field**, never an enum payload. → **manifest `P2`, hard prerequisite.**
2. **The legacy `axiom-proc` stack must not be a third generation's foundation.**
   Adding `field` while v1 and v2 both ship makes the cluster worse. `axiom-proc`,
   `axiom-proc-validate` and `modules/axiom-placement` should converge onto
   `recipe`/`proc-core` first. → **manifest `P1`**; runs in parallel, must land
   before `05`.
3. **`Material` has no `metallic`, and no path to procedural textures.** The
   `Material` → `MaterialData` → `RenderMaterial` → `MaterialAsset` chain must
   carry a surface id. → **manifest `06`.**
4. **The instance stream has zero free lanes** (`INSTANCE_FLOATS = 40`) and the
   rigid pipeline binds **16 of 16** WebGL2-guaranteed vertex attributes. Per-surface
   parameters therefore need a **new uniform channel**, not a new instance lane or
   vertex attribute. This is a real contract change inside `gpu-backend`. → **`07`/`09`.**
5. **No content-hash → program identity exists in the render path.** Every GPU
   cache is keyed on a caller-assigned `u64`. `StableHash` exists in the kernel and
   is the right key. → **`09`.**
6. **Vertex colours are hard-wired white** (`modules/axiom/src/app/resources.rs:98`)
   and absent from the app-facing `MeshData`, though `axiom_mesh::MeshStreams.colors`
   exists. This blocks baking a colour field into a mesh. Not required for the
   slice. → **`06`, optional scope.**
7. **`engine_no_large_enums` caps enums at 24 variants** and is deny-at-zero for a
   new crate. The proposed algebra is **23 ops**. Any growth past 24 must move to a
   bare `u16` code with a `const` catalog, the `axiom-recipe` shape.
8. **`engine_no_unitless_float_public_api`** — a naked `f32` may not appear on
   `axiom-field`'s or `axiom-surface`'s public surface. Quantity newtypes only;
   `axiom-recipe::Scalar(f32)` is the exemplar (a single-field newtype is exempt
   for its own `new`/`get`).
9. **`engine_no_retained_state` bans `F: Fn(..)` generic parameters.** The
   evaluator must be a `const [fn; N]` table, not a closure parameter.
10. **`engine_no_unportable_float` exists but is not registered** in
    `[workspace.metadata.dylint].libraries`, so float-portability is currently
    unenforced. Relevant to CPU/GPU parity tolerance. Flagged, not fixed here.
11. **`.github/workflows/ci.yml` is `workflow_dispatch:` only** (disabled
    2026‑07‑14) and the dylint gate **fails by design** (787 `engine_no_retained_state`
    findings). Every manifest's validation commands are therefore **local, by hand**,
    and "the dylint gate is green" is not an available acceptance criterion — the
    criterion is "this lint's count did not rise".

**No architecture law needs weakening.** Every placement above is legal under the
Layer Law and the Module Law as they stand today.

---

## 10. Recommended implementation sequence

```text
  P1 retire-legacy-proc-stack ─┐   (parallel with everything until 05)
  P2 recipe-node-diagnostics ──┤
                               v
  01 foundational-field-ir  ───────────────────────────────┐
        v                                                  |
  02 field-validation-and-canonicalization                 |
        v                                                  |
  03 field-cpu-evaluator                                   |
        v                                                  |
        ├──> 05 bake-time-field-consumers  (needs P1)      |
        |                                                  |
        └──> 04 material-semantics (axiom-surface) <────────┘
                  v
                  ├──> 06 render-contract-integration
                  |          v
                  |    07 backend-lowering ──> 08 wgsl-generation ──> 09 program-caching
                  |                                                        v
                  ├──> 10 vertex-deformation ─────────────────────────────>|
                  ├──> 11 lighting-integration ───────────────────────────>|
                  └──> 12 agentic-introspection-and-serialization          |
                                                                           v
                                                          13 vertical-slice-and-regression-proof
```

The requested `05-render-shader-ir.md` is **not a work unit** (§3C); the number is
reused for the bake-time consumers, which are the first consumers that prove the
primitive without touching the renderer at all.
