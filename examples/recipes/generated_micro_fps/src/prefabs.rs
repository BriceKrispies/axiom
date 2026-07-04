//! Prefabs — reusable bundles of *a generated mesh + a material + a gameplay
//! tag*. A prefab carries no world transform; the [`crate::grammar`] decides
//! where each instance goes, because the mesh recipe already bakes the object's
//! size. The tag is how the scene and the gameplay ruleset classify a placed
//! instance (structure, door, enemy, weapon, exit, …).

use crate::meshes::ids as mesh;

/// What a placed prefab *is*, for gameplay and scene wiring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// Static geometry (walls, ceiling).
    Structure,
    /// The floor plane.
    Floor,
    /// A passable door.
    Door,
    /// A locked gate that blocks progress until unlocked.
    Gate,
    /// A destructible / decorative prop (crate, pipe).
    Prop,
    /// A light fixture (also drives a scene light).
    Light,
    /// Enemy variant A — "grunt".
    EnemyA,
    /// Enemy variant B — "sentry".
    EnemyB,
    /// The weapon pickup.
    Weapon,
    /// The exit / win marker.
    Exit,
}

/// A reusable object bundle.
#[derive(Debug, Clone, Copy)]
pub struct Prefab {
    /// Stable name.
    pub name: &'static str,
    /// The mesh recipe (an id in [`crate::meshes::ids`]).
    pub mesh_recipe_id: u64,
    /// The material name (in [`crate::materials`]).
    pub material: &'static str,
    /// Gameplay classification.
    pub tag: Tag,
}

impl Prefab {
    const fn new(name: &'static str, mesh_recipe_id: u64, material: &'static str, tag: Tag) -> Self {
        Self { name, mesh_recipe_id, material, tag }
    }
}

/// Every prefab in the facility. The ceiling reuses the floor slab mesh; the gate
/// reuses the door slab mesh with a locked material — reuse is the point of a
/// prefab library.
pub fn catalog() -> Vec<Prefab> {
    vec![
        Prefab::new("wall", mesh::WALL, "wall", Tag::Structure),
        Prefab::new("floor", mesh::FLOOR, "floor", Tag::Floor),
        Prefab::new("ceiling", mesh::FLOOR, "ceiling", Tag::Structure),
        Prefab::new("door", mesh::DOOR, "door", Tag::Door),
        Prefab::new("gate", mesh::DOOR, "gate_locked", Tag::Gate),
        Prefab::new("crate", mesh::CRATE, "crate", Tag::Prop),
        Prefab::new("pipe", mesh::PIPE, "pipe", Tag::Prop),
        Prefab::new("light", mesh::LIGHT, "light", Tag::Light),
        Prefab::new("enemy_grunt", mesh::ENEMY_A, "enemy_a", Tag::EnemyA),
        Prefab::new("enemy_sentry", mesh::ENEMY_B, "enemy_b", Tag::EnemyB),
        Prefab::new("weapon", mesh::WEAPON, "weapon", Tag::Weapon),
        Prefab::new("exit", mesh::EXIT, "exit", Tag::Exit),
        // Structural set dressing (reusable, placed by the grammar).
        Prefab::new("pillar", mesh::PILLAR, "metal", Tag::Structure),
        Prefab::new("base_trim", mesh::TRIM_BAND, "wood", Tag::Structure),
        Prefab::new("ceiling_trim", mesh::CEILING_TRIM, "trim", Tag::Light),
        Prefab::new("platform", mesh::PLATFORM, "metal", Tag::Structure),
        Prefab::new("bracket", mesh::BRACKET, "wood", Tag::Structure),
        Prefab::new("vent", mesh::VENT, "metal", Tag::Structure),
        // Weapon viewmodel parts.
        Prefab::new("weapon_body", mesh::WEAPON_BODY, "weapon", Tag::Weapon),
        Prefab::new("weapon_barrel", mesh::WEAPON_BARREL, "metal", Tag::Weapon),
        Prefab::new("weapon_grip", mesh::WEAPON_GRIP, "wood", Tag::Weapon),
        // Enemy detail: a grunt head and a glowing sentry eye.
        Prefab::new("grunt_head", mesh::ENEMY_HEAD, "enemy_a", Tag::EnemyA),
        Prefab::new("sentry_eye", mesh::BRACKET, "trim", Tag::EnemyB),
    ]
}

/// Look a prefab up by name.
pub fn by_name(name: &str) -> Option<Prefab> {
    catalog().into_iter().find(|p| p.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;
    use crate::{materials, meshes};

    #[test]
    fn every_prefab_resolves_to_a_mesh_and_material() {
        let style = Style::facility();
        let mesh_ids: Vec<u64> = meshes::catalog(&style).iter().map(|(_, r)| r.id().raw()).collect();
        for p in catalog() {
            assert!(mesh_ids.contains(&p.mesh_recipe_id), "prefab {} mesh resolves", p.name);
            assert!(materials::by_name(&style, p.material).is_some(), "prefab {} material resolves", p.name);
        }
    }

    #[test]
    fn both_enemy_variants_exist() {
        assert!(catalog().iter().any(|p| p.tag == Tag::EnemyA));
        assert!(catalog().iter().any(|p| p.tag == Tag::EnemyB));
    }
}
