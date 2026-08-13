//! The reference row: every primitive the library ships, once each, in a line.
//!
//! This is the crucible's index page. The composed objects elsewhere in the
//! scene prove the operators *compose*; this row proves each primitive exists,
//! generates, validates, and is recognisable on its own — and it puts them at a
//! common scale, side by side, where a wrong radius convention or an inside-out
//! winding is obvious rather than hidden inside a tree.
//!
//! The order is the order the row is drawn in, left to right, and it matches the
//! legend on the page. Adding a primitive to the library means adding a row
//! entry: if the row does not grow, the crucible has stopped being a crucible.

use axiom_math::Vec3;
use axiom_mesh::{Mesh, MeshResult};
use axiom_mesh_ops::{
    annulus, box_mesh, capsule, cone, cube, cylinder, disk, frustum, grid, icosphere, quad,
    rounded_box, torus, triangle, uv_sphere, CapPolicy, Rings, Segments, Subdivisions,
};

use crate::quantities::meters;
use crate::variant::DetailParams;

/// One entry in the reference row: its object name and the operator that built
/// it. The mesh builder is selected by the same index, in [`primitive_mesh`].
pub const PRIMITIVES: [(&str, &str); 14] = [
    ("prim-cube", "mesh_ops::cube"),
    ("prim-box", "mesh_ops::box_mesh"),
    ("prim-rounded-box", "mesh_ops::rounded_box"),
    ("prim-uv-sphere", "mesh_ops::uv_sphere"),
    ("prim-icosphere", "mesh_ops::icosphere"),
    ("prim-cylinder", "mesh_ops::cylinder"),
    ("prim-cone", "mesh_ops::cone"),
    ("prim-frustum", "mesh_ops::frustum"),
    ("prim-capsule", "mesh_ops::capsule"),
    ("prim-torus", "mesh_ops::torus"),
    ("prim-disk", "mesh_ops::disk"),
    ("prim-annulus", "mesh_ops::annulus"),
    ("prim-quad", "mesh_ops::quad"),
    ("prim-grid", "mesh_ops::grid"),
];

/// The fifteenth entry: `triangle`, kept separate only because it is the one
/// primitive with no size parameter at all — it *is* its three corners.
pub const TRIANGLE_ENTRY: (&str, &str) = ("prim-triangle", "mesh_ops::triangle");

/// Build the primitive at `index` in [`PRIMITIVES`].
pub fn primitive_mesh(index: usize, params: DetailParams) -> MeshResult<Mesh> {
    let segs = Segments::new(params.ring_segments);
    let sphere_segs = Segments::new(params.sphere_segments);
    let rings = Rings::new(params.sphere_rings);
    let subdivisions = Subdivisions::new(params.icosphere_subdivisions);
    match index {
        0 => cube(meters(1.6)),
        1 => box_mesh(Vec3::new(1.9, 0.9, 1.2)),
        2 => segs.and_then(|s| rounded_box(Vec3::new(1.5, 1.0, 1.1), meters(0.4), s)),
        3 => rings.and_then(|r| sphere_segs.and_then(|s| uv_sphere(meters(1.5), r, s))),
        4 => subdivisions.and_then(|d| icosphere(meters(1.5), d)),
        5 => segs.and_then(|s| cylinder(meters(1.1), meters(1.5), s, CapPolicy::Both)),
        6 => segs.and_then(|s| cone(meters(1.3), meters(1.6), s, CapPolicy::Start)),
        7 => segs.and_then(|s| frustum(meters(1.4), meters(0.6), meters(1.5), s, CapPolicy::Both)),
        8 => rings.and_then(|r| segs.and_then(|s| capsule(meters(0.85), meters(0.9), r, s))),
        9 => segs.and_then(|s| torus(meters(1.2), meters(0.42), s, s)),
        10 => segs.and_then(|s| disk(meters(1.6), s)),
        11 => segs.and_then(|s| annulus(meters(0.7), meters(1.6), s)),
        12 => quad(meters(1.6), meters(1.6)),
        _ => segs.and_then(|s| grid(meters(1.7), meters(1.7), s, s)),
    }
}

/// The lone triangle, standing on its own at the end of the row.
pub fn triangle_mesh() -> MeshResult<Mesh> {
    triangle(
        Vec3::new(-1.6, -1.2, 0.0),
        Vec3::new(1.6, -1.2, 0.0),
        Vec3::new(0.0, 1.7, 0.0),
    )
}
