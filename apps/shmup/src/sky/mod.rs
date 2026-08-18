//! Ported from Claude-of-Duty `src/sky/` — the physical atmosphere model,
//! the three LUT bakes it backs, the sky's shared procedural noise, the
//! sun/moon ephemeris, the layered sky sample, the two cloud decks, the
//! night sky, and the volumetric fog/light-shaft physics.
//!
//! | this module         | source                  |
//! |----------------------|--------------------------|
//! | [`atmosphere`]       | `src/sky/atmosphere.js` |
//! | [`luts`]             | `src/sky/luts.js`       |
//! | [`noise`]            | `src/sky/noise.js`      |
//! | [`celestial`]        | `src/sky/celestial.js`  |
//! | [`dome`]              | `src/sky/dome.js`       |
//! | [`clouds`]            | `src/sky/clouds.js`     |
//! | [`stars`]             | `src/sky/stars.js`      |
//! | [`volumetrics`]       | `src/sky/volumetrics.js`|
//!
//! The source bakes its three atmosphere LUTs (transmittance, multiscatter,
//! sky-view) plus a 2x1 ambient probe, and draws the sky/cloud/star/fog
//! layers, as WebGL2 fragment shaders. This crate has no GPU/WGSL emission
//! path yet, so every `*_FRAG`/`*_GLSL` shader body is ported here as an
//! ordinary `f64` function over the same inputs the shader would read — a
//! CPU reference implementation, the same role `crate::materials::noise`
//! plays for the surface-texture GLSL library. See [`luts`]'s module doc for
//! exactly what that reference does and does not model (no fp16 storage
//! quantization), and `docs/work-manifests/shmup-port/notes/sky.md`
//! for what a real GPU bake would still need on top of this.
//!
//! **Not ported, in any of these modules:** the THREE.js-side plumbing each
//! source file also carries — `SkyDome`/`Volumetrics`' class wiring,
//! `SkyPass`/render-target/`fullScreenGeometry` (`fullscreen.js` in full),
//! `skRayFor`'s camera-matrix ray reconstruction, and `skSunVisibility`'s
//! cascade-shadow-map texture sampling. These are GPU/host-integration
//! concerns with no portable CPU computation to reference-implement; see
//! [`dome`]'s and [`volumetrics`]'s module docs for the boundary each draws.
pub mod atmosphere;
pub mod celestial;
pub mod clouds;
pub mod dome;
pub mod luts;
pub mod noise;
pub mod stars;
pub mod volumetrics;
