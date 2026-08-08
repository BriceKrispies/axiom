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
/// contact entirely. Browsers do drop a `pointerup`, and without this the next
/// press continues the last line instead of starting a new one.
const JUMP: f32 = 0.30;

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
    pub fn update(&mut self, pointer: Option<Pointer>, spacing: f32, short_edge: f32) -> Drawing {
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
                self.begin(p.pos, spacing);
                Drawing::Finished(finished)
            }
            (Some(p), false, _) => {
                self.begin(p.pos, spacing);
                Drawing::Drawing
            }
            (Some(p), true, false) => {
                self.stroke.push(p.pos, spacing);
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

    fn begin(&mut self, at: Vec2, spacing: f32) {
        self.down = true;
        self.stroke.clear();
        self.stroke.push(at, spacing);
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

    fn down(x: f32, y: f32) -> Option<Pointer> {
        Some(Pointer {
            pos: Vec2::new(x, y),
            down: true,
        })
    }

    #[test]
    fn a_drawing_accumulates_while_the_finger_is_down_and_arrives_on_release() {
        let mut c = StrokeCapture::new();
        assert_eq!(c.update(None, 4.0, 390.0), Drawing::Idle);
        assert_eq!(c.update(down(100.0, 700.0), 4.0, 390.0), Drawing::Drawing);
        assert!(c.drawing());
        (1..6).for_each(|i| {
            let step = i as f32 * 20.0;
            assert_eq!(
                c.update(down(100.0 + step, 700.0 - step), 4.0, 390.0),
                Drawing::Drawing
            );
        });
        assert_eq!(c.stroke().len(), 6);
        let Drawing::Finished(line) = c.update(None, 4.0, 390.0) else {
            panic!("releasing must finish the line");
        };
        assert_eq!(line.len(), 6);
        assert!(!c.drawing());
        assert!(c.stroke().is_empty(), "and the capture is ready for the next");
    }

    #[test]
    fn a_new_press_starts_a_new_line_rather_than_extending_the_last() {
        let mut c = StrokeCapture::new();
        c.update(down(10.0, 10.0), 4.0, 390.0);
        c.update(down(60.0, 60.0), 4.0, 390.0);
        c.update(None, 4.0, 390.0);
        c.update(down(300.0, 300.0), 4.0, 390.0);
        assert_eq!(c.stroke().len(), 1);
        assert_eq!(c.stroke().points()[0], Vec2::new(300.0, 300.0));
    }

    #[test]
    fn a_contact_that_teleports_finishes_the_line_and_begins_another() {
        let mut c = StrokeCapture::new();
        c.update(down(50.0, 700.0), 4.0, 390.0);
        c.update(down(80.0, 640.0), 4.0, 390.0);
        // One finger lifts and another lands, with the release lost on the way.
        let Drawing::Finished(line) = c.update(down(360.0, 120.0), 4.0, 390.0) else {
            panic!("a teleport must end the old line");
        };
        assert_eq!(line.len(), 2);
        assert!(c.drawing(), "and the new one is already going");
        assert_eq!(c.stroke().points()[0], Vec2::new(360.0, 120.0));
    }

    #[test]
    fn cancelling_leaves_nothing_stuck_to_the_finger() {
        let mut c = StrokeCapture::new();
        c.update(down(50.0, 700.0), 4.0, 390.0);
        c.update(down(90.0, 600.0), 4.0, 390.0);
        c.cancel();
        assert!(!c.drawing());
        assert!(c.stroke().is_empty());
        // The next contact is a fresh line, and releasing it does not resurrect
        // the abandoned one.
        assert_eq!(c.update(down(10.0, 10.0), 4.0, 390.0), Drawing::Drawing);
        let Drawing::Finished(line) = c.update(None, 4.0, 390.0) else {
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
                4.0,
                390.0
            ),
            Drawing::Idle
        );
        assert!(!c.drawing());
    }
}
