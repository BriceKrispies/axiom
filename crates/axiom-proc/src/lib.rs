//! # Axiom Proc — the procedural-generation graph core (layer)
//!
//! `proc` is the engine's **generation core**: a [`Recipe`] is a versioned DAG of
//! generic nodes, and [`ProcApi`] evaluates it deterministically over an
//! address-keyed entropy stream into a neutral [`Artifact`] (opaque `u64` words +
//! canonical bytes + stable digest) and a [`ProcTrace`] (the decision log). It is
//! the layer everything in Phases 7–12 builds on.
//!
//! ## What it is, and is not
//! - **Domain-free.** A node is a generic op (`const` / entropy-`draw` / `add` /
//!   `xor`); an artifact is opaque words. What the words *mean* — terrain, biome,
//!   a level — is a domain module's job (Phase 9), never this layer's. There is no
//!   noise, no geometry, no terrain here.
//! - **Branchless.** Node dispatch is a table index over the op discriminant, not
//!   a `match` over kinds; a DAG is enforced by construction (inputs reference
//!   only earlier nodes) and an invalid recipe is rejected as data (`None`), never
//!   a panic.
//! - **Budget-independent.** [`Evaluation`] is resumable: stepping one node at a
//!   time yields byte-identical output to one whole evaluation, so generation can
//!   be spread across frames without ever running unbounded.
//!
//! ## Why a layer, depending on kernel + space + entropy
//! Every generator shares this evaluation core, so it is a layer. It genuinely
//! uses the **kernel** (`StableHash` digest, `BinaryWriter` serialization,
//! `SchemaVersion` stamp), **space** (the `Address` it evaluates *at*), and
//! **entropy** (the keyed stream it draws from).
//!
//! ## Public surface
//! [`ProcApi`] (facade), [`Recipe`] (the DAG), [`Artifact`] + [`ProcTrace`] (the
//! neutral outputs), and [`Evaluation`] (the resumable, budgeted run).

mod artifact;
mod evaluation;
mod node;
mod proc_api;
mod recipe;
mod trace;

pub use artifact::Artifact;
pub use evaluation::Evaluation;
pub use proc_api::ProcApi;
pub use recipe::Recipe;
pub use trace::ProcTrace;
