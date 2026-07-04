//! Pass 2 — deterministic depth ordering and net layering.
//!
//! This module owns the app's render-ordering model. It takes the flat
//! [`DioramaObject`] list plus the [`HudModel`] and produces a single, stably
//! ordered [`PenaltyRenderPlan`] that both a Canvas2D-style painter and a
//! hardware-depth renderer can consume (see `PASS_2_DEPTH_ORDERING.md`).
//!
//! ## The ordering model
//! Every render item carries a [`PenaltySortKey`] of three parts, compared in
//! this order:
//! 1. [`PenaltyDrawLayer`] — a fixed, explicit back-to-front bucket.
//! 2. a **coarse depth bucket** — a quantized world-depth so that, *within* a
//!    layer, farther primitives draw first.
//! 3. the **stable object ordinal** ([`ObjectId`](crate::soccer_penalty::penalty_scene::ObjectId))
//!    — the final, total tie-breaker for equal layer/depth.
//!
//! Nothing in the sort depends on hash-map iteration, pointer addresses,
//! allocation order, wall-clock time, or randomness: items are collected into
//! an explicit `Vec` and sorted by this total key. Two builds are equal.
//!
//! ## Net layering (the fake-depth trick)
//! The net is split into two roles ([`DioramaRole::RearNet`],
//! [`DioramaRole::FrontNet`]) mapped to two layers
//! ([`PenaltyDrawLayer::RearNet`] before the actors,
//! [`PenaltyDrawLayer::FrontNet`] after them). This makes the rear net read as
//! *behind* the goalie/ball/kicker and the front net as *in front of* them,
//! giving the goal real perceived depth without simulating the net — a
//! deliberate retro 32-bit-style trick.
//!
//! ## Pass 3 — materials, flat shading, and unlit HUD
//! Each world item resolves its [`PenaltyMaterialId`] against the palette and
//! carries a flat-shaded representative color (the material's base color shaded
//! by the [`PenaltyStylePass`] light model for the top face). HUD items are
//! **unlit** — they carry their materials verbatim and never receive lighting.
//! Materials and shading never affect the sort key, so ordering stays exactly
//! as Pass 2 defined it.

use axiom_math::{Quat, Vec3};

use crate::soccer_penalty::low_poly_assets::{PrimitiveShape, Rgba, WORLD_UP};
use crate::soccer_penalty::penalty_hud::PenaltyHudModel;
use crate::soccer_penalty::penalty_materials::{material, PenaltyMaterialId};
use crate::soccer_penalty::penalty_scene::{DioramaObject, DioramaRole};
use crate::soccer_penalty::penalty_style_pass::PenaltyStylePass;
use crate::soccer_penalty::static_diorama::CameraConfig;

/// The fixed, explicit draw buckets, declared strictly back-to-front. A
/// field-less `derive(Ord)` orders variants by declaration, so a lower variant
/// draws first. This is the single source of truth for "what draws before
/// what".
///
/// `Background` and `ForegroundEffects` are intentionally reserved (empty in
/// Pass 2): they exist so a later stage can slot a sky/backdrop and impact
/// particles into the ordering without renumbering anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PenaltyDrawLayer {
    /// Reserved backmost layer (sky / far backdrop). Empty in Pass 2.
    Background,
    /// Fake crowd cards.
    Crowd,
    /// Stadium wall and ad boards (the "AXIOM" board included).
    StadiumWall,
    /// The green pitch plane and grass bands.
    RearField,
    /// Painted field lines and the penalty spot.
    FieldLines,
    /// The rear net panel — behind the actors.
    RearNet,
    /// Goal posts and crossbar.
    GoalFrame,
    /// Blob shadows on the ground under the actors.
    ActorShadow,
    /// The goalie puppet.
    Goalie,
    /// The ball on the penalty spot.
    Ball,
    /// The foreground kicker puppet.
    Kicker,
    /// The front net panel — in front of the actors.
    FrontNet,
    /// Reserved foreground effects (impacts / particles). Empty in Pass 2.
    ForegroundEffects,
    /// The arcade HUD — always drawn last, over everything.
    Hud,
}

impl PenaltyDrawLayer {
    /// Every layer in canonical draw order, back-to-front.
    pub const ALL: [PenaltyDrawLayer; 14] = [
        PenaltyDrawLayer::Background,
        PenaltyDrawLayer::Crowd,
        PenaltyDrawLayer::StadiumWall,
        PenaltyDrawLayer::RearField,
        PenaltyDrawLayer::FieldLines,
        PenaltyDrawLayer::RearNet,
        PenaltyDrawLayer::GoalFrame,
        PenaltyDrawLayer::ActorShadow,
        PenaltyDrawLayer::Goalie,
        PenaltyDrawLayer::Ball,
        PenaltyDrawLayer::Kicker,
        PenaltyDrawLayer::FrontNet,
        PenaltyDrawLayer::ForegroundEffects,
        PenaltyDrawLayer::Hud,
    ];

    /// This layer's position in the canonical order (`0` == drawn first).
    pub fn order_index(self) -> u8 {
        self as u8
    }
}

/// The layer a semantic [`DioramaRole`] renders in. This is the one place role
/// → draw-layer is decided.
pub fn layer_for_role(role: DioramaRole) -> PenaltyDrawLayer {
    match role {
        DioramaRole::Field => PenaltyDrawLayer::RearField,
        DioramaRole::FieldLine => PenaltyDrawLayer::FieldLines,
        DioramaRole::PenaltySpot => PenaltyDrawLayer::FieldLines,
        DioramaRole::GoalFrame => PenaltyDrawLayer::GoalFrame,
        DioramaRole::RearNet => PenaltyDrawLayer::RearNet,
        DioramaRole::FrontNet => PenaltyDrawLayer::FrontNet,
        DioramaRole::Kicker => PenaltyDrawLayer::Kicker,
        DioramaRole::Ball => PenaltyDrawLayer::Ball,
        DioramaRole::Goalie => PenaltyDrawLayer::Goalie,
        DioramaRole::StadiumWall => PenaltyDrawLayer::StadiumWall,
        DioramaRole::AdBoard => PenaltyDrawLayer::StadiumWall,
        DioramaRole::CrowdCard => PenaltyDrawLayer::Crowd,
        DioramaRole::BlobShadow => PenaltyDrawLayer::ActorShadow,
        DioramaRole::BallTrail => PenaltyDrawLayer::ForegroundEffects,
        DioramaRole::GoalieDebugVolume => PenaltyDrawLayer::ForegroundEffects,
        DioramaRole::ImpactEffect => PenaltyDrawLayer::ForegroundEffects,
    }
}

// Coarse depth bucketing. The camera sits behind the scene at large `+Z` and
// looks down `-Z`, so a smaller world `z` is farther away. We bucket each
// object's *farthest* edge (`center.z - size.z/2`): because every grass band
// lies inside the base plane's extent, the big plane's farthest edge is at
// least as far as any band's, so the plane always sorts first within
// `RearField`. Buckets are coarse (whole meters-ish) so near-coplanar items
// fall together and the stable ordinal — not float noise — orders them.
pub const DEPTH_MIN_Z: f32 = -12.0;
pub const DEPTH_BUCKET_SIZE: f32 = 3.0;

/// The coarse depth bucket for a primitive: farther (smaller `z`) → smaller
/// bucket → drawn first. Pure, finite float math — no NaN, no clock, no rng.
pub fn depth_bucket(position: Vec3, size: Vec3) -> u16 {
    let farthest_z = position.z - size.z * 0.5;
    let steps = ((farthest_z - DEPTH_MIN_Z) / DEPTH_BUCKET_SIZE).max(0.0);
    steps as u16
}

/// The total, deterministic sort key. Compared field-by-field in declaration
/// order: layer, then coarse depth bucket, then stable object ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PenaltySortKey {
    pub layer: PenaltyDrawLayer,
    pub depth_bucket: u16,
    pub ordinal: u32,
}

/// Which HUD element a HUD render item represents. HUD items carry the values
/// on the [`HudModel`]; this names the slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyHudElement {
    Score,
    Round,
    Best,
    PowerMeter,
    Reticle,
    Instruction,
}

/// The payload of a render item: either a world primitive or a HUD element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PenaltyRenderContent {
    World {
        role: DioramaRole,
        shape: PrimitiveShape,
        position: Vec3,
        /// Local orientation about the primitive's center (identity for axis-aligned
        /// objects; posed for humanoid-kit limbs).
        rotation: Quat,
        size: Vec3,
        /// The named material this primitive uses (base color in the palette).
        material: PenaltyMaterialId,
        /// A representative flat-shaded color: the material base color shaded by
        /// the light model for the top face (or the base color, if unlit).
        shaded_color: Rgba,
        /// Whether the light model shades this primitive (false for shadows).
        lit: bool,
    },
    Hud {
        element: PenaltyHudElement,
        /// The dark panel material behind the element.
        panel_material: PenaltyMaterialId,
        /// The element's foreground (text/glyph/mark) material.
        foreground_material: PenaltyMaterialId,
        /// Always false — the HUD is crisp and unlit.
        lit: bool,
    },
}

/// One entry in the ordered render plan: its sort key, a greppable label, and
/// its payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyRenderItem {
    pub key: PenaltySortKey,
    pub label: &'static str,
    pub content: PenaltyRenderContent,
}

impl PenaltyRenderItem {
    /// Convenience accessor for the item's draw layer.
    pub fn layer(&self) -> PenaltyDrawLayer {
        self.key.layer
    }

    /// Whether this item is lit by the flat-shading model. World primitives are
    /// lit (except unlit materials such as blob shadows); HUD is never lit.
    pub fn is_lit(&self) -> bool {
        match self.content {
            PenaltyRenderContent::World { lit, .. } => lit,
            PenaltyRenderContent::Hud { lit, .. } => lit,
        }
    }
}

/// The complete render plan: render items in final sorted order, plus the
/// camera and the Pass 3 style pass (light model + retro 32-bit style) they render with.
#[derive(Debug, Clone, PartialEq)]
pub struct PenaltyRenderPlan {
    /// All render items, sorted by `PenaltySortKey` (back-to-front).
    pub items: Vec<PenaltyRenderItem>,
    pub camera: CameraConfig,
    pub style_pass: PenaltyStylePass,
}

/// The fixed order HUD elements are emitted in. The `Instruction` slot is only
/// included when the HUD carries one. All HUD items share
/// [`PenaltyDrawLayer::Hud`], so they always draw last.
const HUD_ORDER: [PenaltyHudElement; 5] = [
    PenaltyHudElement::Score,
    PenaltyHudElement::Round,
    PenaltyHudElement::Best,
    PenaltyHudElement::PowerMeter,
    PenaltyHudElement::Reticle,
];

fn hud_label(element: PenaltyHudElement) -> &'static str {
    match element {
        PenaltyHudElement::Score => "hud.score",
        PenaltyHudElement::Round => "hud.round",
        PenaltyHudElement::Best => "hud.best",
        PenaltyHudElement::PowerMeter => "hud.power",
        PenaltyHudElement::Reticle => "hud.reticle",
        PenaltyHudElement::Instruction => "hud.instruction",
    }
}

impl PenaltyRenderPlan {
    /// Build the render plan from the diorama objects, HUD, camera, and style
    /// pass. Deterministic: same inputs → identical, identically-ordered
    /// output. Materials are resolved and flat-shaded here; shading never
    /// influences the sort key.
    /// `hud` fixes the HUD *slots* present (always the full set); the live
    /// aim/power *values* travel on the [`PenaltyHudModel`] the app carries
    /// alongside the plan, so the renderer reads both.
    pub fn build(
        objects: &[DioramaObject],
        _hud: &PenaltyHudModel,
        camera: CameraConfig,
        style_pass: PenaltyStylePass,
    ) -> Self {
        // World items: one per object, keyed by (role→layer, depth, id), with
        // the material resolved and flat-shaded for its representative top face.
        let mut items: Vec<PenaltyRenderItem> = objects
            .iter()
            .map(|o| {
                let mat = material(o.material);
                PenaltyRenderItem {
                    key: PenaltySortKey {
                        layer: layer_for_role(o.role),
                        depth_bucket: depth_bucket(o.position, o.size),
                        ordinal: o.id.0,
                    },
                    label: o.label,
                    content: PenaltyRenderContent::World {
                        role: o.role,
                        shape: o.shape,
                        position: o.position,
                        rotation: o.rotation,
                        size: o.size,
                        material: o.material,
                        shaded_color: style_pass.shade(&mat, WORLD_UP),
                        lit: !mat.unlit,
                    },
                }
            })
            .collect();

        // HUD items: always in the Hud layer (drawn last), depth bucket 0, with
        // a fixed intra-HUD ordinal from HUD_ORDER + the instruction slot. The
        // HUD is always unlit.
        HUD_ORDER.iter().enumerate().for_each(|(i, &element)| {
            items.push(hud_item(element, i as u32));
        });
        items.push(hud_item(PenaltyHudElement::Instruction, HUD_ORDER.len() as u32));

        // A single total sort by the deterministic key. `sort_by` is stable,
        // but the key is already total, so stability is belt-and-suspenders.
        items.sort_by_key(|a| a.key);
        Self { items, camera, style_pass }
    }

    /// The ordered draw labels — a compact, testable fingerprint of the order.
    pub fn labels(&self) -> Vec<&'static str> {
        self.items.iter().map(|i| i.label).collect()
    }

    /// The ordered sort keys.
    pub fn keys(&self) -> Vec<PenaltySortKey> {
        self.items.iter().map(|i| i.key).collect()
    }

    /// The layer of each item, in draw order.
    pub fn layer_sequence(&self) -> Vec<PenaltyDrawLayer> {
        self.items.iter().map(|i| i.key.layer).collect()
    }

    /// The distinct layers in the order they first appear — the high-level
    /// bucket order the scene actually renders in.
    pub fn distinct_layers_in_order(&self) -> Vec<PenaltyDrawLayer> {
        let mut out: Vec<PenaltyDrawLayer> = Vec::new();
        self.items.iter().for_each(|i| {
            let last = out.last().copied();
            (last != Some(i.key.layer)).then(|| out.push(i.key.layer));
        });
        out
    }

    /// A deterministic, human-readable dump of the final sorted order — a debug
    /// / documentation view. Not printed by production code; callers (tests,
    /// tools) decide what to do with the lines.
    pub fn debug_lines(&self) -> Vec<String> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                format!(
                    "{i:>3}  {:<18} L{:<2} depth={:<3} ord={:<3} {}",
                    format!("{:?}", it.key.layer),
                    it.key.layer.order_index(),
                    it.key.depth_bucket,
                    it.key.ordinal,
                    it.label,
                )
            })
            .collect()
    }
}

/// The (panel, foreground) materials for a HUD element. Every HUD element sits
/// on the dark panel; the foreground picks a fixed highlight color. HUD is
/// always unlit.
fn hud_materials(element: PenaltyHudElement) -> (PenaltyMaterialId, PenaltyMaterialId) {
    let foreground = match element {
        PenaltyHudElement::Score => PenaltyMaterialId::HudGreenSuccess,
        PenaltyHudElement::Round => PenaltyMaterialId::HudWhiteText,
        PenaltyHudElement::Best => PenaltyMaterialId::HudYellowHighlight,
        PenaltyHudElement::PowerMeter => PenaltyMaterialId::HudRedWarning,
        PenaltyHudElement::Reticle => PenaltyMaterialId::HudGreenSuccess,
        PenaltyHudElement::Instruction => PenaltyMaterialId::HudWhiteText,
    };
    (PenaltyMaterialId::HudDarkPanel, foreground)
}

fn hud_item(element: PenaltyHudElement, ordinal: u32) -> PenaltyRenderItem {
    let (panel_material, foreground_material) = hud_materials(element);
    PenaltyRenderItem {
        key: PenaltySortKey { layer: PenaltyDrawLayer::Hud, depth_bucket: 0, ordinal },
        label: hud_label(element),
        content: PenaltyRenderContent::Hud {
            element,
            panel_material,
            foreground_material,
            lit: false,
        },
    }
}
