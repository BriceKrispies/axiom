//! WGSL transcription of Claude-of-Duty `src/materials/glsl/surfaces-arch.js`.
//!
//! From the source file's own header (`surfaces-arch.js:1-13`):
//!
//! > Architectural surfaces: concrete, brick, plaster, stucco, ceramic tile.
//! >
//! > Every surface implements:
//! >   `void owSurface(vec2 uv, out vec3 alb, out float h, out float rough,`
//! >   `               out float metal, out float ao)`
//! > 'uv' is [0,1) across the tile, 'h' is 0..1 (0.5 ≈ the nominal surface
//! > plane), 'alb' is LINEAR albedo (authored via `owSRGB()` so the numbers read
//! > like paint swatches), and 'ao' is a baked cavity term, not a lighting term.
//! >
//! > uSeed shifts the noise lattice so two variants of the same surface never
//! > line up. Shifting the argument of a periodic function keeps it periodic.
//!
//! Each `pub const` holds the WGSL transcription of one exported GLSL block's
//! `owSurface` body, in the source's order. WGSL has no `out` parameters, so
//! each body is wrapped in a `ptr<function, _>` signature with a `var` copy
//! prologue and a write-back epilogue; that wrapper is the only structural
//! change — every statement between them is line-for-line the source.
//!
//! Uniform renames: `uSeed` -> `U.seed`, `uTintA` -> `U.tint_a`,
//! `uTintB` -> `U.tint_b`, `uParam` -> `U.param`.
//!
//! One identifier rename: GLSL `macro` (in `CONCRETE` and `PLASTER`) is a WGSL
//! reserved word, so it is spelled `macro_` here. Flagged at each site.

/// `CONCRETE` (`surfaces-arch.js:15-179`) — cast concrete: pour variation,
/// exposed aggregate, the coarse sand fraction, bug holes, board-formed
/// formwork (`uParam.x`) or saw-cut control joints (`uParam.y`), structural
/// cracks, spalling, chips, and rain/soot/rust staining.
pub const CONCRETE: &str = include_str!("concrete.wgsl");

/// `BRICK` (`surfaces-arch.js:181-336`) — a running-bond brick wall: 6 bricks
/// across by 18 courses, per-brick jitter and kiln shade, a raked mortar joint
/// with a hard arris, face pores and broken arrises, then efflorescence, soot
/// runoff and hairline cracks over the whole wall.
pub const BRICK: &str = include_str!("brick.wgsl");

/// `PLASTER` (`surfaces-arch.js:338-499`) — trowelled plaster/stucco: sheared
/// trowel sweeps, skim-coat laps, the three 0.1-1 m weathering bands, the sand
/// tooth of the finish coat, pinholes, crazing and structural cracks, blown
/// patches down to the substrate, chipped flakes, water tide marks and mould.
pub const PLASTER: &str = include_str!("plaster.wgsl");

/// `TILE` (`surfaces-arch.js:501-563`) — a 6x6 grid of glazed ceramic tiles on
/// a flat grout bed with a hard arris, per-tile batch shade and glaze noise,
/// cracked/broken tiles showing the bed underneath, and traffic wear.
pub const TILE: &str = include_str!("tile.wgsl");
