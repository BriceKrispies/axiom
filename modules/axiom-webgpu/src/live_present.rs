//! The **live** presentation arm — real `wgpu`, off-screen, native only.
//!
//! This is the body the [`BackendKind::Live`](crate::WebGpuApi) seam always
//! promised: it takes the *same* [`GpuSubmission`] the recording backend
//! records and actually **executes it on a real GPU**, rendering into an
//! off-screen colour target and reading the pixels back to RGBA8. It is the
//! headless proof that the deterministic `RenderCommandList -> GpuSubmission`
//! chain the engine proves is the chain that can render real pixels.
//!
//! ## Why this is compiled only behind `offscreen`
//! Real GPU work is inherently platform-dependent and non-deterministic across
//! adapters, so it is isolated behind the off-by-default `offscreen` feature,
//! exactly as `axiom-gpu-backend`'s own screenshot renderer is. The default
//! build — and therefore the coverage gate, the branchless dylint, and the
//! source-hygiene scan — never compile this arm. The recording backend stays
//! the deterministic, fully-covered, branchless default; this arm is the
//! explicit, bounded escape hatch that turns a submission into pixels.
//!
//! ## Why the wgpu depth convention lives here
//! wgpu clip space uses a `[0,1]` depth range; Axiom's projection is the GL
//! `[-1,1]` convention. That remap ([`GL_TO_WGPU_DEPTH`]) is a *wgpu* fact, so
//! it is applied **here, in the wgpu consumer** — the upstream `GpuSubmission`
//! (and `axiom-render-pipeline`'s `RenderReport`) stay backend-neutral (M2).

use std::collections::HashMap;

use axiom_host::HostPowerPreference;
use axiom_math::Mat4;
use wgpu::util::DeviceExt;

use crate::gpu_submission::GpuSubmission;

/// Off-screen colour target format — sRGB, matching the live/browser output.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Depth target format.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// `copy_texture_to_buffer` requires each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// Column-major matrix remapping GL clip depth `z' = (z + w) / 2` so Axiom's
/// `[-1,1]` projection lands in wgpu's `[0,1]` clip depth. The wgpu depth
/// convention belongs to the wgpu consumer, so it lives here rather than being
/// baked into a backend-neutral upstream contract.
const GL_TO_WGPU_DEPTH: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.5, 0.0, //
    0.0, 0.0, 0.5, 1.0, //
];

/// A flat-shaded pass: each draw binds a per-draw uniform carrying its wgpu-ready
/// model-view-projection and a solid colour. Enough to prove a submission paints
/// the pixels it describes (a cleared frame is the clear colour; a drawn mesh
/// covers the pixels its geometry+camera place it over); richer shading is the
/// `axiom-gpu-backend` renderer's job, not this proof arm's.
const SUBMISSION_WGSL: &str = r#"
struct Draw { mvp: mat4x4<f32>, color: vec4<f32> };
@group(0) @binding(0) var<uniform> draw: Draw;

@vertex
fn vs(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return draw.mvp * vec4<f32>(position, 1.0);
}

@fragment
fn fs() -> @location(0) vec4<f32> {
    return draw.color;
}
"#;

/// One resolved draw ready for the GPU: the mesh to draw, its wgpu-ready MVP
/// (column-major), and its solid colour.
struct PreparedDraw {
    mesh_id: u64,
    mvp: [f32; 16],
    color: [f32; 4],
}

/// Walk the submission's command stream into `(clear_colour, prepared draws)`,
/// resolving each draw's material colour from `materials` and folding the wgpu
/// depth remap into every MVP. This is the deterministic CPU half; the GPU half
/// below simply executes it.
fn interpret(sub: &GpuSubmission, materials: &[(u64, [f32; 4])]) -> ([f32; 4], Vec<PreparedDraw>) {
    let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
    let mut clear = [0.0, 0.0, 0.0, 1.0];
    let mut view = Mat4::IDENTITY;
    let mut projection = Mat4::IDENTITY;
    let mut mesh_id: u64 = 0;
    let mut color = [1.0, 1.0, 1.0, 1.0];
    let mut draws = Vec::new();

    for cmd in sub.commands() {
        if let Some(c) = cmd.as_clear_frame() {
            clear = c;
        }
        if let Some((v, p)) = cmd.as_set_camera() {
            view = v;
            projection = p;
        }
        if let Some(id) = cmd.as_set_mesh() {
            mesh_id = id;
        }
        if let Some(material_id) = cmd.as_set_material() {
            color = materials
                .iter()
                .find(|(id, _)| *id == material_id)
                .map(|(_, c)| *c)
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        }
        if let Some((_index_count, world)) = cmd.as_draw_indexed() {
            let mvp = depth_fix
                .multiply(projection)
                .multiply(view)
                .multiply(world);
            draws.push(PreparedDraw {
                mesh_id,
                mvp: mvp.as_cols_array(),
                color,
            });
        }
    }
    (clear, draws)
}

/// Map the host boundary's abstract adapter power preference onto wgpu's, so a
/// live backend built from a `HostPresentationRequest` actually selects the
/// adapter the host asked for — the host presentation capability is consumed by
/// the real GPU init, not merely validated.
fn wgpu_power_preference(preference: HostPowerPreference) -> wgpu::PowerPreference {
    match preference {
        HostPowerPreference::LowPower => wgpu::PowerPreference::LowPower,
        HostPowerPreference::HighPerformance => wgpu::PowerPreference::HighPerformance,
        HostPowerPreference::Default => wgpu::PowerPreference::None,
    }
}

/// A mesh's uploaded GPU buffers.
struct MeshGpu {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

/// Execute a [`GpuSubmission`] on a real native GPU off-screen and read the
/// frame back as `width * height * 4` RGBA8 bytes (row-major, top-down).
///
/// `meshes` is `(mesh_id, position floats [3 per vertex], triangle indices)` and
/// `materials` is `(material_id, linear-RGBA colour)`, keyed by the same ids the
/// submission's `SetMesh` / `SetMaterial` commands reference — the resource
/// payloads the command stream binds by id (a real backend uploads geometry
/// separately from the per-frame command list). Returns `None` when no native
/// GPU adapter/device is available.
pub(crate) fn render_submission_to_rgba(
    sub: &GpuSubmission,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, [f32; 4])],
    power_preference: HostPowerPreference,
) -> Option<Vec<u8>> {
    let width = sub.target_width().max(1);
    let height = sub.target_height().max(1);
    let (clear, draws) = interpret(sub, materials);

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu_power_preference(power_preference),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("axiom-webgpu-live-device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;

    // Upload each referenced mesh's position + index buffers.
    let mesh_buffers: HashMap<u64, MeshGpu> = meshes
        .iter()
        .map(|(id, positions, indices)| {
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("axiom-webgpu-live-vertices"),
                contents: bytemuck::cast_slice(positions),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("axiom-webgpu-live-indices"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            (
                *id,
                MeshGpu {
                    vertices,
                    indices: index_buffer,
                    index_count: indices.len() as u32,
                },
            )
        })
        .collect();

    let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("axiom-webgpu-live-uniform-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axiom-webgpu-live-shader"),
        source: wgpu::ShaderSource::Wgsl(SUBMISSION_WGSL.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-webgpu-live-pl"),
        bind_group_layouts: &[&uniform_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axiom-webgpu-live-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 3 * 4,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                }],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    // One uniform buffer + bind group per draw (mvp 64B + colour 16B = 80B).
    let draw_bindings: Vec<(wgpu::BindGroup, u64)> = draws
        .iter()
        .map(|d| {
            let mut bytes: Vec<u8> = Vec::with_capacity(80);
            bytes.extend_from_slice(bytemuck::cast_slice(&d.mvp));
            bytes.extend_from_slice(bytemuck::cast_slice(&d.color));
            let ubo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("axiom-webgpu-live-draw-ubo"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-webgpu-live-draw-bind-group"),
                layout: &uniform_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ubo.as_entire_binding(),
                }],
            });
            (bind_group, d.mesh_id)
        })
        .collect();

    let color_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-webgpu-live-color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-webgpu-live-depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("axiom-webgpu-live-encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axiom-webgpu-live-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear[0] as f64,
                        g: clear[1] as f64,
                        b: clear[2] as f64,
                        a: clear[3] as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        for (bind_group, mesh_id) in &draw_bindings {
            if let Some(mesh) = mesh_buffers.get(mesh_id) {
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }
    }
    queue.submit(std::iter::once(encoder.finish()));

    read_back(&device, &queue, &color_texture, width, height)
}

/// Copy the colour texture through a row-aligned staging buffer and strip the
/// per-row padding into a tight `width * height * 4` RGBA8 buffer.
fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color_texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let unpadded_row = width * 4;
    let padded_row = unpadded_row.div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-webgpu-live-readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("axiom-webgpu-live-copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: color_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait).ok()?;
    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((unpadded_row * height) as usize);
    (0..height as usize).for_each(|row| {
        let start = row * padded_row as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
    });
    drop(mapped);
    readback.unmap();
    Some(pixels)
}
