//! What the screen should show, assembled once a tick.
//!
//! Split from the orchestration next door because it is the one place the game's
//! *state* becomes the game's *picture*, and it is worth being able to read that
//! translation on its own. Nothing here decides anything: it reads the session,
//! the capture and the preview, and produces a [`GameView`] — the model the
//! painter in `web/overlay.rs` turns into SVG and the tests assert against
//! without a browser anywhere near them.

use crate::stroke::{hint_for, speed_readout, GameView, Speed, StrokeView};

use super::BendIt;

impl BendIt {
    /// What the screen should show.
    pub(super) fn compose(&self) -> GameView {
        let tuning = self.session.tuning();
        let short = self.surface.x.min(self.surface.y);
        let mut view = GameView::empty(
            self.session.phase(),
            self.surface,
            (self.session.tally().goals, self.session.tally().attempts),
        );
        view.banner = self.session.result().map(|r| r.banner());
        view.hint = hint_for(self.session.phase(), self.session.tally().attempts);
        // The struck speed wins the moment there is one: a fact replaces a
        // promise, in the same place on the screen, so the player can see whether
        // the kicker delivered what their line asked for.
        view.speed = self
            .session
            .struck_speed()
            .map(|v| Speed { kmh: speed_readout(v), struck: true })
            .or_else(|| {
                self.preview.map(|v| Speed { kmh: speed_readout(v), struck: false })
            });
        // The line under the finger, or the one that just left it.
        let live = self.capture.drawing().then(|| StrokeView {
            points: self.capture.stroke().points().to_vec(),
            fade: 1.0,
            live: self.capture.stroke().length() >= short * tuning.stroke.min_length,
        });
        view.stroke = live.or_else(|| {
            self.ghost.as_ref().map(|(line, life)| StrokeView {
                points: line.points().to_vec(),
                fade: *life,
                live: true,
            })
        });
        view
    }
}
