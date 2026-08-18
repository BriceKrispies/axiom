//! Per-material `owSurface` generators (`src/materials/glsl/surfaces-*.js`),
//! one module per source file. Each function implements the
//! [`super::bake::SurfaceFn`] contract: `owSurface(uv) -> SurfaceSample`.

/// Architectural surfaces: concrete, concrete_floor, brick, plaster, tile
/// (`src/materials/glsl/surfaces-arch.js`).
pub mod arch;

/// Ground-plane surfaces: asphalt, sand, dirt, gravel
/// (`src/materials/glsl/surfaces-ground.js`).
pub mod ground;

/// Organic surfaces: wood, fabric, burlap, foliage, rubber, glass
/// (`src/materials/glsl/surfaces-organic.js`).
pub mod organic;

/// Metal surfaces: metal_rust, metal_painted, metal_brushed, corrugated
/// (`src/materials/glsl/surfaces-metal.js`).
pub mod metal;
