//! Pass 3 proofs: deterministic lighting, materials, blob shadows, and retro 32-bit
//! style.
//!
//! Everything is a pure function of fixed constants; determinism is proven by
//! equality across independent rebuilds. No randomness, no wall-clock time, and
//! all construction is over explicit ordered arrays/vectors — never a map.

use axiom_gallery::soccer_penalty::penalty_blob_shadow::BLOB_SHADOWS;
use axiom_gallery::soccer_penalty::penalty_light::PenaltyLightModel;
use axiom_gallery::soccer_penalty::penalty_materials::{material, palette, PenaltyMaterialId, PENALTY_PALETTE};
use axiom_gallery::soccer_penalty::penalty_render_plan::{
    PenaltyDrawLayer, PenaltyRenderContent, PenaltyRenderPlan,
};
use axiom_gallery::soccer_penalty::penalty_style::{PenaltyVisualStyle, TextureFilter};
use axiom_gallery::soccer_penalty::SoccerPenaltyApp;
use axiom_math::Vec3;

fn plan() -> PenaltyRenderPlan {
    SoccerPenaltyApp::build_stage1().render_plan
}

// --- lighting ---------------------------------------------------------------

#[test]
fn light_model_constants_are_deterministic() {
    assert_eq!(PenaltyLightModel::stage1(), PenaltyLightModel::stage1());
    let l = PenaltyLightModel::stage1();
    assert_eq!(l.ambient_strength, 0.55);
    assert_eq!(l.directional_strength, 0.65);
    assert_eq!(l.bands, [0.55, 0.68, 0.80, 0.92]);
    // The stored direction is unit-length (normalized form of (-0.45,-1,-0.35)).
    let len = l.direction.length();
    assert!((len - 1.0).abs() < 1.0e-3, "light direction must be normalized (len {len})");
}

#[test]
fn brightness_quantization_maps_to_expected_bands() {
    let l = PenaltyLightModel::stage1();
    // Snap-down to the largest band met, floored at 0.55.
    assert_eq!(l.quantize(0.55), 0.55);
    assert_eq!(l.quantize(0.67), 0.55);
    assert_eq!(l.quantize(0.68), 0.68);
    assert_eq!(l.quantize(0.79), 0.68);
    assert_eq!(l.quantize(0.80), 0.80);
    assert_eq!(l.quantize(0.91), 0.80);
    assert_eq!(l.quantize(0.92), 0.92);
    assert_eq!(l.quantize(1.00), 0.92);
    // Below the first band still floors to 0.55.
    assert_eq!(l.quantize(0.0), 0.55);
}

#[test]
fn face_brightness_follows_the_light_model() {
    let l = PenaltyLightModel::stage1();
    // A face pointing straight at the light is fully lit: ambient + directional.
    let toward_light = l.direction.mul_scalar(-1.0);
    assert!((l.face_brightness(toward_light) - 1.20).abs() < 1.0e-3);
    // A face pointing away receives only ambient.
    assert!((l.face_brightness(l.direction) - 0.55).abs() < 1.0e-3);
    // The up face is bright (upper-front-left light) → quantizes to the top band.
    let up = Vec3::new(0.0, 1.0, 0.0);
    assert_eq!(l.quantize(l.face_brightness(up)), 0.92);
}

// --- materials --------------------------------------------------------------

#[test]
fn material_palette_contains_all_required_named_materials() {
    let names: Vec<&str> = palette().iter().map(|m| m.name).collect();
    for required in [
        "field grass",
        "darker grass band",
        "white field lines",
        "goal frame white",
        "net off-white",
        "goalie jersey yellow",
        "goalie shorts black",
        "goalie skin",
        "goalie hair",
        "kicker jersey blue",
        "kicker shorts white",
        "kicker socks dark",
        "ball white",
        "ball dark panels",
        "crowd muted colors",
        "stadium wall dark gray",
        "ad board red",
        "HUD dark panel",
        "HUD white text",
        "HUD yellow highlight",
        "HUD green success",
        "HUD red warning",
    ] {
        assert!(names.contains(&required), "material palette is missing '{required}'");
    }
}

#[test]
fn material_palette_ordering_is_stable_and_indexable() {
    // Two reads are identical (ordered array, not a map).
    let a: Vec<&str> = palette().iter().map(|m| m.name).collect();
    let b: Vec<&str> = palette().iter().map(|m| m.name).collect();
    assert_eq!(a, b);

    // Each entry's id discriminant equals its palette index, so `material` is a
    // direct index and the order is fixed.
    PENALTY_PALETTE.iter().enumerate().for_each(|(i, m)| {
        assert_eq!(m.id as usize, i, "palette entry {i} ({}) is out of order", m.name);
        assert_eq!(material(m.id), *m);
    });

    // HUD materials are unlit; a representative world material is lit.
    assert!(material(PenaltyMaterialId::HudWhiteText).unlit);
    assert!(material(PenaltyMaterialId::BlobShadow).unlit);
    assert!(!material(PenaltyMaterialId::GoalieJerseyYellow).unlit);
}

// --- blob shadows -----------------------------------------------------------

#[test]
fn blob_shadow_descriptors_exist_for_each_actor() {
    let labels: Vec<&str> = BLOB_SHADOWS.iter().map(|s| s.label).collect();
    assert_eq!(labels, ["shadow.kicker", "shadow.ball", "shadow.goalie"]);
    // The kicker shadow is elongated along the field; the ball shadow is small.
    let kicker = BLOB_SHADOWS[0];
    assert!(kicker.radius_z > kicker.radius_x, "kicker shadow must be elongated");
    let ball = BLOB_SHADOWS[1];
    assert!(ball.radius_x < 0.5 && ball.radius_z < 0.5, "ball shadow must be small");
}

#[test]
fn blob_shadows_render_in_the_actor_shadow_layer() {
    let p = plan();
    let shadows: Vec<_> = p
        .items
        .iter()
        .filter(|it| it.label.starts_with("shadow."))
        .collect();
    assert_eq!(shadows.len(), 3, "kicker, ball, goalie shadows");
    shadows.iter().for_each(|it| {
        assert_eq!(it.layer(), PenaltyDrawLayer::ActorShadow);
        assert!(!it.is_lit(), "blob shadows are unlit");
        match it.content {
            PenaltyRenderContent::World { material: m, .. } => {
                assert_eq!(m, PenaltyMaterialId::BlobShadow)
            }
            PenaltyRenderContent::Hud { .. } => panic!("shadow must be a world item"),
        }
    });
}

#[test]
fn blob_shadows_sort_after_field_and_before_actors() {
    let p = plan();
    let idx_of = |pred: &dyn Fn(&str) -> bool| {
        p.items
            .iter()
            .enumerate()
            .filter(|(_, it)| pred(it.label))
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let shadows = idx_of(&|l| l.starts_with("shadow."));
    let field = idx_of(&|l| l.starts_with("field.") || l.starts_with("line.") || l == "spot.penalty");
    let actors = idx_of(&|l| l.starts_with("goalie.") || l == "ball" || l.starts_with("kicker."));

    let shadow_lo = *shadows.iter().min().unwrap();
    let shadow_hi = *shadows.iter().max().unwrap();
    assert!(*field.iter().max().unwrap() < shadow_lo, "shadows sort after field/lines");
    assert!(shadow_hi < *actors.iter().min().unwrap(), "shadows sort before actors");
}

// --- HUD unlit --------------------------------------------------------------

#[test]
fn hud_items_are_unlit_and_world_items_are_lit_except_shadows() {
    let p = plan();
    p.items.iter().for_each(|it| match it.content {
        PenaltyRenderContent::Hud { lit, .. } => assert!(!lit, "HUD must be unlit"),
        PenaltyRenderContent::World { lit, material: m, .. } => {
            let unlit_material = m == PenaltyMaterialId::BlobShadow;
            assert_eq!(lit, !unlit_material, "world lit flag must follow the material");
        }
    });
}

// --- style ------------------------------------------------------------------

#[test]
fn retro_32bit_style_descriptor_has_expected_values() {
    let s = PenaltyVisualStyle::stage1();
    assert_eq!(s.internal_width, 426);
    assert_eq!(s.internal_height, 240);
    assert!(s.pixel_snapping);
    assert!(s.flat_shading);
    assert!(s.brightness_quantization);
    assert_eq!(s.texture_filter, TextureFilter::Nearest);
    assert!(!s.physically_based, "no PBR");
    assert!(!s.dynamic_shadows, "no dynamic shadows");
}

// --- determinism ------------------------------------------------------------

#[test]
fn pass3_render_and_style_descriptors_rebuild_identically() {
    let a = SoccerPenaltyApp::build_stage1();
    let b = SoccerPenaltyApp::build_stage1();
    // Whole plan incl. shaded colors, materials, lit flags, and the style pass.
    assert_eq!(a.render_plan, b.render_plan);
    assert_eq!(a.render_plan.style_pass, b.render_plan.style_pass);
}

#[test]
fn representative_shaded_colors_use_the_top_band() {
    // World lit items are shaded by the top face (up normal → 0.92 band), so a
    // fully-white material renders at 0.92 brightness. This confirms shading is
    // actually applied to render items.
    let p = plan();
    let lines = p
        .items
        .iter()
        .find(|it| it.label == "line.goal")
        .expect("goal line exists");
    match lines.content {
        PenaltyRenderContent::World { shaded_color, material: m, .. } => {
            let base = material(m).base_color;
            assert!(shaded_color.r < base.r, "lit material must be darkened by quantized brightness");
            assert!((shaded_color.r - base.r * 0.92).abs() < 1.0e-4);
        }
        PenaltyRenderContent::Hud { .. } => panic!("line.goal must be a world item"),
    }
}
