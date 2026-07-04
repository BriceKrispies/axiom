//! The global art-direction style — the single source of truth for the whole
//! project. Every texture, mesh, material, and grammar decision reads its seed,
//! palette, and parameters from one [`Style`] value, so the entire facility can
//! be re-skinned or re-seeded by editing this one struct.
//!
//! Art direction: a compact industrial sci-fi *training facility* — slightly
//! grimy, readable silhouettes, high-contrast door and enemy language, low-poly
//! but not blocky.

use axiom::prelude::{Color as EngineColor, Ratio};
use axiom_recipe::Color;

/// The facility's shared color palette. Colors are authored as packed RGB and
/// consumed both as recipe texture parameters and as engine material colors.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Steel wall plating.
    pub wall: Color,
    /// Grimy wall grime/streak tone (darker than `wall`).
    pub wall_grime: Color,
    /// Dark floor decking.
    pub floor: Color,
    /// Floor wear/scuff tone.
    pub floor_wear: Color,
    /// Overhead ceiling / structure.
    pub ceiling: Color,
    /// A normal (openable) door — high-contrast amber.
    pub door: Color,
    /// A locked gate — high-contrast hazard red, so "you cannot pass yet" reads
    /// instantly.
    pub gate_locked: Color,
    /// An unlocked gate — go-green.
    pub gate_open: Color,
    /// Supply crates and props — olive drab.
    pub prop: Color,
    /// Pipework — gunmetal.
    pub pipe: Color,
    /// Light-fixture housing (its emissive glow is `light_glow`).
    pub light: Color,
    /// Emissive light glow — warm.
    pub light_glow: Color,
    /// Enemy variant A ("grunt") — hot orange-red, maximally readable.
    pub enemy_a: Color,
    /// Enemy variant B ("sentry") — violet, a distinct second read.
    pub enemy_b: Color,
    /// The weapon pickup body — cyan-steel.
    pub weapon: Color,
    /// The weapon's emissive energy accent.
    pub weapon_glow: Color,
    /// The exit / win marker — bright go-green.
    pub exit: Color,
    /// Hazard striping accent — caution yellow.
    pub hazard: Color,
    /// The dark tile of the worn checker floor (pairs with `floor` as the light
    /// tile).
    pub floor_tile: Color,
    /// Wood support / trim — warm timber.
    pub wood: Color,
    /// Wood grain shadow.
    pub wood_dark: Color,
    /// Brushed structural metal — cool steel.
    pub metal: Color,
    /// Metal shadow / recess.
    pub metal_dark: Color,
    /// Ornamental trim housing — near-black graphite.
    pub trim: Color,
    /// Emissive ornamental trim — cool cyan glow (contrasts the warm lights).
    pub trim_glow: Color,
}

/// Every art-direction knob for the facility.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    /// The deterministic level seed — the single number that fixes the entire
    /// generated layout, textures, and meshes.
    pub level_seed: u64,
    /// The shared palette.
    pub palette: Palette,
    /// Surface grime, `0.0` clean … `1.0` filthy — drives noise contrast in the
    /// wall/floor textures.
    pub grime: f32,
    /// Edge/contrast emphasis, `0.0` flat … `1.0` punchy — brightens accents.
    pub contrast: f32,
    /// Base edge size (px) of a wall / floor texture.
    pub texture_res: u32,
    /// Base edge size (px) of a smaller prop / door / enemy texture.
    pub detail_res: u32,
    /// World size (meters) of one square wall/floor panel.
    pub panel_size: f32,
    /// Interior room height (meters).
    pub room_height: f32,
    /// Checker-tile cell size (px) for the floor texture.
    pub tile_px: u32,
}

impl Style {
    /// The canonical facility style — the shipped art direction and seed.
    pub fn facility() -> Self {
        Self {
            level_seed: 0x0000_0FAC_1117_5EED,
            palette: Palette {
                wall: Color::rgba(0x70, 0x7E, 0x8C, 0xFF),
                wall_grime: Color::rgba(0x3A, 0x42, 0x4A, 0xFF),
                floor: Color::rgba(0x44, 0x4C, 0x54, 0xFF),
                floor_wear: Color::rgba(0x66, 0x6C, 0x72, 0xFF),
                ceiling: Color::rgba(0x34, 0x3A, 0x42, 0xFF),
                door: Color::rgba(0xE0, 0x90, 0x2A, 0xFF),
                gate_locked: Color::rgba(0xC8, 0x30, 0x28, 0xFF),
                gate_open: Color::rgba(0x40, 0xC0, 0x60, 0xFF),
                prop: Color::rgba(0x7A, 0x6A, 0x48, 0xFF),
                pipe: Color::rgba(0x56, 0x5E, 0x66, 0xFF),
                light: Color::rgba(0x8A, 0x90, 0x98, 0xFF),
                light_glow: Color::rgba(0xFF, 0xD8, 0xA0, 0xFF),
                enemy_a: Color::rgba(0xD6, 0x45, 0x30, 0xFF),
                enemy_b: Color::rgba(0x9B, 0x3F, 0xB5, 0xFF),
                weapon: Color::rgba(0x3F, 0xA8, 0xC8, 0xFF),
                weapon_glow: Color::rgba(0x90, 0xF0, 0xFF, 0xFF),
                exit: Color::rgba(0x40, 0xC0, 0x60, 0xFF),
                hazard: Color::rgba(0xE8, 0xC0, 0x20, 0xFF),
                floor_tile: Color::rgba(0x24, 0x28, 0x2E, 0xFF),
                wood: Color::rgba(0x7A, 0x50, 0x2C, 0xFF),
                wood_dark: Color::rgba(0x40, 0x28, 0x14, 0xFF),
                metal: Color::rgba(0x62, 0x6E, 0x7A, 0xFF),
                metal_dark: Color::rgba(0x2A, 0x30, 0x38, 0xFF),
                trim: Color::rgba(0x16, 0x1A, 0x20, 0xFF),
                trim_glow: Color::rgba(0x3C, 0xD0, 0xE0, 0xFF),
            },
            grime: 0.55,
            contrast: 0.85,
            texture_res: 128,
            detail_res: 64,
            panel_size: 4.0,
            room_height: 4.0,
            tile_px: 32,
        }
    }
}

/// Convert a packed recipe [`Color`] into an engine material [`EngineColor`]
/// (linear-space, alpha dropped — materials carry their own opacity).
pub fn engine_color(c: Color) -> EngineColor {
    let f = |v: u8| Ratio::new(f32::from(v) / 255.0).expect("channel is in 0..=1");
    EngineColor::linear_rgb(f(c.r()), f(c.g()), f(c.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facility_style_is_populated() {
        let s = Style::facility();
        assert_eq!(s.texture_res, 128);
        assert_ne!(s.palette.enemy_a, s.palette.enemy_b);
        // Colors convert into engine space without panicking.
        let _ = engine_color(s.palette.wall);
    }
}
