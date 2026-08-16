//! # Axiom Surface — the engine's neutral appearance artifact (layer)
//!
//! A **surface** is a closed record of seven named shading channels — base
//! colour, roughness, metallic, normal, emission, opacity, displacement — each
//! bound to a constant value or to an [`axiom_field::FieldGraph`], plus a
//! lighting-model discriminant and mask-driven layering. It is where
//! *"roughness"* is allowed to exist for the first time: the expression language
//! underneath it knows nothing about rendering, and this layer is the vocabulary
//! that gives an expression a **job**.
//!
//! This crate owns:
//!
//! - [`SurfaceChannel`] — the closed seven-channel vocabulary, each channel's
//!   declared [`axiom_field::FieldType`] and its default value.
//! - [`ChannelBinding`] — a tagged struct: a constant, or a field.
//! - [`Surface`] — the artifact, and [`SurfaceBuilder`], its authoring surface.
//! - [`SurfaceLayer`] / [`LayerBlend`] / [`MAX_LAYERS`] — bounded, mask-driven
//!   layering that **flattens into one field graph per channel**.
//! - [`LightingModel`] — how a surface participates in lighting: a three-variant
//!   discriminant, not a programmable hook.
//! - [`SurfaceRequirements`] / [`SurfaceInput`] — what a backend must satisfy,
//!   derived from the bound graphs and checked *before* anything is lowered.
//! - [`SurfaceError`] / [`SurfaceErrorCode`] / [`SurfaceResult`] — decode and
//!   validation failures, each naming the channel, the layer and the field node
//!   it concerns.
//!
//! ## What it is, and is not
//!
//! - **It is a value, and a preparation-time one.** Same construction → same
//!   bytes → same [`Surface::digest`], on every target. It holds graphs and a
//!   `Vec`; it is addressed by identity after preparation and must never be
//!   cloned per frame.
//! - **Its digest is structural.** A parameter value inside a bound graph is
//!   deliberately outside the digest and each parameter slot's declared *type*
//!   is inside it, exactly as `axiom_field::FieldGraph::digest` does it — so
//!   retuning a material cannot invalidate a compiled program, and animating a
//!   parameter cannot explode into variants.
//! - **Channel graphs are evaluated in object space.** `EvalContext::point` is a
//!   position in the object's own frame. A world-space pattern swims when the
//!   object moves; an object-space one rides with it. Triplanar projection is
//!   therefore *authorable* — three samples blended by `Abs(Normal)` weights —
//!   and needs no new primitive.
//! - **It is not PBR.** [`SurfaceChannel::Metallic`] is a *channel*, not a BRDF:
//!   carried, digested, reported, and read by no lighting model. There is no
//!   transmission, subsurface, clear-coat or anisotropy — a channel nothing can
//!   render is debt, not capability.
//! - **It is not a shader.** No WGSL, no stages, no bindings, no varyings, no
//!   pipelines, no backends. Lowering lives in the backend modules that already
//!   own every shader string in the engine.
//! - **It binds fields, not images.** There is no texture-sampling channel: one
//!   of the engine's two backends cannot sample at all.
//!
//! ## Why a layer, depending on kernel + math + field + recipe
//!
//! Seven engine **modules** must be able to name a material description —
//! `resources`, `render`, `render-pipeline`, the GPU backend, the software
//! rasterizer backend, `assets` and the `axiom` facade — and a module may never
//! depend on another
//! module, so a module placement is structurally impossible rather than merely
//! awkward. This is the `axiom-mesh` precedent verbatim: *seven engine modules
//! need to name triangle geometry*, so the neutral triangle mesh is a layer.
//!
//! It genuinely uses **field** (every binding is a `FieldGraph` or the
//! `FieldValue` a `Const` node carries; flattening is graph composition through
//! `FieldBuilder`; the requirements summary is derived by reading `FieldOp`
//! codes), **math** (the `Vec3`/`Vec4` a channel value carries and the finite
//! differences of the height-to-normal derivation), **kernel** (the canonical
//! bytes, the `SchemaVersion` stamp, the `StableHash` digest, and the
//! `Meters`/`Ratio` quantities the height-to-normal derivation is authored in),
//! and **recipe** (the `Scalar` every channel constant is built from, the
//! `NodeId` that names a node of a bound graph, and the raw `Param` words that
//! graph composition copies and rebases).
//!
//! ## Determinism
//!
//! Same surface → same bytes → same digest, on every target including `wasm32`.
//! Flattening is a pure function, order-stable and idempotent. Nothing here
//! depends on an address, an iteration order or a `TypeId`, and the recursive
//! value type is walked **iteratively** everywhere — a bounded breadth-first
//! linearisation, never a recursive descent.

mod binding;
mod channel;
mod compose;
mod flatten;
mod layer;
mod layer_tree;
mod lighting_model;
mod requirements;
mod surface;
mod surface_builder;
mod surface_bytes;
mod surface_error;

pub use binding::ChannelBinding;
pub use channel::{SurfaceChannel, SURFACE_CHANNEL_COUNT};
pub use layer::{LayerBlend, SurfaceLayer, MAX_LAYERS};
pub use lighting_model::LightingModel;
pub use requirements::{SurfaceInput, SurfaceRequirements};
pub use surface::{Surface, SURFACE_SCHEMA_VERSION};
pub use surface_builder::SurfaceBuilder;
pub use surface_error::{SurfaceError, SurfaceErrorCode, SurfaceResult};
