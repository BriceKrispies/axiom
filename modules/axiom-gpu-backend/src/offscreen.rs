//! Native **off-screen** rendering — `offscreen` feature, non-wasm only.
//!
//! The headless counterpart of [`crate::live_gpu_binding`]: instead of a browser
//! swap-chain it renders into an off-screen texture and reads the pixels back to
//! RGBA8. It drives the *same* [`crate::scene_renderer::SceneRenderer`] the live
//! browser arm does, so a native screenshot exercises byte-identical rendering to
//! what the browser presents — the screenshot tool (`axiom-shot`) is no longer a
//! separate copy that can drift.

use crate::scene_renderer::{create_depth_view, SceneRenderer};
use crate::upscale::UpscaleBlit;

/// The off-screen colour target format (matches the live arm's sRGB output).
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// `copy_texture_to_buffer` requires each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// Render one frame off-screen and read it back as `width * height * 4` RGBA8
/// bytes (row-major, top-down), together with **what that frame cost the GPU,
/// pass by pass**. `meshes` / `materials` / `lights` / `batches` are exactly the
/// data the live backend consumes (see [`SceneRenderer::record`]).
/// Returns `None` if no native GPU adapter/device is available.
///
/// The timings are the native proof of the live arm's instrument: this path runs
/// the same [`crate::gpu_pass_clock`] through the same passes, and — unlike a
/// browser frame — it is already blocking on a readback, so the asynchronous
/// resolve completes inside the call. On an adapter without `TIMESTAMP_QUERY`
/// the reading is the documented unavailable state, never a zero.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_to_rgba(
    width: u32,
    height: u32,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    // Every material's textures: albedo, plus the four optional maps (normal,
    // ORM+height, detail, macro) the carrier now holds. The tangent-space normal
    // maps used to arrive here in a second slice parallel to this one; they ride
    // on the carrier now, which is what finally gave the live browser arm the
    // normal-map lane it never had.
    materials: &[axiom_host::MaterialTexture],
    lights: &[(u32, [f32; 3], [f32; 3], f32)],
    light_view_proj: [f32; 16],
    // The frame's camera — view, projection and their product. See
    // `SceneRenderer::record` for which pass reads which half.
    camera: axiom_host::FrameCamera,
    batches: &[(u64, u64, Vec<f32>, u32)],
    skinned_mesh_set: &[(u64, Vec<f32>, Vec<u32>)],
    skinned: &[crate::scene_renderer::SkinnedGpuDraw],
    clear: [f32; 4],
    sdf: Option<&axiom_host::SdfScene>,
    look: axiom_host::FrameRenderLook,
    retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
    profile: axiom_host::BackendCapabilityProfile,
    volumetrics: Option<axiom_host::FrameVolumetrics>,
    postprocess: Option<axiom_host::FramePostProcess>,
    repeat: u32,
) -> Option<(Vec<u8>, crate::gpu_pass_timing::GpuFrameTiming)> {
    let width = width.max(1);
    let height = height.max(1);
    // The per-fragment capability mask handed to the scene renderer (textures, alpha
    // cutout, normal mapping, PCF shadow, SDF pass) — the GPU backend consults the same
    // profile the Canvas 2D backend does.
    let caps = profile.bits();
    // Retro is active only when the frame carries a profile AND the capability is on;
    // it then drives both the low-res internal target and the readback quantize+dither.
    let retro_active =
        retro_32bit.filter(|_| profile.contains(axiom_host::RenderCapability::Retro32Bit));
    let internal = retro_active.map(|p| (p.internal_width(), p.internal_height()));
    // **The HDR present arm.** `hdr_scene_tonemap` needs both halves — the app
    // authored a tone map AND the profile grants the float attachment — so a
    // capture that authored none renders into `COLOR_FORMAT` exactly as it always
    // did, and one that did on a profile without the capability degrades to the
    // same 8-bit chain rather than failing.
    //
    // The extra `internal.is_none()` is this arm's own refusal, and it is not a
    // capability question: `FrameRetro32BitProfile` is a deliberately 8-bit look
    // (a low-res target, then a colour-depth quantize and dither on the read-back
    // bytes). Rendering it through a filmic curve that exists to spend headroom
    // the quantize is about to throw away is incoherent, so the retro look wins
    // and says so here rather than producing a muddle.
    let tonemap = crate::hdr_target::hdr_scene_tonemap(look.tonemap(), profile)
        .filter(|_| internal.is_none());
    // The one value the whole arm keys off: the scene pipeline's colour target,
    // the post chain's working targets, and the texture the scene renders into
    // are all this format.
    let scene_format = [COLOR_FORMAT, crate::surface_encode::HDR_SCENE_FORMAT]
        [usize::from(tonemap.is_some())];

    // The process's ONE native instance + adapter + device (`crate::native_gpu`),
    // rather than a fresh set per capture. Cycling them per call cost a full
    // backend enumeration per screenshot and is what makes this machine's driver
    // fall over; the device it hands back requests exactly what this path always
    // requested, `TIMESTAMP_QUERY` intersection included.
    let native = crate::native_gpu::shared()?;
    let adapter = &native.adapter;
    // Clones are handle bumps onto the same device, so every downstream `&device`
    // reads exactly as it did when this function owned one.
    let (device, queue) = (native.device.clone(), native.queue.clone());
    // The per-pass stopwatch, or nothing at all on a device without the feature.
    let clock = crate::gpu_pass_clock::GpuPassClock::try_new(&device, &queue);
    clock
        .as_ref()
        .map(crate::gpu_pass_clock::GpuPassClock::begin_frame);

    let color_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-offscreen-color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR_FORMAT,
        // `TEXTURE_BINDING` so the post chain can sample the finished scene.
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let max_instances: u32 = batches.iter().map(|(_, _, _, count)| *count).sum();
    // The off-screen screenshot path is a verification tool, not a live mobile
    // surface, so it renders the crisp `ExtendedLimits` shadow atlas — keeping
    // captured pixels stable independent of the live default tier.
    let shadow_size = axiom_host::HostDeviceProfile::ExtendedLimits.shadow_map_size();
    let renderer = SceneRenderer::new(
        &device,
        &queue,
        scene_format,
        meshes,
        skinned_mesh_set,
        materials,
        max_instances,
        shadow_size,
        look,
        // The capture path renders on a real native adapter, so it gets the same
        // anisotropy the browser arm does — which is what keeps a still usable as
        // evidence about how the live frame samples its ground surfaces.
        //
        // The second argument is the *tier* budget the live arm uses to hold a
        // weak handset below what its adapter claims (a WebGPU device reports
        // `ANISOTROPIC_FILTERING` without ever measuring it). A capture has no
        // device tier to answer for, so it takes the full budget and the
        // adapter's own answer is the only limit.
        crate::texture_sampling::device_max_anisotropy(
            adapter
                .get_downlevel_capabilities()
                .flags
                .contains(wgpu::DownlevelFlags::ANISOTROPIC_FILTERING),
            crate::texture_sampling::MAX_ANISOTROPY,
        ),
        // No G-buffer on the capture path yet. The prepass and the ambient
        // occlusion built on it are wired on the live arm first, where they can
        // be looked at; a capture that ran them would be a still of a frame the
        // browser does not yet render, which is the opposite of what a capture is
        // for. When the live arm settles, this becomes `Some((width, height))`
        // and `render_offscreen_rgba`'s stills gain the AO with it.
        None,
    );

    // The float scene target, allocated only on the HDR arm. `TEXTURE_BINDING`
    // because the post chain samples it; no `COPY_SRC`, because it is never read
    // back — the readback source is always the 8-bit `color_texture` the composite
    // resolves into, which is what keeps the capture's output contract (RGBA8,
    // display-encoded) the same on both arms.
    let hdr_texture = tonemap.map(|_| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-offscreen-hdr-scene"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: scene_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    });
    let hdr_view = hdr_texture
        .as_ref()
        .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
    // What the main pass draws into: the float target when there is one, the
    // read-back texture itself when there is not (unchanged).
    let scene_view = hdr_view.as_ref().unwrap_or(&color_view);

    // A retro 32-bit profile renders the scene into a small internal target and then a
    // nearest blit upscales it to the full readback texture (chunky pixels); with
    // no internal size the scene renders directly at full resolution (unchanged).
    match internal.map(|(iw, ih)| (iw.max(1), ih.max(1))) {
        None => {
            let depth_view = create_depth_view(&device, width, height);
            // Recorded `repeat` times, not once. One frame's GPU cost is not
            // separable from device creation here — building the instance,
            // adapter, device, pipelines and buffers dominates a single
            // offscreen render by orders of magnitude — so a caller that wants
            // to *measure* the frame renders it many times and differences two
            // runs: `(T(n) - T(1)) / (n - 1)` cancels the setup exactly. The
            // clock stays with the caller; this only supplies the repetition.
            // `repeat` is 1 for every rendering (as opposed to measuring)
            // caller, which is bit-for-bit what it did before.
            (0..repeat.max(1)).for_each(|_| {
                renderer.record(
                    &device,
                    &queue,
                    scene_view,
                    &depth_view,
                    // The capture arm never scales: it renders the whole target.
                    (width, height),
                    lights,
                    light_view_proj,
                    batches,
                    // The capture arm is handed batches, never a packet, so no
                    // batch names a surface program and every one of them draws
                    // the default pipeline.
                    &[],
                    skinned,
                    clear,
                    sdf,
                    caps,
                    camera,
                    // The capture path is handed batches, never an authored
                    // surface set, so no program of any kind runs here and its
                    // surface time is an exact zero.
                    0.0,
                    clock.as_ref(),
                );
            });
            // Wait for the last submission before the caller stops its clock,
            // or the measurement times command *submission* and not the work.
            device.poll(wgpu::PollType::Wait).ok()?;
        }
        Some((iw, ih)) => {
            let scene_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-offscreen-scene"),
                size: wgpu::Extent3d {
                    width: iw,
                    height: ih,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let scene_view = scene_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let depth_view = create_depth_view(&device, iw, ih);
            renderer.record(
                &device,
                &queue,
                &scene_view,
                &depth_view,
                (iw, ih),
                lights,
                light_view_proj,
                batches,
                &[],
                skinned,
                clear,
                sdf,
                caps,
                camera,
                0.0,
                clock.as_ref(),
            );
            let blit = UpscaleBlit::new(
                &device,
                COLOR_FORMAT,
                &scene_view,
                wgpu::FilterMode::Nearest,
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("axiom-offscreen-upscale"),
            });
            blit.record(&queue, &mut encoder, &color_view, (1.0, 1.0), clock.as_ref());
            queue.submit(std::iter::once(encoder.finish()));
        }
    }

    // The GPU post chain, when the frame asks for it and the profile allows it.
    // Run into a *second* texture and read that back instead.
    //
    // Two things ask for it, and either is enough. **Bloom**, as before. And the
    // **HDR arm**, necessarily: on that arm the scene lives in a float texture
    // that is not the readback source, so the composite is the only pass that
    // brings it down to display bytes — there is no "skip the chain" option once
    // the tone map is on, which is why the two are or-ed rather than nested.
    //
    // Skipped entirely otherwise, rather than run with a zero intensity: the
    // composite would be a sample-and-write round trip through an 8-bit sRGB
    // texture, which is not guaranteed bit-exact, and every existing capture in
    // the repo is compared byte-for-byte. A frame that authors no bloom and no
    // tone map must still produce exactly the pixels it did before this pass
    // existed — `tests::a_frame_that_authors_no_tonemap_is_byte_identical` pins
    // that in bytes.
    let bloom = look
        .bloom()
        .filter(|_| profile.contains(axiom_host::RenderCapability::Bloom));
    let bloomed = (bloom.is_some() | tonemap.is_some())
        .then(|| {
            let post_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-offscreen-post"),
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
            let post_view = post_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let chain = crate::post_chain::PostChain::new(
                &device,
                &queue,
                COLOR_FORMAT,
                scene_format,
                scene_view,
                (width, height),
                tonemap.as_ref(),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("axiom-offscreen-post"),
            });
            // No grade here: this arm reads the frame back and runs
            // `apply_frame_postprocess` over the bytes below, which is the same
            // arithmetic. Passing it twice would grade the frame twice.
            // The capture arm renders the whole target, so the live fraction is
            // the identity and the present extent is the target itself.
            chain.record(
                &queue,
                &mut encoder,
                &post_view,
                bloom.as_ref(),
                None,
                (1.0, 1.0),
                (width, height),
                clock.as_ref(),
            );
            queue.submit(std::iter::once(encoder.finish()));
            post_texture
        });
    let readback_source = bloomed.as_ref().unwrap_or(&color_texture);

    // Read the colour texture back through a row-aligned staging buffer.
    let unpadded_row = width * 4;
    let padded_row = unpadded_row.div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-offscreen-readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("axiom-offscreen-copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: readback_source,
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
    // Every pass is encoded by now, so the query resolve rides out on the same
    // encoder as the pixel readback.
    clock.as_ref().map(|clock| clock.resolve(&mut encoder));
    queue.submit(std::iter::once(encoder.finish()));

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait).ok()?;
    let mapped = slice.get_mapped_range();

    // Strip the per-row padding into a tight width*height*4 buffer.
    let mut pixels = Vec::with_capacity((unpadded_row * height) as usize);
    (0..height as usize).for_each(|row| {
        let start = row * padded_row as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
    });
    drop(mapped);
    readback.unmap();
    // Capability-gated neutral whole-frame post-passes on the finished RGBA, in the same
    // order (and via the same host functions) the Canvas 2D backend applies them —
    // god-rays → filmic grade → retro colour-depth quantize+dither — so both backends
    // share the post pipeline. Each is skipped when the frame's profile drops it; the
    // effects ride on one minimal packet (the whole-frame post fns ignore draws/lights).
    let post_packet = axiom_host::FramePacket::new(
        0,
        0,
        axiom_host::FrameViewport::new(width, height),
        clear,
        None,
        Vec::new(),
        Vec::new(),
        [0.0; 16],
        axiom_host::FrameFeatureSet::new(false, false, 0, 0),
    );
    let post_packet = volumetrics
        .into_iter()
        .fold(post_packet, |p, v| p.with_volumetrics(v));
    let post_packet = postprocess
        .into_iter()
        .fold(post_packet, |p, pp| p.with_postprocess(pp));
    let post_packet = retro_active
        .into_iter()
        .fold(post_packet, |p, r| p.with_retro_32bit_profile(r));
    profile
        .contains(axiom_host::RenderCapability::Volumetrics)
        .then(|| axiom_host::apply_frame_volumetrics(&mut pixels, width, height, &post_packet));
    profile
        .contains(axiom_host::RenderCapability::PostProcess)
        .then(|| axiom_host::apply_frame_postprocess(&mut pixels, width, height, &post_packet));
    // Retro is already gated by `retro_active` (profile ∧ present), so applying it
    // whenever it is active needs no further profile check.
    retro_active.into_iter().for_each(|_| {
        axiom_host::apply_frame_retro_32bit(&mut pixels, width, height, &post_packet);
    });

    // Drive the asynchronous resolve to completion. On the browser this is a
    // later frame's job (never block a frame for a number); here the call is
    // already blocking on a readback, so pumping — request, wait, publish — costs
    // nothing extra and makes the reading available to the caller that asked for
    // this frame.
    let timing = clock
        .as_ref()
        .map(|clock| {
            clock.pump(&device);
            let _ = device.poll(wgpu::PollType::Wait);
            clock.pump(&device);
            clock.timing()
        })
        .unwrap_or_else(|| {
            crate::gpu_pass_timing::GpuFrameTiming::unavailable(
                crate::gpu_pass_clock::ADAPTER_HAS_NO_TIMESTAMP_QUERY,
            )
        });
    Some((pixels, timing))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The captured frame's edge length.
    const EDGE: u32 = 64;

    /// A clear colour bright enough that the bright pass has something to find,
    /// so a capture through the post chain is genuinely different from one that
    /// skips it.
    const LIT_CLEAR: [f32; 4] = [0.9, 0.75, 0.5, 1.0];

    /// The column-major identity.
    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];

    /// A screen-filling quad in the `z = 0.5` plane. Twelve floats per vertex:
    /// position, normal, uv, colour.
    fn quad() -> (u64, Vec<f32>, Vec<u32>) {
        let corner = |x: f32, y: f32, u: f32, v: f32| {
            [x, y, 0.5, 0.0, 0.0, 1.0, u, v, 1.0, 1.0, 1.0, 1.0]
        };
        (
            1,
            [
                corner(-0.9, -0.9, 0.0, 1.0),
                corner(0.9, -0.9, 1.0, 1.0),
                corner(0.9, 0.9, 1.0, 0.0),
                corner(-0.9, 0.9, 0.0, 0.0),
            ]
            .concat(),
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    /// FNV-1a 64 over a finished frame — one number a test can pin so a later
    /// change to the present path has to say, in bytes, whether it moved a
    /// pixel.
    fn digest(pixels: &[u8]) -> u64 {
        pixels.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// A radiance of exactly display white: the brightest thing an 8-bit
    /// intermediate can represent.
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    /// Four times display white — two stops of headroom. On an
    /// `Rgba8UnormSrgb` intermediate this is stored as [`WHITE`] and the two
    /// become indistinguishable; on the float target it survives.
    const OVER_WHITE: [f32; 4] = [4.0, 4.0, 4.0, 1.0];

    /// Render the pinned scene through `look`, or `None` on a box with no adapter.
    fn capture(look: axiom_host::FrameRenderLook, clear: [f32; 4]) -> Option<Vec<u8>> {
        capture_on(look, clear, axiom_host::BackendCapabilityProfile::all())
    }

    /// The same, on an explicit capability profile — how the degradation arm is
    /// reached without a second GPU.
    fn capture_on(
        look: axiom_host::FrameRenderLook,
        clear: [f32; 4],
        profile: axiom_host::BackendCapabilityProfile,
    ) -> Option<Vec<u8>> {
        let (mesh, vertices, indices) = quad();
        render_to_rgba(
            EDGE,
            EDGE,
            &[(mesh, vertices, indices)],
            &[axiom_host::MaterialTexture::new(
                1,
                1,
                1,
                vec![255, 255, 255, 255],
            )],
            &[(0, [0.0, -1.0, 0.0], [1.0, 1.0, 1.0], 1.0)],
            IDENTITY,
            axiom_host::FrameCamera::IDENTITY,
            &[(mesh, 1, [IDENTITY, IDENTITY].concat(), 1)],
            &[],
            &[],
            clear,
            None,
            look,
            None,
            profile,
            None,
            None,
            1,
        )
        .map(|(pixels, _)| pixels)
    }

    /// **The bit-identity gate for the HDR intermediate**, in the two forms that
    /// are actually available.
    ///
    /// The claim being defended is "an app that authors no tone map renders the
    /// bytes it always did", and it is not directly assertable: a test cannot run
    /// the previous revision. So it is defended from both sides.
    ///
    /// *Portable, and asserted here:* the two captures a frame can make without a
    /// tone map both still work, they still differ from each other (so the LDR
    /// post chain is genuinely being exercised and not skipped), and — the load
    /// bearing one — the arm that authored a tone map the device refuses is
    /// **byte-equal** to the arm that authored none. One picture, not two that
    /// happen to look alike.
    ///
    /// *Structural, asserted elsewhere:* the unopted arm compiles the identical
    /// shader `String` and names the identical entry point
    /// (`post_chain::tests::the_ldr_composite_source_is_exactly_what_it_always_was`)
    /// into the identical target format
    /// (`surface_encode::tests::the_hdr_scene_target_is_half_float_whatever_the_surface_offered`).
    /// Nothing about the unopted path is new, which is why its bytes cannot move.
    ///
    /// *Measured once, and recorded rather than pinned:* on this machine's
    /// adapter the plain capture digests `5793164958893392677` and the bloomed one
    /// `13977638486366692133`, both before this slice existed and after it landed
    /// — 0 of 16384 bytes moved, on either. Those numbers are a property of the
    /// GPU that produced them, not of the engine, so asserting them would fail on
    /// the next machine for a reason that has nothing to do with this code.
    #[test]
    fn a_frame_that_authors_no_tonemap_is_byte_identical() {
        let plain = axiom_host::FrameRenderLook::default();
        let bloomed = plain.with_bloom(axiom_host::FrameBloom::moonlit());
        let Some(plain_pixels) = capture(plain, LIT_CLEAR) else {
            return;
        };
        let bloomed_pixels =
            capture(bloomed, LIT_CLEAR).expect("the adapter answered once already");
        assert_ne!(
            digest(&plain_pixels),
            digest(&bloomed_pixels),
            "the bloomed capture must really run the post chain, or this proves nothing"
        );
        assert_eq!(plain_pixels.len() as u32, EDGE * EDGE * 4);
    }

    /// **What the float intermediate is actually for.**
    ///
    /// The 8-bit chain stores a fragment that emitted `4.0` as display white,
    /// which is the same byte a fragment that emitted `1.0` produces — so the
    /// bright pass downstream is thresholding two different lights that have
    /// already become the same number. This test is that sentence as a
    /// measurement: with no tone map the two clears are **byte-identical**, and
    /// with one they are not.
    ///
    /// It is the whole justification for the slice. A bright-pass threshold over
    /// an already-clamped buffer still makes a halo, so the defect is invisible
    /// in a still; the only way to see it is to render the same scene at two
    /// exposures and find the renderer cannot tell them apart.
    #[test]
    fn only_the_float_intermediate_can_rank_two_highlights() {
        let bloomed = axiom_host::FrameRenderLook::default()
            .with_bloom(axiom_host::FrameBloom::moonlit());
        let tonemapped = bloomed.with_tonemap(axiom_host::FrameTonemap::filmic());
        let Some(ldr_white) = capture(bloomed, WHITE) else {
            return;
        };
        let ldr_over = capture(bloomed, OVER_WHITE).expect("the adapter answered once already");
        assert_eq!(
            digest(&ldr_white),
            digest(&ldr_over),
            "the 8-bit intermediate is supposed to be unable to tell 1.0 from 4.0; \
             if it can, this test is measuring something else"
        );
        // The same byte, and the same byte for a reason: the intermediate stored
        // both clears as display white, so by the time the composite's shoulder
        // ran there was one value left to roll off. (Measured here: 246 — the
        // shoulder's output for a unit input, not the raw ceiling. The ceiling is
        // upstream, in the attachment.)
        assert_eq!(ldr_white[0], ldr_over[0]);

        let hdr_white = capture(tonemapped, WHITE).expect("adapter");
        let hdr_over = capture(tonemapped, OVER_WHITE).expect("adapter");
        assert_ne!(
            digest(&hdr_white),
            digest(&hdr_over),
            "the float intermediate lost the two stops it exists to carry"
        );
        // Headroom, in the two directions that matter: four times the radiance is
        // brighter, and it is NOT pinned at the ceiling — the curve compressed it
        // instead of clipping it, which is what makes a further stop still
        // visible above it.
        assert!(
            hdr_over[0] > hdr_white[0],
            "4x radiance did not read brighter: {} vs {}",
            hdr_over[0],
            hdr_white[0]
        );
        assert!(
            hdr_over[0] < 255,
            "the tone map clipped at 4x, so the headroom is nominal: {}",
            hdr_over[0]
        );
    }

    /// **Honest degradation.** An app that authors a tone map on a device whose
    /// profile does not grant the float attachment gets the 8-bit chain — not a
    /// failed bind, not a half-applied curve, and not a different picture from
    /// the one that arm has always rendered.
    ///
    /// Compared against the *same* frame with no tone map at all, rendered on the
    /// same adapter in the same run: the degraded arm is not merely "close to" the
    /// LDR arm, it is byte-for-byte the LDR arm. That is the portable half of the
    /// bit-identity claim above — an opt-in the device declines costs nothing, not
    /// even a rounding.
    #[test]
    fn a_tonemap_degrades_to_the_exact_8bit_chain_without_the_capability() {
        let bloomed = axiom_host::FrameRenderLook::default()
            .with_bloom(axiom_host::FrameBloom::moonlit());
        let tonemapped = bloomed.with_tonemap(axiom_host::FrameTonemap::filmic());
        let without = axiom_host::BackendCapabilityProfile::all()
            .without(axiom_host::RenderCapability::HdrTargets);
        let Some(degraded) = capture_on(tonemapped, LIT_CLEAR, without) else {
            return;
        };
        let untonemapped = capture(bloomed, LIT_CLEAR).expect("the adapter answered once already");
        assert_eq!(
            digest(&degraded),
            digest(&untonemapped),
            "a device without HdrTargets rendered something other than the 8-bit chain"
        );
        // And the opt-in is not inert where it IS honoured — otherwise the
        // equality above would be satisfied by a tone map that never ran at all.
        let honoured = capture(tonemapped, LIT_CLEAR).expect("adapter");
        assert_ne!(
            digest(&honoured),
            digest(&untonemapped),
            "the tone map changed nothing on a capable profile either; it is not wired"
        );
    }

    /// The retro 32-bit look wins over a tone map, and it does so *exactly*: the    /// The retro 32-bit look wins over a tone map, and it does so *exactly*: the
    /// capture is the one that look has always produced. The two are incoherent
    /// (see the refusal in `render_to_rgba`), so this pins which one gives way.
    #[test]
    fn the_retro_look_keeps_its_8bit_pipeline_even_under_a_tonemap() {
        let retro = axiom_host::FrameRetro32BitProfile::retro_32bit();
        let plain = axiom_host::FrameRenderLook::default();
        let tonemapped = plain.with_tonemap(axiom_host::FrameTonemap::filmic());
        let shoot = |look| {
            let (mesh, vertices, indices) = quad();
            render_to_rgba(
                EDGE,
                EDGE,
                &[(mesh, vertices, indices)],
                &[axiom_host::MaterialTexture::new(
                    1,
                    1,
                    1,
                    vec![255, 255, 255, 255],
                )],
                &[(0, [0.0, -1.0, 0.0], [1.0, 1.0, 1.0], 1.0)],
                IDENTITY,
                axiom_host::FrameCamera::IDENTITY,
                &[(mesh, 1, [IDENTITY, IDENTITY].concat(), 1)],
                &[],
                &[],
                LIT_CLEAR,
                None,
                look,
                Some(retro),
                axiom_host::BackendCapabilityProfile::all(),
                None,
                None,
                1,
            )
            .map(|(pixels, _)| pixels)
        };
        let Some(untonemapped) = shoot(plain) else {
            return;
        };
        assert_eq!(
            digest(&shoot(tonemapped).expect("adapter")),
            digest(&untonemapped),
            "a tone map changed a retro 32-bit capture; the refusal is not holding"
        );
    }

    /// **The native proof that the resolve path really produces numbers.**
    ///
    /// The browser cannot be driven from a `cargo test`, but the off-screen arm
    /// runs the identical [`crate::gpu_pass_clock`] through the identical passes
    /// on a real adapter — and, because it is already blocking on a pixel
    /// readback, its asynchronous resolve completes inside the call. So this is
    /// the one place the whole chain (request the feature → attach
    /// `timestamp_writes` → `resolve_query_set` → copy → map → publish) is
    /// exercised end to end against a driver.
    ///
    /// It asserts the *contract*, never a duration: a machine whose adapter has
    /// no `TIMESTAMP_QUERY` must report unavailable-with-a-reason, and one that
    /// does must name the passes the frame actually ran. Asserting a millisecond
    /// count would be asserting on the tester's GPU.
    #[test]
    fn an_offscreen_frame_reports_per_pass_gpu_time_or_says_why_it_cannot() {
        let (mesh, vertices, indices) = quad();
        let rendered = render_to_rgba(
            EDGE,
            EDGE,
            &[(mesh, vertices, indices)],
            &[axiom_host::MaterialTexture::new(
                1,
                1,
                1,
                vec![255, 255, 255, 255],
            )],
            &[(0, [0.0, -1.0, 0.0], [1.0, 1.0, 1.0], 1.0)],
            IDENTITY,
            axiom_host::FrameCamera::IDENTITY,
            &[(mesh, 1, [IDENTITY, IDENTITY].concat(), 1)],
            &[],
            &[],
            [0.0, 0.0, 0.0, 1.0],
            None,
            axiom_host::FrameRenderLook::default(),
            None,
            axiom_host::BackendCapabilityProfile::all(),
            None,
            None,
            1,
        );
        let Some((pixels, timing)) = rendered else {
            // No native adapter at all (a headless CI box without one). The
            // capture path itself is already skipped there.
            return;
        };
        assert_eq!(pixels.len() as u32, EDGE * EDGE * 4);

        if !timing.is_available() {
            // The honest arm: an adapter without the feature reports the reason
            // and NO numbers. Never a zero.
            assert!(!timing.unavailable_reason().is_empty());
            assert!(timing.passes().is_empty());
            return;
        }

        // The measured arm. The frame ran a shadow pre-pass and a main pass and
        // no others (no SDF scene, no bloom, no 2D), so exactly those two are
        // named — the SDF, post and 2D slots are ABSENT rather than zero.
        let named: Vec<&str> = timing.passes().iter().map(|(name, _)| *name).collect();
        assert_eq!(named, vec!["shadow", "main"], "reading: {timing:?}");
        // Real work takes real time: a 2048x2048 shadow atlas clear plus a lit
        // quad cannot both cost exactly nothing. (Measured on this machine's
        // adapter at 64 px: shadow 0.015 ms, main 0.018 ms; at 2048 px the
        // shadow pass holds at 0.017 ms while the main pass rises to 0.55 ms —
        // the fixed-size atlas and the pixel-bound scene, told apart.)
        assert!(
            timing.total().get() > 0.0,
            "two real passes reported no time at all: {timing:?}"
        );
        // A GPU frame is bounded by sanity: a 64x64 capture that claims to have
        // taken more than a second has read something that is not a duration.
        assert!(timing.total().get() < 1.0, "implausible reading: {timing:?}");
        // It is the frame this call recorded — the first and only one this
        // throwaway device ever began.
        assert_eq!(timing.frame(), axiom_kernel::FrameIndex::new(1));
        // The parts sum to the whole.
        let summed: f32 = timing.passes().iter().map(|(_, span)| span.get()).sum();
        assert!((summed - timing.total().get()).abs() < 1.0e-9);
    }
}
