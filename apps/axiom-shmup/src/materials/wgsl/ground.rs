//! WGSL transcription of Claude-of-Duty `src/materials/glsl/surfaces-ground.js`.
//!
//! Ground surfaces: asphalt, sand, dirt, gravel. These are usually triplanar
//! or planar-projected and are the surfaces most prone to visible tiling, so
//! they carry strong low-frequency content that the macro-variation layer in
//! the material shader can push around.
//!
//! NYQUIST BUDGET (read this before adding a band). Every generator writes
//! `p = uv * 8`, so a term at `p * K` lays 8K cells across the bake, and at a
//! bake of N texels that is N/(8K) texels per cell. Under ~5 texels the cell is
//! not a feature, it is white noise: mip 0 shows salt-and-pepper dither and
//! mip 1 has already averaged it to a flat wash. That single mistake is what
//! made the whole street read as sandpaper at 3 m and as flat colour at 15 m.
//! All ground bakes are 1024, so K is capped at 24 (5.3 texels) and the
//! sub-millimetre read is delegated to the shared detail map, which is tiled
//! ten times finer and has the texel budget for it.

/// `ASPHALT` (`surfaces-ground.js:18-119`).
///
/// The source carries no per-block doc comment; the shared NYQUIST BUDGET
/// header (`surfaces-ground.js:1-16`) reproduced in this module's docs covers
/// all four ground blocks.
pub const ASPHALT: &str = include_str!("asphalt.wgsl");

/// `SAND` (`surfaces-ground.js:121-179`).
///
/// The source carries no per-block doc comment; the shared NYQUIST BUDGET
/// header (`surfaces-ground.js:1-16`) reproduced in this module's docs covers
/// all four ground blocks.
pub const SAND: &str = include_str!("sand.wgsl");

/// `DIRT` (`surfaces-ground.js:181-248`).
///
/// The source carries no per-block doc comment; the shared NYQUIST BUDGET
/// header (`surfaces-ground.js:1-16`) reproduced in this module's docs covers
/// all four ground blocks.
pub const DIRT: &str = include_str!("dirt.wgsl");

/// `GRAVEL` (`surfaces-ground.js:250-366`).
///
/// The source carries no per-block doc comment; the shared NYQUIST BUDGET
/// header (`surfaces-ground.js:1-16`) reproduced in this module's docs covers
/// all four ground blocks. The two long block comments inside the body (the
/// "this is the street" note and the "AO IS THE WHOLE BALLGAME" note) are
/// preserved verbatim inside the WGSL, as line comments.
pub const GRAVEL: &str = include_str!("gravel.wgsl");
