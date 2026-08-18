//! Ported from Claude-of-Duty `src/sky/` — the physical atmosphere model,
//! the three LUT bakes it backs, the sky's shared procedural noise, and the
//! sun/moon ephemeris.
//!
//! | this module        | source              |
//! |---------------------|---------------------|
//! | [`atmosphere`]      | `src/sky/atmosphere.js` |
//! | [`luts`]            | `src/sky/luts.js`   |
//! | [`noise`]           | `src/sky/noise.js`  |
//! | [`celestial`]       | `src/sky/celestial.js` |
//!
//! The source bakes its three atmosphere LUTs (transmittance, multiscatter,
//! sky-view) plus a 2x1 ambient probe as WebGL2 fragment shaders
//! (`luts.js`'s `SkyLuts`). This crate has no GPU/WGSL emission path yet, so
//! every `*_FRAG` shader body is ported here as an ordinary `f64` function
//! over the same texel grid the shader would rasterize — a CPU reference
//! implementation, the same role `crate::materials::noise` plays for the
//! surface-texture GLSL library. See [`luts`]'s module doc for exactly what
//! that reference does and does not model (no fp16 storage quantization),
//! and `docs/work-manifests/claude-of-duty-port/notes/sky.md` for what a real
//! GPU bake would still need on top of this.
pub mod atmosphere;
pub mod celestial;
pub mod luts;
pub mod noise;
