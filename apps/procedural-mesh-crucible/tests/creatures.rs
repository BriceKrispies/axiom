//! The two articulated creatures' proof suite.
//!
//! The dog and the human are the crucible's answer to "can these operators be
//! composed into something *articulated*?" — a shape with limbs, joints and a
//! pose, rather than a hull or a tube. Nothing anatomical exists in
//! `axiom-mesh-ops`; both creatures are compositions authored in the app, and
//! these tests hold them to exactly the properties every other generated object
//! in this scene is held to:
//!
//! 1. **Validity** — real meshes: triangles, finite positions, in-range indices,
//!    unit normals, UVs, a sane AABB.
//! 2. **Topology change** — the variant really re-tessellates both creatures,
//!    with the concrete vertex and index counts printed.
//! 3. **Determinism** — two builds of the same variant have the same digest.
//! 4. **Proportion** — the human is taller than it is wide, the dog is longer
//!    than it is tall, both stand on `y = 0`, and the human is taller than the
//!    dog.

use axiom_mesh::{aabb, digest, Mesh};
use axiom_procedural_mesh_crucible::{
    crucible_meshes, dog, dog_parts, ground_y, human, human_parts, CrucibleObject, CrucibleVariant,
};

/// How far above or below zero a sole may sit and still be "standing on the
/// ground". The feet and paws are rounded boxes whose half-height is their
/// centre height, so the only error here is the fillet's own arc sampling.
const GROUND_TOLERANCE: f32 = 0.005;

fn build(name: &str, variant: CrucibleVariant) -> Mesh {
    let built = match name {
        "dog" => dog(variant),
        _ => human(variant),
    };
    built.unwrap_or_else(|error| panic!("the {name} must build at {}: {error:?}", variant.label()))
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
fn both_creatures_are_valid_geometry_at_every_variant() {
    for variant in CrucibleVariant::ALL {
        for name in ["dog", "human"] {
            let mesh = build(name, variant);
            let label = format!("{}/{name}", variant.label());

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
            // Every part of both creatures comes out of an operator with a real
            // parameterization, and combine keeps a stream only when every input
            // has it — so a missing UV stream here means a part lost its UVs.
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
}

#[test]
fn the_variants_really_re_tessellate_both_creatures() {
    println!(
        "\n{:<8} {:>20} {:>20} {:>20}",
        "creature", "base (verts/idx)", "dense (verts/idx)", "coarse (verts/idx)"
    );
    for name in ["dog", "human"] {
        let b = counts(&build(name, CrucibleVariant::Base));
        let d = counts(&build(name, CrucibleVariant::Dense));
        let c = counts(&build(name, CrucibleVariant::Coarse));
        println!(
            "{name:<8} {:>9}/{:<10} {:>9}/{:<10} {:>9}/{:<10}",
            b.0, b.1, d.0, d.1, c.0, c.1
        );
        assert_ne!(b, d, "{name} did not change between base and dense");
        assert_ne!(b, c, "{name} did not change between base and coarse");
        assert_ne!(d, c, "{name} did not change between dense and coarse");
        assert!(
            d.0 > b.0 && b.0 > c.0,
            "{name} vertex counts are not ordered coarse < base < dense: {c:?} {b:?} {d:?}"
        );
        assert!(
            d.1 > b.1 && b.1 > c.1,
            "{name} index counts are not ordered coarse < base < dense: {c:?} {b:?} {d:?}"
        );
    }
    for variant in CrucibleVariant::ALL {
        for name in ["dog", "human"] {
            let mesh = build(name, variant);
            println!(
                "[triangles] {:>6}/{name:<6}: {} triangles",
                variant.label(),
                mesh.triangle_count()
            );
        }
    }
}

#[test]
fn building_a_creature_twice_is_byte_identical() {
    for variant in CrucibleVariant::ALL {
        for name in ["dog", "human"] {
            let first = build(name, variant);
            let second = build(name, variant);
            assert_eq!(
                digest(&first),
                digest(&second),
                "{}/{name} is not deterministic",
                variant.label()
            );
            assert_eq!(first, second, "{}/{name} differs stream-wise", variant.label());
            println!(
                "[determinism] {:>6}/{name:<6}: digest {:016x}",
                variant.label(),
                digest(&first).raw()
            );
        }
    }
}

#[test]
fn the_creatures_proportions_read_as_a_dog_and_a_person() {
    let dog_mesh = build("dog", CrucibleVariant::Base);
    let human_mesh = build("human", CrucibleVariant::Base);
    let (dog_w, dog_h, dog_d, dog_floor) = extents(&dog_mesh);
    let (human_w, human_h, human_d, human_floor) = extents(&human_mesh);

    println!(
        "[aabb] dog   w {dog_w:.3} h {dog_h:.3} d {dog_d:.3} floor {dog_floor:.4}"
    );
    println!(
        "[aabb] human w {human_w:.3} h {human_h:.3} d {human_d:.3} floor {human_floor:.4}"
    );

    // A dog is longer than it is tall; a person is taller than they are wide.
    assert!(
        dog_d > dog_h,
        "the dog is not longer (z {dog_d}) than it is tall ({dog_h})"
    );
    assert!(
        human_h > human_w,
        "the human is not taller ({human_h}) than it is wide ({human_w})"
    );
    assert!(
        human_h > human_d,
        "the human is not taller ({human_h}) than it is deep ({human_d})"
    );
    // Both stand on the ground, and the person stands over the dog.
    assert!(dog_floor.abs() < GROUND_TOLERANCE);
    assert!(human_floor.abs() < GROUND_TOLERANCE);
    assert!(
        human_h > dog_h,
        "the human ({human_h}) is not taller than the dog ({dog_h})"
    );
    // The authored figures: ~1.8 at the crown, ~0.9 at the dog's shoulder (so a
    // little more at the ear tips).
    assert!(human_h > 1.7 && human_h < 1.9, "the human is {human_h} tall");
    assert!(dog_h > 0.85 && dog_h < 1.3, "the dog is {dog_h} tall");
}

/// The scene no longer holds one `dog` object and one `human` object: both
/// creatures are spawned **as bones**, one scene object per bone, because that
/// is the only shape a WebGL2 fallback can animate (see `creature_rig.rs`). The
/// combined meshes above are still the geometry proof; this is the placement
/// one.
#[test]
fn both_creatures_are_in_the_scene_as_bones_at_every_variant() {
    for variant in CrucibleVariant::ALL {
        let objects = crucible_meshes(variant).expect("the scene builds");
        for (creature, rig) in [
            ("dog", dog_parts(variant).expect("the dog rigs")),
            ("human", human_parts(variant).expect("the human rigs")),
        ] {
            // Every bone the rig declares is in the scene, under its own name.
            for part in rig.parts() {
                let object = objects
                    .iter()
                    .find(|object| object.name == part.name)
                    .unwrap_or_else(|| {
                        panic!(
                            "the {} scene contains the {creature} bone {}",
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
                    "the {creature} bone {} has no geometry",
                    part.name
                );
            }
            println!("[rig] {:>6}/{creature:<6}: {} bones", variant.label(), rig.len());
        }
        // The creatures stand on the terrain they run over, not at y = 0. The
        // loop rings the scene out on the rim, where the ground is well above
        // the basin, so the claim is that each foot sits on *its own* ground —
        // not that it sits at any particular height.
        let feet: Vec<&CrucibleObject> = objects
            .iter()
            .filter(|object| object.name.ends_with("-paw") || object.name.contains("-foot-"))
            .collect();
        assert_eq!(feet.len(), 6, "expected four paws and two feet");
        for foot in feet {
            let at = foot.placement.translation;
            let ground = ground_y(at.x, at.z);
            assert!(
                (at.y - ground).abs() < 1.0,
                "{} rests at y = {}, but the terrain there is {ground}",
                foot.name,
                at.y
            );
        }
    }
}

/// The rest pose the scene spawns is the assembled creature: dropping every
/// bone's geometry through its own placement reproduces a body standing on the
/// ground it was placed at, which is what proves the split rig did not smear
/// the animal across the map.
#[test]
fn the_spawned_bones_reassemble_into_a_creature_standing_on_the_ground() {
    let objects = crucible_meshes(CrucibleVariant::Base).expect("the scene builds");
    for (creature, rig) in [
        ("dog", dog_parts(CrucibleVariant::Base).expect("the dog rigs")),
        (
            "human",
            human_parts(CrucibleVariant::Base).expect("the human rigs"),
        ),
    ] {
        let bones: Vec<&CrucibleObject> = rig
            .parts()
            .iter()
            .map(|part| {
                objects
                    .iter()
                    .find(|object| object.name == part.name)
                    .expect("every bone is spawned")
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
        let bounds = aabb(&whole).expect("the assembled creature has bounds");
        let (min, max) = (bounds.min(), bounds.max());
        // Presented at 10x, so a 1.8-unit human is 18 tall and a ~1.1-unit dog
        // ~11 — and neither is scattered across the terrain.
        let height = max.y - min.y;
        let span = (max.x - min.x).max(max.z - min.z);
        let centre_x = 0.5 * (min.x + max.x);
        let centre_z = 0.5 * (min.z + max.z);
        let ground = ground_y(centre_x, centre_z);
        println!(
            "[assembled] {creature:<6}: height {height:.2} span {span:.2} floor {:.2} ground {ground:.2}",
            min.y
        );
        assert!(height > 8.0 && height < 24.0, "{creature} is {height} tall");
        assert!(span < 30.0, "{creature}'s bones span {span} — they came apart");
        // The soles rest on the terrain under the body. The tolerance is the
        // terrain's own relief across a creature's footprint, not slack.
        assert!(
            (min.y - ground).abs() < 1.2,
            "{creature}'s soles are at y = {}, but the ground there is {ground}",
            min.y
        );
    }
}
