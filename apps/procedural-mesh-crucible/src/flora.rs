//! The trees: a tapered swept trunk plus a crown, four times, varied by index.
//!
//! A tree is the smallest honest test of *taper*. The trunk is one circular
//! profile swept along a leaning spline with `start_scale` well above
//! `end_scale`, which is the only way this library expresses "thick at the base,
//! thin at the tip" without authoring two profiles. The crown then rides the top
//! of that same spline — read off the curve rather than guessed at — so a tree
//! whose lean changes still wears its crown.
//!
//! **Every difference between the four trees is a pure function of the tree's
//! index.** There is no random number anywhere in this app; a "varied" scene
//! built on an unseeded generator would be a scene no test could pin.
//!
//! The crown alternates between `icosphere` and `capsule` by index parity, so
//! the row shows two different primitive families standing next to each other at
//! the same scale.

use axiom_math::{Mat4, Vec3};
use axiom_mesh::{combine, transform, Mesh, MeshResult};
use axiom_mesh_ops::{
    capsule, icosphere, sweep, CapPolicy, Profile, Rings, Samples, Segments, Subdivisions,
    SweepOptions,
};

use crate::curves::trunk_curve;
use crate::quantities::{meters, radians, ratio};
use crate::variant::DetailParams;

/// How many trees the scene grows.
pub const TREE_COUNT: u32 = 4;

/// Per-tree shape, indexed by tree number: trunk height, base radius, crown
/// radius. Authored, not generated — four trees is few enough that a table is
/// clearer than a formula.
const TREE_SHAPES: [[f32; 3]; TREE_COUNT as usize] = [
    [7.5, 0.72, 2.9],
    [5.6, 0.58, 2.2],
    [9.2, 0.88, 3.4],
    [6.4, 0.64, 2.5],
];

/// How much of its base radius the trunk keeps at the tip.
const TRUNK_TIP_FRACTION: f32 = 0.28;

/// One tree, centred on its own base.
pub fn tree_mesh(index: u32, params: DetailParams) -> MeshResult<Mesh> {
    let shape = TREE_SHAPES[(index % TREE_COUNT) as usize];
    trunk_curve(index, shape[0]).and_then(|path| {
        trunk_mesh(&path, shape[1], params)
            .and_then(|trunk| {
                crown_mesh(index, shape[2], params).map(|crown| (trunk, crown, path))
            })
            .and_then(|(trunk, crown, path)| {
                let top = crate::curves::point_on(&path, 1.0);
                transform(&crown, Mat4::translation(top)).map(|placed| vec![trunk, placed])
            })
            .and_then(|parts| combine(&parts))
    })
}

/// The trunk: a circle swept along the lean, tapering to a fraction of its base.
fn trunk_mesh(
    path: &axiom_math::Curve,
    base_radius: f32,
    params: DetailParams,
) -> MeshResult<Mesh> {
    Segments::new(params.ring_segments)
        .and_then(|segs| Profile::circle(meters(base_radius), segs))
        .and_then(|profile| {
            Samples::new(params.trunk_samples).map(|samples| (profile, samples))
        })
        .and_then(|(profile, samples)| {
            sweep(
                &profile,
                path,
                samples,
                SweepOptions {
                    caps: CapPolicy::Both,
                    twist: radians(0.0),
                    start_scale: ratio(1.0),
                    end_scale: ratio(TRUNK_TIP_FRACTION),
                    closed_path: false,
                    // The trunk climbs, so a `+Y` seed would be parallel to the
                    // first tangent and fall back to a library-chosen axis. `+X`
                    // is a real perpendicular for every trunk in this scene.
                    initial_reference: Vec3::UNIT_X,
                },
            )
        })
}

/// The crown: an icosphere on even-indexed trees, a capsule on odd-indexed ones.
fn crown_mesh(index: u32, radius: f32, params: DetailParams) -> MeshResult<Mesh> {
    let even = index % 2 == 0;
    even.then(|| {
        Subdivisions::new(params.icosphere_subdivisions)
            .and_then(|levels| icosphere(meters(radius), levels))
    })
    .unwrap_or_else(|| {
        Rings::new(params.sphere_rings)
            .and_then(|rings| Segments::new(params.ring_segments).map(|segs| (rings, segs)))
            .and_then(|(rings, segs)| {
                capsule(meters(radius * 0.72), meters(radius * 0.75), rings, segs)
            })
    })
}
