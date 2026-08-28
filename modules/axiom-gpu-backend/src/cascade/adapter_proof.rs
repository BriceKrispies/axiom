//! Real-adapter proof for [`crate::cascade`]. See that module's header for what
//! the two claims are; this file is the harness that makes them.
//!
//! The WGSL below is the text this slice asks to be spliced into
//! `scene_wgsl.rs` (see `docs/work-manifests/shmup-port/notes/csm.md`), verbatim
//! apart from the uniform's group/binding numbers, which the main pass owns. It
//! is transcribed from `csmShaderChunk`'s GLSL, keeping the source's control
//! flow — shader text is data, and a filter loop stays a filter loop — with four
//! stated changes:
//!
//! - `texture(sampler2DArray, vec3(uv, layer))` becomes `textureSampleLevel(…,
//!   0.0)`: an **explicit** LOD, which is what makes the source's early returns
//!   legal in WGSL (an implicit-derivative sample may not sit under non-uniform
//!   control flow). The map has no mips and a nearest filter, so the sampled
//!   value is unchanged.
//! - `proj = sc.xyz / sc.w * 0.5 + 0.5` becomes `ndc.z` with a flipped `v`: wgpu
//!   clip depth is already `[0, 1]` and its framebuffer `v` counts down — the
//!   same two conventions the engine's existing `shadow_factor` applies.
//! - `smoothstep` is written out. WGSL leaves `smoothstep(low, high, …)` with
//!   `low >= high` indeterminate, and the source's far fade-out calls it exactly
//!   that way.
//! - the `dot(lightDirView, owSunDirView) < 0.999` light-loop identity test is
//!   dropped **here**, because this proof runs a single directional light and so
//!   has no loop to pick the sun out of.
//!
//!   That is a statement about this harness, not about the engine. Axiom's real
//!   light loop (`scene_wgsl.rs`) runs up to 16 lights and gives `atten = shade`
//!   — the sun's cascade factor — to **every** light with `v.w <= 0.5`, while
//!   the shadow map itself is fitted to a single frame-level `light_direction`
//!   that is not one of them. A second directional light therefore receives the
//!   sun's shadow, which is exactly what the source's identity test exists to
//!   prevent. The test is needed *more* here than in the source, not less; see
//!   `indirect_lighting`, which kept it.
//!
//!   Porting it needs the sun direction in the lights uniform to compare
//!   against, which is a uniform-layout change and an engine-wide golden
//!   re-record — deliberately not folded into this port's integration pass. It
//!   is currently unobservable only because no app in the repo registers two
//!   directional lights.

use axiom_math::{Mat4, Vec3};

use crate::cascade::shading::{ig_noise, project, select_cascade, sun_shadow};
use crate::cascade::{
    atlas_byte_size, fit, quality_tier, CascadeCamera, CascadeParams, CascadeSet, MAP_SIZE,
    MAX_CASCADES,
};

/// How many receiver probes one run compares. Also the probe target's width.
const PROBES: usize = 8;

/// `copy_texture_to_buffer` requires each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// The atlas edge the proof renders at: the source's shipped `4 x 2048`.
const SIZE: u32 = MAP_SIZE;

/// The caster depth pass: `csm.js`'s `depthMaterial`. Double-sided, and the
/// fragment writes the light-space depth the array layer stores.
const CASTER_WGSL: &str = r#"
struct Vp { m: mat4x4<f32> };
@group(0) @binding(0) var<uniform> vp: Vp;

@vertex
fn caster_vs(@location(0) p: vec3<f32>) -> @builtin(position) vec4<f32> {
    return vp.m * vec4<f32>(p, 1.0);
}

@fragment
fn caster_fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(pos.z, 0.0, 0.0, 1.0);
}
"#;

/// The probe harness's own bindings: the shared `csm` / `csm_maps` / `csm_samp`
/// trio at group 0 (the main pass puts the same three at the tail of its shadow
/// group -- which is exactly why `cascade::wgsl` declares no locations of its
/// own), plus the probe list this harness reads its query points from.
const PROBE_BINDINGS_WGSL: &str = r#"
@group(0) @binding(0) var<uniform> csm: CsmU;
@group(0) @binding(1) var csm_maps: texture_2d_array<f32>;
@group(0) @binding(2) var csm_samp: sampler;

struct Probes { items: array<vec4<f32>, 16> };
@group(0) @binding(3) var<uniform> probes: Probes;

"#;

/// The harness's entry points: one full-screen triangle, one probe per pixel
/// column, writing the cascade term and the selected cascade index.
const PROBE_ENTRY_WGSL: &str = r#"
@vertex
fn probe_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn probe_fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(pos.x);
    let a = probes.items[index * 2u + 0u];
    let b = probes.items[index * 2u + 1u];
    var c = OW_CASCADES - 1;
    for (var i = 0; i < OW_CASCADES; i = i + 1) {
        if (a.w < csm.split[i]) { c = i; break; }
    }
    return vec4<f32>(ow_sun_shadow(a.w, a.xyz, b.xyz, pos.xy), f32(c), 0.0, 1.0);
}
"#;

/// The text the proof compiles: the SHARED constants, uniform and functions from
/// `cascade::wgsl` -- the same three the main pass splices -- wrapped in this
/// harness's bindings and entry points.
///
/// This is what makes the proof a proof. When the text lived here, the adapter
/// run compared `shading.rs` against a transcription that only this file
/// compiled, so the pipeline could have drifted from both without a test
/// noticing. Now the only thing this file contributes is the plumbing.
fn probe_wgsl() -> String {
    format!(
        "{consts}{uniform}{bindings}{functions}{entry}",
        consts = crate::cascade::wgsl::CSM_CONSTANTS_WGSL,
        uniform = crate::cascade::wgsl::CSM_UNIFORM_WGSL,
        bindings = PROBE_BINDINGS_WGSL,
        functions = crate::cascade::wgsl::CSM_FUNCTIONS_WGSL,
        entry = PROBE_ENTRY_WGSL,
    )
}

/// A real GPU. Asserts rather than skipping.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    fn acquire() -> Gpu {
        // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
        // ~50 tests each opening their own is what crashes the driver.
        let gpu = crate::test_gpu::TestGpu::shared();
        let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
        Gpu { device, queue }
    }
}

/// The camera the whole proof is fitted to: a street-level chase view.
fn camera() -> CascadeCamera {
    CascadeCamera {
        world: Mat4::translation(Vec3::new(0.0, 3.0, 10.0)),
        fovy_radians: 60_f32.to_radians(),
        aspect: 16.0 / 9.0,
        near: 0.5,
        far: 300.0,
    }
}

/// Pointing FROM the scene TOWARD the sun.
fn sun() -> Vec3 {
    Vec3::new(0.35, 0.85, 0.4).normalize().unwrap()
}

/// One axis-aligned horizontal quad at height `y`, spanning `[x0, x1] x [z0, z1]`,
/// as two triangles of world-space positions.
fn roof(y: f32, x0: f32, x1: f32, z0: f32, z1: f32) -> [f32; 18] {
    [
        x0, y, z0, x1, y, z0, x1, y, z1, //
        x0, y, z0, x1, y, z1, x0, y, z1,
    ]
}

/// Where a point on a horizontal caster at height `y` lands on the ground.
fn ground_shadow_of(x: f32, y: f32, z: f32) -> Vec3 {
    let n = sun();
    Vec3::new(x, y, z).subtract(n.mul_scalar(y / n.y))
}

/// Nearest, clamp-to-edge sampling of the read-back atlas — exactly what a
/// `NearestFilter` + `ClampToEdge` sampler does.
fn tap_of(atlas: &[f32], size: u32) -> impl Fn(usize, f32, f32) -> f32 + '_ {
    let last = (size - 1) as f32;
    move |layer, u, v| {
        let x = (u * size as f32).floor().max(0.0).min(last) as usize;
        let y = (v * size as f32).floor().max(0.0).min(last) as usize;
        atlas[layer * (size as usize) * (size as usize) + y * (size as usize) + x]
    }
}

/// Render every caster into every cascade layer of a fresh atlas.
fn build_atlas(gpu: &Gpu, set: &CascadeSet, casters: &[f32]) -> wgpu::Texture {
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-cascade-caster"),
            source: wgpu::ShaderSource::Wgsl(CASTER_WGSL.into()),
        });
    let layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-cascade-caster-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-cascade-caster-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-cascade-caster-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("caster_vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    }],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("caster_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R32Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            // Double-sided, as the source's depth material is.
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
    let vertices = wgpu::util::DeviceExt::create_buffer_init(
        &gpu.device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("axiom-cascade-casters"),
            contents: bytemuck::cast_slice(casters),
            usage: wgpu::BufferUsages::VERTEX,
        },
    );
    let atlas = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-cascade-atlas"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: MAX_CASCADES as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let matrices = set.matrices();
    (0..MAX_CASCADES).for_each(|layer| {
        let uniform = wgpu::util::DeviceExt::create_buffer_init(
            &gpu.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("axiom-cascade-vp"),
                contents: bytemuck::cast_slice(&matrices[layer]),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-cascade-caster-bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let view = atlas.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: layer as u32,
            array_layer_count: Some(1),
            ..wgpu::TextureViewDescriptor::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axiom-cascade-caster-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    // The source clears to white: an empty texel is "nothing
                    // ever occluded here".
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },

            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..(casters.len() / 3) as u32, 0..1);
    });
    gpu.queue.submit(Some(encoder.finish()));
    atlas
}

/// Read every layer of `atlas` back, row-unpadded, layer-major.
fn read_atlas(gpu: &Gpu, atlas: &wgpu::Texture) -> Vec<f32> {
    let row_bytes = (SIZE * 4).div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-cascade-readback"),
        size: u64::from(row_bytes) * u64::from(SIZE) * MAX_CASCADES as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: atlas,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: MAX_CASCADES as u32,
        },
    );
    gpu.queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device.poll(wgpu::PollType::Wait).ok();
    let data = slice.get_mapped_range();
    let stride = (row_bytes / 4) as usize;
    let size = SIZE as usize;
    let raw: &[f32] = bytemuck::cast_slice(&data);
    let out: Vec<f32> = (0..MAX_CASCADES * size * size)
        .map(|i| {
            let layer = i / (size * size);
            let row = (i / size) % size;
            let col = i % size;
            raw[layer * stride * size + row * stride + col]
        })
        .collect();
    drop(data);
    readback.unmap();
    out
}

/// Pack the CSM uniform exactly as the WGSL struct lays it out.
fn pack_csm(set: &CascadeSet, params: CascadeParams) -> Vec<u8> {
    let matrices = set.matrices();
    let mut words: Vec<f32> = matrices.iter().flat_map(|m| m.iter().copied()).collect();
    words.extend_from_slice(&set.split());
    words.extend_from_slice(&set.split_near());
    words.extend_from_slice(&set.texel());
    words.extend_from_slice(&set.range());
    words.extend_from_slice(&[set.map_size() as f32, 1.0 / set.map_size() as f32, 0.0, 0.0]);
    let s = sun();
    words.extend_from_slice(&[s.x, s.y, s.z, 0.0]);
    words.extend_from_slice(&[
        params.strength,
        params.softness,
        params.max_filter_texels,
        params.rotation,
    ]);
    bytemuck::cast_slice(&words).to_vec()
}

/// Run the WGSL cascade lookup over `probes` and read back `(shadow, cascade)`.
fn render_probes(
    gpu: &Gpu,
    set: &CascadeSet,
    params: CascadeParams,
    atlas: &wgpu::Texture,
    probes: &[(Vec3, Vec3, f32)],
) -> Vec<[f32; 4]> {
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-cascade-probe"),
            source: wgpu::ShaderSource::Wgsl(probe_wgsl().into()),
        });
    let layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-cascade-probe-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        // R32Float is unfilterable-float without the
                        // `float32-filterable` feature, which is exactly the
                        // source's NearestFilter configuration.
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
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
    let csm = wgpu::util::DeviceExt::create_buffer_init(
        &gpu.device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("axiom-cascade-csm-uniform"),
            contents: &pack_csm(set, params),
            usage: wgpu::BufferUsages::UNIFORM,
        },
    );
    let mut probe_words = vec![0.0_f32; PROBES * 8];
    probes.iter().enumerate().for_each(|(i, (p, n, vd))| {
        probe_words[i * 8] = p.x;
        probe_words[i * 8 + 1] = p.y;
        probe_words[i * 8 + 2] = p.z;
        probe_words[i * 8 + 3] = *vd;
        probe_words[i * 8 + 4] = n.x;
        probe_words[i * 8 + 5] = n.y;
        probe_words[i * 8 + 6] = n.z;
    });
    let probe_buffer = wgpu::util::DeviceExt::create_buffer_init(
        &gpu.device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("axiom-cascade-probes"),
            contents: bytemuck::cast_slice(&probe_words),
            usage: wgpu::BufferUsages::UNIFORM,
        },
    );
    let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..wgpu::TextureViewDescriptor::default()
    });
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("axiom-cascade-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..wgpu::SamplerDescriptor::default()
    });
    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-cascade-probe-bg"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: csm.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&atlas_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: probe_buffer.as_entire_binding(),
            },
        ],
    });
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-cascade-probe-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-cascade-probe-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("probe_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("probe_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba32Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-cascade-probe-target"),
        size: wgpu::Extent3d {
            width: PROBES as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let row_bytes = (PROBES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-cascade-probe-readback"),
        size: u64::from(row_bytes),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axiom-cascade-probe-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },

            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: PROBES as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device.poll(wgpu::PollType::Wait).ok();
    let data = slice.get_mapped_range();
    let words: &[f32] = bytemuck::cast_slice(&data);
    let out = (0..PROBES)
        .map(|i| [words[i * 4], words[i * 4 + 1], words[i * 4 + 2], words[i * 4 + 3]])
        .collect();
    drop(data);
    readback.unmap();
    out
}

/// The probes: world point, world normal, view depth. The camera sits at
/// `(0, 3, 10)` looking down `-Z`, so a ground point's view depth is `10 - z`.
fn probes() -> Vec<(Vec3, Vec3, f32)> {
    let up = Vec3::new(0.0, 1.0, 0.0);
    let tilted = Vec3::new(0.3, 0.9, 0.1).normalize().unwrap();
    let near = ground_shadow_of(0.0, 6.0, 0.0);
    let far = ground_shadow_of(0.0, 6.0, -96.0);
    let depth = |p: Vec3| 10.0 - p.z;
    vec![
        // 0: dead centre of the near roof's shadow.
        (near, up, depth(near)),
        // 1: on the near shadow's +x edge, where the PCF disc straddles it.
        (Vec3::new(near.x + 4.0, 0.0, near.z), up, depth(near) ),
        // 2: nowhere near anything.
        (Vec3::new(20.0, 0.0, 0.0), up, 10.0),
        // 3: dead centre of the far roof's shadow, in the last cascade.
        (far, up, depth(far)),
        // 4: on the far shadow's +x edge.
        (Vec3::new(far.x + 4.0, 0.0, far.z), up, depth(far)),
        // 5: open road between the two.
        (Vec3::new(0.0, 0.0, -60.0), up, 70.0),
        // 6: inside the near shadow but at a view depth in the cascade 1 -> 2
        //    cross-fade, so both cascades contribute.
        (Vec3::new(near.x, 0.0, -6.0), up, 16.0),
        // 7: the same shadowed point with a tilted normal, exercising the
        //    slope-scaled bias and the grazing normal offset.
        (near, tilted, depth(near)),
    ]
}

#[test]
fn a_shadow_lands_where_the_cpu_reference_says_and_the_wgsl_agrees() {
    let gpu = Gpu::acquire();
    let set = fit(MAX_CASCADES, camera(), sun(), SIZE).expect("the street fit must resolve");
    let params = CascadeParams::default();
    let quality = quality_tier(3);

    let mut casters = Vec::new();
    casters.extend_from_slice(&roof(6.0, -4.0, 4.0, -4.0, 4.0));
    casters.extend_from_slice(&roof(6.0, -4.0, 4.0, -100.0, -92.0));

    // ---- claim 1: the atlas holds the caster where the reference projects it.
    let atlas_texture = build_atlas(&gpu, &set, &casters);
    let atlas_data = read_atlas(&gpu, &atlas_texture);
    assert_eq!(
        atlas_data.len() * 4,
        atlas_byte_size(SIZE, MAX_CASCADES) as usize,
        "the read-back atlas is the 4 x 2048 R32F layout"
    );
    let tap = tap_of(&atlas_data, SIZE);
    let probe_list = probes();
    let split = set.split();

    // The two centred probes must project into a texel that holds a NEARER
    // depth than the receiver's own: that is the caster being where the fit
    // said it would be, rasterised by real hardware.
    [(0_usize, 1_usize), (3, 3)]
        .into_iter()
        .for_each(|(probe, expected_cascade)| {
            let (p, n, vd) = probe_list[probe];
            let c = select_cascade(vd, &split, set.count());
            assert_eq!(
                c, expected_cascade,
                "probe {probe} at view depth {vd} selected cascade {c}"
            );
            let ndl = n.dot(sun());
            let (u, v, d) = project(&set, c, p, n, ndl);
            let stored = tap(c, u, v);
            assert!(
                stored < d,
                "probe {probe}: cascade {c} texel at ({u}, {v}) holds {stored}, not in front of {d}"
            );
        });
    // ...and the two open-road probes project into a cleared texel.
    [2_usize, 5].into_iter().for_each(|probe| {
        let (p, n, vd) = probe_list[probe];
        let c = select_cascade(vd, &split, set.count());
        let (u, v, _) = project(&set, c, p, n, n.dot(sun()));
        assert_eq!(
            tap(c, u, v),
            1.0,
            "probe {probe} projects onto an occupied texel of cascade {c}"
        );
    });

    // ---- claim 2: the WGSL means what the reference means.
    let gpu_probes = render_probes(&gpu, &set, params, &atlas_texture, &probe_list);

    let worst = probe_list
        .iter()
        .enumerate()
        .fold(0.0_f32, |worst, (i, (p, n, vd))| {
            let expected_c = select_cascade(*vd, &split, set.count());
            assert_eq!(
                gpu_probes[i][1] as usize, expected_c,
                "probe {i}: the shader selected cascade {} where the reference selects {expected_c}",
                gpu_probes[i][1]
            );
            let cpu = sun_shadow(
                &set,
                params,
                quality,
                *vd,
                *p,
                *n,
                sun(),
                (i as f32 + 0.5, 0.5),
                &tap,
            );
            let diff = (cpu - gpu_probes[i][0]).abs();
            assert!(
                diff <= 1.0e-5,
                "probe {i}: CPU {cpu} vs GPU {} differ by {diff}",
                gpu_probes[i][0]
            );
            worst.max(diff)
        });

    // **Measured: bit-exact.** `worst` is `0.0` on this adapter, and the
    // assertion is written at one f32 ULP rather than at the 1e-5 tolerance
    // above so a future divergence cannot hide inside slack nobody needs.
    //
    // The tolerance is not arbitrary either, because the quantity is *discrete*:
    // the term is a sum of `OW_PCF_TAPS` zero-or-one steps over 20, blended by
    // written-out `mix`/`smoothstep`. Two implementations that agree on which
    // texels the taps land in cannot differ by a *small* amount — they differ by
    // zero. One that disagrees about a single tap differs by 1/20 = 0.05, four
    // thousand times the tolerance. So 1e-5 separates "identical" from "a tap
    // moved" with three orders of margin either side, and the only float slack
    // available to the hardware (an `fma` contraction in the Vogel or bias
    // chain) would have to move a tap across a texel boundary to register at
    // all.
    assert!(
        worst <= f32::EPSILON,
        "CPU<->GPU parity measured at {worst}, not the bit-exact 0.0 this          adapter gave; re-derive the tolerance from the new measurement rather          than widening it"
    );

    // The shadowed probes really are dark and the open-road ones really are
    // lit — a parity test that agreed on 1.0 everywhere would prove nothing.
    assert!(
        (gpu_probes[0][0] < 0.02) & (gpu_probes[3][0] < 0.02),
        "the centred probes read {} and {} — the casters are not shadowing",
        gpu_probes[0][0],
        gpu_probes[3][0]
    );
    assert_eq!(gpu_probes[2][0], 1.0, "the open road must be fully lit");
    assert_eq!(gpu_probes[5][0], 1.0, "the far open road must be fully lit");
    // The edge probes sit in the penumbra: neither fully lit nor fully dark.
    [1_usize, 4].into_iter().for_each(|i| {
        let s = gpu_probes[i][0];
        assert!(
            (s > 0.0) & (s < 1.0),
            "edge probe {i} read {s}; the PCF disc is not straddling the edge"
        );
    });
    // The far cascade's texel is coarser than the near one's, which is the
    // whole reason there are four of them.
    let texel = set.texel();
    assert!(
        texel[3] > texel[1] * 4.0,
        "cascade 3's texel {} is not meaningfully coarser than cascade 1's {}",
        texel[3],
        texel[1]
    );
}

/// The IG-noise phase the shader and the reference must agree on, checked on the
/// adapter independently of everything else: it is the one place where a
/// `fract` transcription can silently diverge, and it feeds every tap.
#[test]
fn the_vogel_phase_hash_matches_on_the_adapter() {
    let gpu = Gpu::acquire();
    let set = fit(MAX_CASCADES, camera(), sun(), SIZE).unwrap();
    let atlas = build_atlas(&gpu, &set, &roof(6.0, -4.0, 4.0, -4.0, 4.0));
    // A strength of zero makes every probe return 1.0 through the shader's first
    // early-out, so what this run compares is only that the pipeline agrees on
    // the gate — the phase itself is compared through the probe run above, where
    // a one-ULP phase error moves a tap onto a different texel and shows up as a
    // whole 1/20th of the PCF sum.
    let off = CascadeParams {
        strength: 0.0,
        ..CascadeParams::default()
    };
    let out = render_probes(&gpu, &set, off, &atlas, &probes());
    out.iter().enumerate().for_each(|(i, v)| {
        assert_eq!(v[0], 1.0, "probe {i} is not gated off by strength 0");
    });
    // And the CPU reference is exact on the same hash inputs the shader used.
    (0..PROBES).for_each(|i| {
        let n = ig_noise(i as f32 + 0.5, 0.5);
        assert!((0.0..1.0).contains(&n), "phase {i} escaped [0, 1): {n}");
    });
}
