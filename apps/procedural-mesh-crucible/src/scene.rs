//! The scene: every generated object, where it stands, and what colour it is.
//!
//! This is the one place that knows the crucible is a *place* rather than a list
//! of meshes. Each builder module produces geometry in its own local space and
//! knows nothing about the others; this file gives each of them a name, an
//! operator credit, a world placement and a colour, and hands back one ordered
//! `Vec<CrucibleObject>`.
//!
//! It is a **pure function of the variant**. No clock, no randomness, no
//! environment: the same variant produces the same objects in the same order
//! with the same geometry, which is what the determinism test relies on and what
//! makes the digest of the whole scene a meaningful fingerprint.

use axiom_math::{Quat, Transform, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult};

use crate::building::building_mesh;
use crate::creature_rig::{aim, CreatureRig};
use crate::curves::{heading_on, point_on, road_curve, tunnel_curve};
use crate::locomotion::{LoopPath, HUMAN_LAG};
use crate::detail_ladder::{
    icosphere_rung, ladder_base, ladder_reduced, ladder_refined, ICOSPHERE_RUNGS,
};
use crate::flora::{tree_mesh, TREE_COUNT};
use crate::object::CrucibleObject;
use crate::primitive_row::{primitive_mesh, triangle_mesh, PRIMITIVES, TRIANGLE_ENTRY};
use crate::road::{road_mesh, tunnel_mesh};
use crate::sculpture::sculpture_mesh;
use crate::terrain::{ground_y, terrain_mesh};
use crate::variant::CrucibleVariant;
use crate::vehicle::vehicle_mesh;

/// Per-tree names and the parameter along the road each tree stands beside,
/// with its lateral offset from the road centre.
const TREE_PLACEMENTS: [(&str, f32, f32); TREE_COUNT as usize] = [
    ("tree-0", 0.14, -13.0),
    ("tree-1", 0.30, 12.5),
    ("tree-2", 0.78, -15.5),
    ("tree-3", 0.90, 11.0),
];

/// The four generated-density rungs' names, matching [`ICOSPHERE_RUNGS`].
const RUNG_NAMES: [&str; 4] = [
    "lod-icosphere-0",
    "lod-icosphere-1",
    "lod-icosphere-2",
    "lod-icosphere-3",
];

/// Where the reference primitive row stands, and how far apart its entries are.
const ROW_Z: f32 = -74.0;
const ROW_SPACING: f32 = 5.4;
const ROW_CLEARANCE: f32 = 2.2;

/// Where the detail ladder stands.
const LADDER_Z: f32 = -60.0;
const LADDER_SPACING: f32 = 6.2;
const LADDER_CLEARANCE: f32 = 2.4;

/// The whole crucible scene, built at `variant`.
///
/// Every object in the returned vector was produced by an `axiom-mesh-ops`
/// operator and validated by `axiom-mesh` on the way out of it; nothing here
/// hand-writes a vertex.
pub fn crucible_meshes(variant: CrucibleVariant) -> MeshResult<Vec<CrucibleObject>> {
    crucible_scene(variant).map(|scene| scene.objects)
}

/// The whole crucible: every object to spawn, plus the two creature rigs those
/// objects' creature entries came from.
///
/// The rigs are handed back rather than rebuilt because the animation needs the
/// *same* bones the scene drew — the parent indices, the rest transforms and the
/// bone order all have to line up with the entities that were spawned, and
/// re-deriving them is exactly the kind of quiet drift this app exists to
/// prevent.
#[derive(Debug, Clone)]
pub struct CrucibleScene {
    /// Every generated object, in spawn order.
    pub objects: Vec<CrucibleObject>,
    /// The index in `objects` of the dog's first bone. Its bones run
    /// contiguously from there, in rig order, and the human's follow them.
    pub dog_first: usize,
    /// The dog's rig.
    pub dog: CreatureRig,
    /// The human's rig.
    pub human: CreatureRig,
}

/// Build the whole crucible.
pub fn crucible_scene(variant: CrucibleVariant) -> MeshResult<CrucibleScene> {
    let params = variant.params();
    let mut objects: Vec<CrucibleObject> = Vec::new();

    let road_path = road_curve()?;
    objects.push(CrucibleObject::new(
        "terrain",
        "mesh_ops::heightfield_mesh (analytic sine sum + skirt)",
        terrain_mesh(params)?,
        Transform::IDENTITY,
        [0.20, 0.30, 0.17],
    ));
    objects.push(CrucibleObject::new(
        "road",
        "mesh_ops::sweep (slab profile along Curve::catmull_rom, twist bank)",
        road_mesh(&road_path, params)?,
        Transform::IDENTITY,
        [0.16, 0.16, 0.19],
    ));
    objects.push(CrucibleObject::new(
        "tunnel",
        "mesh_ops::sweep (closed arch profile, CapPolicy::None)",
        tunnel_mesh(&tunnel_curve(&road_path)?, params)?,
        Transform::IDENTITY,
        [0.42, 0.38, 0.34],
    ));
    objects.push(vehicle_object(&road_path, params)?);
    trees(&road_path, params, &mut objects)?;
    objects.push(building_object(params)?);
    objects.push(sculpture_object(params)?);
    primitive_row(params, &mut objects)?;
    detail_ladder(params, &mut objects)?;
    let dog_first = objects.len();
    let (dog, human) = creatures(variant, &mut objects)?;
    Ok(CrucibleScene {
        objects,
        dog_first,
        dog,
        human,
    })
}

/// The vehicle, standing on the road and pointing along it.
fn vehicle_object(
    road: &axiom_math::Curve,
    params: crate::variant::DetailParams,
) -> MeshResult<CrucibleObject> {
    const STATION: f32 = 0.24;
    let seat = point_on(road, STATION).add(Vec3::new(0.0, 1.05, 0.0));
    // The hull's nose is at local `-Z` and `look_rotation` maps local `+Z` onto
    // `-forward`, so handing it the road heading points the car down the road.
    let facing = Quat::look_rotation(heading_on(road, STATION), Vec3::UNIT_Y).map_err(|_| {
        MeshError::new(
            MeshErrorCode::DegenerateAxis,
            "the road heading at the vehicle's station is not vertical",
        )
    })?;
    Ok(CrucibleObject::new(
        "vehicle",
        "mesh_ops::loft (5 sections) + revolve (4 wheels) + box_mesh/cube, mesh::combine",
        vehicle_mesh(params)?,
        Transform::new(seat, facing, Vec3::ONE),
        [0.78, 0.22, 0.20],
    ))
}

/// Four trees, each standing on the ground beside its station on the road.
fn trees(
    road: &axiom_math::Curve,
    params: crate::variant::DetailParams,
    objects: &mut Vec<CrucibleObject>,
) -> MeshResult<()> {
    for (index, (name, station, offset)) in TREE_PLACEMENTS.iter().enumerate() {
        let centre = point_on(road, *station);
        let heading = heading_on(road, *station);
        // A horizontal perpendicular to the road at this station.
        let across = Vec3::new(heading.z, 0.0, -heading.x)
            .normalize()
            .unwrap_or(Vec3::UNIT_X);
        let base = centre.add(across.mul_scalar(*offset));
        objects.push(CrucibleObject::new(
            name,
            "mesh_ops::sweep (tapered trunk) + icosphere/capsule crown, mesh::combine",
            tree_mesh(index as u32, params)?,
            Transform::from_translation(Vec3::new(base.x, ground_y(base.x, base.z), base.z)),
            [0.24, 0.44 + 0.06 * (index % 3) as f32, 0.20],
        ));
    }
    Ok(())
}

/// The building, on the ground off the road's inside shoulder.
fn building_object(params: crate::variant::DetailParams) -> MeshResult<CrucibleObject> {
    const SITE: (f32, f32) = (-44.0, -16.0);
    // Sunk a little so the ground floor meets uneven terrain instead of hovering
    // over its low corner.
    let base = ground_y(SITE.0, SITE.1) - 1.2;
    Ok(CrucibleObject::new(
        "building",
        "mesh_ops::extrude (concave L footprint) x floors, mesh::combine",
        building_mesh(params)?,
        Transform::from_translation(Vec3::new(SITE.0, base, SITE.1)),
        [0.55, 0.50, 0.62],
    ))
}

/// The implicit sculpture, raised on the far shoulder where its silhouette
/// reads against the sky.
fn sculpture_object(params: crate::variant::DetailParams) -> MeshResult<CrucibleObject> {
    const SITE: (f32, f32) = (44.0, 8.0);
    Ok(CrucibleObject::new(
        "sculpture",
        "mesh_ops::implicit_surface_mesh (smooth-min of 5 sphere SDFs)",
        sculpture_mesh(params.field_resolution)?,
        Transform::from_translation(Vec3::new(SITE.0, ground_y(SITE.0, SITE.1) + 7.0, SITE.1)),
        [0.85, 0.62, 0.18],
    ))
}

/// The reference row: fourteen sized primitives plus the triangle, evenly spaced
/// and standing on the ground.
fn primitive_row(
    params: crate::variant::DetailParams,
    objects: &mut Vec<CrucibleObject>,
) -> MeshResult<()> {
    let count = PRIMITIVES.len() + 1;
    let span = (count - 1) as f32 * ROW_SPACING;
    for index in 0..count {
        // World `+X` presents on the LEFT of this engine's camera, so the row
        // is laid out in DECREASING x. Laying it out the natural way put
        // `prim-cube` on the right and the triangle on the left — a row that
        // read backwards against the page legend for no reason a viewer could
        // see. The mirror is a property of the projection, not of this scene,
        // so it is compensated for here, once, where the order is authored.
        let x = 0.5 * span - index as f32 * ROW_SPACING;
        let placement =
            Transform::from_translation(Vec3::new(x, ground_y(x, ROW_Z) + ROW_CLEARANCE, ROW_Z));
        let last = index == PRIMITIVES.len();
        let (name, operator) = if last {
            TRIANGLE_ENTRY
        } else {
            PRIMITIVES[index]
        };
        let mesh = if last {
            triangle_mesh()?
        } else {
            primitive_mesh(index, params)?
        };
        objects.push(CrucibleObject::new(
            name,
            operator,
            mesh,
            placement,
            row_color(index),
        ));
    }
    Ok(())
}

/// The detail ladder: four icosphere rungs, then base / refined / reduced.
fn detail_ladder(
    params: crate::variant::DetailParams,
    objects: &mut Vec<CrucibleObject>,
) -> MeshResult<()> {
    let entries: Vec<(&'static str, &'static str, Mesh)> = ICOSPHERE_RUNGS
        .iter()
        .enumerate()
        .map(|(slot, level)| {
            icosphere_rung(*level)
                .map(|mesh| (RUNG_NAMES[slot], "mesh_ops::icosphere (subdivisions 0..3)", mesh))
        })
        .chain([
            ladder_base(params).map(|mesh| {
                (
                    "lod-uv-sphere-base",
                    "mesh_ops::uv_sphere (the refine/reduce reference)",
                    mesh,
                )
            }),
            ladder_refined(params).map(|mesh| {
                (
                    "lod-loop-subdivided",
                    "mesh_ops::subdivide_loop over the reference sphere",
                    mesh,
                )
            }),
            ladder_reduced(params).map(|mesh| {
                (
                    "lod-quadric-simplified",
                    "mesh_ops::simplify_quadric (Fraction) over the reference sphere",
                    mesh,
                )
            }),
        ])
        .collect::<MeshResult<Vec<_>>>()?;
    let span = (entries.len() - 1) as f32 * LADDER_SPACING;
    for (slot, (name, operator, mesh)) in entries.into_iter().enumerate() {
        // Decreasing x, for the same screen-order reason as the row above.
        let x = 0.5 * span - slot as f32 * LADDER_SPACING;
        objects.push(CrucibleObject::new(
            name,
            operator,
            mesh,
            Transform::from_translation(Vec3::new(
                x,
                ground_y(x, LADDER_Z) + LADDER_CLEARANCE,
                LADDER_Z,
            )),
            ladder_color(slot),
        ));
    }
    Ok(())
}

/// A cool-to-warm sweep across the reference row, so neighbours never share a
/// colour and the row reads as a row.
fn row_color(index: usize) -> [f32; 3] {
    let t = index as f32 / 14.0;
    [0.24 + 0.62 * t, 0.46 + 0.22 * (1.0 - t), 0.88 - 0.60 * t]
}

/// The ladder's palette: the four generated rungs in one hue, the
/// refine/reduce trio in three distinct ones.
fn ladder_color(slot: usize) -> [f32; 3] {
    const LADDER_PALETTE: [[f32; 3]; 7] = [
        [0.30, 0.55, 0.85],
        [0.30, 0.62, 0.85],
        [0.30, 0.70, 0.85],
        [0.30, 0.78, 0.85],
        [0.86, 0.86, 0.86],
        [0.35, 0.88, 0.48],
        [0.92, 0.44, 0.72],
    ];
    LADDER_PALETTE[slot.min(LADDER_PALETTE.len() - 1)]
}

/// The operator credit both creatures' bones carry. Every bone in either rig
/// came out of this chain; which bone got which operator is documented, part by
/// part, in `creature_dog.rs` and `creature_human.rs`.
const DOG_OPERATORS: &str = "mesh_ops::loft (torso halves) + sweep (neck/muzzle/ears/legs/tail) + icosphere skull + uv_sphere nose + rounded_box paws, cut at the joints into a rig";
const HUMAN_OPERATORS: &str = "mesh_ops::loft (torso halves) + sweep (arms/legs) + icosphere head + cylinder neck + uv_sphere joints + rounded_box hands/feet, cut at the joints into a rig";

/// The dog's and the human's linear base colours.
const DOG_COLOR: [f32; 3] = [0.66, 0.44, 0.22];
const HUMAN_COLOR: [f32; 3] = [0.32, 0.60, 0.78];

/// The dog and the human, **as bones** — one scene object per bone, so the app
/// can re-author each one's instance transform per frame and the pair can run.
///
/// One draw per bone rather than one per creature is not a compromise, it is the
/// only thing that renders here: this machine falls back to WebGL2, which has no
/// vertex-stage storage buffers and therefore draws no skinned geometry at all.
/// A rigid part is an ordinary instanced draw.
///
/// The rest placement below is the pose the *native* scene tests digest; the
/// live app overwrites every one of these transforms on its first frame. Both
/// creatures are placed at the start of the loop they run
/// ([`crate::locomotion::LoopPath`]), so a headless build stands them exactly
/// where the animated build starts them.
///
/// Both are *compositions of the generic operators*, authored in this app: the
/// mesh layers know about lofts and sweeps, not about shoulders and muzzles.
fn creatures(
    variant: CrucibleVariant,
    objects: &mut Vec<CrucibleObject>,
) -> MeshResult<(CreatureRig, CreatureRig)> {
    let dog = crate::creature_dog::dog_parts(variant)?;
    let human = crate::creature_human::human_parts(variant)?;
    push_rig(&dog, DOG_OPERATORS, DOG_COLOR, 0.0, objects);
    push_rig(&human, HUMAN_OPERATORS, HUMAN_COLOR, HUMAN_LAG, objects);
    Ok((dog, human))
}

/// Spawn one rig's bones, resting at `lag` units back along the loop.
fn push_rig(
    rig: &CreatureRig,
    operator: &'static str,
    color: [f32; 3],
    lag: f32,
    objects: &mut Vec<CrucibleObject>,
) {
    let root = creature_root(-lag);
    let world = rig.rest_world(root);
    rig.parts()
        .iter()
        .zip(world)
        .for_each(|(part, placement)| {
            objects.push(CrucibleObject::new(
                part.name,
                operator,
                part.mesh.clone(),
                placement,
                color,
            ));
        });
}

/// A creature's rest placement `arc` units along the loop: on the terrain, at
/// the presentation scale, facing down the path.
///
/// This deliberately mirrors `CreaturePose::body` minus the gait — same path,
/// same facing convention, same scale — so the static scene and the first
/// animated frame agree about where a creature is.
fn creature_root(arc: f32) -> Transform {
    LoopPath::perimeter()
        .map(|path| {
            let here = path.at(arc);
            Transform::new(
                here.position,
                aim(here.forward, Vec3::UNIT_Y),
                Vec3::new(CREATURE_SCALE, CREATURE_SCALE, CREATURE_SCALE),
            )
        })
        .unwrap_or_else(|_| {
            Transform::new(
                Vec3::new(0.0, ground_y(0.0, 0.0), 0.0),
                Quat::IDENTITY,
                Vec3::new(CREATURE_SCALE, CREATURE_SCALE, CREATURE_SCALE),
            )
        })
}

/// The creatures are authored at life size (a 1.8-unit human, a 1.1-unit dog),
/// which is honest next to the reference primitives but leaves them a few pixels
/// tall from the framing camera. They are presented at 10x so the swept limbs,
/// lofted torsos and joint articulation are actually legible.
///
/// Scaling is uniform and about the creature's own origin, and both creatures
/// are authored with their soles on local `y = 0` — so a scaled creature still
/// stands exactly on the terrain, with no placement compensation needed. It is
/// also the scale [`crate::creature_pose::DOG_GAIT`] and its human counterpart
/// carry, and the two must agree.
const CREATURE_SCALE: f32 = 10.0;
