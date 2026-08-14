//! The scene's geometry proof suite.
//!
//! This app *is* the proof that the mesh layers work, so its integration tests
//! are not smoke tests. They assert four properties, on every **distinct**
//! generated mesh in the scene — the terrain and the dog's 23 bones — at every
//! variant:
//!
//! 1. **Validity** — every object is a real `Mesh`: triangles, finite positions,
//!    in-range indices, one normal per vertex, sane bounds.
//! 2. **Determinism** — building the same variant twice produces byte-identical
//!    geometry, per object and as a whole-scene digest vector.
//! 3. **Topology change** — the variants really do re-tessellate: named objects
//!    differ in vertex and index count, and the concrete numbers are printed.
//! 4. **Digest sensitivity** — changing a generator parameter changes the
//!    digest, and restoring it restores the original digest exactly.
//!
//! The *crowd* — which dog walks where, in what colour, which way round — is
//! proved separately, in `tests/rings.rs`. The split is the same one the code
//! makes: this file is about geometry, and there is exactly one dog's worth of
//! it no matter how many dogs are on screen.

use axiom_mesh::{aabb, bounding_sphere, digest, Mesh};
use axiom_dog::{scene_meshes, SceneObject, SceneVariant};

/// The objects the topology proof names explicitly. Each must change vertex and
/// index count between `Base`, `Dense` and `Coarse` — one of every operator the
/// scene still uses: a heightfield, a loft, three different sweeps and an
/// icosphere.
const TOPOLOGY_WITNESSES: [&str; 6] = [
    "terrain",
    "dog-pelvis",
    "dog-neck",
    "dog-head",
    "dog-fore-l-upper",
    "dog-tail-base",
];

fn scene(variant: SceneVariant) -> Vec<SceneObject> {
    scene_meshes(variant)
        .unwrap_or_else(|error| panic!("the {} scene must build: {error:?}", variant.label()))
}

fn find<'a>(objects: &'a [SceneObject], name: &str) -> &'a SceneObject {
    objects
        .iter()
        .find(|object| object.name == name)
        .unwrap_or_else(|| panic!("the scene contains an object named {name}"))
}

fn counts(mesh: &Mesh) -> (usize, usize) {
    (mesh.vertex_count(), mesh.indices().len())
}

#[test]
fn every_generated_object_is_valid_renderable_geometry() {
    for variant in SceneVariant::ALL {
        let objects = scene(variant);
        // The terrain plus one dog's 23 bones. Not one per dog: the rings are
        // instances of this set, and `tests/rings.rs` holds that claim.
        assert_eq!(
            objects.len(),
            24,
            "{} scene produced {} distinct meshes",
            variant.label(),
            objects.len()
        );
        for object in &objects {
            let mesh = &object.mesh;
            let label = format!("{}/{}", variant.label(), object.name);

            assert!(
                mesh.triangle_count() > 0,
                "{label} has no triangles (operator: {})",
                object.operator
            );
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
            assert_eq!(
                mesh.normals().len(),
                mesh.vertex_count(),
                "{label} normal count does not match its vertex count"
            );
            assert!(
                mesh.normals()
                    .iter()
                    .all(|n| n.length_squared() > 0.25 && n.length_squared() < 4.0),
                "{label} has a degenerate normal"
            );
            // Every operator left in the scene has a natural parameterization,
            // so every object carries UVs. (The one documented exception used to
            // be the marching-cubes sculpture, which no longer stands here.)
            assert!(mesh.has_uvs(), "{label} carries no UVs");
            assert_eq!(
                mesh.uvs().len(),
                mesh.vertex_count(),
                "{label} UV count does not match its vertex count"
            );

            let bounds = aabb(mesh).unwrap_or_else(|e| panic!("{label} has bounds: {e:?}"));
            let sphere =
                bounding_sphere(mesh).unwrap_or_else(|e| panic!("{label} has a sphere: {e:?}"));
            assert!(
                bounds.max().x >= bounds.min().x,
                "{label} has an inverted AABB"
            );
            assert!(
                sphere.radius() > 0.0,
                "{label} has a zero-radius bounding sphere"
            );
            assert!(
                !object.operator.is_empty(),
                "{label} does not credit the operator that built it"
            );
        }

        // Names are unique — the tests below address objects by name.
        let mut names: Vec<&str> = objects.iter().map(|o| o.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(
            names.len(),
            count,
            "{} has duplicate object names",
            variant.label()
        );
    }
}

#[test]
fn building_the_same_variant_twice_is_byte_identical() {
    for variant in SceneVariant::ALL {
        let first = scene(variant);
        let second = scene(variant);
        assert_eq!(
            first.len(),
            second.len(),
            "{} object count is not stable",
            variant.label()
        );

        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(
                a.name,
                b.name,
                "{} object order is not stable",
                variant.label()
            );
            assert_eq!(
                digest(&a.mesh),
                digest(&b.mesh),
                "{}/{} is not deterministic",
                variant.label(),
                a.name
            );
            assert_eq!(
                a.mesh,
                b.mesh,
                "{}/{} differs stream-wise",
                variant.label(),
                a.name
            );
        }

        // The whole-scene fingerprint: the ordered digest vector.
        let fingerprint: Vec<u64> = first.iter().map(|o| digest(&o.mesh).raw()).collect();
        let replay: Vec<u64> = second.iter().map(|o| digest(&o.mesh).raw()).collect();
        assert_eq!(
            fingerprint, replay,
            "{} whole-scene digest vector is not deterministic",
            variant.label()
        );
        println!(
            "[determinism] {:>6}: {} distinct meshes, scene digest {:016x}",
            variant.label(),
            first.len(),
            axiom_mesh::digest(
                &axiom_mesh::combine(&first.iter().map(|o| o.mesh.clone()).collect::<Vec<Mesh>>())
                    .expect("the whole scene combines")
            )
            .raw()
        );
    }
}

#[test]
fn the_variants_really_re_tessellate() {
    let base = scene(SceneVariant::Base);
    let dense = scene(SceneVariant::Dense);
    let coarse = scene(SceneVariant::Coarse);

    println!(
        "\n{:<24} {:>20} {:>20} {:>20}",
        "object", "base (verts/idx)", "dense (verts/idx)", "coarse (verts/idx)"
    );
    for name in TOPOLOGY_WITNESSES {
        let b = counts(&find(&base, name).mesh);
        let d = counts(&find(&dense, name).mesh);
        let c = counts(&find(&coarse, name).mesh);
        println!(
            "{name:<24} {:>9}/{:<10} {:>9}/{:<10} {:>9}/{:<10}",
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

    // Whole-scene totals, as one headline number per variant.
    for (label, objects) in [("base", &base), ("dense", &dense), ("coarse", &coarse)] {
        let vertices: usize = objects.iter().map(|o| o.mesh.vertex_count()).sum();
        let triangles: usize = objects.iter().map(|o| o.mesh.triangle_count()).sum();
        println!("[totals] {label:>6}: {vertices} vertices, {triangles} triangles (uploaded once)");
    }

    let total =
        |objects: &[SceneObject]| -> usize { objects.iter().map(|o| o.mesh.triangle_count()).sum() };
    assert!(total(&dense) > total(&base));
    assert!(total(&base) > total(&coarse));
}

#[test]
fn a_parameter_change_moves_the_digest_and_restoring_it_moves_it_back() {
    let original: Vec<u64> = scene(SceneVariant::Base)
        .iter()
        .map(|o| digest(&o.mesh).raw())
        .collect();
    let changed: Vec<u64> = scene(SceneVariant::Dense)
        .iter()
        .map(|o| digest(&o.mesh).raw())
        .collect();
    assert_eq!(original.len(), changed.len());
    assert_ne!(
        original, changed,
        "changing the detail parameters left every digest unmoved"
    );

    let restored: Vec<u64> = scene(SceneVariant::Base)
        .iter()
        .map(|o| digest(&o.mesh).raw())
        .collect();
    assert_eq!(
        original, restored,
        "restoring the detail parameters did not restore the digests"
    );

    // The change is broad, not one object: most of the scene moved.
    let moved = original
        .iter()
        .zip(changed.iter())
        .filter(|(a, b)| a != b)
        .count();
    println!(
        "[digest] {moved} of {} objects moved base -> dense",
        original.len()
    );
    assert!(
        moved * 2 > original.len(),
        "only {moved} of {} digests moved",
        original.len()
    );
}
