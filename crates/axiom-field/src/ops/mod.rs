//! The twenty-seven operator implementations, one function per operator,
//! grouped by family.
//!
//! **These functions are the definition of what the field language means.** The
//! WGSL emitted for a GPU backend and any per-triangle CPU shading path are
//! mirrors of them, checked against them, and every one carries a one-line
//! statement of its semantics because those are the words a mirror is written
//! against.
//!
//! Every operator is **total**: a `FieldValue` in, a `FieldValue` out, no
//! `Option` and no error. Every rejection already happened in
//! [`crate::FieldGraph::validate`], and each remaining out-of-range read is made
//! total against a documented default rather than a panic.
//!
//! | Family | Operators |
//! |---|---|
//! | [`source`] | `Const`, `Point`, `Uv`, `Normal`, `Time`, `Param` |
//! | [`arith`] | `Add`, `Sub`, `Mul`, `Min`, `Max`, `Abs` |
//! | [`shape`] | `Clamp`, `Mix`, `Smoothstep` |
//! | [`vector`] | `Dot`, `Length`, `Normalize`, `Compose`, `Component` |
//! | [`spatial`] | `Noise`, `Fbm`, `Transform` |
//! | [`transcendental`] | `Sin`, `Cos`, `Pow`, `Exp` |
//!
//! [`transcendental`] is the one family whose CPU↔GPU agreement is a *measured,
//! per-operator* tolerance rather than the algebra's shared `1e-4`: both sides
//! approximate those four with different polynomials. Its module header states
//! the budget and the `Pow` rule that makes it mirrorable at all.

pub(crate) mod arith;
pub(crate) mod shape;
pub(crate) mod source;
pub(crate) mod spatial;
pub(crate) mod transcendental;
pub(crate) mod vector;
