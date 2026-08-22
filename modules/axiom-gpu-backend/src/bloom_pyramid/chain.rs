//! **The real wgpu passes** — `Bloom.setSize` and `Bloom.render`, as a pipeline
//! pair and a chain of `Rgba16Float` mips.
//!
//! Compiled only where a real GPU renders (`wasm32` / the `offscreen` feature),
//! and therefore outside the coverage gate by the same construction as
//! [`crate::post_chain`] and `crate::offscreen`. What it *is* proved by is
//! [`crate::bloom_pyramid::parity`], which drives this struct end to end on a
//! real adapter and compares every texel against
//! [`crate::bloom_pyramid::reference`].
//!
//! # Why one uniform buffer per pass
//!
//! `queue.write_buffer` is ordered against the encoder's **submission**, not
//! against the passes inside it. Eleven writes to one buffer would all land
//! before any pass ran, and every level would render with the last level's
//! texel size — a bug that looks like "the bloom is a bit soft" and is invisible
//! in a still. [`crate::post_chain`] pays for two buffers for exactly this
//! reason; a pyramid needs `2n - 1`.
//!
//! # Targets are `Rgba16Float`, unconditionally
//!
//! `pass.js`'s `hdrTarget` is `THREE.HalfFloatType`, so the source's mips are
//! half-float and [`crate::bloom_pyramid::half_storage`] models that rounding on
//! the CPU side. There is no LDR substitute here: an 8-bit pyramid cannot hold a
//! prefiltered highlight of 24.0 at all, so degrading the *format* would silently
//! change the algorithm rather than its precision. A device without
//! [`axiom_host::RenderCapability::HdrTargets`] should decline the pyramid, not
//! run a fake one — the caller's decision, made from the profile, not this
//! struct's.
//!
//! # What is not ported here
//!
//! The render-scale sub-rect. [`crate::post_chain`] allocates at full tier size
//! and draws into the lower-left `live` fraction; this chain sizes its mips from
//! the source extent it is given. Threading `live` through eleven passes is real
//! work with its own parity story and it belongs with whoever wires this into the
//! frame graph, so it is called out rather than half-done.

use crate::bloom_pyramid::schedule::{mip_sizes, upsample_step};
use crate::bloom_pyramid::wgsl::{BLOOM_PASSES_WGSL, BLOOM_PYRAMID_WGSL};
use crate::bloom_pyramid::BloomTuning;

/// Every level of the source's pyramid is a `THREE.HalfFloatType` target.
pub(crate) const LEVEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// One mip: its texture, its view and its extent.
///
/// The texture is kept alongside the view because a view cannot be the source of
/// a `copy_texture_to_buffer`, and reading level 0 back is how
/// [`crate::bloom_pyramid::parity`] proves the chain.
struct Level {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}

/// One recorded pass: what it reads, what it writes, and the uniform it reads
/// its texel size and tuning from.
struct Step {
    group: wgpu::BindGroup,
    params: wgpu::Buffer,
    target: usize,
    /// The four `vec4`s this pass's uniform carries, minus the tuning the
    /// caller supplies per frame. `texel` is fixed by the pyramid's geometry;
    /// `karis` and `weight` by its schedule.
    texel: [f32; 2],
    karis: f32,
    weight: f32,
}

/// `Bloom` — the pipelines, the mips, and the ordered passes over them.
pub(crate) struct BloomPyramid {
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    levels: Vec<Level>,
    descend: Vec<Step>,
    ascend: Vec<Step>,
}

impl std::fmt::Debug for BloomPyramid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BloomPyramid")
            .field(
                "levels",
                &self.levels.iter().map(|l| l.size).collect::<Vec<(u32, u32)>>(),
            )
            .finish_non_exhaustive()
    }
}

impl BloomPyramid {
    /// `setSize` plus the pipeline build: a pyramid of at most `levels` mips over
    /// a `source_size` scene target bound at `source_view`.
    ///
    /// Returns `None` for a zero-level budget — the source's
    /// `if (n === 0) return null`.
    pub(crate) fn new(
        device: &wgpu::Device,
        source_view: &wgpu::TextureView,
        source_size: (u32, u32),
        levels: usize,
    ) -> Option<BloomPyramid> {
        let sizes = mip_sizes(source_size.0, source_size.1, levels);
        let count = sizes.len();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-bloom-pyramid"),
            source: wgpu::ShaderSource::Wgsl([BLOOM_PYRAMID_WGSL, BLOOM_PASSES_WGSL].concat().into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-bloom-sampler"),
            // Clamped: a tap that walks off the edge repeats the border texel
            // rather than wrapping a bright corner to the far side.
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-bloom-bgl"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-bloom-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("axiom-bloom-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("bloom_vs"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: LEVEL_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let levels: Vec<Level> = sizes
            .iter()
            .map(|&(width, height)| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("axiom-bloom-level"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: LEVEL_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("axiom-bloom-level-view"),
                    ..Default::default()
                });
                Level {
                    texture,
                    view,
                    size: (width, height),
                }
            })
            .collect();

        let step = |source: &wgpu::TextureView, source_size: (u32, u32), target: usize, karis: f32, weight: f32, radius: f32| {
            let params = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-bloom-params"),
                size: std::mem::size_of::<[f32; 12]>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-bloom-bg"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                ],
            });
            Step {
                group,
                params,
                target,
                // `t = uTexel * uRadius`: the reciprocal first, then the radius —
                // the source's grouping, and a different `f32` from `radius / w`.
                texel: [
                    (1.0 / source_size.0 as f32) * radius,
                    (1.0 / source_size.1 as f32) * radius,
                ],
                karis,
                weight,
            }
        };

        let descend: Vec<Step> = (0..count)
            .map(|index| {
                // Level 0 reads the scene; every other level reads the one above.
                let source = [&levels.get(index.wrapping_sub(1)).unwrap_or(&levels[0]).view, source_view]
                    [usize::from(index == 0)];
                let size = [
                    levels.get(index.wrapping_sub(1)).unwrap_or(&levels[0]).size,
                    source_size,
                ][usize::from(index == 0)];
                step(source, size, index, f32::from(u8::from(index == 0)), 1.0, 1.0)
            })
            .collect();
        let ascend: Vec<Step> = (1..count)
            .rev()
            .map(|index| {
                let (radius, weight) = upsample_step(index, count);
                step(&levels[index].view, levels[index].size, index - 1, 0.0, weight, radius)
            })
            .collect();

        (count > 0).then(|| BloomPyramid {
            down: pipeline("bloom_down_fs", None),
            up: pipeline(
                "bloom_up_fs",
                // `THREE.NormalBlending` with `premultipliedAlpha = false`:
                // `src·α + dst·(1-α)`. The fixed-function blender performs the
                // energy-preserving accumulation the source relies on.
                Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
            ),
            levels,
            descend,
            ascend,
        })
    }

    /// The finished pyramid's level 0 — `this.texture = this.mips[0].rt.texture`.
    pub(crate) fn output(&self) -> &wgpu::TextureView {
        &self.levels[0].view
    }

    /// Level 0's extent, which is the bloom texture a composite samples.
    pub(crate) fn output_size(&self) -> (u32, u32) {
        self.levels[0].size
    }

    /// Copy level 0 into `readback`, `bytes_per_row` apart.
    ///
    /// The chain owns its textures and hands out only views, so this is the one
    /// affordance that lets a caller read the finished pyramid back. It exists
    /// for [`crate::bloom_pyramid::parity`], which is the only reason to want
    /// the bits on the CPU at all.
    pub(crate) fn copy_output_to_buffer(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        readback: &wgpu::Buffer,
        bytes_per_row: u32,
    ) {
        let level = &self.levels[0];
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &level.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(level.size.1),
                },
            },
            wgpu::Extent3d {
                width: level.size.0,
                height: level.size.1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// `Bloom.render` — every downsample, then every blended upsample.
    pub(crate) fn record(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        tuning: BloomTuning,
    ) {
        let record = |steps: &Vec<Step>,
                      pipeline: &wgpu::RenderPipeline,
                      load: wgpu::LoadOp<wgpu::Color>,
                      encoder: &mut wgpu::CommandEncoder| {
            steps.iter().for_each(|step| {
                queue.write_buffer(
                    &step.params,
                    0,
                    bytemuck::cast_slice(&[
                        step.texel[0],
                        step.texel[1],
                        0.0,
                        0.0,
                        step.karis,
                        tuning.threshold,
                        tuning.knee,
                        tuning.exposure,
                        step.weight,
                        0.0,
                        0.0,
                        0.0,
                    ]),
                );
                let target = &self.levels[step.target];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-bloom-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_viewport(0.0, 0.0, target.size.0 as f32, target.size.1 as f32, 0.0, 1.0);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &step.group, &[]);
                pass.draw(0..3, 0..1);
            });
        };
        // The downsamples overwrite their target outright; the upsamples blend
        // into a level the downsample already filled, so they must LOAD it.
        record(&self.descend, &self.down, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), encoder);
        record(&self.ascend, &self.up, wgpu::LoadOp::Load, encoder);
    }
}
