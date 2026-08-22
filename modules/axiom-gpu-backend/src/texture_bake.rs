//! Procedural texture bake — a fragment program evaluated into render targets.
//!
//! Ported from Claude-of-Duty `src/materials/generator.js:1-393`: the
//! `TextureForge`, **minus** the GLSL it bakes. The forge and the generator
//! bodies are two different things in the source and they are two different
//! things here:
//!
//! * the *forge* — render targets, their formats, the four full-screen draws,
//!   the Sobel, the read-back — is engine machinery, and lives here;
//! * the *generator bodies* (`glsl/surfaces-*.js`, `DETAIL_SRC`, `MACRO_SRC`)
//!   are game content, and live in the app that owns them
//!   (`apps/shmup/src/materials/wgsl/`).
//!
//! That is the same seam the source draws, and it is the reason this module
//! takes the surface program as a `&str` rather than knowing any surface.
//!
//! ## Why this exists at all: a CPU bake cannot be a runtime path
//!
//! `apps/shmup/src/materials/bake.rs` is the faithful CPU bake, and it is
//! correct — it is also, measured natively in `--release`, **16.6 s** for one
//! 512² surface and **~930 s** for the nineteen at their authored sizes,
//! against **1.3 s** for the same work on the GPU in the source. The cause is
//! structural: a single `owSurface` evaluation makes hundreds of hash and
//! trigonometric calls, and the answer to that is 1024²-way parallelism, not a
//! faster scalar loop. See `docs/work-manifests/shmup-port/notes/materials-upload.md`.
//!
//! ## The four passes (`generator.js:280-308`), reproduced exactly
//!
//! ```text
//! uOutput = 0 -> the scratch HEIGHT target   (half-float; feeds the Sobel only)
//! uOutput = 1 -> ALBEDO   rgb = colour, a = height   (sRGB target unless linear)
//! uOutput = 2 -> ORM      r = AO, g = roughness, b = metalness, a = 1
//! Sobel(height) -> NORMAL tangent-space, OpenGL +Y, `* 0.5 + 0.5`
//! ```
//!
//! The height pass "exists only to feed the Sobel, so it is skipped with it"
//! (`generator.js:280`), and the ORM and normal passes are individually
//! switchable because `buildDetail`/`buildMacro` switch them off
//! (`generator.js:344-381`).
//!
//! ## Storage width is part of the algorithm
//!
//! Three things here are format decisions the source makes deliberately, and
//! changing any of them changes the answer:
//!
//! 1. **The height scratch is half-float** (`generator.js:180-186`), because an
//!    8-bit height field stair-steps the Sobel. `height_format` mirrors the
//!    source's own `canHalf ? HalfFloatType : UnsignedByteType` capability
//!    fallback. Note that the CPU bake keeps the height in **`f32`** and says
//!    so (`bake.rs`'s "Height precision" section) — so the CPU reference and a
//!    faithful GPU bake differ here by construction. The effect is small and
//!    computable: an `f16` round-off of ±1.2e-4 at h≈0.5, carried through a
//!    Sobel whose weights sum to 8 and are scaled by `0.125`, then by
//!    `size * relief / worldSize` (≈10 at 1024² and `0.02 / 2`), lands at
//!    ≈5e-4 on a unit normal, i.e. **≈0.07 of one 8-bit LSB**.
//! 2. **The albedo target is sRGB unless `linear_albedo`**
//!    (`generator.js:276`). The hardware performs the encode on write, so a
//!    baked tile uploaded as `Rgba8UnormSrgb` decodes back to the linear colour
//!    the surface function computed. This is the write half of gap **G16** —
//!    "baked field textures are written linear and bound as `Rgba8UnormSrgb`,
//!    so a baked tile reads darker" (`01-engine-gaps.md`). A bake that goes
//!    through `albedo_texture_format` cannot land in G16, because the encode and the
//!    binding are chosen by the same flag. The two shared maps set
//!    `linear_albedo` for exactly the reason the source gives: "the detail map
//!    is DATA, not colour" (`generator.js:355-358`).
//! 3. **The ORM and normal targets are linear 8-bit** (`NoColorSpace`,
//!    `generator.js:201-213`).
//!
//! ## The v axis
//!
//! A WebGL render target's row 0 is its **bottom** row, so the source's
//! `vUv = uv` varying over a `PlaneGeometry(2, 2)` makes texel row 0 the
//! `v ≈ 0` row. A WebGPU render target's row 0 is its **top** row. Deriving the
//! UV from `@builtin(position)` — `position.xy * inv_size`, i.e. exactly
//! `((x + 0.5) / size, (y + 0.5) / size)` — reproduces the source's mapping
//! **and** matches `bake.rs::texel_uv`, whose row 0 is likewise `v ≈ 0`. Using
//! a `-y` clip-space varying instead would flip every normal's green channel
//! and mirror every anisotropic surface. This is the same axis hazard
//! `VELOCITY_TEXTURE_V_SIGN` names in `gbuffer.rs`.

// The bake's data contract lives in the **host layer**, not here.
// `ProceduralBakeRequest` and `ProceduralBakeMaps` are backend-neutral by
// necessity: a request travels from an app, through the engine facade, to
// whichever backend holds a device, and the Module Law lets this module publish
// exactly one facade — so a request shaped here would be unnameable by everyone
// who has to send one. `axiom_host` is the one place `axiom`, `axiom-windowing`
// and both render backends can all name a type, which is the same argument
// `MaterialTexture`'s own module doc makes. This module is the *execution* of
// that contract, and nothing else.
use axiom_host::{BakeOutput, ProceduralBakeMaps, ProceduralBakeRequest};

// ---------------------------------------------------------------------------
// WGSL. Every loop and branch below lives inside a `&str`: it is shader text,
// not Rust, and the Branchless Law reads Rust HIR. A raymarch stays a
// raymarch; a Sobel stays a Sobel.
// ---------------------------------------------------------------------------

/// `HEADER` (`generator.js:32-40`) — the uniform block every generator reads.
///
/// The source's five loose uniforms become one struct because WGSL has no
/// loose uniforms. The generator bodies therefore spell `uSeed` as `U.seed`,
/// `uTintA` as `U.tint_a`, `uTintB` as `U.tint_b` and `uParam` as `U.param`;
/// that renaming is the only change the transcription makes to a uniform read.
///
/// `inv_size` is not a source uniform. It replaces the `vUv` varying: see the
/// module doc's "v axis" section for why the UV is derived from
/// `@builtin(position)` rather than interpolated.
///
/// Layout (std140-compatible, 64 bytes): `tint_a` 0..12, `seed` 12..16,
/// `tint_b` 16..28, `output_mode` 28..32, `param` 32..48, `inv_size` 48..52,
/// padding 52..64. [`surface_uniform_bytes`] writes exactly this.
pub(crate) const BAKE_HEADER_WGSL: &str = r#"
struct OwBakeUniforms {
  tint_a : vec3<f32>,
  seed : f32,
  tint_b : vec3<f32>,
  output_mode : i32,
  param : vec4<f32>,
  inv_size : f32,
  pad0 : f32,
  pad1 : vec2<f32>,
}

@group(0) @binding(0) var<uniform> U : OwBakeUniforms;

@vertex
fn ow_bake_vs(@builtin(vertex_index) index : u32) -> @builtin(position) vec4<f32> {
  var corners = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -3.0),
    vec2<f32>(-1.0, 1.0),
    vec2<f32>(3.0, 1.0),
  );
  return vec4<f32>(corners[index], 0.0, 1.0);
}
"#;

/// `FOOTER` (`generator.js:42-51`).
///
/// The GLSL `out` parameters become `ptr<function, T>`, which is why every
/// transcribed body opens with `var alb = *albOut;` and closes with
/// `*albOut = alb;` — that pair *is* GLSL out-parameter semantics, and it lets
/// the body itself stay line-for-line with the source.
///
/// The five defaults `vec3 alb = vec3(0.5); float h = 0.5, rough = 0.5,
/// metal = 0.0, ao = 1.0;` are the source's, reproduced even though every
/// ported body assigns all five (`bake.rs`'s `FOOTER_DEFAULT` makes the same
/// observation): dead initialisation in the source is still part of the source.
pub(crate) const BAKE_FOOTER_WGSL: &str = r#"
@fragment
fn ow_bake_fs(@builtin(position) position : vec4<f32>) -> @location(0) vec4<f32> {
  let vUv = position.xy * U.inv_size;
  var alb = vec3<f32>(0.5);
  var h : f32 = 0.5;
  var rough : f32 = 0.5;
  var metal : f32 = 0.0;
  var ao : f32 = 1.0;
  owSurface(vUv, &alb, &h, &rough, &metal, &ao);
  if (U.output_mode == 0) {
    return vec4<f32>(h, h, h, 1.0);
  } else if (U.output_mode == 1) {
    return vec4<f32>(alb, h);
  }
  return vec4<f32>(ao, rough, metal, 1.0);
}
"#;

/// `SOBEL` (`generator.js:53-78`), transcribed from the GLSL text.
///
/// Two details are load-bearing:
///
/// * `sx = dx / uTexel.x` stays a **division**. The CPU port writes it as
///   `dx * size`, which is the same number only because `uTexel.x` is `1/size`
///   for a power-of-two `size` and therefore exact; the source's grouping is
///   the specification, so the division is what is written here.
/// * `H()` samples with `RepeatWrapping` (`generator.js:190-191`) so the tile's
///   edge texels read their wrapped neighbours. The sampler is `Repeat` +
///   `Linear`, and every offset is an exact texel multiple, so the filter
///   returns the texel centre unmodified — the same value
///   `bake.rs::Texture::wrapped_r` returns.
pub(crate) const BAKE_SOBEL_WGSL: &str = r#"
struct OwSobelUniforms {
  texel : vec2<f32>,
  strength : f32,
  pad0 : f32,
}

@group(0) @binding(0) var<uniform> S : OwSobelUniforms;
@group(0) @binding(1) var ow_height_tex : texture_2d<f32>;
@group(0) @binding(2) var ow_height_smp : sampler;

@vertex
fn ow_sobel_vs(@builtin(vertex_index) index : u32) -> @builtin(position) vec4<f32> {
  var corners = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -3.0),
    vec2<f32>(-1.0, 1.0),
    vec2<f32>(3.0, 1.0),
  );
  return vec4<f32>(corners[index], 0.0, 1.0);
}

fn owH(vUv : vec2<f32>, o : vec2<f32>) -> f32 {
  return textureSample(ow_height_tex, ow_height_smp, vUv + o * S.texel).r;
}

@fragment
fn ow_sobel_fs(@builtin(position) position : vec4<f32>) -> @location(0) vec4<f32> {
  let vUv = position.xy * S.texel;
  let tl = owH(vUv, vec2<f32>(-1.0,  1.0));
  let t  = owH(vUv, vec2<f32>( 0.0,  1.0));
  let tr = owH(vUv, vec2<f32>( 1.0,  1.0));
  let l  = owH(vUv, vec2<f32>(-1.0,  0.0));
  let r  = owH(vUv, vec2<f32>( 1.0,  0.0));
  let bl = owH(vUv, vec2<f32>(-1.0, -1.0));
  let b  = owH(vUv, vec2<f32>( 0.0, -1.0));
  let br = owH(vUv, vec2<f32>( 1.0, -1.0));

  // Sobel over the height field; the 1/8 normalises the kernel weight.
  let dx = ((tr + 2.0 * r + br) - (tl + 2.0 * l + bl)) * 0.125;
  let dy = ((tl + 2.0 * t + tr) - (bl + 2.0 * b + br)) * 0.125;

  // dx/dy are per-texel; convert to a slope over the whole tile.
  let sx = dx / S.texel.x;
  let sy = dy / S.texel.y;

  let n = normalize(vec3<f32>(-sx * S.strength, -sy * S.strength, 1.0));
  return vec4<f32>(n * 0.5 + vec3<f32>(0.5), 1.0);
}
"#;

// ---------------------------------------------------------------------------
// The decisions. Everything in this section is plain arithmetic on plain data:
// it compiles on every arm, without `wgpu`, so the coverage gate sees all of
// it. The wgpu execution below is the same shape as `offscreen.rs` and is
// covered by the `--features offscreen` parity run, exactly as that module is.
// ---------------------------------------------------------------------------

/// `HEADER + NOISE_GLSL + RUST_HELPERS + glsl + FOOTER` (`generator.js:224`),
/// in that order, by concatenation.
///
/// The caller supplies `library` (its noise library and any shared helpers) and
/// `surface` (one `owSurface`); this module supplies the header, the shared
/// vertex stage and the footer. Concatenation, not a preprocessor — the same
/// assembly `wgsl_template::scene_shader` and `surface_program::parity` use.
pub(crate) fn bake_program_wgsl(library: &str, surface: &str) -> String {
    [BAKE_HEADER_WGSL, library, surface, BAKE_FOOTER_WGSL].concat()
}

/// `uStrength = (def.relief ?? 0.02) / (def.worldSize ?? 2)`
/// (`generator.js:305`) — "slope is (relief metres / worldSize metres) so the
/// normal map is physically consistent with the mapping scale used later".
///
/// The defaults are applied by the caller (they are `??` on an absent JS
/// property, not on a present zero), so this is the division alone.
pub(crate) fn sobel_strength(relief: f32, world_size: f32) -> f32 {
    relief / world_size
}

/// `1 / size` — the source's `uTexel` (`generator.js:304`) and the `inv_size`
/// that turns a fragment coordinate into the source's `vUv`.
pub(crate) fn inv_size(size: u32) -> f32 {
    1.0 / (size as f32)
}

/// `this._target(size, { srgb: def.linearAlbedo !== true })`
/// (`generator.js:276`, `201-213`): the albedo target is sRGB-encoded on write
/// unless the map is data rather than colour.
///
/// Returned as a bool rather than a `wgpu::TextureFormat` so the decision is
/// visible on every build arm; [`albedo_texture_format`] applies it.
pub(crate) fn albedo_is_srgb(linear_albedo: bool) -> bool {
    !linear_albedo
}

/// `copy_texture_to_buffer` aligns each row to 256 bytes; this is the padded
/// stride for a `size`-wide RGBA8 map. Same rule as `offscreen.rs:346-347`.
pub(crate) fn padded_row_bytes(size: u32) -> u32 {
    (size * 4).div_ceil(256) * 256
}

/// Strip the `copy_texture_to_buffer` row padding back to `size * 4` per row.
pub(crate) fn unpad_rows(mapped: &[u8], size: u32) -> Vec<u8> {
    let unpadded = (size * 4) as usize;
    let padded = padded_row_bytes(size) as usize;
    (0..size as usize).fold(
        Vec::with_capacity(ProceduralBakeMaps::map_bytes(size)),
        |mut pixels, row| {
            let start = row * padded;
            pixels.extend_from_slice(&mapped[start..start + unpadded]);
            pixels
        },
    )
}

/// The 64 bytes of [`BAKE_HEADER_WGSL`]'s `OwBakeUniforms`, in its declared
/// layout. Written by hand rather than through `bytemuck` so the layout is
/// legible beside the WGSL struct it must match, and so it is checked by a test
/// on every build arm rather than only under `--features offscreen`.
pub(crate) fn surface_uniform_bytes(
    request: &ProceduralBakeRequest,
    output: BakeOutput,
) -> [u8; 64] {
    let mut bytes = [0_u8; 64];
    let fields: [(usize, [u8; 4]); 12] = [
        (0, request.tint_a()[0].to_le_bytes()),
        (4, request.tint_a()[1].to_le_bytes()),
        (8, request.tint_a()[2].to_le_bytes()),
        (12, request.seed().to_le_bytes()),
        (16, request.tint_b()[0].to_le_bytes()),
        (20, request.tint_b()[1].to_le_bytes()),
        (24, request.tint_b()[2].to_le_bytes()),
        (28, output.code().to_le_bytes()),
        (32, request.param()[0].to_le_bytes()),
        (36, request.param()[1].to_le_bytes()),
        (40, request.param()[2].to_le_bytes()),
        (44, request.param()[3].to_le_bytes()),
    ];
    fields.iter().for_each(|(at, word)| {
        bytes[*at..*at + 4].copy_from_slice(word);
    });
    bytes[48..52].copy_from_slice(&inv_size(request.size()).to_le_bytes());
    bytes
}

/// The 16 bytes of [`BAKE_SOBEL_WGSL`]'s `OwSobelUniforms`: `uTexel` then
/// `uStrength` (`generator.js:303-305`).
pub(crate) fn sobel_uniform_bytes(size: u32, relief: f32, world_size: f32) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    let texel = inv_size(size);
    bytes[0..4].copy_from_slice(&texel.to_le_bytes());
    bytes[4..8].copy_from_slice(&texel.to_le_bytes());
    bytes[8..12].copy_from_slice(&sobel_strength(relief, world_size).to_le_bytes());
    bytes
}

// ---------------------------------------------------------------------------
// The wgpu arm.
// ---------------------------------------------------------------------------

/// The scratch height target's format — the source's
/// `canHalf ? THREE.HalfFloatType : THREE.UnsignedByteType`
/// (`generator.js:180-186`).
///
/// `half_float_targets` is the caller's capability answer, standing in for the
/// source's `EXT_color_buffer_float` / `EXT_color_buffer_half_float` probe. On
/// a WebGPU core adapter it is always true; the WebGL2 downlevel arm is the one
/// that can say no, which is exactly the case the source guards.
///
/// The 8-bit fallback is `Rgba8Unorm`, **not** `Rgba8UnormSrgb`: the source's
/// fallback target is `RGBAFormat` with no colour space
/// (`generator.js:185-195`), and an sRGB encode on a height field would be a
/// silent, invisible corruption of the Sobel's input.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const fn height_format(half_float_targets: bool) -> wgpu::TextureFormat {
    [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba16Float,
    ][half_float_targets as usize]
}

/// The albedo target's format — see [`albedo_is_srgb`].
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const fn albedo_texture_format(linear_albedo: bool) -> wgpu::TextureFormat {
    [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Rgba8Unorm,
    ][linear_albedo as usize]
}

/// The ORM and normal targets: linear 8-bit, `THREE.NoColorSpace`
/// (`generator.js:201-213`).
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const DATA_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A render target plus its view. `TextureForge._target` / `_heightRT`
/// (`generator.js:177-217`), minus the mip chain and the anisotropy — both are
/// sampler-side settings this engine applies at upload, not bake, time.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
struct BakeTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
impl core::fmt::Debug for BakeTarget {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BakeTarget")
            .field("size", &self.texture.size())
            .field("format", &self.texture.format())
            .finish()
    }
}

#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
fn make_target(
    device: &wgpu::Device,
    label: &str,
    size: u32,
    format: wgpu::TextureFormat,
) -> BakeTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    BakeTarget { texture, view }
}

/// One full-screen draw into `target`, with `bind_group` at slot 0.
///
/// `r.autoClear = false` (`generator.js:267`) is why the load op is
/// `LoadOp::Load`-equivalent in the source; here every draw covers every texel
/// of a freshly created target, so the clear is a no-op either way and
/// `LoadOp::Clear` is the honest description of a fresh attachment.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
fn draw_fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    view: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("axiom-texture-bake-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Read one RGBA8 target back to the CPU, un-padding the 256-byte rows.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &BakeTarget,
    size: u32,
) -> Vec<u8> {
    let padded = padded_row_bytes(size);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("axiom-texture-bake-readback"),
        size: u64::from(padded) * u64::from(size),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("axiom-texture-bake-readback-encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size),
            },
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(core::iter::once(encoder.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait)
        .expect("the bake read-back must complete");
    let mapped = slice.get_mapped_range();
    let pixels = unpad_rows(&mapped, size);
    drop(mapped);
    readback.unmap();
    pixels
}

/// `TextureForge.build(def)` (`generator.js:260-321`) on a real device.
///
/// The four passes run in the source's order — height, albedo, ORM, Sobel —
/// against one program compiled from [`bake_program_wgsl`]. Returns `None` only
/// if the surface program fails to compile, which the caller surfaces as the
/// authoring error it is.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) fn bake_on_device(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    library_wgsl: &str,
    request: &ProceduralBakeRequest,
    half_float_targets: bool,
) -> ProceduralBakeMaps {
    use wgpu::util::DeviceExt as _;

    let size = request.size();
    let source = bake_program_wgsl(library_wgsl, request.surface_wgsl());
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("axiom-texture-bake-surface"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });

    let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("axiom-texture-bake-uniform-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let surface_pipeline_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-texture-bake-surface-layout"),
            bind_group_layouts: &[&uniform_layout],
            push_constant_ranges: &[],
        });

    // One pipeline per target format: the colour attachment's format is part of
    // the pipeline, and the three outputs do not share one.
    let surface_pipeline = |format: wgpu::TextureFormat, label: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&surface_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("ow_bake_vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("ow_bake_fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        })
    };

    let uniform_buffer = |output: BakeOutput| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("axiom-texture-bake-uniforms"),
            contents: &surface_uniform_bytes(request, output),
            usage: wgpu::BufferUsages::UNIFORM,
        })
    };
    let uniform_group = |buffer: &wgpu::Buffer| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-texture-bake-uniform-group"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        })
    };

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("axiom-texture-bake-encoder"),
    });

    // Pass 0 — the scratch height target. "The height pass exists only to feed
    // the Sobel, so it is skipped with it" (generator.js:280).
    let height_target = request.want_normal().then(|| {
        let target = make_target(
            device,
            "axiom-texture-bake-height",
            size,
            height_format(half_float_targets),
        );
        let pipeline = surface_pipeline(
            height_format(half_float_targets),
            "axiom-texture-bake-height-pipeline",
        );
        let buffer = uniform_buffer(BakeOutput::Height);
        let group = uniform_group(&buffer);
        draw_fullscreen(&mut encoder, &pipeline, &group, &target.view);
        target
    });

    // Pass 1 — albedo.
    let albedo_format = albedo_texture_format(request.linear_albedo());
    let albedo_target = make_target(device, "axiom-texture-bake-albedo", size, albedo_format);
    let albedo_pipeline = surface_pipeline(albedo_format, "axiom-texture-bake-albedo-pipeline");
    let albedo_uniforms = uniform_buffer(BakeOutput::Albedo);
    let albedo_group = uniform_group(&albedo_uniforms);
    draw_fullscreen(
        &mut encoder,
        &albedo_pipeline,
        &albedo_group,
        &albedo_target.view,
    );

    // Pass 2 — ORM.
    let orm_target = request.want_orm().then(|| {
        let target = make_target(device, "axiom-texture-bake-orm", size, DATA_FORMAT);
        let pipeline = surface_pipeline(DATA_FORMAT, "axiom-texture-bake-orm-pipeline");
        let buffer = uniform_buffer(BakeOutput::Orm);
        let group = uniform_group(&buffer);
        draw_fullscreen(&mut encoder, &pipeline, &group, &target.view);
        target
    });

    // Pass 3 — height -> normal.
    let normal_target = height_target.as_ref().map(|height| {
        let target = make_target(device, "axiom-texture-bake-normal", size, DATA_FORMAT);
        let sobel_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-texture-bake-sobel"),
            source: wgpu::ShaderSource::Wgsl(BAKE_SOBEL_WGSL.into()),
        });
        let sobel_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-texture-bake-sobel-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // RepeatWrapping + LinearFilter (generator.js:188-191).
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-texture-bake-height-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        let sobel_uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("axiom-texture-bake-sobel-uniforms"),
            contents: &sobel_uniform_bytes(size, request.relief(), request.world_size()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-texture-bake-sobel-group"),
            layout: &sobel_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: sobel_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&height.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-texture-bake-sobel-pipeline-layout"),
            bind_group_layouts: &[&sobel_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-texture-bake-sobel-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sobel_module,
                entry_point: Some("ow_sobel_vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &sobel_module,
                entry_point: Some("ow_sobel_fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DATA_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        draw_fullscreen(&mut encoder, &pipeline, &group, &target.view);
        target
    });

    queue.submit(core::iter::once(encoder.finish()));

    ProceduralBakeMaps::new(
        size,
        read_back(device, queue, &albedo_target, size),
        orm_target
            .as_ref()
            .map(|target| read_back(device, queue, target, size)),
        normal_target
            .as_ref()
            .map(|target| read_back(device, queue, target, size)),
    )
}

/// The native off-screen entry: bake one request on the process-wide device.
///
/// `None` when the machine has no adapter — the same contract
/// `offscreen::render_to_rgba` has, and for the same reason.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
pub(crate) fn bake_offscreen(
    library_wgsl: &str,
    request: &ProceduralBakeRequest,
) -> Option<ProceduralBakeMaps> {
    crate::native_gpu::shared().map(|native| {
        bake_on_device(
            &native.device,
            &native.queue,
            library_wgsl,
            request,
            // A WebGPU core adapter always renders to Rgba16Float, which is the
            // `canHalf == true` arm of generator.js:182-186.
            true,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A probe request with every field at a distinct, recognisable value, so a
    /// uniform-layout test can tell one lane from another.
    fn probe(surface: &str) -> ProceduralBakeRequest {
        ProceduralBakeRequest::new("probe".to_string(), surface.to_string(), 4)
            .with_seed(1.0)
            .with_tints([0.25, 0.5, 0.75], [1.0, 0.5, 0.0])
            .with_param([1.5, 2.5, 3.5, 4.5])
            .with_scale(2.0, 0.02)
    }

    #[test]
    fn the_program_is_the_sources_four_part_concatenation() {
        let source = bake_program_wgsl("// library\n", "// surface\n");
        let header_at = source.find("struct OwBakeUniforms").expect("header present");
        let library_at = source.find("// library").expect("library present");
        let surface_at = source.find("// surface").expect("surface present");
        let footer_at = source.find("fn ow_bake_fs").expect("footer present");
        assert!(
            header_at < library_at && library_at < surface_at && surface_at < footer_at,
            "generator.js:224 orders HEADER + NOISE + surface + FOOTER, got {header_at} \
             {library_at} {surface_at} {footer_at}"
        );
    }

    #[test]
    fn the_footer_selects_the_three_outputs_by_the_sources_integers() {
        assert_eq!(BakeOutput::Height.code(), 0, "generator.js:47");
        assert_eq!(BakeOutput::Albedo.code(), 1, "generator.js:48");
        assert_eq!(BakeOutput::Orm.code(), 2, "generator.js:49 is the else arm");
        assert!(
            BAKE_FOOTER_WGSL.contains("U.output_mode == 0")
                && BAKE_FOOTER_WGSL.contains("U.output_mode == 1"),
            "the footer compares uOutput against 0 and 1"
        );
    }

    #[test]
    fn the_footer_carries_the_sources_five_defaults() {
        // generator.js:44-45. Dead in every ported body, and still the source.
        assert!(BAKE_FOOTER_WGSL.contains("var alb = vec3<f32>(0.5);"));
        assert!(BAKE_FOOTER_WGSL.contains("var h : f32 = 0.5;"));
        assert!(BAKE_FOOTER_WGSL.contains("var rough : f32 = 0.5;"));
        assert!(BAKE_FOOTER_WGSL.contains("var metal : f32 = 0.0;"));
        assert!(BAKE_FOOTER_WGSL.contains("var ao : f32 = 1.0;"));
    }

    #[test]
    fn the_sobel_keeps_the_sources_division_by_the_texel_size() {
        // The CPU port writes this as `* size`; the source writes a division and
        // the source's grouping is the specification.
        assert!(
            BAKE_SOBEL_WGSL.contains("let sx = dx / S.texel.x;")
                && BAKE_SOBEL_WGSL.contains("let sy = dy / S.texel.y;"),
            "generator.js:72-73 divides by uTexel"
        );
        assert!(
            BAKE_SOBEL_WGSL.contains("* 0.125;"),
            "the 1/8 kernel normalisation is a multiply by 0.125, as written"
        );
    }

    #[test]
    fn the_sobel_strength_is_relief_over_world_size() {
        // generator.js:305.
        assert_eq!(sobel_strength(0.02, 2.0), 0.01);
        assert_eq!(sobel_strength(0.0034, 0.25), 0.0136);
        assert_eq!(sobel_strength(0.5, 32.0), 0.015625);
    }

    #[test]
    fn inv_size_is_one_over_the_tile() {
        assert_eq!(inv_size(1024), 1.0 / 1024.0);
        assert_eq!(inv_size(64), 0.015_625);
    }

    #[test]
    fn the_albedo_target_is_srgb_unless_the_map_is_data() {
        // generator.js:276 + 355-358.
        assert!(albedo_is_srgb(false), "a colour map is sRGB-encoded on write");
        assert!(
            !albedo_is_srgb(true),
            "linearAlbedo: true is the detail/macro case — data, not colour"
        );
    }

    #[test]
    fn rows_pad_to_the_copy_alignment() {
        assert_eq!(padded_row_bytes(64), 256, "64 texels is exactly one row");
        assert_eq!(padded_row_bytes(1024), 4096);
        assert_eq!(padded_row_bytes(3), 256, "12 bytes pads up to 256");
    }

    #[test]
    fn unpadding_recovers_the_tight_rows() {
        let size = 3_u32;
        let padded = padded_row_bytes(size) as usize;
        let mut mapped = vec![0_u8; padded * size as usize];
        (0..size as usize).for_each(|row| {
            (0..12).for_each(|byte| {
                mapped[row * padded + byte] = (row * 12 + byte) as u8;
            });
        });
        let pixels = unpad_rows(&mapped, size);
        assert_eq!(pixels.len(), ProceduralBakeMaps::map_bytes(size));
        assert_eq!(
            pixels,
            (0..36).map(|byte| byte as u8).collect::<Vec<u8>>(),
            "the padding between rows is dropped and nothing else"
        );
    }

    #[test]
    fn the_surface_uniform_matches_the_wgsl_struct_layout() {
        let request = probe("");
        let bytes = surface_uniform_bytes(&request, BakeOutput::Albedo);
        let word = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
        assert_eq!(word(0), 0.25, "tint_a.x at 0");
        assert_eq!(word(4), 0.5, "tint_a.y at 4");
        assert_eq!(word(8), 0.75, "tint_a.z at 8");
        assert_eq!(word(12), 1.0, "seed at 12");
        assert_eq!(word(16), 1.0, "tint_b.x at 16");
        assert_eq!(word(20), 0.5, "tint_b.y at 20");
        assert_eq!(word(24), 0.0, "tint_b.z at 24");
        assert_eq!(
            i32::from_le_bytes(bytes[28..32].try_into().expect("4 bytes")),
            1,
            "output_mode at 28"
        );
        assert_eq!(word(32), 1.5, "param.x at 32");
        assert_eq!(word(36), 2.5, "param.y at 36");
        assert_eq!(word(40), 3.5, "param.z at 40");
        assert_eq!(word(44), 4.5, "param.w at 44");
        assert_eq!(word(48), 0.25, "inv_size at 48 for a 4px tile");
        assert_eq!(&bytes[52..64], &[0_u8; 12], "the tail is padding");
    }

    #[test]
    fn the_output_mode_is_the_only_thing_that_moves_between_passes() {
        let request = probe("");
        let height = surface_uniform_bytes(&request, BakeOutput::Height);
        let orm = surface_uniform_bytes(&request, BakeOutput::Orm);
        assert_eq!(&height[0..28], &orm[0..28], "the surface inputs are shared");
        assert_eq!(&height[32..], &orm[32..], "so is everything past the mode");
        assert_eq!(
            i32::from_le_bytes(height[28..32].try_into().expect("4 bytes")),
            0
        );
        assert_eq!(
            i32::from_le_bytes(orm[28..32].try_into().expect("4 bytes")),
            2
        );
    }

    #[test]
    fn the_sobel_uniform_carries_the_texel_and_the_strength() {
        let bytes = sobel_uniform_bytes(1024, 0.02, 2.0);
        let word = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
        assert_eq!(word(0), 1.0 / 1024.0, "uTexel.x");
        assert_eq!(word(4), 1.0 / 1024.0, "uTexel.y");
        assert_eq!(word(8), 0.01, "uStrength = relief / worldSize");
        assert_eq!(word(12), 0.0, "the pad word");
    }

    #[test]
    fn the_header_names_the_uniforms_the_generators_read() {
        // The transcription contract: uSeed -> U.seed, uTintA -> U.tint_a,
        // uTintB -> U.tint_b, uParam -> U.param.
        ["tint_a", "seed", "tint_b", "output_mode", "param", "inv_size"]
            .iter()
            .for_each(|field| {
                assert!(
                    BAKE_HEADER_WGSL.contains(field),
                    "OwBakeUniforms must declare {field}"
                );
            });
        assert!(
            BAKE_HEADER_WGSL.contains("var<uniform> U : OwBakeUniforms"),
            "the generators spell the uniform block `U`"
        );
    }

    #[test]
    fn the_uv_comes_from_the_fragment_position_not_a_flipped_varying() {
        // See the module doc's "v axis": a WebGL target's row 0 is the bottom,
        // a WebGPU target's row 0 is the top, and `position.xy * inv_size` is
        // the mapping that agrees with both the source and `bake.rs::texel_uv`.
        assert!(BAKE_FOOTER_WGSL.contains("let vUv = position.xy * U.inv_size;"));
        assert!(BAKE_SOBEL_WGSL.contains("let vUv = position.xy * S.texel;"));
    }

    #[cfg(feature = "offscreen")]
    mod gpu {
        use super::*;

        /// A surface with no noise in it at all: every output is a constant, so
        /// the expected bytes are computable by hand and any disagreement is
        /// the bake's, not the generator's.
        const CONSTANT_SURFACE: &str = r##"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  *albOut = U.tint_a;
  *hOut = U.param.x;
  *roughOut = U.param.y;
  *metalOut = U.param.z;
  *aoOut = U.param.w;
}
"##;

        /// A height field that is an exact linear ramp in u, so the Sobel has a
        /// closed-form answer: `dx = slope / size`, `dy = 0`.
        const RAMP_SURFACE: &str = r##"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  *albOut = vec3<f32>(uv.x, uv.y, 0.0);
  *hOut = uv.x * U.param.x + U.param.y;
  *roughOut = 0.5;
  *metalOut = 0.0;
  *aoOut = 1.0;
}
"##;

        fn bake(request: &ProceduralBakeRequest) -> ProceduralBakeMaps {
            let gpu = crate::test_gpu::TestGpu::shared();
            assert_ne!(
                gpu.backend,
                wgpu::Backend::Noop,
                "a bake test needs a real adapter"
            );
            bake_on_device(&gpu.device, &gpu.queue, "", request, true)
        }

        fn texel(map: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
            let at = ((y * size + x) * 4) as usize;
            [map[at], map[at + 1], map[at + 2], map[at + 3]]
        }

        #[test]
        fn a_constant_surface_bakes_the_constant_into_every_channel() {
            let request =
                ProceduralBakeRequest::new("constant".to_string(), CONSTANT_SURFACE.to_string(), 8)
                    .with_linear_albedo(true)
                    // 0, 128, 255 exactly, and 64, 192, 0, 255 exactly.
                    .with_tints([0.0, 0.5019608, 1.0], [1.0, 1.0, 1.0])
                    .with_param([0.2509804, 0.7529412, 0.0, 1.0]);
            let maps = bake(&request);
            assert_eq!(maps.size(), 8);
            assert_eq!(maps.albedo().len(), ProceduralBakeMaps::map_bytes(8));
            let albedo = texel(maps.albedo(), 8, 3, 5);
            assert_eq!(
                albedo,
                [0, 128, 255, 64],
                "albedo.rgb is the linear tint and albedo.a is the height"
            );
            let orm = texel(maps.orm().expect("ORM was requested"), 8, 3, 5);
            assert_eq!(orm, [255, 192, 0, 255], "ORM is (ao, rough, metal, 1)");
        }

        #[test]
        fn the_srgb_target_encodes_on_write_and_the_linear_one_does_not() {
            let base =
                ProceduralBakeRequest::new("encode".to_string(), CONSTANT_SURFACE.to_string(), 8)
                    .with_tints([0.2159, 0.2159, 0.2159], [1.0, 1.0, 1.0])
                    .with_param([0.5, 0.5, 0.0, 1.0]);

            let linear = texel(
                bake(&base.clone().with_linear_albedo(true)).albedo(),
                8,
                0,
                0,
            );
            let encoded = texel(bake(&base.with_linear_albedo(false)).albedo(), 8, 0, 0);

            assert_eq!(linear[0], 55, "0.2159 * 255 rounds to 55");
            // The IEC 61966-2-1 encode of 0.2159 is 0.5039, i.e. 128/255.
            assert!(
                (127..=129).contains(&encoded[0]),
                "an Rgba8UnormSrgb target encodes on write: got {}, expected ~128. \
                 This is the write half of G16 — the encode and the binding are \
                 chosen by the same flag, so a bake cannot land in it.",
                encoded[0]
            );
            assert_eq!(
                linear[3], encoded[3],
                "alpha is never sRGB-encoded, so the height survives both targets"
            );
        }

        #[test]
        fn the_sobel_of_a_linear_ramp_is_the_analytic_normal() {
            let size = 32_u32;
            let slope = 0.5_f32;
            let request =
                ProceduralBakeRequest::new("ramp".to_string(), RAMP_SURFACE.to_string(), size)
                    .with_param([slope, 0.25, 0.0, 0.0])
                    .with_scale(2.0, 0.02)
                    .with_maps(false, true);
            let maps = bake(&request);
            assert!(maps.orm().is_none(), "want_orm: false skips the ORM pass");
            let normal = maps.normal().expect("a normal was requested");

            // h = u * slope + c, so dh/du over one texel is slope / size; the
            // Sobel's per-tile slope is therefore `slope`, and
            // n = normalize(-slope * strength, 0, 1).
            let strength = sobel_strength(request.relief(), request.world_size());
            let nx = -slope * strength;
            let len = (nx * nx + 1.0_f32).sqrt();
            let expect_x = (nx / len) * 0.5 + 0.5;
            let expect_y = 0.5;
            let expect_z = (1.0 / len) * 0.5 + 0.5;

            // Away from the wrap seam, where the ramp is continuous.
            let got = texel(normal, size, size / 2, size / 2);
            let delta = |byte: u8, expect: f32| (f32::from(byte) / 255.0 - expect).abs();
            assert!(
                delta(got[0], expect_x) <= 2.0 / 255.0,
                "normal.x: got {}, expected {expect_x} ({} LSB out)",
                got[0],
                delta(got[0], expect_x) * 255.0
            );
            assert!(
                delta(got[1], expect_y) <= 2.0 / 255.0,
                "normal.y: a u-only ramp has no v slope; got {}",
                got[1]
            );
            assert!(
                delta(got[2], expect_z) <= 2.0 / 255.0,
                "normal.z: got {}, expected {expect_z}",
                got[2]
            );
            assert_eq!(got[3], 255, "the normal target's alpha is 1");
        }

        #[test]
        fn the_uv_of_row_zero_is_the_low_v_row() {
            // The v-axis check the module doc argues for: with `RAMP_SURFACE`
            // writing `alb = vec3(uv.x, uv.y, 0)`, texel (0, 0) must read
            // (0.5/size, 0.5/size) and NOT (0.5/size, 1 - 0.5/size).
            let size = 16_u32;
            let request =
                ProceduralBakeRequest::new("axis".to_string(), RAMP_SURFACE.to_string(), size)
                    .with_linear_albedo(true)
                    .with_maps(false, false);
            let maps = bake(&request);
            let first = texel(maps.albedo(), size, 0, 0);
            let last = texel(maps.albedo(), size, size - 1, size - 1);
            assert!(
                first[1] < 16,
                "row 0 must be the v ~ 0 row (bake.rs::texel_uv's convention); \
                 got v byte {}",
                first[1]
            );
            assert!(
                last[1] > 239,
                "the last row must be the v ~ 1 row; got v byte {}",
                last[1]
            );
            assert!(
                first[0] < 16 && last[0] > 239,
                "u must increase with x: got {} then {}",
                first[0],
                last[0]
            );
        }

        #[test]
        fn skipping_the_normal_skips_the_height_pass_with_it() {
            // generator.js:280 — "the height pass exists only to feed the
            // Sobel, so it is skipped with it".
            let request = ProceduralBakeRequest::new(
                "albedo-only".to_string(),
                CONSTANT_SURFACE.to_string(),
                8,
            )
            .with_maps(false, false);
            assert_eq!(request.pass_count(), 1, "one draw, not four");
            let maps = bake(&request);
            assert!(maps.normal().is_none());
            assert!(maps.orm().is_none());
            assert_eq!(maps.albedo().len(), ProceduralBakeMaps::map_bytes(8));
        }

        #[test]
        fn the_height_scratch_format_mirrors_the_sources_capability_fallback() {
            assert_eq!(height_format(true), wgpu::TextureFormat::Rgba16Float);
            assert_eq!(
                height_format(false),
                wgpu::TextureFormat::Rgba8Unorm,
                "the 8-bit fallback is linear, never sRGB — an sRGB encode of a \
                 height field would silently corrupt the Sobel's input"
            );
            assert_eq!(
                albedo_texture_format(false),
                wgpu::TextureFormat::Rgba8UnormSrgb
            );
            assert_eq!(albedo_texture_format(true), wgpu::TextureFormat::Rgba8Unorm);
            assert_eq!(DATA_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
        }

        #[test]
        fn a_half_float_scratch_is_finer_than_an_eight_bit_one() {
            // The whole reason generator.js:180-181 asks for a half-float
            // target. A gentle ramp differentiated through an 8-bit scratch
            // stair-steps; through a half-float one it does not.
            let size = 64_u32;
            let request =
                ProceduralBakeRequest::new("scratch".to_string(), RAMP_SURFACE.to_string(), size)
                    // 0.3 LSB of 8-bit per texel: below the 8-bit scratch's
                    // resolution and far above the half-float's.
                    .with_param([0.02, 0.4, 0.0, 0.0])
                    .with_maps(false, true);
            let gpu = crate::test_gpu::TestGpu::shared();
            let half = bake_on_device(&gpu.device, &gpu.queue, "", &request, true);
            let byte = bake_on_device(&gpu.device, &gpu.queue, "", &request, false);
            let spread = |maps: &ProceduralBakeMaps| {
                let map = maps.normal().expect("normal requested");
                let row: Vec<u8> = (0..size).map(|x| texel(map, size, x, size / 2)[0]).collect();
                let hi = row.iter().copied().max().expect("a row");
                let lo = row.iter().copied().min().expect("a row");
                u32::from(hi) - u32::from(lo)
            };
            assert!(
                spread(&half) < spread(&byte),
                "the half-float scratch must give a flatter normal across a \
                 constant-slope ramp than an 8-bit one: half {} vs byte {}",
                spread(&half),
                spread(&byte)
            );
        }
    }
}
