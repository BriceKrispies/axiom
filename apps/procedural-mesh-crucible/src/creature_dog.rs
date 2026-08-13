//! The dog: a quadruped assembled entirely from generic operators, and cut into
//! bones so it can run.
//!
//! A dog is a **semantic** shape, so it is built here, in the app, out of the
//! same domain-free operators everything else in this scene uses. Nothing
//! anatomical exists in `axiom-mesh-ops` and nothing anatomical should: a leg is
//! a tapered sweep, a rib cage is a loft, and the meaning of those words is the
//! app's business. The same goes for the *skeleton* — `axiom-mesh` knows about
//! triangles, not about hocks.
//!
//! ## Anatomy — which operator builds which part
//!
//! | Bone | Operator |
//! |---|---|
//! | pelvis / spine | one `loft` each, through the shared rump and chest halves of the same six placed circle sections; each section is non-uniformly scaled by its `LoftSection::placement`, which is what makes the chest deep and the waist narrow without authoring six different outlines |
//! | neck | tapered `sweep` of a circle along a two-point `Curve::polyline` |
//! | head | `icosphere` non-uniformly scaled by `transform` into an egg elongated along `-Z` |
//! | muzzle | tapered `sweep` protruding forward, `combine`d with the `uv_sphere` nose that caps it |
//! | ears ×2 | tapered `sweep`s |
//! | tail ×2 | two tapered `sweep`s along the two halves of one `Curve::catmull_rom`; Catmull-Rom is *local*, so the two sub-curves reproduce the original spline span for span |
//! | legs | tapered `sweep`s: two bones per front leg, **three** per hind leg (femur/tibia/metatarsus) so the hind stance is angled and the animal does not read as a table |
//! | paws ×4 | `rounded_box` |
//!
//! ## The split torso
//!
//! The torso is two lofts rather than one, sharing the station at `z = 0.22`, so
//! that `pelvis` and `spine` are real bones with a real joint between them. The
//! shared station makes the seam watertight and the interior caps are never
//! seen. Front legs hang off the spine and hind legs off the pelvis, which is
//! what lets the back flex under the gait.
//!
//! ## Pose and frame
//!
//! Facing `-Z`, feet on `y = 0`, roughly 0.9 units at the shoulder. Every number
//! below is an authored literal or arithmetic over authored literals — there is
//! no randomness here, so two builds of the same variant are byte-identical.
//!
//! ## Normals: generated per bone, and deliberately **not** welded first
//!
//! Each bone's geometry is run through `axiom_mesh::generate_normals`, which is
//! what makes the non-uniformly scaled skull shade correctly (a scaled normal is
//! not the scaled surface's normal). Nothing is welded beforehand: welding
//! compares positions only, so it would collapse the duplicated seam vertices
//! every swept and lofted part carries — destroying the UV seams for the sake of
//! a crease no viewer can see, since limbs interpenetrate rather than share a
//! boundary.

use axiom_math::{Curve, Mat4, Quat, Transform, Vec3};
use axiom_mesh::{
    combine, generate_normals, transform, Mesh, MeshError, MeshErrorCode, MeshResult,
};
use axiom_mesh_ops::{
    icosphere, loft, rounded_box, uv_sphere, CapPolicy, LoftOptions, LoftSection, Profile, Rings,
    Segments, Subdivisions,
};

use crate::creature_rig::{
    bone, bone_length, bone_tip, swept, CreatureRig, LimbChain, RigPart,
};
use crate::quantities::meters;
use crate::variant::{CrucibleVariant, DetailParams};

/// The torso's stations along the spine: `z`, centre height, half-width,
/// half-depth. The deep chest at `z = -0.28` and the narrow waist at `z = 0.22`
/// are the whole reason this is a loft and not a swept tube.
const TORSO_STATIONS: [[f32; 4]; 6] = [
    [-0.50, 0.66, 0.150, 0.160],
    [-0.28, 0.64, 0.210, 0.230],
    [-0.05, 0.65, 0.190, 0.200],
    [0.22, 0.67, 0.170, 0.175],
    [0.45, 0.69, 0.190, 0.190],
    [0.62, 0.70, 0.130, 0.130],
];

/// Which stations each half of the torso skins. They share station 3, so the
/// two lofts meet exactly.
const SPINE_STATIONS: (usize, usize) = (0, 4);
const PELVIS_STATIONS: (usize, usize) = (3, 6);

/// The two torso bones' pivots in creature space.
const PELVIS_PIVOT: Vec3 = Vec3::new(0.0, 0.670, 0.400);
const SPINE_PIVOT: Vec3 = Vec3::new(0.0, 0.650, -0.050);

/// The neck, muzzle and ear bones: pivot, tip, base radius, tip ratio.
const NECK_BONE: [f32; 8] = [0.000, 0.700, -0.440, 0.000, 0.900, -0.660, 0.115, 0.80];
const MUZZLE_BONE: [f32; 8] = [0.000, 0.905, -0.800, 0.000, 0.875, -1.000, 0.072, 0.72];
const EAR_BONE: [f32; 8] = [0.062, 1.000, -0.700, 0.108, 1.130, -0.665, 0.048, 0.30];

/// The skull's centre and its non-uniform scale — an egg elongated along `-Z`.
const SKULL_CENTRE: Vec3 = Vec3::new(0.0, 0.930, -0.720);
const SKULL_SCALE: Vec3 = Vec3::new(0.105, 0.100, 0.140);

/// The nose ball capping the muzzle.
const NOSE_CENTRE: Vec3 = Vec3::new(0.0, 0.872, -1.012);
const NOSE_RADIUS: f32 = 0.036;

/// One side's leg bones: `from(x, y, z)`, `to(x, y, z)`, base radius, tip
/// ratio. Each is emitted at `+x` and `-x`. The first two are the front leg
/// (near vertical, sloping slightly back); the last three are the hind leg,
/// whose femur runs forward-and-down, tibia back-and-down to the hock, and
/// metatarsus down to the paw.
const LEG_BONES: [[f32; 8]; 5] = [
    [0.135, 0.620, -0.300, 0.145, 0.360, -0.270, 0.070, 0.79],
    [0.145, 0.360, -0.270, 0.148, 0.070, -0.280, 0.055, 0.76],
    [0.130, 0.660, 0.400, 0.145, 0.420, 0.280, 0.082, 0.76],
    [0.145, 0.420, 0.280, 0.150, 0.220, 0.450, 0.062, 0.73],
    [0.150, 0.220, 0.450, 0.150, 0.065, 0.430, 0.045, 0.85],
];

/// Where the four paws sit: `(x, z)`. `y` is the paw's own half-height, so the
/// box's underside lands exactly on the ground plane.
const PAW_MOUNTS: [[f32; 2]; 4] = [
    [0.148, -0.310],
    [-0.148, -0.310],
    [0.150, 0.420],
    [-0.150, 0.420],
];

/// The paw block's half-extents.
const PAW_HALF_EXTENTS: [f32; 3] = [0.055, 0.038, 0.078];

/// The tail's control points. Catmull-Rom reaches only its interior points, so
/// the first and last are shaping handles: the tail runs from `(0, 0.74, 0.66)`
/// up and back to `(0, 1.02, 1.06)`.
const TAIL_CONTROLS: [[f32; 3]; 6] = [
    [0.00, 0.700, 0.550],
    [0.00, 0.740, 0.660],
    [0.00, 0.880, 0.850],
    [0.00, 1.000, 0.980],
    [0.00, 1.020, 1.060],
    [0.00, 1.000, 1.140],
];

/// Where the split falls in the tail's taper. The base bone covers one of the
/// spline's three spans, so it carries a third of the shrink.
const TAIL_MID_SCALE: f32 = 0.7667;
const TAIL_TIP_SCALE: f32 = 0.30;
const TAIL_RADIUS: f32 = 0.055;

/// The bone names, in rig order. Public so the pose pass and the scene can
/// address a bone without re-deriving the anatomy.
pub const DOG_PELVIS: &str = "dog-pelvis";
pub const DOG_SPINE: &str = "dog-spine";

/// The four legs, front pair first, in the order [`dog_limbs`] returns them.
const LEG_NAMES: [[&str; 4]; 4] = [
    ["dog-fore-l-upper", "dog-fore-l-lower", "", "dog-fore-l-paw"],
    ["dog-fore-r-upper", "dog-fore-r-lower", "", "dog-fore-r-paw"],
    [
        "dog-hind-l-femur",
        "dog-hind-l-tibia",
        "dog-hind-l-meta",
        "dog-hind-l-paw",
    ],
    [
        "dog-hind-r-femur",
        "dog-hind-r-tibia",
        "dog-hind-r-meta",
        "dog-hind-r-paw",
    ],
];

/// The **diagonal trot**: a foreleg and the opposite hind leg swing together.
/// Anything else — pacing (same-side pairs) or four legs in phase — reads as a
/// pantomime horse, and the only thing that makes it a trot is these numbers.
const LEG_OFFSETS: [f32; 4] = [0.0, 0.5, 0.5, 0.0];

/// The whole dog as one mesh, in its own local space: facing `-Z`, feet on
/// `y = 0`.
///
/// Derived from [`dog_parts`] rather than authored separately, so the combined
/// shape and the rigged one can never disagree about where a shoulder is.
pub fn dog(variant: CrucibleVariant) -> MeshResult<Mesh> {
    dog_parts(variant)?.assembled(Transform::IDENTITY)
}

/// The dog as named bones, each authored in its own local space with the origin
/// at its joint pivot.
pub fn dog_parts(variant: CrucibleVariant) -> MeshResult<CreatureRig> {
    let params = variant.params();
    let mut parts: Vec<RigPart> = Vec::new();

    parts.push(RigPart {
        name: DOG_PELVIS,
        mesh: torso_half(PELVIS_STATIONS, PELVIS_PIVOT, params)?,
        parent: None,
        rest: Transform::from_translation(PELVIS_PIVOT),
    });
    parts.push(RigPart {
        name: DOG_SPINE,
        mesh: torso_half(SPINE_STATIONS, SPINE_PIVOT, params)?,
        parent: Some(0),
        rest: Transform::from_translation(SPINE_PIVOT),
    });
    parts.push(bone("dog-neck", Some(1), &NECK_BONE, 1.0, Vec3::UNIT_Y, params)?);
    parts.push(RigPart {
        name: "dog-head",
        mesh: skull(params)?,
        parent: Some(2),
        rest: Transform::from_translation(SKULL_CENTRE),
    });
    parts.push(muzzle(params)?);
    parts.push(bone("dog-ear-l", Some(3), &EAR_BONE, 1.0, Vec3::UNIT_Z, params)?);
    parts.push(bone("dog-ear-r", Some(3), &EAR_BONE, -1.0, Vec3::UNIT_Z, params)?);
    parts.extend(tail(params)?);
    parts.extend(legs(params)?);

    CreatureRig::from_creature_space(parts)
}

/// The four legs as solvable chains, front pair first, left before right.
pub fn dog_limbs() -> [LimbChain; 4] {
    // Every offset is derived from the same authored bone rows the geometry is
    // built from, so a moved joint moves the solve with it.
    let front = |slot: usize, side: f32| {
        let contact = paw_centre(0);
        LimbChain {
            upper: LEG_NAMES[slot][0],
            lower: LEG_NAMES[slot][1],
            extra: None,
            tip: LEG_NAMES[slot][3],
            contact,
            tip_offset: bone_tip(&LEG_BONES[1]).subtract(contact),
            ankle_offset: Vec3::ZERO,
            len_upper: bone_length(&LEG_BONES[0]),
            len_lower: bone_length(&LEG_BONES[1]),
            len_extra: 0.0,
            // A dog's elbow points BACKWARD — the front leg folds the opposite
            // way to the hind one, and a forward-bending elbow is the single
            // most obvious way to make a quadruped look wrong.
            pole: Vec3::new(0.0, 0.0, 1.0),
            offset: LEG_OFFSETS[slot],
        }
        .mirrored(side)
    };
    let hind = |slot: usize, side: f32| {
        let contact = paw_centre(2);
        LimbChain {
            upper: LEG_NAMES[slot][0],
            lower: LEG_NAMES[slot][1],
            extra: Some(LEG_NAMES[slot][2]),
            tip: LEG_NAMES[slot][3],
            contact,
            // The pair solves down to the HOCK; the metatarsus below it is not
            // the solver's business.
            tip_offset: bone_tip(&LEG_BONES[3]).subtract(contact),
            ankle_offset: bone_tip(&LEG_BONES[4]).subtract(contact),
            len_upper: bone_length(&LEG_BONES[2]),
            len_lower: bone_length(&LEG_BONES[3]),
            len_extra: bone_length(&LEG_BONES[4]),
            // The stifle leads FORWARD; the hock below it then folds back,
            // which is the metatarsus's job rather than the solver's.
            pole: Vec3::new(0.0, 0.0, -1.0),
            offset: LEG_OFFSETS[slot],
        }
        .mirrored(side)
    };
    [front(0, 1.0), front(1, -1.0), hind(2, 1.0), hind(3, -1.0)]
}

/// A paw's contact centre in creature space.
fn paw_centre(slot: usize) -> Vec3 {
    Vec3::new(
        PAW_MOUNTS[slot][0],
        PAW_HALF_EXTENTS[1],
        PAW_MOUNTS[slot][1],
    )
}

/// One half of the torso: the named station range, skinned and re-based onto
/// `pivot`.
fn torso_half(range: (usize, usize), pivot: Vec3, params: DetailParams) -> MeshResult<Mesh> {
    let segments = Segments::new(params.ring_segments.max(3))?;
    let unit = Profile::circle(meters(1.0), segments)?;
    let sections: Vec<LoftSection> = TORSO_STATIONS[range.0..range.1]
        .iter()
        .map(|station| LoftSection {
            profile: unit.clone(),
            placement: Transform::new(
                Vec3::new(-pivot.x, station[1] - pivot.y, station[0] - pivot.z),
                Quat::IDENTITY,
                Vec3::new(station[2], station[3], 1.0),
            ),
        })
        .collect();
    let skin = loft(
        &sections,
        LoftOptions {
            caps: CapPolicy::Both,
            closed_loop: false,
        },
    )?;
    generate_normals(&skin)
}

/// The skull: a unit icosphere squashed into an egg — narrower than it is long,
/// so the head reads as a muzzle-forward animal head rather than a ball.
fn skull(params: DetailParams) -> MeshResult<Mesh> {
    let levels = Subdivisions::new(params.icosphere_subdivisions)?;
    let ball = icosphere(meters(1.0), levels)?;
    generate_normals(&transform(
        &ball,
        Transform::new(Vec3::ZERO, Quat::IDENTITY, SKULL_SCALE).to_matrix(),
    )?)
}

/// The muzzle, with the nose ball that caps it folded in. The nose is placed by
/// pulling its authored creature-space centre back through the muzzle's own rest
/// transform, so it lands exactly where it always did rather than approximately
/// down the bone's axis.
fn muzzle(params: DetailParams) -> MeshResult<RigPart> {
    let mut part = bone("dog-muzzle", Some(3), &MUZZLE_BONE, 1.0, Vec3::UNIT_Y, params)?;
    let rings = Rings::new((params.sphere_rings / 2).max(2))?;
    let segments = Segments::new((params.sphere_segments / 2).max(3))?;
    let ball = uv_sphere(meters(NOSE_RADIUS), rings, segments)?;
    let local = part.rest.inverse().map_err(|_| {
        MeshError::new(
            MeshErrorCode::InvalidParameter,
            "the muzzle's rest placement is a rigid transform and inverts",
        )
    })?;
    let nose = transform(
        &ball,
        Mat4::translation(local.transform_point(NOSE_CENTRE)),
    )?;
    part.mesh = generate_normals(&combine(&[part.mesh, nose])?)?;
    Ok(part)
}

/// The tail, in two bones. The two sub-curves are drawn from the *same* control
/// list as the original single spline: Catmull-Rom is local — each span depends
/// only on the four controls around it — so the pair reproduces the original
/// curve span for span rather than approximating it.
fn tail(params: DetailParams) -> MeshResult<Vec<RigPart>> {
    let point = |index: usize| {
        Vec3::new(
            TAIL_CONTROLS[index][0],
            TAIL_CONTROLS[index][1],
            TAIL_CONTROLS[index][2],
        )
    };
    let base_pivot = point(1);
    let tip_pivot = point(2);
    let shifted = |controls: &[usize], pivot: Vec3| -> MeshResult<Curve> {
        Curve::catmull_rom(
            controls
                .iter()
                .map(|index| point(*index).subtract(pivot))
                .collect(),
        )
        .map_err(|_| invalid_path("the authored tail spline is a valid Catmull-Rom curve"))
    };
    Ok(vec![
        RigPart {
            name: "dog-tail-base",
            mesh: swept(
                &shifted(&[0, 1, 2, 3], base_pivot)?,
                TAIL_RADIUS,
                1.0,
                TAIL_MID_SCALE,
                Vec3::UNIT_X,
                params,
            )?,
            parent: Some(0),
            rest: Transform::from_translation(base_pivot),
        },
        RigPart {
            name: "dog-tail-tip",
            mesh: swept(
                &shifted(&[1, 2, 3, 4, 5], tip_pivot)?,
                TAIL_RADIUS * TAIL_MID_SCALE,
                1.0,
                TAIL_TIP_SCALE / TAIL_MID_SCALE,
                Vec3::UNIT_X,
                params,
            )?,
            parent: Some(7),
            rest: Transform::from_translation(tip_pivot),
        },
    ])
}

/// Every leg bone and paw, in the order [`dog_limbs`] names them. Front legs
/// hang off the spine, hind legs off the pelvis.
fn legs(params: DetailParams) -> MeshResult<Vec<RigPart>> {
    // The indices the leg bones parent onto, filled as the vector grows: each
    // leg's chain hangs off the torso and then off itself.
    const SPINE: usize = 1;
    const PELVIS: usize = 0;
    const FIRST_LEG_PART: usize = 9;
    let mut parts: Vec<RigPart> = Vec::new();
    let mut base = FIRST_LEG_PART;
    for (slot, side) in [(0usize, 1.0_f32), (1, -1.0), (2, 1.0), (3, -1.0)] {
        let front = slot < 2;
        let bones: &[usize] = if front { &[0, 1] } else { &[2, 3, 4] };
        let root = if front { SPINE } else { PELVIS };
        let names = LEG_NAMES[slot];
        for (rung, spec) in bones.iter().enumerate() {
            let parent = if rung == 0 { root } else { base + rung - 1 };
            parts.push(bone(
                names[rung],
                Some(parent),
                &LEG_BONES[*spec],
                side,
                // Every leg bone is near-vertical, so the sagittal axis is a
                // real perpendicular for the sweep's transport frame.
                Vec3::UNIT_Z,
                params,
            )?);
        }
        parts.push(paw(names[3], base + bones.len() - 1, slot, params)?);
        base += bones.len() + 1;
    }
    Ok(parts)
}

/// One rounded paw, resting exactly on `y = 0`.
fn paw(
    name: &'static str,
    parent: usize,
    slot: usize,
    params: DetailParams,
) -> MeshResult<RigPart> {
    let segments = Segments::new((params.ring_segments / 5).clamp(3, 6))?;
    let half = Vec3::new(
        PAW_HALF_EXTENTS[0],
        PAW_HALF_EXTENTS[1],
        PAW_HALF_EXTENTS[2],
    );
    Ok(RigPart {
        name,
        mesh: generate_normals(&rounded_box(half, meters(0.026), segments)?)?,
        parent: Some(parent),
        rest: Transform::from_translation(paw_centre(slot)),
    })
}

/// A curve failure, reported as a geometry failure.
fn invalid_path(message: &'static str) -> MeshError {
    MeshError::new(MeshErrorCode::InvalidPath, message)
}
