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
//! ## Two determinism models
//!
//! [`ProcCore::execute`] keys a stream per node by address, which lets a graph
//! be evaluated in any order, partially, or in parallel — what a texture or a
//! mesh wants. [`ProcCore::evaluate`] hands the domain no stream at all and
//! keeps every node's output, for a domain whose randomness is a **single
//! sequential source shared across the frame**, where the *order* of the draws
//! is the contract and one extra draw shifts every later effect.
//!
//! Neither model is more correct. `execute` is written in terms of `evaluate`,
//! so there is one walk of the graph and not two.
//!
//! `evaluate` also allocates nothing per node — inputs are borrowed from the
//! caller's cache rather than copied into a fresh buffer — because a domain
//! that runs a graph once per emitted particle cannot pay two allocations a
//! node for the privilege.
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
mod node_step;
mod proc_core;
mod proc_error;

pub use node_eval::NodeEval;
pub use node_step::NodeStep;
pub use proc_core::ProcCore;
pub use proc_error::{ProcError, ProcResult};
