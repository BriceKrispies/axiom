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
//! ## Deferred in this slice (documented, follow-up)
//! The §4.1 priority core landed: shapes (`rect`/`circle`/`ellipse`/`line`/
//! `path`), `sprite`, `text` + `measure_text`, `linear`/`radial` gradients, the
//! camera + transform stack, and the `(layer, submission)` sort. Two §4.1 items
//! are **deliberately deferred** to keep this slice fully covered and branchless:
//! - **Particles (§10.1)** — needs a kernel `Seconds` dimensioned scalar
//!   (absent today) for the presentation-`dt` step; adding it is a kernel-layer
//!   change beyond this module slice.
//! - **Render targets (§10.3)** — nested off-screen draw-lists / routing.
//!
//! Both are additive: they extend the builder and the command set without
//! reshaping the host-owned contract.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`Draw2dApi`]
//! (Module Law #8). The value-type vocabulary it traffics in lives in the host
//! layer, so there is no `ids` re-export here: callers reach it via
//! `use axiom_host::{Draw2dList, Common2d, Fill2d, Rect, Rgba, …}`.

mod draw2d_api;

pub use draw2d_api::Draw2dApi;
