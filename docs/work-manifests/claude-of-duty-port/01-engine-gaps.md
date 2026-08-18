# The shader/material/field stack: what it gives, what it blocks

Audit of `crates/axiom-field`, `crates/axiom-surface`, `crates/axiom-proc-texture`,
`modules/axiom-gpu-backend`, `apps/shader-crucible` at `9b43ae5e`.

**This is a code port.** The source subsystems are ported into Axiom as code.
Reproducing the reference frame is how we *verify* the port is faithful — it is
not the method.

## What the new stack gives us for free

Substantially more than expected. The manifests in
`docs/work-manifests/shader-material-field-system/` are implemented, not planned.

- **A closed, typed shader IR with a proven-correct WGSL compiler.** 27
  operators, a CPU reference evaluator that *is* the semantic definition, and
  per-operator CPU↔GPU parity tested on a real adapter with per-operator
  measured tolerances — where a tolerance more than 10× looser than the hardware
  needs is itself a failure. We will not have to debug "the shader disagrees
  with my CPU bake."
- **Real Perlin gradient noise and fbm, bit-identical on both sides**, including
  a hand-built 64-bit FNV-1a in WGSL (WGSL has no `u64`).
- **Content-addressed program identity.** `Surface::digest()` is structural and
  deliberately excludes parameter *values*, so retuning a parameter cannot force
  a recompile. Two identical materials authored independently collapse to one
  pipeline.
- **Zero mid-frame compilation, structurally.** Everything compiles inside a
  `PreparationTask`; a cache miss renders a fallback and *reports* rather than
  compiling. This is the whole of `core/prewarm.js` solved by construction.
- **Shadow mapping that already works** — 5×5 PCF, ortho volume fitted to the
  frustum's bounding sphere so it neither shimmers while steering nor falls off
  at distance.
- **A post-chain scaffold already in place** — bright-pass, separable blur,
  composite, colour grade, upscale-for-free. The plumbing for an HDR chain
  exists; only the buffer format is wrong.
- **Texture infrastructure that is already good** — full mip chains with correct
  per-format (sRGB vs linear) averaging, anisotropy negotiated against the
  device's real maximum, a real normal-map slot with a Mikkelsen tangent frame
  and a degenerate-UV NaN guard.
- `apps/shader-crucible` is an honest working reference for the whole path,
  including a hand-rolled browser loop, and it documents four limitations it
  refuses to hide.

## The four blockers that stop the port before aesthetics

**G1 — No float render target.** The scene target is 8-bit sRGB *by choice on
every arm*. Nothing above 1.0 survives to the post chain, bloom cannot rank two
highlights, and the tonemap runs on already-clamped values. Being fixed now
(`RenderCapability::HdrTargets` + declared degradation).

**G12 — `axiom-windowing` cannot carry a surface at all. FIXED.** Its GPU arm
called `present_frame_result`, which passed `&[]` for the program slice and `0.0`
for surface time, and it never called `prepare_surfaces` — so **any app using
`App::run` rendered every authored surface as its constant fallback**, no matter
what it authored. Now:

- `present_frame_result` takes the per-batch program slice and a kernel
  `Seconds`, gated through the prepared set exactly as the packet path gates it,
  so a backend that prepared nothing still writes an exact zero.
- `WindowingApi::set_surfaces` / `set_material_programs` hand the driver the
  authored set and the `(material id, program)` table. A batch is one
  `(mesh, material)` pair and a material names at most one surface, so the
  program is recovered from the material id the batch already carries — no run
  loop's frame tuple had to grow a lane.
- The driver's binder compiles the set onto the device it binds (and re-prepares
  it on a device-loss rebuild), and the idle-frame gate is held open when any
  authored surface reads the frame clock.
- `App::surfaces` puts the catalog half of that compilation inside the engine's
  own `axiom_runtime::PreparationTask`, before `RuntimeState::Prepared`; the
  device half runs at the bind, strictly before the first frame. Nothing
  compiles inside a frame, and an overflowing set fails the barrier loudly.

`GpuBackendApi::program_degradations` reports a batch-path cache miss from the
same rule the packet path uses.

**G11 — Skinned geometry gets no surface program.** `SkinnedGpuDraw` carries no
`surface_program` lane, and the skinned pipeline already binds all 16
WebGL2-guaranteed vertex attributes (having dropped emissive *and* specular to
fit). For this port that means **soldiers and the viewmodel — the two things the
player looks at most — cannot use the material system at all.** The skinned
`joint_base` vec4 has 3 free lanes; that is the way in.

**G14 — No per-frame parameter write.** Parameters are written once at the
barrier. `surface_parameter_bytes()` exists on the API but has no caller outside
tests. So "animating a parameter is a uniform write" is true in principle and
has no path in practice.

## Two genuine defects (not stated limitations)

**G4 — Authored normals are silently discarded.** `scene_wgsl.rs:454` reads
`select(surface.normal, textureSample(normal_tex, ...) * 2 - 1, caps & CAP_NORMALMAP)`.
The GPU backend's default profile is `all()`, which includes that bit — so on the
default path a surface's authored normal is thrown away in favour of a normal-map
texture that, for materials without one, is a 1×1 flat `(128,128,255)`.
`normal_from_height` is dead on arrival. The two normals must **compose**, not
`select`.

**G3 — `Roughness` is inert and undocumented.** It reaches `SurfaceOut` and is
read by nothing; the shader's specular strength comes from the instance-stream
lane derived from the *legacy* `Material::roughness`. `Metallic`'s inertness is
documented; `Roughness`'s is not. Two of seven channels are decorative.

Also **G16**: baked field textures are written linear and bound as
`Rgba8UnormSrgb`, so a baked tile reads darker than the same graph rendered live.

## The load-bearing porting decision

**The field algebra is the wrong home for the runtime material shader, and the
right home for bake-time texture generation.**

The field algebra has no control flow, no comparisons, no loops, no division, no
derivatives, and no texture sampling — all deliberate, and the branchlessness is
the Branchless Law itself, so it is immovable. Budgets are 256 nodes per graph,
**256 nodes per whole surface across all channels and layers**, 4 layers, 64
distinct programs.

The source's runtime material shader needs exactly the things the algebra
refuses: parallax occlusion mapping is a bounded *loop* with a linear refine,
de-tiling needs `textureGrad` with explicit derivatives, triplanar needs nine
texture fetches, and every generator is far larger than 256 nodes.

So the port splits along the same seam the source already has:

- **Bake time** — the 19 procedural surface generators are straight-line noise
  math with no sampling and no derivatives. They belong in the field/proc-texture
  path, and the node budgets need raising to hold them.
- **Run time** — POM, triplanar, de-tiling, the detail and macro layers, the
  weathering stack, curvature wear and the BRDF belong in hand-written WGSL in
  `axiom-gpu-backend`, which is where a BRDF belongs anyway.

That is not a compromise. It mirrors how the source is built: a GPU bake to a
render target, then a runtime shader that samples the result.

## Remaining gaps, in dependency order

- **G2** — no PBR BRDF. Blinn-Phong with a global `SPECULAR_POWER = 48.0`, no
  Fresnel, no GGX/Smith, no energy conservation. Needs a fourth `LightingModel`
  landed together with the BRDF in both backends.
- **G8** — no MRT, no G-buffer, no depth prepass, no velocity buffer. One colour
  attachment, one depth. Blocks GTAO, SSR, TAA, motion blur and decals.
- **G9** — 16 forward lights, two types, one shadow-casting directional, one
  cascade, hardcoded point attenuation, no light range or spot cone, no
  clustering. An FPS with muzzle flashes will exhaust this.
- **G10** — the shadow pass runs no vertex program, so displaced geometry casts
  an undisplaced shadow.
- **G13** — no surface registry. `Material::from_surface` keeps only the digest
  and drops the `Surface`, so an app still declares its authored set separately
  (`App::surfaces`). **The hand-syncing is gone** — the set and the materials are
  joined by the surface's own content digest, so a mismatch is impossible to
  author — but the *declaration* is still a second list. Closing it means
  `Material` owning its `Surface` and therefore giving up `Copy`, which ripples
  through every app; separable from G12 and deliberately not done with it.
- **G15** — the budgets above.
- **G17** — the Canvas2D arm is a per-triangle centroid sampler with no textures
  and no point lights; its declared policy is already "legibility, not parity."
  HDR/PBR work is GPU-only and the software arm's divergence widens.

## Revised order

1. HDR targets (G1) — in flight
2. ~~Windowing carries surfaces (G12)~~ — done; every later change is now visible
   through the normal app path
3. Compose authored normal with normal map (G4) + document/wire `Roughness` (G3)
4. Per-frame parameter writes (G14)
5. Surface program lane on skinned draws (G11)
6. Cook-Torrance `LightingModel` + BRDF in both backends (G2)
7. Raise node/program budgets (G15); port the 19 generators to the bake path
8. MRT + prepass (G8); then GTAO → contact → SSR → TAA → motion blur → DOF
9. Lights: range, spot cone, cascades (G9)
