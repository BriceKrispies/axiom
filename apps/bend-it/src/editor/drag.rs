//! Pointer samples in, drag intents out.
//!
//! One code path covers a finger, a pen and a mouse, because the platform edge
//! reduces all three to the same neutral `(position, is_down)` and this is what
//! reads it. There is no touch build and no mouse build of the game.
//!
//! What it adds over a raw contact is the three things a trajectory editor needs
//! and a raw contact does not have: **where the gesture started** (so a drag can
//! be relative, and therefore 1:1, instead of snapping the curve to wherever the
//! finger lands), **whether it has moved far enough to be a drag at all** (so a
//! tap on a button is not a microscopic edit), and **a clean end** — including
//! the end that happens because the browser took the pointer away mid-gesture,
//! which is otherwise how a curve gets stuck to a finger that is no longer there.

use axiom::prelude::Vec2;
use axiom_input::Pointer;

/// What the gesture did this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragEvent {
    /// Nothing is happening.
    Idle,
    /// A contact went down at this point.
    Begin { at: Vec2 },
    /// A contact is down and has moved past the dead zone.
    Move {
        origin: Vec2,
        at: Vec2,
        delta: Vec2,
    },
    /// A contact was released (or cancelled). `moved` distinguishes a tap from
    /// the end of a drag.
    End {
        origin: Vec2,
        at: Vec2,
        moved: bool,
    },
}

/// How many dead zones a contact may move in one tick before it is treated as a
/// different contact entirely.
const JUMP_MULTIPLE: f32 = 22.0;

/// Tracks one contact across ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragTracker {
    down: bool,
    origin: Vec2,
    at: Vec2,
    moved: bool,
}

impl Default for DragTracker {
    fn default() -> Self {
        DragTracker::new()
    }
}

impl DragTracker {
    pub const fn new() -> DragTracker {
        DragTracker {
            down: false,
            origin: Vec2::ZERO,
            at: Vec2::ZERO,
            moved: false,
        }
    }

    /// Whether a gesture is in progress.
    pub fn active(&self) -> bool {
        self.down
    }

    /// Where the gesture started, and where it is now.
    pub fn origin(&self) -> Vec2 {
        self.origin
    }
    pub fn at(&self) -> Vec2 {
        self.at
    }
    /// Whether it has passed the dead zone.
    pub fn moved(&self) -> bool {
        self.moved
    }

    /// Fold this tick's contact into the gesture.
    ///
    /// `dead_zone` is in pixels. A gesture that has once passed it stays a drag
    /// for the rest of its life, so a finger that drifts back toward its start
    /// does not flicker between tap and drag.
    pub fn update(&mut self, pointer: Option<Pointer>, dead_zone: f32) -> DragEvent {
        // A contact that teleports across the screen between two ticks is not the
        // same contact. Browsers do drop a `pointerup` — a system gesture, a tab
        // switch, one finger lifting as another lands — and without this the next
        // press is read as a continuation of the last one, which welds the curve
        // to a finger that was never there. Anything that moves further than this
        // in a sixtieth of a second is a new gesture.
        let teleported = pointer
            .filter(|p| p.down)
            .filter(|_| self.down)
            .map(|p| p.pos.subtract(self.at).length() > dead_zone.max(1.0) * JUMP_MULTIPLE)
            .unwrap_or(false);
        self.down &= !teleported;
        match (pointer.filter(|p| p.down), self.down) {
            // A new contact.
            (Some(p), false) => {
                self.down = true;
                self.moved = false;
                self.origin = p.pos;
                self.at = p.pos;
                DragEvent::Begin { at: p.pos }
            }
            // A contact that is still down.
            (Some(p), true) => {
                let delta = p.pos.subtract(self.at);
                self.at = p.pos;
                self.moved |= p.pos.subtract(self.origin).length() > dead_zone;
                match self.moved {
                    true => DragEvent::Move {
                        origin: self.origin,
                        at: p.pos,
                        delta,
                    },
                    false => DragEvent::Idle,
                }
            }
            // Released, cancelled, or the browser simply stopped reporting it —
            // all of which have to end the gesture, not leave it hanging.
            (None, true) => {
                self.down = false;
                DragEvent::End {
                    origin: self.origin,
                    at: self.at,
                    moved: self.moved,
                }
            }
            (None, false) => DragEvent::Idle,
        }
    }

    /// Abandon any gesture in progress without emitting an end — used when the
    /// game changes phase underneath the player's finger.
    pub fn cancel(&mut self) {
        self.down = false;
        self.moved = false;
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
    fn a_tap_begins_and_ends_without_ever_moving() {
        let mut t = DragTracker::new();
        assert_eq!(t.update(None, 4.0), DragEvent::Idle);
        assert_eq!(t.update(down(10.0, 10.0), 4.0), DragEvent::Begin {
            at: Vec2::new(10.0, 10.0)
        });
        assert!(t.active());
        // A wobble inside the dead zone is not a drag.
        assert_eq!(t.update(down(12.0, 11.0), 4.0), DragEvent::Idle);
        assert_eq!(
            t.update(None, 4.0),
            DragEvent::End {
                origin: Vec2::new(10.0, 10.0),
                at: Vec2::new(12.0, 11.0),
                moved: false,
            }
        );
        assert!(!t.active());
    }

    #[test]
    fn a_drag_reports_its_origin_so_the_edit_can_be_relative() {
        let mut t = DragTracker::new();
        t.update(down(100.0, 100.0), 4.0);
        let event = t.update(down(140.0, 100.0), 4.0);
        assert_eq!(
            event,
            DragEvent::Move {
                origin: Vec2::new(100.0, 100.0),
                at: Vec2::new(140.0, 100.0),
                delta: Vec2::new(40.0, 0.0),
            }
        );
        assert!(t.moved());
        assert_eq!(t.origin(), Vec2::new(100.0, 100.0));
        assert_eq!(t.at(), Vec2::new(140.0, 100.0));
        // Once it is a drag it stays one, even coming back to the start.
        assert!(matches!(
            t.update(down(100.0, 100.0), 4.0),
            DragEvent::Move { .. }
        ));
    }

    #[test]
    fn a_contact_that_simply_vanishes_still_ends_the_gesture() {
        let mut t = DragTracker::new();
        t.update(down(50.0, 50.0), 4.0);
        t.update(down(90.0, 50.0), 4.0);
        // The browser cancels the pointer: no sample at all.
        let end = t.update(None, 4.0);
        assert!(matches!(end, DragEvent::End { moved: true, .. }));
        assert!(!t.active());
        // A sample that arrives already released is not a gesture either.
        assert_eq!(
            t.update(
                Some(Pointer {
                    pos: Vec2::ZERO,
                    down: false
                }),
                4.0
            ),
            DragEvent::Idle
        );
    }

    #[test]
    fn a_contact_that_teleports_is_treated_as_a_new_gesture() {
        let mut t = DragTracker::new();
        t.update(down(50.0, 50.0), 4.0);
        t.update(down(70.0, 50.0), 4.0);
        // One finger lifts and another lands, with the release lost on the way.
        let event = t.update(down(900.0, 700.0), 4.0);
        assert_eq!(
            event,
            DragEvent::Begin {
                at: Vec2::new(900.0, 700.0)
            },
            "the far press starts a fresh gesture rather than continuing the old"
        );
        assert_eq!(t.origin(), Vec2::new(900.0, 700.0));
        assert!(!t.moved());
        // An ordinary fast drag is NOT a teleport.
        assert!(matches!(
            t.update(down(930.0, 700.0), 4.0),
            DragEvent::Move { .. }
        ));
    }

    #[test]
    fn a_cancelled_gesture_leaves_nothing_stuck_to_the_finger() {
        let mut t = DragTracker::new();
        t.update(down(50.0, 50.0), 4.0);
        t.update(down(200.0, 50.0), 4.0);
        t.cancel();
        assert!(!t.active());
        assert!(!t.moved());
        // The next contact is a fresh gesture, not a continuation.
        assert_eq!(
            t.update(down(10.0, 10.0), 4.0),
            DragEvent::Begin {
                at: Vec2::new(10.0, 10.0)
            }
        );
    }
}
