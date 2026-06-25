//! # Axiom Entropy — deterministic, address-keyed entropy streams (layer)
//!
//! `entropy` expands one root **seed** into an independent, reproducible stream
//! per **`(seed, Address, version)`**, so two generated sites never share state
//! and a site's stream is identical on every run and platform. [`EntropyApi`]
//! mints streams; [`EntropyStream`] draws values and forks isolated sub-streams.
//!
//! ## What it is, and is not
//! - It **routes and keys** the kernel's existing [`axiom_kernel::DeterministicRng`]
//!   (seeded splitmix64): a `(seed, address-digest, version)` tuple is folded into
//!   a derived key with the kernel's `StableHash`, and that key seeds the stream.
//! - It introduces **no new RNG algorithm**, **no noise functions** (noise
//!   graduates into a Phase 9 domain module), and **no ambient entropy** — there
//!   is no OS RNG and no wall clock anywhere in the layer.
//!
//! ## Why a layer, depending on kernel + space
//! Every generator must key its randomness the same way; the keying primitive is
//! shared substrate, so it is a layer. It genuinely uses the **kernel** (the RNG
//! it routes and the digest it keys with) and **space** (the [`axiom_space::Address`]
//! that names the site), so `depends_on = ["kernel", "space"]`.
//!
//! ## Public surface
//! [`EntropyApi`] (the facade) and [`EntropyStream`] (the keyed stream it hands back).

mod entropy_api;
mod entropy_stream;

pub use entropy_api::EntropyApi;
pub use entropy_stream::EntropyStream;
