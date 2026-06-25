//! # Axiom Space — deterministic content addressing (root-adjacent layer)
//!
//! `space` is the engine's **addressing** primitive: a stable, hashable,
//! serializable [`Address`] names *what/where* content is generated, and
//! [`SpaceApi`] mints, navigates, serializes, and digests addresses through the
//! kernel's canonical byte + digest primitives. It is the first
//! procedural-generation substrate layer: every later layer (`entropy`, `proc`)
//! and every domain generator keys its work by an `Address`, so a site's identity
//! is the same on every platform and every run.
//!
//! ## What it is, and is not
//! - An address is a hierarchical `u64` **key-path**. It is **domain-free**: a
//!   segment is an opaque key, not a coordinate with geometry semantics. Callers
//!   encode their own space (signed coords, multi-axis chunks) into segments.
//! - It owns **no geometry** (that is `math`) and **no generation** (an address
//!   *names* a site; it does not generate one). No browser/platform APIs.
//!
//! ## Why a layer, and why root-adjacent
//! Many sibling generators must share one addressing primitive, and engine
//! modules may not depend on one another — so the shared substrate must be a
//! layer. It genuinely uses exactly one lower layer: the **kernel** (its
//! `StableHash` digest and `BinaryWriter`/`BinaryReader` serialization), so
//! `depends_on = ["kernel"]` and it sits beside `crypto`/`runtime`, not stacked
//! above `introspect`.
//!
//! ## Public surface
//! [`SpaceApi`] (the facade) and [`Address`] (the value vocabulary it hands back).

mod address;
mod space_api;

pub use address::Address;
pub use space_api::SpaceApi;
