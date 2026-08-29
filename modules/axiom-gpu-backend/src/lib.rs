//! # Axiom GPU Backend — platform-facing engine module (the real wgpu executor)
//!
//! The impure half of presentation: the part that actually owns the browser's
//! `wgpu` device, pipeline, and buffers and draws real pixels. It is constructed
//! from a `host`-layer [`axiom_host::HostPresentationRequest`] (so it composes no
//! other module — it consumes nameable host data, not a module contract type) and
//! presents instanced draws. The deterministic *what/when* of presentation stays
//! in `axiom-windowing`, which drives the run loop and delegates each frame's draw
//! to this backend.
//!
//! ## What this module is
//! - The single owner of the real GPU binding (surface/device/pipeline/buffers)
//!   and the per-frame present, plus mid-loop geometry replacement.
//! - The native-testable surface size + readiness + no-op present, with the real
//!   browser-only `wgpu` work compiled in behind the `wasm32` arm.
//!
//! ## What this module is not
//! Not the run loop, not a scene/world, not a renderer that knows about meshes or
//! materials by name. It takes plain engine data (vertex/instance float streams +
//! a clear colour) and a host presentation request, and issues GPU calls.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`GpuBackendApi`].

mod gpu_backend_api;

// Pure, native-testable adapter from host::FramePacket to the live path's
// instance-batch + light shape.
mod frame_packet_adapter;

// What one GPU frame cost, pass by pass: the vocabulary a caller reads and the
// tick -> duration arithmetic behind it. Pure and compiled everywhere, so the
// rules that decide whether a number is real (a pass the frame never ran, an
// adapter that cannot time at all) are measured by the coverage gate rather than
// hidden behind the GPU arm's `cfg`.
mod gpu_pass_timing;

// The real wgpu timestamp query set that feeds it: one query per pass boundary,
// resolved asynchronously so no frame ever blocks for a number. Compiled only
// where a GPU exists, and built only on a device that actually has
// `TIMESTAMP_QUERY` — absent, every pass records exactly what it always did.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod gpu_pass_clock;

// What an authored axiom_surface::Surface means to THIS backend: the program
// plan (stage split, interstage lanes, parameter layout), the uniform parameter
// channel and its offset scheme, and the capability gate that decides — once, at
// preparation time — whether a surface can be lowered at all. Pure and
// native-testable; it contains no shader text, because generating one is
// separate work.
mod surface_program;

// Walks a layer-sorted host::Draw2dList into backend-neutral quad geometry
// (positions, UVs, alpha-folded colours, per-quad texture). Pure and
// branchless — the 2D peer of `frame_packet_adapter`.
mod draw2d_geometry;

// Box-filtered mip reductions of a material texture, averaged in linear light.
// Pure and branchless, so the filtering arithmetic that decides whether a
// receding surface aliases is measured by the coverage gate rather than hidden
// behind the GPU arm's `cfg`.
#[cfg(any(test, target_arch = "wasm32", feature = "offscreen"))]
mod mip_chain;

// Resolves a material's host-authored TextureSampling mode into the concrete
// filters + anisotropy clamp the sampler is built from, bounded by what the
// device reports. Pure and branchless, for the same reason as `mip_chain`.
#[cfg(any(test, target_arch = "wasm32", feature = "offscreen"))]
mod texture_sampling;

// The real wgpu pipeline that draws `draw2d_geometry`'s output, alpha-blended,
// to a wgpu colour target — the 2D peer of `scene_renderer`.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod draw2d_renderer;

// Native off-screen 2D capture entry: renders a Draw2dList's geometry into a
// linear RGBA8 texture and reads it back; drives `axiom-shot` and the SPEC-04
// alpha-blend parity proof.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
mod draw2d_offscreen;

// The deterministic surface-recovery decision (what to do when the GPU surface
// is lost/outdated, as a backgrounded mobile browser does).
#[cfg(any(target_arch = "wasm32", test))]
mod surface_recovery;

// What the bound device can do, as one resolved value rather than a dozen
// scattered adapter reads. See its module docs: this is what makes a frame a
// function of (data, capability profile) again, and therefore reproducible on a
// machine other than the one that rendered it.
mod device_facts;

// The shared, target-agnostic renderer (pipeline + caches + draw).
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod scene_renderer;

// The main pass's WGSL, in the two halves a generated surface program sits
// between. Split out of `scene_renderer` so the shader text and the pipeline
// that compiles it are separately readable, and so the splice point is a
// greppable file rather than a line number in a 2600-line module.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod scene_wgsl;

// Whether this device can hold a high-dynamic-range colour attachment, resolved
// from what the adapter reports rather than asserted from a policy — and the
// capability profile that answer produces. Pure booleans, compiled everywhere and
// covered natively, for the same reason `shadow_cull` is: a rule that decides what
// a frame may render into is impossible to debug from inside a render pass.
mod hdr_target;

// The G-buffer prepass: one geometry pass writing a view normal, a screen-space
// velocity and a linear view depth into three colour attachments at once, plus
// its own depth buffer. The foundation the screen-space passes (ambient
// occlusion, reflections, temporal resolve, motion blur) share. Its attachment
// set, its capability gate and its CPU reference for the octahedral packing are
// pure and compiled everywhere — the rule that decides whether a device can hold
// a G-buffer at all is impossible to debug from inside a render pass — while the
// pipeline and targets sit behind the GPU arm's `cfg`.
mod gbuffer;

// The AgX filmic tone map and the EV100 metering chain that feeds it, ported
// from the reference's `src/render/glsl.js` and `src/render/exposure.js`. WGSL
// text plus a CPU reference that is its semantic definition; pure arithmetic, so
// compiled everywhere and covered natively. Nothing binds them yet — see each
// module's `nothing_in_the_present_path_compiles_this_yet`.
mod agx;
mod exposure;

// The bloom pyramid: `render/bloom.js` as WGSL plus its CPU reference — the
// Jimenez/COD progressive dual filter (13-tap Karis downsample, 9-tap tent
// upsample, blended not summed). The arithmetic and the pyramid's shape are pure
// and compiled everywhere; only the wgpu passes are behind the GPU arms.
mod bloom_pyramid;

// Which draws can actually reach the directional shadow map. Pure geometry over
// plain arrays, compiled everywhere (and covered natively) precisely because the
// rule is impossible to debug from inside a render pass.
mod shadow_cull;

// Cascaded shadow maps: the split scheme, the per-cascade bounding-sphere ortho
// fit, the whole-texel snap and the fragment stage's selection/PCSS reference,
// transcribed from Claude-of-Duty's `render/csm.js` (`4x2048 CSM`).
//
// Nothing binds it yet — the frame contract carries ONE `light_view_proj`, so
// the shipped pass has no four-matrix lane to fill. See
// `cascade::tests::nothing_in_the_shadow_path_compiles_this_yet`.
mod cascade;

// Which attachment performs the linear -> sRGB encode, and the crate's single
// WGSL definition of that curve. Pure format arithmetic: a browser surface may or
// may not offer an sRGB format, and this is where that accident is absorbed so
// exactly one encode reaches the display on every arm.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod surface_encode;

// Upscale-blit pipeline presenting a reduced-resolution render target: the live
// binding's mobile-first render-scale path, and the offscreen retro 32-bit low-res +
// nearest upscale. Available wherever a real GPU renders (wasm32 / offscreen).
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod post_chain;
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod upscale;

// The real wgpu swap-chain binding.
#[cfg(target_arch = "wasm32")]
mod live_gpu_binding;

// The native off-screen renderer. Drives the same `scene_renderer` as the live arm.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
mod offscreen;

// The ONE instance + adapter + device the native headless capture paths share,
// held for the process instead of created and destroyed per capture. Cycling them
// per call cost a full backend enumeration per screenshot AND is what makes this
// machine's driver fall over; see the module docs for the measurement.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
mod native_gpu;

// The ONE instance + adapter + device every GPU test in this crate shares. Not a
// convenience: ~50 tests each opening their own is what makes the offscreen suite
// intermittently crash the driver. Test-only, so it enters no build the engine
// ships. See the module docs for the measurement.
#[cfg(all(test, feature = "offscreen"))]
mod test_gpu;
pub(crate) mod material_shader;

// ---------------------------------------------------------------------------
// The render frame graph — `src/render/` of Claude-of-Duty, 18 passes.
//
// Declared unconditionally. Every one of these is pure Rust plus WGSL held in
// `&str` constants; only their *tests* need an adapter, and those carry their
// own `offscreen` gates. Gating a string on a rendering feature is the mistake
// `material_shader/compose.rs` already had to undo.
//
// `frame_graph` names the others through `FramePass::module_path()` rather than
// `use`, so the ordering below is alphabetical for readability and carries no
// dependency meaning — except `contact`, which imports `ssr`.
// ---------------------------------------------------------------------------
mod contact;
mod dof;
mod env;
mod frame_graph;
mod gtao;
mod indirect_lighting;
mod lut;
mod motionblur;
mod ssr;
mod taa;
mod texture_bake;


pub use gpu_backend_api::GpuBackendApi;
