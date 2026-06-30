//! The **coverage-exempt platform arm** of the GPU 2D raster: the real wgpu
//! pipeline that turns the covered core's [`Draw2dGeometry`] into alpha-blended
//! pixels on a wgpu colour target.
//!
//! This is the 2D peer of [`crate::scene_renderer`]: the covered core
//! ([`crate::draw2d_geometry`]) decides *what* to draw (the layer-sorted quad
//! stream, its colours, UVs, and per-quad texture) with no GPU types; this module
//! owns *only* the genuine `wgpu` device/queue/pipeline/buffer calls. Per Module
//! Law #9 it is compiled only behind the native `offscreen` feature (the
//! screenshot tool + the parity proofs), so the default native build, the coverage
//! gate, and the branchless lint never see this wgpu code — the same exemption
//! `scene_renderer`/`offscreen` already take. When the live browser run loop
//! routes GPU 2D it reuses this same renderer (flip the `cfg` to add `wasm32`),
//! exactly as the live 3D binding and the off-screen path share `scene_renderer`.
//!
//! ## Parity-preserving choices
//! * **Blend** = `wgpu::BlendState::ALPHA_BLENDING` (`out = src·a + dst·(1−a)`,
//!   `out_a = a + dst_a·(1−a)`), which reproduces the software `over()` blend
//!   byte-for-byte when the target is linear.
//! * **No depth** — 2D order is painter's order; quads are drawn in the covered
//!   core's emitted order (already `(layer, submission)`-sorted).
//! * **Nearest sampling, clamp-to-edge** — matches the software sprite
//!   `sample()` (floored texel, clamped).
//! * The caller renders into a **linear** (non-sRGB) target so the GPU's
//!   quantization matches the software path's `linear → byte` write; see
//!   [`crate::draw2d_offscreen`].

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use crate::draw2d_geometry::{Draw2dGeometry, QuadSource, VERTEX_FLOATS, VERTS_PER_QUAD};

/// Bytes per emitted vertex (`VERTEX_FLOATS` f32: pos2 + uv2 + colour4).
const VERTEX_STRIDE: u64 = (VERTEX_FLOATS as u64) * 4;
/// Triangle indices per quad (`0,1,2,0,2,3`).
const INDICES_PER_QUAD: usize = 6;

/// WGSL for the 2D quad pass: convert pixel positions to NDC via the viewport
/// uniform (flipping Y so framebuffer-top maps to NDC-top), pass the UV + colour
/// through, and modulate the sampled texel by the per-vertex colour. A solid fill
/// samples the 1×1 white texture, so this one shader serves fills and sprites with
/// no branch.
const DRAW2D_WGSL: &str = r#"
struct Viewport { size: vec4<f32> };
@group(1) @binding(0) var<uniform> viewport: Viewport;

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    let ndc = vec2<f32>(
        pos.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pos.y / viewport.size.y * 2.0,
    );
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * in.color;
}
"#;

/// The wgpu 2D pipeline + per-frame resources: the alpha-blended quad pipeline,
/// the viewport uniform bind group, a shared nearest sampler, a 1×1 white texture
/// bind group (for solid fills), and one bind group per uploaded sprite texture.
#[derive(Debug)]
pub(crate) struct Draw2dRenderer {
    pipeline: wgpu::RenderPipeline,
    viewport_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::BindGroup,
    sprites: HashMap<u64, wgpu::BindGroup>,
}

impl Draw2dRenderer {
    /// Build the 2D pipeline for colour target `format`, the `width`×`height`
    /// viewport uniform, the white fill texture, and a bind group for each
    /// `(texture_id, width, height, RGBA8 pixels)` sprite atlas.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        textures: &[(u64, u32, u32, Vec<u8>)],
    ) -> Draw2dRenderer {
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-draw2d-texture-layout"),
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
            ],
        });

        let viewport_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-draw2d-viewport-layout"),
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
        let viewport_data: [f32; 4] = [width.max(1) as f32, height.max(1) as f32, 0.0, 0.0];
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("axiom-draw2d-viewport"),
            contents: bytemuck::cast_slice(&viewport_data),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-draw2d-viewport-bg"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-draw2d-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let white = upload_texture(
            device,
            queue,
            &texture_layout,
            &sampler,
            1,
            1,
            &[255, 255, 255, 255],
        );
        let sprites = textures
            .iter()
            .map(|(id, w, h, rgba)| {
                (
                    *id,
                    upload_texture(device, queue, &texture_layout, &sampler, *w, *h, rgba),
                )
            })
            .collect();

        let pipeline = build_pipeline(device, format, &texture_layout, &viewport_layout);

        Draw2dRenderer {
            pipeline,
            viewport_bind_group,
            texture_layout,
            sampler,
            white,
            sprites,
        }
    }

    /// Replace the uploaded sprite atlases (the live browser arm calls this when
    /// the app uploads 2D textures after binding). Unused by the off-screen entry,
    /// which uploads at construction; present so the renderer is the single owner
    /// of the sprite registry.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn set_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        textures: &[(u64, u32, u32, Vec<u8>)],
    ) {
        self.sprites = textures
            .iter()
            .map(|(id, w, h, rgba)| {
                (
                    *id,
                    upload_texture(
                        device,
                        queue,
                        &self.texture_layout,
                        &self.sampler,
                        *w,
                        *h,
                        rgba,
                    ),
                )
            })
            .collect();
    }

    /// Record + submit one 2D frame: clear `view` to `clear`, then draw each quad
    /// of `geo` in painter's order with its bound texture (white for a solid fill,
    /// the named atlas for a sprite, falling back to white if the atlas is
    /// missing). An empty geometry clears and presents nothing.
    pub(crate) fn record(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        clear: [f32; 4],
        geo: &Draw2dGeometry,
    ) {
        let quad_count = geo.quad_count();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("axiom-draw2d-vertices"),
            contents: bytemuck::cast_slice(geo.vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = build_indices(quad_count);
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("axiom-draw2d-indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("axiom-draw2d-encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-draw2d-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(1, &self.viewport_bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            geo.sources().iter().enumerate().for_each(|(q, source)| {
                let bind_group = match source {
                    QuadSource::Solid => &self.white,
                    QuadSource::Sprite(id) => self.sprites.get(id).unwrap_or(&self.white),
                };
                pass.set_bind_group(0, bind_group, &[]);
                let start = (q * INDICES_PER_QUAD) as u32;
                pass.draw_indexed(start..start + INDICES_PER_QUAD as u32, 0, 0..1);
            });
        }
        queue.submit(std::iter::once(encoder.finish()));
    }
}

/// Build the `0,1,2,0,2,3` index list for `quad_count` quads (quad `q` references
/// vertices `4q..4q+4`).
fn build_indices(quad_count: usize) -> Vec<u32> {
    (0..quad_count)
        .flat_map(|q| {
            let base = (q * VERTS_PER_QUAD) as u32;
            [base, base + 1, base + 2, base, base + 2, base + 3]
        })
        .collect()
}

/// Upload one RGBA8 texture (linear, **non-sRGB**, so a sampled texel equals
/// `byte/255` exactly as the software `sample()` does) and build its
/// texture+sampler bind group.
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    rgba8: &[u8],
) -> wgpu::BindGroup {
    let width = width.max(1);
    let height = height.max(1);
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-draw2d-texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("axiom-draw2d-texture-bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Build the alpha-blended, depth-less 2D quad pipeline for colour target
/// `format`.
fn build_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    texture_layout: &wgpu::BindGroupLayout,
    viewport_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axiom-draw2d-shader"),
        source: wgpu::ShaderSource::Wgsl(DRAW2D_WGSL.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axiom-draw2d-pl"),
        bind_group_layouts: &[texture_layout, viewport_layout],
        push_constant_ranges: &[],
    });
    let attrs = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 8,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 16,
            shader_location: 2,
        },
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axiom-draw2d-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: VERTEX_STRIDE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attrs,
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
    })
}
