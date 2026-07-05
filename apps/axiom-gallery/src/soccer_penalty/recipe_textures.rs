//! Reusable **texture recipe macros** for the soccer diorama's surfaces.
//!
//! These replace the deleted hand-authored `penalty_textures` pixel arrays: every
//! surface is now a small [`RecipeGraph`] of texture operators
//! (`axiom-proc-texture`), parameterised entirely by
//! [`crate::soccer_penalty::recipe_style::SoccerRecipeStyle`]. The render bridge
//! (`penalty_render_meshed`) bakes each graph and registers the result through
//! the ordinary `RunningApp::add_texture_data` hook — the app authors no pixels of
//! its own.
//!
//! Lettering (the jersey number, the "AXIOM"/"SPORTS" ad boards) is baked by the
//! [`TextureOp::Text`] operator, which was added to `axiom-proc-texture` for this
//! port — so the readable text the old pixel-art carried is reproduced from
//! recipes rather than dropped.
//!
//! Recipe ids are allocated in the `600..` band so they never collide with the
//! engine's other recipe consumers.

use axiom_proc_texture::TextureOp;
use axiom_recipe::{Color, Param, RecipeGraph, RecipeId, Scalar};

use crate::soccer_penalty::recipe_style::SoccerRecipeStyle;

fn s(v: f32) -> Param {
    Param::scalar(Scalar::new(v))
}
fn i(v: u32) -> Param {
    Param::int(v)
}
fn c(col: Color) -> Param {
    Param::color(col)
}

/// Build the parameter list for a [`TextureOp::Text`] node: the
/// `[width, height, fg, bg, scale, count]` header followed by the string packed
/// four ASCII bytes per word (low byte first), matching the operator's decoder.
/// (App code, so an ordinary loop is fine — the branchless spine is the operators
/// themselves, which this only *invokes*.)
fn text_params(w: u32, h: u32, fg: Color, bg: Color, scale: u32, text: &str) -> Vec<Param> {
    let bytes = text.as_bytes();
    let mut params = vec![i(w), i(h), c(fg), c(bg), i(scale), i(bytes.len() as u32)];
    for chunk in bytes.chunks(4) {
        let word = chunk.iter().enumerate().fold(0u32, |w, (k, &b)| w | ((b as u32) << (8 * k)));
        params.push(i(word));
    }
    params
}

/// The stable recipe id of every soccer texture.
pub mod ids {
    pub const CROWD: u64 = 600;
    pub const JERSEY: u64 = 601;
    pub const KEEPER: u64 = 602;
    pub const AD_AXIOM: u64 = 603;
    pub const AD_GENERIC: u64 = 604;
    pub const BALL: u64 = 605;
    pub const SKIN: u64 = 606;
}

/// A packed crowd: a dark terrace densely filled with individual shirt "seats".
///
/// The recipe `Noise` operator is *smooth* value noise (interpolated between
/// lattice points), so it can only make soft colour smears — it cannot reproduce
/// the hard per-pixel dots the old hand-authored hash crowd used. The hard-edged
/// operators are `Checker` / `Bricks`, so the crowd is built from three fine,
/// deliberately **misaligned** staggered brick grids (each rows-of-seats of one
/// shirt colour over the dark terrace) blended together: the mismatched row/column
/// counts interfere so the seats never line up into stripes, reading as a dense
/// stand of thousands rather than a few colour bands.
pub fn crowd(style: &SoccerRecipeStyle) -> RecipeGraph {
    let r = style.texture_res;
    let p = &style.palette;
    let d = p.crowd_dark;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::CROWD), 1);
    let red = g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(18), i(13), i(1), c(p.crowd_shirt_a), c(d)], vec![]);
    let blue = g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(20), i(15), i(1), c(p.crowd_shirt_b), c(d)], vec![]);
    let pale = g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(22), i(17), i(1), c(p.crowd_bright), c(d)], vec![]);
    let rb = g.add(TextureOp::Blend as u16, vec![s(0.5)], vec![red, blue]);
    g.add(TextureOp::Blend as u16, vec![s(0.4)], vec![rb, pale]);
    g
}

/// A woven jersey/kit fabric: fine directional noise value-ramped over the kit
/// base colour. `base`/`dark` pick the team.
fn fabric(id: u64, base: Color, dark: Color, res: u32) -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(id), 1);
    let weave = g.add(TextureOp::Noise as u16, vec![i(res), i(res), i(16), c(dark), c(base)], vec![]);
    g.add(TextureOp::ColorRamp as u16, vec![c(dark), c(base)], vec![weave]);
    g
}

/// The kicker jersey — blue woven fabric with the number "10" over it (the fabric
/// blended with a [`TextureOp::Text`] pass, since Blend has no alpha key). The
/// number is now expressible because the recipe layer gained a text operator.
pub fn jersey(style: &SoccerRecipeStyle) -> RecipeGraph {
    let r = style.texture_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::JERSEY), 1);
    let weave = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(16), c(p.jersey_dark), c(p.jersey)], vec![]);
    let base = g.add(TextureOp::ColorRamp as u16, vec![c(p.jersey_dark), c(p.jersey)], vec![weave]);
    let number = g.add(TextureOp::Text as u16, text_params(r, r, p.ball_white, p.jersey, 4, "10"), vec![]);
    g.add(TextureOp::Blend as u16, vec![s(0.7)], vec![base, number]);
    g
}

/// The goalkeeper kit — yellow woven fabric.
pub fn keeper(style: &SoccerRecipeStyle) -> RecipeGraph {
    fabric(ids::KEEPER, style.palette.keeper, style.palette.keeper_dark, style.texture_res)
}

/// An ad board: the sponsor text (white) baked directly onto a coloured panel by
/// the [`TextureOp::Text`] operator — real lettering, now that the recipe layer
/// has a text op.
///
/// The panel is sized to the ad-board *mesh*, which is nearly square (~1.30 × 1.25
/// world units in `penalty_scene`). The earlier 80×20 (4:1) texture stretched
/// every 5×7 glyph ~4× vertically on that near-square quad and, at only 20 px tall,
/// aliased into the illegible dark smear the boards read as under the low 426×240
/// retro render target. A 128×120 panel matches the mesh aspect (no vertical
/// stretch) and carries the lettering at scale 3, so "AXIOM"/"SPORTS" fills the
/// board width and stays crisp as the reference's white-on-red hoardings do.
fn ad_board(id: u64, text: &str, panel: Color, ink: Color) -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(id), 1);
    g.add(TextureOp::Text as u16, text_params(128, 120, ink, panel, 3, text), vec![]);
    g
}

/// The AXIOM ad board — white "AXIOM" on a red panel.
pub fn ad_axiom(style: &SoccerRecipeStyle) -> RecipeGraph {
    ad_board(ids::AD_AXIOM, "AXIOM", style.palette.ad_axiom, style.palette.ball_white)
}

/// The generic ad board — white "SPORTS" on a red panel (matching the AXIOM board).
pub fn ad_generic(style: &SoccerRecipeStyle) -> RecipeGraph {
    ad_board(ids::AD_GENERIC, "SPORTS", style.palette.ad_generic, style.palette.ball_white)
}

/// The soccer ball: white leather with a faint noise grain. The classic dark
/// panels are the proud quads the scene places on the ball's front hemisphere —
/// baking a regular brick/checker grid here instead aliases into speckle under the
/// retro downsampling, so the texture stays a soft near-white leather.
pub fn ball(style: &SoccerRecipeStyle) -> RecipeGraph {
    let r = style.detail_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::BALL), 1);
    let grain = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(4), c(p.ball_grain), c(p.ball_white)], vec![]);
    g.add(TextureOp::ColorRamp as u16, vec![c(p.ball_grain), c(p.ball_white)], vec![grain]);
    g
}

/// Athlete skin: a warm base with faint noise dither so heads/hands aren't
/// perfectly flat.
pub fn skin(style: &SoccerRecipeStyle) -> RecipeGraph {
    let r = style.detail_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::SKIN), 1);
    let dither = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(8), c(p.skin_dark), c(p.skin)], vec![]);
    g.add(TextureOp::ColorRamp as u16, vec![c(p.skin_dark), c(p.skin)], vec![dither]);
    g
}

/// Every soccer texture recipe, paired with a stable name — the catalog the
/// render bridge and validation iterate.
pub fn catalog(style: &SoccerRecipeStyle) -> Vec<(&'static str, RecipeGraph)> {
    vec![
        ("crowd", crowd(style)),
        ("jersey", jersey(style)),
        ("keeper", keeper(style)),
        ("ad_axiom", ad_axiom(style)),
        ("ad_generic", ad_generic(style)),
        ("ball", ball(style)),
        ("skin", skin(style)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_texture_recipe_validates() {
        let style = SoccerRecipeStyle::stadium();
        for (name, recipe) in catalog(&style) {
            assert_eq!(recipe.validate(), Ok(()), "{name} texture recipe is a valid DAG");
        }
    }

    #[test]
    fn texture_ids_are_unique() {
        let style = SoccerRecipeStyle::stadium();
        let mut ids: Vec<u64> = catalog(&style).iter().map(|(_, r)| r.id().raw()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), catalog(&style).len(), "no two textures share a recipe id");
    }
}
