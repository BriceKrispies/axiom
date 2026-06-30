//! Native **off-screen** 2D raster — `offscreen` feature, non-wasm only.
//!
//! The 2D peer of [`crate::offscreen`]: it builds a throwaway wgpu device, renders
//! a [`Draw2dGeometry`] through the shared [`crate::draw2d_renderer::Draw2dRenderer`]
//! into an off-screen colour texture, and reads the pixels back to RGBA8. It is
//! the headless capture path the screenshot tool (`axiom-shot`) and the SPEC-04
//! alpha-blend parity proof drive.
//!
//! ## Why a **linear** (non-sRGB) target
//! The software Canvas 2D backend writes linear `0.0..=1.0` colours straight to
//! bytes (`linear → round → u8`) with **no gamma encoding**, and composites in
//! that linear space. To match it byte-for-byte, this path renders into
//! `Rgba8Unorm` (linear), not the `Rgba8UnormSrgb` the 3D off-screen path uses:
//! the GPU then quantizes `linear → byte` exactly as the software path does and
//! blends in the same space, so the only residual difference is ±1 rounding. (The
//! 3D path wants sRGB output for display; the 2D parity proof wants byte parity
//! with the software rasterizer, so the two deliberately differ here.)
//!
//! Compiled only behind the `offscreen` feature, so the engine's default native
//! build, coverage gate, and branchless lint never see this wgpu code.

use crate::draw2d_geometry::Draw2dGeometry;
use crate::draw2d_renderer::Draw2dRenderer;

/// The off-screen colour target format: **linear** RGBA8 (see the module note),
/// so the GPU's quantization + blend match the software backend's linear path.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// `copy_texture_to_buffer` requires each row aligned to this many bytes.
const ROW_ALIGN: u32 = 256;

/// Render `geometry` off-screen into a `width`×`height` linear RGBA8 image and
/// read it back (row-major, top-left origin). `textures` are the sprite atlases
/// (`(id, w, h, RGBA8)`) the geometry's sprite quads sample. `None` if no native
/// GPU adapter/device is available.
pub(crate) fn render_draw2d_to_rgba(
    width: u32,
    height: u32,
    geometry: &Draw2dGeometry,
    textures: &[(u64, u32, u32, Vec<u8>)],
) -> Option<Vec<u8>> {
    let width = width.max(1);
    let height = height.max(1);

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("axiom-draw2d-offscreen-device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;

    let color_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("axiom-draw2d-offscreen-color"),
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

    let renderer = Draw2dRenderer::new(&device, &queue, COLOR_FORMAT, width, height, textures);
    // Clear to fully transparent (matching the software backend's fresh
    // transparent-black framebuffer), then composite the quads over it.
    renderer.record(
        &device,
        &queue,
        &color_view,
        [0.0, 0.0, 0.0, 0.0],
        geometry,
    );

    // Read the colour texture back through a row-aligned staging buffer.
    let unpadded_row = width * 4;
    let padded_row = unpadded_row.div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-draw2d-offscreen-readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("axiom-draw2d-offscreen-copy"),
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

    let mut pixels = Vec::with_capacity((unpadded_row * height) as usize);
    (0..height as usize).for_each(|row| {
        let start = row * padded_row as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
    });
    drop(mapped);
    readback.unmap();
    Some(pixels)
}
