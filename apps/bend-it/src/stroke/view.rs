//! The overlay view model: the line under the finger, a few short words, and one
//! number.
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
    /// How hard the last shot was struck, km/h — shown from the moment of
    /// contact and held until the next attempt is set up.
    pub speed: Option<u32>,
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
            speed: None,
            tally,
            viewport,
            short: viewport.x.min(viewport.y),
        }
    }
}

/// The ball's speed as the readout says it, kilometres per hour.
///
/// The engine works in metres and seconds, and metres per second is the wrong
/// unit to show a person a football in: 38 is a number without a feel, and 137 is
/// a number everybody has seen on a television. The conversion lives here, with
/// the rest of the view model, because choosing what a number *means* to a reader
/// is not something the painter should be deciding.
pub fn speed_readout(metres_per_second: f32) -> u32 {
    (metres_per_second.max(0.0) * 3.6).round() as u32
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
    fn the_speed_readout_is_the_number_a_person_recognises() {
        // 27.8 m/s is 100 km/h, and it is the slowest a penalty leaves here — so
        // the readout should never open with a two-digit number.
        assert_eq!(speed_readout(27.8), 100);
        assert_eq!(speed_readout(44.4), 160);
        assert_eq!(speed_readout(0.0), 0);
        // It rounds rather than truncating, and a nonsense input does not produce
        // a nonsense readout.
        assert_eq!(speed_readout(10.0), 36);
        assert_eq!(speed_readout(-5.0), 0);
    }

    #[test]
    fn an_empty_view_still_carries_the_score() {
        let view = GameView::empty(Phase::BallInFlight, Vec2::new(390.0, 844.0), (2, 5));
        assert_eq!(view.tally, (2, 5));
        assert_eq!(view.stroke, None);
        assert_eq!(view.banner, None);
        assert_eq!(view.short, 390.0);
        assert_eq!(view.speed, None, "nothing has been struck yet");
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
