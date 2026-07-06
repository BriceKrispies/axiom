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
    pub const TURF: u64 = 607;
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
///
/// The crowd bakes at its **own** resolution (`CROWD_RES`), decoupled from the
/// shared `texture_res` the kits use: the reference terrace is a mass of *many
/// hundreds* of individuals across the frame, but ~20 seat-columns on the 48px
/// kit texture — then softened by the anti-moiré blur — collapses into a handful
/// of soft colour blobs, not a packed stand. Baking at 128px lets the seat grids
/// carry ~3x the column/row count (finer, more numerous individuals) with pixels
/// to spare, so the crowd survives the anti-moiré blur *and* the low retro target
/// still reading as a dense stand rather than smeared bands. The blur radius is
/// lifted proportionally (2→4) so the higher-frequency seat grid is diffused by
/// the same *relative* amount that killed the moiré at 48px.
pub fn crowd(style: &SoccerRecipeStyle) -> RecipeGraph {
    const CROWD_RES: u32 = 128;
    let r = CROWD_RES;
    let p = &style.palette;
    let d = p.crowd_dark;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::CROWD), 1);
    let red = g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(52), i(38), i(1), c(p.crowd_shirt_a), c(d)], vec![]);
    let blue = g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(58), i(43), i(1), c(p.crowd_shirt_b), c(d)], vec![]);
    let pale = g.add(TextureOp::Bricks as u16, vec![i(r), i(r), i(64), i(49), i(1), c(p.crowd_bright), c(d)], vec![]);
    let rb = g.add(TextureOp::Blend as u16, vec![s(0.5)], vec![red, blue]);
    let seats = g.add(TextureOp::Blend as u16, vec![s(0.4)], vec![rb, pale]);
    // A final box blur diffuses the hard per-seat brick edges — which aliased
    // into harsh vertical moiré streaks through the low retro render target —
    // into the soft, out-of-focus colour haze of a real stadium crowd seen past
    // the pitch, matching the reference's blurred terrace and removing the
    // dominant background artifact. Radius scales with CROWD_RES (2 at 48px →
    // 4 at 128px) so the relative softening — and the moiré suppression — holds.
    g.add(TextureOp::Blur as u16, vec![i(4)], vec![seats]);
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

/// The soccer ball: white leather with the classic dark pentagon panels baked
/// straight onto the sphere's UVs as filled [`TextureOp::Spots`]. The panels used
/// to be six separate world-space quads the scene parked on the ball's front
/// hemisphere — so they floated in place at the penalty spot while the ball flew
/// (only the sphere carried the per-frame ball pose). Baked into the surface
/// texture the panels are *part of the ball*: they translate with it now, and roll
/// with it the moment the ball is given spin. A handful of large spots (unlike a
/// brick/checker grid of many small cells) survives the retro downsample without
/// aliasing into speckle. Centres/radii are texel-space on the `detail_res` grid;
/// the rosette — a central pentagon ringed by five — sits on the camera-facing
/// meridian (`u≈0.25` → `x≈res/4`, a touch above the equator for the elevated
/// camera), the truncated-icosahedron signature of a real ball.
pub fn ball(style: &SoccerRecipeStyle) -> RecipeGraph {
    let r = style.detail_res;
    let p = &style.palette;
    // Rosette laid out on a nominal 32-texel grid, then scaled to detail_res.
    let q = r as f32 / 32.0;
    // A mostly-white ball: one small central pentagon ringed by five, well spread
    // so they read as separate panels (an oversized or clustered rosette turns the
    // ball into a black blob under the retro downsample). The ring is spread wider
    // in `y` than `x` because equirectangular `u` compresses horizontally near the
    // equator, so equal texel steps in x cover more of the sphere than in y.
    let rosette: [(f32, f32, f32); 6] =
        [(8.0, 12.5, 2.5), (8.0, 5.5, 2.0), (13.5, 10.0, 2.0), (12.0, 18.5, 2.0), (3.5, 18.5, 2.0), (2.5, 10.0, 2.0)];
    let mut params = vec![i(r), i(r), c(p.ball_white), c(p.ball_dark), i(rosette.len() as u32)];
    rosette.iter().for_each(|&(cx, cy, rad)| {
        params.push(i((cx * q).round() as u32));
        params.push(i((cy * q).round() as u32));
        params.push(i((rad * q).round().max(1.0) as u32));
    });
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::BALL), 1);
    g.add(TextureOp::Spots as u16, params, vec![]);
    g
}

/// The pitch turf: a fine mown grain — soft near-neutral value noise that
/// modulates the flat grass band base colour, so the largest surface in the
/// frame reads as textured turf instead of a dead-flat green slab. The mowing
/// stripes stay geometry (the alternating light/dark band quads); this only adds
/// the intra-band grain those flat quads are missing. Each band quad carries its
/// own `0..1` UV, so the texture tiles once per band and the grain reads fine.
pub fn turf(style: &SoccerRecipeStyle) -> RecipeGraph {
    let r = style.texture_res;
    let p = &style.palette;
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::TURF), 1);
    let grain = g.add(TextureOp::Noise as u16, vec![i(r), i(r), i(20), c(p.turf_grain), c(p.turf_light)], vec![]);
    g.add(TextureOp::ColorRamp as u16, vec![c(p.turf_grain), c(p.turf_light)], vec![grain]);
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
        ("turf", turf(style)),
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
