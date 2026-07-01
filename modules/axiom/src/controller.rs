//! First-person control: a `Controller` marks a node as a first-person camera,
//! and a `FirstPersonInput` is one tick's look + move for it.
//!
//! This is the orientation-carrying companion to [`crate::player`]: where a
//! `Player` node is translated in world space by a [`crate::player::PlayerInput`]
//! delta, a `Controller` node is yawed and moved **relative to its own facing**
//! by a [`FirstPersonInput`] — the engine's first-person camera primitive.

use axiom_kernel::Meters;
use axiom_math::Vec3;

use crate::angle::Angle;

/// Marks a spawned node as the first-person controller for `index`. Per-tick
/// [`FirstPersonInput`]s addressed to that index yaw the node about +Y and move
/// it along its own facing during the engine's controller system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Controller {
    /// The controller index this node belongs to.
    pub index: u32,
}

impl Controller {
    /// The first-person controller node for `index`.
    pub const fn new(index: u32) -> Self {
        Controller { index }
    }
}

/// One tick's first-person input for a controller: look by `yaw` (about +Y) and
/// `pitch` (about local +X; the engine clamps it), then move by `move_local` in
/// the node's own frame — local -Z is forward, local +X is right. Movement is
/// applied in the yaw-only frame, so looking up/down never tilts it. The app
/// builds these from input each tick and hands them to
/// [`crate::prelude::RunningApp::tick_with_controls`]; the engine applies them
/// deterministically before stepping the frame.
///
/// The optional `seat_y` is an **absolute** vertical seat: when present, the
/// engine seats the controller's local eye exactly at that height (in metres)
/// instead of applying `move_local`'s vertical component as a delta. It is the
/// native, deterministic way for a ground-follow game to keep the eye riding the
/// terrain surface — a plain value (no closure), so the input stays replayable.
/// Left `None` by [`FirstPersonInput::new`], so a free-fly controller (retro FPS's
/// jump/gravity in `move_local.y`) is unaffected; opt in with
/// [`FirstPersonInput::with_seat_y`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FirstPersonInput {
    /// The controller this input is for.
    pub index: u32,
    /// Translation in the node's own frame (-Z forward, +X right).
    pub move_local: Vec3,
    /// Yaw delta about +Y (positive turns left).
    pub yaw: Angle,
    /// Pitch delta about local +X (positive looks up; clamped by the engine).
    pub pitch: Angle,
    /// Optional absolute vertical seat (metres). `Some(m)` seats the local eye
    /// at exactly `m`, overriding `move_local`'s vertical delta; `None` leaves
    /// the vertical component to `move_local` as before.
    pub seat_y: Option<Meters>,
}

impl FirstPersonInput {
    /// A first-person input for `index`, with no vertical seat (`seat_y = None`)
    /// — `move_local`'s vertical component is applied as a delta, exactly as a
    /// free-fly controller expects.
    pub const fn new(index: u32, move_local: Vec3, yaw: Angle, pitch: Angle) -> Self {
        FirstPersonInput {
            index,
            move_local,
            yaw,
            pitch,
            seat_y: None,
        }
    }

    /// This input with an explicit absolute vertical seat: the engine will seat
    /// the controller's local eye at exactly `seat_y` metres this tick, instead
    /// of applying `move_local`'s vertical component. The horizontal step is
    /// applied unchanged. This is the deterministic ground-follow path.
    pub const fn with_seat_y(self, seat_y: Meters) -> Self {
        FirstPersonInput {
            index: self.index,
            move_local: self.move_local,
            yaw: self.yaw,
            pitch: self.pitch,
            seat_y: Some(seat_y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_carries_its_index() {
        assert_eq!(Controller::new(3).index, 3);
    }

    #[test]
    fn first_person_input_carries_its_fields() {
        let input = FirstPersonInput::new(
            1,
            Vec3::new(-0.25, 0.0, -0.5),
            Angle::radians(0.1),
            Angle::radians(-0.05),
        );
        assert_eq!(input.index, 1);
        assert_eq!(input.move_local, Vec3::new(-0.25, 0.0, -0.5));
        assert_eq!(input.yaw.as_radians(), 0.1);
        assert_eq!(input.pitch.as_radians(), -0.05);
        assert_eq!(input.seat_y, None, "new() leaves the vertical seat unset");
    }

    #[test]
    fn with_seat_y_sets_an_absolute_seat_and_preserves_the_rest() {
        let base = FirstPersonInput::new(
            2,
            Vec3::new(0.5, 0.0, -0.25),
            Angle::radians(0.2),
            Angle::radians(-0.1),
        );
        let seated = base.with_seat_y(Meters::new(3.75).expect("seat is finite"));
        assert_eq!(seated.seat_y, Some(Meters::new(3.75).expect("seat is finite")));
        // Every other field is carried through unchanged.
        assert_eq!(seated.index, base.index);
        assert_eq!(seated.move_local, base.move_local);
        assert_eq!(seated.yaw.as_radians(), base.yaw.as_radians());
        assert_eq!(seated.pitch.as_radians(), base.pitch.as_radians());
    }
}
