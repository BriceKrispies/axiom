//! **The contact-shadow pass, on a real device** — the runner for
//! [`super::CONTACT_PASS_WGSL`] and [`super::CONTACT_BLUR_WGSL`].
//!
//! [`super`] is the semantic definition (the CPU reference) and the shader text;
//! `super::parity` pins the two against each other on a real adapter. This file
//! is the third piece, in exactly the relationship [`crate::gtao::pass`] has with
//! [`crate::gtao::reference`]: the pipelines, targets and bind groups that put
//! those shaders in a frame.
//!
//! # The chain
//!
//! ```text
//! G-buffer depth + normal ──> march ──> bilateral X ──> bilateral Y ──> shadow
//! ```
//!
//! Three passes over three **full-resolution** `Rg16Float` targets.
//!
//! # Full resolution, unlike the ambient occlusion beside it
//!
//! [`crate::gtao::pass`] renders at `w >> 1` and this does not, and that is the
//! source's own split rather than an oversight: `gtao.js` allocates at `w >> 1`,
//! `contact.js` allocates at the full size. The reason is what each signal *is*.
//! Ambient occlusion is low-frequency — a bilateral blur is about to remove
//! anything the extra samples would have resolved. A contact shadow is the
//! opposite: it exists precisely to put back **the last few centimetres** of
//! occlusion in the seam where a prop meets the floor, which is a handful of
//! pixels wide. Half-resolving it removes the only thing it had to say.
//!
//! # There is no history, and therefore no fifth target
//!
//! The occlusion chain needs a history target because its temporal pass
//! accumulates across frames and a render pass may not read and write one
//! texture. The march has no temporal stage at all — its dither is resolved by
//! the two bilateral passes within the frame — so the chain is a straight line
//! and each pass reads a target no later pass writes.
//!
//! # `&self` and the frame counter
//!
//! [`SceneRenderer`](crate::scene_renderer::SceneRenderer) records with `&self`,
//! so the dither's frame index lives in a [`Cell`], counted modulo
//! [`super::CONTACT_FRAME_CYCLE`] by [`super::ContactParams::at_frame`]. It only
//! ever feeds the noise — a wrong value makes a different dither, never a wrong
//! pixel.

use std::cell::Cell;

use super::{
    pack_contact_blur_uniform, pack_contact_uniform, ContactParams, CONTACT_BLUR_UNIFORM_FLOATS,
    CONTACT_BLUR_WGSL, CONTACT_COMMON_WGSL, CONTACT_FRAME_CYCLE, CONTACT_PASS_WGSL,
    CONTACT_UNIFORM_FLOATS,
};

/// The working format: `THREE.HalfFloatType` + `THREE.RGFormat` in the source.
/// `r` is the shadow multiplier and `g` is the linear view depth the bilateral
/// uses as its edge-stopping signal — see [`super`]'s header on why the width is
/// part of the algorithm rather than a storage choice.
const CONTACT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// `floats` rounded up to a whole 16-byte uniform slot.
const fn slot(floats: usize) -> u64 {
    ((floats as u64 * 4) + 15) & !15
}

/// The target size for a scene of `scene` pixels: **the same size**, floored at
/// one.
///
/// Written as a named function rather than inlined because the *absence* of a
/// downscale here is a decision, not an omission — see the module header — and a
/// decision with a name is one a later change has to argue with. The floor is
/// what stops a zero-sized surface (a minimised browser window) asking for a
/// zero-extent texture, which wgpu rejects.
fn target_size(scene: (u32, u32)) -> (u32, u32) {
    (scene.0.max(1), scene.1.max(1))
}

/// One full-resolution `Rg16Float` target and its view.
struct ContactTarget {
    view: wgpu::TextureView,
}

impl ContactTarget {
    fn new(device: &wgpu::Device, size: (u32, u32), label: &str) -> ContactTarget {
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
            format: CONTACT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        ContactTarget {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        }
    }
}

/// The compiled contact-shadow chain for one scene resolution.
pub(crate) struct ContactPass {
    march: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    march_uniform: wgpu::Buffer,
    /// Two blur uniforms, one per axis — **not one rewritten between passes**.
    /// `queue.write_buffer` is ordered against submission, not against the passes
    /// inside an encoder, so a single buffer would give both passes the last
    /// write and blur one axis twice. `crate::gtao::pass` and `crate::post_chain`
    /// both learned this the same way.
    blur_x_uniform: wgpu::Buffer,
    blur_y_uniform: wgpu::Buffer,
    march_group: wgpu::BindGroup,
    blur_x_group: wgpu::BindGroup,
    blur_y_group: wgpu::BindGroup,
    /// The march writes here; bilateral X reads it.
    raw: ContactTarget,
    /// Bilateral X writes here; bilateral Y reads it.
    scratch: ContactTarget,
    /// Bilateral Y writes here. This is what the main pass samples.
    resolved: ContactTarget,
    size: (u32, u32),
    frame: Cell<u32>,
}

impl std::fmt::Debug for ContactPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContactPass")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl ContactPass {
    /// Compile the chain against a G-buffer's depth and normal views, at the
    /// **full** scene resolution.
    pub(crate) fn new(
        device: &wgpu::Device,
        scene: (u32, u32),
        depth_view: &wgpu::TextureView,
        normal_view: &wgpu::TextureView,
    ) -> ContactPass {
        let size = target_size(scene);

        let march_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-contact-march"),
            source: wgpu::ShaderSource::Wgsl(
                [CONTACT_COMMON_WGSL, CONTACT_PASS_WGSL].concat().into(),
            ),
        });
        // The bilateral carries its own bindings and calls nothing from `COMMON`,
        // so it compiles as its own module — which is what `super`'s header says
        // it is for.
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-contact-bilateral"),
            source: wgpu::ShaderSource::Wgsl(CONTACT_BLUR_WGSL.into()),
        });

        // NEAREST on both G-buffer fetches. `prepass.js` sets `NearestFilter` on
        // both attachments, and the depth slot is `R32Float`, which is not
        // filterable at all — a filtering sampler on it is a validation error,
        // not merely a different image.
        let point = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-contact-point"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        // LINEAR, because `contact_blur_pixel` — the reference this shader is
        // proved against — samples its five taps BILINEARLY. A nearest sampler
        // here would still produce a plausible blur and would silently disagree
        // with the reference the parity harness pins the WGSL to.
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-contact-linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        // The march's two G-buffer textures are declared UNFILTERABLE, matching
        // its nearest sampler: `R32Float` genuinely is unfilterable, and claiming
        // otherwise would make the layout ask for a capability the device need
        // not have. The march's binding order is the WGSL's — uniform, sampler,
        // depth, normal — not `gtao`'s uniform-textures-sampler.
        let unfilterable = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let march_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-contact-march-bgl"),
            entries: &[
                uniform_entry,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                unfilterable(2),
                unfilterable(3),
            ],
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-contact-bilateral-bgl"),
            entries: &[
                uniform_entry,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let buffer = |label: &str, floats: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: slot(floats),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let march_uniform = buffer("axiom-contact-march-u", CONTACT_UNIFORM_FLOATS);
        let blur_x_uniform = buffer("axiom-contact-blur-x-u", CONTACT_BLUR_UNIFORM_FLOATS);
        let blur_y_uniform = buffer("axiom-contact-blur-y-u", CONTACT_BLUR_UNIFORM_FLOATS);

        let raw = ContactTarget::new(device, size, "axiom-contact-raw");
        let scratch = ContactTarget::new(device, size, "axiom-contact-scratch");
        let resolved = ContactTarget::new(device, size, "axiom-contact-resolved");

        let march_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-contact-march-bg"),
            layout: &march_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: march_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&point),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(normal_view),
                },
            ],
        });
        let blur_group = |label: &str, uniform: &wgpu::Buffer, src: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &blur_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&linear),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                ],
            })
        };
        let blur_x_group = blur_group("axiom-contact-blur-x-bg", &blur_x_uniform, &raw.view);
        let blur_y_group = blur_group("axiom-contact-blur-y-bg", &blur_y_uniform, &scratch.view);

        let pipeline = |label: &str,
                        bgl: &wgpu::BindGroupLayout,
                        shader: &wgpu::ShaderModule,
                        vertex: &str,
                        fragment: &str| {
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
                    entry_point: Some(vertex),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(fragment),
                    targets: &[Some(CONTACT_FORMAT.into())],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };

        ContactPass {
            march: pipeline(
                "axiom-contact-march-pipeline",
                &march_layout,
                &march_shader,
                "contact_vs",
                "contact_fs",
            ),
            blur: pipeline(
                "axiom-contact-bilateral-pipeline",
                &blur_layout,
                &blur_shader,
                "contact_blur_vs",
                "contact_blur_fs",
            ),
            march_uniform,
            blur_x_uniform,
            blur_y_uniform,
            march_group,
            blur_x_group,
            blur_y_group,
            raw,
            scratch,
            resolved,
            size,
            frame: Cell::new(0),
        }
    }

    /// The finished shadow, as the main pass samples it: `r` is the multiplier
    /// the sun term takes, `g` the depth lane the bilateral needed.
    pub(crate) fn resolved_view(&self) -> &wgpu::TextureView {
        &self.resolved.view
    }

    /// Record the whole chain into `encoder`.
    ///
    /// `proj` and `proj_inv` are column-major, and `sun_dir_view` is the
    /// direction **toward** the sun in view space, **normalised** — the march
    /// scales it by `len / 14` to get its step, so a non-unit vector silently
    /// rescales the ray and the pass reaches the wrong distance. See
    /// [`super::ContactInputs::sun_dir_view`].
    pub(crate) fn record(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        proj: [f32; 16],
        proj_inv: [f32; 16],
        sun_dir_view: [f32; 3],
    ) {
        // `uParams.value.z = frame % 64` — the dither's temporal cycle, reduced
        // by `ContactParams::at_frame` so the modulo cannot be lost here.
        let frame = self.frame.get();
        self.frame.set((frame + 1) % CONTACT_FRAME_CYCLE);
        let params = ContactParams::at_frame(frame);

        let size = [self.size.0 as f32, self.size.1 as f32];
        queue.write_buffer(
            &self.march_uniform,
            0,
            bytemuck::cast_slice(&pack_contact_uniform(
                &proj,
                &proj_inv,
                sun_dir_view,
                params,
                size,
            )),
        );
        // Horizontal first, then vertical — the source's order, and the order the
        // targets below chain in.
        queue.write_buffer(
            &self.blur_x_uniform,
            0,
            bytemuck::cast_slice(&pack_contact_blur_uniform([1.0 / size[0], 0.0], size)),
        );
        queue.write_buffer(
            &self.blur_y_uniform,
            0,
            bytemuck::cast_slice(&pack_contact_blur_uniform([0.0, 1.0 / size[1]], size)),
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
        pass(
            "axiom-contact-march",
            &self.march,
            &self.march_group,
            &self.raw.view,
        );
        pass(
            "axiom-contact-blur-x",
            &self.blur,
            &self.blur_x_group,
            &self.scratch.view,
        );
        pass(
            "axiom-contact-blur-y",
            &self.blur,
            &self.blur_y_group,
            &self.resolved.view,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        slot, target_size, CONTACT_BLUR_UNIFORM_FLOATS, CONTACT_FORMAT, CONTACT_UNIFORM_FLOATS,
    };

    /// The uniform slots are what the WGSL structs declare, rounded to the
    /// 16-byte alignment a uniform buffer requires.
    ///
    /// `176` is `mat4 + mat4 + vec4 + vec4 + vec2 + vec2` exactly, with no
    /// rounding at all — which is the point of the sun's explicit `w` pad and the
    /// tail pad that `super::super::pack_contact_uniform` writes: a block whose
    /// declared size already lands on the alignment cannot be mis-bound by a
    /// packer and a shader disagreeing about where the padding went.
    #[test]
    fn the_uniform_slots_match_the_declared_structs() {
        assert_eq!(
            CONTACT_UNIFORM_FLOATS, 44,
            "proj + proj_inv + sun vec4 + params vec4 + size vec2 + pad vec2"
        );
        assert_eq!(slot(CONTACT_UNIFORM_FLOATS), 176);
        assert_eq!(CONTACT_UNIFORM_FLOATS * 4, 176, "no rounding is applied");
        assert_eq!(
            slot(CONTACT_BLUR_UNIFORM_FLOATS),
            16,
            "direction + size are two vec2s in one slot"
        );
        // The rounding is real for a block that needs it.
        assert_eq!(slot(1), 16);
        assert_eq!(slot(5), 32);
    }

    /// **The contact targets are FULL resolution**, unlike the occlusion chain's
    /// half. A contact shadow is a few pixels wide in the seam where a prop meets
    /// the floor; half-resolving it removes the only signal it carries. Pinned so
    /// a later "make it match GTAO" tidy-up has to argue with a test.
    ///
    /// The odd sizes are the ones that would expose a `>> 1`: `1281 >> 1` is
    /// `640`, and this must be `1281`.
    #[test]
    fn the_contact_targets_are_the_full_scene_size() {
        [(1920_u32, 1080_u32), (1281, 721), (1, 1)]
            .iter()
            .for_each(|scene| {
                assert_eq!(target_size(*scene), *scene, "no downscale at {scene:?}");
            });
        // A zero-extent texture is a wgpu validation error, so a minimised
        // surface still gets a 1x1 chain rather than killing the frame.
        assert_eq!(target_size((0, 0)), (1, 1));
        assert_eq!(target_size((1920, 0)), (1920, 1));
        assert_eq!(
            CONTACT_FORMAT,
            wgpu::TextureFormat::Rg16Float,
            "the source's HalfFloatType + RGFormat: shadow in r, view depth in g"
        );
    }
}
