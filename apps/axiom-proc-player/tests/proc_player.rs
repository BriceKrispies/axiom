//! Deliverable #8 — the Axiom Proc v0 property suite.
//!
//! These prove the invariants the whole pipeline rests on: determinism from
//! `(recipe, seed)`, stable resource identity, bounded output, cycle detection,
//! rejection of malformed input as *data*, and — the load-bearing one — that a
//! player needs nothing but the recipe bytes.

use axiom::prelude::App;
use axiom_proc_mesh::{MeshOp, ProcMeshApi};
use axiom_proc_player::{expand, DemoRecipes};
use axiom_proc_texture::{ProcTextureApi, TextureOp};
use axiom_recipe::{NodeId, Param, RecipeError, RecipeGraph, RecipeId, Scalar};

const SEED: u64 = 0xA11CE;

// 1. Deterministic output from the same recipe + seed (byte-identical), and
//    genuinely seed-sensitive where a recipe draws entropy.
#[test]
fn same_recipe_and_seed_is_byte_identical() {
    let r = DemoRecipes::build();
    let tex = ProcTextureApi::new();
    let mesh = ProcMeshApi::new();

    assert_eq!(
        tex.bake(&r.brick_texture, SEED).unwrap(),
        tex.bake(&r.brick_texture, SEED).unwrap()
    );
    assert_eq!(
        mesh.bake(&r.crate_mesh, SEED).unwrap(),
        mesh.bake(&r.crate_mesh, SEED).unwrap()
    );

    // The floor texture is value-noise seeded, so a different seed differs.
    let floor_a = tex.bake(&r.floor_texture, SEED).unwrap();
    let floor_b = tex.bake(&r.floor_texture, SEED + 1).unwrap();
    assert_ne!(floor_a, floor_b);

    // Whole-room expansion is deterministic across runs.
    assert_eq!(
        expand(&r, SEED).1.fingerprint(),
        expand(&r, SEED).1.fingerprint()
    );
}

// 2. Stable resource IDs: registering the generated resources in a fresh app
//    twice yields the same handle ids, and the recipes carry stable digests.
#[test]
fn resource_ids_and_recipe_digests_are_stable() {
    let bake_and_register_ids = || {
        let r = DemoRecipes::build();
        let tex = ProcTextureApi::new();
        let mut app = App::new().setup(|_, _, _| {}).build();
        let brick = tex.bake(&r.brick_texture, SEED).unwrap();
        let (w, h) = (brick.width(), brick.height());
        let a = app
            .add_texture_data(w, h, brick.into_pixels())
            .unwrap()
            .id();
        let floor = tex.bake(&r.floor_texture, SEED).unwrap();
        let (w, h) = (floor.width(), floor.height());
        let b = app
            .add_texture_data(w, h, floor.into_pixels())
            .unwrap()
            .id();
        (a, b)
    };
    assert_eq!(bake_and_register_ids(), bake_and_register_ids());

    // Two authorings of the same recipe are byte- and digest-identical.
    assert_eq!(
        DemoRecipes::build().brick_texture.digest(),
        DemoRecipes::build().brick_texture.digest()
    );
}

// 3. Bounded vertex counts: the demo is small, and an over-subdivided recipe
//    clamps rather than allocating without bound.
#[test]
fn vertex_counts_are_bounded() {
    let report = expand(&DemoRecipes::build(), SEED).1;
    assert!(
        report.mesh_vertices < 1_000,
        "demo mesh is small: {}",
        report.mesh_vertices
    );

    // A grid asking for a million cells per axis clamps to the layer's cap.
    let mut huge = RecipeGraph::new(RecipeId::from_raw(99), 1);
    huge.add(
        MeshOp::Grid as u16,
        vec![
            Param::int(1_000_000),
            Param::int(1_000_000),
            Param::scalar(Scalar::new(1.0)),
        ],
        vec![],
    );
    let baked = ProcMeshApi::new().bake(&huge, SEED).unwrap();
    assert!(
        baked.vertex_count() <= 65 * 65,
        "grid subdivision is clamped: {}",
        baked.vertex_count()
    );
}

// 4. Cycle detection in graphs: a forward / self reference is rejected on
//    validation and refuses to bake.
#[test]
fn cycles_are_detected() {
    let mut forward = RecipeGraph::new(RecipeId::from_raw(1), 1);
    forward.add(
        MeshOp::Bevel as u16,
        vec![Param::scalar(Scalar::new(0.1))],
        vec![NodeId::from_raw(5)],
    );
    assert_eq!(forward.validate(), Err(RecipeError::CyclicInput));
    assert!(ProcMeshApi::new().bake(&forward, SEED).is_err());

    let mut selfref = RecipeGraph::new(RecipeId::from_raw(1), 1);
    selfref.add(
        MeshOp::Bevel as u16,
        vec![Param::scalar(Scalar::new(0.1))],
        vec![NodeId::from_raw(0)],
    );
    assert_eq!(selfref.validate(), Err(RecipeError::CyclicInput));
}

// 5. Invalid input is rejected as data — never a panic.
#[test]
fn invalid_input_is_rejected_as_data() {
    // An operator missing its required parameters.
    let mut short = RecipeGraph::new(RecipeId::from_raw(1), 1);
    short.add(TextureOp::Bricks as u16, vec![Param::int(8)], vec![]);
    assert!(ProcTextureApi::new().bake(&short, SEED).is_err());

    // An unknown operator code.
    let mut unknown = RecipeGraph::new(RecipeId::from_raw(1), 1);
    unknown.add(500, vec![], vec![]);
    assert!(ProcMeshApi::new().bake(&unknown, SEED).is_err());

    // Malformed serialized bytes decode to an error, not a panic.
    assert_eq!(
        RecipeGraph::deserialize(&[0x00, 0x01, 0x02]),
        Err(RecipeError::MalformedData)
    );
}

// 6. No runtime dependency on editor-only data: expanding from recipes that were
//    round-tripped through their serialized bytes gives the identical result.
//    This is the "ship the recipe, not the resources" proof.
#[test]
fn player_depends_only_on_recipe_bytes() {
    let authored = DemoRecipes::build();
    let from_bytes = authored.round_tripped();
    assert_eq!(
        expand(&authored, SEED).1.fingerprint(),
        expand(&from_bytes, SEED).1.fingerprint()
    );

    // And the two texture bakes are byte-identical, not merely same-sized.
    let tex = ProcTextureApi::new();
    assert_eq!(
        tex.bake(&authored.brick_texture, SEED).unwrap(),
        tex.bake(&from_bytes.brick_texture, SEED).unwrap()
    );
}
