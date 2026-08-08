//! The overlay view model: everything the screen-space interface draws, as data.
//!
//! The painter that turns this into pixels lives at the platform edge and makes
//! no decisions. Every position, label and highlight is resolved here, natively,
//! where it can be tested — so "the aim reticle is inside the goal" and "the
//! action button says HEIGHT during the bend stage" are assertions rather than
//! things somebody looked at once.

use axiom::prelude::Vec2;

use crate::play::{Phase, Projection};

use super::layout::Rect;

/// The sculpt panel, resolved for drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelView {
    pub rect: Rect,
    pub projection: Projection,
    /// One word: `BEND` or `HEIGHT`.
    pub label: &'static str,
    /// The two ends of the shot, so the panel reads as a shot and not a graph.
    pub ball_end: Vec2,
    pub goal_end: Vec2,
    /// The straight reference the curve deforms away from.
    pub baseline: [Vec2; 2],
    /// The curve itself.
    pub curve: Vec<Vec2>,
    /// The feedback handle, lifted clear of the finger, while a drag is live.
    pub handle: Option<Vec2>,
    /// Whether the panel is being dragged right now.
    pub active: bool,
    /// The two directions, named at the panel's edges: `LEFT`/`RIGHT` or
    /// `LOW`/`HIGH`.
    pub hint_low: &'static str,
    pub hint_high: &'static str,
}

/// A button, resolved for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonView {
    pub rect: Rect,
    pub label: &'static str,
    pub pressed: bool,
}

/// The whole overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorView {
    pub phase: Phase,
    /// The goal's mouth on screen, bottom-left first. Absent when the goal is
    /// somehow off camera.
    pub goal_quad: Option<[Vec2; 4]>,
    /// The chosen finish, on screen.
    pub target: Option<Vec2>,
    /// How strongly the aim overlay is drawn, `0..1` — it fades out once the
    /// player has moved on from aiming so it never competes with the panel.
    pub aim_emphasis: f32,
    pub panel: Option<PanelView>,
    /// The one-word stage label.
    pub prompt: &'static str,
    pub action: Option<ButtonView>,
    pub back: Option<ButtonView>,
    /// The result banner.
    pub banner: Option<&'static str>,
    /// Goals and attempts.
    pub tally: (u32, u32),
    /// The viewport this was resolved for, physical pixels.
    pub viewport: Vec2,
    /// The short edge, for sizing text at the edge.
    pub short: f32,
}

impl EditorView {
    /// An empty overlay for a phase that has no interface (the flight, the
    /// reset), still carrying the tally and the banner.
    pub fn quiet(phase: Phase, viewport: Vec2, short: f32, tally: (u32, u32)) -> EditorView {
        EditorView {
            phase,
            goal_quad: None,
            target: None,
            aim_emphasis: 0.0,
            panel: None,
            prompt: "",
            action: None,
            back: None,
            banner: None,
            tally,
            viewport,
            short,
        }
    }
}

/// The two edge labels for a projection.
pub fn hints(projection: Projection) -> (&'static str, &'static str) {
    match projection {
        Projection::Horizontal => ("LEFT", "RIGHT"),
        Projection::Vertical => ("LOW", "HIGH"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_overlay_still_carries_the_score() {
        let view = EditorView::quiet(Phase::BallInFlight, Vec2::new(390.0, 844.0), 390.0, (3, 7));
        assert_eq!(view.tally, (3, 7));
        assert_eq!(view.panel, None);
        assert_eq!(view.action, None);
        assert_eq!(view.prompt, "");
        assert_eq!(view.viewport, Vec2::new(390.0, 844.0));
        assert_eq!(view.short, 390.0);
    }

    #[test]
    fn each_projection_names_its_own_two_directions() {
        assert_eq!(hints(Projection::Horizontal), ("LEFT", "RIGHT"));
        assert_eq!(hints(Projection::Vertical), ("LOW", "HIGH"));
    }
}
