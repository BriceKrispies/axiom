//! The building: an L-shaped footprint extruded into floors.
//!
//! The footprint is deliberately **concave**. A convex outline would prove
//! nothing about `extrude`, because a convex polygon triangulates the same way
//! under any algorithm; an L needs real ear clipping, and its reflex corner is
//! where a wrong-winding or a fan triangulation shows up immediately. This is
//! the one object in the crucible whose job is to stress
//! `triangulate_profile` through the cap path.
//!
//! ## Why floors are separate extrusions
//!
//! `extrude` sweeps a profile along `+Z` into a solid. A building wants its
//! footprint in the XZ plane rising along `+Y`, so each floor is extruded and
//! then laid down by a quarter turn about `+X`. Stacking N of those — each
//! slightly inset, with a slab gap between them — gives a tower whose floor
//! count is a variant parameter, which is exactly the kind of parameter change
//! the topology proof wants to observe.

use axiom_math::{Quat, Transform, Vec2, Vec3};
use axiom_mesh::{combine, transform, Mesh, MeshError, MeshErrorCode, MeshResult};
use axiom_mesh_ops::{extrude, CapPolicy, Profile};

use crate::quantities::meters;
use crate::variant::DetailParams;

/// The L-shaped footprint, authored counter-clockwise about its own origin.
/// The reflex corner is the fourth point.
const FOOTPRINT: [[f32; 2]; 6] = [
    [-7.0, -9.0],
    [7.0, -9.0],
    [7.0, -1.0],
    [-1.0, -1.0],
    [-1.0, 9.0],
    [-7.0, 9.0],
];

/// The height of one extruded floor slab, in metres.
const FLOOR_HEIGHT: f32 = 3.4;
/// The vertical gap left between floors, so the stack reads as storeys.
const FLOOR_GAP: f32 = 0.5;
/// How much each successive floor is inset, as a fraction of the one below.
const FLOOR_INSET: f32 = 0.055;

/// The whole building, with its ground floor sitting at `y = 0`.
pub fn building_mesh(params: DetailParams) -> MeshResult<Mesh> {
    lay_down()
        .and_then(|orientation| {
            (0..params.building_floors.max(1))
                .map(|floor| floor_mesh(floor, orientation))
                .collect::<MeshResult<Vec<Mesh>>>()
        })
        .and_then(|floors| combine(&floors))
}

/// One extruded floor, inset and lifted to its storey.
fn floor_mesh(floor: u32, orientation: Quat) -> MeshResult<Mesh> {
    let scale = (1.0 - FLOOR_INSET * floor as f32).max(0.35);
    footprint()
        .map(|base| base.scaled(meters(scale)))
        .and_then(|profile| extrude(&profile, meters(FLOOR_HEIGHT), CapPolicy::Both))
        .and_then(|slab| {
            // The extrusion runs `0..FLOOR_HEIGHT` along `+Z`; laying it down
            // maps that onto `+Y`, so the lift is the storey's own base height.
            let lift = floor as f32 * (FLOOR_HEIGHT + FLOOR_GAP);
            transform(
                &slab,
                Transform::new(
                    Vec3::new(0.0, lift, 0.0),
                    orientation,
                    Vec3::new(1.0, 1.0, 1.0),
                )
                .to_matrix(),
            )
        })
}

/// The authored L, as a validated closed profile.
fn footprint() -> MeshResult<Profile> {
    Profile::closed(FOOTPRINT.iter().map(|p| Vec2::new(p[0], p[1])).collect())
}

/// The quarter turn about `+X` that takes an extrusion's `+Z` run onto `+Y`.
fn lay_down() -> MeshResult<Quat> {
    Quat::from_axis_angle(Vec3::UNIT_X, -core::f32::consts::FRAC_PI_2).map_err(|_| {
        MeshError::new(
            MeshErrorCode::DegenerateAxis,
            "the building's authored lay-down axis is a unit axis",
        )
    })
}
