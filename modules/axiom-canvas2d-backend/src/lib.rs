//! # Axiom Canvas 2D Backend — platform-facing engine module (software fallback)
//!
//! The last-resort browser presentation backend: when neither WebGPU nor WebGL2
//! is available, this renders the scene with a CPU **software z-buffer
//! rasterizer** into a small RGBA framebuffer and blits it to a Canvas 2D
//! context with `putImageData`. It consumes the **same** backend-neutral
//! [`axiom_host::FramePacket`] the GPU backend consumes, so a game authored and
//! tested against the normal render path stays recognizable, playable, and
//! deterministic when it falls all the way back to Canvas 2D — degraded in
//! fidelity (flat-shaded triangles, no textures/shadows/lighting, a low internal
//! resolution) but with the same object identities, transforms, camera framing,
//! draw order, **per-pixel depth ordering**, and clear colour.
//!
//! ## What this module is
//! - A pure, native-testable software **z-buffer rasterizer**: project a
//!   packet's triangles through each draw's `mvp`, rasterize them into a small
//!   RGBA colour buffer with a per-pixel f32 depth buffer (real occlusion), and
//!   hand back the finished framebuffer bytes. Canvas 2D is the *blit target*,
//!   not the renderer.
//! - A thin `wasm32` arm that uploads those bytes to the canvas with
//!   `putImageData` (nearest-neighbour upscaled from the low internal
//!   resolution).
//!
//! ## What this module is not
//! Not a renderer that knows about scenes/resources/meshes by name, not the run
//! loop, not a GPU backend. It takes uploaded mesh geometry plus a frame packet
//! and produces pixels.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`Canvas2dBackendApi`].

mod canvas2d_backend_api;

// Backend-owned presentation policy (visual profile, debug overlay, terrain
// importance). Pure data, no browser types.
mod canvas_policy;

// The pure, native-testable software rasterizer pipeline:
//   FramePacket --frame_packet_raster--> RasterTriangle[] (projected, LOD'd)
//             --software_rasterizer--> SoftwareFramebuffer (RGBA) + DepthBuffer
// plus the CPU mesh cache it reads geometry from and the projection math.
mod canvas_depth_cue;
mod canvas_depth_cue_profile;
mod canvas_post_pass;
mod depth_buffer;
mod frame_packet_raster;
mod low_poly_raster_options;
mod mesh_cache;
mod planar_shadow;
mod projection;
mod raster_triangle;
mod raster_vertex;
mod sdf_raymarch;
mod software_framebuffer;
mod software_raster_result;
mod software_rasterizer;

// The host-neutral 2D draw-list (`host::Draw2dList`) software consumer:
// composites the layer-sorted 2D commands onto a framebuffer with src-over
// alpha blending — the 2D peer of the FramePacket raster.
mod draw2d_raster;

// The real Canvas 2D presentation arm — compiled only for wasm32, behind the
// facade. A thin `putImageData` blit of the rasterizer's RGBA bytes.
#[cfg(target_arch = "wasm32")]
mod live_canvas_binding;

pub use canvas2d_backend_api::Canvas2dBackendApi;
