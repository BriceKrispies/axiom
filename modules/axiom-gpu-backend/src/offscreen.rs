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
/// bytes (row-major, top-down). `meshes` / `materials` / `lights` / `batches` are
/// exactly the data the live backend consumes (see [`SceneRenderer::record`]).
/// Returns `None` if no native GPU adapter/device is available.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_to_rgba(
    width: u32,
    height: u32,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    normals: &[(u64, u32, u32, Vec<u8>)],
    lights: &[(u32, [f32; 3], [f32; 3], f32)],
    light_view_proj: [f32; 16],
    batches: &[(u64, u64, Vec<f32>, u32)],
    clear: [f32; 4],
    sdf: Option<&axiom_host::SdfScene>,
    ambient: axiom_host::FrameAmbient,
    postprocess: Option<axiom_host::FramePostProcess>,
    retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
) -> Option<Vec<u8>> {
    let width = width.max(1);
    let height = height.max(1);
    // The retro 32-bit profile drives the low-res internal target (its internal resolution)
    // and the colour-depth quantize + ordered dither applied to the readback.
    let internal = retro_32bit.map(|p| (p.internal_width(), p.internal_height()));

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("axiom-offscreen-device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;

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
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
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
        COLOR_FORMAT,
        meshes,
        materials,
        normals,
        max_instances,
        shadow_size,
        ambient,
    );

    // A retro 32-bit profile renders the scene into a small internal target and then a
    // nearest blit upscales it to the full readback texture (chunky pixels); with
    // no internal size the scene renders directly at full resolution (unchanged).
    match internal.map(|(iw, ih)| (iw.max(1), ih.max(1))) {
        None => {
            let depth_view = create_depth_view(&device, width, height);
            renderer.record(
                &device, &queue, &color_view, &depth_view, lights, light_view_proj, batches,
                clear, sdf,
            );
        }
        Some((iw, ih)) => {
            let scene_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-offscreen-scene"),
                size: wgpu::Extent3d { width: iw, height: ih, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let scene_view = scene_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let depth_view = create_depth_view(&device, iw, ih);
            renderer.record(
                &device, &queue, &scene_view, &depth_view, lights, light_view_proj, batches,
                clear, sdf,
            );
            let blit = UpscaleBlit::new(&device, COLOR_FORMAT, &scene_view, wgpu::FilterMode::Nearest);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("axiom-offscreen-upscale"),
            });
            blit.record(&mut encoder, &color_view);
            queue.submit(std::iter::once(encoder.finish()));
        }
    }

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
            texture: &color_texture,
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

    // Strip the per-row padding into a tight width*height*4 buffer.
    let mut pixels = Vec::with_capacity((unpadded_row * height) as usize);
    (0..height as usize).for_each(|row| {
        let start = row * padded_row as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
    });
    drop(mapped);
    readback.unmap();
    // Cinematic grade (exposure / contrast / saturation) on the finished RGBA, applied
    // BEFORE the retro colour-depth quantize below — the same whole-frame order the host
    // composits (grade, then quantize), so the scored GPU champion carries the grade the
    // live/canvas2d arms carry. A no-op when the caller passes no profile; mirrors the
    // retro `Option` handling (no branch — the feature-gated offscreen arm is out of the
    // branchless gate, but stays combinator-shaped anyway).
    postprocess.into_iter().for_each(|pp| {
        let packet = axiom_host::FramePacket::new(
            0,
            0,
            axiom_host::FrameViewport::new(width, height),
            clear,
            None,
            Vec::new(),
            Vec::new(),
            [0.0; 16],
            axiom_host::FrameFeatureSet::new(false, false, 0, 0),
        )
        .with_postprocess(pp);
        axiom_host::apply_frame_postprocess(&mut pixels, width, height, &packet);
    });
    // retro 32-bit colour-depth quantize + ordered dither on the finished RGBA — the shared
    // host post (canvas2d + the live WGSL mirror the same numbers). Built from a
    // minimal packet carrying just the profile.
    retro_32bit.into_iter().for_each(|p| {
        let packet = axiom_host::FramePacket::new(
            0,
            0,
            axiom_host::FrameViewport::new(width, height),
            clear,
            None,
            Vec::new(),
            Vec::new(),
            [0.0; 16],
            axiom_host::FrameFeatureSet::new(false, false, 0, 0),
        )
        .with_retro_32bit_profile(p);
        axiom_host::apply_frame_retro_32bit(&mut pixels, width, height, &packet);
    });
    Some(pixels)
}
