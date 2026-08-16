# 09 — Program and pipeline caching, at the preparation barrier

## Objective

Give the engine its first content-addressed GPU program cache, and put every
shader compilation behind the existing `RuntimeState::Prepared` barrier so that
**no shader is ever compiled during a frame**. This is the manifest that
reconciles a generated-shader system with the renderer's twice-documented
anti-variant doctrine.

## Architectural placement

**Engine module: `gpu-backend`**, driven by the **layer `runtime`**'s existing
preparation phase. No new package; `axiom-runtime` is used, not modified.

## The problem this manifest exists to solve

The renderer's own comments state the doctrine twice:

* `modules/axiom-gpu-backend/src/post_chain.rs:462-466` — *"a frame that toggles
  either one on and off does not change which pipelines exist, so it cannot
  stutter on a pipeline the driver compiles the first time it is used."*
* `modules/axiom-gpu-backend/src/surface_encode.rs:82-84` — *"keeps one pipeline
  for both cases so a device cannot stutter compiling a second variant
  mid-session."*

Both were written after the fact, which reads as a lesson learned. And on the
browser's WebGL2 fallback path, `wgpu` cross-compiles WGSL→GLSL at pipeline
creation, so lazy variant compilation is a guaranteed mid-session hitch.

**The resolution is structural, not a compromise.** `crates/axiom-runtime`
already owns a startup preparation phase — `PreparationTask`,
`PreparationSchedule`, and `RuntimeState::Prepared`, whose stated invariant is
that *"the deterministic simulation cannot advance until a preparation phase has
completed successfully"*. Shader compilation is exactly the shape of work that
phase exists for: expensive, startup-only, producing runtime-ready in-memory
data. Compile every surface program there, and the doctrine is satisfied by
construction — not weakened.

## Existing code involved

| Path | Role |
|---|---|
| `crates/axiom-runtime/src/{preparation_task,preparation_schedule,runtime_state}.rs` | the barrier; `RuntimeState::Prepared` |
| `docs/work-manifests/startup-preparation/README.md` | the barrier's design record — read §2 |
| `modules/axiom-gpu-backend/src/scene_renderer.rs:1121-1160` | pipelines as **named struct fields**: `pipeline`, `shadow_pipeline`, `sdf_pipeline`, `sky: Option<SkyPass>`, `skinning: Option<Skinning>` |
| `scene_renderer.rs:1825-1838` | the draw loop; `set_pipeline` is hoisted **outside** the batch loop |
| `scene_renderer.rs:1686-1687` | *"on the WebGL2 path a draw costs ~52 GL calls"* |
| `scene_renderer.rs` `materials: HashMap<u64, wgpu::BindGroup>` | the only existing per-id GPU cache, built entirely in `new()` |
| `apps/burnt-rubber/src/preparation/textures.rs` | `PreparedTextures::generate()` — an existing preparation task to model on |
| `crates/axiom-kernel/src/stable_hash.rs` | `StableHash` — the cache key primitive |

## Files owned

| Path | Action |
|---|---|
| `modules/axiom-gpu-backend/src/surface_program/cache.rs` | create |
| `modules/axiom-gpu-backend/src/surface_program/compile.rs` | create |
| `modules/axiom-gpu-backend/src/scene_renderer.rs` | modify — pipeline selection in the draw loop |
| `modules/axiom-gpu-backend/src/gpu_backend_api.rs` | modify — the prepare entry point |
| `modules/axiom-windowing/src/windowing_api/**` | modify — call prepare at bind |

## Dependencies on earlier manifests

**`08`.** Should land after `10` and `11` if those change the emitted shader
shape — a cache keyed on a digest that does not cover displacement or lighting
model would be silently wrong. **Safest order: `08 → 10 → 11 → 09`.** If `09`
lands earlier, the cache key must be `(Surface::digest(), plan_version)` where
`plan_version` bumps on every emitter change.

## Public API / data contracts

### The cache

```rust
pub(crate) struct SurfaceProgramCache {
    programs: HashMap<u64, CompiledSurfaceProgram>,   // key = Surface::digest().raw()
}
pub(crate) struct CompiledSurfaceProgram {
    pipeline: wgpu::RenderPipeline,
    params_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}
```

**This is the engine's first content-addressed GPU cache.** Every existing cache
— `meshes: HashMap<u64, MeshBuffers>`, `materials: HashMap<u64, BindGroup>`,
Canvas2D's `MeshCache` — is keyed on a *caller-assigned* `u64`, so two
byte-identical resources upload twice. Keying on `Surface::digest()` means two
surfaces authored independently but computing the same thing **collapse to one
pipeline**, which is the only structural defence against variant explosion.

The key property that makes this work was designed in at `01`: **a parameter
value change does not alter the digest.** Animating a material parameter writes a
uniform; it never compiles anything. Verify this with a test that animates a
parameter for 100 frames and asserts the cache size never changes.

### Compilation happens only at the barrier

```rust
impl GpuBackendApi {
    pub fn prepare_surfaces(&mut self, surfaces: &[Surface]) -> Result<PreparedSurfaces, …>;
}
```

Called from the app's `PreparationTask`, before `RuntimeState::Prepared`.
**A cache miss during a frame is a hard error, not a lazy compile.** Report it
through `FrameSubmissionReport::degraded_features` and render the constant
fallback. That rule is what makes the doctrine hold; write it in the code as a
comment naming `post_chain.rs:462`.

### Bounding the cache

Cap the program count (start at 64) and fail preparation loudly on overflow. An
unbounded pipeline cache is the failure mode the doctrine warns about; a bound
that fails at startup is a design signal an author can act on, and it is why the
digest-collapse above matters.

### The draw loop change — the real cost

Today `set_pipeline(&self.pipeline)` is hoisted **outside** the batch loop
(`scene_renderer.rs:1825`), and groups 1 (`lights`) and 2 (`shadow_sample`) are
set once per pass (`:1826-1827`). Per-surface pipelines make the pipeline a
per-batch state change.

Mitigations, in order of preference:

1. **Sort draws by `(surface_program, mesh_id, material_id)`** so each program is
   set once per frame, not once per batch. The batcher is already a sort +
   `chunk_by` (`frame_packet_adapter.rs:32-49`) — extend the key, do not
   reintroduce a `HashMap`; that map was ~10% of a throttled frame before it was
   removed.
2. **Keep one shared `BindGroupLayout`** across all surface programs (decided in
   `07`), so groups 1 and 2 stay hoisted. A per-surface layout would un-hoist
   them, which is the expensive mistake.
3. **`surface_program == 0` keeps the existing hoisted fast path entirely**, so
   content that uses no surfaces pays nothing. Prove it with a GL-call count.

## Explicitly excluded

* **No lazy/on-demand compilation.** Ever. This is the point of the manifest.
* **No pipeline-derivative or async-compile APIs.** `wgpu`'s
  `downlevel_webgl2_defaults` path does not offer them portably.
* **No disk cache, no `.axpkg`, no persisted shader binaries.** Nothing is written
  to disk, IndexedDB, or any store — the same scope line the startup-preparation
  work drew. Programs are regenerated each launch.
* **No cache in `axiom-canvas2d-backend`** — it compiles nothing.
* **No eviction.** A bounded cache that fails loudly beats an evicting cache that
  stutters.

## Determinism requirements

* Same surface set → same cache contents, same program ids, same order.
* Preparation is deterministic and must not depend on iteration order of a map —
  compile in sorted digest order.

## Serialization requirements

None. Nothing persists.

## Testing requirements (100%)

* Two equal surfaces produce one cache entry; two different surfaces produce two.
* **Animating a parameter for N frames leaves the cache size unchanged** — the
  load-bearing test.
* A frame requesting an unprepared program reports a degraded feature and renders
  the fallback; it does **not** compile and does **not** panic.
* Cache overflow fails preparation with a structured error.
* GL/draw-call count for a surface-free scene is unchanged before and after.
* Program count after preparing burnt-rubber is asserted against an expected
  number, so a variant explosion shows up as a failing test rather than a slow
  frame.

## Architecture tests

`cargo xtask check-architecture`. `engine_no_large_files` on
`scene_renderer.rs` — it is 2650 lines against a 1000-line cap already in the
baseline at 0, so **do not add lines to it**; put the cache in
`surface_program/cache.rs` and touch the draw loop as little as possible.

## Performance risks

This manifest *is* the performance risk register for the whole design. The
concrete pressure points, each with its source:

| Risk | Current code | Mitigation |
|---|---|---|
| Pipeline switch per batch | `set_pipeline` hoisted at `:1825`; ~52 GL calls per draw on WebGL2 at `:1686` | sort by program; `0` keeps the hoisted path |
| Bind-group churn | every `create_bind_group` is in a `new()` today | one shared layout; buffers allocated at the barrier |
| Uniform write ordering | `post_chain.rs:245-251` — writes order against *submission*, not passes | per-program buffers or dynamic offsets, never one rewritten buffer |
| Variant explosion | no cache exists to explode yet | content-addressed key + hard cap + an asserted program count |
| Mid-session compile stutter | WGSL→GLSL cross-compile at pipeline creation on the fallback path | the preparation barrier; a frame-time miss is an error |
| Batch fragmentation | *"almost every draw carries its own mesh"* on burnt-rubber's road | measure before/after; `0` is free |
| CPU `Vec` churn in `record()` | `pack_lights`/`pack_sky`/`pack_sdf` each allocate per frame | do not add a per-frame per-surface pack; parameters are written at the barrier unless animated |

## Migration considerations

`GpuBackendApi` gains a preparation entry point that apps must call. Apps that do
not use surfaces need no change — that is the compatibility contract.

## Completion criteria

1. Surface programs compile only inside a `PreparationTask`, before
   `RuntimeState::Prepared`.
2. The cache is keyed on `Surface::digest()`, bounded, and asserted.
3. Parameter animation never changes cache size.
4. A surface-free scene's draw-call and GL-call counts are unchanged.
5. A frame-time cache miss degrades and reports; it never compiles.
6. Coverage 100/100/100; `cargo xtask check-architecture` exits 0; no dylint count
   rises.

## Validation commands

```sh
cargo test -p axiom-gpu-backend --features offscreen
cargo test --workspace
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
uv run scripts/localhost_servers.py start-app burnt-rubber --port 8085
uv run scripts/playwright_controller.py goto http://localhost:8085/
uv run scripts/playwright_controller.py console      # must be error-free
```

## Parallel safety

**Wave 9, width 1.** Owns the draw loop. Nothing else may touch
`scene_renderer.rs` concurrently.
