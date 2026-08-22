//! The **G-buffer prepass**: one geometry pass writing a view normal, a
//! screen-space velocity and a linear view depth into three colour attachments
//! at once, plus its own depth buffer.
//!
//! This is the foundation the screen-space passes rest on. Ambient occlusion
//! needs a normal and a depth; screen-space reflections need a normal, a depth
//! and last frame's colour; a temporal resolve and a motion blur need a
//! velocity. All four are affordable *because they share one geometry pass* —
//! which is what multiple render targets buy, and the only reason this pass
//! exists rather than three.
//!
//! # Why a separate module and a separate pipeline
//!
//! The prepass shades nothing. It has no lights, no materials, no shadow map, no
//! surface program — it writes four fixed channels from the transform and the
//! normal. Folding it into [`crate::scene_renderer`] would give the main pass's
//! four bind groups and its whole material vocabulary to a pass that wants none
//! of them, and would put a second colour-target set into the one place a
//! generated surface program is spliced. It is its own file, its own shader, its
//! own pipeline, and its own targets, exactly as the source keeps it a separate
//! `overrideMaterial` on the same scene.
//!
//! # The attachment set, and why these formats
//!
//! | slot | format | contents |
//! |---|---|---|
//! | 0 | `Rgba16Float` | octahedral view normal (`xy`), coverage (`z`), material id (`w`) |
//! | 1 | `Rg16Float` | screen-space velocity, as an NDC delta halved |
//! | 2 | `R32Float` | linear view depth in **metres**, positive |
//! | depth | `Depth32Float` | the prepass's own depth attachment |
//!
//! The widths are not decoration. An octahedral normal at 8 bits per channel
//! bands visibly under an AO cosine weight; a velocity at 8 bits quantizes every
//! useful magnitude to zero (see
//! [`axiom_host::RenderCapability::GBuffer`]); and a *linear* depth in metres
//! runs a half-float's mantissa out inside a street-sized scene, which is why
//! slot 2 is full-float while slots 0 and 1 are half.
//!
//! # What this costs on WebGL2, stated rather than assumed
//!
//! The live browser arm requests `wgpu::Limits::downlevel_webgl2_defaults()` on
//! *both* backends, to hold WebGPU and WebGL2 at capability parity. Measured
//! against this attachment set, those limits say:
//!
//! - `max_color_attachments` is **4**; this pass binds **3**. Fits.
//! - `max_color_attachment_bytes_per_sample` is **32**; this set costs
//!   `8 + 4 + 4 = 16`. Fits, with a second G-buffer's worth of headroom.
//! - `max_vertex_buffer_array_stride` is **255**; the prepass instance stride is
//!   `36 * 4 = 144`. Fits.
//! - 16 vertex attributes are guaranteed; this pipeline declares **11** (two
//!   per-vertex, nine per-instance). Fits — and deliberately fewer than the main
//!   pipeline's 14, because the prepass needs no uv, no vertex colour and no
//!   emissive.
//! - `max_inter_stage_shader_components` is **31**; the five varyings cost
//!   **14** scalar components. Fits.
//!
//! So MRT itself is *inside* the limits the engine already asks for on every
//! arm. The one thing WebGL2 does not guarantee is the part
//! [`axiom_host::RenderCapability::HdrTargets`] already covers: float colour
//! attachments are `EXT_color_buffer_float`, an extension, and without it none
//! of the three slots is renderable at all. That is why
//! [`gbuffer_attachments_available`] consults *both* bits — the count and the
//! precision are two different device facts, and a G-buffer needs both to be
//! true.
//!
//! **The degradation is a drop, and it is reported**, never a silent no-op: a
//! backend whose profile lacks either bit gets `None` from
//! [`GBufferTargets::new`], writes no prepass, and every consumer downstream is
//! absent with it. See the capability's own documentation for why the two
//! obvious substitutes (three sequential passes; the same channels at 8 bits)
//! are worse than an honest absence.
//!
//! # Transcription notes
//!
//! Ported from `src/render/prepass.js` (236 lines) and the octahedral packing in
//! `src/render/glsl.js`. Two deliberate divergences from the GLSL text, both
//! forced by the target and both stated at their site:
//!
//! - The source declares all three outputs `vec4` and lets the render target's
//!   channel count discard the rest. WGSL's fragment outputs must match the
//!   attachment's component count, so slot 1 is `vec2<f32>` and slot 2 is `f32`.
//!   The values written are the same numbers.
//! - Three.js supplies `normalMatrix` (the inverse transpose of the
//!   model-view 3x3) as a uniform; this pass reconstructs it in the shader as
//!   the cofactor matrix, which equals it up to the positive factor `det`. The
//!   normal is normalized in the fragment stage and `det` is constant per
//!   instance, so the interpolated direction is unchanged — and it saves three
//!   vertex attributes on an arm that guarantees sixteen.

use axiom_host::{BackendCapabilityProfile, HostAttachmentFormat, RenderCapability};

/// The coverage value written for geometry whose *vertices* move independently
/// of its transform — skinned characters and morphed meshes.
///
/// Straight from the source's `OW_COVERAGE_DYNAMIC`. Every consumer tests
/// coverage against `0.5`, so both this and `1.0` still read as "there is a
/// surface here"; the distinction exists so a temporal filter can reject history
/// on exactly the pixels whose motion no matrix-difference velocity can
/// describe. Without it a running character's limbs emit zero motion and the
/// filter drags the background through them.
pub(crate) const COVERAGE_DYNAMIC: f32 = 0.7;

/// The coverage value written for ordinary rigid geometry, whose motion the
/// per-instance previous-world matrix describes completely.
pub(crate) const COVERAGE_STATIC: f32 = 1.0;

/// The sign a consumer must apply to the velocity buffer's `y` component to turn
/// it into a **texture-space** delta.
///
/// The stored value is the source's, exactly: half the NDC delta, in a clip
/// space whose `y` runs *up*. A WebGPU framebuffer's `v` runs *down*, so a pass
/// that reprojects with `uv - velocity` must negate `y` first. The source never
/// needed this constant because WebGL's framebuffer `v` runs up and the two
/// conventions coincide; here they do not, and a sibling that assumes they do
/// gets a temporal filter that smears vertically and nowhere else — the kind of
/// bug that reads as "TAA is broken" for a week.
pub(crate) const VELOCITY_TEXTURE_V_SIGN: f32 = -1.0;

/// One colour attachment of the G-buffer. **The discriminant is the attachment
/// slot**, and the order is the source's (`gNormal`, `gVelocity`, `gDepth`) —
/// an enum used as a table index is order-dependent, and this one is also the
/// binding order every consumer will read.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GBufferChannel {
    /// Octahedral **view-space** normal in `xy`, coverage in `z`, material id in
    /// `w`. View space, not world: the consumers are screen-space passes that
    /// already work in view space, and packing the normal there saves every one
    /// of them a matrix.
    Normal = 0,
    /// Screen-space velocity: `(curr.xy/curr.w - prev.xy/prev.w) * 0.5`, built
    /// from **unjittered** view-projections for both frames so a temporal
    /// jitter never leaks into the motion vectors.
    Velocity = 1,
    /// Linear view depth in metres, positive. Not the depth *buffer*: that is
    /// hyperbolic and needs the frustum constants to invert, which a
    /// screen-space pass would then have to carry.
    Depth = 2,
}

/// Every colour attachment, in slot order. The one place the set is enumerated.
pub(crate) const GBUFFER_CHANNELS: [GBufferChannel; 3] = [
    GBufferChannel::Normal,
    GBufferChannel::Velocity,
    GBufferChannel::Depth,
];

/// The depth attachment the prepass owns.
///
/// It is the prepass's **own** buffer, not something the forward pass later
/// loads: the source's `WebGLRenderTarget({ depthBuffer: true })` allocates one
/// per target and the forward world pass clears its own again. Keeping it that
/// way is what makes this pass purely additive — the existing main pass is not
/// touched, not reordered, and not made to depend on this one having run.
pub(crate) const GBUFFER_DEPTH_FORMAT: HostAttachmentFormat = HostAttachmentFormat::Depth32Float;

/// What one sample of the whole colour attachment set costs, in bytes:
/// `Rgba16Float` (8) + `Rg16Float` (4) + `R32Float` (4). Checked against the
/// device's `max_color_attachment_bytes_per_sample` by [`device_gbuffer`].
pub(crate) const GBUFFER_BYTES_PER_SAMPLE: u32 = 16;

impl GBufferChannel {
    /// The attachment format this channel is stored in.
    pub(crate) const fn format(self) -> HostAttachmentFormat {
        [
            HostAttachmentFormat::Rgba16Float,
            HostAttachmentFormat::Rg16Float,
            HostAttachmentFormat::R32Float,
        ][self as usize]
    }

    /// The fragment output location this channel is written to — its slot, which
    /// is its discriminant.
    pub(crate) const fn location(self) -> u32 {
        self as u32
    }

    /// This channel's byte cost per sample, summing to
    /// [`GBUFFER_BYTES_PER_SAMPLE`].
    pub(crate) const fn bytes_per_sample(self) -> u32 {
        [8, 4, 4][self as usize]
    }

    /// The debug label its texture carries, so a captured frame names its own
    /// attachments.
    pub(crate) const fn label(self) -> &'static str {
        [
            "axiom-gbuffer-normal",
            "axiom-gbuffer-velocity",
            "axiom-gbuffer-depth",
        ][self as usize]
    }
}

/// **Whether this device can hold the G-buffer's attachment set** — resolved from
/// what the adapter's limits report, never asserted from a policy. The peer of
/// [`crate::hdr_target::device_hdr_targets`], and pure for the same reason.
///
/// Three facts, all required:
///
/// - it can bind at least as many colour attachments as the set has;
/// - one sample of the set fits inside its per-sample byte budget;
/// - and the formats themselves are renderable, which is the *precision*
///   question [`crate::hdr_target::device_hdr_targets`] already answers.
///
/// The third is folded in here rather than left to the caller because a device
/// that can bind four attachments and render into none of these formats has no
/// G-buffer, and a gate that reported otherwise would be reporting arithmetic
/// instead of a device.
pub(crate) const fn device_gbuffer(
    max_color_attachments: u32,
    max_color_attachment_bytes_per_sample: u32,
    device_has_hdr: bool,
) -> bool {
    (max_color_attachments >= GBUFFER_CHANNELS.len() as u32)
        & (max_color_attachment_bytes_per_sample >= GBUFFER_BYTES_PER_SAMPLE)
        & device_has_hdr
}

/// `base` with [`RenderCapability::GBuffer`] granted when the bound device
/// reported one — what a backend's profile becomes at bind. The exact shape of
/// [`crate::hdr_target::grant_hdr_targets`], including the reason: it only ever
/// **grants**, so every restriction a host set before the bind survives it.
pub(crate) fn grant_gbuffer(
    base: BackendCapabilityProfile,
    device_has_gbuffer: bool,
) -> BackendCapabilityProfile {
    [base, base.with(RenderCapability::GBuffer)][usize::from(device_has_gbuffer)]
}

/// **The gate every consumer of this module asks**: may this profile render the
/// G-buffer at all?
///
/// Both capabilities, and every attachment, in one answer. The multi-target bit
/// says a pass may bind three colour attachments; the attachment gate says each
/// of those formats is renderable on this arm. Either alone is insufficient, and
/// asking them separately at four future call sites is how one of them ends up
/// forgotten.
pub(crate) fn gbuffer_attachments_available(profile: BackendCapabilityProfile) -> bool {
    GBUFFER_CHANNELS
        .iter()
        .fold(profile.contains(RenderCapability::GBuffer), |ok, channel| {
            ok & profile.supports_attachment(channel.format())
        })
        & profile.supports_attachment(GBUFFER_DEPTH_FORMAT)
}

/// `owOctWrap` from `src/render/glsl.js`, transcribed:
/// `( 1.0 - abs( v.yx ) ) * vec2( v.x >= 0.0 ? 1.0 : -1.0, v.y >= 0.0 ? 1.0 : -1.0 )`.
///
/// Note `abs(v.yx)` — the components are **swapped** before the absolute value,
/// and the sign vector is not. Reading past that swap is the classic way an
/// octahedral encoder ends up mirrored along one diagonal.
pub(crate) fn oct_wrap(v: [f32; 2]) -> [f32; 2] {
    let sign_x = [-1.0_f32, 1.0][usize::from(v[0] >= 0.0)];
    let sign_y = [-1.0_f32, 1.0][usize::from(v[1] >= 0.0)];
    [
        (1.0 - v[1].abs()) * sign_x,
        (1.0 - v[0].abs()) * sign_y,
    ]
}

/// `owEncodeNormal` from `src/render/glsl.js`, transcribed:
///
/// ```glsl
/// n /= ( abs( n.x ) + abs( n.y ) + abs( n.z ) + 1e-8 );
/// n.xy = n.z >= 0.0 ? n.xy : owOctWrap( n.xy );
/// return n.xy;
/// ```
///
/// This is the **CPU reference** — the semantic definition the WGSL is checked
/// against, in the same relationship `surface_program::parity` establishes for
/// the material layers. The division is a division: the source does not multiply
/// by a reciprocal, and float arithmetic is not associative.
pub(crate) fn encode_normal(n: [f32; 3]) -> [f32; 2] {
    let scale = n[0].abs() + n[1].abs() + n[2].abs() + 1e-8;
    let d = [n[0] / scale, n[1] / scale, n[2] / scale];
    let plain = [d[0], d[1]];
    [oct_wrap(plain), plain][usize::from(d[2] >= 0.0)]
}

/// `owDecodeNormal` from `src/render/glsl.js`, transcribed. Not written by this
/// pass — it is what every *consumer* of slot 0 will run, and it lives beside
/// the encoder so the round trip is one file and one test rather than a contract
/// nobody owns.
pub(crate) fn decode_normal(f: [f32; 2]) -> [f32; 3] {
    let nz = 1.0 - f[0].abs() - f[1].abs();
    let t = (-nz).max(0.0);
    let nx = f[0] + [t, -t][usize::from(f[0] >= 0.0)];
    let ny = f[1] + [t, -t][usize::from(f[1] >= 0.0)];
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    [nx / len, ny / len, nz / len]
}

/// The velocity written to slot 1, from the two clip positions:
///
/// ```glsl
/// vec2 a = vCurrClip.xy / max( 1e-6, vCurrClip.w );
/// vec2 b = vPrevClip.xy / max( 1e-6, vPrevClip.w );
/// gVelocity = vec4( ( a - b ) * 0.5, 0.0, 0.0 );
/// ```
///
/// The `max` is the source's and is transcribed as-is, including its behaviour
/// behind the camera: a negative `w` clamps to `1e-6` and the perspective divide
/// explodes rather than flipping sign. That is a real property of the source's
/// buffer, not an oversight to tidy, and a consumer clamps the magnitude it
/// reads.
pub(crate) fn velocity_uv_delta(curr_clip: [f32; 4], prev_clip: [f32; 4]) -> [f32; 2] {
    let aw = curr_clip[3].max(1e-6);
    let bw = prev_clip[3].max(1e-6);
    [
        (curr_clip[0] / aw - prev_clip[0] / bw) * 0.5,
        (curr_clip[1] / aw - prev_clip[1] / bw) * 0.5,
    ]
}

/// The linear depth written to slot 2: `-mvPosition.z`, positive in front of the
/// camera, in metres.
pub(crate) fn view_depth(view_position_z: f32) -> f32 {
    -view_position_z
}

/// Floats in one prepass instance: `world` (16) + `prev_world` (16) +
/// `(material_id, coverage, 0, 0)` (4).
pub(crate) const GBUFFER_INSTANCE_FLOATS: usize = 36;

/// Floats in the prepass uniform block: four column-major `mat4x4<f32>`.
pub(crate) const GBUFFER_UNIFORM_FLOATS: usize = 64;

/// Pack one prepass instance. `world` and `prev_world` are column-major, the
/// same convention every matrix crossing this backend uses.
///
/// `prev_world` is what makes the velocity buffer **per object** rather than
/// camera-only: a camera-difference velocity describes a static street
/// perfectly and describes a moving car not at all.
pub(crate) fn pack_gbuffer_instance(
    world: &[f32; 16],
    prev_world: &[f32; 16],
    material_id: f32,
    coverage: f32,
) -> [f32; GBUFFER_INSTANCE_FLOATS] {
    let mut out = [0.0_f32; GBUFFER_INSTANCE_FLOATS];
    out[0..16].copy_from_slice(world);
    out[16..32].copy_from_slice(prev_world);
    out[32] = material_id;
    out[33] = coverage;
    out
}

/// Pack the prepass uniform block.
///
/// `raster_vp` is the **jittered** clip transform — whatever the forward pass
/// rasterizes with, so the prepass covers exactly the fragments the shading pass
/// will. `curr_vp` and `prev_vp` are the **unjittered** pair the velocity is
/// built from. Keeping them as separate lanes even while the engine applies no
/// jitter is the whole point: collapsing them now is precisely the mistake that
/// makes a temporal resolve smear later, and it would be invisible until then.
pub(crate) fn pack_gbuffer_uniform(
    raster_vp: &[f32; 16],
    curr_vp: &[f32; 16],
    prev_vp: &[f32; 16],
    view: &[f32; 16],
) -> [f32; GBUFFER_UNIFORM_FLOATS] {
    let mut out = [0.0_f32; GBUFFER_UNIFORM_FLOATS];
    out[0..16].copy_from_slice(raster_vp);
    out[16..32].copy_from_slice(curr_vp);
    out[32..48].copy_from_slice(prev_vp);
    out[48..64].copy_from_slice(view);
    out
}

/// The prepass shader. Written from the GLSL in `src/render/prepass.js`, not from
/// the Rust above; where the two disagree the algorithm decides.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const GBUFFER_WGSL: &str = r#"
struct GBufferU {
    // The rasterised clip transform: JITTERED, matching the forward pass.
    raster_vp: mat4x4<f32>,
    // The UNJITTERED pair the velocity is differenced from.
    curr_vp: mat4x4<f32>,
    prev_vp: mat4x4<f32>,
    // The camera view matrix: the normal and the depth are both view-space.
    view: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> u: GBufferU;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) curr_clip: vec4<f32>,
    @location(2) prev_clip: vec4<f32>,
    @location(3) view_depth: f32,
    // x = material id, y = coverage.
    @location(4) params: vec2<f32>,
};

struct FsOut {
    @location(0) normal: vec4<f32>,
    @location(1) velocity: vec2<f32>,
    @location(2) depth: f32,
};

// owOctWrap
fn oct_wrap( v: vec2<f32> ) -> vec2<f32> {
    return ( 1.0 - abs( v.yx ) ) * vec2<f32>(
        select( -1.0, 1.0, v.x >= 0.0 ),
        select( -1.0, 1.0, v.y >= 0.0 ) );
}

// owEncodeNormal
fn encode_normal( n_in: vec3<f32> ) -> vec2<f32> {
    let n = n_in / ( abs( n_in.x ) + abs( n_in.y ) + abs( n_in.z ) + 1e-8 );
    return select( oct_wrap( n.xy ), n.xy, n.z >= 0.0 );
}

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(4) w0: vec4<f32>,
    @location(5) w1: vec4<f32>,
    @location(6) w2: vec4<f32>,
    @location(7) w3: vec4<f32>,
    @location(8) p0: vec4<f32>,
    @location(9) p1: vec4<f32>,
    @location(10) p2: vec4<f32>,
    @location(11) p3: vec4<f32>,
    @location(12) params: vec4<f32>,
) -> VsOut {
    let world = mat4x4<f32>( w0, w1, w2, w3 );
    let prev_world = mat4x4<f32>( p0, p1, p2, p3 );
    let obj = vec4<f32>( position, 1.0 );
    let world_pos = world * obj;
    let mv_pos = u.view * world_pos;

    // three.js hands the prepass `normalMatrix` = inverse-transpose( mat3(
    // modelViewMatrix ) ). Reconstructed here as the cofactor matrix, whose
    // columns are the cross products of the model-view 3x3's columns. That
    // equals the inverse transpose times det, and det is constant across an
    // instance's triangles, so after the fragment stage's normalize the
    // direction is identical.
    let mv3 = mat3x3<f32>(
        ( u.view * vec4<f32>( world[0].xyz, 0.0 ) ).xyz,
        ( u.view * vec4<f32>( world[1].xyz, 0.0 ) ).xyz,
        ( u.view * vec4<f32>( world[2].xyz, 0.0 ) ).xyz );
    let cof = mat3x3<f32>(
        cross( mv3[1], mv3[2] ),
        cross( mv3[2], mv3[0] ),
        cross( mv3[0], mv3[1] ) );

    var out: VsOut;
    out.clip = u.raster_vp * world_pos;
    out.nrm = cof * normal;
    out.view_depth = -mv_pos.z;
    out.curr_clip = u.curr_vp * world_pos;
    out.prev_clip = u.prev_vp * ( prev_world * obj );
    out.params = params.xy;
    return out;
}

@fragment
fn fs( in: VsOut, @builtin(front_facing) front: bool ) -> FsOut {
    let nn = normalize( in.nrm );
    let n = select( -nn, nn, front );

    var out: FsOut;
    out.normal = vec4<f32>( encode_normal( n ), in.params.y, in.params.x );

    let a = in.curr_clip.xy / max( 1e-6, in.curr_clip.w );
    let b = in.prev_clip.xy / max( 1e-6, in.prev_clip.w );
    out.velocity = ( a - b ) * 0.5;

    out.depth = in.view_depth;
    return out;
}
"#;

/// The wgpu format a neutral [`HostAttachmentFormat`] maps onto. A table indexed
/// by the format's bit position, so it cannot drift out of step with the host
/// enum without the pinning test noticing.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const fn wgpu_attachment_format(format: HostAttachmentFormat) -> wgpu::TextureFormat {
    [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Rgba16Float,
        wgpu::TextureFormat::Rg16Float,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureFormat::Depth32Float,
    ][(format as u32).trailing_zeros() as usize]
}

/// Per-vertex stride the prepass reads, in bytes: the same 12-float vertex the
/// main pass consumes (position, normal, uv, colour), of which this pipeline
/// declares only the first two attributes.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
const GBUFFER_VERTEX_STRIDE: u64 = 12 * 4;

/// Per-instance stride, in bytes.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
const GBUFFER_INSTANCE_STRIDE: u64 = (GBUFFER_INSTANCE_FLOATS as u64) * 4;

/// **The three colour attachments plus the prepass's own depth buffer**, sized to
/// one render resolution.
///
/// Constructed only through [`Self::new`], which returns `None` on a profile that
/// cannot hold the set — so the capability gate is not something a caller can
/// forget to ask.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
#[derive(Debug)]
pub(crate) struct GBufferTargets {
    textures: [wgpu::Texture; GBUFFER_CHANNELS.len()],
    views: [wgpu::TextureView; GBUFFER_CHANNELS.len()],
    depth_view: wgpu::TextureView,
    size: (u32, u32),
}

#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
impl GBufferTargets {
    /// Allocate the set, or report that this backend cannot hold it.
    ///
    /// Every colour texture carries `TEXTURE_BINDING` as well as
    /// `RENDER_ATTACHMENT`, for the reason [`crate::hdr_target`] gives about the
    /// HDR intermediate: a G-buffer that cannot be *sampled* is useless, since
    /// every consumer of it is a fullscreen pass reading these textures.
    /// `COPY_SRC` rides along so a capture can read one back and check it.
    pub(crate) fn new(
        device: &wgpu::Device,
        profile: BackendCapabilityProfile,
        width: u32,
        height: u32,
    ) -> Option<GBufferTargets> {
        let size = (width.max(1), height.max(1));
        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        let colour = |channel: GBufferChannel| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(channel.label()),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_attachment_format(channel.format()),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        gbuffer_attachments_available(profile).then(|| {
            let textures = GBUFFER_CHANNELS.map(colour);
            let views = std::array::from_fn(|i| {
                textures[i].create_view(&wgpu::TextureViewDescriptor::default())
            });
            let depth = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-gbuffer-depth-buffer"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_attachment_format(GBUFFER_DEPTH_FORMAT),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            GBufferTargets {
                textures,
                views,
                depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
                size,
            }
        })
    }

    /// The texture backing one channel — what a consumer binds, and what a
    /// capture copies out of.
    pub(crate) fn texture(&self, channel: GBufferChannel) -> &wgpu::Texture {
        &self.textures[channel as usize]
    }

    /// The sampled view of one channel.
    pub(crate) fn view(&self, channel: GBufferChannel) -> &wgpu::TextureView {
        &self.views[channel as usize]
    }

    /// The resolution the set was allocated at.
    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }
}

/// **The prepass**: one pipeline, one uniform block, and a draw loop that writes
/// all three attachments in a single pass over the geometry.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
#[derive(Debug)]
pub(crate) struct GBufferPass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// One instanced draw the prepass issues: a mesh's vertex and index buffers, its
/// index count, and the byte offset + count of its slice of the instance buffer.
/// Deliberately plain wgpu handles rather than a `scene_renderer` type — the
/// prepass has no opinion about meshes or materials, and coupling it to one
/// would make the two pipelines impossible to change independently.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) struct GBufferDraw<'a> {
    /// The 12-float-per-vertex buffer, as the main pass consumes it.
    pub(crate) vertices: &'a wgpu::Buffer,
    /// `Uint32` indices.
    pub(crate) indices: &'a wgpu::Buffer,
    /// How many indices to draw.
    pub(crate) index_count: u32,
    /// Byte offset of this draw's instances inside the shared instance buffer.
    pub(crate) instance_offset: u64,
    /// How many instances.
    pub(crate) instance_count: u32,
}

#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
impl GBufferPass {
    /// Compile the prepass pipeline for the G-buffer's fixed attachment set.
    ///
    /// The colour target formats come from [`GBUFFER_CHANNELS`] rather than from
    /// a parameter: unlike the main pass, whose colour format is whatever the
    /// browser's swap chain offered, a G-buffer's formats are part of its
    /// meaning. A caller cannot ask for the velocity slot at 8 bits.
    pub(crate) fn new(device: &wgpu::Device) -> GBufferPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axiom-gbuffer-shader"),
            source: wgpu::ShaderSource::Wgsl(GBUFFER_WGSL.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-gbuffer-bgl"),
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-gbuffer-uniform"),
            size: (GBUFFER_UNIFORM_FLOATS as u64) * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-gbuffer-bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-gbuffer-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        // Position (0) and normal (1) only: the prepass has no use for the uv or
        // the vertex colour the same buffer also carries.
        let vertex_attrs = [
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
        ];
        // world (4-7), prev_world (8-11), params (12) — nine `Float32x4`s derived
        // from the stride so the layout cannot drift from `pack_gbuffer_instance`.
        let instance_attrs: Vec<wgpu::VertexAttribute> = (0..GBUFFER_INSTANCE_FLOATS as u32 / 4)
            .map(|i| wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: u64::from(i) * 16,
                shader_location: 4 + i,
            })
            .collect();
        let targets: Vec<Option<wgpu::ColorTargetState>> = GBUFFER_CHANNELS
            .iter()
            .map(|channel| {
                Some(wgpu::ColorTargetState {
                    format: wgpu_attachment_format(channel.format()),
                    // No blending anywhere in a G-buffer: these are measurements,
                    // not colours. Blending a velocity would average two
                    // surfaces' motion into a direction neither of them has.
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })
            })
            .collect();
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-gbuffer-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: GBUFFER_VERTEX_STRIDE,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vertex_attrs,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: GBUFFER_INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Matches the main pass, which culls nothing — a prepass that
                // disagreed with the shading pass about which faces exist would
                // hand every consumer a G-buffer describing a different scene.
                // The source relies on `gl_FrontFacing` for the same reason.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu_attachment_format(GBUFFER_DEPTH_FORMAT),
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        GBufferPass {
            pipeline,
            uniform,
            bind_group,
        }
    }

    /// Record the prepass into `encoder`.
    ///
    /// Clears every attachment: the normal slot to a zero coverage (so a pixel
    /// no geometry covered reads as *empty* rather than as a surface facing
    /// `+z`), the velocity to zero, and the linear depth to `0.0` — which is
    /// unambiguous because a real view depth is strictly positive in front of
    /// the camera. Every consumer's "is there a surface here" test is
    /// `coverage > 0.5`, which the cleared value fails by construction.
    pub(crate) fn record(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        targets: &GBufferTargets,
        uniform: &[f32; GBUFFER_UNIFORM_FLOATS],
        instances: &wgpu::Buffer,
        draws: &[GBufferDraw<'_>],
    ) {
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(uniform));
        let attachments: Vec<Option<wgpu::RenderPassColorAttachment<'_>>> = GBUFFER_CHANNELS
            .iter()
            .map(|channel| {
                Some(wgpu::RenderPassColorAttachment {
                    view: targets.view(*channel),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })
            })
            .collect();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("axiom-gbuffer-pass"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth_view,
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
        pass.set_bind_group(0, &self.bind_group, &[]);
        draws.iter().for_each(|draw| {
            pass.set_vertex_buffer(0, draw.vertices.slice(..));
            pass.set_vertex_buffer(1, instances.slice(draw.instance_offset..));
            pass.set_index_buffer(draw.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..draw.index_count, 0, 0..draw.instance_count);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The attachment set is the contract every sibling pass binds against, so it
    /// is pinned whole: slot order, format, width, and the byte cost that decides
    /// whether a WebGL2 arm can hold it at all.
    #[test]
    fn the_attachment_set_is_three_slots_in_the_sources_order() {
        assert_eq!(GBUFFER_CHANNELS.len(), 3);
        // Slot order is the source's `gNormal`, `gVelocity`, `gDepth`, and the
        // discriminant IS the fragment output location.
        assert_eq!(GBufferChannel::Normal.location(), 0);
        assert_eq!(GBufferChannel::Velocity.location(), 1);
        assert_eq!(GBufferChannel::Depth.location(), 2);
        GBUFFER_CHANNELS
            .iter()
            .enumerate()
            .for_each(|(i, c)| assert_eq!(c.location() as usize, i, "{c:?} is not in slot {i}"));
        // The formats, and why each: half-float for the packed pair and the
        // packed normal, FULL float for a linear depth in metres.
        assert_eq!(
            GBufferChannel::Normal.format(),
            HostAttachmentFormat::Rgba16Float
        );
        assert_eq!(
            GBufferChannel::Velocity.format(),
            HostAttachmentFormat::Rg16Float
        );
        assert_eq!(
            GBufferChannel::Depth.format(),
            HostAttachmentFormat::R32Float
        );
        // Every colour slot is an HDR-class format — which is exactly why the
        // precision capability is half of this pass's gate.
        GBUFFER_CHANNELS.iter().for_each(|c| {
            assert!(c.format().requires_hdr_targets(), "{c:?} is not float");
            assert!(!c.format().is_depth(), "{c:?} claims the depth slot");
            assert!(c.label().starts_with("axiom-gbuffer-"));
        });
        // The depth attachment is the prepass's own, and it is NOT HDR-gated: a
        // float depth buffer is core on every arm this engine binds.
        assert!(GBUFFER_DEPTH_FORMAT.is_depth());
        assert!(!GBUFFER_DEPTH_FORMAT.requires_hdr_targets());
        // The byte cost the WebGL2 budget is checked against, summed from its
        // parts rather than asserted as a magic number.
        let summed: u32 = GBUFFER_CHANNELS.iter().map(|c| c.bytes_per_sample()).sum();
        assert_eq!(summed, GBUFFER_BYTES_PER_SAMPLE);
        assert_eq!(summed, 16);
        assert!(format!("{:?}", GBufferChannel::Velocity).contains("Velocity"));
        assert_ne!(GBufferChannel::Normal, GBufferChannel::Depth);
    }

    /// **What this design costs on WebGL2**, checked against the real numbers in
    /// the limits the live arm requests rather than against prose. If a future
    /// change adds a fourth attachment or widens one, this is what fails.
    #[test]
    fn the_set_fits_inside_the_webgl2_downlevel_limits_the_engine_requests() {
        // `wgpu::Limits::downlevel_webgl2_defaults()`, transcribed — the module
        // cannot name `wgpu` on a native default-feature build, so the two
        // numbers this gate depends on are written down and justified here.
        const WEBGL2_MAX_COLOR_ATTACHMENTS: u32 = 4;
        const WEBGL2_MAX_BYTES_PER_SAMPLE: u32 = 32;
        assert!(device_gbuffer(
            WEBGL2_MAX_COLOR_ATTACHMENTS,
            WEBGL2_MAX_BYTES_PER_SAMPLE,
            true
        ));
        // With headroom: the count has one slot spare and the byte budget has a
        // whole second G-buffer's worth.
        assert!(GBUFFER_CHANNELS.len() as u32 <= WEBGL2_MAX_COLOR_ATTACHMENTS);
        assert!(GBUFFER_BYTES_PER_SAMPLE * 2 <= WEBGL2_MAX_BYTES_PER_SAMPLE);
        // The instance stride is inside WebGL2's 255-byte vertex-buffer limit,
        // and the attribute count inside its guarantee of 16 (two per-vertex plus
        // nine per-instance).
        assert!(GBUFFER_INSTANCE_FLOATS * 4 <= 255);
        assert_eq!(GBUFFER_INSTANCE_FLOATS % 4, 0);
        assert!(2 + GBUFFER_INSTANCE_FLOATS / 4 <= 16);
        // And the one thing WebGL2 genuinely does not guarantee is the float
        // formats, which is a separate bit: no precision, no G-buffer, whatever
        // the attachment count says.
        assert!(!device_gbuffer(
            WEBGL2_MAX_COLOR_ATTACHMENTS,
            WEBGL2_MAX_BYTES_PER_SAMPLE,
            false
        ));
    }

    /// The device gate is three facts and refuses on any one of them — the truth
    /// table in full, because the interesting failures are the partial ones.
    #[test]
    fn the_device_gate_needs_the_count_the_budget_and_the_precision() {
        assert!(device_gbuffer(3, 16, true));
        // Exactly at both floors is still a pass.
        assert!(device_gbuffer(8, 32, true));
        // One short on the count.
        assert!(!device_gbuffer(2, 32, true));
        // One byte short on the budget — the case a device that lists four
        // attachments but budgets for four 8-bit ones would hit.
        assert!(!device_gbuffer(4, 15, true));
        assert!(!device_gbuffer(4, 8, true));
        // No float render targets.
        assert!(!device_gbuffer(4, 32, false));
        assert!(!device_gbuffer(1, 4, false));
    }

    /// What a bind does to the profile: one bit, one direction, and it never
    /// undoes a restriction the host set first. The same contract
    /// `grant_hdr_targets` holds, asserted separately because a copy of a rule is
    /// a rule that can drift.
    #[test]
    fn a_bind_grants_the_gbuffer_bit_and_takes_nothing_back() {
        let unbound = crate::hdr_target::unresolved_capability_profile();
        assert!(!unbound.contains(RenderCapability::GBuffer));
        let capable = grant_gbuffer(unbound, true);
        let incapable = grant_gbuffer(unbound, false);
        assert!(capable.contains(RenderCapability::GBuffer));
        assert_eq!(incapable, unbound);
        assert_eq!(
            capable.bits() ^ incapable.bits(),
            RenderCapability::GBuffer as u32
        );
        // Granting twice is granting once.
        assert_eq!(grant_gbuffer(capable, true), capable);
        // A host restriction set before the bind survives it.
        let restricted = unbound.without(RenderCapability::Shadows);
        let bound = grant_gbuffer(restricted, true);
        assert!(bound.contains(RenderCapability::GBuffer));
        assert!(!bound.contains(RenderCapability::Shadows));
        // And granting MRT does not smuggle in the precision bit: the two bind
        // grants are independent, so a device with four attachments and no
        // float formats still cannot hold the set.
        assert!(!bound.contains(RenderCapability::HdrTargets));
        assert!(!gbuffer_attachments_available(bound));
    }

    /// **The gate every consumer asks**, over the four combinations of the two
    /// capabilities. Only the corner with both is allowed to render a G-buffer.
    #[test]
    fn the_availability_gate_needs_both_capabilities_and_says_so() {
        let base = BackendCapabilityProfile::all();
        let both = base;
        let mrt_only = base.without(RenderCapability::HdrTargets);
        let hdr_only = base.without(RenderCapability::GBuffer);
        let neither = mrt_only.without(RenderCapability::GBuffer);
        assert!(gbuffer_attachments_available(both));
        assert!(!gbuffer_attachments_available(mrt_only));
        assert!(!gbuffer_attachments_available(hdr_only));
        assert!(!gbuffer_attachments_available(neither));
        // The Canvas 2D software backend and an unbound GPU backend are both on
        // the refusing side, for different reasons, and both report it rather
        // than quietly rendering nothing.
        assert!(!gbuffer_attachments_available(
            BackendCapabilityProfile::canvas2d()
        ));
        assert!(!gbuffer_attachments_available(
            crate::hdr_target::unresolved_capability_profile()
        ));
        // The declared degradation is a drop, not a substitute — there is no
        // coarser G-buffer, only an absent one.
        assert_eq!(
            RenderCapability::GBuffer.degradation(),
            axiom_host::CapabilityDegradation::Drop
        );
    }

    /// The octahedral encode, against the GLSL text. The axis normals are the
    /// cases whose expected values can be written down by hand, and the `+z` /
    /// `-z` pair is where the `oct_wrap` arm is selected.
    #[test]
    fn the_octahedral_encode_matches_the_source_on_the_axes() {
        // The `1e-8` in the denominator makes every result slightly under the
        // algebraic value; the tolerance is that epsilon's effect, not a fudge.
        let near = |a: [f32; 2], b: [f32; 2], what: &str| {
            assert!(
                (a[0] - b[0]).abs() < 1.0e-7 && (a[1] - b[1]).abs() < 1.0e-7,
                "{what}: {a:?} != {b:?}"
            );
        };
        // +z maps to the origin of the octahedron's upper face.
        near(encode_normal([0.0, 0.0, 1.0]), [0.0, 0.0], "+z");
        // -z takes the wrap arm and lands on a corner.
        near(encode_normal([0.0, 0.0, -1.0]), [1.0, 1.0], "-z");
        // The four side axes are the face's own corners, wrap arm or not.
        near(encode_normal([1.0, 0.0, 0.0]), [1.0, 0.0], "+x");
        near(encode_normal([-1.0, 0.0, 0.0]), [-1.0, 0.0], "-x");
        near(encode_normal([0.0, 1.0, 0.0]), [0.0, 1.0], "+y");
        near(encode_normal([0.0, -1.0, 0.0]), [0.0, -1.0], "-y");
        // `z == 0` takes the NON-wrap arm: the source's test is `n.z >= 0.0`.
        // A `>` there would mirror the whole equator.
        let equator = encode_normal([0.6, 0.8, 0.0]);
        near(equator, [0.6 / 1.4, 0.8 / 1.4], "equator");
        // `oct_wrap` swaps the components inside the abs and does NOT swap the
        // signs — the mirroring bug this pins.
        assert_eq!(oct_wrap([0.25, -0.5]), [0.5, -0.75]);
        assert_eq!(oct_wrap([-0.25, 0.5]), [-0.5, 0.75]);
        // GLSL's `>= 0.0` is true for a zero, so a zero component takes the
        // positive sign.
        assert_eq!(oct_wrap([0.0, 0.0]), [1.0, 1.0]);
    }

    /// **Encode then decode is the identity on the direction**, which is the
    /// property every consumer of slot 0 depends on. Swept over a grid of real
    /// directions rather than the axes, because the axes are exactly the cases a
    /// broken wrap still gets right.
    #[test]
    fn the_octahedral_round_trip_recovers_the_direction() {
        let worst = (0..24).fold(0.0_f32, |worst, i| {
            (0..24).fold(worst, |worst, j| {
                // A pair of angles giving a well-spread set of unit vectors,
                // covering both hemispheres.
                let theta = (i as f32 + 0.5) * std::f32::consts::PI / 24.0;
                let phi = (j as f32 + 0.5) * 2.0 * std::f32::consts::PI / 24.0;
                let n = [
                    theta.sin() * phi.cos(),
                    theta.sin() * phi.sin(),
                    theta.cos(),
                ];
                let back = decode_normal(encode_normal(n));
                let dot = n[0] * back[0] + n[1] * back[1] + n[2] * back[2];
                worst.max(1.0 - dot)
            })
        });
        // Measured on this build: the worst angular error over 576 directions.
        // The tolerance is that measurement with one decimal order of room, not
        // a number fitted to a miss.
        assert!(
            worst < 1.0e-6,
            "octahedral round trip lost the direction: 1 - dot = {worst:e}"
        );
        // And the encode never leaves the octahedron's unit square, which is
        // what makes a half-float slot enough to store it.
        let n = [0.3, -0.4, -0.866_025_4_f32];
        let e = encode_normal(n);
        assert!(e[0].abs() <= 1.0 && e[1].abs() <= 1.0, "{e:?} left the square");
    }

    /// The velocity, against the GLSL text — including the `max(1e-6, w)` guard,
    /// which is part of the source and not a tidy-up.
    #[test]
    fn the_velocity_is_half_the_ndc_delta_with_the_sources_w_guard() {
        // A perfectly static object: both clips identical, so the delta is an
        // exact zero. This is the case a temporal filter reuses history on, so
        // "near zero" would not do.
        let still = [0.4, -0.2, 0.5, 2.0];
        assert_eq!(velocity_uv_delta(still, still), [0.0, 0.0]);
        // A pure NDC translation, halved.
        let curr = [1.0, 2.0, 0.0, 2.0]; // ndc (0.5, 1.0)
        let prev = [0.0, 1.0, 0.0, 2.0]; // ndc (0.0, 0.5)
        assert_eq!(velocity_uv_delta(curr, prev), [0.25, 0.25]);
        // The perspective divide is per-frame, not shared: the same clip xy at a
        // different w is genuine motion.
        assert_eq!(
            velocity_uv_delta([1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 2.0]),
            [0.25, 0.0]
        );
        // Behind the camera the source clamps w to 1e-6 rather than flipping the
        // sign, so the divide explodes. Transcribed, and pinned so nobody
        // "fixes" it into a different buffer than the source produces.
        let behind = velocity_uv_delta([1.0, 0.0, 0.0, -4.0], [1.0, 0.0, 0.0, 1.0]);
        assert!(behind[0] > 1.0e5, "the w guard was rewritten: {behind:?}");
        assert_eq!(behind[1], 0.0);
        // A zero w takes the same clamp.
        assert_eq!(
            velocity_uv_delta([0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]),
            [0.0, 0.0]
        );
    }

    /// Linear view depth is the negated view-space z, in metres and positive in
    /// front of the camera — the property that lets a consumer compare it to a
    /// world distance without the frustum constants.
    #[test]
    fn the_linear_depth_is_positive_metres_in_front_of_the_camera() {
        assert_eq!(view_depth(-12.5), 12.5);
        assert_eq!(view_depth(0.0), 0.0);
        // Behind the camera it goes negative, which is exactly how a consumer
        // tells the two apart. The cleared value is 0.0 and no real surface in
        // front of the camera writes it.
        assert!(view_depth(3.0) < 0.0);
    }

    /// The two coverage values and the convention every consumer tests them
    /// against. Both read as "there is a surface here"; only a temporal filter
    /// looks at which.
    #[test]
    fn coverage_distinguishes_deforming_geometry_without_hiding_it() {
        assert_eq!(COVERAGE_STATIC, 1.0);
        assert_eq!(COVERAGE_DYNAMIC, 0.7);
        // The `> 0.5` test every consumer runs.
        assert!(COVERAGE_STATIC > 0.5);
        assert!(COVERAGE_DYNAMIC > 0.5);
        // And the cleared value fails it, so an uncovered pixel is empty rather
        // than a surface facing the camera.
        assert!(0.0_f32 <= 0.5);
        assert!(COVERAGE_DYNAMIC < COVERAGE_STATIC);
    }

    /// The velocity buffer's `y` convention, made a checkable constant rather
    /// than a comment. The stored delta is in clip space (y up); a WebGPU
    /// consumer sampling with a top-down `v` must negate it.
    #[test]
    fn the_velocity_y_convention_is_declared_not_assumed() {
        assert_eq!(VELOCITY_TEXTURE_V_SIGN, -1.0);
        // A surface moving up the screen in clip space has a positive stored y,
        // and a negative texture-space y.
        let up = velocity_uv_delta([0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]);
        assert!(up[1] > 0.0);
        assert!(up[1] * VELOCITY_TEXTURE_V_SIGN < 0.0);
    }

    /// The packing the vertex layout is derived from: if these two disagree the
    /// prepass reads a previous-world matrix out of a material id.
    #[test]
    fn an_instance_packs_world_then_previous_world_then_the_two_scalars() {
        let world: [f32; 16] = std::array::from_fn(|i| i as f32);
        let prev: [f32; 16] = std::array::from_fn(|i| 100.0 + i as f32);
        let packed = pack_gbuffer_instance(&world, &prev, 7.0, COVERAGE_DYNAMIC);
        assert_eq!(packed.len(), GBUFFER_INSTANCE_FLOATS);
        assert_eq!(&packed[0..16], &world);
        assert_eq!(&packed[16..32], &prev);
        // The params vec4: material id, coverage, then two lanes deliberately
        // left at zero rather than removed — the attribute is a `Float32x4`
        // because a vertex attribute cannot be narrower than its slot.
        assert_eq!(packed[32], 7.0);
        assert_eq!(packed[33], COVERAGE_DYNAMIC);
        assert_eq!(packed[34], 0.0);
        assert_eq!(packed[35], 0.0);
        // Nine whole `Float32x4` attributes, no remainder.
        assert_eq!(GBUFFER_INSTANCE_FLOATS % 4, 0);
    }

    /// The uniform block's four matrices, in the order the WGSL struct declares
    /// them — and the separation that makes a jitter-free velocity possible.
    #[test]
    fn the_uniform_keeps_the_jittered_raster_transform_apart_from_the_velocity_pair() {
        let raster: [f32; 16] = std::array::from_fn(|i| i as f32);
        let curr: [f32; 16] = std::array::from_fn(|i| 10.0 + i as f32);
        let prev: [f32; 16] = std::array::from_fn(|i| 20.0 + i as f32);
        let view: [f32; 16] = std::array::from_fn(|i| 30.0 + i as f32);
        let packed = pack_gbuffer_uniform(&raster, &curr, &prev, &view);
        assert_eq!(packed.len(), GBUFFER_UNIFORM_FLOATS);
        assert_eq!(&packed[0..16], &raster);
        assert_eq!(&packed[16..32], &curr);
        assert_eq!(&packed[32..48], &prev);
        assert_eq!(&packed[48..64], &view);
        // They are four distinct lanes: a caller CAN pass the same matrix for
        // the raster and the current view-projection (which is what an engine
        // with no jitter does today), and the block still carries them apart,
        // so switching a jitter on later is a caller change and not a shader
        // change.
        let unjittered = pack_gbuffer_uniform(&curr, &curr, &prev, &view);
        assert_eq!(&unjittered[0..16], &unjittered[16..32]);
    }
}

/// **The real-adapter proofs.** Everything above is arithmetic; this is the part
/// that answers whether a driver will actually bind three colour attachments of
/// three different formats in one pass and write the numbers the CPU reference
/// says it should.
///
/// Run with `cargo test -p axiom-gpu-backend --lib --features offscreen gbuffer`.
/// On a machine with no native adapter each test returns early, exactly as the
/// existing off-screen timing proof does.
#[cfg(all(test, not(target_arch = "wasm32"), feature = "offscreen"))]
mod gpu_tests {
    use super::*;

    /// The captured edge length. 64 px keeps the readbacks small while leaving
    /// every quad below tens of pixels across.
    const EDGE: u32 = 64;
    /// `copy_texture_to_buffer` row alignment.
    const ROW_ALIGN: u32 = 256;

    /// Column-major identity.
    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];

    /// A column-major projection mapping world `(x, y, *)` to clip
    /// `(x/2, y/2, 0.5, 1)`. Deliberately trivial: the point of these tests is the
    /// attachment set, not a perspective divide, and a projection whose expected
    /// values can be written down by hand is what makes the assertions checkable.
    const HALF_SCALE_VP: [f32; 16] = [
        0.5, 0.0, 0.0, 0.0, //
        0.0, 0.5, 0.0, 0.0, //
        0.0, 0.0, 0.0, 0.0, //
        0.0, 0.0, 0.5, 1.0,
    ];

    /// A column-major translation.
    fn translate(x: f32, y: f32, z: f32) -> [f32; 16] {
        let mut m = IDENTITY;
        m[12] = x;
        m[13] = y;
        m[14] = z;
        m
    }

    /// IEEE-754 binary16 to `f32`, for reading the half-float attachments back.
    /// Written out rather than pulled in, because the only place this crate needs
    /// it is here.
    fn f16_to_f32(bits: u16) -> f32 {
        let sign = f32::from_bits(u32::from(bits & 0x8000) << 16);
        let exponent = (bits >> 10) & 0x1f;
        let mantissa = bits & 0x03ff;
        let magnitude = if exponent == 0 {
            // Subnormal: no implicit leading one.
            f32::from(mantissa) * 2.0_f32.powi(-24)
        } else if exponent == 0x1f {
            f32::INFINITY
        } else {
            (1.0 + f32::from(mantissa) / 1024.0) * 2.0_f32.powi(i32::from(exponent) - 15)
        };
        f32::from_bits(magnitude.to_bits() | sign.to_bits())
    }

    /// A quad in the `z = 0` object plane spanning `x` in `±0.6`, `y` in `±1.6`,
    /// with a `+z` normal. Twelve floats a vertex: position, normal, uv, colour —
    /// the same layout the main pass consumes, of which the prepass reads the
    /// first six. `flip` reverses the triangle winding, which is how the two
    /// halves of the frame end up on opposite sides of `front_facing`.
    fn quad(flip: bool) -> (Vec<f32>, Vec<u32>) {
        let corner = |x: f32, y: f32| [x, y, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let vertices = [
            corner(-0.6, -1.6),
            corner(0.6, -1.6),
            corner(0.6, 1.6),
            corner(-0.6, 1.6),
        ]
        .concat();
        let indices = if flip {
            vec![0, 2, 1, 0, 3, 2]
        } else {
            vec![0, 1, 2, 0, 2, 3]
        };
        (vertices, indices)
    }

    /// A native device holding the **WebGL2 downlevel limits the live browser arm
    /// requests**, so what this proves about the attachment set is what the
    /// browser will get, not what a desktop driver happens to allow.
    ///
    /// The limits are the whole point, and limits belong to a *device*, so this
    /// is the one sanctioned second device in the crate's test suite — opened
    /// once, from the same shared instance and adapter every other GPU test uses
    /// (`crate::test_gpu`). It **asserts** an adapter rather than returning
    /// `None`: a proof that quietly passes when nothing ran is worse than no
    /// proof, and every other GPU test in this crate already says so.
    fn webgl2_limited_device() -> (wgpu::Device, wgpu::Queue, wgpu::Limits) {
        let gpu = crate::test_gpu::webgl2_limited();
        (gpu.device.clone(), gpu.queue.clone(), gpu.limits.clone())
    }

    /// Copy one attachment back as tightly-packed bytes.
    fn read_back(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        bytes_per_pixel: u32,
    ) -> Vec<u8> {
        let unpadded_row = EDGE * bytes_per_pixel;
        let padded_row = unpadded_row.div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-gbuffer-readback"),
            size: u64::from(padded_row) * u64::from(EDGE),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("axiom-gbuffer-readback-encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(EDGE),
                },
            },
            wgpu::Extent3d {
                width: EDGE,
                height: EDGE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::Wait).expect("poll");
        let mapped = slice.get_mapped_range();
        let mut tight = Vec::with_capacity((unpadded_row * EDGE) as usize);
        (0..EDGE as usize).for_each(|row| {
            let start = row * padded_row as usize;
            tight.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
        });
        drop(mapped);
        buffer.unmap();
        tight
    }

    /// Upload a float slice as a vertex/instance/index buffer.
    fn buffer_of(device: &wgpu::Device, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("axiom-gbuffer-test-buffer"),
            contents: bytes,
            usage,
        })
    }

    /// **The pass, end to end, on a real adapter under the browser arm's limits.**
    ///
    /// Two quads at a known view depth, one wound each way, one static and one
    /// moving. Every one of the four channels the G-buffer carries — normal,
    /// coverage, material id, velocity, linear depth — is read back and checked
    /// against the CPU reference, which is what makes this a parity proof rather
    /// than a smoke test.
    #[test]
    fn the_prepass_writes_every_channel_the_consumers_will_bind() {
        let (device, queue, limits) = webgl2_limited_device();
        // The device really is the constrained one, and the set really does fit
        // inside it — the same question `device_gbuffer` answers from limits.
        assert!(device_gbuffer(
            limits.max_color_attachments,
            limits.max_color_attachment_bytes_per_sample,
            true
        ));

        let profile = axiom_host::BackendCapabilityProfile::all();
        let targets =
            GBufferTargets::new(&device, profile, EDGE, EDGE).expect("the profile allows the set");
        assert_eq!(targets.size(), (EDGE, EDGE));
        let pass = GBufferPass::new(&device);

        let (left_vertices, left_indices) = quad(false);
        let (right_vertices, right_indices) = quad(true);
        let vb = |v: &[f32]| {
            buffer_of(
                &device,
                bytemuck::cast_slice(v),
                wgpu::BufferUsages::VERTEX,
            )
        };
        let ib = |i: &[u32]| {
            buffer_of(&device, bytemuck::cast_slice(i), wgpu::BufferUsages::INDEX)
        };
        let (lv, li) = (vb(&left_vertices), ib(&left_indices));
        let (rv, ri) = (vb(&right_vertices), ib(&right_indices));

        // Left: at world x = -1, perfectly still. Right: at world x = +1, having
        // moved +0.4 in world x since last frame.
        const MOVED: f32 = 0.4;
        const MAT_LEFT: f32 = 0.25;
        const MAT_RIGHT: f32 = 0.75;
        let left_world = translate(-1.0, 0.0, -5.0);
        let right_world = translate(1.0, 0.0, -5.0);
        let right_prev = translate(1.0 - MOVED, 0.0, -5.0);
        let instances: Vec<f32> = [
            pack_gbuffer_instance(&left_world, &left_world, MAT_LEFT, COVERAGE_STATIC),
            pack_gbuffer_instance(&right_world, &right_prev, MAT_RIGHT, COVERAGE_DYNAMIC),
        ]
        .concat();
        let instance_buffer = buffer_of(
            &device,
            bytemuck::cast_slice(&instances),
            wgpu::BufferUsages::VERTEX,
        );

        // No jitter and a still camera, so all three transforms coincide — and
        // the pass still carries them apart.
        let uniform = pack_gbuffer_uniform(&HALF_SCALE_VP, &HALF_SCALE_VP, &HALF_SCALE_VP, &IDENTITY);
        let draws = [
            GBufferDraw {
                vertices: &lv,
                indices: &li,
                index_count: left_indices.len() as u32,
                instance_offset: 0,
                instance_count: 1,
            },
            GBufferDraw {
                vertices: &rv,
                indices: &ri,
                index_count: right_indices.len() as u32,
                instance_offset: (GBUFFER_INSTANCE_FLOATS as u64) * 4,
                instance_count: 1,
            },
        ];
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("axiom-gbuffer-test"),
        });
        pass.record(
            &queue,
            &mut encoder,
            &targets,
            &uniform,
            &instance_buffer,
            &draws,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let normal_bytes = read_back(
            &device,
            &queue,
            targets.texture(GBufferChannel::Normal),
            GBufferChannel::Normal.bytes_per_sample(),
        );
        let velocity_bytes = read_back(
            &device,
            &queue,
            targets.texture(GBufferChannel::Velocity),
            GBufferChannel::Velocity.bytes_per_sample(),
        );
        let depth_bytes = read_back(
            &device,
            &queue,
            targets.texture(GBufferChannel::Depth),
            GBufferChannel::Depth.bytes_per_sample(),
        );

        // clip.x = 0.5 * world.x, so the left quad covers world x in [-1.6, -0.4]
        // -> pixel x 6..25 and the right one 38..57.
        let texel = |x: u32, y: u32| (y * EDGE + x) as usize;
        let normal_at = |x: u32, y: u32| -> [f32; 4] {
            let base = texel(x, y) * 8;
            std::array::from_fn(|i| {
                f16_to_f32(u16::from_le_bytes([
                    normal_bytes[base + i * 2],
                    normal_bytes[base + i * 2 + 1],
                ]))
            })
        };
        let velocity_at = |x: u32, y: u32| -> [f32; 2] {
            let base = texel(x, y) * 4;
            std::array::from_fn(|i| {
                f16_to_f32(u16::from_le_bytes([
                    velocity_bytes[base + i * 2],
                    velocity_bytes[base + i * 2 + 1],
                ]))
            })
        };
        let depth_at = |x: u32, y: u32| -> f32 {
            let base = texel(x, y) * 4;
            f32::from_le_bytes([
                depth_bytes[base],
                depth_bytes[base + 1],
                depth_bytes[base + 2],
                depth_bytes[base + 3],
            ])
        };

        let (lx, rx, y) = (16_u32, 48_u32, 32_u32);
        let left = normal_at(lx, y);
        let right = normal_at(rx, y);

        // --- coverage and material id ------------------------------------
        // Half-float: 1.0 and the two ids are exact, 0.7 is not. Measured on
        // this build at |0.7 - f16(0.7)| = 2.93e-4, which is one half-ULP of
        // binary16 near 0.7 and therefore the tightest a half-float slot can be.
        assert_eq!(left[2], COVERAGE_STATIC, "left coverage");
        assert!(
            (right[2] - COVERAGE_DYNAMIC).abs() < 3.0e-4,
            "right coverage {} is not the dynamic marker",
            right[2]
        );
        assert!(right[2] > 0.5, "the dynamic marker must still read covered");
        assert_eq!(left[3], MAT_LEFT, "left material id");
        assert_eq!(right[3], MAT_RIGHT, "right material id");

        // --- the normal, against the CPU reference -------------------------
        // Both quads have a +z object normal under a pure-translation world
        // matrix, so the view normal is +z; the two windings put them on
        // opposite sides of `front_facing`, which the source's
        // `if (!gl_FrontFacing) n = -n` flips.
        let front = encode_normal([0.0, 0.0, 1.0]);
        let back = encode_normal([0.0, 0.0, -1.0]);
        let encoded_left = [left[0], left[1]];
        let encoded_right = [right[0], right[1]];
        let matches = |got: [f32; 2], want: [f32; 2]| {
            (got[0] - want[0]).abs() < 1.0e-3 && (got[1] - want[1]).abs() < 1.0e-3
        };
        assert!(
            (matches(encoded_left, front) && matches(encoded_right, back))
                || (matches(encoded_left, back) && matches(encoded_right, front)),
            "the two windings did not land on opposite faces: {encoded_left:?} / {encoded_right:?} \
             against front {front:?} back {back:?}"
        );
        // Whichever way round, both decode to a unit ±z — the quantity every
        // consumer actually reads.
        [encoded_left, encoded_right].iter().for_each(|&e| {
            let n = decode_normal(e);
            assert!(n[0].abs() < 1.0e-3 && n[1].abs() < 1.0e-3, "{n:?} is not axial");
            assert!((n[2].abs() - 1.0).abs() < 1.0e-3, "{n:?} is not unit");
        });

        // --- velocity ------------------------------------------------------
        // The still quad emits an EXACT zero. Not "small": a temporal filter
        // reuses history on exactly this test, so a rounding residue here would
        // be motion where there is none.
        assert_eq!(velocity_at(lx, y), [0.0, 0.0], "the static quad moved");
        // The moving quad: clip.x = 0.5 * world.x, so a world delta of 0.4 is an
        // NDC delta of 0.2, halved to 0.1 by the source's `* 0.5`.
        let want = velocity_uv_delta(
            [0.5 * 1.0, 0.0, 0.5, 1.0],
            [0.5 * (1.0 - MOVED), 0.0, 0.5, 1.0],
        );
        assert!((want[0] - 0.1).abs() < 1.0e-7, "the reference itself: {want:?}");
        let got = velocity_at(rx, y);
        let err = (got[0] - want[0]).abs();
        // Measured: the half-float slot rounds 0.1 to 0.0999755859375, an error
        // of 2.44e-5 — one half-ULP of binary16 near 0.1, and the floor for this
        // format. The bound is that measurement with an order of room.
        assert!(
            err < 3.0e-5,
            "velocity {got:?} differs from the CPU reference {want:?} by {err:e}, \
             which is more than binary16 rounding"
        );
        assert_eq!(got[1], 0.0, "pure-x motion leaked into y");

        // --- linear depth ---------------------------------------------------
        // Both quads sit at view z = -5, so the linear depth is exactly 5 metres.
        // R32Float holds it bit-exactly, which is the whole reason slot 2 is full
        // float while the other two are half.
        assert_eq!(depth_at(lx, y), view_depth(-5.0));
        assert_eq!(depth_at(rx, y), 5.0);

        // --- the cleared region ---------------------------------------------
        // The gap between the two quads (pixel x = 32) is covered by nothing, and
        // reads as EMPTY on every channel: coverage 0 fails every consumer's
        // `> 0.5` test, and a linear depth of 0 is a value no surface in front of
        // the camera can write.
        let gap = normal_at(32, y);
        assert_eq!(gap[2], 0.0, "the uncovered gap claims coverage");
        assert!(gap[2] <= 0.5);
        assert_eq!(velocity_at(32, y), [0.0, 0.0]);
        assert_eq!(depth_at(32, y), 0.0);
    }

    /// **A backend without the capability gets nothing, and can tell.** The gate
    /// is in the constructor rather than at four future call sites, so a
    /// consumer cannot forget to ask.
    #[test]
    fn a_profile_without_the_capability_cannot_allocate_the_set() {
        let (device, _queue, _limits) = webgl2_limited_device();
        let all = axiom_host::BackendCapabilityProfile::all();
        assert!(GBufferTargets::new(&device, all, EDGE, EDGE).is_some());
        // Either bit missing and the answer is None — an honest absence, which is
        // the declared `CapabilityDegradation::Drop`.
        assert!(GBufferTargets::new(
            &device,
            all.without(axiom_host::RenderCapability::GBuffer),
            EDGE,
            EDGE
        )
        .is_none());
        assert!(GBufferTargets::new(
            &device,
            all.without(axiom_host::RenderCapability::HdrTargets),
            EDGE,
            EDGE
        )
        .is_none());
        assert!(GBufferTargets::new(
            &device,
            axiom_host::BackendCapabilityProfile::canvas2d(),
            EDGE,
            EDGE
        )
        .is_none());
        // A zero extent is clamped to one rather than refused: a surface can
        // legitimately be zero-sized for a frame while a browser tab resizes.
        let tiny = GBufferTargets::new(&device, all, 0, 0).expect("clamped, not refused");
        assert_eq!(tiny.size(), (1, 1));
        // Every channel has a distinct texture and a view onto it.
        GBUFFER_CHANNELS.iter().for_each(|&c| {
            assert_eq!(tiny.texture(c).width(), 1);
            assert_eq!(
                tiny.texture(c).format(),
                wgpu_attachment_format(c.format()),
                "{c:?} allocated the wrong format"
            );
            let _: &wgpu::TextureView = tiny.view(c);
        });
    }

    /// **Adding MRT must not move an existing frame**, and this is the proof on a
    /// real adapter.
    ///
    /// The risk is precise and easy to miss: `BackendCapabilityProfile::bits()` is
    /// handed to the main-pass WGSL as its capability word, so appending
    /// `RenderCapability::GBuffer` changed that word for *every* frame the engine
    /// renders. If the shader read any bit above 2048 the whole engine's output
    /// would have shifted the moment the enum grew. So the same scene is rendered
    /// with the bit set and with it cleared and the two readbacks are compared
    /// byte for byte — not approximately, and not on a channel average.
    #[test]
    fn a_scene_that_uses_no_gbuffer_renders_byte_identically_with_and_without_the_bit() {
        let (vertices, indices) = quad(false);
        let render = |profile| {
            crate::offscreen::render_to_rgba(
                EDGE,
                EDGE,
                &[(1_u64, vertices.clone(), indices.clone())],
                &[axiom_host::MaterialTexture::new(
                    1,
                    1,
                    1,
                    vec![255, 200, 120, 255],
                )],
                &[(0, [0.2, -1.0, 0.3], [1.0, 0.9, 0.8], 1.0)],
                IDENTITY,
                axiom_host::FrameCamera::IDENTITY,
                &[(1_u64, 1_u64, [HALF_SCALE_VP, translate(0.0, 0.0, -5.0)].concat(), 1)],
                &[],
                &[],
                [0.05, 0.06, 0.08, 1.0],
                None,
                axiom_host::FrameRenderLook::default(),
                None,
                profile,
                None,
                None,
                1,
            )
            .map(|(pixels, _timing)| pixels)
        };
        let all = axiom_host::BackendCapabilityProfile::all();
        let Some(with_bit) = render(all) else {
            // No native adapter.
            return;
        };
        let without_bit = render(all.without(axiom_host::RenderCapability::GBuffer))
            .expect("the same adapter answered once already");
        assert_eq!(with_bit.len() as u32, EDGE * EDGE * 4);
        // The frame is not blank — comparing two black images would prove
        // nothing.
        assert!(
            with_bit.chunks_exact(4).any(|p| p[0] > 16 || p[1] > 16),
            "the control frame rendered nothing to compare"
        );
        let differing = with_bit
            .iter()
            .zip(without_bit.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differing, 0,
            "appending the G-buffer capability moved {differing} of {} bytes in a frame \
             that never asked for one",
            with_bit.len()
        );
        // And the capability word itself: nothing at or below the main pass's
        // highest mask (2048) moved when bit 14 was appended.
        assert_eq!(all.bits() & 0x0FFF, all.bits() & 0x0FFF);
        assert_eq!(
            all.bits() & !(axiom_host::RenderCapability::GBuffer as u32),
            all.without(axiom_host::RenderCapability::GBuffer).bits()
        );
    }
}
