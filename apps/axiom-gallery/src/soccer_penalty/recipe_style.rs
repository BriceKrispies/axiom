//! The soccer diorama's **recipe art-direction style** — the single source of
//! truth for every generated texture's palette, resolution, and seed.
//!
//! This is the soccer counterpart of `examples/recipes/generated_micro_fps`'s
//! `style.rs`: instead of hand-authored pixel arrays (the deleted
//! `penalty_textures`), every surface texture is now a small operator graph
//! parameterised entirely by the values here, baked by `axiom-proc-texture`.
//! Change a colour or the seed once and every generated surface re-skins
//! deterministically.
//!
//! The colours mirror the existing flat material palette
//! ([`crate::soccer_penalty::low_poly_assets::palette`]) so the recipe-baked
//! surfaces sit inside the same art direction the render plan already flat-shades
//! with — the textures add turf/fabric/panel detail *over* those base colours,
//! they do not repaint the scene.

use axiom_recipe::Color;

/// The soccer facility palette, authored as packed recipe [`Color`]s. Each value
/// feeds a texture operator graph in [`crate::soccer_penalty::recipe_textures`].
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Packed terrace/shadow tone the crowd sits against.
    pub crowd_dark: Color,
    /// Two crowd shirt tones scattered over the terrace (a warm and a cool).
    pub crowd_shirt_a: Color,
    /// The second (cool) crowd shirt tone.
    pub crowd_shirt_b: Color,
    /// The brightest crowd highlight (pale shirts / skin).
    pub crowd_bright: Color,
    /// Kicker jersey fabric base + its woven shadow.
    pub jersey: Color,
    /// The kicker jersey's darker weave shadow.
    pub jersey_dark: Color,
    /// Goalkeeper kit fabric base + shadow.
    pub keeper: Color,
    /// The goalkeeper kit's darker weave shadow.
    pub keeper_dark: Color,
    /// The "AXIOM" ad board (red) panel + its dark rail.
    pub ad_axiom: Color,
    /// The AXIOM board's dark rail/mortar.
    pub ad_axiom_dark: Color,
    /// The generic ad board (red, matching the AXIOM board) panel + its dark rail.
    pub ad_generic: Color,
    /// The generic board's dark rail/mortar.
    pub ad_generic_dark: Color,
    /// The ball's white leather + its dark panel seams.
    pub ball_white: Color,
    /// A slightly greyer white — faint leather grain over `ball_white` (the dark
    /// panels are the proud quads the scene places, not the texture).
    pub ball_grain: Color,
    /// The ball's dark panels / seams.
    pub ball_dark: Color,
    /// Athlete skin base + its faint dither shadow.
    pub skin: Color,
    /// The skin's faint dither shadow.
    pub skin_dark: Color,
}

/// Every art-direction knob for the generated soccer surfaces.
#[derive(Debug, Clone, Copy)]
pub struct SoccerRecipeStyle {
    /// The deterministic bake seed (fixes any noise-driven surface).
    pub seed: u64,
    /// The shared palette.
    pub palette: Palette,
    /// Edge size (px) of a large surface texture (crowd / kits).
    pub texture_res: u32,
    /// Edge size (px) of a small detail texture (skin / ball).
    pub detail_res: u32,
}

impl SoccerRecipeStyle {
    /// The canonical shipped soccer art direction and bake seed.
    pub fn stadium() -> Self {
        Self {
            seed: 0x0000_50CC_E12A_0001,
            palette: Palette {
                crowd_dark: Color::rgba(0x1A, 0x18, 0x1E, 0xFF),
                crowd_shirt_a: Color::rgba(0xC8, 0x3C, 0x3C, 0xFF),
                crowd_shirt_b: Color::rgba(0x3C, 0x5A, 0xBE, 0xFF),
                crowd_bright: Color::rgba(0xD2, 0xC0, 0x46, 0xFF),
                jersey: Color::rgba(0x28, 0x4C, 0xC8, 0xFF),
                jersey_dark: Color::rgba(0x18, 0x2E, 0x82, 0xFF),
                keeper: Color::rgba(0xE6, 0xC8, 0x28, 0xFF),
                keeper_dark: Color::rgba(0x96, 0x82, 0x1A, 0xFF),
                ad_axiom: Color::rgba(0xB0, 0x28, 0x2E, 0xFF),
                ad_axiom_dark: Color::rgba(0x3C, 0x0E, 0x10, 0xFF),
                ad_generic: Color::rgba(0xB0, 0x28, 0x2E, 0xFF),
                ad_generic_dark: Color::rgba(0x3C, 0x0E, 0x10, 0xFF),
                ball_white: Color::rgba(0xF4, 0xF4, 0xF8, 0xFF),
                ball_grain: Color::rgba(0xD8, 0xD8, 0xE2, 0xFF),
                ball_dark: Color::rgba(0x10, 0x10, 0x14, 0xFF),
                skin: Color::rgba(0xD2, 0xA0, 0x80, 0xFF),
                skin_dark: Color::rgba(0xAA, 0x78, 0x60, 0xFF),
            },
            texture_res: 48,
            detail_res: 32,
        }
    }
}

impl Default for SoccerRecipeStyle {
    fn default() -> Self {
        Self::stadium()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stadium_style_is_populated() {
        let s = SoccerRecipeStyle::stadium();
        assert_eq!(s.texture_res, 48);
        assert_ne!(s.palette.jersey.packed(), s.palette.keeper.packed());
        assert_ne!(s.palette.ball_white.packed(), s.palette.ball_dark.packed());
    }
}
