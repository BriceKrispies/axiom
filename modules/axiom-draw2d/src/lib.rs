//! # Axiom Draw2D — Engine Module
//!
//! The neutral, ordered, hashable **2D draw-list as data** — the 2D peer of
//! `axiom-render`'s `RenderCommandList` (SPEC-04). [`Draw2dApi`] compiles
//! author 2D-draw calls (shapes, sprites, text, gradients, a camera + transform
//! stack) into a deterministic [`Draw2dList`] of `KIND_*`-tagged
//! [`Draw2dCommand`]s, sorted once by `(layer, submission)` so draw order is
//! submit order with an explicit z-layer reorder.
//!
//! ## What this module is
//! - A pure, native-testable, fully-covered, **branchless** core that builds
//!   and layer-sorts the list. It **rasterizes nothing**.
//! - The owner of the neutral 2D contract: [`Draw2dList`], [`Draw2dCommand`],
//!   and the resolved value vocabulary ([`Common2d`], [`Fill2d`], [`Rgba`],
//!   [`Rect`], [`SpriteDraw2d`], [`GlyphRun`], [`TextDraw2d`], gradients, …).
//!
//! ## What this module is not
//! Not a rasterizer. Turning the list into pixels — software raster on canvas,
//! `wgpu` on GPU — and the per-draw **alpha-blend** fix are a **separate
//! backend slice**; this module touches no backend, no GPU/DOM/font/scene type.
//! It imports no other module. The app/runtime reads the list out and submits it
//! to a backend (the single legal home for cross-module translation).
//!
//! ## Presentation class (SPEC-04 §6, §17.5)
//! The whole surface is presentation-class: `onRender` is the only caller and
//! **nothing it produces is authoritative**. The facade exposes no getter that
//! returns draw state into a sim-readable form — there is no read-back path.
//!
//! ## Deferred in this slice (documented, follow-up)
//! The §4.1 priority core landed here: shapes (`rect`/`circle`/`ellipse`/
//! `line`/`path`), `sprite`, `text` + `measure_text`, `linear`/`radial`
//! gradients, the camera + transform stack, the `(layer, submission)` sort, and
//! the [`Draw2dList`]/[`Draw2dCommand`] contract. Two §4.1 items are
//! **deliberately deferred** to keep this slice fully covered and branchless:
//! - **Particles (§10.1)** — needs a kernel `Seconds` dimensioned scalar
//!   (absent today) for the presentation-`dt` step; adding it is a kernel-layer
//!   change beyond this module slice.
//! - **Render targets (§10.3)** — nested off-screen draw-lists / routing.
//!
//! Both are additive: they extend the facade and the command set without
//! reshaping the contract above.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`Draw2dApi`] — plus its
//! value-type vocabulary via a single `pub use ids::{…}` line (Module Law #8).

mod camera2d;
mod common2d;
mod draw2d_api;
mod draw2d_command;
mod draw2d_list;
mod fill2d;
mod handles;
mod ids;
mod paint;
mod rect;
mod rgba;
mod sprite_draw2d;
mod text2d;

pub use draw2d_api::Draw2dApi;
pub use ids::{
    Camera2d, Common2d, Draw2dCommand, Draw2dList, Fill2d, FontHandle, Glyph2d, GlyphRun,
    GradientStop, PaintId, Rect, Rgba, Shadow2d, SpriteDraw2d, Stroke2d, TextAlign, TextDraw2d,
    TextMetrics, TextureId, TransformDepth,
};
