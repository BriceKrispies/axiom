//! Material specs — the binding of a generated texture to a palette base color
//! (and an optional emissive glow). Materials are not procedural operators, so a
//! [`MaterialSpec`] is a small value the scene resolves at expansion time: it
//! bakes the referenced texture recipe, registers it, and builds a
//! `Material::lit(base).with_custom_texture(handle)`.
//!
//! Every base color comes from the one [`Style`] palette, so the facility shares
//! a global look.

use axiom_recipe::Color;

use crate::style::Style;
use crate::textures::ids as tex;

/// A material: a palette base color, the generated texture it wears, an optional
/// emissive glow, and a roughness (`0` glossy … `1` matte) — the last gives each
/// surface family a distinct material read (shiny metal vs matte stone/wood).
#[derive(Debug, Clone, Copy)]
pub struct MaterialSpec {
    /// Stable name — prefabs reference materials by this.
    pub name: &'static str,
    /// Base (albedo) color, from the palette.
    pub base: Color,
    /// The texture recipe this material wears (an id in [`crate::textures::ids`]).
    pub texture_recipe_id: u64,
    /// Optional emissive color for self-lit surfaces.
    pub emissive: Option<Color>,
    /// Surface roughness, `0.0` glossy … `1.0` matte.
    pub roughness: f32,
}

impl MaterialSpec {
    const fn lit(name: &'static str, base: Color, texture_recipe_id: u64) -> Self {
        Self { name, base, texture_recipe_id, emissive: None, roughness: 0.8 }
    }

    const fn glowing(name: &'static str, base: Color, texture_recipe_id: u64, emissive: Color) -> Self {
        Self { name, base, texture_recipe_id, emissive: Some(emissive), roughness: 0.8 }
    }

    const fn rough(mut self, roughness: f32) -> Self {
        self.roughness = roughness;
        self
    }
}

/// Every material, sharing the global palette. Ceiling reuses the wall texture in
/// a darker base; new wood/metal/trim families give structure real variety.
pub fn catalog(style: &Style) -> Vec<MaterialSpec> {
    let p = &style.palette;
    vec![
        // Structure carries a soft self-glow so the enclosed rooms stay readable
        // even under the software (canvas2d) renderer's darker profile.
        MaterialSpec::glowing("wall", p.wall, tex::WALL, soft(p.wall)).rough(0.9),
        MaterialSpec::glowing("floor", p.floor, tex::FLOOR, soft(p.floor)).rough(0.7),
        MaterialSpec::glowing("ceiling", p.ceiling, tex::WALL, soft(p.ceiling)).rough(0.9),
        MaterialSpec::lit("door", p.door, tex::DOOR).rough(0.5),
        MaterialSpec::glowing("gate_locked", p.gate_locked, tex::GATE_LOCKED, dim(p.gate_locked)).rough(0.5),
        MaterialSpec::glowing("gate_open", p.gate_open, tex::GATE_OPEN, dim(p.gate_open)).rough(0.5),
        MaterialSpec::lit("crate", p.prop, tex::CRATE).rough(0.85),
        MaterialSpec::lit("pipe", p.pipe, tex::PIPE).rough(0.35),
        MaterialSpec::glowing("light", p.light, tex::LIGHT, p.light_glow).rough(0.6),
        MaterialSpec::glowing("enemy_a", p.enemy_a, tex::ENEMY_A, dim(p.enemy_a)).rough(0.7),
        MaterialSpec::lit("enemy_b", p.enemy_b, tex::ENEMY_B).rough(0.5),
        MaterialSpec::glowing("weapon", p.weapon, tex::WEAPON, p.weapon_glow).rough(0.25),
        MaterialSpec::glowing("exit", p.exit, tex::EXIT, p.exit).rough(0.5),
        // New structural material families.
        MaterialSpec::lit("wood", p.wood, tex::WOOD).rough(0.85),
        MaterialSpec::lit("metal", p.metal, tex::METAL).rough(0.3),
        MaterialSpec::glowing("trim", p.trim, tex::TRIM, p.trim_glow).rough(0.4),
    ]
}

/// A dimmed copy of a color — a soft self-glow that keeps a surface readable in
/// shadow without blowing out.
const fn dim(c: Color) -> Color {
    Color::rgba(c.r() / 3, c.g() / 3, c.b() / 3, 0xFF)
}

/// A stronger self-glow for large structural surfaces (walls/floor) so an
/// enclosed room does not fall to black under a software renderer.
const fn soft(c: Color) -> Color {
    Color::rgba(c.r() / 2, c.g() / 2, c.b() / 2, 0xFF)
}

/// Look a material up by name (used by prefabs).
pub fn by_name(style: &Style, name: &str) -> Option<MaterialSpec> {
    catalog(style).into_iter().find(|m| m.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textures;

    #[test]
    fn every_material_texture_resolves_to_a_recipe() {
        let style = Style::facility();
        let texture_ids: Vec<u64> = textures::catalog(&style).iter().map(|(_, r)| r.id().raw()).collect();
        for m in catalog(&style) {
            assert!(texture_ids.contains(&m.texture_recipe_id), "material {} references a real texture recipe", m.name);
        }
    }

    #[test]
    fn material_names_are_unique() {
        let style = Style::facility();
        let mut names: Vec<&str> = catalog(&style).iter().map(|m| m.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), catalog(&style).len());
    }
}
