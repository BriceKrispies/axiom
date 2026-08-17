//! Upscale blit: present a reduced-resolution render target to the swapchain.
//!
//! The render-scale path renders the 3D scene into an intermediate colour texture
//! sized by [`axiom_host::HostDeviceProfile::render_size`] — below the physical
//! surface on a high-DPR phone, above it on a supersampling tier — then this blit
//! samples that texture across the full swapchain, magnifying or resolving it on
//! present as the two sizes require. One
//! fullscreen triangle, no vertex buffer; the source texture is fixed (the
//! intermediate target), so the pipeline + bind group are built once and the only
//! per-frame work is one draw into the acquired swapchain view.
//!
//! Compiled only where a real GPU is in play (wasm32 / the native `offscreen`
//! feature), exactly like [`crate::scene_renderer`].

/// A fullscreen-triangle pass that samples one source texture to the target. The
/// vertex shader emits a triangle that covers clip space with matching UVs; the
/// fragment shader is a single `textureSample`.
const BLIT_WGSL: &str = r#"
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: VsOut;
    out.clip = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
// xy = the LIVE FRACTION of the source: a frame rendered at a reduced scale
// occupies only the lower-left sub-rect of a target that stays allocated at full
// size, so a fullscreen uv must be mapped into it. `1,1` is the no-op.
// z  = the sRGB ENCODE FLAG: 1 when the swap chain will not encode this store
// for us (see `crate::surface_encode`), 0 when it will. w unused.
@group(0) @binding(2) var<uniform> live: vec4<f32>;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // A plain scale: at full scale `live` is `1,1` and this is the exact
    // identity, so an unscaled frame blits bit-for-bit as it always did. The
    // scene pass clears the whole attachment, so the margin outside a reduced
    // frame is the clear colour rather than a stale larger frame.
    let c = textureSample(src_tex, src_sampler, in.uv * live.xy);
    // `mix`, not a branch: one pipeline serves both kinds of surface, so the
    // flag can never select a pipeline the driver has not compiled yet. At
    // `live.z == 0` this is the exact identity — the sRGB-surface arm blits
    // bit-for-bit as it always did.
    return vec4<f32>(mix(c.rgb, srgb_encode(c.rgb), live.z), c.a);
}
"#;

/// The built upscale pipeline plus the bind group for its (fixed) source texture.
#[derive(Debug)]
pub(crate) struct UpscaleBlit {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    /// The live-fraction uniform, rewritten per present. Holding it (rather than
    /// rebuilding the bind group) is the whole point: a render-scale change must
    /// cost a buffer write, not a reallocation.
    live: wgpu::Buffer,
    /// Whether this blit must apply the sRGB encode itself, decided once from the
    /// target format at build (see [`crate::surface_encode::present_encode_flag`])
    /// and packed into the `live` uniform's `z` on every present. Held here rather
    /// than passed per frame because it is a property of the surface the blit was
    /// built for, and the surface cannot change without rebuilding the blit.
    encode: f32,
}

impl UpscaleBlit {
    /// Build the blit for a `source_view` (the intermediate colour target) to a
    /// swapchain of `target_format`. The `filter` chooses the **magnification**
    /// character — the direction where the target is smaller than the swapchain:
    /// `Linear` smooths, `Nearest` gives hard retro 32-bit-style chunky pixels.
    ///
    /// Minification is **not** the caller's choice: it is always `Linear`. When
    /// the target is larger than the swapchain the pass is not an upscale at all,
    /// it is a supersample *resolve* (`HostDeviceProfile::render_supersample`),
    /// and at an exact 2× the destination pixel centre lands on the corner of
    /// four source texels, so a linear tap is precisely the 2×2 box average those
    /// extra samples were rendered for. Point-sampling there would render four
    /// samples per pixel and then throw three of them away — the aliasing would
    /// be identical to no supersampling at all, at four times the cost.
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        source_view: &wgpu::TextureView,
        filter: wgpu::FilterMode,
    ) -> UpscaleBlit {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-upscale-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: filter,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: filter,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-upscale-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let live = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-upscale-live"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-upscale-bind-group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: live.as_entire_binding(),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-upscale-shader"),
            source: wgpu::ShaderSource::Wgsl(
                crate::surface_encode::shader_source(BLIT_WGSL).into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-upscale-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-upscale-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        UpscaleBlit {
            pipeline,
            bind_group,
            live,
            encode: crate::surface_encode::present_encode_flag(target_format),
        }
    }

    /// Record the upscale pass into `target_view` (the acquired swapchain view).
    pub(crate) fn record(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        // The fraction of the source target the frame occupies; `(1.0, 1.0)` at
        // full scale. See the `live` uniform in `BLIT_WGSL`.
        live: (f32, f32),
        // The frame's GPU timestamp clock. This blit is what stands in for the
        // post chain on a frame that authored neither bloom nor a grade, so it
        // reports under the same `post` name — that slot is "the present-side
        // fullscreen work", whichever pipeline performed it. `None` leaves the
        // pass exactly as it has always been recorded.
        clock: Option<&crate::gpu_pass_clock::GpuPassClock>,
    ) {
        queue.write_buffer(
            &self.live,
            0,
            bytemuck::cast_slice(&[live.0, live.1, self.encode, 0.0]),
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axiom-upscale-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: clock.map(|clock| clock.writes(crate::gpu_pass_clock::PASS_POST)),
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
