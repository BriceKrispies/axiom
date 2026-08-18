# Claude of Duty → Axiom: port manifest

Port the browser FPS at `C:\dev\Claude-of-Duty` (Three.js r180 + WebGL2, ISC
licensed, ~55k lines, zero art assets) onto Axiom, reproducing its rendered
frame as closely as the engine can be made to reproduce it.

Three.js is MIT, Claude-of-Duty is ISC. Both are freely portable with
attribution.

## The target

Not byte-equality — different shader source, different float ordering, different
driver. The target is **pixel-faithful**: same scene, same materials, same light
rig, same photometric scale, same grade, converged with `/visual-convergence`
against a reference capture until a side-by-side reads as the same frame.

**Reproduce the source faithfully, including its known defects.** Specifically
the viewmodel irradiance bug (below) is *in* the reference image; cloning the
frame means cloning the bug. Fixes land after parity is demonstrated, never
before.

## The governing split

- **Engine capability** → `crates/` and `modules/`. Branchless Law, Coverage Law,
  Layer Law, Module Law all apply. Expensive, and correct.
- **The game itself** → `apps/`. Outside the branchless and coverage gates by
  Axiom's own scope line. Ports at porting speed.

This is not a dodge; it is where Axiom's laws put composition roots. It means
only genuine engine primitives pay the gate tax.

## Placement — forced, not chosen

Four of the five new render capabilities land in `crates/axiom-host`. It is
already the render-contract layer: it owns `FramePacket`, `RenderCapability` /
`BackendCapabilityProfile`, `RenderTargetId`, `HostColorFormat`,
`MaterialTexture`, and the whole `Frame*` effect family.

| capability | placement | forcing law |
|---|---|---|
| HDR / half-float / depth formats, MRT attachments | layer `host` — new `HostAttachmentFormat` sibling type + a `RenderCapability` bit. **Landed `843577af`.** | Module Law #2 (5+ modules need it) + #9 (GPU APIs are host-only); Layer Law bans ceremonial layers |
| frame-graph pass description | layer `host` (vocabulary) + feature module `render-pipeline` (composition) | Module Law #2; `render` may not import `scene` |
| PBR params + BRDF | layer `surface` — add a `LightingModel` variant, landed *with* its BRDF | Module Law #2, argued verbatim in `crates/axiom-surface/layer.toml:10-13` |
| EV100 / photometric quantities | layer `host` (built on kernel `Ratio`/`Meters`/`Radians`); camera rig stays in module `scene` | kernel excludes domain quantities (`axiom-kernel/ARCHITECTURE.md:83-87`) |
| post-process operators | layer `host` (params + reference impl); realization in `gpu-backend` / `canvas2d-backend` | Module Law #2, stated at `frame_postprocess.rs:6-11` |

Existing precedent to imitate: `FrameBloom::tonemap` is a pure `const fn` in
`host` that the WGSL mirrors — one definition, testable without a GPU. Every new
operator follows that shape.

## The root blocker — RESOLVED, `843577af`

`modules/axiom-gpu-backend/src/post_chain.rs:18-35` argued against `Rgba16Float`
targets: the engine requests `downlevel_webgl2_defaults` on both browser arms to
keep them at capability parity, and half-float render targets are not guaranteed
there.

**Correction to the original reading of this:** those lines are a module *doc
comment*, not a code path. No check refused the format — the policy existed as
prose plus the absence of any HDR path. The fix therefore re-argues the prose and
supplies the mechanism; it deletes no guard.

**Correction two:** `RenderTargetId` does not exist anywhere in the repo. The
layer audit listed it among `host`'s capabilities; `crates/axiom-host/src/handles.rs`
has only `TextureId`, `FontHandle`, `TransformDepth`, `PaintId`.

Resolved by `RenderCapability::HdrTargets` (bit 13, appended above every mask the
main-pass WGSL reads, so no existing bit moved) plus a new sibling type
`HostAttachmentFormat`. Two decisions worth preserving:

- `Depth32Float` deliberately does **not** require the HDR capability. A float
  depth buffer is core on every arm; gating it would make cascaded shadows
  unavailable on devices that render them fine — an accidental capability split
  rather than a real one.
- No `FrameFeature` peer was added yet. A per-frame degradation report is keyed
  on a frame having *authored* something the backend could not honour, and no
  frame names an attachment format until the pass vocabulary exists. Reporting
  unconditionally would fire on every frame in the engine. It lands with step 2.

## What the source needs — the frame graph

18 ordered passes. Conventions: linear depth is positive view-space metres in
R32F (0 = sky); normals are octahedral, 2 half-float channels, view space;
velocity is a UV-space delta; every post pass is a full-screen triangle.

1. CSM depth, 3–4 cascades → R32F `sampler2DArray`, PSSM split λ=0.86,
   bounding-sphere fit, texel snap, extruded-cylinder caster cull (32-texel margin)
2. TAA jitter — Halton(2,3), 16 samples, ±0.5px, world camera only
3. MRT prepass → RGBA16F (oct normal + coverage + matId), RG16F velocity, R32F depth
4. GTAO — 3 slices × 8 steps, horizon-arc integral, quadratic step distribution,
   temporal reproject, separable bilateral
5. Contact shadows — 14-step screen-space march
6. SSR — 28-step geometric march + 5-step refine, half res, into *previous resolved frame*
7. Forward world pass → RGBA16F HDR
8. Viewmodel pass → separate scene/camera, own MSAA target, transparent clear
9. TAA resolve — dilated velocity, Catmull-Rom history, YCoCg variance clip (γ=1.25)
10. Motion blur — 16×16 tile-max + 12-tap reconstruction
11. ADS depth of field — prefilter/gather/combine, 32-tap golden-angle
12. Registered passes by order — sky volumetrics at −70
13. Viewmodel composite — FXAA-style edge filter, premultiplied over
14. Metering — log-luminance reduce to 1×1 RGBA32F
15. Bloom — Karis pyramid, soft-knee threshold on level 0 only, energy-preserving tent upsample
16. Composite — CA → chroma denoise → CAS → exposure → bloom → vignette → **AgX** → sRGB → 33³ LUT → grain → dither
17. FXAA (no-TAA path only)
18. Matrix bookkeeping for next frame's velocity

## The photometric contract — reproduce exactly

```
1 light-intensity unit = 1 framebuffer radiance unit = 25000 lux
```

Three's Lambert BRDF carries the 1/π, so a scattering integral that already
evaluates a radiance is written to the buffer **as-is and must not be multiplied
by π**. That π was once present and put the sky 1.65 stops over the surfaces it
lit — clouds darker than the gaps between them. Getting this constant wrong is
not recoverable by tuning.

Consequences that fall out rather than being dialled in: sun 5.12 units before
extinction, ~3.9 after; clear zenith sky ~0.06; sunlit stucco ~0.32; whole-sky
diffuse ≈15% of the sun.

Two acknowledged non-physical corrections in the source, both to be reproduced:
`SUN_KEY_GAIN = 1.55` and `MOON_ILLUMINANCE_NIGHT = 0.30`.

## The material forge

GPU bake to render target, nothing read back. Per surface, four full-screen draws
producing three textures:

```
albedo.rgb = base colour (sRGB)      albedo.a = height (or cutout mask)
orm.r = AO/cavity   orm.g = roughness   orm.b = metalness
normal.rgb = tangent-space, OpenGL +Y — Sobel of the height at strength relief/worldSize
```

Each surface is one function `owSurface(uv) -> albedo, height, roughness, metal, ao`.
All noise is **periodic** — the hash lattice wraps with `mod`, so a texture baked
over uv ∈ [0,1) tiles seamlessly. 19 surfaces plus a shared detail map (0.25 m)
and a shared 4-band macro map (32 m).

The physical rule that drives every metal: **bare metal is 1, and every oxide,
paint, dust or grime layer on top forces it to 0.**

An explicit Nyquist budget governs authoring: a term at frequency K lays 8K cells
across an N-texel bake; under ~5 texels per cell it bakes as white noise and mips
to flat grey. K is capped near 20–24 at 1024²; everything finer is delegated to
the detail map.

## What ports nearly verbatim

Plain math, data, DOM or WebAudio — no Three.js contact:

- `core/registry.js` (topo-sort + event bus), `core/rng.js` (xoshiro128\*\*),
  the `engine.step` accumulator loop, `config.js` data
- `weapons/`: `mathx.js`, `defs.js`, `clips.js`, `ballistics.js`, the viewmodel
  layer stack, the ADS solve, the two-bone IK and its pole-vector convention,
  and every dimension sheet in `parts.js` / `models/`
- **all of `audio/`** — 4,241 lines, zero Three.js; `ir.js` is a self-contained
  procedural reverb
- `world/`: the noise basis, layout data, palette data, the `[wear, grime, ao]`
  vertex-mask convention, seam interlocking, scatter distributions, bay-selection tables
- all of `ui/` except `minimap.js` and a 30-line `project()`

## What needs real reimplementation

- The Assembler geometry back end — merged static batches, instancing with
  per-instance `[wear, grime, ao]`, extrude-with-holes (the real-hole wall
  system), rounded-box, lathe
- `materials/` — the GPU texture forge and the runtime material shader
- The split-scene viewmodel composite
- The minimap depth bake

## What gets deleted rather than ported

Three.js pathologies with no analogue here:

- `core/prewarm.js` entirely
- `world/index.js` `_addBallast` / `_stabiliseLightCount` — these exist only
  because Three bakes visible-point-light count into its material program cache
  key, so one lamp crossing its cull radius recompiles every lit material
- The `owNoPrepass` / `owNoShadow` userData protocol

## Determinism

The source is already fully seed-driven: xoshiro128\*\* with SplitMix32
expansion, disciplined `fork()` per subsystem, position-hashed noise, and
deliberately pinned sub-seeds so editing one system cannot reshuffle the level.
The only `Math.random()` in the entire source is the root seed line, replaced by
a constant under `?capture=1`.

Make the root seed explicit and the whole port is reproducible for free — which
is also what makes rigorous frame-vs-frame comparison possible.

## The defect being reproduced deliberately

The viewmodel light rig delivers roughly 20× the irradiance per unit albedo that
the world does. Four view-space directionals summing to ~5.6 units arrive from
directions clustered around the camera, with no cascades in most poses, no
contact shadows, no interior gate, and only a 0.45 trim on the fill bands —
against a world that gets one shadowed sun and a deliberately starved indirect
budget. A plain black material in the view scene renders at L=110 against a
background of 91, purely from F0=0.04 specular.

The shipped workaround cheats every weapon albedo to a third of physical, which
caps material separation on the most-looked-at object in the game.

The correct fix is to drive the view rig from the world's irradiance budget with
an explicit exposure offset, or to subtract the specular-only floor. **Not yet** —
it changes the frame we are matching against.

## Physics: what actually blocks

The world authors box collision proxies separately from its visuals, so triangle
colliders are *not* on the critical path. What blocks is:

- capsule contacts — `contact_pair.rs:57-82`, the whole capsule row and (Box,Box)
  are `no_contact`
- swept capsule tests — none exist at any tier; the character controller is built
  entirely on "what did my capsule hit on the way there"
- capsule overlap — `OVERLAP_TABLE` maps capsule to unsupported

All three are `axiom-math` primitives (triangle, capsule, segment, closest-point,
swept tests), and all three are layer-level.

Separately, a live defect found during the audit: `physics_query.rs:161` declares
`RAY_TABLE: [RayFn; 4]` and indexes by shape kind, but `PhysicsShapeKind` has
five variants (`Heightfield = 4`) and `attach_heightfield_collider` is public.
Any raycast or `overlap_sphere` on a world containing a heightfield collider
indexes out of bounds and panics. `OVERLAP_TABLE` has the same 4-vs-5 mismatch.
The contact path was widened correctly to `[ContactFn; 25]`, so this is drift
between two dispatch sites.

## Dependency order

What unblocks what, not a schedule.

1. `RenderCapability::HdrTargets` + HDR/depth variants in `HostColorFormat`,
   with the Canvas2D degradation declared — unblocks every pass
2. Frame-graph pass vocabulary in `host`; make the implicit hard-coded pass
   order in the `Frame*` family explicit and ordered
3. MRT attachments + the prepass contract (oct normal, velocity, R32F depth)
4. `LightingModel::CookTorrance` in `surface`, landed with the BRDF in both backends
5. Cascaded shadows (R32F array target, PSSM splits, PCSS)
6. The post chain in dependency order: metering → tonemap (AgX) → bloom → TAA →
   GTAO → contact → SSR → motion blur → DOF → grade LUT
7. `axiom-math` capsule/segment/triangle + swept primitives; then capsule
   contacts and overlap in physics
8. Character controller, movement state machine, camera feel
9. The game app: world gen, weapons, ballistics, AI, audio, UI

Steps 1–6 are the visual critical path. 7–8 are the play critical path. They are
independent and can proceed in parallel.
