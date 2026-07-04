//! Pass 2 proofs: deterministic depth ordering and net layering.
//!
//! These pin the render-ordering model: the fixed layer order, the three-part
//! sort key, stable rebuilds, the rear/front net split, HUD-last, and the
//! high-level bucket order. Determinism is proven by equality across rebuilds —
//! no randomness or wall-clock time can enter, and the plan is built from
//! explicit ordered vectors, never hash-map iteration.

use axiom_gallery::soccer_penalty::penalty_render_plan::{
    depth_bucket, PenaltyDrawLayer, PenaltyRenderContent, PenaltyRenderPlan, PenaltySortKey,
};
use axiom_gallery::soccer_penalty::SoccerPenaltyApp;
use axiom_math::Vec3;

/// The exact number of render items = world objects (173) + HUD items
/// (score, round, best, power, reticle, instruction = 6). The world count grew
/// with the visual-convergence pass: the crowd is now 3 stacked rows of 26 cards
/// (78) instead of a single row of 9, the kicker is an 11-part posed figure, and
/// the ball carries 6 dark panels instead of 2.
const EXPECTED_RENDER_ITEMS: usize = 187;

fn plan() -> PenaltyRenderPlan {
    SoccerPenaltyApp::build_stage1().render_plan
}

/// First/last item index whose label starts with `prefix`.
fn index_span(plan: &PenaltyRenderPlan, prefix: &str) -> Option<(usize, usize)> {
    let hits: Vec<usize> = plan
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.label.starts_with(prefix))
        .map(|(i, _)| i)
        .collect();
    hits.first().copied().zip(hits.last().copied())
}

/// Highest index among any actor item (goalie / ball / kicker).
fn actor_index_range(plan: &PenaltyRenderPlan) -> (usize, usize) {
    let actor = |label: &str| {
        label.starts_with("goalie.") || label == "ball" || label.starts_with("kicker.")
    };
    let hits: Vec<usize> = plan
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| actor(it.label))
        .map(|(i, _)| i)
        .collect();
    (*hits.first().expect("actors exist"), *hits.last().expect("actors exist"))
}

#[test]
fn penalty_draw_layer_order_is_stable() {
    // ALL is the canonical back-to-front order, and `order_index` agrees with it.
    PenaltyDrawLayer::ALL.iter().enumerate().for_each(|(i, layer)| {
        assert_eq!(layer.order_index() as usize, i, "{layer:?} has an unexpected index");
    });
    // The enum's own `Ord` matches the ALL order (strictly increasing).
    PenaltyDrawLayer::ALL.windows(2).for_each(|w| {
        assert!(w[0] < w[1], "{:?} must sort before {:?}", w[0], w[1]);
    });
    // The exact expected sequence — a change here is a deliberate re-ordering.
    assert_eq!(
        PenaltyDrawLayer::ALL,
        [
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
        ]
    );
}

#[test]
fn sort_key_orders_by_layer_then_depth_then_ordinal() {
    let key = |layer, depth_bucket, ordinal| PenaltySortKey { layer, depth_bucket, ordinal };

    // Layer dominates depth and ordinal.
    assert!(
        key(PenaltyDrawLayer::Goalie, 9, 999) < key(PenaltyDrawLayer::Kicker, 0, 0),
        "a lower layer must sort first regardless of depth/ordinal",
    );
    // Within a layer, coarse depth bucket dominates the ordinal.
    assert!(
        key(PenaltyDrawLayer::Goalie, 2, 999) < key(PenaltyDrawLayer::Goalie, 3, 0),
        "a farther depth bucket must sort first within a layer",
    );
    // Within equal layer+depth, the stable ordinal breaks the tie.
    assert!(
        key(PenaltyDrawLayer::Goalie, 2, 1) < key(PenaltyDrawLayer::Goalie, 2, 2),
        "the stable object ordinal must break layer/depth ties",
    );
}

#[test]
fn depth_bucket_is_deterministic_and_far_first() {
    // Same inputs → same bucket.
    let a = depth_bucket(Vec3::new(0.0, 0.0, 4.0), Vec3::new(1.0, 1.0, 1.0));
    let b = depth_bucket(Vec3::new(0.0, 0.0, 4.0), Vec3::new(1.0, 1.0, 1.0));
    assert_eq!(a, b);
    // Farther (smaller z) yields a smaller bucket → drawn first.
    let far = depth_bucket(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO);
    let near = depth_bucket(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO);
    assert!(far < near, "farther objects must land in an earlier bucket");
}

#[test]
fn render_plan_rebuilds_with_identical_order() {
    let a = plan();
    let b = plan();
    assert_eq!(a.keys(), b.keys(), "sort keys must be identical across rebuilds");
    assert_eq!(a.labels(), b.labels(), "draw order labels must be identical across rebuilds");
    assert_eq!(a.debug_lines(), b.debug_lines(), "the debug view must be reproducible");
}

#[test]
fn render_items_are_a_total_sorted_order() {
    let p = plan();
    assert_eq!(p.items.len(), EXPECTED_RENDER_ITEMS);

    // Keys are sorted ascending: a total, reproducible order.
    let keys = p.keys();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "render items must be in ascending sort-key order");

    // World items correspond one-to-one with the diorama objects; the rest are
    // the appended HUD items.
    let world = p.items.iter().filter(|it| matches!(it.content, PenaltyRenderContent::World { .. })).count();
    let hud = p.items.iter().filter(|it| matches!(it.content, PenaltyRenderContent::Hud { .. })).count();
    assert_eq!(world, 181);
    assert_eq!(hud, 6);
}

#[test]
fn net_pocket_renders_behind_the_actors() {
    let p = plan();
    // The net is now a real rear pocket (back/top/side planes, all RearNet); the
    // keeper stands in front of it, so the whole net renders before every actor.
    let (net_lo, net_hi) = index_span(&p, "net.").expect("net exists");
    let (actor_lo, actor_hi) = actor_index_range(&p);
    assert!(net_hi < actor_lo, "the net pocket must render before the goalie/ball/kicker");
    let _ = (net_lo, actor_hi);
}

#[test]
fn hud_renders_after_all_world_items() {
    let p = plan();
    let last_world = p
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| matches!(it.content, PenaltyRenderContent::World { .. }))
        .map(|(i, _)| i)
        .max()
        .expect("world items exist");
    let first_hud = p
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| matches!(it.content, PenaltyRenderContent::Hud { .. }))
        .map(|(i, _)| i)
        .min()
        .expect("hud items exist");
    assert!(first_hud > last_world, "every HUD item must render after every world item");
    // Every HUD item is in the Hud layer.
    p.items
        .iter()
        .filter(|it| matches!(it.content, PenaltyRenderContent::Hud { .. }))
        .for_each(|it| assert_eq!(it.layer(), PenaltyDrawLayer::Hud));
}

#[test]
fn high_level_bucket_order_is_fixed() {
    let p = plan();
    // background/crowd/stadium → field → rear net → goal/shadows/actors/ball/
    // kicker → HUD. The net is now a single rear pocket (no FrontNet items), and
    // Background/ForegroundEffects are reserved and empty in Pass 2, so none of
    // those layers appear.
    assert_eq!(
        p.distinct_layers_in_order(),
        vec![
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
            PenaltyDrawLayer::Hud,
        ]
    );
}

#[test]
fn field_plane_sorts_behind_its_grass_bands() {
    // The base plane must draw before every grass band despite the bands being
    // nearer the camera — the depth-bucket-from-farthest-edge rule.
    let p = plan();
    let plane = p.items.iter().position(|it| it.label == "field.plane").expect("plane exists");
    let first_band = p
        .items
        .iter()
        .position(|it| it.label == "field.band")
        .expect("bands exist");
    assert!(plane < first_band, "the field plane must sort behind the grass bands");
}

#[test]
fn ball_sorts_between_goalie_and_kicker() {
    // Composition sanity: goalie (far) → ball → kicker (near) in draw order.
    let p = plan();
    let goalie = p.items.iter().position(|it| it.label == "goalie.torso").expect("goalie");
    let ball = p.items.iter().position(|it| it.label == "ball").expect("ball");
    let kicker = p.items.iter().position(|it| it.label == "kicker.torso").expect("kicker");
    assert!(goalie < ball && ball < kicker, "draw order must be goalie → ball → kicker");
}
