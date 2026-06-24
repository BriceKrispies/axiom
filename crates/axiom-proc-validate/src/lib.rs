//! # Axiom Proc-Validate — constraints, scoring, and bounded repair (layer)
//!
//! `proc-validate` makes generated artifacts **trustworthy**: [`ProcValidateApi`]
//! validates a proc [`axiom_proc::Artifact`]'s neutral words against declarative
//! [`Constraint`]s into a deterministic [`ValidationReport`] (a per-constraint
//! verdict + score), and `repair`s a failing artifact with a single bounded pass
//! of word-level fixes, returning a new, re-validatable artifact.
//!
//! ## What it is, and is not
//! - **Domain-free.** A constraint is a generic numeric property of opaque words
//!   (minimum count, upper bound, non-zeroness). Domain rules — "rivers reach the
//!   sea", "a room has a door" — are a *terrain/level module's* job, never this
//!   layer's.
//! - **Bounded.** Repair is one pass of word-level transforms; it never loops to a
//!   fixpoint and never invents content (a structural minimum-count failure is
//!   left unsatisfied by design). No browser/platform APIs.
//!
//! ## Why a layer, depending on kernel + proc
//! Validation is shared substrate every generator wants, so it is a layer. It
//! genuinely uses **proc** (the `Artifact` it validates and the repaired artifact
//! it builds) and the **kernel** (`StableHash` + `BinaryWriter` for the report's
//! canonical bytes + digest).
//!
//! ## Public surface
//! [`ProcValidateApi`] (facade), [`Constraint`] (the declarative checks), and
//! [`ValidationReport`] (the deterministic verdict).

mod constraint;
mod report;
mod validate_api;

pub use constraint::Constraint;
pub use report::ValidationReport;
pub use validate_api::ProcValidateApi;
