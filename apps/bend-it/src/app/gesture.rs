//! One finger, two jobs.
//!
//! The whole game is a line under a thumb, and which line it is depends on which
//! end of the pitch the player is at. Taking a penalty it is the ball's path, and
//! it is read when you let go. Keeping one it is your own body, and *letting go
//! is the decision* — the same gesture, the same release, an entirely different
//! sentence.
//!
//! Kept in its own file because it is the only place in the app where a pixel
//! becomes an instruction, and that boundary is worth being able to read on its
//! own: everything above it is a hand, everything below it is a command, and the
//! two vocabularies never mix.

use axiom_input::Pointer;

use crate::play::{DiveCall, PlayCommand};
use crate::projection::ScreenProjection;
use crate::stroke::{interpret, Drawing};
use crate::tuning::DT;

use super::BendIt;

impl BendIt {
    /// Fold this tick's contact into the drawing, and — if it finished — read it.
    pub(super) fn draw(
        &mut self,
        pointer: Option<Pointer>,
        projection: &ScreenProjection,
    ) -> Vec<PlayCommand> {
        let tuning = *self.session.tuning();
        let short = self.surface.x.min(self.surface.y);
        // A phase change under the player's finger abandons the line rather than
        // letting it fire into the next attempt.
        (self.session.phase() != self.last_phase).then(|| self.capture.cancel());
        self.last_phase = self.session.phase();

        // One gesture, two jobs. Taking, the line is the ball's path and it is
        // read when you let go. Keeping, the line is your body and *letting go is
        // the decision* — so it is read the moment the finger lifts too, but what
        // it means is "dive now, there".
        let accepting = self.session.phase().accepts_drawing()
            | (self.session.keeping() & self.session.phase().accepts_dive());
        let sample = pointer.filter(|_| accepting);
        match self
            .capture
            .update(sample, self.frame_n, short * tuning.stroke.spacing, short)
        {
            Drawing::Idle => {
                self.preview = None;
                Vec::new()
            }
            // What the line would be struck at if it were let go now.
            //
            // Read with exactly the same call the finished line gets, not an
            // approximation of it — a preview that could disagree with the kick
            // is worse than no preview, because it teaches the player something
            // untrue about their own hand. Below the length that counts as a shot
            // there is nothing honest to say yet, so it says nothing.
            Drawing::Drawing => {
                self.preview = (self.capture.stroke().length()
                    >= short * tuning.stroke.min_length)
                    .then(|| {
                        interpret(
                            self.capture.stroke(),
                            projection,
                            self.session.shot().origin,
                            self.session.mouth(),
                            &tuning,
                        )
                    })
                    .flatten()
                    .map(|r| r.intent.launch_speed(&tuning));
                Vec::new()
            }
            Drawing::Finished(line) if self.session.keeping() => {
                self.ghost = Some((line.clone(), 1.0));
                self.preview = None;
                // Where the finger finished is where the hands go. Only the
                // finish, because a keeper's line is a gesture toward a corner
                // rather than a path a body follows — and asking for an accurate
                // arc in the third of a second somebody has would be asking for
                // precision nobody has when they need it.
                line.points()
                    .last()
                    .map(|finish| {
                        vec![PlayCommand::Dive(DiveCall::read(
                            *finish,
                            self.surface,
                            self.session.keeper().motion().hips,
                            &tuning.keeper,
                        ))]
                    })
                    .unwrap_or_default()
            }
            Drawing::Finished(line) => {
                // The line leaves the screen the moment it is let go, whether or
                // not it was long enough to mean anything.
                self.ghost = Some((line.clone(), 1.0));
                let reading = interpret(
                    &line,
                    projection,
                    self.session.shot().origin,
                    self.session.mouth(),
                    &tuning,
                );
                self.reading = reading.clone();
                reading
                    .map(|r| vec![PlayCommand::Kick(r.intent)])
                    .unwrap_or_default()
            }
        }
    }

    /// Advance the released line's flick-away.
    pub(super) fn fade(&mut self) {
        let rate = DT / self.session.tuning().stroke.fade.max(1.0e-3);
        self.ghost = self
            .ghost
            .take()
            .map(|(line, life)| (line, life - rate))
            .filter(|(_, life)| *life > 0.0);
    }

}
