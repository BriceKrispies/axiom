//! The sculpture: a smooth-min blend of sphere SDFs, extracted by marching
//! cubes.
//!
//! `implicit_surface_mesh` takes a **filled lattice**, not a field function, for
//! the same reason `heightfield_mesh` takes heights: a callback across the
//! engine spine is forbidden, and a sampled input is a replayable input. So the
//! blend lives here, in the app, and what crosses the boundary is `cols * rows *
//! depth` floats plus an iso level.
//!
//! The blend is a **polynomial smooth minimum** of five sphere distances. A hard
//! `min` would give the union of five spheres with visible creases where they
//! meet; the smooth min fuses them into one organic body, which is the whole
//! point of putting a marching-cubes operator in a geometry library — it is the
//! only operator here that can produce a shape no sweep, loft or lathe can.
//!
//! Everything is a pure function of the resolution: same resolution, same field,
//! same mesh, byte for byte.

use axiom_math::Vec3;
use axiom_mesh::{Mesh, MeshResult};
use axiom_mesh_ops::{
    implicit_surface_mesh, DetailBudget, ImplicitSurfaceOptions, IsoValue, ScalarField,
};

/// The cube of space the field is sampled over, as a half-extent in metres.
const FIELD_HALF_EXTENT: f32 = 6.0;

/// How hard the union between two spheres is rounded. Larger fuses more.
const BLEND_RADIUS: f32 = 2.4;

/// The blob's spheres: centre `(x, y, z)` and radius, in field space.
const BLOBS: [[f32; 4]; 5] = [
    [0.0, -1.4, 0.0, 2.6],
    [1.9, 0.9, 0.4, 2.1],
    [-1.8, 0.7, -0.6, 1.9],
    [0.3, 2.9, 1.1, 1.6],
    [-0.6, 2.6, -1.5, 1.3],
];

/// The sculpture mesh, centred on its own origin.
pub fn sculpture_mesh(resolution: u32) -> MeshResult<Mesh> {
    let side = resolution.max(4);
    let spacing = 2.0 * FIELD_HALF_EXTENT / (side - 1) as f32;
    let values: Vec<f32> = (0..side * side * side)
        .map(|k| {
            let x = k % side;
            let y = (k / side) % side;
            let z = k / (side * side);
            field_at(
                Vec3::new(
                    -FIELD_HALF_EXTENT + x as f32 * spacing,
                    -FIELD_HALF_EXTENT + y as f32 * spacing,
                    -FIELD_HALF_EXTENT + z as f32 * spacing,
                ),
            )
        })
        .collect();
    ScalarField::new(values, side, side, side)
        .and_then(|field| IsoValue::new(0.0).map(|iso| (field, iso)))
        .and_then(|(field, iso)| {
            implicit_surface_mesh(
                &field,
                iso,
                ImplicitSurfaceOptions {
                    origin: Vec3::new(-FIELD_HALF_EXTENT, -FIELD_HALF_EXTENT, -FIELD_HALF_EXTENT),
                    spacing: Vec3::new(spacing, spacing, spacing),
                    budget: DetailBudget::default(),
                },
            )
        })
}

/// The signed field: negative inside the blended body, positive outside.
///
/// The fold is seeded with the *first* blob's distance rather than with an
/// infinity, because the polynomial smooth minimum multiplies its seed by a
/// weight that clamps to zero — and `inf * 0` is `NaN`, which would poison the
/// whole lattice.
fn field_at(point: Vec3) -> f32 {
    let mut value = 0.0;
    for (index, blob) in BLOBS.iter().enumerate() {
        let distance = point.distance(Vec3::new(blob[0], blob[1], blob[2])) - blob[3];
        value = if index == 0 {
            distance
        } else {
            smooth_min(value, distance)
        };
    }
    value
}

/// The polynomial smooth minimum of two signed distances.
fn smooth_min(a: f32, b: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / BLEND_RADIUS).clamp(0.0, 1.0);
    b * (1.0 - h) + a * h - BLEND_RADIUS * h * (1.0 - h)
}
