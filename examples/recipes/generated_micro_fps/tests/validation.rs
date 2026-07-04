//! Integration validation suite — the nine required proofs, exercised through the
//! project's public API exactly as the `validate` CLI command runs them, plus the
//! explicit budget assertions.

use generated_micro_fps::pack::{pack, SizeReport};
use generated_micro_fps::scenes::expand_level;
use generated_micro_fps::validation::{self, budget};
use generated_micro_fps::Style;

#[test]
fn all_nine_checks_pass() {
    let style = Style::facility();
    for (name, ok) in validation::run(&style) {
        assert!(ok, "validation check failed: {name}");
    }
}

#[test]
fn determinism_hash_is_stable_for_a_seed() {
    let style = Style::facility();
    assert_eq!(pack(&style).determinism_hash, pack(&style).determinism_hash);
}

#[test]
fn a_different_seed_changes_the_hash() {
    let mut a = Style::facility();
    let mut b = Style::facility();
    a.level_seed = 1;
    b.level_seed = 2;
    assert_ne!(pack(&a).determinism_hash, pack(&b).determinism_hash);
}

#[test]
fn every_budget_is_met() {
    let level = expand_level(&Style::facility());
    let report = SizeReport::generate(&Style::facility());
    assert!(report.packed_recipe_bytes < budget::PACKED_BYTES, "packed {} bytes", report.packed_recipe_bytes);
    assert!(level.mesh_vertices < budget::MESH_VERTICES, "verts {}", level.mesh_vertices);
    assert!(level.texture_bytes < budget::TEXTURE_BYTES, "texture bytes {}", level.texture_bytes);
    assert!(level.entity_count <= budget::ENTITY_COUNT, "entities {}", level.entity_count);
}

#[test]
fn the_size_report_is_produced() {
    let report = SizeReport::generate(&Style::facility());
    assert!(report.entity_count > 0);
    assert!(report.renderable_count > 0);
    assert!(report.texture_count > 0 && report.mesh_count > 0 && report.material_count > 0);
    assert!(report.enemy_count == 4);
    // The report renders to text (the shipped command output).
    assert!(format!("{report}").contains("determinism hash"));
}
