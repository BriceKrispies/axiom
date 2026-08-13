//! The terrain: an analytic height array meshed by `heightfield_mesh`.
//!
//! `heightfield_mesh` is a **sampled-data** operator — it takes heights, not a
//! callback — so the interesting half of this file is the sampler, and the
//! sampler lives here in the app because *what the heights mean* is a domain
//! question the geometry layer is right to refuse.
//!
//! The height function is a sum of three sine products plus a radial basin. It
//! is deterministic, cheap, has no seed and no table: the same `(x, z)` gives
//! the same height in every process, on every platform, forever. That is what
//! lets the determinism test compare digests across two builds without pinning a
//! noise implementation.
//!
//! A **skirt** is requested so the terrain's border drops away instead of
//! showing the camera the underside of a floating sheet at the horizon.

use axiom_kernel::Meters;
use axiom_math::Vec3;
use axiom_mesh::{Mesh, MeshResult};
use axiom_mesh_ops::{heightfield_mesh, HeightfieldOptions, HeightfieldSamples, TriangleDiagonal};

use crate::quantities::meters;
use crate::variant::DetailParams;

/// How far the terrain reaches from the origin on each axis, in metres.
pub const TERRAIN_HALF_EXTENT: f32 = 96.0;

/// The base level the height function oscillates about.
const TERRAIN_BASE_Y: f32 = -2.0;

/// How far the border skirt is dropped.
const SKIRT_DEPTH: f32 = 14.0;

/// The terrain mesh, already in world space (a heightfield carries its own
/// origin, so it is placed by its options rather than by a scene transform).
pub fn terrain_mesh(params: DetailParams) -> MeshResult<Mesh> {
    let cells = params.terrain_cells.max(2);
    let side = cells + 1;
    let spacing = 2.0 * TERRAIN_HALF_EXTENT / cells as f32;
    let heights: Vec<Meters> = (0..side * side)
        .map(|k| {
            let col = (k % side) as f32;
            let row = (k / side) as f32;
            meters(height_at(
                -TERRAIN_HALF_EXTENT + col * spacing,
                -TERRAIN_HALF_EXTENT + row * spacing,
            ))
        })
        .collect();
    HeightfieldSamples::new(heights, side, side).and_then(|samples| {
        heightfield_mesh(
            &samples,
            HeightfieldOptions {
                origin: Vec3::new(-TERRAIN_HALF_EXTENT, TERRAIN_BASE_Y, -TERRAIN_HALF_EXTENT),
                spacing_x: meters(spacing),
                spacing_z: meters(spacing),
                diagonal: TriangleDiagonal::Forward,
                skirt_depth: Some(meters(SKIRT_DEPTH)),
            },
        )
    })
}

/// The world `y` of the terrain surface at `(x, z)` — what everything standing
/// on the ground is placed against, so nothing floats and nothing is buried.
pub fn ground_y(x: f32, z: f32) -> f32 {
    TERRAIN_BASE_Y + height_at(x, z)
}

/// The analytic height at a world `(x, z)`.
///
/// Three sine products at decreasing wavelength and amplitude give rolling
/// ground with some fine relief; the radial term scoops a shallow basin out of
/// the middle so the road and the buildings sit in a valley rather than on a
/// featureless plate.
pub fn height_at(x: f32, z: f32) -> f32 {
    let broad = 5.6 * (x * 0.021).sin() * (z * 0.017).cos();
    let medium = 2.1 * (x * 0.058 + 1.3).sin() * (z * 0.049).sin();
    let fine = 0.7 * (x * 0.15).cos() * (z * 0.131 + 0.6).sin();
    let radius = ((x * x + z * z) / (TERRAIN_HALF_EXTENT * TERRAIN_HALF_EXTENT)).min(1.0);
    let basin = -6.0 * (1.0 - radius);
    broad + medium + fine + basin
}
