//! The project's **validation checks**, as reusable functions the `validate` CLI
//! command prints and the `tests/validation.rs` suite asserts. Each returns a
//! `(name, passed)` pair; [`run`] gathers all nine required proofs.

use axiom_recipe::RecipeGraph;

use crate::grammar::build_level;
use crate::pack::{pack, SizeReport};
use crate::scenes::expand_level;
use crate::style::Style;
use crate::{materials, meshes, prefabs, textures};

/// Budgets (the task's stated limits).
pub mod budget {
    /// Packed recipe must stay under 150 KB.
    pub const PACKED_BYTES: usize = 150_000;
    /// Generated vertices under 200,000.
    pub const MESH_VERTICES: usize = 200_000;
    /// Generated texture memory under 96 MB.
    pub const TEXTURE_BYTES: usize = 96 * 1024 * 1024;
    /// Entity count is kept comfortably bounded for a small browser game.
    pub const ENTITY_COUNT: usize = 500;
}

/// Every recipe in the project.
fn all_recipes(style: &Style) -> Vec<RecipeGraph> {
    textures::catalog(style)
        .into_iter()
        .map(|(_, r)| r)
        .chain(meshes::catalog(style).into_iter().map(|(_, r)| r))
        .collect()
}

/// 1. The same seed produces the same determinism hash (and a different seed
///    produces a different one).
fn determinism_holds() -> bool {
    let base = Style::facility();
    let mut other = Style::facility();
    other.level_seed ^= 0xABCD_1234;
    pack(&base).determinism_hash == pack(&base).determinism_hash
        && pack(&base).determinism_hash != pack(&other).determinism_hash
}

/// 2. The project expands from recipe data alone — every recipe round-trips
///    through its serialized bytes and re-validates (no editor-only state).
fn expands_from_data_only(style: &Style) -> bool {
    all_recipes(style).iter().all(|r| {
        RecipeGraph::deserialize(&r.serialize()).map(|d| d == *r).unwrap_or(false)
    })
}

/// 6. The recipe graphs have no cycles (validation is exactly the acyclic check).
fn no_cycles(style: &Style) -> bool {
    all_recipes(style).iter().all(|r| r.validate().is_ok())
}

/// 7. Every referenced recipe / resource resolves: materials → textures, prefabs
///    → meshes + materials, placements → prefabs.
fn everything_resolves(style: &Style) -> bool {
    let tex_ids: Vec<u64> = textures::catalog(style).iter().map(|(_, r)| r.id().raw()).collect();
    let mesh_ids: Vec<u64> = meshes::catalog(style).iter().map(|(_, r)| r.id().raw()).collect();
    let materials_ok = materials::catalog(style).iter().all(|m| tex_ids.contains(&m.texture_recipe_id));
    let prefabs_ok = prefabs::catalog().iter().all(|p| {
        mesh_ids.contains(&p.mesh_recipe_id) && materials::by_name(style, p.material).is_some()
    });
    let placements_ok = build_level(style).placements.iter().all(|pl| prefabs::by_name(pl.prefab).is_some());
    materials_ok && prefabs_ok && placements_ok
}

/// Run all nine required checks and return `(name, passed)` in report order.
pub fn run(style: &Style) -> Vec<(&'static str, bool)> {
    let report = SizeReport::generate(style);
    let packed = pack(style);
    let level = expand_level(style);
    vec![
        ("1. same seed → same determinism hash", determinism_holds()),
        ("2. expands from recipe data only (no editor data)", expands_from_data_only(style)),
        ("3. entity count bounded", level.entity_count <= budget::ENTITY_COUNT),
        ("4. vertex + index counts bounded", level.mesh_vertices < budget::MESH_VERTICES && level.mesh_indices < budget::MESH_VERTICES * 6),
        ("5. texture memory bounded", level.texture_bytes < budget::TEXTURE_BYTES),
        ("6. recipe graphs are acyclic", no_cycles(style)),
        ("7. all referenced recipes/resources resolve", everything_resolves(style)),
        ("8. packed recipe produced (< 150 KB)", !packed.bytes.is_empty() && packed.bytes.len() < budget::PACKED_BYTES),
        ("9. size report generated", report.entity_count > 0 && report.packed_recipe_bytes > 0),
    ]
}

/// Whether every check passed.
pub fn all_pass(style: &Style) -> bool {
    run(style).iter().all(|(_, ok)| *ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_nine_checks_pass() {
        for (name, ok) in run(&Style::facility()) {
            assert!(ok, "{name}");
        }
        assert!(all_pass(&Style::facility()));
    }
}
