//! Golden / determinism proofs for `axiom-draw2d`, driven solely through the
//! public [`Draw2dApi`] facade and the value vocabulary that crosses it.
//!
//! Three SPEC-04 §7 proofs:
//!  1. **Layer-sort** — draws submitted out of layer order (with ties) come out
//!     of `finish()` stably sorted by `(layer, submit-order)`.
//!  2. **Determinism-as-function** — the same facade-call sequence yields a
//!     byte-identical `Draw2dList` on a second run (whole-value equality, a
//!     stronger statement than a hash match).
//!  3. **Presentation-exclusion** — see `architecture.rs`
//!     `facade_has_no_sim_readable_draw_state_getter` (structural, no read-back).

use axiom_draw2d::{
    Common2d, Draw2dApi, Draw2dCommand, Fill2d, FontHandle, Glyph2d, GlyphRun, GradientStop, Rect,
    Rgba, Shadow2d, SpriteDraw2d, TextAlign, TextDraw2d, TextureId,
};
use axiom_kernel::{Meters, Radians, Ratio};
use axiom_math::{Mat3, Vec2};

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn meters(v: f32) -> Meters {
    Meters::new(v).unwrap()
}

fn radians(v: f32) -> Radians {
    Radians::new(v).unwrap()
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
    Rgba::new(ratio(r), ratio(g), ratio(b), ratio(a))
}

fn common(layer: i32) -> Common2d {
    Common2d::new(layer, ratio(1.0))
}

fn at(min: f32) -> Rect {
    Rect::new(Vec2::new(min, 0.0), Vec2::ONE)
}

// ---------- 1. layer-sort golden ----------

#[test]
fn finish_stably_sorts_by_layer_then_submit_order() {
    let mut api = Draw2dApi::new();
    // Submit rects out of layer order, with ties on layers 0, 1, 2. The rect's
    // min.x doubles as the submission marker (i-th submitted draw has min.x = i).
    let layers = [2, 0, 1, 0, 2, 1];
    for (i, layer) in layers.iter().enumerate() {
        api.rect(at(i as f32), Fill2d::color(rgba(1.0, 1.0, 1.0, 1.0)), common(*layer));
    }
    let list = api.finish();

    // Collect (layer, submission, min.x) in final order.
    let observed: Vec<(i32, u32, f32)> = list
        .commands()
        .iter()
        .map(|c| {
            let min_x = c.as_rect().expect("all draws are rects").min.x;
            (c.layer(), c.submission_index(), min_x)
        })
        .collect();

    // Expected: sorted by layer, ties by ascending submission (= ascending
    // min.x). Layer 0: submits 1,3; layer 1: submits 2,5; layer 2: submits 0,4.
    let expected = vec![
        (0, 1, 1.0),
        (0, 3, 3.0),
        (1, 2, 2.0),
        (1, 5, 5.0),
        (2, 0, 0.0),
        (2, 4, 4.0),
    ];
    assert_eq!(observed, expected);

    // The sort is a permutation: layers non-decreasing, and within each layer
    // the submission order is strictly the original call order.
    assert!(observed.windows(2).all(|w| w[0].0 <= w[1].0));
    assert!(observed
        .windows(2)
        .all(|w| w[0].0 != w[1].0 || w[0].1 < w[1].1));
}

// ---------- 2. determinism-as-function ----------

/// A rich, deterministic frame exercising every landed surface: camera, the
/// transform stack (push/pop), every shape kind, sprite, text, and both
/// gradient kinds referenced by a fill.
fn render_a_frame() -> axiom_draw2d::Draw2dList {
    let mut api = Draw2dApi::new();
    api.set_camera2d(Vec2::new(3.0, 4.0), ratio(1.5));

    let lin = api.linear_gradient(
        Vec2::ZERO,
        Vec2::new(1.0, 0.0),
        &[
            GradientStop::new(ratio(0.0), rgba(1.0, 0.0, 0.0, 1.0)),
            GradientStop::new(ratio(1.0), rgba(0.0, 0.0, 1.0, 1.0)),
        ],
    );
    let _rad = api.radial_gradient(
        Vec2::new(2.0, 2.0),
        meters(5.0),
        &[GradientStop::new(ratio(0.5), rgba(0.0, 1.0, 0.0, 0.5))],
    );

    let depth = api.push_transform(Mat3::translation(Vec2::new(10.0, 20.0)));
    api.push_transform(Mat3::rotation(radians(0.25)));
    api.rect(at(0.0), Fill2d::paint(lin), common(3));
    api.circle(Vec2::new(1.0, 1.0), meters(2.0), Fill2d::color(rgba(1.0, 1.0, 0.0, 1.0)), common(1));
    api.pop_transform(depth);

    api.ellipse(
        Vec2::ZERO,
        meters(4.0),
        meters(2.0),
        radians(0.1),
        Fill2d::color(rgba(0.2, 0.3, 0.4, 1.0)),
        Common2d::with_shadow(2, ratio(0.8), Shadow2d::new(rgba(0.0, 0.0, 0.0, 0.5), meters(3.0))),
    );
    api.line(Vec2::ZERO, Vec2::new(9.0, 9.0), rgba(1.0, 1.0, 1.0, 1.0), meters(1.0), common(0));
    api.path(
        &[Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)],
        Fill2d::color(rgba(0.5, 0.5, 0.5, 1.0)),
        common(2),
        true,
    );
    api.sprite(
        TextureId::from_raw(7),
        SpriteDraw2d::new(at(2.0), Vec2::new(0.5, 0.5), rgba(1.0, 1.0, 1.0, 1.0), true, false),
        common(4),
    );
    api.text(
        GlyphRun::new(
            vec![
                Glyph2d::new(at(0.0), meters(6.0)),
                Glyph2d::new(at(6.0), meters(6.0)),
            ],
            meters(12.0),
        ),
        TextDraw2d::new(FontHandle::from_raw(1), rgba(1.0, 1.0, 1.0, 1.0), TextAlign::CENTER),
        common(5),
    );
    api.finish()
}

#[test]
fn identical_call_sequences_yield_byte_identical_lists() {
    let a = render_a_frame();
    let b = render_a_frame();
    assert_eq!(
        a, b,
        "the same facade-call sequence must produce an identical Draw2dList \
         (whole-value equality is a byte-identical, hash-stable result)"
    );
}

#[test]
fn rendered_frame_is_well_formed() {
    // Guard that the determinism frame actually exercised the surface (so the
    // equality above is not vacuously over an empty list).
    let list = render_a_frame();
    assert_eq!(list.len(), 7);
    assert_eq!(list.paint_count(), 2);
    assert_eq!(list.camera().map(|c| c.center), Some(Vec2::new(3.0, 4.0)));
    // Every KIND landed exactly once.
    let mut kinds: Vec<u32> = list.commands().iter().map(Draw2dCommand::kind_code).collect();
    kinds.sort_unstable();
    assert_eq!(
        kinds,
        vec![
            Draw2dCommand::KIND_RECT,
            Draw2dCommand::KIND_CIRCLE,
            Draw2dCommand::KIND_ELLIPSE,
            Draw2dCommand::KIND_LINE,
            Draw2dCommand::KIND_PATH,
            Draw2dCommand::KIND_SPRITE,
            Draw2dCommand::KIND_TEXT_GLYPHS,
        ]
    );
}
