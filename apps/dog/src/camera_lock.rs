//! Whether the page's camera gestures are live — **app policy**, browser-free.
//!
//! A scene you can orbit is worth having, and a scene that *only* orbits is not:
//! on a phone the canvas is most of the screen, and a canvas that owns every
//! touch inside its box is a canvas the page cannot be scrolled past. The lock is
//! the answer to that. It is one bit with two consequences, and both of them are
//! deliberate:
//!
//! * the camera stops moving — every gesture path into [`crate::OrbitState`] is a
//!   no-op while it holds (see `src/orbit.rs`);
//! * the canvas stops *claiming* the gestures it is no longer using, so a drag
//!   scrolls the page, a pinch zooms it and a right-click opens the menu, exactly
//!   as they would over any other element (see `src/pointer_input.rs`).
//!
//! Locking the camera and leaving the canvas swallowing every touch would be the
//! worse half of the feature on its own: the shot would hold still and the page
//! would still be unusable around it.
//!
//! Everything the lock *means* is answered here, natively, so the browser edge
//! (`src/lock_input.rs`) is left doing nothing but turning a click into a value —
//! the same split the dial panel has between [`crate::Dial`] and
//! `src/slider_input.rs`, and the stage switch has between [`crate::Stage`] and
//! `src/stage_input.rs`.

/// How many lock states there are: one button, two faces.
pub const LOCK_COUNT: usize = 2;

/// The query parameter the lock round-trips through.
///
/// The detail dial reloads the page and so does a device rotation (`NOTES.md`
/// §7, §8). A reload that silently handed the camera back would undo a choice
/// the user made on purpose, so the lock travels in the address bar next to the
/// stage and the debug view.
pub const LOCK_KEY: &str = "lock";

/// Whether the camera answers the page's gestures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraLock {
    /// Gestures drive the camera: drag orbits, wheel or pinch zooms, right-drag
    /// pans. The state the app opens in.
    Free,
    /// The camera holds the shot it is on, and the canvas hands every gesture
    /// back to the page.
    Locked,
}

impl CameraLock {
    /// Both states, in the order the button cycles them.
    pub const ALL: [CameraLock; LOCK_COUNT] = [CameraLock::Free, CameraLock::Locked];

    /// Whether this state holds the camera still.
    pub fn holds(self) -> bool {
        matches!(self, CameraLock::Locked)
    }

    /// The label the button wears in this state. A single toggle is clearer
    /// stating what it *is* than what it would do, and the lit style the active
    /// stage button already uses says the same thing a second way.
    pub fn label(self) -> &'static str {
        ["lock camera", "camera locked"][self as usize]
    }

    /// The class the **camera** hint wears in this state — the one naming the
    /// orbit gestures, hidden while they do nothing.
    pub fn camera_hint_class(self) -> &'static str {
        ["hint", "hint gone"][self as usize]
    }

    /// The class the **dog** hint wears — the one naming the drag, shown only
    /// while the crowd is the thing the pointer reaches.
    pub fn dog_hint_class(self) -> &'static str {
        ["hint gone", "hint"][self as usize]
    }

    /// This state's `?lock=` value. [`CameraLock::Free`] is the empty string,
    /// which `page_url::remember_param` writes by *removing* the parameter — the
    /// opening state leaves no trace in the bar.
    pub fn param(self) -> &'static str {
        ["", "on"][self as usize]
    }

    /// The state `value` names, or [`CameraLock::Free`] for anything else — an
    /// absent parameter and a junk one both open free.
    pub fn from_param(value: &str) -> CameraLock {
        CameraLock::ALL
            .into_iter()
            .find(|lock| lock.param() == value)
            .unwrap_or(CameraLock::Free)
    }

    /// The other state — the whole of what pressing the button decides.
    pub fn toggled(self) -> CameraLock {
        CameraLock::ALL[usize::from(!self.holds())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_states_are_opposites_and_the_button_cycles_between_them() {
        assert!(!CameraLock::Free.holds());
        assert!(CameraLock::Locked.holds());
        assert_eq!(CameraLock::Free.toggled(), CameraLock::Locked);
        assert_eq!(CameraLock::Locked.toggled(), CameraLock::Free);
        // Two presses are where you started: the toggle is an involution, not a
        // one-way latch.
        assert_eq!(CameraLock::Free.toggled().toggled(), CameraLock::Free);
    }

    #[test]
    fn each_state_wears_its_own_label_and_shows_exactly_one_hint() {
        let labels: Vec<&str> = CameraLock::ALL.into_iter().map(CameraLock::label).collect();
        assert_ne!(labels[0], labels[1]);
        assert!(labels.iter().all(|label| !label.is_empty()));
        // Exactly one of the two hints is up in either state — never both (which
        // would name gestures that do nothing) and never neither.
        for lock in CameraLock::ALL {
            let up = [lock.camera_hint_class(), lock.dog_hint_class()]
                .into_iter()
                .filter(|class| *class == "hint")
                .count();
            assert_eq!(up, 1, "{lock:?} shows {up} hints");
        }
        // ...and it is the camera's hint when the camera is what moves.
        assert_eq!(CameraLock::Free.camera_hint_class(), "hint");
        assert_eq!(CameraLock::Locked.dog_hint_class(), "hint");
    }

    #[test]
    fn the_lock_round_trips_through_the_address_bar_and_opens_free_by_default() {
        for lock in CameraLock::ALL {
            assert_eq!(CameraLock::from_param(lock.param()), lock);
        }
        // A bare URL, and a hostile one, both open on a camera the user can move.
        assert_eq!(CameraLock::from_param(""), CameraLock::Free);
        assert_eq!(CameraLock::from_param("nonsense"), CameraLock::Free);
        assert_eq!(CameraLock::from_param("off"), CameraLock::Free);
        // ...and the free state is written by *removing* the parameter.
        assert!(CameraLock::Free.param().is_empty());
        assert!(!CameraLock::Locked.param().is_empty());
        // The key is not a dial key, so it survives a slider move (see
        // `page_url::kept`).
        assert!(crate::config::Dial::from_key(LOCK_KEY).is_none());
    }
}
