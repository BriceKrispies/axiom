//! The device half of the program cache: one pipeline, one parameter buffer and
//! one bind group per prepared surface program.
//!
//! [`crate::surface_program::cache`] decides *which* programs exist, in *what*
//! order, and *whether* a frame's draw named one. This turns that decision into
//! GPU objects. Everything here needs an adapter, which is why it is compiled
//! only on the `wasm32` / `offscreen` arms — the same gate `crate::mip_chain` and
//! `crate::texture_sampling` carry, and the reason the cache's semantics were
//! split out rather than living here.
//!
//! ## Every compile in this file happens at the preparation barrier
//!
//! [`SurfaceProgramCache::compile`] is called from
//! `crate::scene_renderer::SceneRenderer::prepare_surfaces`, which an app drives
//! from its `axiom_runtime::PreparationTask` before `RuntimeState::Prepared`.
//! There is no lazy path, no on-demand entry point, and
//! [`SurfaceProgramCache::program`] is a pure lookup that returns `None` rather
//! than compiling. That is the whole point of the manifest: `crate::post_chain`
//! states in writing that the renderer keeps one pipeline for a toggled feature
//! *"so a device cannot stutter compiling a second variant mid-session"*, and on
//! the browser's WebGL2 fallback `wgpu` cross-compiles WGSL to GLSL at pipeline
//! creation, so a first-use compile is a guaranteed hitch.
//!
//! ## One shared bind group layout, and why that is the load-bearing decision
//!
//! Every compiled program's parameter bind group is built against
//! [`surface_bind_group_layout`] — one layout, for all of them. That makes every
//! surface pipeline's *pipeline layout* identical, so switching pipelines mid-pass
//! does not invalidate the bind groups already set: groups 1 (`lights`) and 2
//! (`shadow_sample`) stay set exactly **once per pass**, outside the batch loop,
//! where `crate::scene_renderer` has always set them. A per-surface layout would
//! un-hoist both and pay for them once per program — the expensive mistake.
//!
//! ## One buffer per program, never one buffer rewritten
//!
//! Each program owns its own 512-byte parameter buffer, written once here.
//! `crate::post_chain` records the alternative's defect: `queue.write_buffer` is
//! ordered against *submission*, not against the passes inside an encoder, so N
//! writes to one shared buffer leave every draw in that pass reading the last of
//! them. The engine already paid for that bug once and fixed it with separate
//! buffers; this generalises the fix rather than re-earning it.
//!
//! Nothing here is written to disk, IndexedDB or any store, and nothing is ever
//! evicted. Programs are regenerated each launch.

use std::collections::HashMap;

use crate::surface_program::cache::{SurfaceProgramCatalog, SurfaceProgramSource};
use crate::surface_program::params::SURFACE_PARAM_REGION_BYTES;
use crate::surface_program::wgsl_template;

/// The binding number the surface parameter region occupies inside group 3.
/// `0` in that group is the skinned pass's joint palette; the two are disjoint
/// and naga resolves per entry point, so neither pipeline is asked for a
/// resource it does not read. Group 3 is the last one
/// `wgpu::Limits::downlevel_webgl2_defaults` guarantees, which is why they share.
pub(crate) const SURFACE_PARAMS_BINDING: u32 = 1;

/// The **one** bind group layout every surface program's parameter group is built
/// against.
///
/// Shared deliberately — see this module's header. Visible to both stages
/// because a displacement program reads its parameters in the vertex stage and a
/// colour program reads them in the fragment stage, and they are the two halves
/// of one program keyed by one digest.
pub(crate) fn surface_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("axiom-surface-params-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: SURFACE_PARAMS_BINDING,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// The fixed inputs a surface pipeline is built from: the colour target's format
/// and the four bind group layouts the main pass declares, in group order.
///
/// A struct rather than four positional arguments because
/// [`SurfaceProgramCache::compile`] passes them through unchanged to every
/// program, and a positional mix-up between three same-typed layout references is
/// exactly the bug a reader cannot see.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfacePipelineInputs<'a> {
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) material: &'a wgpu::BindGroupLayout,
    pub(crate) lights: &'a wgpu::BindGroupLayout,
    pub(crate) shadow_sample: &'a wgpu::BindGroupLayout,
    pub(crate) surface: &'a wgpu::BindGroupLayout,
}

/// One surface's program, bound: the pipeline it draws with and the parameter
/// group it reads.
///
/// The 512-byte buffer behind that group is not a third field. It is written
/// once at the barrier and never read back by the CPU, and a `wgpu::BindGroup`
/// keeps every resource it binds alive — so naming the buffer again here would
/// be a second owner of one thing, which is how two owners disagree.
#[derive(Debug)]
pub(crate) struct CompiledSurfaceProgram {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
}

impl CompiledSurfaceProgram {
    /// The pipeline a draw naming this program is drawn with.
    pub(crate) fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }

    /// This program's own parameter bind group (group 3).
    pub(crate) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// Every surface program this device compiled, keyed on the surface's digest.
///
/// The map is a lookup, not an order: the compile *order* is the catalog's
/// (ascending digest), which is where determinism is decided. A miss returns
/// `None` and the caller renders the constant fallback.
#[derive(Debug)]
pub(crate) struct SurfaceProgramCache {
    programs: HashMap<u64, CompiledSurfaceProgram>,
    /// The parameter group a draw naming **no** surface binds: an all-zero
    /// region. The default program reads none of it, so its contents are
    /// unobservable — but a bind group must be bound for the pipeline layout to
    /// be satisfied, and binding a shared zero one costs a single call per pass.
    default_bind_group: wgpu::BindGroup,
}

impl SurfaceProgramCache {
    /// A cache holding no programs: what a device starts with, and what it keeps
    /// when the app authors no surface. Every existing app takes this path and
    /// pays exactly one bind group per pass for it.
    pub(crate) fn empty(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> SurfaceProgramCache {
        SurfaceProgramCache {
            programs: HashMap::new(),
            // The buffer is dropped here on purpose: a `wgpu::BindGroup` keeps
            // every resource it binds alive, and a zeroed region nobody writes
            // needs no second owner.
            default_bind_group: region(device, layout).1,
        }
    }

    /// Compile every program in `catalog`, **in the catalog's order** — ascending
    /// by digest — so a given surface set always compiles the same programs in
    /// the same sequence on every run and every device.
    pub(crate) fn compile(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        catalog: &SurfaceProgramCatalog,
        inputs: SurfacePipelineInputs<'_>,
    ) -> SurfaceProgramCache {
        let mut cache = SurfaceProgramCache::empty(device, inputs.surface);
        // **One pipeline per SHAPE, one parameter region per MATERIAL.**
        //
        // The catalog is keyed by parameter region, so forty-six runtime
        // materials are forty-six entries — but they share one digest and must
        // therefore share one pipeline, or this would compile forty-six
        // identical programs and reintroduce the stutter the digest key exists
        // to prevent. `wgpu::RenderPipeline` is reference-counted, so the clone
        // is a handle, not a compile.
        let mut pipelines: HashMap<u64, wgpu::RenderPipeline> = HashMap::new();
        cache.programs = catalog
            .sources()
            .iter()
            .map(|source| {
                let shared = pipelines.get(&source.pipeline_key()).cloned();
                let compiled = compile_one(device, queue, source, inputs, shared);
                pipelines
                    .entry(source.pipeline_key())
                    .or_insert_with(|| compiled.pipeline().clone());
                (source.program_id(), compiled)
            })
            .collect();
        cache
    }

    /// The compiled program `program_id` names, or `None`.
    ///
    /// **A miss is never a compile.** `None` means the barrier did not prepare
    /// this program, and the caller draws it with the default pipeline and the
    /// constant fallback while the frame reports
    /// `axiom_host::FrameFeature::ProceduralSurface`. See the doctrine
    /// `crate::post_chain` states at its render-target comment: the set of
    /// pipelines a session holds is fixed before the first frame, so no frame can
    /// stutter compiling one.
    pub(crate) fn program(&self, program_id: u64) -> Option<&CompiledSurfaceProgram> {
        self.programs.get(&program_id)
    }

    /// The parameter group bound for `surface_program == 0` and for a miss.
    pub(crate) fn default_bind_group(&self) -> &wgpu::BindGroup {
        &self.default_bind_group
    }

    /// How many programs this device holds. Asserted by the scene tests so a
    /// variant explosion is a failing test rather than a slow frame.
    pub(crate) fn len(&self) -> u32 {
        self.programs.len() as u32
    }
}

/// Build one program's parameter buffer and the bind group that reads it, both
/// against the shared layout. The buffer starts zeroed, which is the documented
/// value of a slot nobody declared.
fn region(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-surface-params"),
        size: SURFACE_PARAM_REGION_BYTES,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-surface-params-bind-group"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: SURFACE_PARAMS_BINDING,
            resource: buffer.as_entire_binding(),
        }],
    });
    (buffer, bind_group)
}

/// Compile one catalog entry into a pipeline, a buffer and a bind group.
fn compile_one(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &SurfaceProgramSource,
    inputs: SurfacePipelineInputs<'_>,
    // The pipeline an earlier region of the SAME shape already built, if any.
    // Present for every region after the first of its digest.
    shared: Option<wgpu::RenderPipeline>,
) -> CompiledSurfaceProgram {
    let (params_buffer, bind_group) = region(device, inputs.surface);
    // Written ONCE, here, at the barrier. Not per frame, and not per draw: the
    // bytes are a function of the surface's authored parameter values, and
    // changing one of those values is a fresh preparation, never a fresh
    // pipeline (the digest does not move — see `cache`).
    queue.write_buffer(&params_buffer, 0, source.params());
    CompiledSurfaceProgram {
        pipeline: shared.unwrap_or_else(|| crate::scene_renderer::build_main_pipeline(
            device,
            inputs.format,
            inputs.material,
            inputs.lights,
            inputs.shadow_sample,
            inputs.surface,
            &wgsl_template::scene_shader(
                crate::scene_wgsl::SCENE_WGSL_PREFIX,
                source.vertex(),
                source.fragment(),
                crate::scene_wgsl::SCENE_WGSL_SUFFIX,
            ),
        )),
        bind_group,
    }
}
