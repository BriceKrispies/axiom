//! The **packed-recipe export** and the **size report**.
//!
//! The packed recipe is the shippable artifact: every texture and mesh recipe,
//! serialized (each already `SchemaVersion`-stamped) into one seed-stamped blob.
//! Its `StableHash` is the determinism hash — the same seed always produces the
//! same bytes and the same hash. The size report expands the level once and
//! records every budgeted figure.

use std::fmt;
use std::time::Instant;

use axiom_kernel::{BinaryWriter, StableHash};
use axiom_recipe::RecipeGraph;

use crate::scenes::expand_level;
use crate::style::Style;
use crate::{materials, meshes, textures};

/// The packed recipe blob plus its identity.
#[derive(Debug, Clone)]
pub struct PackedProject {
    /// The packed bytes (what ships).
    pub bytes: Vec<u8>,
    /// How many recipes are packed.
    pub recipe_count: usize,
    /// The determinism hash over the packed bytes.
    pub determinism_hash: u64,
}

/// Every recipe in the project, in a stable order (textures then meshes).
fn all_recipes(style: &Style) -> Vec<RecipeGraph> {
    textures::catalog(style)
        .into_iter()
        .map(|(_, r)| r)
        .chain(meshes::catalog(style).into_iter().map(|(_, r)| r))
        .collect()
}

/// Pack every recipe into one seed-stamped blob and hash it.
pub fn pack(style: &Style) -> PackedProject {
    let recipes = all_recipes(style);
    let mut writer = BinaryWriter::new();
    writer.write_u64(style.level_seed);
    writer.write_u32(recipes.len() as u32);
    for recipe in &recipes {
        writer.write_byte_slice(&recipe.serialize());
    }
    let bytes = writer.into_bytes();
    let determinism_hash = StableHash::of_bytes(&bytes).raw();
    PackedProject { recipe_count: recipes.len(), determinism_hash, bytes }
}

/// The full size / performance report — deliverable of the size-report command.
#[derive(Debug, Clone)]
pub struct SizeReport {
    /// The level seed.
    pub seed: u64,
    /// Determinism hash (over the packed recipe).
    pub determinism_hash: u64,
    /// Packed recipe size (bytes) — the shipped artifact.
    pub packed_recipe_bytes: usize,
    /// Generated texture RAM (bytes).
    pub texture_memory_bytes: usize,
    /// Generated vertex count.
    pub mesh_vertices: usize,
    /// Generated index count.
    pub mesh_indices: usize,
    /// Number of texture recipes.
    pub texture_count: usize,
    /// Number of mesh recipes.
    pub mesh_count: usize,
    /// Number of materials.
    pub material_count: usize,
    /// Number of enemies.
    pub enemy_count: usize,
    /// Renderable instances placed.
    pub renderable_count: usize,
    /// Total scene entities.
    pub entity_count: usize,
    /// Wall-clock microseconds to expand the level.
    pub expansion_micros: u128,
}

impl SizeReport {
    /// Expand the level once and gather every figure.
    pub fn generate(style: &Style) -> Self {
        let packed = pack(style);
        let start = Instant::now();
        let level = expand_level(style);
        let expansion_micros = start.elapsed().as_micros();
        Self {
            seed: style.level_seed,
            determinism_hash: packed.determinism_hash,
            packed_recipe_bytes: packed.bytes.len(),
            texture_memory_bytes: level.texture_bytes,
            mesh_vertices: level.mesh_vertices,
            mesh_indices: level.mesh_indices,
            texture_count: textures::catalog(style).len(),
            mesh_count: meshes::catalog(style).len(),
            material_count: materials::catalog(style).len(),
            enemy_count: level.layout.enemies.len(),
            renderable_count: level.renderable_count,
            entity_count: level.entity_count,
            expansion_micros,
        }
    }
}

impl fmt::Display for SizeReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Generated Micro-FPS — size & performance report")?;
        writeln!(f, "  level seed             : {:#018x}", self.seed)?;
        writeln!(f, "  determinism hash       : {:#018x}", self.determinism_hash)?;
        writeln!(f, "  packed recipe (shipped): {} bytes", self.packed_recipe_bytes)?;
        writeln!(f, "  texture RAM (generated): {} bytes ({:.2} MB)", self.texture_memory_bytes, self.texture_memory_bytes as f64 / 1.0e6)?;
        writeln!(f, "  mesh vertices          : {}", self.mesh_vertices)?;
        writeln!(f, "  mesh indices           : {}", self.mesh_indices)?;
        writeln!(f, "  recipes                : {} textures, {} meshes, {} materials", self.texture_count, self.mesh_count, self.material_count)?;
        writeln!(f, "  scene                  : {} renderables, {} enemies, {} entities", self.renderable_count, self.enemy_count, self.entity_count)?;
        write!(f, "  expansion time         : {} us", self.expansion_micros)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_is_deterministic() {
        let style = Style::facility();
        let a = pack(&style);
        let b = pack(&style);
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.determinism_hash, b.determinism_hash);
        assert_eq!(a.recipe_count, textures::catalog(&style).len() + meshes::catalog(&style).len());
    }

    #[test]
    fn report_generates_and_is_within_budget() {
        let r = SizeReport::generate(&Style::facility());
        assert!(r.packed_recipe_bytes < 150_000);
        assert!(r.mesh_vertices < 200_000);
        assert!(r.texture_memory_bytes < 96 * 1024 * 1024);
    }
}
