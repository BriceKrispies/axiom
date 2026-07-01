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
//! ## Public surface
//! - [`value_noise`] — single-octave gradient noise, a bounded [`NoiseValue`].
//! - [`Fbm`] + [`FbmConfig`] — the multi-octave field and its typed parameters.
//! - The typed knobs [`Frequency`], [`Lacunarity`], [`WarpStrength`], and the
//!   [`NoiseValue`] output — so no naked scalar reaches the public API.

mod fbm;
mod fbm_config;
mod frequency;
mod gradient_noise;
mod lacunarity;
mod noise_value;
mod warp_strength;

pub use fbm::Fbm;
pub use fbm_config::FbmConfig;
pub use frequency::Frequency;
pub use gradient_noise::value_noise;
pub use lacunarity::Lacunarity;
pub use noise_value::NoiseValue;
pub use warp_strength::WarpStrength;
