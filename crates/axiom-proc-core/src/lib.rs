//! # Axiom Proc-Core — the recipe-graph executor (layer)
//!
//! One deterministic, domain-agnostic executor that every generation layer
//! shares. Given a validated [`axiom_recipe::RecipeGraph`] and a domain
//! evaluator, [`ProcCore::execute`] walks the graph in dependency (id) order,
//! caches each node's output for its dependents, keys a per-node
//! [`axiom_entropy::EntropyStream`] by `(seed, address, version)`, and hands each
//! node's operator code, parameters, inputs, and stream to the evaluator through
//! a [`NodeEval`]. It returns the final node's output, or a stable [`ProcError`].
//!
//! ## What it is, and is not
//! - **Generic over the output type.** Textures and meshes reuse one executor;
//!   the executor owns no operators — what a node computes is the domain's job.
//! - **Deterministic.** The same recipe, seed, and base address produce the same
//!   output; determinism rides `axiom-space` addresses and `axiom-entropy`
//!   streams, never wall-clock or ambient state.
//! - **Branchless.** Node walking is a fold; dispatch to the operator lives in
//!   the domain evaluator (a table over the operator code), never here.

mod node_eval;
mod proc_core;
mod proc_error;

pub use node_eval::NodeEval;
pub use proc_core::ProcCore;
pub use proc_error::{ProcError, ProcResult};
