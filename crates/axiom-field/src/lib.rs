//! # Axiom Field — the typed pointwise field IR (layer)
//!
//! A **field** is a deterministic, hashable, canonically-serializable pure
//! function from an explicitly supplied typed evaluation context to a typed
//! value, represented as a closed-algebra, id-ordered, acyclic expression graph.
//! It is the engine's *function-as-a-value*, and it has **nothing to do with
//! rendering**: a height for a displacement, a mask for a placement rule, a
//! density for an implicit surface and a colour for a material are all the same
//! value here, and nothing downstream can tell them apart.
//!
//! This crate owns the **representation** only:
//!
//! - [`FieldType`] — the four-type lattice, and [`FieldValue`], the tagged
//!   struct that carries one.
//! - [`FieldOp`] — the closed 23-operator algebra, and [`FieldSignature`] /
//!   [`SignatureKind`], the `const` table that gives each operator its shape.
//! - [`FieldGraph`] — the typed graph, and [`FieldBuilder`], its append-only
//!   authoring surface.
//! - [`FieldParams`] / [`FieldParamSlot`] — the parameter table that keeps a
//!   value change out of the structural digest.
//! - [`EvalContext`] — the explicit description of every external input.
//! - [`FieldError`] / [`FieldErrorCode`] / [`FieldResult`] — decode and
//!   validation failures, each naming the node it concerns.
//!
//! ## What it is, and is not
//!
//! - **It wraps `recipe`, it does not replace it.** [`FieldGraph`] holds an
//!   [`axiom_recipe::RecipeGraph`]; acyclicity, the node budget, dense ids and
//!   the canonical node encoding are the container's, for free. What this layer
//!   adds is the type lattice, the operator meanings, the declared output node
//!   and the parameter table.
//! - **It carries the semantic reference evaluator.**
//!   [`FieldGraph::evaluate`] is a flat fold in id order over a fixed-size
//!   register file — no allocation, no recursion, and a `const` table of
//!   function pointers in place of a `match`. **What those operator functions
//!   compute *is* what the language means**; a shader emitted for a GPU backend
//!   is a mirror checked against them, never a second definition.
//! - **It type-checks and canonicalises.** [`FieldGraph::validate`] proves a
//!   graph is a well-formed, well-typed program in one forward fold;
//!   [`FieldGraph::canonicalize`] folds constants, shares common
//!   subexpressions, drops dead nodes and relabels ids, so that **two graphs
//!   computing the same thing produce identical bytes and identical digests**.
//! - **It is not a shader graph VM.** The algebra is *closed* — 23 operators
//!   fixed in Rust, no registry, no runtime-extensible verb, no dynamic
//!   dispatch. A new visual effect is a new *graph*, never a new Rust function.
//! - **It knows nothing of scenes, materials, textures, backends or GPUs.** A
//!   coordinate space is a property of the [`EvalContext`] the caller supplies,
//!   not of any type here.
//! - **It is mechanically editable.** [`FieldGraph::describe`],
//!   [`FieldGraph::explain`], [`FieldGraph::dependents_of`],
//!   [`FieldGraph::replace_subgraph`], [`FieldGraph::insert_before`],
//!   [`FieldGraph::inline`] and [`FieldGraph::diff`] are the agent-facing half:
//!   identify, inspect, walk, rewrite, validate, serialize, diff, hash, explain.
//!   Every rewrite returns a **new** graph.
//!
//! ## This layer publishes the vocabulary its own API traffics in
//!
//! [`NodeId`], [`Param`], [`Scalar`] and [`MAX_NODES`] are `recipe`'s types, and
//! they are **re-exported here on purpose**. Every one of them appears on this
//! layer's own public boundary — a node is named by a `NodeId`,
//! [`FieldBuilder::push`] takes `Vec<Param>`, a [`FieldValue`] lane is a
//! `Scalar`, and the node budget a rewrite is checked against is `MAX_NODES` —
//! so a consumer that could not name them could not call this layer at all.
//!
//! Before they were published here, a consumer had to declare `recipe` itself to
//! spell a type `field` had handed it, which put a layer in a `depends_on` for a
//! reason that had nothing to do with what that layer *does*. Republishing them
//! is the structural fix: **the layer that hands you a value is the layer you
//! name it through.**
//!
//! It is not a widening of the surface. Nothing new is invented, nothing is
//! wrapped, and `recipe` remains the one definition — `axiom_field::NodeId`
//! **is** `axiom_recipe::NodeId`, so the two spellings can never drift.
//! ## Why a layer, depending on kernel + math + recipe + noise
//!
//! Three engine **layers** (`mesh-ops`, `proc-texture`, `proc-mesh`) must be
//! able to name a field, and a layer may never depend on a module — so a module
//! placement is structurally impossible, not merely awkward. This is the
//! `axiom-mesh` precedent verbatim. It genuinely uses **recipe** (the container,
//! its ids, its raw [`axiom_recipe::Param`] words and its
//! [`axiom_recipe::Scalar`] quantity), **math** (the `Vec2`/`Vec3`/`Vec4` lanes
//! of the value union and the `Mat4` whose columns fix `Transform`'s arity),
//! **kernel** (serialization, the [`axiom_kernel::SchemaVersion`] stamp, the
//! [`axiom_kernel::StableHash`] that mints identity and labels the bytes, and
//! the [`axiom_kernel::Seconds`] the context carries), and **noise** (the
//! `FbmConfig` whose knob set fixes `Fbm`'s parameter arity).
//!
//! ## Determinism
//!
//! Same graph → same bytes → same digest, on every target including `wasm32`.
//! Node ids are dense insertion indices; nothing depends on an address, an
//! iteration order or a `TypeId`. There is no `f64` anywhere. **The bytes are
//! the determinism proof; the digest is the label.**

mod canonical;
mod const_fold;
mod diff;
mod dispatch;
mod eval;
mod eval_context;
mod field_builder;
mod field_error;
mod field_graph;
mod field_op;
mod field_params;
mod field_type;
mod field_value;
mod ids;
mod inspect;
mod noise_words;
mod ops;
mod rewrite;
mod signature;
mod type_check;

pub use axiom_recipe::{NodeId, Param, Scalar, MAX_NODES};
pub use diff::FieldDiff;
pub use eval_context::EvalContext;
pub use inspect::{FieldDescription, FieldExplanation, FieldNodeDescription, FIELD_DESCRIPTION_SCHEMA_VERSION};
pub use field_builder::FieldBuilder;
pub use field_error::{FieldError, FieldErrorCode, FieldResult};
pub use field_graph::{FieldGraph, FIELD_SCHEMA_VERSION};
pub use field_op::{FieldOp, FIELD_OP_COUNT};
pub use field_params::FieldParams;
pub use field_type::FieldType;
pub use field_value::FieldValue;
pub use ids::{FieldId, FieldParamSlot};
pub use signature::{FieldSignature, SignatureKind};
