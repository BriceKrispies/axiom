//! # Axiom Draw2D — Engine Module
//!
//! The **2D draw-list builder** — the 2D peer of `axiom-render`'s `RenderApi`
//! (SPEC-04). [`Draw2dApi`] compiles author 2D-draw calls (shapes, sprites,
//! text, gradients, a camera + transform stack) into a deterministic,
//! `(layer, submission)`-sorted draw-list, so draw order is submit order with an
//! explicit z-layer reorder.
//!
//! ## What this module is
//! - A pure, native-testable, fully-covered, **branchless** *builder* that
//!   accumulates and layer-sorts the list. It **rasterizes nothing**.
//! - The owner of the authoring ergonomics the neutral contract deliberately
//!   does not carry: the transform stack (push/pop/compose) and the per-frame
//!   submit counter.
//!
//! ## What this module is not
//! - **Not the owner of the draw contract.** `Draw2dList`, `Draw2dCommand`, and
//!   the resolved value vocabulary (`Common2d`, `Fill2d`, `Rgba`, `Rect`,
//!   `SpriteDraw2d`, `GlyphRun`, `TextDraw2d`, gradients, …) are **host-layer**
//!   types (`axiom_host`), relocated there so the render backends that depend on
//!   host can name and rasterize them — the 2D peer of host's `FramePacket`.
//!   The builder assembles those host-owned types and returns
//!   `axiom_host::Draw2dList`; callers `use axiom_host::{…}` for the vocabulary.
//! - **Not a rasterizer.** Turning the list into pixels — software raster on
//!   canvas, `wgpu` on GPU — and the per-draw **alpha-blend** fix are a separate
//!   backend slice; this module touches no backend, no GPU/DOM/font/scene type.
//!   It imports no other module. The app/runtime reads the list out and submits
//!   it to a backend (the single legal home for cross-module translation).
//!
//! ## Presentation class (SPEC-04 §6, §17.5)
//! The whole surface is presentation-class: `onRender` is the only caller and
//! **nothing it produces is authoritative**. The facade exposes no getter that
//! returns draw state into a sim-readable form — there is no read-back path.
//!
//! ## Particles (§10.1) and render targets (§10.3)
//! Both §4.1 follow-ups now land, additively — they extend the builder and the
//! command set without reshaping the host-owned contract:
//! - **Particles** — [`Draw2dApi::create_emitter`] / [`Draw2dApi::emit`] /
//!   [`Draw2dApi::advance_particles`] step a private, presentation-only particle
//!   field on the kernel [`axiom_kernel::Seconds`] *presentation* delta (never a
//!   sim tick) and append each survivor as a `KIND_PARTICLE_QUAD` command. The
//!   field is deterministic as a function of its inputs; it has **no read-back
//!   getter**, so a particle can never feed sim (SPEC-04 §6, §17.5).
//! - **Render targets** — [`Draw2dApi::create_render_target`] /
//!   [`Draw2dApi::begin_target`] / [`Draw2dApi::end_target`] /
//!   [`Draw2dApi::target_texture`] route draws into a named nested
//!   `axiom_host::Draw2dList`; the backend owns the off-screen surface.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`Draw2dApi`] — alongside
//! its identity vocabulary (Module Law #8): the [`EmitterId`] handle and
//! [`EmitterConfig`] recipe the particle methods traffic in (`pub use ids::{…}`).
//! The neutral draw-contract value types still live in the host layer; callers
//! reach them via `use axiom_host::{Draw2dList, Common2d, Fill2d, Rect, Rgba, …}`.

mod draw2d_api;
mod ids;
mod particles;

pub use draw2d_api::Draw2dApi;
pub use ids::{EmitterConfig, EmitterId};
