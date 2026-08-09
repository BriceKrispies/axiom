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

/// How hard the shot is being hit, kilometres per hour.
///
/// One reading with two lives. While a line is under the finger it is what the
/// shot **would** leave at if it were let go now; from the moment of contact it
/// is what the ball actually left at. `struck` is which, and it is what lets the
/// screen show the first as a promise and the second as a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Speed {
    pub kmh: u32,
    pub struck: bool,
}

/// The shootout, as the screen needs it.
///
/// Five marks a side, filled in as they are taken, plus whose kick it is. It is
/// the one piece of state the player must be able to read without thinking,
/// because it is the reason the kick they are about to take matters — and a
/// scoreboard that has to be worked out is a scoreboard nobody looks at while
/// their thumb is on the glass.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Board {
    /// `Some(true)` scored, `Some(false)` missed, `None` still to come.
    pub yours: Vec<Option<bool>>,
    pub theirs: Vec<Option<bool>>,
    /// Whether the player is in the goal for this one.
    pub keeping: bool,
    /// Sudden death: every kick is the last one.
    pub sudden_death: bool,
    /// How it finished, once it has.
    pub outcome: Option<&'static str>,
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
    /// The shootout: your marks, theirs, and whether you are the one in the goal.
    pub board: Board,
    /// How hard the shot is being hit — previewed while the line is drawn, then
    /// held from contact until the next attempt is set up.
    pub speed: Option<Speed>,
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
            board: Board::default(),
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
pub fn hint_for(phase: Phase, attempts: u32, keeping: bool) -> Option<&'static str> {
    // Keeping, the instruction never goes away, because the instruction is not
    // "here is how to play" — it is "the decision is yours and it is happening
    // now". A keeper with no prompt is a player watching a cutscene.
    let taking = matches!(phase, Phase::Aiming | Phase::Ready) & (attempts < 2) & !keeping;
    let saving = keeping & phase.accepts_dive();
    [None, Some("DRAW THE SHOT")][usize::from(taking)]
        .or([None, Some("DIVE")][usize::from(saving)])
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
    fn a_preview_and_a_result_are_the_same_number_wearing_different_hats() {
        let previewed = Speed { kmh: 137, struck: false };
        let struck = Speed { kmh: 137, struck: true };
        assert_ne!(previewed, struck, "the screen has to be able to tell them apart");
        assert_eq!(previewed.kmh, struck.kmh);
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
        assert_eq!(hint_for(Phase::Aiming, 0, false), Some("DRAW THE SHOT"));
        assert_eq!(hint_for(Phase::Aiming, 1, false), Some("DRAW THE SHOT"));
        assert_eq!(hint_for(Phase::Aiming, 2, false), None, "two shots is the tutorial");
        assert_eq!(hint_for(Phase::BallInFlight, 0, false), None);
        assert_eq!(hint_for(Phase::Resolution, 0, false), None);
        assert_eq!(hint_for(Phase::Ready, 0, false), Some("DRAW THE SHOT"));
        // Keeping, the prompt is there every single time and never times out.
        assert_eq!(hint_for(Phase::Kicking, 40, true), Some("DIVE"));
        assert_eq!(hint_for(Phase::BallInFlight, 40, true), Some("DIVE"));
        assert_eq!(hint_for(Phase::Aiming, 0, true), None, "nothing to do yet");
        assert_eq!(hint_for(Phase::Resolution, 0, true), None);
    }
}
