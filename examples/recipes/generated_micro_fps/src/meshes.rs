//! Reusable **mesh recipe macros**. Each returns a small [`RecipeGraph`] of mesh
//! operators sized in world units, so a prefab only has to position and rotate
//! it. Low-poly but not blocky: boxes are beveled, enemies and pipes use the
//! rounder primitives, and the weapon/exit read as distinct silhouettes.
//!
//! Recipe ids are allocated in the `200..` band (meshes).

use axiom_proc_mesh::MeshOp;
use axiom_recipe::{Param, RecipeGraph, RecipeId, Scalar};

use crate::style::Style;

fn s(v: f32) -> Param {
    Param::scalar(Scalar::new(v))
}

/// Stable mesh recipe ids.
pub mod ids {
    pub const WALL: u64 = 200;
    pub const FLOOR: u64 = 201;
    pub const DOOR: u64 = 202;
    pub const CRATE: u64 = 203;
    pub const PIPE: u64 = 204;
    pub const LIGHT: u64 = 205;
    pub const ENEMY_A: u64 = 206;
    pub const ENEMY_B: u64 = 207;
    pub const WEAPON: u64 = 208;
    pub const EXIT: u64 = 209;
    pub const PILLAR: u64 = 210;
    pub const TRIM_BAND: u64 = 211;
    pub const PLATFORM: u64 = 212;
    pub const BRACKET: u64 = 213;
    pub const VENT: u64 = 214;
    pub const CEILING_TRIM: u64 = 215;
    pub const WEAPON_BODY: u64 = 216;
    pub const WEAPON_BARREL: u64 = 217;
    pub const WEAPON_GRIP: u64 = 218;
    pub const ENEMY_HEAD: u64 = 219;
}

/// A unit cube, non-uniformly scaled, then UV-projected — the shared shape for
/// every axis-aligned slab (walls, floors, doors, light bars, the exit pillar).
fn scaled_box(id: u64, sx: f32, sy: f32, sz: f32) -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(id), 1);
    let cube = g.add(MeshOp::Cube as u16, vec![s(1.0)], vec![]);
    let sized = g.add(MeshOp::Transform as u16, vec![s(0.0), s(0.0), s(0.0), s(sx), s(sy), s(sz)], vec![cube]);
    g.add(MeshOp::UVProject as u16, vec![s(0.5)], vec![sized]);
    g
}

/// A beveled, non-uniformly scaled box — for crates and the weapon body (low-poly
/// but not blocky).
fn beveled_box(id: u64, sx: f32, sy: f32, sz: f32, bevel: f32) -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(id), 1);
    let cube = g.add(MeshOp::Cube as u16, vec![s(1.0)], vec![]);
    let rounded = g.add(MeshOp::Bevel as u16, vec![s(bevel)], vec![cube]);
    let sized = g.add(MeshOp::Transform as u16, vec![s(0.0), s(0.0), s(0.0), s(sx), s(sy), s(sz)], vec![rounded]);
    let uv = g.add(MeshOp::UVProject as u16, vec![s(0.5)], vec![sized]);
    g.add(MeshOp::Triangulate as u16, vec![], vec![uv]);
    g
}

/// A wall panel: one `panel_size` × `room_height` slab, thin in Z.
pub fn wall_panel(style: &Style) -> RecipeGraph {
    scaled_box(ids::WALL, style.panel_size, style.room_height, 0.25)
}

/// A floor tile: one `panel_size` square slab, thin in Y.
pub fn floor_panel(style: &Style) -> RecipeGraph {
    scaled_box(ids::FLOOR, style.panel_size, 0.2, style.panel_size)
}

/// A door leaf: a 2×3 slab.
pub fn door(_style: &Style) -> RecipeGraph {
    scaled_box(ids::DOOR, 2.0, 3.0, 0.3)
}

/// A supply crate: a ~1.2 m beveled cube.
pub fn crate_mesh(_style: &Style) -> RecipeGraph {
    beveled_box(ids::CRATE, 1.2, 1.2, 1.2, 0.08)
}

/// A pipe segment: a thin capped cylinder running along +Y.
pub fn pipe(style: &Style) -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::PIPE), 1);
    let cyl = g.add(MeshOp::Cylinder as u16, vec![s(0.18), s(style.panel_size), s(12.0)], vec![]);
    g.add(MeshOp::UVProject as u16, vec![s(0.5)], vec![cyl]);
    g
}

/// A ceiling light bar: a flat wide slab.
pub fn light_fixture(_style: &Style) -> RecipeGraph {
    scaled_box(ids::LIGHT, 1.6, 0.2, 0.5)
}

/// Enemy A ("grunt"): a beveled box, noise-displaced to an irregular silhouette,
/// scaled tall. Distinct from the smooth cylindrical sentry.
pub fn enemy_a(_style: &Style) -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::ENEMY_A), 1);
    let cube = g.add(MeshOp::Cube as u16, vec![s(1.0)], vec![]);
    let rounded = g.add(MeshOp::Bevel as u16, vec![s(0.12)], vec![cube]);
    let lumpy = g.add(MeshOp::Displace as u16, vec![s(0.12)], vec![rounded]);
    let sized = g.add(MeshOp::Transform as u16, vec![s(0.0), s(0.0), s(0.0), s(0.9), s(1.7), s(0.9)], vec![lumpy]);
    g.add(MeshOp::Triangulate as u16, vec![], vec![sized]);
    g
}

/// Enemy B ("sentry"): a tall thin beveled cylinder — a clean, different read.
pub fn enemy_b(_style: &Style) -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::ENEMY_B), 1);
    let cyl = g.add(MeshOp::Cylinder as u16, vec![s(0.45), s(2.0), s(10.0)], vec![]);
    let rounded = g.add(MeshOp::Bevel as u16, vec![s(0.05)], vec![cyl]);
    g.add(MeshOp::UVProject as u16, vec![s(0.5)], vec![rounded]);
    g
}

/// The weapon pickup: a small elongated beveled box.
pub fn weapon_pickup(_style: &Style) -> RecipeGraph {
    beveled_box(ids::WEAPON, 0.9, 0.28, 0.28, 0.1)
}

/// The exit marker: a tall glowing pillar.
pub fn exit_marker(_style: &Style) -> RecipeGraph {
    scaled_box(ids::EXIT, 1.0, 3.5, 1.0)
}

/// A structural pillar: a tall beveled column spanning floor to ceiling.
pub fn pillar(style: &Style) -> RecipeGraph {
    beveled_box(ids::PILLAR, 0.6, style.room_height, 0.6, 0.06)
}

/// A trim band: a long thin bar sized to a wall panel (a base/top skirting run).
pub fn trim_band(style: &Style) -> RecipeGraph {
    scaled_box(ids::TRIM_BAND, style.panel_size, 0.35, 0.18)
}

/// An emissive ceiling-trim strip: a long slim bar of glowing ornament.
pub fn ceiling_trim(style: &Style) -> RecipeGraph {
    scaled_box(ids::CEILING_TRIM, style.panel_size, 0.18, 0.22)
}

/// A raised platform: a wide, low beveled slab.
pub fn platform(_style: &Style) -> RecipeGraph {
    beveled_box(ids::PLATFORM, 3.2, 0.5, 3.2, 0.1)
}

/// A support bracket: a small beveled block that trims a pillar/platform join.
pub fn bracket(_style: &Style) -> RecipeGraph {
    beveled_box(ids::BRACKET, 0.45, 0.7, 0.45, 0.05)
}

/// A wall vent: a shallow slatted panel (its slats come from the metal texture).
pub fn vent(_style: &Style) -> RecipeGraph {
    scaled_box(ids::VENT, 1.1, 0.7, 0.12)
}

/// The viewmodel weapon **body** — a beveled receiver.
pub fn weapon_body(_style: &Style) -> RecipeGraph {
    beveled_box(ids::WEAPON_BODY, 0.5, 0.22, 0.24, 0.04)
}

/// The viewmodel weapon **barrel** — a slim bar running forward (−Z).
pub fn weapon_barrel(_style: &Style) -> RecipeGraph {
    scaled_box(ids::WEAPON_BARREL, 0.1, 0.1, 0.6)
}

/// The viewmodel weapon **grip** — a small angled handle block.
pub fn weapon_grip(_style: &Style) -> RecipeGraph {
    beveled_box(ids::WEAPON_GRIP, 0.15, 0.32, 0.18, 0.03)
}

/// An enemy **head** — a small beveled block that tops an enemy body, giving it a
/// readable silhouette.
pub fn enemy_head(_style: &Style) -> RecipeGraph {
    beveled_box(ids::ENEMY_HEAD, 0.55, 0.45, 0.55, 0.12)
}

/// Every mesh recipe, paired with a stable name.
pub fn catalog(style: &Style) -> Vec<(&'static str, RecipeGraph)> {
    vec![
        ("wall_panel", wall_panel(style)),
        ("floor_panel", floor_panel(style)),
        ("door", door(style)),
        ("crate", crate_mesh(style)),
        ("pipe", pipe(style)),
        ("light_fixture", light_fixture(style)),
        ("enemy_a", enemy_a(style)),
        ("enemy_b", enemy_b(style)),
        ("weapon_pickup", weapon_pickup(style)),
        ("exit_marker", exit_marker(style)),
        ("pillar", pillar(style)),
        ("trim_band", trim_band(style)),
        ("ceiling_trim", ceiling_trim(style)),
        ("platform", platform(style)),
        ("bracket", bracket(style)),
        ("vent", vent(style)),
        ("weapon_body", weapon_body(style)),
        ("weapon_barrel", weapon_barrel(style)),
        ("weapon_grip", weapon_grip(style)),
        ("enemy_head", enemy_head(style)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mesh_recipe_validates() {
        let style = Style::facility();
        for (name, recipe) in catalog(&style) {
            assert_eq!(recipe.validate(), Ok(()), "{name} mesh recipe is a valid DAG");
        }
    }

    #[test]
    fn mesh_ids_are_unique() {
        let style = Style::facility();
        let mut ids: Vec<u64> = catalog(&style).iter().map(|(_, r)| r.id().raw()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), catalog(&style).len());
    }
}
