//! The vehicle: a lofted hull, four lathed wheels, and two box details, combined
//! into one mesh.
//!
//! This object is the crucible's answer to "can the constructive operators be
//! composed into a recognisable thing?" — and it is deliberately built from
//! three *different* families:
//!
//! - the **hull** is `loft` through five placed cross-sections, because a car
//!   body is authored as a stack of outlines and nothing else in the library
//!   interpolates between differing outlines;
//! - the **wheels** are `revolve` of a tube cross-section about `+X`, because a
//!   wheel is a silhouette spun about its axle and a lathe is exactly that;
//! - the **cabin and the spoiler** are `box_mesh` / `cube`, because a box is a
//!   box and reaching for a loft to make one would be theatre.
//!
//! The parts are placed with `axiom_mesh::transform` and merged with
//! `axiom_mesh::combine`, so the whole vehicle is a single registered mesh with
//! one material — the composition happens in geometry, at build time, not in the
//! scene graph every frame.

use axiom_math::{Mat4, Quat, Transform, Vec2, Vec3};
use axiom_mesh::{combine, transform, Mesh, MeshError, MeshErrorCode, MeshResult};
use axiom_mesh_ops::{box_mesh, cube, loft, revolve, CapPolicy, LoftOptions, LoftSection, Profile, Segments};

use crate::quantities::{meters, radians};
use crate::variant::DetailParams;

/// The hull's cross-section outline, in the section's own XY plane. Eight points,
/// the same count in every section — `loft` corresponds by index and rejects a
/// mismatch, which is the contract that keeps the skin from twisting.
const HULL_OUTLINE: [[f32; 2]; 8] = [
    [-1.00, -0.42],
    [-0.62, -0.72],
    [0.62, -0.72],
    [1.00, -0.42],
    [1.00, 0.36],
    [0.55, 0.70],
    [-0.55, 0.70],
    [-1.00, 0.36],
];

/// Each hull station: how far along `+Z` it sits, and how much the outline is
/// scaled there. Nose at `-Z`, tail at `+Z`.
const HULL_STATIONS: [[f32; 2]; 5] = [
    [-2.30, 0.46],
    [-1.05, 0.92],
    [0.15, 1.00],
    [1.35, 0.88],
    [2.45, 0.58],
];

/// Wheel geometry: outer radius, hub radius, and half the tread width.
const WHEEL_OUTER: f32 = 0.62;
const WHEEL_HUB: f32 = 0.20;
const WHEEL_HALF_WIDTH: f32 = 0.22;

/// Where the four wheels sit: `(x, z)`, at a fixed ride height.
const WHEEL_MOUNTS: [[f32; 2]; 4] = [
    [-1.05, -1.45],
    [1.05, -1.45],
    [-1.05, 1.55],
    [1.05, 1.55],
];
/// The wheel centres' height, below the hull's own centre line.
const WHEEL_Y: f32 = -0.58;

/// The whole vehicle as one mesh, centred on its own origin.
pub fn vehicle_mesh(params: DetailParams) -> MeshResult<Mesh> {
    hull_mesh()
        .and_then(|hull| wheel_set(params).map(|wheels| (hull, wheels)))
        .and_then(|(hull, wheels)| detail_set().map(|details| (hull, wheels, details)))
        .and_then(|(hull, wheels, details)| {
            let parts: Vec<Mesh> = core::iter::once(hull)
                .chain(wheels)
                .chain(details)
                .collect();
            combine(&parts)
        })
}

/// The hull: five scaled copies of one outline, skinned in order and capped.
fn hull_mesh() -> MeshResult<Mesh> {
    let outline: Vec<Vec2> = HULL_OUTLINE
        .iter()
        .map(|p| Vec2::new(p[0], p[1]))
        .collect();
    Profile::closed(outline).and_then(|base| {
        let sections: Vec<LoftSection> = HULL_STATIONS
            .iter()
            .map(|station| LoftSection {
                profile: base.scaled(meters(station[1])),
                placement: Transform::from_translation(Vec3::new(0.0, 0.0, station[0])),
            })
            .collect();
        loft(
            &sections,
            LoftOptions {
                caps: CapPolicy::Both,
                closed_loop: false,
            },
        )
    })
}

/// Four identical lathed wheels, each translated onto its mount.
fn wheel_set(params: DetailParams) -> MeshResult<Vec<Mesh>> {
    wheel_mesh(params).and_then(|wheel| {
        WHEEL_MOUNTS
            .iter()
            .map(|mount| {
                transform(
                    &wheel,
                    Mat4::translation(Vec3::new(mount[0], WHEEL_Y, mount[1])),
                )
            })
            .collect()
    })
}

/// One wheel: a rectangular tube cross-section — `(radius, height)` in the
/// half-plane containing the axle — revolved a full turn about `+X`.
fn wheel_mesh(params: DetailParams) -> MeshResult<Mesh> {
    Profile::closed(vec![
        Vec2::new(WHEEL_HUB, -WHEEL_HALF_WIDTH),
        Vec2::new(WHEEL_OUTER, -WHEEL_HALF_WIDTH),
        Vec2::new(WHEEL_OUTER, WHEEL_HALF_WIDTH),
        Vec2::new(WHEEL_HUB, WHEEL_HALF_WIDTH),
    ])
    .and_then(|profile| Segments::new(params.ring_segments).map(|segs| (profile, segs)))
    .and_then(|(profile, segs)| {
        revolve(
            &profile,
            Vec3::UNIT_X,
            radians(core::f32::consts::TAU),
            segs,
            CapPolicy::None,
        )
    })
}

/// The two box details: a cabin block on the hull's spine and a rear spoiler
/// blade raised on the tail.
fn detail_set() -> MeshResult<Vec<Mesh>> {
    box_mesh(Vec3::new(0.62, 0.34, 1.15))
        .and_then(|cabin| {
            transform(
                &cabin,
                Mat4::translation(Vec3::new(0.0, 0.62, 0.05)),
            )
        })
        .and_then(|cabin| spoiler_mesh().map(|spoiler| vec![cabin, spoiler]))
}

/// The spoiler: a unit cube stretched into a blade and tilted, so the vehicle
/// carries at least one part whose placement is a rotation rather than a
/// translation.
fn spoiler_mesh() -> MeshResult<Mesh> {
    cube(meters(0.5)).and_then(|blade| {
        Quat::from_axis_angle(Vec3::UNIT_X, -0.22)
            .map_err(|_| {
                MeshError::new(
                    MeshErrorCode::DegenerateAxis,
                    "the spoiler's authored tilt axis is a unit axis",
                )
            })
            .and_then(|tilt| {
                transform(
                    &blade,
                    Transform::new(
                        Vec3::new(0.0, 0.86, 2.45),
                        tilt,
                        Vec3::new(2.4, 0.16, 0.7),
                    )
                    .to_matrix(),
                )
            })
    })
}
