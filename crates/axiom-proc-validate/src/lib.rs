//! # Axiom Proc-Validate — constraints, scoring, and bounded repair (layer)
//!
//! `proc-validate` makes generated output **trustworthy**: [`ProcValidateApi`]
//! validates a generation's neutral output words against declarative
//! [`Constraint`]s into a deterministic [`ValidationReport`] (a per-constraint
//! verdict + score), and `repair`s a failing word list with a single bounded pass
//! of word-level fixes, returning a new, re-validatable list.
//!
//! ## What it is, and is not
//! - **Domain-free.** A constraint is a generic numeric property of opaque words
//!   (minimum count, upper bound, non-zeroness). Domain rules — "rivers reach the
//!   sea", "a room has a door" — are a *terrain/level module's* job, never this
//!   layer's.
//! - **Container-free.** The words are whatever a generator produced. The recipe
//!   stack (`axiom-recipe` + `axiom-proc-core`) is generic over its output type
//!   and deliberately owns no artifact struct, so this layer names none either —
//!   binding validation to one concrete container is what coupled it to the
//!   retired v1 stack.
//! - **Bounded.** Repair is one pass of word-level transforms; it never loops to a
//!   fixpoint and never invents content (a structural minimum-count failure is
//!   left unsatisfied by design). No browser/platform APIs.
//!
//! ## Why a layer, depending on the kernel
//! Validation is shared substrate every generator wants, so it is a layer. It
//! genuinely uses the **kernel** (`StableHash` + `BinaryWriter` for the report's
//! canonical bytes + digest) and nothing else: a verdict over neutral words needs
//! no generation machinery, and declaring one it does not call would be exactly
//! the ceremonial dependency the Layer Law forbids.
//!
//! ## Public surface
//! [`ProcValidateApi`] (facade), [`Constraint`] (the declarative checks),
//! [`ValidationReport`] (the deterministic verdict), and [`sample_until_valid`]
//! (the generative counterpart: bounded, branchless rejection sampling — draw
//! candidates until one validates, else keep the last).

mod constraint;
mod report;
mod sampling;
mod validate_api;

pub use constraint::Constraint;
pub use report::ValidationReport;
pub use sampling::sample_until_valid;
pub use validate_api::ProcValidateApi;
