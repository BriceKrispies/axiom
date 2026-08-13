//! The human figure: a biped assembled entirely from generic operators, and cut
//! into bones so it can run.
//!
//! Like the dog, this is a **semantic** shape and it therefore lives in the app.
//! `axiom-mesh-ops` has no torso, no limb and no head, and adding one would make
//! the geometry layer the junk drawer every future domain reaches into. What the
//! layer offers is a loft, a sweep and a handful of primitives; what "shoulder"
//! means is knowledge that stops here.
//!
//! ## Anatomy — which operator builds which part
//!
//! | Bone | Operator |
//! |---|---|
//! | pelvis / torso | one `loft` each, through the shared lower and upper halves of the same six placed circle sections, then rotated upright by `transform`; the section scales are what make the shoulders broader than the waist |
//! | neck | `cylinder` (already built about `+Y`, so it needs no rotation) |
//! | head | `icosphere` non-uniformly scaled by `transform` into an egg ≈ 0.24 tall — one seventh and a half of the figure's 1.8 |
//! | arms ×2 | two tapered `sweep`s each (upper arm, forearm), each `combine`d with the `uv_sphere` joint ball at its own pivot so the segments meet in a rounded joint instead of a mitre |
//! | legs ×2 | two tapered `sweep`s each (thigh, shin), likewise ball-jointed |
//! | hands ×2, feet ×2 | `rounded_box` blocks, so the silhouette terminates instead of tapering into nothing |
//!
//! ## The split torso
//!
//! The loft is cut at its third station into a `pelvis` and a `torso` sharing
//! that station, so the figure has a waist joint the gait can rotate. Legs hang
//! off the pelvis and arms off the torso, which is what lets the shoulders
//! counter-rotate against the hips instead of riding along with them.
//!
//! ## Pose and frame
//!
//! Facing `-Z`, feet on `y = 0`, exactly 1.8 units to the crown. Every number is
//! an authored literal: the figure is a pure function of the variant, with no
//! randomness anywhere, so two builds of the same variant are byte-identical.
//!
//! ## Normals: generated per bone, and deliberately **not** welded first
//!
//! Each bone is run through `axiom_mesh::generate_normals` — required here,
//! because the head and both torso halves are non-uniformly scaled and a scaled
//! normal is not the scaled surface's normal. Nothing is welded first: `weld`
//! compares positions only and would collapse the duplicated seam vertices that
//! every swept and lofted part carries, destroying the UV seams to smooth a
//! crease that does not exist — limbs interpenetrate their joint balls rather
//! than sharing a boundary with them.

use core::f32::consts::FRAC_PI_2;

use axiom_math::{Mat4, Quat, Transform, Vec3};
use axiom_mesh::{
    combine, generate_normals, transform, Mesh, MeshError, MeshErrorCode, MeshResult,
};
use axiom_mesh_ops::{
    cylinder, icosphere, loft, rounded_box, uv_sphere, CapPolicy, LoftOptions, LoftSection, Profile,
    Rings, Segments, Subdivisions,
};

use crate::creature_rig::{bone, bone_length, bone_tip, CreatureRig, LimbChain, RigPart};
use crate::quantities::meters;
use crate::variant::{CrucibleVariant, DetailParams};

/// Where the torso's hip section sits. The torso is lofted along its own `+Z`
/// (the only direction a loft runs) and then stood upright, so its stations are
/// heights above this.
const HIP_Y: f32 = 0.940;

/// Where the waist joint falls: the station both torso halves share.
const WAIST_RISE: f32 = 0.280;

/// The torso's stations: distance above the hips, half-width, half-depth.
/// Shoulders at `0.51` are the broadest, the waist at `0.16` the narrowest.
const TORSO_STATIONS: [[f32; 3]; 6] = [
    [0.000, 0.155, 0.105],
    [0.160, 0.135, 0.098],
    [0.280, 0.155, 0.108],
    [0.400, 0.180, 0.115],
    [0.510, 0.200, 0.105],
    [0.560, 0.150, 0.090],
];

/// Which stations each half skins. They share station 2, so the halves meet
/// exactly at the waist.
const PELVIS_STATIONS: (usize, usize) = (0, 3);
const TORSO_UPPER_STATIONS: (usize, usize) = (2, 6);

/// One side's limb bones: `from(x, y, z)`, `to(x, y, z)`, base radius, tip
/// ratio. Each is emitted at `+x` and `-x`. The elbow (`1.10`) sits a little
/// behind the shoulder and the wrist well forward of it, which is what a relaxed
/// arm does; the knee (`0.52`) sits forward of the hip–ankle line.
const LIMB_BONES: [[f32; 8]; 4] = [
    [0.195, 1.430, 0.000, 0.215, 1.100, -0.015, 0.052, 0.85],
    [0.215, 1.100, -0.015, 0.205, 0.780, -0.075, 0.045, 0.75],
    [0.085, 0.940, 0.000, 0.093, 0.520, -0.020, 0.084, 0.74],
    [0.093, 0.520, -0.020, 0.095, 0.080, 0.005, 0.062, 0.66],
];

/// The joint balls, one per limb bone and sitting at that bone's own pivot:
/// `(x, y, z)` and radius, mirrored across `x`.
const JOINTS: [[f32; 4]; 4] = [
    [0.195, 1.435, 0.000, 0.058],
    [0.215, 1.100, -0.015, 0.046],
    [0.085, 0.940, 0.000, 0.075],
    [0.093, 0.520, -0.020, 0.060],
];

/// The hands: centre `(x, y, z)` and half-extents, mirrored across `x`.
const HAND_CENTRE: [f32; 3] = [0.203, 0.735, -0.083];
const HAND_HALF_EXTENTS: [f32; 3] = [0.032, 0.052, 0.020];

/// The feet: centre `(x, z)` and half-extents. `y` is the half-height, so each
/// sole lands exactly on the ground plane, and the block runs forward along
/// `-Z` into a toe.
const FOOT_CENTRE: [f32; 2] = [0.095, -0.045];
const FOOT_HALF_EXTENTS: [f32; 3] = [0.045, 0.042, 0.105];

/// The neck cylinder.
const NECK_CENTRE: Vec3 = Vec3::new(0.0, 1.500, -0.005);
const NECK_RADIUS: f32 = 0.052;
const NECK_HEIGHT: f32 = 0.055;

/// The head's centre and the non-uniform scale that turns a unit icosphere into
/// an egg.
const HEAD_CENTRE: Vec3 = Vec3::new(0.0, 1.680, -0.005);
const HEAD_SCALE: Vec3 = Vec3::new(0.102, 0.120, 0.110);

/// The two torso bones' pivots in creature space.
const PELVIS_PIVOT: Vec3 = Vec3::new(0.0, HIP_Y, 0.0);
const TORSO_PIVOT: Vec3 = Vec3::new(0.0, HIP_Y + WAIST_RISE, 0.0);

/// The bone names of the four limbs, legs first, in the order
/// [`human_limbs`] returns them.
const LIMB_NAMES: [[&str; 3]; 4] = [
    ["human-leg-l-thigh", "human-leg-l-shin", "human-foot-l"],
    ["human-leg-r-thigh", "human-leg-r-shin", "human-foot-r"],
    ["human-arm-l-upper", "human-arm-l-lower", "human-hand-l"],
    ["human-arm-r-upper", "human-arm-r-lower", "human-hand-r"],
];

/// The gait offsets. The two legs are exactly half a cycle apart, and each arm
/// is half a cycle from the leg on its own side — the counter-swing that stops a
/// runner looking like they are marching.
const LIMB_OFFSETS: [f32; 4] = [0.0, 0.5, 0.5, 0.0];

/// The whole figure as one mesh, in its own local space: facing `-Z`, feet on
/// `y = 0`, crown at `y = 1.8`.
///
/// Derived from [`human_parts`], so the combined shape and the rigged one can
/// never disagree about where a shoulder is.
pub fn human(variant: CrucibleVariant) -> MeshResult<Mesh> {
    human_parts(variant)?.assembled(Transform::IDENTITY)
}

/// The figure as named bones, each authored in its own local space with the
/// origin at its joint pivot.
pub fn human_parts(variant: CrucibleVariant) -> MeshResult<CreatureRig> {
    let params = variant.params();
    let mut parts: Vec<RigPart> = Vec::new();

    parts.push(RigPart {
        name: "human-pelvis",
        mesh: torso_half(PELVIS_STATIONS, 0.0, params)?,
        parent: None,
        rest: Transform::from_translation(PELVIS_PIVOT),
    });
    parts.push(RigPart {
        name: "human-torso",
        mesh: torso_half(TORSO_UPPER_STATIONS, WAIST_RISE, params)?,
        parent: Some(0),
        rest: Transform::from_translation(TORSO_PIVOT),
    });
    parts.push(RigPart {
        name: "human-neck",
        mesh: neck(params)?,
        parent: Some(1),
        rest: Transform::from_translation(NECK_CENTRE),
    });
    parts.push(RigPart {
        name: "human-head",
        mesh: head(params)?,
        parent: Some(2),
        rest: Transform::from_translation(HEAD_CENTRE),
    });
    parts.extend(limbs(params)?);

    CreatureRig::from_creature_space(parts)
}

/// The four limbs as solvable chains: left leg, right leg, left arm, right arm.
pub fn human_limbs() -> [LimbChain; 4] {
    let leg = |slot: usize, side: f32| {
        let contact = Vec3::new(FOOT_CENTRE[0], FOOT_HALF_EXTENTS[1], FOOT_CENTRE[1]);
        LimbChain {
            upper: LIMB_NAMES[slot][0],
            lower: LIMB_NAMES[slot][1],
            extra: None,
            tip: LIMB_NAMES[slot][2],
            contact,
            tip_offset: bone_tip(&LIMB_BONES[3]).subtract(contact),
            ankle_offset: Vec3::ZERO,
            grounded: true,
            len_upper: bone_length(&LIMB_BONES[2]),
            len_lower: bone_length(&LIMB_BONES[3]),
            len_extra: 0.0,
            // A knee leads FORWARD.
            pole: Vec3::new(0.0, 0.0, -1.0),
            offset: LIMB_OFFSETS[slot],
        }
        .mirrored(side)
    };
    let arm = |slot: usize, side: f32| {
        let contact = Vec3::new(HAND_CENTRE[0], HAND_CENTRE[1], HAND_CENTRE[2]);
        LimbChain {
            upper: LIMB_NAMES[slot][0],
            lower: LIMB_NAMES[slot][1],
            extra: None,
            tip: LIMB_NAMES[slot][2],
            contact,
            tip_offset: bone_tip(&LIMB_BONES[1]).subtract(contact),
            ankle_offset: Vec3::ZERO,
            // An arm's hand is carried by the body, not planted on the ground.
            grounded: false,
            len_upper: bone_length(&LIMB_BONES[0]),
            len_lower: bone_length(&LIMB_BONES[1]),
            len_extra: 0.0,
            // An elbow leads BACKWARD — the opposite of the knee above it.
            pole: Vec3::new(0.0, 0.0, 1.0),
            offset: LIMB_OFFSETS[slot],
        }
        .mirrored(side)
    };
    [leg(0, 1.0), leg(1, -1.0), arm(2, 1.0), arm(3, -1.0)]
}

/// One half of the torso: the named station range, skinned lying along `+Z`,
/// stood upright, and re-based so `rise` is its own origin.
///
/// A loft runs along the direction its sections advance in, and there is no way
/// to stack sections up `+Y` without rotating each of them — so the skin is
/// built lying down and rotated once, which is both cheaper and easier to read
/// than four rotated placements.
fn torso_half(range: (usize, usize), rise: f32, params: DetailParams) -> MeshResult<Mesh> {
    let segments = Segments::new(params.ring_segments.max(3))?;
    let unit = Profile::circle(meters(1.0), segments)?;
    let sections: Vec<LoftSection> = TORSO_STATIONS[range.0..range.1]
        .iter()
        .map(|station| LoftSection {
            profile: unit.clone(),
            placement: Transform::new(
                Vec3::new(0.0, 0.0, station[0] - rise),
                Quat::IDENTITY,
                Vec3::new(station[1], station[2], 1.0),
            ),
        })
        .collect();
    let lying = loft(
        &sections,
        LoftOptions {
            caps: CapPolicy::Both,
            closed_loop: false,
        },
    )?;
    generate_normals(&transform(
        &lying,
        Transform::new(Vec3::ZERO, upright()?, Vec3::ONE).to_matrix(),
    )?)
}

/// The quarter turn about `+X` that maps the loft's own `+Z` onto world `+Y`.
fn upright() -> MeshResult<Quat> {
    Quat::from_axis_angle(Vec3::UNIT_X, -FRAC_PI_2).map_err(|_| {
        MeshError::new(
            MeshErrorCode::DegenerateAxis,
            "the torso's authored upright axis is a unit axis",
        )
    })
}

/// The neck: a short cylinder, already built about `+Y`.
fn neck(params: DetailParams) -> MeshResult<Mesh> {
    let segments = Segments::new(params.ring_segments.max(3))?;
    generate_normals(&cylinder(
        meters(NECK_RADIUS),
        meters(NECK_HEIGHT),
        segments,
        CapPolicy::Both,
    )?)
}

/// The head: a unit icosphere scaled into an egg 0.24 tall — one seventh and a
/// half of the figure's height, which is what makes the proportions read human.
fn head(params: DetailParams) -> MeshResult<Mesh> {
    let levels = Subdivisions::new(params.icosphere_subdivisions)?;
    let ball = icosphere(meters(1.0), levels)?;
    generate_normals(&transform(
        &ball,
        Transform::new(Vec3::ZERO, Quat::IDENTITY, HEAD_SCALE).to_matrix(),
    )?)
}

/// Every limb bone and its terminating block, in the order [`human_limbs`]
/// names them. Legs hang off the pelvis, arms off the torso.
fn limbs(params: DetailParams) -> MeshResult<Vec<RigPart>> {
    const PELVIS: usize = 0;
    const TORSO: usize = 1;
    const FIRST_LIMB_PART: usize = 4;
    let mut parts: Vec<RigPart> = Vec::new();
    let mut base = FIRST_LIMB_PART;
    for (slot, side) in [(0usize, 1.0_f32), (1, -1.0), (2, 1.0), (3, -1.0)] {
        let leg = slot < 2;
        let bones: [usize; 2] = if leg { [2, 3] } else { [0, 1] };
        let root = if leg { PELVIS } else { TORSO };
        let names = LIMB_NAMES[slot];
        for (rung, spec) in bones.iter().enumerate() {
            let parent = if rung == 0 { root } else { base };
            let mut part = bone(
                names[rung],
                Some(parent),
                &LIMB_BONES[*spec],
                side,
                // Every limb bone here is near-vertical, so the sagittal axis is
                // a real perpendicular for the sweep's transport frame.
                Vec3::UNIT_Z,
                params,
            )?;
            part.mesh = with_joint_ball(part.mesh, &part.rest, &JOINTS[*spec], side, params)?;
            parts.push(part);
        }
        parts.push(terminator(names[2], base + 1, leg, side, params)?);
        base += 3;
    }
    Ok(parts)
}

/// Fold a limb bone's joint ball into the bone's own mesh.
///
/// The ball's centre is authored in creature space and pulled back through the
/// bone's rest transform, so it lands exactly where it always did rather than
/// approximately at the bone's origin — the shoulder ball in particular sits
/// 5 mm above its bone's pivot.
fn with_joint_ball(
    mesh: Mesh,
    rest: &Transform,
    joint: &[f32; 4],
    side: f32,
    params: DetailParams,
) -> MeshResult<Mesh> {
    let rings = Rings::new((params.sphere_rings / 2).max(2))?;
    let segments = Segments::new((params.sphere_segments / 2).max(3))?;
    let ball = uv_sphere(meters(joint[3]), rings, segments)?;
    let local = rest.inverse().map_err(|_| {
        MeshError::new(
            MeshErrorCode::InvalidParameter,
            "a limb bone's rest placement is a rigid transform and inverts",
        )
    })?;
    let centre = local.transform_point(Vec3::new(side * joint[0], joint[1], joint[2]));
    let placed = transform(&ball, Mat4::translation(centre))?;
    generate_normals(&combine(&[mesh, placed])?)
}

/// The block that terminates a limb: a foot resting on `y = 0`, or a hand.
fn terminator(
    name: &'static str,
    parent: usize,
    leg: bool,
    side: f32,
    params: DetailParams,
) -> MeshResult<RigPart> {
    let segments = Segments::new((params.ring_segments / 5).clamp(3, 6))?;
    let (half, fillet, centre) = match leg {
        true => (
            FOOT_HALF_EXTENTS,
            0.028,
            Vec3::new(side * FOOT_CENTRE[0], FOOT_HALF_EXTENTS[1], FOOT_CENTRE[1]),
        ),
        false => (
            HAND_HALF_EXTENTS,
            0.018,
            Vec3::new(side * HAND_CENTRE[0], HAND_CENTRE[1], HAND_CENTRE[2]),
        ),
    };
    let block = rounded_box(
        Vec3::new(half[0], half[1], half[2]),
        meters(fillet),
        segments,
    )?;
    Ok(RigPart {
        name,
        mesh: generate_normals(&block)?,
        parent: Some(parent),
        rest: Transform::from_translation(centre),
    })
}
