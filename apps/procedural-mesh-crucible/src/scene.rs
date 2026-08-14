//! The scene: every **distinct** generated mesh, and the crowd that instances
//! them.
//!
//! This is the one place that knows the crucible is a *place* rather than a list
//! of meshes. There are exactly two things in it: the terrain, and one dog. The
//! concentric rings of walking dogs are not more geometry — they are the same 23 bone
//! meshes drawn again at other transforms, and the whole of what makes one dog
//! different from the next lives in [`crate::rings::RingDog`].
//!
//! That split is the point of the file. `objects` is the **upload**: every mesh
//! that will ever be registered with the engine, once each, each with the
//! operator chain that produced it. `dogs` is the **crowd**: as many walkers as
//! the rings hold, carrying no geometry at all. Adding a dog costs a transform
//! and a colour; it does not cost a vertex.
//!
//! It is a **pure function of the variant**. No clock, no randomness, no
//! environment: the same variant produces the same objects in the same order
//! with the same geometry, which is what the determinism test relies on and what
//! makes the digest of the whole scene a meaningful fingerprint.

use axiom_math::Transform;
use axiom_mesh::MeshResult;

use crate::creature_rig::CreatureRig;
use crate::object::CrucibleObject;
use crate::rings::{ring_dogs, RingDog};
use crate::terrain::terrain_mesh;
use crate::variant::CrucibleVariant;

/// The whole crucible's distinct geometry, built at `variant`.
///
/// Every object in the returned vector was produced by an `axiom-mesh-ops`
/// operator and validated by `axiom-mesh` on the way out of it; nothing here
/// hand-writes a vertex.
pub fn crucible_meshes(variant: CrucibleVariant) -> MeshResult<Vec<CrucibleObject>> {
    crucible_scene(variant).map(|scene| scene.objects)
}

/// The whole crucible: the distinct meshes to register, the rig those meshes are
/// the bones of, and every dog walking the field.
///
/// The rig is handed back rather than rebuilt because the animation needs the
/// *same* bones the scene registered — the parent indices, the rest transforms
/// and the bone order all have to line up with the entities that were spawned,
/// and re-deriving them is exactly the kind of quiet drift this app exists to
/// prevent.
#[derive(Debug, Clone)]
pub struct CrucibleScene {
    /// Every distinct generated mesh, in registration order: the terrain, then
    /// the dog's bones in rig order.
    pub objects: Vec<CrucibleObject>,
    /// The index in `objects` of the dog's first bone. Its bones run
    /// contiguously from there, in rig order, to the end of the vector.
    pub dog_first: usize,
    /// The dog's rig — the one skeleton every walker in the field wears.
    pub dog: CreatureRig,
    /// Every dog in the field, in spawn order.
    pub dogs: Vec<RingDog>,
}

impl CrucibleScene {
    /// The dog's bones, as registered geometry.
    pub fn bones(&self) -> &[CrucibleObject] {
        &self.objects[self.dog_first..]
    }
}

/// Build the whole crucible.
pub fn crucible_scene(variant: CrucibleVariant) -> MeshResult<CrucibleScene> {
    let mut objects: Vec<CrucibleObject> = vec![CrucibleObject::new(
        "terrain",
        "mesh_ops::heightfield_mesh (analytic sine sum + skirt)",
        terrain_mesh(variant.params())?,
        Transform::IDENTITY,
        TERRAIN_COLOR,
    )];
    let dog_first = objects.len();
    let dog = crate::creature_dog::dog_parts(variant)?;
    push_rig(&dog, &mut objects);
    Ok(CrucibleScene {
        objects,
        dog_first,
        dog,
        dogs: ring_dogs(),
    })
}

/// The terrain the rings are walked on.
///
/// It is the one thing in the scene that is not a dog, and it earns its place:
/// the walkers are stood on `ground_y`, so the two chains rise and fall with the
/// ground under them. Without it they would be circling in a void — the rings
/// would still be right, and they would read as flat.
const TERRAIN_COLOR: [f32; 3] = [0.20, 0.30, 0.17];

/// The operator credit every bone carries. Every bone in the rig came out of
/// this chain; which bone got which operator is documented, part by part, in
/// `creature_dog.rs`.
const DOG_OPERATORS: &str = "mesh_ops::loft (torso halves) + sweep (neck/muzzle/ears/legs/tail) + icosphere skull + uv_sphere nose + rounded_box paws, cut at the joints into a rig";

/// The coat colour a dog is drawn in when nothing has painted it — the reference
/// value the geometry audit and the native scene tests see. Every dog actually
/// on a ring is painted from the rainbow instead (see `rings.rs`), which is why
/// this is a property of the *mesh set* rather than of any walker.
const DOG_REFERENCE_COAT: [f32; 3] = [0.66, 0.44, 0.22];

/// Register the rig's bones as scene objects, each posed where it rests on a dog
/// standing at the origin at its authored size.
///
/// The placement is the rest pose, not a ring position: these objects are the
/// *geometry*, and the rings place their instances themselves, every frame, from
/// the tick. A headless build that never animates therefore shows one whole,
/// coherent dog rather than 23 bones at the origin.
fn push_rig(rig: &CreatureRig, objects: &mut Vec<CrucibleObject>) {
    rig.parts()
        .iter()
        .zip(rig.rest_world(Transform::IDENTITY))
        .for_each(|(part, placement)| {
            objects.push(CrucibleObject::new(
                part.name,
                DOG_OPERATORS,
                part.mesh.clone(),
                placement,
                DOG_REFERENCE_COAT,
            ));
        });
}
