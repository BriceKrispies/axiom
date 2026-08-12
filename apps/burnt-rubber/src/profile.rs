//! [`PlayProfile`] — which lateral control model a session is playing.
//!
//! Burnt Rubber has two, and they are genuinely different games. [`Wheel`] is
//! the driving game this app was built as: continuous analogue steering, a
//! chassis that oversteers, a handbrake that breaks traction, and a lateral
//! position that is *emergent* from all three. [`Rails`] inverts that — lateral
//! position is *driven*, you name a lane and the car goes to it, and the skill
//! moves from holding a line to picking the right gap at speed.
//!
//! # The shipping game is Rails, on every device
//!
//! It was not always. Rails existed because the wheel game is unplayable with a
//! thumb, so a phone got the lane game and a desktop kept the driving game, and
//! this type existed to ask "is this a phone?" exactly once. **That split is
//! gone.** Both surfaces now play Rails, and the profile is chosen by the
//! composition root ([`crate::web`]) rather than derived from the device.
//!
//! The device predicate that used to make the choice — a `for_presentation`
//! mirroring the stylesheet's `@media (pointer: coarse), (max-width: 820px)` —
//! is deleted rather than left returning a constant. A function whose arguments
//! no longer affect its answer is a lie with a test suite attached, and the
//! lockstep it had to keep with the stylesheet was a maintenance obligation
//! bought for a decision nothing makes any more. The stylesheet still switches
//! *layout* at that width; layout is all it ever decided on its own.
//!
//! What remains device-shaped is not the game but the **input**: a phone drives
//! Rails with lane buttons and swipes, a desktop drives the same Rails with
//! `A`/`D`. Both arrive as [`crate::DriveCommand::lane_step`] and the simulation
//! cannot tell them apart — which is exactly the property that let the desktop
//! move onto rails without the sim learning a new word.
//!
//! # Why this is one value and not a scatter of `is_mobile` checks
//!
//! The temptation is to test "am I on a phone?" wherever it matters — in the
//! controller, in the pad layout, in the HUD. That is how a codebase acquires
//! six subtly different definitions of "mobile" that disagree on a tablet. This
//! type exists so the question is asked **once** and every consequence is
//! derived from the answer.
//!
//! Everything the profile decides is listed here, and this list is the contract:
//!
//! | Decision | [`PlayProfile::Wheel`] | [`PlayProfile::Rails`] |
//! |---|---|---|
//! | Lateral control | analogue steer, `-1..1` | discrete lane hops |
//! | Chassis yaw | integrated from steering | derived from lane motion |
//! | Handbrake / drifting | yes | no — there is no slide to catch |
//! | On-screen controls | dynamic joystick + GAS/BRAKE/DRIFT/BOOST | LEFT/RIGHT + GAS/BOOST, and swipe |
//! | Off-road / traffic contact | arcade bump, run continues | arcade bump, run continues |
//!
//! The course, the traffic, the boost economy, the near-miss reward and the
//! finish line are deliberately **not** in that table: they are the game, and
//! they are the same game on both. The profile changes how you steer, not what
//! you are steering through.
//!
//! [`Wheel`]: PlayProfile::Wheel
//! [`Rails`]: PlayProfile::Rails

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

impl PlayProfile {
    /// The profile every shipping session plays, on every device.
    ///
    /// Named rather than written inline at the one call site so that "what game
    /// does the browser start?" is answerable from this file — the same reason
    /// the device predicate it replaced lived here.
    pub const SHIPPING: PlayProfile = PlayProfile::Rails;

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

    /// The desktop and the phone play the same game. There is no viewport, no
    /// pointer type and no device to consult — which is the whole content of
    /// this change, so it is worth one assertion rather than none.
    #[test]
    fn every_device_ships_the_lane_game() {
        assert_eq!(PlayProfile::SHIPPING, PlayProfile::Rails);
        assert!(PlayProfile::SHIPPING.is_rails());
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
