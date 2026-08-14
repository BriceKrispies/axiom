//! [`CreatureRig`]: a creature's geometry cut into **named bones**, and the
//! forward pass that resolves them into world transforms.
//!
//! The dog in this scene exists twice over. `dog()` hands back one welded mesh —
//! the honest answer to "can these operators compose into an articulated body?",
//! and what the geometry tests digest. `dog_parts()` hands back the *same
//! geometry* cut at its joints, so the app can move it.
//!
//! Both come from one authoring pass, and that is the point: a rig whose bones
//! were authored separately from the combined mesh would drift, silently, the
//! first time somebody moved a shoulder. Here the combined mesh is *derived*
//! from the rig ([`CreatureRig::assembled`]), so the two can never disagree.
//!
//! ## Why parts and not skinning
//!
//! This machine's WebGPU device creation fails and the app presents through the
//! WebGL2 fallback, which has no vertex-stage storage buffers and therefore
//! draws no skinned geometry at all. Rigid parts are ordinary instanced draws:
//! one registered mesh per bone, one instance transform per bone per frame. The
//! joints are hard rather than smooth, which for a creature assembled from
//! interpenetrating swept tubes is what it already looked like.
//!
//! ## The one convention every bone obeys
//!
//! A bone's mesh is authored **in its own local space with the origin at its
//! joint pivot**, and a *limb* bone additionally runs down local `-Z`, from the
//! origin to `(0, 0, -length)`. That is not an arbitrary axis: [`Quat::look_rotation`]
//! maps local `-Z` onto its `forward` argument and local `+Y` onto the
//! perpendicular part of its `up` argument, so
//!
//! ```text
//! aim(direction, pole)
//! ```
//!
//! is *exactly* "point this bone down `direction`, with its bend plane facing
//! `pole`" — one call, no matrix assembly, and the pole that keeps a knee from
//! flipping is the same argument that controls the roll.

use axiom_math::{Curve, Quat, Transform, Vec3};
use axiom_mesh::{combine, generate_normals, transform, Mesh, MeshError, MeshErrorCode, MeshResult};
use axiom_mesh_ops::{sweep, CapPolicy, Profile, Samples, Segments, SweepOptions};

use crate::quantities::{meters, radians, ratio};
use crate::variant::DetailParams;

/// One bone: its geometry, where it hangs, and where it hangs from.
#[derive(Debug, Clone)]
pub struct RigPart {
    /// A stable identifier, unique within the rig and used as the scene object
    /// name, so a part is addressable from a test and from the page legend.
    pub name: &'static str,
    /// The bone's geometry, in its own local space with the origin at the joint
    /// pivot.
    pub mesh: Mesh,
    /// The index of this bone's parent in [`CreatureRig::parts`], or `None` for
    /// the root. Always **less than** this bone's own index.
    pub parent: Option<usize>,
    /// Where the bone sits in its parent's space when the creature is at rest.
    pub rest: Transform,
}

/// A creature's bones, ordered so that **every parent precedes its children**.
///
/// That ordering is the whole reason a single forward pass resolves the
/// hierarchy: by the time [`CreatureRig::resolve`] reaches bone `i`, bone
/// `parent(i)` is already world-resolved. [`CreatureRig::new`] rejects any part
/// list that does not have it, so the invariant is checked once at construction
/// rather than assumed at every pose.
#[derive(Debug, Clone)]
pub struct CreatureRig {
    parts: Vec<RigPart>,
}

impl CreatureRig {
    /// Build a rig, rejecting a part list whose parents do not precede their
    /// children.
    pub fn new(parts: Vec<RigPart>) -> MeshResult<CreatureRig> {
        ordered(&parts).map(|()| CreatureRig { parts })
    }

    /// Build a rig from parts whose `rest` is stated in **creature space** —
    /// "this bone's pivot is *here*, on the whole animal" — converting each to
    /// the parent-relative form the forward pass needs.
    ///
    /// Authoring a skeleton parent-relatively means expressing every child in
    /// the rotated frame of its parent, which for a limb is a mental inverse
    /// rotation per bone and a class of bug (a paw that is subtly cocked)
    /// nobody spots until it moves. Authoring in creature space is how the
    /// anatomy is actually known, so it is authored that way and converted
    /// once, here.
    pub fn from_creature_space(mut parts: Vec<RigPart>) -> MeshResult<CreatureRig> {
        ordered(&parts)?;
        let creature: Vec<Transform> = parts.iter().map(|part| part.rest).collect();
        parts
            .iter_mut()
            .enumerate()
            .try_for_each(|(index, part)| match part.parent {
                None => Ok(()),
                Some(parent) => creature[parent]
                    .inverse()
                    .map(|inverse| part.rest = Transform::combine(inverse, creature[index]))
                    .map_err(|_| {
                        MeshError::new(
                            MeshErrorCode::InvalidParameter,
                            "a rig part's rest placement is not invertible, so its children cannot be re-based onto it",
                        )
                    }),
            })?;
        Ok(CreatureRig { parts })
    }

    /// The bones, in resolution order.
    pub fn parts(&self) -> &[RigPart] {
        &self.parts
    }

    /// How many bones the rig has.
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// The index of the bone called `name`.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|part| part.name == name)
    }

    /// Resolve every bone to a world transform, given the creature's root
    /// placement and one **parent-relative** transform per bone.
    ///
    /// `locals` shorter than the rig falls back to the bone's rest transform, so
    /// a caller that only wants to move a few bones can pass what it has.
    pub fn resolve(&self, root: Transform, locals: &[Transform]) -> Vec<Transform> {
        self.parts
            .iter()
            .enumerate()
            .fold(Vec::with_capacity(self.parts.len()), |mut world, (index, part)| {
                let local = locals.get(index).copied().unwrap_or(part.rest);
                let parent = part.parent.map_or(root, |p| world[p]);
                world.push(Transform::combine(parent, local));
                world
            })
    }

    /// Every bone's world transform with the creature at rest.
    pub fn rest_world(&self, root: Transform) -> Vec<Transform> {
        self.resolve(root, &[])
    }

    /// The whole creature as **one** mesh, posed at rest under `root` — the
    /// combined shape `dog()` returns.
    ///
    /// Normals are regenerated after the combine, not carried through it: the
    /// head and the dog's skull are non-uniformly scaled, and a scaled normal is
    /// not the scaled surface's normal.
    pub fn assembled(&self, root: Transform) -> MeshResult<Mesh> {
        let world = self.rest_world(root);
        let placed: Vec<Mesh> = self
            .parts
            .iter()
            .zip(world.iter())
            .map(|(part, placement)| transform(&part.mesh, placement.to_matrix()))
            .collect::<MeshResult<Vec<Mesh>>>()?;
        generate_normals(&combine(&placed)?)
    }
}

/// Reject a part list whose parents do not precede their children.
fn ordered(parts: &[RigPart]) -> MeshResult<()> {
    let out_of_order = parts
        .iter()
        .enumerate()
        .any(|(index, part)| part.parent.is_some_and(|parent| parent >= index));
    match out_of_order {
        true => Err(MeshError::new(
            MeshErrorCode::InvalidParameter,
            "a rig part is declared before its parent, so one forward pass cannot resolve it",
        )),
        false => Ok(()),
    }
}

/// One two-bone limb, named so the pose pass can address it without knowing any
/// anatomy: which bones it is made of, how long they are, where its tip rests,
/// which way its middle joint must bend, and its share of the gait cycle.
///
/// A dog's hind leg has a third bone below the solved pair (the metatarsus,
/// hock → paw); `extra` names it and `len_extra` is its length. A limb without
/// one leaves `extra` `None` and the tip is the solved end.
#[derive(Debug, Clone, Copy)]
pub struct LimbChain {
    /// The bone pivoting at the hip/shoulder.
    pub upper: &'static str,
    /// The bone pivoting at the knee/elbow.
    pub lower: &'static str,
    /// A bone below the solved pair, if the anatomy has one.
    pub extra: Option<&'static str>,
    /// The paw/foot/hand block that terminates the limb.
    pub tip: &'static str,
    /// Where the limb's contact point sits in creature space at rest — the paw
    /// or foot centre on the ground, or the hand block's centre.
    pub contact: Vec3,
    /// From `contact` to the pivot the two-bone solve actually targets, in
    /// creature space. That pivot is the *ankle* for a two-bone limb and the
    /// *hock* for the dog's three-bone hind leg; either way it sits inside the
    /// terminating block rather than at its centre, and this is the difference.
    pub tip_offset: Vec3,
    /// From `contact` to the extra bone's own far end, in creature space.
    /// Meaningless when `extra` is `None`.
    pub ankle_offset: Vec3,
    /// The upper bone's length, in creature-local units.
    pub len_upper: f32,
    /// The lower bone's length, in creature-local units.
    pub len_lower: f32,
    /// The extra bone's length, in creature-local units (`0.0` when absent).
    pub len_extra: f32,
    /// The bend hint in creature space: `-Z` bends the joint forward (a knee),
    /// `+Z` backward (an elbow, or a dog's front leg).
    pub pole: Vec3,
    /// This limb's share of the gait cycle, `0..1`.
    pub offset: f32,
}

impl LimbChain {
    /// The same chain on the other side of the body. Every authored value is
    /// the `+x` one, so mirroring is a sign flip on the lateral component; the
    /// bend pole lies in the sagittal plane and is shared unchanged.
    pub fn mirrored(mut self, side: f32) -> LimbChain {
        self.contact = mirror_x(self.contact, side);
        self.tip_offset = mirror_x(self.tip_offset, side);
        self.ankle_offset = mirror_x(self.ankle_offset, side);
        self
    }
}

/// A creature-space point reflected onto `side`.
fn mirror_x(point: Vec3, side: f32) -> Vec3 {
    Vec3::new(point.x * side, point.y, point.z)
}

/// The far endpoint of an authored `[from, to, …]` bone row.
pub fn bone_tip(spec: &[f32; 8]) -> Vec3 {
    Vec3::new(spec[3], spec[4], spec[5])
}

/// The rotation that points a bone authored along local `-Z` down `direction`,
/// carrying its local `+Y` toward `pole`.
///
/// `pole` is the bend hint: for a two-bone chain it is the side the joint must
/// bulge toward, which is what stops a knee from choosing its own roll and
/// snapping inside-out between frames. A `pole` parallel to `direction` (or a
/// degenerate `direction`) would leave the basis undefined, so this falls back
/// to a perpendicular axis rather than failing — a bone that cannot be aimed is
/// a pose bug, not a geometry error, and a NaN transform would poison the whole
/// frame.
pub fn aim(direction: Vec3, pole: Vec3) -> Quat {
    Quat::look_rotation(direction, pole)
        .or_else(|_| Quat::look_rotation(direction, fallback_pole(direction)))
        .unwrap_or(Quat::IDENTITY)
}

/// One tapered bone from an authored `[from(x,y,z), to(x,y,z), radius,
/// tip_ratio]` row, mirrored to `side`.
///
/// The geometry is authored along the bone's own local `-Z`, from the origin to
/// `(0, 0, -length)`, and the rest transform aims it down the authored
/// direction — so a pose that overwrites the rotation moves the bone and
/// nothing else. `reference` seeds the sweep's parallel-transport frames and
/// must not be parallel to `-Z`.
pub fn bone(
    name: &'static str,
    parent: Option<usize>,
    spec: &[f32; 8],
    side: f32,
    reference: Vec3,
    params: DetailParams,
) -> MeshResult<RigPart> {
    let from = Vec3::new(side * spec[0], spec[1], spec[2]);
    let to = Vec3::new(side * spec[3], spec[4], spec[5]);
    let direction = to.subtract(from);
    let path = Curve::polyline(vec![Vec3::ZERO, Vec3::new(0.0, 0.0, -direction.length())])
        .map_err(|_| {
            MeshError::new(
                MeshErrorCode::InvalidPath,
                "an authored bone has two distinct endpoints",
            )
        })?;
    Ok(RigPart {
        name,
        mesh: swept(&path, spec[6], 1.0, spec[7], reference, params)?,
        parent,
        rest: Transform::new(from, aim(direction, reference), Vec3::ONE),
    })
}

/// The length of an authored `[from, to, …]` bone row.
pub fn bone_length(spec: &[f32; 8]) -> f32 {
    Vec3::new(spec[3] - spec[0], spec[4] - spec[1], spec[5] - spec[2]).length()
}

/// One tapered tube: a circle of `radius` swept along `path`, running from
/// `start_ratio` to `end_ratio` of its base.
pub fn swept(
    path: &Curve,
    radius: f32,
    start_ratio: f32,
    end_ratio: f32,
    reference: Vec3,
    params: DetailParams,
) -> MeshResult<Mesh> {
    let segments = Segments::new(params.ring_segments.max(3))?;
    let profile = Profile::circle(meters(radius), segments)?;
    let samples = Samples::new(params.sweep_samples.max(2))?;
    generate_normals(&sweep(
        &profile,
        path,
        samples,
        SweepOptions {
            caps: CapPolicy::Both,
            twist: radians(0.0),
            start_scale: ratio(start_ratio),
            end_scale: ratio(end_ratio),
            closed_path: false,
            initial_reference: reference,
        },
    )?)
}

/// An axis `direction` is certainly not parallel to.
fn fallback_pole(direction: Vec3) -> Vec3 {
    let unit = direction.normalize().unwrap_or(Vec3::UNIT_Z);
    [Vec3::UNIT_Y, Vec3::UNIT_X][usize::from(unit.y.abs() > 0.9)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_mesh_ops::cube;
    use crate::quantities::meters;

    fn block(name: &'static str, parent: Option<usize>, rest: Transform) -> RigPart {
        RigPart {
            name,
            mesh: cube(meters(0.2)).expect("a unit cube builds"),
            parent,
            rest,
        }
    }

    #[test]
    fn a_part_declared_before_its_parent_is_rejected() {
        let parts = vec![
            block("child", Some(1), Transform::IDENTITY),
            block("parent", None, Transform::IDENTITY),
        ];
        assert!(CreatureRig::new(parts).is_err());
    }

    #[test]
    fn the_forward_pass_composes_a_chain() {
        let rig = CreatureRig::new(vec![
            block("root", None, Transform::from_translation(Vec3::new(0.0, 1.0, 0.0))),
            block("child", Some(0), Transform::from_translation(Vec3::new(0.0, 2.0, 0.0))),
        ])
        .expect("the chain is ordered");
        let world = rig.rest_world(Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)));
        assert_eq!(world[0].translation, Vec3::new(5.0, 1.0, 0.0));
        assert_eq!(world[1].translation, Vec3::new(5.0, 3.0, 0.0));
        assert_eq!(rig.index_of("child"), Some(1));
        assert_eq!(rig.len(), 2);
    }

    #[test]
    fn aim_points_a_minus_z_bone_down_its_direction() {
        let down = Vec3::new(0.0, -1.0, 0.0);
        let rotation = aim(down, Vec3::UNIT_Z);
        let pointed = rotation.rotate(Vec3::new(0.0, 0.0, -1.0));
        assert!((pointed.y + 1.0).abs() < 1.0e-4, "aimed at {pointed:?}");
        // A pole parallel to the direction still yields a usable basis.
        let degenerate = aim(down, Vec3::UNIT_Y);
        let pointed = degenerate.rotate(Vec3::new(0.0, 0.0, -1.0));
        assert!((pointed.y + 1.0).abs() < 1.0e-4, "aimed at {pointed:?}");
    }
}
