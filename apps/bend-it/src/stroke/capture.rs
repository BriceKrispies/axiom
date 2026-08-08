//! Pointer samples in, a finished drawing out.
//!
//! One code path covers a finger, a pen and a mouse, because the platform edge
//! reduces all three to the same neutral `(position, is_down)`. There is no touch
//! build and no mouse build of the game.
//!
//! What it adds over a raw contact is the two things a drawing needs and a
//! contact does not have: **a line that accumulates** while the pointer is down,
//! and **a clean end** — including the end that happens because the browser took
//! the pointer away mid-gesture, which is otherwise how a line gets welded to a
//! finger that is no longer on the glass.

use axiom::prelude::Vec2;
use axiom_input::Pointer;

use super::line::Stroke;

/// How far a contact may jump in one tick before it is treated as a different
/// contact entirely, as a fraction of the viewport's short edge. Browsers do drop
/// a `pointerup`, and without this the next press continues the last line instead
/// of starting a new one.
///
/// It has to sit **above** the fastest hand and **below** a fresh press across
/// the screen, and that gap is narrower than it looks now that the tempo of the
/// drawing is what decides how hard the ball is hit. At 60 Hz, `0.55` of a
/// 390-pixel screen is 214 px a tick — about 12,800 px/s, faster than any hand
/// — so the guard can never split a genuine flick into two lines and quietly
/// throw away the half that was drawn hardest.
const JUMP: f32 = 0.55;

/// What the gesture did this tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Drawing {
    /// Nothing is being drawn.
    Idle,
    /// A line is in progress.
    Drawing,
    /// The finger came up, and this is what it left behind.
    Finished(Stroke),
}

/// Accumulates one drawing at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeCapture {
    stroke: Stroke,
    down: bool,
    last: Vec2,
}

impl Default for StrokeCapture {
    fn default() -> Self {
        StrokeCapture::new()
    }
}

impl StrokeCapture {
    pub fn new() -> StrokeCapture {
        StrokeCapture {
            stroke: Stroke::new(),
            down: false,
            last: Vec2::ZERO,
        }
    }

    /// The line as it stands (empty when nothing is being drawn).
    pub fn stroke(&self) -> &Stroke {
        &self.stroke
    }

    /// Whether a finger is currently down.
    pub fn drawing(&self) -> bool {
        self.down
    }

    /// Fold this tick's contact into the drawing.
    ///
    /// `spacing` decimates the incoming samples and `short_edge` scales the
    /// teleport guard, so both are in the caller's pixels rather than baked in.
    pub fn update(
        &mut self,
        pointer: Option<Pointer>,
        tick: u64,
        spacing: f32,
        short_edge: f32,
    ) -> Drawing {
        let contact = pointer.filter(|p| p.down);
        let teleported = contact
            .filter(|_| self.down)
            .map(|p| p.pos.subtract(self.last).length() > short_edge * JUMP)
            .unwrap_or(false);

        match (contact, self.down, teleported) {
            // A contact that jumped across the screen is a different contact: end
            // the old line and start a fresh one from here.
            (Some(p), true, true) => {
                let finished = core::mem::take(&mut self.stroke);
                self.begin(p.pos, tick, spacing);
                Drawing::Finished(finished)
            }
            (Some(p), false, _) => {
                self.begin(p.pos, tick, spacing);
                Drawing::Drawing
            }
            (Some(p), true, false) => {
                self.stroke.push(p.pos, tick, spacing);
                self.last = p.pos;
                Drawing::Drawing
            }
            // Released, cancelled, or simply no longer reported.
            (None, true, _) => {
                self.down = false;
                Drawing::Finished(core::mem::take(&mut self.stroke))
            }
            (None, false, _) => Drawing::Idle,
        }
    }

    fn begin(&mut self, at: Vec2, tick: u64, spacing: f32) {
        self.down = true;
        self.stroke.clear();
        self.stroke.push(at, tick, spacing);
        self.last = at;
    }

    /// Throw away any line in progress without finishing it — used when the game
    /// changes phase underneath the player's finger.
    pub fn cancel(&mut self) {
        self.down = false;
        self.stroke.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    thread_local! {
        /// A monotonic clock for the tests, so each `update` lands on its own
        /// tick exactly as the frame loop delivers them.
        static CLOCK: Cell<u64> = const { Cell::new(0) };
    }

    /// The next tick.
    fn tick() -> u64 {
        CLOCK.with(|c| {
            c.set(c.get() + 1);
            c.get()
        })
    }

    fn down(x: f32, y: f32) -> Option<Pointer> {
        Some(Pointer {
            pos: Vec2::new(x, y),
            down: true,
        })
    }

    #[test]
    fn a_drawing_accumulates_while_the_finger_is_down_and_arrives_on_release() {
        let mut c = StrokeCapture::new();
        assert_eq!(c.update(None, tick(), 4.0, 390.0), Drawing::Idle);
        assert_eq!(c.update(down(100.0, 700.0), tick(), 4.0, 390.0), Drawing::Drawing);
        assert!(c.drawing());
        (1..6).for_each(|i| {
            let step = i as f32 * 20.0;
            assert_eq!(
                c.update(down(100.0 + step, 700.0 - step), tick(), 4.0, 390.0),
                Drawing::Drawing
            );
        });
        assert_eq!(c.stroke().len(), 6);
        let Drawing::Finished(line) = c.update(None, tick(), 4.0, 390.0) else {
            panic!("releasing must finish the line");
        };
        assert_eq!(line.len(), 6);
        assert!(!c.drawing());
        assert!(c.stroke().is_empty(), "and the capture is ready for the next");
    }

    #[test]
    fn a_new_press_starts_a_new_line_rather_than_extending_the_last() {
        let mut c = StrokeCapture::new();
        c.update(down(10.0, 10.0), tick(), 4.0, 390.0);
        c.update(down(60.0, 60.0), tick(), 4.0, 390.0);
        c.update(None, tick(), 4.0, 390.0);
        c.update(down(300.0, 300.0), tick(), 4.0, 390.0);
        assert_eq!(c.stroke().len(), 1);
        assert_eq!(c.stroke().points()[0], Vec2::new(300.0, 300.0));
    }

    #[test]
    fn a_contact_that_teleports_finishes_the_line_and_begins_another() {
        let mut c = StrokeCapture::new();
        c.update(down(50.0, 700.0), tick(), 4.0, 390.0);
        c.update(down(80.0, 640.0), tick(), 4.0, 390.0);
        // One finger lifts and another lands, with the release lost on the way.
        let Drawing::Finished(line) = c.update(down(360.0, 120.0), tick(), 4.0, 390.0) else {
            panic!("a teleport must end the old line");
        };
        assert_eq!(line.len(), 2);
        assert!(c.drawing(), "and the new one is already going");
        assert_eq!(c.stroke().points()[0], Vec2::new(360.0, 120.0));
    }

    #[test]
    fn cancelling_leaves_nothing_stuck_to_the_finger() {
        let mut c = StrokeCapture::new();
        c.update(down(50.0, 700.0), tick(), 4.0, 390.0);
        c.update(down(90.0, 600.0), tick(), 4.0, 390.0);
        c.cancel();
        assert!(!c.drawing());
        assert!(c.stroke().is_empty());
        // The next contact is a fresh line, and releasing it does not resurrect
        // the abandoned one.
        assert_eq!(c.update(down(10.0, 10.0), tick(), 4.0, 390.0), Drawing::Drawing);
        let Drawing::Finished(line) = c.update(None, tick(), 4.0, 390.0) else {
            panic!("finished");
        };
        assert_eq!(line.len(), 1);
    }

    #[test]
    fn a_sample_that_arrives_already_released_is_not_a_drawing() {
        let mut c = StrokeCapture::new();
        assert_eq!(
            c.update(
                Some(Pointer {
                    pos: Vec2::ZERO,
                    down: false
                }),
                tick(),
                4.0,
                390.0
            ),
            Drawing::Idle
        );
        assert!(!c.drawing());
    }
}
