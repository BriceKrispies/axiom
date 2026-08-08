//! The overlay view model: the line under the finger, and four short words.
//!
//! There is almost nothing here now, and that is the design. The old interface
//! was three stages, two panels and four buttons; this one is the line you are
//! drawing, and it disappears the moment you let go.

use axiom::prelude::Vec2;

use crate::play::Phase;

/// The line as it is being drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeView {
    /// The drawn points, physical pixels.
    pub points: Vec<Vec2>,
    /// `1` while the finger is down, falling to `0` as the line flicks away
    /// after release.
    pub fade: f32,
    /// Whether the line is currently long enough to be a shot. Below this the
    /// line is drawn faintly, which is the only feedback needed for "keep going".
    pub live: bool,
}

/// Everything the screen draws.
#[derive(Debug, Clone, PartialEq)]
pub struct GameView {
    pub phase: Phase,
    pub stroke: Option<StrokeView>,
    /// The one-line instruction, shown only until the player has taken a shot.
    pub hint: Option<&'static str>,
    /// The result banner.
    pub banner: Option<&'static str>,
    /// Goals and attempts.
    pub tally: (u32, u32),
    pub viewport: Vec2,
    pub short: f32,
}

impl GameView {
    pub fn empty(phase: Phase, viewport: Vec2, tally: (u32, u32)) -> GameView {
        GameView {
            phase,
            stroke: None,
            hint: None,
            banner: None,
            tally,
            viewport,
            short: viewport.x.min(viewport.y),
        }
    }
}

/// The instruction, which stops being shown once the player has clearly got it.
///
/// Two attempts is the whole tutorial. The gesture teaches the mechanic; a line
/// of text that never goes away is just clutter that outlived its usefulness.
pub fn hint_for(phase: Phase, attempts: u32) -> Option<&'static str> {
    (matches!(phase, Phase::Aiming | Phase::Ready) & (attempts < 2))
        .then_some("DRAW THE SHOT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_view_still_carries_the_score() {
        let view = GameView::empty(Phase::BallInFlight, Vec2::new(390.0, 844.0), (2, 5));
        assert_eq!(view.tally, (2, 5));
        assert_eq!(view.stroke, None);
        assert_eq!(view.banner, None);
        assert_eq!(view.short, 390.0);
    }

    #[test]
    fn the_instruction_appears_early_and_then_gets_out_of_the_way() {
        assert_eq!(hint_for(Phase::Aiming, 0), Some("DRAW THE SHOT"));
        assert_eq!(hint_for(Phase::Aiming, 1), Some("DRAW THE SHOT"));
        assert_eq!(hint_for(Phase::Aiming, 2), None, "two shots is the tutorial");
        assert_eq!(hint_for(Phase::BallInFlight, 0), None);
        assert_eq!(hint_for(Phase::Resolution, 0), None);
        assert_eq!(hint_for(Phase::Ready, 0), Some("DRAW THE SHOT"));
    }
}
