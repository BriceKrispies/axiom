//! Pass 3 — the fixed, named material palette.
//!
//! Every diorama object references a [`PenaltyMaterialId`] instead of a raw
//! color; this module is the single source of truth for what each material *is*
//! (its base color and whether it is unlit). The palette is a fixed, ordered
//! array — never a map — so it is deterministic, indexable by id, and testable
//! by name and order.
//!
//! Materials are flat base colors; the [`crate::soccer_penalty::penalty_light`] model shades
//! them per face at render time. HUD materials are marked `unlit` so the HUD
//! stays crisp and never receives lighting.

use crate::soccer_penalty::low_poly_assets::{palette, Rgba};

/// A stable, `#[repr(u8)]` id for each material. The discriminant equals the
/// material's index in [`PENALTY_PALETTE`], so [`material`] is a direct index —
/// no lookup, no map, no fallible unwrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PenaltyMaterialId {
    // Field & markings.
    FieldGrass,
    DarkerGrassBand,
    WhiteFieldLines,
    // Goal & net.
    GoalFrameWhite,
    NetOffWhite,
    // Goalie.
    GoalieJerseyYellow,
    GoalieShortsBlack,
    GoalieSkin,
    GoalieHair,
    GoalieGloves,
    // Kicker.
    KickerJerseyBlue,
    KickerShortsWhite,
    KickerSocksDark,
    KickerSkin,
    // Ball.
    BallWhite,
    BallDarkPanels,
    // Backdrop.
    CrowdMutedColors,
    CrowdMutedColorsAltA,
    CrowdMutedColorsAltB,
    StadiumWallDarkGray,
    AdBoardRed,
    AdBoardDark,
    // Shadows.
    BlobShadow,
    // HUD (all unlit).
    HudDarkPanel,
    HudWhiteText,
    HudYellowHighlight,
    HudGreenSuccess,
    HudRedWarning,
    // Athlete footwear / socks (appended for the humanoid character kit; appended
    // at the end so every existing discriminant/index stays stable).
    KickerShoes,
    GoalieShoes,
    GoalieSocks,
}

/// One named material: its id, human name, base color, and lit/unlit flag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyMaterial {
    pub id: PenaltyMaterialId,
    pub name: &'static str,
    pub base_color: Rgba,
    /// When true the material is never shaded by the light model (HUD, shadows).
    pub unlit: bool,
}

const fn lit(id: PenaltyMaterialId, name: &'static str, base_color: Rgba) -> PenaltyMaterial {
    PenaltyMaterial { id, name, base_color, unlit: false }
}

const fn unlit(id: PenaltyMaterialId, name: &'static str, base_color: Rgba) -> PenaltyMaterial {
    PenaltyMaterial { id, name, base_color, unlit: true }
}

/// The fixed material palette, ordered to match [`PenaltyMaterialId`]. This is
/// the whole palette — deterministic and stable.
pub const PENALTY_PALETTE: [PenaltyMaterial; 31] = [
    lit(PenaltyMaterialId::FieldGrass, "field grass", palette::GRASS_LIGHT),
    lit(PenaltyMaterialId::DarkerGrassBand, "darker grass band", palette::GRASS_DARK),
    lit(PenaltyMaterialId::WhiteFieldLines, "white field lines", palette::LINE_WHITE),
    // Posts/crossbar: near-pure bright white so the solid frame reads distinctly
    // in front of the dimmer net (as in the reference).
    lit(PenaltyMaterialId::GoalFrameWhite, "goal frame white", Rgba::rgb(0.99, 0.99, 1.0)),
    // Net: dimmer greyed off-white so the fine mesh recedes behind the bright
    // frame instead of merging with it into one boxy cage.
    lit(PenaltyMaterialId::NetOffWhite, "net off-white", Rgba::rgb(0.70, 0.72, 0.76)),
    lit(PenaltyMaterialId::GoalieJerseyYellow, "goalie jersey yellow", palette::GOALIE_JERSEY),
    lit(PenaltyMaterialId::GoalieShortsBlack, "goalie shorts black", palette::GOALIE_SHORTS),
    lit(PenaltyMaterialId::GoalieSkin, "goalie skin", palette::GOALIE_SKIN),
    lit(PenaltyMaterialId::GoalieHair, "goalie hair", Rgba::rgb(0.12, 0.09, 0.07)),
    lit(PenaltyMaterialId::GoalieGloves, "goalie gloves", palette::GOALIE_GLOVES),
    lit(PenaltyMaterialId::KickerJerseyBlue, "kicker jersey blue", palette::KICKER_JERSEY),
    lit(PenaltyMaterialId::KickerShortsWhite, "kicker shorts white", palette::KICKER_SHORTS),
    lit(PenaltyMaterialId::KickerSocksDark, "kicker socks dark", palette::KICKER_SOCKS),
    lit(PenaltyMaterialId::KickerSkin, "kicker skin", palette::KICKER_SKIN),
    lit(PenaltyMaterialId::BallWhite, "ball white", palette::BALL_WHITE),
    lit(PenaltyMaterialId::BallDarkPanels, "ball dark panels", Rgba::rgb(0.10, 0.11, 0.13)),
    // Crowd: a coherent terracotta -> ochre -> amber family. The reference crowd
    // is a uniformly WARM ochre/tan mass with no cold-blue section, so the old
    // cold-blue alt-A card (0.32, 0.44, 0.66) is replaced by a mid ochre that
    // sits between the terracotta and amber cards instead of fighting them.
    lit(PenaltyMaterialId::CrowdMutedColors, "crowd muted colors", Rgba::rgb(0.62, 0.30, 0.32)),
    lit(PenaltyMaterialId::CrowdMutedColorsAltA, "crowd muted colors (alt a)", Rgba::rgb(0.64, 0.44, 0.24)),
    lit(PenaltyMaterialId::CrowdMutedColorsAltB, "crowd muted colors (alt b)", Rgba::rgb(0.78, 0.62, 0.26)),
    lit(PenaltyMaterialId::StadiumWallDarkGray, "stadium wall dark gray", palette::STADIUM_WALL),
    lit(PenaltyMaterialId::AdBoardRed, "ad board red", Rgba::rgb(0.80, 0.15, 0.19)),
    lit(PenaltyMaterialId::AdBoardDark, "ad board red (generic)", Rgba::rgb(0.80, 0.15, 0.19)),
    unlit(PenaltyMaterialId::BlobShadow, "blob shadow", palette::BLOB_SHADOW),
    unlit(PenaltyMaterialId::HudDarkPanel, "HUD dark panel", Rgba::new(0.05, 0.06, 0.09, 0.85)),
    unlit(PenaltyMaterialId::HudWhiteText, "HUD white text", Rgba::rgb(0.96, 0.97, 0.98)),
    unlit(PenaltyMaterialId::HudYellowHighlight, "HUD yellow highlight", Rgba::rgb(0.98, 0.83, 0.16)),
    unlit(PenaltyMaterialId::HudGreenSuccess, "HUD green success", Rgba::rgb(0.30, 0.82, 0.38)),
    unlit(PenaltyMaterialId::HudRedWarning, "HUD red warning", Rgba::rgb(0.86, 0.24, 0.24)),
    lit(PenaltyMaterialId::KickerShoes, "kicker shoes", Rgba::rgb(0.06, 0.06, 0.08)),
    lit(PenaltyMaterialId::GoalieShoes, "goalie shoes", Rgba::rgb(0.06, 0.06, 0.08)),
    lit(PenaltyMaterialId::GoalieSocks, "goalie socks", Rgba::rgb(0.10, 0.10, 0.12)),
];

/// The material for an id. A direct index — the id's discriminant is its
/// palette position.
pub fn material(id: PenaltyMaterialId) -> PenaltyMaterial {
    PENALTY_PALETTE[id as usize]
}

/// The whole ordered palette.
pub fn palette() -> &'static [PenaltyMaterial] {
    &PENALTY_PALETTE
}
