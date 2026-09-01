//! # Axiom Noise — deterministic coherent noise + FBM with domain warp (layer)
//!
//! `noise` is the procedural-field primitive worldgen samples: deterministic 3D
//! gradient (Perlin-style) noise, its multi-octave fractal Brownian motion (FBM),
//! and domain warp. The same **seed + point** always yields the same value on
//! every run and platform — the only source of "randomness" is the kernel's
//! canonical-bytes digest, keyed by a lattice coordinate.
//!
//! ## What it is, and is not
//! - It **keys** the kernel's [`axiom_kernel::StableHash`] (FNV-1a over canonical
//!   bytes) by an integer lattice cell `(seed, xi, yi, zi)` to pick a per-cell
//!   gradient — it invents **no bespoke RNG or mixer** (the old splitmix64 copy is
//!   retired) and carries **no ambient entropy** and **no wall clock**.
//! - It is a **spatial field**, not a sequential recipe: it depends on `math` for
//!   the [`axiom_math::Vec3`] positions/gradients, not on the `proc` recipe layer.
//!
//! ## Why a layer, depending on kernel + math
//! Many generators need the same coherent noise, and an engine **module** may
//! depend only on **layers** (never on another module) — so the shared noise
//! primitive is a layer a terrain/biome module can build on. It genuinely uses the
//! **kernel** (the [`StableHash`] that keys its lattice, the [`axiom_kernel::Ratio`]
//! that types its gain) and **math** (the [`Vec3`] it samples and gradients with),
//! so `depends_on = ["kernel", "math"]`.
//!
//! ## Two bases, and how to choose
//!
//! A noise layer that can express exactly one noise function is not a layer, it
//! is a function. There are two bases here, they are peers, and they are not
//! interchangeable — each is the identity of the fields built on it, so
//! "harmonising" them would silently move every texture and every placement
//! downstream.
//!
//! | | **gradient** ([`value_noise`], [`Fbm`]) | **positional value** ([`value_noise_01`], [`value_fbm_01`]) |
//! |---|---|---|
//! | lattice key | [`axiom_kernel::StableHash`] of `(seed, cell)` | [`hash_01`], a pure function of position |
//! | seeded? | yes — a `seed` selects a field | **no** — there is no seed to pass |
//! | output | [`NoiseValue`], signed `[-1, 1]` | [`UnitNoise`], unsigned `[0, 1]` |
//! | precision | `f32` | `f64` |
//! | fade | quintic `6t⁵-15t⁴+10t³` | cubic `3t²-2t³` |
//! | reach for it when | a field should differ per world/run/instance | a field must be **stable against unrelated change** |
//!
//! The second row is the real distinction. A seeded basis is the right default
//! and the wrong tool for surface variation: the moment some *other* subsystem
//! takes one more draw from the shared stream, every seeded field downstream
//! reshuffles. The positional basis never consults a stream, so the wear on a
//! wall is a function of where the wall is and nothing else — it cannot be
//! moved by an edit somewhere unrelated. That is a durability property, not a
//! performance one, and it is why both exist.
//!
//! The precision row follows `axiom_math::Scalar`: `f32` is the *interchange*
//! scalar, and the positional basis is evaluated at bake time to produce texture
//! tiles and drive geometry — and serves as the oracle a shader gets pinned
//! against — so it evaluates in `f64` and narrows once, at
//! [`UnitNoise::get_single`]. The gradient basis stays `f32` because it feeds
//! `axiom_field`'s WGSL compiler, whose CPU↔GPU parity is measured against
//! per-operator tolerances at that precision; moving it would perturb every one
//! of those measurements for no gain.
//!
//! ## Public surface
//! - [`value_noise`] — single-octave gradient noise, a bounded [`NoiseValue`].
//! - [`Fbm`] + [`FbmConfig`] — the multi-octave field and its typed parameters.
//! - The typed knobs [`Frequency`], [`Lacunarity`], [`WarpStrength`], and the
//!   [`NoiseValue`] output — so no naked scalar reaches the public API.
//! - [`hash_01`] / [`value_noise_01`] / [`value_fbm_01`] — the positional value
//!   basis, and [`UnitNoise`], its unsigned double-precision output.
//!
//! The tie-breaking rule the lattice hash quantises with is deliberately
//! **private** — see `hash_01.rs`. It is load-bearing, and it has no caller
//! outside that file; a layer that exports every internal constant a
//! reimplementation "might" need is a layer with an API nobody called.

mod fbm;
mod fbm_config;
mod frequency;
mod gradient_noise;
mod lacunarity;
mod noise_value;
mod warp_strength;

// The positional value-noise family: a second basis, at double precision. See
// the "Two bases" section above for why it is a peer rather than a variant.
mod cellular_2d;
mod hash_01;
mod perlin_2d;
mod permutation_lattice;
mod signed_noise;
mod unit_noise;
mod value_fbm_01;
mod value_noise_01;

pub use fbm::Fbm;
pub use fbm_config::FbmConfig;
pub use frequency::Frequency;
pub use gradient_noise::value_noise;
pub use lacunarity::Lacunarity;
pub use noise_value::NoiseValue;
pub use warp_strength::WarpStrength;

pub use hash_01::hash_01;
pub use cellular_2d::{worley_edge, worley_f1};
pub use perlin_2d::{perlin_2d, perlin_fbm_2d, perlin_ridged_2d, perlin_warped_2d};
pub use permutation_lattice::PermutationLattice;
pub use signed_noise::SignedNoise;
pub use unit_noise::UnitNoise;
pub use value_fbm_01::value_fbm_01;
pub use value_noise_01::value_noise_01;
