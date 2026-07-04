//! Reusable **texture recipe macros**. Each function returns a small
//! [`RecipeGraph`] of texture operators, parameterized entirely by the
//! [`Style`]: change the palette or grime once and every surface re-skins. None
//! is hand-authored pixel data — they are all operator graphs the expander bakes.
//!
//! Recipe ids are allocated in the `100..` band (textures); meshes use `200..`.

use axiom_proc_texture::TextureOp;
use axiom_recipe::{Color, Param, RecipeGraph, RecipeId, Scalar};

use crate::style::Style;

fn s(v: f32) -> Param {
    Param::scalar(Scalar::new(v))
}
fn i(v: u32) -> Param {
    Param::int(v)
}
fn c(col: Color) -> Param {
    Param::color(col)
}

/// The stable recipe id of every texture, so materials/prefabs reference them by
/// a durable number and validation can prove they all resolve.
pub mod ids {
    pub const WALL: u64 = 100;
    pub const FLOOR: u64 = 101;
    pub const DOOR: u64 = 102;
    pub const GATE_LOCKED: u64 = 103;
    pub const GATE_OPEN: u64 = 104;
    pub const CRATE: u64 = 105;
    pub const PIPE: u64 = 106;
    pub const LIGHT: u64 = 107;
    pub const ENEMY_A: u64 = 108;
    pub const ENEMY_B: u64 = 109;
    pub const WEAPON: u64 = 110;
    pub const EXIT: u64 = 111;
    pub const WOOD: u64 = 112;
    pub const METAL: u64 = 113;
    pub const TRIM: u64 = 114;
}

/// A stone/block wall with deep seams and grime: large mortar-seamed blocks, a
/// fine sub-panel grid, a noise grime pass, and a value spread for depth.
pub fn wall(style: &Style) -> RecipeGraph {
    let r = style.texture_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::WALL), 1);
    // Big stone blocks with thick dark seams.
    let blocks = g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(3), i(2), i(4), c(p.wall), c(p.wall_grime)], vec![]);
    // A finer sub-grid of panel lines over the blocks.
    let subgrid = g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(6), i(6), i(1), c(p.wall), c(p.metal_dark)], vec![]);
    let seamed = g.add(TextureOp::Blend as u16, vec![s(0.35)], vec![blocks, subgrid]);
    // Grime mottle, then a value ramp for high-contrast depth.
    let grime = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(7), c(p.wall_grime), c(p.wall)], vec![]);
    let dirtied = g.add(TextureOp::Blend as u16, vec![s(0.5 * style.grime)], vec![seamed, grime]);
    g.add(TextureOp::ColorRamp as u16, vec![c(p.wall_grime), c(p.wall)], vec![dirtied]);
    g
}

/// A worn checker-tile floor: an alternating light/dark tile grid, grime-blended
/// and value-ramped for wear.
pub fn floor(style: &Style) -> RecipeGraph {
    let r = style.texture_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::FLOOR), 1);
    let tiles = g.add(TextureOp::Checker as u16, vec![i(r), i(r), i(style.tile_px), c(p.floor), c(p.floor_tile)], vec![]);
    let grime = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(6), c(p.floor_tile), c(p.floor_wear)], vec![]);
    let worn = g.add(TextureOp::Blend as u16, vec![s(0.4 * style.grime)], vec![tiles, grime]);
    g.add(TextureOp::ColorRamp as u16, vec![c(p.floor_tile), c(p.floor_wear)], vec![worn]);
    g
}

/// A wood support/trim surface: a warm grain gradient streaked with grain noise.
pub fn wood(style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::WOOD), 1);
    let grain = g.add(TextureOp::Gradient as u16, vec![i(r), i(r), c(p.wood), c(p.wood_dark)], vec![]);
    let streaks = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(14), c(p.wood_dark), c(p.wood)], vec![]);
    g.add(TextureOp::Blend as u16, vec![s(0.45)], vec![grain, streaks]);
    g
}

/// A brushed-metal surface: fine directional noise, value-ramped, with a sheen.
pub fn metal(style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::METAL), 1);
    let brushed = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(20), c(p.metal_dark), c(p.metal)], vec![]);
    let ramped = g.add(TextureOp::ColorRamp as u16, vec![c(p.metal_dark), c(p.metal)], vec![brushed]);
    let sheen = g.add(TextureOp::Gradient as u16, vec![i(r), i(r), c(p.metal), c(p.metal_dark)], vec![]);
    g.add(TextureOp::Blend as u16, vec![s(0.3)], vec![ramped, sheen]);
    g
}

/// An emissive ornamental trim: a fine glow/graphite checker, so the trim reads
/// as a repeating lit ornament.
pub fn trim(style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::TRIM), 1);
    g.add(TextureOp::Checker as u16, vec![i(r), i(r), i(8), c(p.trim_glow), c(p.trim)], vec![]);
    g
}

/// A horizontally-slatted door in a given base color with hazard banding — the
/// shared shape for normal doors and gates.
fn slatted_door(id: u64, base: Color, style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let mut g = RecipeGraph::new(RecipeId::from_raw(id), 1);
    g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(5), i(1), i(3), c(base), c(style.palette.hazard)], vec![]);
    g
}

/// A normal (openable) door surface — amber slats.
pub fn door(style: &Style) -> RecipeGraph {
    slatted_door(ids::DOOR, style.palette.door, style)
}

/// A locked-gate surface — hazard-red slats.
pub fn gate_locked(style: &Style) -> RecipeGraph {
    slatted_door(ids::GATE_LOCKED, style.palette.gate_locked, style)
}

/// An unlocked-gate surface — go-green slats.
pub fn gate_open(style: &Style) -> RecipeGraph {
    slatted_door(ids::GATE_OPEN, style.palette.gate_open, style)
}

/// A supply crate: a 3×3 bolt-panel face with hazard edges.
pub fn crate_surface(style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::CRATE), 1);
    g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(3), i(3), i(2), c(style.palette.prop), c(style.palette.hazard)], vec![]);
    g
}

/// A pipe surface: a vertical light→dark gradient that fakes cylindrical shading.
pub fn pipe(style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::PIPE), 1);
    // Gradient runs left→right; combined with the pipe's cylinder UVs this reads
    // as a round highlight.
    g.add(TextureOp::Gradient as u16, vec![i(r), i(r), c(style.palette.pipe), c(style.palette.wall_grime)], vec![]);
    g
}

/// A light-fixture surface: a bright warm glow gradient.
pub fn light(style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::LIGHT), 1);
    g.add(TextureOp::Gradient as u16, vec![i(r), i(r), c(style.palette.light_glow), c(style.palette.light)], vec![]);
    g
}

/// An enemy body surface: an organic mottle ramped over the variant color, then
/// broken up with fine darker cracks so it reads as living tissue / plating.
fn enemy(id: u64, base: Color, dark: Color, style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let mut g = RecipeGraph::new(RecipeId::from_raw(id), 1);
    let mottle = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(4), c(dark), c(base)], vec![]);
    let ramped = g.add(TextureOp::ColorRamp as u16, vec![c(dark), c(base)], vec![mottle]);
    let cracks = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(13), c(base), c(dark)], vec![]);
    g.add(TextureOp::Blend as u16, vec![s(0.3)], vec![ramped, cracks]);
    g
}

/// The "grunt" enemy surface (variant A) — hot orange-red.
pub fn enemy_a(style: &Style) -> RecipeGraph {
    enemy(ids::ENEMY_A, style.palette.enemy_a, style.palette.wall_grime, style)
}

/// The "sentry" enemy surface (variant B) — violet.
pub fn enemy_b(style: &Style) -> RecipeGraph {
    enemy(ids::ENEMY_B, style.palette.enemy_b, style.palette.ceiling, style)
}

/// The weapon surface: brushed steel tinted toward the weapon color, with a
/// bright energy strip.
pub fn weapon(style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::WEAPON), 1);
    let brushed = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(18), c(p.metal_dark), c(p.weapon)], vec![]);
    let steel = g.add(TextureOp::ColorRamp as u16, vec![c(p.metal_dark), c(p.weapon)], vec![brushed]);
    let energy = g.add(TextureOp::Gradient as u16, vec![i(r), i(r), c(p.weapon), c(p.weapon_glow)], vec![]);
    g.add(TextureOp::Blend as u16, vec![s(0.4)], vec![steel, energy]);
    g
}

/// The exit / win-marker surface: a bright go-green glow.
pub fn exit(style: &Style) -> RecipeGraph {
    let r = style.detail_res;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::EXIT), 1);
    g.add(TextureOp::Gradient as u16, vec![i(r), i(r), c(style.palette.exit), c(style.palette.light_glow)], vec![]);
    g
}

/// Every texture recipe, paired with a stable name — the catalog materials,
/// packing, and validation iterate.
pub fn catalog(style: &Style) -> Vec<(&'static str, RecipeGraph)> {
    vec![
        ("wall", wall(style)),
        ("floor", floor(style)),
        ("door", door(style)),
        ("gate_locked", gate_locked(style)),
        ("gate_open", gate_open(style)),
        ("crate", crate_surface(style)),
        ("pipe", pipe(style)),
        ("light", light(style)),
        ("enemy_a", enemy_a(style)),
        ("enemy_b", enemy_b(style)),
        ("weapon", weapon(style)),
        ("exit", exit(style)),
        ("wood", wood(style)),
        ("metal", metal(style)),
        ("trim", trim(style)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_texture_recipe_validates() {
        let style = Style::facility();
        for (name, recipe) in catalog(&style) {
            assert_eq!(recipe.validate(), Ok(()), "{name} texture recipe is a valid DAG");
        }
    }

    #[test]
    fn texture_ids_are_unique() {
        let style = Style::facility();
        let mut ids: Vec<u64> = catalog(&style).iter().map(|(_, r)| r.id().raw()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), catalog(&style).len(), "no two textures share a recipe id");
    }
}
