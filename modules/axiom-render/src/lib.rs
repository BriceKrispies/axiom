//! # Axiom Render — Engine Module
//!
//! Backend-neutral render compilation. `RenderApi` takes a
//! scene-independent [`RenderInput`](crate::render_input::RenderInput)
//! and produces a deterministic
//! [`RenderCommandList`](crate::render_command_list::RenderCommandList).
//!
//! ## What this module is
//! - The translator from a frame's *render-facing* inputs (camera
//!   matrices, light arrays, mesh/material arrays, objects) into a
//!   deterministic command stream.
//! - The owner of `RenderInput`, `RenderCommand`, and
//!   `RenderCommandList`.
//!
//! ## What this module is not
//! Not a renderer with a backend. Not a scene module. Not a resources
//! module. Not a WebGPU module. Not an asset loader. It imports no other
//! module. Its only host-layer coupling is producing the backend-neutral
//! [`axiom_host::FramePacket`] from a command list
//! ([`RenderApi::build_frame_packet`](crate::RenderApi::build_frame_packet));
//! it consumes none of host's presentation/stepping/surface APIs.
//!
//! ## Frame capture boundary
//! [`RenderApi`] can also capture a deterministic, engine-owned
//! [`RenderReceipt`](crate::render_receipt::RenderReceipt) for one frame: the
//! frame identity plus the ordered command stream, serialized and hashable.
//! **This is not pixel capture** — see `render_receipt.rs`.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`RenderApi`].

mod render_api;
mod render_camera;
mod render_command;
mod render_command_list;
mod render_input;
mod render_light;
mod render_material;
mod render_mesh;
mod render_object;
mod render_pipeline_kind;
mod render_receipt;
mod render_sdf;

pub use render_api::RenderApi;
