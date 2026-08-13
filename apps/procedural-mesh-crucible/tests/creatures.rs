//! The articulated dog's proof suite.
//!
//! The dog is the crucible's answer to "can these operators be composed into
//! something *articulated*?" — a shape with limbs, joints and a pose, rather
//! than a hull or a tube. Nothing anatomical exists in `axiom-mesh-ops`; the dog
//! is a composition authored in the app, and these tests hold it to exactly the
//! properties every other generated object in this scene is held to:
//!
//! 1. **Validity** — a real mesh: triangles, finite positions, in-range indices,
//!    unit normals, UVs, a sane AABB.
//! 2. **Topology change** — the variant really re-tessellates it, with the
//!    concrete vertex and index counts printed.
//! 3. **Determinism** — two builds of the same variant have the same digest.
//! 4. **Proportion** — longer than it is tall, standing on `y = 0`, and the
//!    length the ring spacing is derived from.
//! 5. **The rig is the scene's geometry** — every bone the rig declares is a
//!    registered scene object, and dropping those objects through their own
//!    placements reassembles one whole dog rather than 23 scattered parts.

use axiom_mesh::{aabb, digest, Mesh};
use axiom_procedural_mesh_crucible::{
    crucible_meshes, dog, dog_parts, CrucibleObject, CrucibleVariant, DOG_BODY_LENGTH,
};

/// How far above or below zero a sole may sit and still be "standing on the
/// ground". The paws are rounded boxes whose half-height is their centre height,
/// so the only error here is the fillet's own arc sampling.
const GROUND_TOLERANCE: f32 = 0.005;

fn build(variant: CrucibleVariant) -> Mesh {
    dog(variant)
        .unwrap_or_else(|error| panic!("the dog must build at {}: {error:?}", variant.label()))
}

fn counts(mesh: &Mesh) -> (usize, usize) {
    (mesh.vertex_count(), mesh.indices().len())
}

/// `(width, height, depth)` of a mesh's axis-aligned bounds, plus its floor.
fn extents(mesh: &Mesh) -> (f32, f32, f32, f32) {
    let bounds = aabb(mesh).expect("a creature has bounds");
    let (min, max) = (bounds.min(), bounds.max());
    (max.x - min.x, max.y - min.y, max.z - min.z, min.y)
}

#[test]
fn the_dog_is_valid_geometry_at_every_variant() {
    for variant in CrucibleVariant::ALL {
        let mesh = build(variant);
        let label = variant.label();

        assert!(mesh.triangle_count() > 0, "{label} has no triangles");
        assert_eq!(
            mesh.indices().len() % 3,
            0,
            "{label} index count is not a triangle list"
        );
        assert!(
            mesh.positions()
                .iter()
                .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()),
            "{label} has a non-finite position"
        );
        let vertices = mesh.vertex_count() as u32;
        assert!(
            mesh.indices().iter().all(|index| *index < vertices),
            "{label} has an out-of-range index"
        );
        assert!(mesh.has_normals(), "{label} carries no normals");
        assert_eq!(mesh.normals().len(), mesh.vertex_count());
        assert!(
            mesh.normals()
                .iter()
                .all(|n| (n.length_squared() - 1.0).abs() < 1.0e-3),
            "{label} has a normal generate_normals did not normalize"
        );
        // Every part comes out of an operator with a real parameterization, and
        // combine keeps a stream only when every input has it — so a missing UV
        // stream here means a part lost its UVs.
        assert!(mesh.has_uvs(), "{label} lost its UVs in the combine");
        assert_eq!(mesh.uvs().len(), mesh.vertex_count());

        let (width, height, depth, floor) = extents(&mesh);
        assert!(width > 0.0 && height > 0.0 && depth > 0.0, "{label} is flat");
        assert!(
            floor.abs() < GROUND_TOLERANCE,
            "{label} does not stand on y = 0 (floor {floor})"
        );
    }
}

#[test]
fn the_variant_really_re_tessellates_the_dog() {
    let b = counts(&build(CrucibleVariant::Base));
    let d = counts(&build(CrucibleVariant::Dense));
    let c = counts(&build(CrucibleVariant::Coarse));
    println!(
        "\n{:<8} {:>20} {:>20} {:>20}\n{:<8} {:>9}/{:<10} {:>9}/{:<10} {:>9}/{:<10}",
        "creature",
        "base (verts/idx)",
        "dense (verts/idx)",
        "coarse (verts/idx)",
        "dog",
        b.0,
        b.1,
        d.0,
        d.1,
        c.0,
        c.1
    );
    assert!(
        d.0 > b.0 && b.0 > c.0,
        "vertex counts are not ordered coarse < base < dense: {c:?} {b:?} {d:?}"
    );
    assert!(
        d.1 > b.1 && b.1 > c.1,
        "index counts are not ordered coarse < base < dense: {c:?} {b:?} {d:?}"
    );
    for variant in CrucibleVariant::ALL {
        println!(
            "[triangles] {:>6}: {} triangles",
            variant.label(),
            build(variant).triangle_count()
        );
    }
}

#[test]
fn building_the_dog_twice_is_byte_identical() {
    for variant in CrucibleVariant::ALL {
        let first = build(variant);
        let second = build(variant);
        assert_eq!(
            digest(&first),
            digest(&second),
            "{} is not deterministic",
            variant.label()
        );
        assert_eq!(first, second, "{} differs stream-wise", variant.label());
        println!(
            "[determinism] {:>6}: digest {:016x}",
            variant.label(),
            digest(&first).raw()
        );
    }
}

#[test]
fn the_dogs_proportions_read_as_a_dog_and_match_the_ring_spacing() {
    let mesh = build(CrucibleVariant::Base);
    let (width, height, depth, floor) = extents(&mesh);
    println!("[aabb] dog w {width:.3} h {height:.3} d {depth:.3} floor {floor:.4}");

    // A dog is longer than it is tall and than it is wide.
    assert!(
        depth > height,
        "the dog is not longer (z {depth}) than it is tall ({height})"
    );
    assert!(depth > width, "the dog is not longer than it is wide");
    assert!(floor.abs() < GROUND_TOLERANCE);
    // The authored figure: ~0.9 at the shoulder, a little more at the ear tips.
    assert!(height > 0.85 && height < 1.3, "the dog is {height} tall");
    // And the nose-to-tail length the two rings space their chains by is the
    // length of the animal actually being drawn, not a number that drifted.
    assert!(
        (depth - DOG_BODY_LENGTH).abs() < 0.15,
        "the ring spacing is built on a {DOG_BODY_LENGTH}-unit dog, but the dog measures {depth}"
    );
}

/// The scene holds no single `dog` object: the dog is spawned **as bones**, one
/// registered mesh per bone, because that is the only shape a WebGL2 fallback
/// can animate (see `creature_rig.rs`). The combined mesh above is still the
/// geometry proof; this is the registration one.
#[test]
fn every_bone_the_rig_declares_is_a_registered_scene_object() {
    for variant in CrucibleVariant::ALL {
        let objects = crucible_meshes(variant).expect("the scene builds");
        let rig = dog_parts(variant).expect("the dog rigs");
        for part in rig.parts() {
            let object = objects
                .iter()
                .find(|object| object.name == part.name)
                .unwrap_or_else(|| {
                    panic!(
                        "the {} scene contains the dog bone {}",
                        variant.label(),
                        part.name
                    )
                });
            assert!(
                !object.operator.is_empty(),
                "{} does not credit the operators that built it",
                part.name
            );
            assert!(
                object.mesh.triangle_count() > 0,
                "the dog bone {} has no geometry",
                part.name
            );
        }
        // 23 bones, and the terrain is the only thing that is not one of them.
        assert_eq!(rig.len(), 23, "the dog has {} bones", rig.len());
        assert_eq!(objects.len(), rig.len() + 1);
        println!("[rig] {:>6}: {} bones", variant.label(), rig.len());
    }
}

/// Dropping every registered bone through its own placement reproduces a dog
/// standing at the origin — which is what proves the split rig did not smear the
/// animal across the map.
#[test]
fn the_registered_bones_reassemble_into_one_standing_dog() {
    let objects = crucible_meshes(CrucibleVariant::Base).expect("the scene builds");
    let rig = dog_parts(CrucibleVariant::Base).expect("the dog rigs");
    let bones: Vec<&CrucibleObject> = rig
        .parts()
        .iter()
        .map(|part| {
            objects
                .iter()
                .find(|object| object.name == part.name)
                .expect("every bone is registered")
        })
        .collect();
    let placed: Vec<Mesh> = bones
        .iter()
        .map(|object| {
            axiom_mesh::transform(&object.mesh, object.placement.to_matrix())
                .expect("a placed bone is valid geometry")
        })
        .collect();
    let whole = axiom_mesh::combine(&placed).expect("the bones combine");
    let assembled = aabb(&whole).expect("the assembled dog has bounds");
    let solo = aabb(&build(CrucibleVariant::Base)).expect("the combined dog has bounds");
    println!(
        "[assembled] min {:?} max {:?}",
        assembled.min(),
        assembled.max()
    );
    // The bones, replaced at their rest placements, are the same animal the
    // combined `dog()` mesh is — to within the normals pass that welds nothing.
    for (a, b) in [
        (assembled.min(), solo.min()),
        (assembled.max(), solo.max()),
    ] {
        assert!(
            a.distance(b) < 1.0e-3,
            "the reassembled dog's bounds {a:?} are not the dog's own {b:?}"
        );
    }
}
