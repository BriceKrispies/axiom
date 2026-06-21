//! The real wgpu presentation binding — **wasm32 only**.
//!
//! None of this compiles on native, so the deterministic engine (and
//! `cargo test --workspace` / the coverage gate) never pulls in wgpu. This is
//! the thin, logic-free platform arm: it takes plain engine data (vertex streams
//! of position+normal+colour + per-instance MVP/colour floats + a clear colour)
//! and issues the real GPU calls. Every decision lives on the deterministic side.

use std::collections::HashMap;

use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;
use wgpu::util::DeviceExt;

/// WGSL for the cubes/terrain: per-vertex position+normal+**colour**, per-instance
/// MVP (four columns) + colour. The final base colour is the component-wise
/// product of the per-vertex colour and the per-instance colour, so a mesh that
/// supplies per-vertex white `(1,1,1,1)` renders exactly as the per-instance
/// colour alone (the backward-compatible default), while a mesh that supplies
/// real per-vertex colours and a white instance shows the per-vertex colours.
const CUBE_WGSL: &str = r#"
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) vertex_color: vec4<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) instance_color: vec4<f32>,
) -> VsOut {
    let mvp = mat4x4<f32>(m0, m1, m2, m3);
    var out: VsOut;
    out.clip = mvp * vec4<f32>(position, 1.0);
    out.normal = normal;
    out.color = vertex_color * instance_color;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let light_dir = normalize(vec3<f32>(0.4, 0.7, 0.6));
    let diffuse = max(dot(normalize(in.normal), light_dir), 0.0);
    let shaded = in.color.rgb * (0.25 + 0.75 * diffuse);
    return vec4<f32>(shaded, in.color.a);
}
"#;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Bytes per instance: mvp(16 f32) + colour(4 f32) = 20 f32.
const INSTANCE_STRIDE: u64 = 20 * 4;
/// Bytes per vertex: position(3 f32) + normal(3 f32) + colour(4 f32) = 10 f32.
const VERTEX_STRIDE: u64 = 10 * 4;

/// One uploaded mesh's GPU buffers: its interleaved vertex stream and triangle
/// index buffer, plus the index count to draw.
#[derive(Debug)]
struct MeshBuffers {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

/// The real, browser-owned GPU objects. Held only here, on wasm32.
///
/// A frame is a list of per-mesh instance batches: distinct meshes are uploaded
/// once into `meshes` (keyed by the mesh id the engine's command stream carries),
/// and each frame the shared `instance_buffer` is filled with every batch's
/// instances back-to-back; each batch is then drawn against its own mesh buffers
/// using a byte-offset slice of the instance buffer (so no `firstInstance` is
/// needed — WebGL-downlevel safe).
#[derive(Debug)]
pub struct LiveGpuBinding {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    meshes: HashMap<u64, MeshBuffers>,
    instance_buffer: wgpu::Buffer,
    depth_view: wgpu::TextureView,
    max_instances: u32,
}

/// Build a mesh's GPU buffers from an interleaved vertex stream (10 floats/vertex:
/// position+normal+colour) and a triangle-list index buffer.
fn upload_mesh(device: &wgpu::Device, vertices: &[f32], indices: &[u32]) -> MeshBuffers {
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("axiom-mesh-vertices"),
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("axiom-mesh-indices"),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    MeshBuffers {
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
    }
}

impl LiveGpuBinding {
    /// Real GPU initialisation: instance → surface from canvas → adapter →
    /// device/queue → configure surface → upload every distinct mesh into the
    /// mesh cache → build the instanced render pipeline + depth buffer. `meshes`
    /// is the distinct geometry set as `(mesh_id, interleaved vertices [10
    /// floats/vertex], triangle indices)`; per-frame draws reference these ids.
    /// Errors are surfaced as `JsValue`; this never fakes success.
    pub async fn initialize(
        canvas: HtmlCanvasElement,
        width: u32,
        height: u32,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        max_instances: u32,
    ) -> Result<LiveGpuBinding, JsValue> {
        let width = width.max(1);
        let height = height.max(1);
        let max_instances = max_instances.max(1);

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsValue::from_str(&format!("create_surface failed: {e}")))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|e| JsValue::from_str(&format!("request_adapter failed: {e}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("axiom-live-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| JsValue::from_str(&format!("request_device failed: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // Upload every distinct mesh once into the cache (shared across frames).
        let meshes: HashMap<u64, MeshBuffers> = meshes
            .iter()
            .map(|(id, vertices, indices)| (*id, upload_mesh(&device, vertices, indices)))
            .collect();

        // Per-instance MVP + colour buffer, rewritten each frame.
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-cube-instances"),
            size: INSTANCE_STRIDE * max_instances as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-cube-shader"),
            source: wgpu::ShaderSource::Wgsl(CUBE_WGSL.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-cube-pl"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-cube-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    // Per-vertex: position(3) + normal(3) + colour(4).
                    wgpu::VertexBufferLayout {
                        array_stride: VERTEX_STRIDE,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 12,
                                shader_location: 1,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 24,
                                shader_location: 2,
                            },
                        ],
                    },
                    // Per-instance: mvp columns (4 x vec4) + colour.
                    wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 0,
                                shader_location: 3,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 16,
                                shader_location: 4,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 32,
                                shader_location: 5,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 48,
                                shader_location: 6,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 64,
                                shader_location: 7,
                            },
                        ],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
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

        let depth_view = create_depth_view(&device, width, height);

        Ok(LiveGpuBinding {
            surface,
            device,
            queue,
            config,
            pipeline,
            meshes,
            instance_buffer,
            depth_view,
            max_instances,
        })
    }

    /// Draw one real frame from per-mesh instance batches. Each batch is
    /// `(mesh_id, instances [mvp(16)+colour(4) per instance], count)`. All
    /// batches' instances are packed back-to-back into the shared instance
    /// buffer (capped at `max_instances` total), then each batch is drawn against
    /// its cached mesh buffers using a byte-offset slice of the instance buffer.
    /// A batch whose `mesh_id` was never uploaded is skipped. Real pixels.
    pub fn render_frame(&self, batches: &[(u64, Vec<f32>, u32)], clear: [f32; 4]) -> Result<(), JsValue> {
        // Pack instances and record each batch's draw range (mesh id, byte
        // offset into the instance buffer, instance count), capped at capacity.
        let mut packed: Vec<f32> = Vec::new();
        let mut draws: Vec<(u64, u64, u32)> = Vec::new();
        let mut written: u32 = 0;
        for (mesh_id, instances, count) in batches {
            let room = self.max_instances.saturating_sub(written);
            let count = (*count).min(room);
            let floats = (count as usize) * 20;
            let byte_offset = u64::from(written) * INSTANCE_STRIDE;
            packed.extend_from_slice(&instances[..floats.min(instances.len())]);
            draws.push((*mesh_id, byte_offset, count));
            written += count;
        }
        self.queue
            .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&packed));

        let frame = self
            .surface
            .get_current_texture()
            .map_err(|e| JsValue::from_str(&format!("get_current_texture failed: {e}")))?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("axiom-frame-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (mesh_id, byte_offset, count) in &draws {
                if let Some(mesh) = self.meshes.get(mesh_id) {
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_vertex_buffer(1, self.instance_buffer.slice(*byte_offset..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..*count);
                }
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }

    /// Replace one cached mesh's geometry mid-loop (recreate its vertex + index
    /// buffers from a freshly-built mesh; interleaved position+normal+colour
    /// `vertices`, 10 floats/vertex; triangle-list `indices`). The pipeline,
    /// instance buffer, surface and depth view are untouched. Used by the
    /// streaming run loop to slide the terrain mesh around the player without
    /// rebuilding the binding. Dropping the old buffers frees their GPU
    /// allocations.
    pub fn replace_geometry(&mut self, mesh_id: u64, vertices: &[f32], indices: &[u32]) {
        self.meshes
            .insert(mesh_id, upload_mesh(&self.device, vertices, indices));
    }
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-depth"),
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
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
