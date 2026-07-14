//! Native offscreen renderer for the agent bridge (feature `agent-render`).
//!
//! Lives in the BIN, not the lib: wgpu pulls in a huge symbol set, and linking
//! that into the crate's `cdylib` (the browser artifact) overflows the mingw
//! DLL export table. An executable has no such limit, so the renderer stays here
//! and the lib/cdylib never references wgpu.
//!
//! It mirrors the live wasm binding (`axiom-windowing`'s `LiveGpuBinding`) minus
//! the surface: upload the engine's cube vertex stream once, write the frame's
//! per-instance MVP+colour floats, draw instanced **with a depth buffer** into an
//! off-screen texture, read it back, and write a PNG — the same first-person
//! image a browser shows. No GPU adapter → returns `None` (structured-only).

use std::cell::RefCell;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 600;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Bytes per instance: mvp(16 f32) + colour(4 f32) = 20 f32.
const INSTANCE_STRIDE: u64 = 20 * 4;
/// Bytes per vertex: position(3 f32) + normal(3 f32) + colour(4 f32) = 10 f32.
const VERTEX_STRIDE: u64 = 10 * 4;

/// The same shader the live binding uses (copied — the live const is wasm-only,
/// and the native bin cannot depend on the wasm-only `axiom-windowing`). The base
/// colour is the per-vertex colour times the per-instance colour, matching the
/// live arm; the engine's white per-vertex default keeps the per-instance colour
/// authoritative.
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

struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    color_view: wgpu::TextureView,
    color_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    readback: wgpu::Buffer,
    index_count: u32,
    max_instances: u32,
}

thread_local! {
    /// The server is single-threaded (one request at a time), so a thread-local
    /// holds the lazily-built renderer across frames. `Some(None)` = tried and no
    /// GPU; `None` = not yet attempted.
    static RENDERER: RefCell<Option<Option<Renderer>>> = const { RefCell::new(None) };
}

/// Render the frame to `frames/<tick>.png` and return its path, or `None` if no
/// GPU is available (the bridge then reports structured state only).
pub(crate) fn render_frame(
    vertices: &[f32],
    indices: &[u32],
    max_instances: u32,
    instances: &[f32],
    clear: [f32; 4],
    tick: u64,
) -> Option<String> {
    RENDERER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(Renderer::new(vertices, indices, max_instances));
        }
        let renderer = slot.as_ref().and_then(|r| r.as_ref())?;
        let png = renderer.render(instances, clear)?;
        let path = format!("frames/{tick:06}.png");
        std::fs::create_dir_all("frames").ok()?;
        std::fs::write(&path, png).ok()?;
        Some(path)
    })
}

impl Renderer {
    fn new(vertices: &[f32], indices: &[u32], max_instances: u32) -> Option<Renderer> {
        use wgpu::util::DeviceExt;
        let max_instances = max_instances.max(1);
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("axiom-agent-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .ok()?;

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-agent-color"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
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
        let depth_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-agent-depth"),
                size: wgpu::Extent3d {
                    width: WIDTH,
                    height: HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("axiom-agent-vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("axiom-agent-indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-agent-instances"),
            size: INSTANCE_STRIDE * u64::from(max_instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-agent-readback"),
            size: u64::from(WIDTH * HEIGHT * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-agent-shader"),
            source: wgpu::ShaderSource::Wgsl(CUBE_WGSL.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-agent-pl"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-agent-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
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
                    format: COLOR_FORMAT,
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

        Some(Renderer {
            device,
            queue,
            pipeline,
            color_view,
            color_texture,
            depth_view,
            vertex_buffer,
            index_buffer,
            instance_buffer,
            readback,
            index_count: indices.len() as u32,
            max_instances,
        })
    }

    fn render(&self, instances: &[f32], clear: [f32; 4]) -> Option<Vec<u8>> {
        let instance_count = (instances.len() / 20).min(self.max_instances as usize) as u32;
        self.queue
            .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("axiom-agent-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-agent-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
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
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.index_count, 0, 0..instance_count);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH * 4),
                    rows_per_image: Some(HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
        let _ = self.device.poll(wgpu::PollType::Wait);
        rx.recv().ok()?.ok()?;

        let png = {
            let data = self.readback.slice(..).get_mapped_range();
            encode_png(&data)
        };
        self.readback.unmap();
        png
    }
}

/// Encode an `Rgba8` `WIDTH`x`HEIGHT` byte buffer as a PNG.
fn encode_png(rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, WIDTH, HEIGHT);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}
