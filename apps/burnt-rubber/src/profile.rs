//! [`PlayProfile`] — the one decision that makes Burnt Rubber a different game
//! on a phone.
//!
//! Burnt Rubber ships two games from one codebase. On a desktop it is the
//! driving game this app was built as: continuous analogue steering, a chassis
//! that oversteers, a handbrake that breaks traction. On a phone that game is
//! not playable — a thumb on a dynamic joystick cannot hold a racing line, and
//! the interesting part of the car model (catching a slide) is exactly the part
//! a touchscreen cannot express. So the phone gets a *lane* game: the car is on
//! rails, you pick a lane, and the skill is threading traffic at speed.
//!
//! # Why this is one value and not a scatter of `is_mobile` checks
//!
//! The temptation is to test "am I on a phone?" wherever it matters — in the
//! controller, in the pad layout, in the HUD. That is how a codebase acquires
//! six subtly different definitions of "mobile" that disagree on a tablet. This
//! type exists so the question is asked **once**, at the platform edge, and
//! every consequence is derived from the answer.
//!
//! Everything the profile decides is listed here, and this list is the contract:
//!
//! | Decision | [`PlayProfile::Wheel`] | [`PlayProfile::Rails`] |
//! |---|---|---|
//! | Lateral control | analogue steer, `-1..1` | discrete lane hops |
//! | Chassis yaw | integrated from steering | derived from lane motion |
//! | Handbrake / drifting | yes | no — there is no slide to catch |
//! | On-screen controls | dynamic joystick + GAS/BRAKE/DRIFT/BOOST | LEFT/RIGHT + GAS/BOOST |
//! | Off-road / traffic contact | arcade bump, run continues | arcade bump, run continues |
//!
//! The course, the traffic, the boost economy, the near-miss reward and the
//! finish line are deliberately **not** in that table: they are the game, and
//! they are the same game on both. The profile changes how you steer, not what
//! you are steering through.
//!
//! # The platform predicate lives with the profile
//!
//! [`PlayProfile::for_presentation`] is the single definition of "this is a
//! phone", and it is deliberately the same condition the stylesheet uses to go
//! full-screen (`@media (pointer: coarse), (max-width: 820px)` in `web/index.html`).
//! If the two ever disagree you get the worst outcome available: a full-screen
//! phone layout driving the desktop game, or a desktop layout with lane buttons.
//! Keeping the predicate here — as a pure function of values the browser edge
//! passes in — is what lets a native test assert the boundary without a browser.

/// Which of the two games this session is playing.
///
/// See the module docs for the full list of what this decides; the short version
/// is that `Wheel` steers a car and `Rails` picks a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayProfile {
    /// The desktop driving game: analogue steering, oversteer, handbrake.
    #[default]
    Wheel,
    /// The phone lane game: the car is on rails and hops between lanes.
    Rails,
}

/// The viewport width (CSS px) at or below which the page switches to its
/// full-screen phone layout. Must stay in lockstep with the `max-width` in
/// `web/index.html`'s media query — see the module docs.
pub const PHONE_MAX_WIDTH: f32 = 820.0;

impl PlayProfile {
    /// The profile for a presentation of `viewport_width` CSS pixels on a device
    /// whose primary pointer is `coarse` (a finger rather than a mouse).
    ///
    /// Mirrors `@media (pointer: coarse), (max-width: 820px)` exactly, including
    /// its `or`: a phone qualifies on the pointer alone even held in landscape
    /// past 820 px, and a narrow desktop window qualifies on width alone, which
    /// is what makes the lane game reachable on a development machine without a
    /// phone in hand.
    pub fn for_presentation(viewport_width: f32, coarse_pointer: bool) -> PlayProfile {
        let narrow = viewport_width.is_finite() && viewport_width <= PHONE_MAX_WIDTH;
        [PlayProfile::Wheel, PlayProfile::Rails][usize::from(coarse_pointer | narrow)]
    }

    /// Whether the car is lane-locked rather than freely steered.
    pub const fn is_rails(self) -> bool {
        matches!(self, PlayProfile::Rails)
    }

    /// Whether this profile offers the handbrake at all. Rails has no slide to
    /// catch, so a drift button would be a control that does nothing.
    pub const fn has_handbrake(self) -> bool {
        matches!(self, PlayProfile::Wheel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_mouse_driven_window_is_the_wheel_game() {
        assert_eq!(
            PlayProfile::for_presentation(1440.0, false),
            PlayProfile::Wheel
        );
    }

    #[test]
    fn a_coarse_pointer_is_rails_even_when_the_window_is_wide() {
        // A phone in landscape, or a tablet: wider than the width cut-off, but a
        // finger cannot drive the wheel game. The media query's `or` is why this
        // is Rails, and this test is what keeps the two in step.
        assert_eq!(
            PlayProfile::for_presentation(1180.0, true),
            PlayProfile::Rails
        );
    }

    #[test]
    fn a_narrow_window_is_rails_even_with_a_mouse() {
        // The development path: drag a desktop window narrow and you get the
        // phone game, no device required.
        assert_eq!(
            PlayProfile::for_presentation(PHONE_MAX_WIDTH, false),
            PlayProfile::Rails
        );
        assert_eq!(
            PlayProfile::for_presentation(PHONE_MAX_WIDTH + 1.0, false),
            PlayProfile::Wheel
        );
    }

    #[test]
    fn a_non_finite_width_is_not_treated_as_narrow() {
        // `viewport()` falls back to a NaN-free default, but a browser that
        // hands back a garbage width must not silently switch the game.
        assert_eq!(
            PlayProfile::for_presentation(f32::NAN, false),
            PlayProfile::Wheel
        );
    }

    #[test]
    fn the_profile_decides_rails_and_the_handbrake_together() {
        assert!(!PlayProfile::Wheel.is_rails());
        assert!(PlayProfile::Wheel.has_handbrake());
        assert!(PlayProfile::Rails.is_rails());
        assert!(!PlayProfile::Rails.has_handbrake());
    }

    #[test]
    fn the_default_profile_is_the_desktop_game() {
        assert_eq!(PlayProfile::default(), PlayProfile::Wheel);
    }
}
