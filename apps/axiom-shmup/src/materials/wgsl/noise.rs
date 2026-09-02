//! WGSL transcription of Claude-of-Duty `src/materials/glsl/noise.js` — the
//! tileable procedural noise library every surface generator is built from.
//!
//! The source's own header, verbatim:
//!
//! > Tileable procedural noise library (GLSL, shared by every surface
//! > generator).
//! >
//! > Everything here is *periodic*: each function takes a `per` (period, in
//! > lattice cells) and wraps its hash lattice with mod(), so a texture
//! > generated over uv in \[0,1) with p = uv * per tiles seamlessly. Octaves
//! > double both the frequency and the period, which keeps the whole fbm stack
//! > seamless.
//! >
//! > Hashes are sin-free (Dave Hoskins style) — sin() based hashes band badly
//! > on Apple GPUs at high lattice coordinates.
//!
//! This is the **twin** of [`crate::materials::noise`], which is the same
//! library on the CPU in `f64`. Neither is derived from the other: both are
//! transcribed from `noise.js`, so a disagreement between them is a real
//! finding rather than a shared misreading. That is the whole point of keeping
//! two — the port has already measured what happens when one transcription
//! checks another written by the same hand (ten defects in `sky/`).
//!
//! ## The GLSL-semantics helpers
//!
//! `mix`, `clamp`, `step`, `smoothstep`, `mod` and `sign` all exist in WGSL,
//! and WGSL is permitted to factor them differently from GLSL. They are
//! therefore written out here to their exact GLSL definitions, as `ow*`
//! helpers — the precedent `surface_program::emit` sets, and the shape the CPU
//! twin already has (`gl_mix`, `gl_clamp`, `gl_smoothstep`, `gl_mod`,
//! `gl_fract`). Two of them are not interchangeable with their WGSL
//! namesakes at all:
//!
//! * **`owMod`** is `x - y * floor(x / y)`, not a truncated remainder. Lattice
//!   coordinates go negative at the tile's wrapped edge, where `%` and `mod`
//!   disagree in sign.
//! * **`owSign`** returns `0.0` for zero, which is GLSL's rule and the one
//!   `CORRUGATED`'s ridge crossings rely on.
//!
//! `abs`, `min`, `max`, `floor`, `fract`, `sqrt`, `sin`, `cos`, `pow`, `exp`,
//! `length`, `dot` and `normalize` are exact in both languages and are used as
//! builtins.

/// The GLSL-semantics shims, emitted ahead of the library because the library
/// itself calls them.
///
/// Only the widths the ported generators actually use are declared. A generator
/// that needs another (`owMix4`, `owClamp2`, …) adds it here, next to its
/// siblings, rather than reaching for the WGSL builtin.
pub const GL_SEMANTICS: &str = include_str!("gl_semantics.wgsl");

/// `NOISE_GLSL` (`noise.js:12-218`), transcribed function for function.
///
/// Every loop and branch below is inside a `&str`: it is shader text, and a
/// `for` over nine Worley cells is exactly what the source writes.
pub const NOISE: &str = include_str!("noise.wgsl");

/// `DETAIL_SRC` (`generator.js:91-120`) — the shared micro-detail tile.
///
/// The source's NYQUIST note (`generator.js:80-90`) is why every band is capped
/// at `K = 20`: "the tile is 1024 px across 0.25 m, so one texel is 0.244 mm …
/// anything past K≈24 is under five texels and bakes as white noise."
pub const DETAIL: &str = include_str!("detail.wgsl");

/// `MACRO_SRC` (`generator.js:127-137`) — four bands of low-frequency variation
/// used by every material to break up tiling: R = very low fbm, G = warped
/// blotches, B = mid fbm, A = fine fbm.
pub const MACRO: &str = include_str!("macro.wgsl");
