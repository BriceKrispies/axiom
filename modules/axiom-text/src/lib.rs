//! # Axiom Text — Engine Module
//!
//! Axiom's deterministic, backend-neutral **text and typography** capability.
//! `Text` is one unified primitive: plain and rich text, screen- and world-space
//! placement, and per-character effects are all configurations of the same
//! thing, never separate `Label`/`Text2D`/`RichText` systems.
//!
//! This crate owns everything from text content down to a backend-neutral,
//! ordered glyph batch, and consumes only the compiled [`CompiledFont`]
//! (`.axfont`) runtime format. External font containers (TTF/OTF/WOFF/WOFF2) are
//! compiled into `.axfont` **offline** by the `axiom-font-import` tool — nothing
//! here parses a font container, rasterizes a glyph, touches a filesystem or
//! browser API, reads a wall clock, or draws to a GPU. It exposes neutral
//! placement + glyph-batch data; an app translates that into a renderer contract.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`TextApi`] — plus its
//! pure value vocabulary via `pub use ids::{…}`, per Module Law #8.

mod atlas_page;
mod codepoint_entry;
mod color;
mod compiled_font;
mod effect;
mod face_metrics;
mod face_slant;
mod fallback_font;
mod font_builder;
mod font_registry;
mod font_table;
mod glyph_batch;
mod glyph_metric;
mod glyph_raster;
mod hit_test;
mod ids;
mod import_provenance;
mod kern_pair;
mod laid_out_text;
mod layout;
mod placement;
mod size_layer;
mod span;
mod style;
mod text_api;
mod text_error;
mod text_limits;
mod text_record;
mod text_snapshot;

pub use text_api::TextApi;

pub use ids::{
    Align, Billboard, CompiledFont, DirtyFlags, EffectKind, FaceMetrics, FaceSlant, FontBuild,
    FontHandle, GlyphBatch, GlyphInput, GlyphInstance, LaidOutText, LayoutConfig, LineMetrics,
    Overflow, Rgba, ScreenPlacement, Space, StyleOverride, TextBounds, TextEffect, TextError,
    TextHandle, TextLimits, TextPlacement, TextResult, TextSnapshot, TextSpan, TextStyle,
    UpdateMode, VerticalAlign, WorldPlacement, Wrap,
};
