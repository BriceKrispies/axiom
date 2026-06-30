# SPEC-11 — 3D scene authoring surface

> Status: Landed (2026-06-30, fully). See README footnote ⁴ and [`../reports/SPEC_VS_IMPL_GAP_AUDIT.md`](../reports/SPEC_VS_IMPL_GAP_AUDIT.md).
> Landed: `Mesh::Cylinder` + `Material` `emissive`/`roughness`/`opacity` fields + the hemisphere-ambient term (GPU shader and canvas2d `hemisphere_ambient`); `@axiom/game` projects `createMesh`/`createMaterial`/`setCamera3D` (with look-at `target`)/`addLight` and the `v3`/`mat4`/`quat` namespaces — **routed to the native `MathApi`, no TS math twin**. The §7 proofs are landed: the end-to-end render-one-frame slice ("nova-roll": cube + cylinder + camera + light) and the GPU↔canvas2d backend-parity test for a cylinder + emissive draw, both driven via `axiom-shot`.
> Landed (translucency + custom geometry): `opacity` now **affects the rendered pixel** — it folds into the per-draw alpha at the `axiom-render` layer (`material_base_color`: `base.a × opacity`) so both backends blend it, translucent draws **sort back-to-front** (`draw_order.rs`, deterministic stable sort), and canvas2d src-over composites; authored `createMaterial` emissive/roughness/opacity reach `RenderMaterial` end-to-end (the umbrella `Material → MaterialAsset` path no longer drops them). Author-supplied **`MeshData`** (`createMeshData(positions, normals, uvs, indices)`) is landed, validated and resolved through the **same** geometry pipeline as the catalog primitives (no special render path).
> Contract: §11   Vocabulary: Procedural geometry, Perspective camera, 3D mesh raster, Materials, Lighting   Determinism: presentation

## 1. Summary

The author-facing surface for a **true-3D game**: spawn meshes, give them
materials, place a perspective camera, add lights, and let the retained scene
graph draw them. This is Axiom's **strongest** subsystem natively — a z-buffered
perspective rasterizer (GPU `wgpu` + software canvas2d), directional PCF shadows,
distance-attenuated point lights — but **none of it is reachable from an author**:
the `createMesh`/`createMaterial`/`setCamera3D`/`addLight` TS surface is absent,
and three native gaps stand between today's catalog and the contract's wording.

Of the 11 games only **nova-roll** is genuinely 3D, so this is a completion +
projection of an already-built area, not a green-field subsystem. The work is
**not** "build 3D"; it is "close three small native gaps and expose the existing
3D core across the wasm boundary."

## 2. Current state (verified)

This is `have`, broadly. Present and working:

- **Perspective camera.** `axiom-scene::SceneApi::add_perspective_camera(math,
  node, fovy_radians, aspect, near, far)` (validated via
  `MathApi::mat4_perspective`), `camera_projection_matrix`. Right-handed,
  contract-shaped already.
- **Mesh primitives.** `axiom::Mesh` enum — `Cube`, `Plane`, `Sphere` (unit,
  scaled by the node `Transform`). The discriminant indexes a generator table in
  `axiom-resources`; adding a variant means adding its generator at the matching
  index.
- **3D mesh raster.** GPU path (`axiom-gpu-backend::scene_renderer`) does
  perspective-divide z-buffered rasterization with a directional **shadow depth
  pre-pass** + 3×3 PCF lookup and per-light loop; software path
  (`axiom-canvas2d-backend`, `software_rasterizer` + `canvas_depth_cue`) is a
  depth-buffered rasterizer with a depth-cue (fog) post pass. Both real.
- **Lighting.** `axiom-scene` `add_directional_light` / `add_point_light`
  (`color: Vec3`, `intensity: Ratio`); `axiom-render::RenderLight`
  {`Directional`|`Point`}. The shader applies a flat ambient term (`base.rgb *
  0.12`), per-directional **PCF-shadowed** diffuse, and per-point **distance
  attenuation** (`1/(1 + 0.09d + 0.032d²)`).
- **Materials.** `axiom::Material` = base `Color` + optional albedo `Texture`;
  `axiom-render::RenderMaterial` = `id`, `base_color: Vec4` (the `w` channel is an
  opacity slot), `texture_id`. Final colour = albedo × base × vertex colour.
- **3D math.** `axiom-math` already exposes `Vec3`/`Vec4`, `Mat4`, `Quat`,
  `Transform` with full algebra (perspective/look-at, axis-angle, TRS compose) —
  see SPEC-03 §2. The native types the contract's `v3`/`mat4`/`quat` project
  already exist.

Gaps to close (all small, all named by contract §11):

- **No `cylinder` primitive.** `Mesh` is Cube/Plane/Sphere; contract §11 names
  `"box" | "sphere" | "cylinder"`. (`"box"` = `Cube`.)
- **No `emissive`, no `roughness`.** `Material` carries base colour + texture
  only; contract `createMaterial` names `emissive?` and `roughness?`. The `Vec4.w`
  **opacity** slot exists in `RenderMaterial` but the umbrella `Material` exposes
  no opacity setter.
- **No hemisphere lighting term.** Ambient is a single flat scalar (`0.12`); the
  vocabulary's "hemisphere" sky/ground gradient term is absent.
- **Opacity does not yet affect the pixel — but not because blending is off.**
  The GPU main pass now uses `wgpu::BlendState::ALPHA_BLENDING` (`scene_renderer.rs`
  `blend_state`; SPEC-04 §2 closed the old hardcoded-`REPLACE` gap). The remaining
  defect is that **`opacity` is never plumbed into the shader's `base.a`**: the
  fragment shader computes `base = albedo * in.color` and returns `vec4(lit,
  base.a)`, where `in.color` is `vertex_color * instance_color` — and the
  material's separate `opacity` field is not threaded into that instance/material
  color alpha (the app sets the instance color from the material's base-color `w`,
  not from `opacity`). So a translucent material renders opaque because its alpha
  never reaches the fragment, not because the pipeline overwrites. The fix is to
  carry `opacity` into `base.a`; 3D translucency may additionally want
  back-to-front draw ordering — but it no longer waits on a blend-state change.
- **The whole TS 3D surface is absent.** No `createMesh`, `createMaterial`,
  `setCamera3D`, `addLight`, no `v3`/`mat4`/`quat` namespaces (consistent with
  SPEC-00: 0 contract entry points exist in TS today).

## 3. Architectural placement

No new module — this is completion + projection of an already-strong area. The
cross-module wiring (scene → render → GPU) already lives where the Module Law
puts it: in the app / render-pipeline composition tier (SPEC-00), never inside an
isolated engine module. Three small extensions plus one projection:

1. **`cylinder` primitive — extend `axiom` (`Mesh`) + `axiom-resources`.** Add
   the `Cylinder` variant and its generator at the matching table index. Legal:
   the umbrella owns the `Mesh` value vocabulary and already bridges it to a
   resources generator (the same shape `Sphere` uses); the tessellation lives in
   the resources layer beside the other primitive generators, the lowest correct
   home. No new module.
2. **`emissive` / `roughness` / `opacity` — extend `axiom::Material` +
   `axiom-render::RenderMaterial`.** Add the fields to the umbrella `Material`
   description and carry them on the neutral `RenderMaterial` contract so a draw's
   receipt captures them. Legal: `RenderMaterial` is the render module's own
   neutral data contract (it already carries `base_color`/`texture_id`); widening
   that record is the render module's job, and the umbrella owns the authoring
   `Material`. **Resist PBR scope creep** (§9): emissive + roughness + opacity are
   the *catalog* fields the contract names — not a full metallic/IBL model.
3. **Hemisphere term — extend the shader in `axiom-gpu-backend` (and the
   canvas2d depth-cue/software path for parity).** Replace the flat `0.12` ambient
   with a sky/ground hemisphere gradient driven by surface normal. Legal: this is
   presentation shading inside the platform-facing backend that already owns the
   lit pipeline; nothing new crosses a module boundary. The hemisphere colours are
   a render input the app supplies (a scene/light parameter), not new geometry.
4. **Opacity actually affecting the pixel — the blend state has landed; the
   remaining work is plumbing.** SPEC-04 already grew the GPU alpha-blend state
   (`ALPHA_BLENDING` on the main pass), so the old "needs a blend path" dependency
   is **closed**. What remains is to thread the material's `opacity` into the
   shader's `base.a` (today `base.a = albedo.a * instance_color.a`, and `opacity`
   never reaches the instance/material color alpha). This is a render-pipeline /
   backend plumbing fix inside the pass that already owns the lit shader — not a
   blend-state change and not a second blend path. Until that plumbing lands,
   `opacity` is carried on the receipt but renders opaque (documented, not silently
   dropped).
5. **TS projection — `@axiom/game` SDK + `apps/axiom-game-runtime` (SPEC-00).**
   `createMesh`/`createMaterial`/`setCamera3D`/`addLight` marshal to the existing
   scene/material/mesh facades across the wasm boundary; `v3`/`mat4`/`quat` route
   to the **native** `MathApi` (one deterministic source of truth — SPEC-03 §3.2),
   never a re-implemented TS twin. This is **projection only**: every native type
   `v3`/`mat4`/`quat` need already exists in `axiom-math`.

## 4. API surface

### 4.1 Native

`axiom` (umbrella value types — new variant + new material fields):

```rust
pub enum Mesh { Cube, Plane, Sphere, Cylinder }   // Cylinder added at table index 3

impl Material {
    pub const fn lit(base_color: Color) -> Self;            // unchanged
    pub const fn with_emissive(self, emissive: Color) -> Self;   // new
    pub const fn with_roughness(self, roughness: Ratio) -> Self; // new (0 = mirror-smooth..1 = matte)
    pub const fn with_opacity(self, opacity: Ratio) -> Self;     // new (1 = opaque; blends only after SPEC-04)
}
```

`axiom-render` (neutral contract — widened record):

```rust
impl RenderMaterial {
    // base_color unchanged; emissive/roughness/opacity carried on the receipt.
    pub const fn new_lit(id: u64, base_color: Vec4, emissive: Vec3,
                         roughness: f32, opacity: f32, texture_id: u64) -> Self;
    pub const fn emissive(&self) -> Vec3;
    pub const fn roughness(&self) -> f32;
    pub const fn opacity(&self) -> f32;
}
```

Camera and lights are **unchanged** — `add_perspective_camera`,
`add_directional_light`, `add_point_light` already match the contract; they are
projected, not re-cut.

### 4.2 TS authoring projection (contract §11)

```ts
function createMesh(kind: "box" | "sphere" | "cylinder" | MeshData): MeshId;
function createMaterial(spec: {
  baseColor: Rgba; emissive?: Rgba; roughness?: number; opacity?: number;
}): MaterialId;

// Renderable component: Transform + a mesh + a material.
interface Renderable extends Component { mesh: MeshId; material: MaterialId }

function setCamera3D(cam: { position: Vec3; target: Vec3; fovY: number; near: number; far: number }): void;
function addLight(light:
  | { kind: "directional"; direction: Vec3; color: Rgba; intensity: number }
  | { kind: "point"; position: Vec3; color: Rgba; intensity: number }
): Entity;

// 3D math — projected from native axiom-math (no TS re-implementation):
const v3:   { add; sub; scale; dot; cross; len; normalize; dist; lerp /* … */ };
const mat4: { identity; multiply; perspective; lookAt; invert; fromTRS /* … */ };
const quat: { identity; fromEuler; multiply; normalize; toMat4 /* … */ };
```

`setCamera3D({position, target, fovY, near, far})` builds the camera node
transform (look-at) + perspective intrinsics from the contract's flat record;
`addLight` returns the new light `Entity`. `MeshData` (author-supplied vertex
data) is an open question (§9) — the three named primitives land first.

## 5. Data contracts

- **`MeshId` / `MaterialId`** — opaque handles (contract `Handle`, §0.2), bound by
  the SPEC-00 handle table; never serialized into sim state.
- **`Renderable { mesh, material }`** — the component the author sets on a 3D
  entity (`axiom-scene::Renderable`, projected through the World facade, SPEC-02).
- **`RenderMaterial`** (neutral, `axiom-render`) — widened to carry
  `emissive`/`roughness`/`opacity` so the deterministic `RenderCommandList`
  receipt fully describes each draw.
- **Camera/light records** — the contract's flat `{position,target,fovY,near,far}`
  and the directional/point light variants; marshalled to the existing scene
  facade.
- **`Vec3`/`Mat4`/`Quat`** — plain number records crossing the boundary, mapping
  1:1 to native `axiom-math` types.

## 6. Determinism (presentation)

This is **presentation** class — the retained scene graph is the engine's
existing display model, listed here for contract completeness.

- The 3D surface is drawn from `onRender` (§17.5): mesh/material/camera/light
  values feed the rasterizer; **no value produced here re-enters sim**.
- The scene graph *holds state across frames* (it is retained, not immediate),
  but the transforms it reads are the **committed, propagated world transforms for
  the current tick** — written by `SceneApi::advance` / `update_world_transforms`
  during the fixed update (a sim-class operation, SPEC-02/03). Authoring (spawn a
  mesh, set a material) happens in sim; **drawing** it is presentation. The split
  is the same one SPEC-03 §6 draws for scene queries: the data is committed in
  sim, consumed in presentation.
- Lighting/shading math (hemisphere term, PCF, attenuation) is presentation-only
  and unconstrained by §17.6 — it never affects gameplay state.

## 7. Acceptance / proof

- **Native, 100% covered + branchless** (spine discipline):
  - `Mesh::Cylinder` resolves to well-formed geometry (positive vertex/index
    counts, the generator-table-index invariant test extended to 4 entries).
  - `Material` builder round-trips `emissive`/`roughness`/`opacity` (mutation-
    killing: each field read back distinct from its default); `RenderMaterial`
    accessors round-trip and equality requires every new field.
  - Hemisphere term: a parity/golden render asserts a normal facing the sky
    receives the sky colour and a downward normal the ground colour (a value test,
    not coverage theater).
- **Backend parity:** the GPU and canvas2d paths agree on a cylinder + emissive
  material draw (reuse the `axiom-shot` headless harness, both backends).
- **Opacity gate:** a translucent-material test asserts the `opacity` field is
  *carried* on the receipt today. The blend state has landed (SPEC-04 §2,
  `ALPHA_BLENDING`), so the remaining gate is the plumbing fix that threads
  `opacity` into the shader's `base.a` — until then the test annotates `opacity`
  as carried-but-opaque, no opaque-looking pass pretending to be transparent.
- **TS projection:** tsgo + Oxlint (branch ban) + 100% `node:test`. A cross-check
  asserts the `v3`/`mat4`/`quat` TS surface and native `MathApi` agree on a vector
  of sample inputs (no second implementation to drift). A slice test in
  `apps/axiom-game-runtime` authors a cube + cylinder + perspective camera + one
  directional light and renders one frame (the nova-roll smoke path).

## 8. Dependencies & order

- **Independent, land-now natives:** `Cylinder` primitive and the
  `emissive`/`roughness`/`opacity` material fields have no new dependency — extend
  the umbrella + render contract immediately.
- **Hemisphere term** depends only on the existing backends.
- **Opacity rendering:** the SPEC-04 alpha blend state has landed, so this no
  longer waits on a blend path. The field is carried now; rendering it correctly
  needs the plumbing fix that threads `opacity` into the shader's `base.a` (§3.4).
- **TS projection** depends on **SPEC-00** (handle tables, wasm boundary,
  `@axiom/game`), **SPEC-02** (`World`/`Entity`/`Renderable` component), and
  **SPEC-03** (the `v3`/`mat4`/`quat` namespaces are *declared* by SPEC-03 to be
  projected *here*). Contract §18 places 3D last — this rounds out the surface, it
  does not gate other specs.
- Consumed by: **nova-roll** (the one true-3D game); the rotating-cube /
  physics-crucible demos already exercise the native core.

## 9. Open questions

- **Cylinder tessellation params.** The other primitives are param-free (unit,
  Transform-scaled). A cylinder needs a radial-segment count (and cap topology).
  Lean: a fixed engine default (e.g. 16 segments, capped) baked into the generator
  — the contract's `"cylinder"` is a *named* primitive, not a parametric one;
  parametric meshes arrive via `MeshData`, below.
- **`MeshData` (author vertex data).** Contract §11 allows `createMesh(MeshData)`.
  Defer until a game needs non-catalog geometry; the three named primitives cover
  nova-roll. When it lands it is neutral vertex/index/normal/uv arrays through
  `axiom-resources`, not a new module.
- **PBR vs. the catalog floor.** Do materials grow toward a full PBR model
  (metallic, IBL, normal maps) or stay the contract's "basic-lit" minimum
  (baseColor + emissive + roughness + opacity)? **Lean: stay the catalog
  minimum.** The Vocabulary Law admits capability only under *proven pressure* —
  add metallic/IBL when a game demonstrably needs it, not speculatively. Roughness
  is carried as a single scalar feeding the existing diffuse term until a game
  forces a real specular lobe.
- **Opacity blend ownership.** Confirmed: the alpha-blend state belongs to
  SPEC-04 (the 2D surface owns `alpha`/blend). SPEC-11 must not fork a second
  blend path. Open: whether 3D transparency needs depth-sorted back-to-front draw
  ordering (a render-pipeline concern) or whether order-independent suffices for
  nova-roll's use.
- **Hemisphere colours' authoring surface.** Are sky/ground colours a scene-level
  ambient parameter the author sets, or fixed engine defaults? Lean: a scene
  ambient setter (small), defaulted so the term works with zero author input.
