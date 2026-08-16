# 04 — Material semantics: the `surface` layer

## Objective

Create the layer `crates/axiom-surface`: the engine's neutral **appearance
artifact**, a closed record of named channels each bound to a constant or a
field, plus mask-driven layering, a lighting-model discriminant, and a derived
requirements summary. This is where "roughness" is allowed to exist for the first
time.

## Architectural placement

**Layer: `surface`** — a new crate `crates/axiom-surface`.

```toml
[layer]
name = "surface"
crate_name = "axiom-surface"
depends_on = ["kernel", "math", "field"]
```

**Why a layer.** Seven engine **modules** must name a material description:
`axiom-resources`, `axiom-render`, `axiom-render-pipeline`, `axiom-gpu-backend`,
`axiom-canvas2d-backend`, `axiom-assets`, and `axiom` (the engine feature
module). An engine module may never depend on another module, so the shared
primitive has to be a layer. This is the identical argument that made
`crates/axiom-mesh` a layer (*"seven engine modules need to name triangle
geometry"*) and that put `FramePacket`/`RenderCapability` in `crates/axiom-host`.

**Why not in `axiom-host`.** `host` is the *flattened presentation boundary* —
`frame_packet.rs:1-14` states it carries *"only primitives — no GPU, browser,
DOM, render-module, or scene types"* — consumed by backends after all authoring
is resolved. A graph-bearing authoring type inverts that role. `host` is also the
one platform-facing layer, and its own `ARCHITECTURE.md:133` rules a shader
compiler out of it. `FrameDrawItem` stays exactly as it is and gains only an
opaque `surface_program: u64` identity lane in `06`.

**Why not in `axiom-field`.** Channel names *are* rendering semantics. Putting
`roughness` in the generic primitive is the contamination
`00-architecture-findings.md` §2.2 forbids, and it would make `mesh-ops` — which
needs fields for implicit surfaces and has nothing to do with rendering — depend
on a crate that knows about emission.

### `layer.toml`

```toml
meaningful_dependency = """
Surface adapts field's typed pointwise expressions into the engine's one neutral
appearance artifact: it names the closed set of shading channels a renderer
consumes (base colour, roughness, metallic, normal, emission, opacity,
displacement), binds each to either a constant FieldValue or a FieldGraph
evaluated in surface-local coordinates, composes surfaces through mask fields
that are themselves scalar field expressions, and derives from that graph the
requirements a backend must satisfy before it can render. Field owns expressions
but no notion of what an expression is FOR; math supplies the Vec3/Vec4 the
channels carry and the Mat4 of the surface's coordinate frame; the kernel supplies
the canonical bytes and the StableHash that give a surface a stable program
identity. This layer adds the channel vocabulary, the layering algebra, and the
capability contract.
"""

introduced_capabilities = [
  "Surface", "SurfaceBuilder", "SurfaceChannel", "ChannelBinding",
  "SurfaceLayer", "LayerBlend", "LightingModel",
  "SurfaceRequirements", "SurfaceInput",
  "SurfaceError", "SurfaceErrorCode", "SurfaceResult",
  "SURFACE_SCHEMA_VERSION",
]

consumed_capabilities = [
  "FieldGraph", "FieldId", "FieldType", "FieldValue", "EvalContext", "FieldError",
  "Vec3", "Vec4", "Mat4",
  "StableHash", "BinaryWriter", "BinaryReader", "SchemaVersion", "KernelError", "Ratio",
]

[[proof_exports]]
export = "Surface"
must_reference = ["FieldGraph", "StableHash"]

[[proof_exports]]
export = "SurfaceRequirements"
must_reference = ["FieldGraph", "EvalContext"]

[[proof_exports]]
export = "SurfaceLayer"
must_reference = ["FieldGraph", "Ratio"]
```

## Existing code involved

| Path | Why |
|---|---|
| `modules/axiom/src/material.rs:31` | today's 7-field `Material` — the vocabulary to subsume |
| `modules/axiom-resources/src/material_data.rs:39` | the resource tier |
| `modules/axiom-render/src/render_material.rs:29` | the render tier, and the `ratio_lit!` macro idiom |
| `modules/axiom-render-pipeline/src/render_pipeline_api.rs:391,413` | where `opacity` folds into alpha and `roughness` becomes `1.0 - roughness` specular |
| `crates/axiom-host/src/frame_packet.rs:151` | `FrameDrawItem` — the flattened target, and its comment explaining why there is exactly one free specular lane |
| `crates/axiom-host/src/frame_capability.rs` | `RenderCapability`, 12 bits, and `BackendCapabilityProfile` |
| `modules/axiom-render/src/render_pipeline_kind.rs` | `BASIC_LIT = 1`, `UNLIT = 2` — the unwired seam this manifest finally gives meaning |
| `docs/specs/SPEC-11-3d-scene-surface.md` | *"Resist PBR scope creep"* — still binding |

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-surface/{Cargo.toml, layer.toml, ARCHITECTURE.md}` | create |
| `crates/axiom-surface/src/lib.rs` | create |
| `crates/axiom-surface/src/surface.rs` | create |
| `crates/axiom-surface/src/surface_builder.rs` | create |
| `crates/axiom-surface/src/channel.rs` | create |
| `crates/axiom-surface/src/binding.rs` | create |
| `crates/axiom-surface/src/layer.rs` | create |
| `crates/axiom-surface/src/lighting_model.rs` | create |
| `crates/axiom-surface/src/requirements.rs` | create |
| `crates/axiom-surface/src/surface_error.rs` | create |
| root `Cargo.toml` | add `"crates/axiom-surface"` to `members` |
| `docs/engine-datafication.md` | **modify** — §7 Frontier and §10 non-goals, see below |

## Dependencies on earlier manifests

**`03`** (the evaluator must exist, because `Surface` offers CPU channel
evaluation and because `07` depends on it). May run **in parallel with `05`** —
no file overlap.

## Public API / data contracts

### The channel set — closed, and this is the whole point

```rust
#[repr(u16)]
pub enum SurfaceChannel {
    BaseColor = 0,     // Vec4, linear RGBA
    Roughness = 1,     // Scalar 0..=1
    Metallic = 2,      // Scalar 0..=1   <- does not exist in the engine today
    Normal = 3,        // Vec3, tangent space, or derived from a height Scalar
    Emission = 4,      // Vec4, linear RGB radiance (a is ignored)
    Opacity = 5,       // Scalar 0..=1
    Displacement = 6,  // Vec3, object space, VERTEX stage
}
```

Seven channels. A closed `#[repr(u16)]` enum whose discriminant indexes a `const`
table — the same shape as `FieldOp` and `TextureOp`. **This closedness is what
keeps the design inside `docs/engine-datafication.md`'s stated non-goal**: the
model is fixed and parameterised by data, not open and extensible at runtime.

```rust
pub struct ChannelBinding { /* tagged struct: constant | field */ }
impl ChannelBinding {
    pub fn constant(value: FieldValue) -> Self;
    pub fn field(graph: FieldGraph) -> Self;
}
```

`ChannelBinding` is a **tagged struct**, not a data-carrying enum (Branchless
Law; the `RenderCommand` precedent).

### The surface

```rust
pub struct Surface {
    bindings: [ChannelBinding; 7],
    lighting: LightingModel,
    layers: Vec<SurfaceLayer>,
}

pub struct SurfaceLayer { surface: Surface, mask: ChannelBinding /* Scalar */, blend: LayerBlend }

#[repr(u16)]
pub enum LayerBlend { Over = 0, Add = 1, Multiply = 2 }

#[repr(u16)]
pub enum LightingModel { Unlit = 0, Lambert = 1, LambertSpecular = 2 }
```

Layering resolves to, per channel, `mix(under, over, mask)` for `Over` and the
obvious forms for `Add`/`Multiply` — **defined as `FieldGraph` composition**, so
a layered surface flattens into a single graph per channel. That is what makes
the brief's `painted_metal = metal_base + paint_layer(mask) + scratch_layer(mask)
+ dirt_layer(mask)` expressible without any new primitive: each layer is a
`Surface`, each mask is a scalar field, and the flattening is `Mix` nodes.

**Layer nesting is bounded.** Cap at 4 (`MAX_LAYERS`), because a layered surface
flattens into one graph and `MAX_NODES = 256` is the real budget. Exceeding it is
a `SurfaceErrorCode::LayerBudgetExceeded`, not a silent truncation.

### `LightingModel` is the smallest extensibility point for lighting

The brief asks for *"materials that can participate differently in lighting
without turning every material into arbitrary raw shader code."* The answer is a
three-variant discriminant, not a programmable lighting hook. Justification and
the wiring are `11`; this manifest lands the discriminant. `Unlit = 0` is what
finally gives `RenderPipelineKind::UNLIT = 2` something behind it, and it removes
the need for the `glowOverlay` hack the casino app documents at length
(`baseColor: [0,0,0,1]` to fake an emissive-only surface).

### Normal from height — the derivation, not a derivative op

`SurfaceChannel::Normal` may be bound to a `Vec3` field directly, **or** derived
from a `Scalar` height field:

```rust
impl SurfaceBuilder {
    pub fn normal_from_height(self, height: FieldGraph, offset: Meters, strength: Ratio) -> Self;
}
```

This *constructs* a `Vec3` field by finite differences — four `Point`-offset
samples of the height graph composed into a normal — at a **caller-supplied
offset**. No screen-space derivative operator exists in the algebra, deliberately
(`01`), because `dpdx`/`dpdy` are backend-specific, absent on CPU and Canvas2D,
and already the cause of a real mobile-GPU NaN defect in this engine. The
existing `TextureOp::HeightToNormal` and
`crates/axiom-mesh-ops/src/implicit_surface.rs`'s central-difference gradient are
the two precedents; this is the same technique expressed in the algebra.

### Coordinate convention — fix it here, once

A surface's channel graphs are evaluated with `EvalContext::point` in **object
space**. Rationale: a world-space pattern swims when the object moves; object
space is what makes a boulder's noise ride with the boulder. Triplanar projection
is then *authorable* — three `Component`/`Compose` samples blended by
`Abs(Normal)` weights — and needs no new primitive. Say this in `ARCHITECTURE.md`;
it is a contract `07` and `08` both depend on.

### `SurfaceRequirements` — the backend-neutral half of the rejected shader IR

```rust
pub struct SurfaceRequirements {
    inputs: SurfaceInput,        // bitset: Point | Uv | Normal | Time
    varying_channels: u16,       // bitset over SurfaceChannel: which are non-constant
    has_displacement: bool,
    param_count: u16,
    node_count: u16,
}
impl Surface { pub fn requirements(&self) -> SurfaceRequirements; }
```

This is derived by walking the bound graphs — it is not authored. It is what a
backend checks against `BackendCapabilityProfile` **before attempting to lower
anything**, which is the brief's "determine backend support before attempting to
render it". `00-architecture-findings.md` §3C explains why this is a method here
rather than a separate IR stratum.

### Digest

`Surface::digest() -> StableHash` over canonical bytes: schema stamp, the seven
bindings in channel order, the lighting model, then the layers. **A parameter
value change does not alter the digest** — it lives in `FieldParams`, per `01`.
This is the property that makes the digest a safe program-cache key in `09` and
that prevents variant explosion from parameter animation.

## Documentation change required

`docs/engine-datafication.md` must be amended, in this manifest, to record the
reading in `00-architecture-findings.md` §5:

* §7 Frontier — mark the "Data-described render graph + material/lighting model
  (cap)" item as **in progress**, referencing this directory.
* §10 non-goals — keep *"A data-described shader-graph VM"* as a non-goal, and
  add one clarifying sentence: the ban is on an **open, runtime-extensible op
  vocabulary interpreted at frame time**; a closed algebra fixed in Rust and
  lowered at the preparation barrier is *"parameterizing a closed model"* and is
  in scope.

**Do not land this work silently against a written non-goal.**

## Explicitly excluded

* **No PBR.** `metallic` is a *channel*, not a BRDF. The shader remains
  Blinn-Phong with `SPECULAR_POWER = 48.0` until `11` says otherwise, and
  `SPEC-11`'s *"Resist PBR scope creep"* still binds.
* **No transmission, no subsurface, no clear-coat, no anisotropy.** The brief
  lists "opacity / transmission where supported"; opacity is supported (alpha
  blending is live), transmission is not, and adding a channel nothing can render
  is the "engine capability nothing composes is debt" failure that got commit
  `454707c0` reverted.
* **No texture sampling channel.** A `Surface` binds fields, not images. Image
  input is a later, separate decision with real capability consequences —
  Canvas2D cannot sample at all.
* No WGSL, no stages, no bindings, no varyings — those are `07`/`08`.
* No decals, no environment mapping, no post-processing graphs.

## Determinism requirements

`Surface` is a value. Same construction → same bytes → same digest, on every
target. Flattening a layered surface is a pure function and must be idempotent
and order-stable.

## Serialization requirements

`SURFACE_SCHEMA_VERSION = SchemaVersion::new(1, 0)`. Canonical little-endian,
built on `FieldGraph`'s bytes. Committed golden bytes + digest for one layered,
multi-channel surface. Truncation at every prefix must fail cleanly.

## Testing requirements (100%)

* Every channel bound as a constant and as a field.
* Every `LayerBlend` and every `LightingModel`.
* `normal_from_height` produces the expected normal for a known ramp — assert
  against a hand-computed value, not just non-vacuity.
* `requirements()` reports exactly the inputs a graph reads: a surface reading
  only `Uv` must not claim `Point`.
* Layer budget exceeded is rejected with the node id / layer index.
* Digest stability: same surface → same digest; changed structure → different
  digest; **changed parameter value → same digest** (the load-bearing test).
* Flattening a 3-layer surface yields a graph whose CPU evaluation equals the
  hand-composed `Mix` chain.
* Round-trip through bytes.

## Architecture tests

`cargo xtask check-architecture` — new layer, all seven layer rules.
`engine_genuine_dependency` must see real references to `axiom_field`,
`axiom_math`, `axiom_kernel`.

## Performance risks

* Flattening layers multiplies node count. With `MAX_LAYERS = 4` and
  `MAX_NODES = 256`, an over-eager author hits the cap — which is the correct,
  loud failure. Do not raise either cap to make a scene fit.
* `Surface` holds `Vec<SurfaceLayer>` and each layer holds a `Surface` — a
  recursive value type. Keep it bounded and do **not** write a recursive
  flattener (`engine_no_recursion`); flatten iteratively over a bounded worklist.
* `Surface` must not be cloned per frame. It is preparation-time data addressed
  by id afterwards; state this in `ARCHITECTURE.md` and enforce it in `06`.

## Migration considerations

None yet — nothing consumes `Surface` until `06`. `modules/axiom/src/material.rs`
is untouched by this manifest.

## Completion criteria

1. `crates/axiom-surface` exists and is a legal layer.
2. Seven channels, three blends, three lighting models, bounded layering.
3. `requirements()` is exact and tested.
4. Digest is stable under parameter change and unstable under structure change.
5. `docs/engine-datafication.md` amended as specified.
6. `cargo xtask check-architecture` exits 0; coverage 100/100/100; no dylint
   count rises.

## Validation commands

```sh
cargo build -p axiom-surface
cargo test -p axiom-surface
cargo xtask check-architecture
cargo test --workspace
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 5.** Parallel with `05`. Owns `crates/axiom-surface/**` plus one root
`Cargo.toml` members line and `docs/engine-datafication.md` — coordinate the
`Cargo.toml` edit with `05` if `05` also adds a member (it does not).
