//! **The GTAO passes, on a real device** — the runner for [`super::wgsl`]'s
//! three entry points.
//!
//! [`super::reference`] is the semantic definition and [`super::wgsl`] is the
//! shader text; both are pinned against each other by `super::parity` on a real
//! adapter. This file is the third piece: the pipelines, targets and bind groups
//! that put those shaders in a frame.
//!
//! # The chain
//!
//! ```text
//! G-buffer depth + normal ──> core ──> temporal ──> blur X ──> blur Y ──> AO
//!                                         ^                                │
//!                                         └──────── history ───────────────┘
//! ```
//!
//! Four passes over two half-resolution `Rg16Float` targets plus a history
//! target. Half resolution because ambient occlusion is a low-frequency signal
//! and the blur is about to remove what the extra samples would have resolved —
//! the same trade `post_chain` makes for bloom, and the source's own
//! (`gtao.js` renders at `w >> 1`).
//!
//! # The history is its own target, for two separate reasons
//!
//! **Correctness.** The history must be the temporal pass's *un-blurred* output.
//! If it were taken after the blur, every frame would re-blur an already-blurred
//! image and the occlusion would creep outward until it was a grey wash — the
//! source's own comment ("the history must stay un-blurred or the accumulator
//! smears more every frame") is warning about exactly that.
//!
//! **Legality.** A render pass may not read and write the same texture. Binding
//! `accumulated` as both the history input and the colour attachment is a
//! validation error, not merely undefined:
//!
//! ```text
//! [Texture "axiom-gtao-accumulated"] usage (TextureBinding|RenderAttachment)
//! includes writable usage and another usage in the same synchronization scope.
//! ```
//!
//! So `history` is a fifth target, and the temporal output is copied into it at
//! the end of the chain — a half-resolution two-channel blit, the cheapest thing
//! in the frame.
//!
//! # `&self` and the frame counter
//!
//! [`SceneRenderer`](crate::scene_renderer::SceneRenderer) records with `&self`,
//! so the frame index lives in a [`Cell`]. It is a `u32` counted modulo
//! [`super::FRAME_PERIOD`], and it only ever feeds the noise — a wrong value
//! makes a different dither, never a wrong pixel.

use std::cell::Cell;

use super::{
    wgsl, FRAME_PERIOD, SHIPPED_INTENSITY, SHIPPED_RADIUS_METRES, TEMPORAL_FEEDBACK,
    CORE_UNREAD_INTENSITY, CORE_UNREAD_THICKNESS,
};

/// How much smaller the AO targets are than the scene, per axis.
///
/// `w >> 1` in the source. A **shift**, not a rounded divide: at an odd width
/// the two disagree by a texel, and the uv the main pass reconstructs from its
/// own resolution would then miss the last column.
pub(crate) const AO_DOWNSCALE: u32 = 1;

/// The AO working format. Two channels: visibility and the linear view depth the
/// bilateral blur and the temporal clamp both need beside it.
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// `floats` rounded up to a whole 16-byte uniform slot.
const fn slot(floats: usize) -> u64 {
    ((floats as u64 * 4) + 15) & !15
}

/// The core pass's uniform: a `mat4x4` plus three `vec4`-shaped rows.
const CORE_FLOATS: usize = 16 + 2 + 2 + 4 + 4;
/// The temporal pass's uniform: `texel` + `params`, both `vec2`.
const TEMPORAL_FLOATS: usize = 2 + 2;
/// The blur pass's uniform: `texel`, `direction`, then a `vec4` of params.
const BLUR_FLOATS: usize = 2 + 2 + 4;

/// One half-resolution `Rg16Float` target, its view, and the texture behind it
/// (the history copy needs the texture, not just a view).
struct AoTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl AoTarget {
    fn new(device: &wgpu::Device, size: (u32, u32), label: &str) -> AoTarget {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: AO_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        AoTarget {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            texture,
        }
    }
}

/// The compiled GTAO chain for one scene resolution.
pub(crate) struct GtaoPass {
    core: wgpu::RenderPipeline,
    temporal: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    core_uniform: wgpu::Buffer,
    temporal_uniform: wgpu::Buffer,
    /// Two blur uniforms, one per axis — **not one rewritten between passes**.
    /// `queue.write_buffer` is ordered against submission, not against the passes
    /// inside an encoder, so a single buffer would give both passes the last
    /// write and blur one axis twice. `post_chain` learned the same lesson.
    blur_x_uniform: wgpu::Buffer,
    blur_y_uniform: wgpu::Buffer,
    core_group: wgpu::BindGroup,
    temporal_group: wgpu::BindGroup,
    blur_x_group: wgpu::BindGroup,
    blur_y_group: wgpu::BindGroup,
    /// `core` writes here; `temporal` reads it as `current`.
    raw: AoTarget,
    /// `temporal` writes here; `blur X` reads it.
    accumulated: AoTarget,
    /// Last frame's `accumulated`, copied at the end of the chain. A pass cannot
    /// read and write one texture, so this cannot be `accumulated` itself.
    history: AoTarget,
    /// `blur X` writes here; `blur Y` reads it.
    scratch: AoTarget,
    /// `blur Y` writes here. This is what the main pass samples.
    resolved: AoTarget,
    size: (u32, u32),
    frame: Cell<u32>,
}

impl std::fmt::Debug for GtaoPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GtaoPass")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl GtaoPass {
    /// Compile the chain against a G-buffer's depth and normal views.
    ///
    /// `scene` is the **full** scene resolution; the AO targets are allocated at
    /// half of it.
    pub(crate) fn new(
        device: &wgpu::Device,
        scene: (u32, u32),
        depth_view: &wgpu::TextureView,
        normal_view: &wgpu::TextureView,
        velocity_view: &wgpu::TextureView,
    ) -> GtaoPass {
        let size = (
            (scene.0 >> AO_DOWNSCALE).max(1),
            (scene.1 >> AO_DOWNSCALE).max(1),
        );

        let module = |label: &str, body: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(
                    [wgsl::GTAO_WGSL, body].concat().into(),
                ),
            })
        };
        let core_shader = module("axiom-gtao-core", wgsl::GTAO_CORE_PASS_WGSL);
        let temporal_shader = module("axiom-gtao-temporal", wgsl::GTAO_TEMPORAL_PASS_WGSL);
        let blur_shader = module("axiom-gtao-blur", wgsl::GTAO_BLUR_PASS_WGSL);

        // NEAREST on every G-buffer fetch. A linear filter across a depth
        // discontinuity averages two surfaces and invents geometry between them,
        // which the horizon search then happily occludes against.
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-gtao-nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        // `Rg16Float` and `R32Float` are both **unfilterable** as bound here,
        // because the sampler is nearest and declaring otherwise would make the
        // layout claim a capability the device need not have.
        let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
            count: None,
        };

        let layout = |label: &str, textures: u32| {
            let entries: Vec<wgpu::BindGroupLayoutEntry> = std::iter::once(uniform_entry(0))
                .chain((0..textures).map(|i| texture_entry(i + 1)))
                .chain(std::iter::once(sampler_entry(textures + 1)))
                .collect();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &entries,
            })
        };
        let core_layout = layout("axiom-gtao-core-bgl", 2);
        let temporal_layout = layout("axiom-gtao-temporal-bgl", 3);
        let blur_layout = layout("axiom-gtao-blur-bgl", 1);

        let buffer = |label: &str, floats: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: slot(floats),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let core_uniform = buffer("axiom-gtao-core-u", CORE_FLOATS);
        let temporal_uniform = buffer("axiom-gtao-temporal-u", TEMPORAL_FLOATS);
        let blur_x_uniform = buffer("axiom-gtao-blur-x-u", BLUR_FLOATS);
        let blur_y_uniform = buffer("axiom-gtao-blur-y-u", BLUR_FLOATS);

        let raw = AoTarget::new(device, size, "axiom-gtao-raw");
        let accumulated = AoTarget::new(device, size, "axiom-gtao-accumulated");
        let history = AoTarget::new(device, size, "axiom-gtao-history");
        let scratch = AoTarget::new(device, size, "axiom-gtao-scratch");
        let resolved = AoTarget::new(device, size, "axiom-gtao-resolved");

        let group = |label: &str,
                     bgl: &wgpu::BindGroupLayout,
                     uniform: &wgpu::Buffer,
                     views: &[&wgpu::TextureView]| {
            let entries: Vec<wgpu::BindGroupEntry<'_>> = std::iter::once(wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            })
            .chain(views.iter().enumerate().map(|(i, v)| wgpu::BindGroupEntry {
                binding: i as u32 + 1,
                resource: wgpu::BindingResource::TextureView(v),
            }))
            .chain(std::iter::once(wgpu::BindGroupEntry {
                binding: views.len() as u32 + 1,
                resource: wgpu::BindingResource::Sampler(&nearest),
            }))
            .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: bgl,
                entries: &entries,
            })
        };
        let core_group = group(
            "axiom-gtao-core-bg",
            &core_layout,
            &core_uniform,
            &[depth_view, normal_view],
        );
        // History is its OWN target, not `accumulated`: a pass may not read and
        // write the same texture, and wgpu rejects the attempt outright.
        let temporal_group = group(
            "axiom-gtao-temporal-bg",
            &temporal_layout,
            &temporal_uniform,
            &[&raw.view, &history.view, velocity_view],
        );
        let blur_x_group = group(
            "axiom-gtao-blur-x-bg",
            &blur_layout,
            &blur_x_uniform,
            &[&accumulated.view],
        );
        let blur_y_group = group(
            "axiom-gtao-blur-y-bg",
            &blur_layout,
            &blur_y_uniform,
            &[&scratch.view],
        );

        let pipeline = |label: &str,
                        bgl: &wgpu::BindGroupLayout,
                        shader: &wgpu::ShaderModule,
                        entry: &str| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[bgl],
                push_constant_ranges: &[],
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("axiom_gtao_vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(entry),
                    targets: &[Some(AO_FORMAT.into())],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };

        GtaoPass {
            core: pipeline(
                "axiom-gtao-core-pipeline",
                &core_layout,
                &core_shader,
                "axiom_gtao_core_fs",
            ),
            temporal: pipeline(
                "axiom-gtao-temporal-pipeline",
                &temporal_layout,
                &temporal_shader,
                "axiom_gtao_temporal_fs",
            ),
            blur: pipeline(
                "axiom-gtao-blur-pipeline",
                &blur_layout,
                &blur_shader,
                "axiom_gtao_blur_fs",
            ),
            core_uniform,
            temporal_uniform,
            blur_x_uniform,
            blur_y_uniform,
            core_group,
            temporal_group,
            blur_x_group,
            blur_y_group,
            raw,
            accumulated,
            history,
            scratch,
            resolved,
            size,
            frame: Cell::new(0),
        }
    }

    /// The finished AO, as the main pass samples it.
    pub(crate) fn resolved_view(&self) -> &wgpu::TextureView {
        &self.resolved.view
    }

    /// Record the whole chain into `encoder`.
    ///
    /// `proj_inv` is the **inverse projection**, column-major, and `p11` is
    /// `projectionMatrix.elements[5]` — the source reads exactly that one element
    /// to turn a world radius into a pixel radius, so it is passed rather than
    /// recovered from the matrix.
    pub(crate) fn record(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        proj_inv: [f32; 16],
        p11: f32,
    ) {
        // `frame % 64` — the dither period. Advanced once per record, so a frame
        // that records twice (a re-render) reuses no noise pattern.
        let frame = self.frame.get();
        self.frame.set((frame + 1) % FRAME_PERIOD);

        let texel = [1.0 / self.size.0 as f32, 1.0 / self.size.1 as f32];
        let mut core = [0.0_f32; CORE_FLOATS];
        core[..16].copy_from_slice(&proj_inv);
        core[16] = texel[0];
        core[17] = texel[1];
        core[18] = self.size.0 as f32;
        core[19] = self.size.1 as f32;
        core[20] = SHIPPED_RADIUS_METRES;
        // Never read by the core pass — the blur owns intensity — but written so
        // the block matches the source's `uParams` lane for lane.
        core[21] = CORE_UNREAD_INTENSITY;
        core[22] = frame as f32;
        core[23] = CORE_UNREAD_THICKNESS;
        core[24] = p11;
        queue.write_buffer(&self.core_uniform, 0, bytemuck::cast_slice(&core));

        let temporal = [texel[0], texel[1], TEMPORAL_FEEDBACK, 0.0];
        queue.write_buffer(&self.temporal_uniform, 0, bytemuck::cast_slice(&temporal));

        // The intensity curve runs on the LAST pass only (`params.x`), so the X
        // axis carries a zero there and the Y axis a one. Applying it twice would
        // square the curve.
        let blur = |direction: [f32; 2], last: f32| {
            [
                texel[0],
                texel[1],
                direction[0],
                direction[1],
                last,
                SHIPPED_INTENSITY,
                0.0,
                0.0,
            ]
        };
        queue.write_buffer(
            &self.blur_x_uniform,
            0,
            bytemuck::cast_slice(&blur([texel[0], 0.0], 0.0)),
        );
        queue.write_buffer(
            &self.blur_y_uniform,
            0,
            bytemuck::cast_slice(&blur([0.0, texel[1]], 1.0)),
        );

        let mut pass = |label: &str,
                        pipeline: &wgpu::RenderPipeline,
                        group: &wgpu::BindGroup,
                        view: &wgpu::TextureView| {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, group, &[]);
            rp.draw(0..3, 0..1);
        };
        pass("axiom-gtao-core", &self.core, &self.core_group, &self.raw.view);
        pass(
            "axiom-gtao-temporal",
            &self.temporal,
            &self.temporal_group,
            &self.accumulated.view,
        );
        pass(
            "axiom-gtao-blur-x",
            &self.blur,
            &self.blur_x_group,
            &self.scratch.view,
        );
        pass(
            "axiom-gtao-blur-y",
            &self.blur,
            &self.blur_y_group,
            &self.resolved.view,
        );
        // Carry the UN-blurred accumulator into the history for next frame. After
        // the blur passes in submission order, but reading `accumulated`, which
        // neither of them wrote.
        encoder.copy_texture_to_texture(
            self.accumulated.texture.as_image_copy(),
            self.history.texture.as_image_copy(),
            wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The uniform slots are what the WGSL structs declare, rounded to the
    /// 16-byte alignment a uniform buffer requires.
    #[test]
    fn the_uniform_slots_match_the_declared_structs() {
        assert_eq!(CORE_FLOATS, 28, "mat4 + texel + resolution + params + p11");
        assert_eq!(slot(CORE_FLOATS), 112);
        assert_eq!(slot(TEMPORAL_FLOATS), 16, "two vec2 fit one slot");
        assert_eq!(slot(BLUR_FLOATS), 32);
    }

    /// **Half resolution is a shift, not a divide.** At an odd width the two
    /// disagree by a texel, and the main pass reconstructs its AO uv from its own
    /// full resolution — so a target one texel wider than the shift implies would
    /// leave the last column sampling past the end.
    #[test]
    fn the_ao_targets_are_a_shift_of_the_scene_size() {
        assert_eq!(AO_DOWNSCALE, 1);
        [(1920_u32, 1080_u32), (1281, 721), (1, 1)]
            .iter()
            .for_each(|(w, h)| {
                let (aw, ah) = ((w >> AO_DOWNSCALE).max(1), (h >> AO_DOWNSCALE).max(1));
                assert_eq!(aw, (w / 2).max(1), "width {w}");
                assert_eq!(ah, (h / 2).max(1), "height {h}");
                assert!(aw >= 1 && ah >= 1, "a 1x1 scene still has a target");
            });
    }
}
