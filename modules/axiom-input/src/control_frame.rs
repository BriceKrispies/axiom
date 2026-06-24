//! The per-frame control intent synthesized from pointer samples.

use axiom_kernel::Radians;
use axiom_math::Vec2;

/// One frame of synthesized control intent, the output of
/// [`crate::TouchControls::update`].
///
/// Reached only through that facade: an app holds the returned value and maps it
/// onto its first-person controller via these accessors, so it never needs to
/// name this type (the same shape the render pipeline's frame report uses).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlFrame {
    move_vector: Vec2,
    yaw: Radians,
    pitch: Radians,
}

impl ControlFrame {
    /// Assemble a control frame. Crate-private: only the synthesizer mints one.
    pub(crate) const fn new(move_vector: Vec2, yaw: Radians, pitch: Radians) -> Self {
        ControlFrame {
            move_vector,
            yaw,
            pitch,
        }
    }

    /// The normalized movement this frame: `x` strafes (+x is right), `y` drives
    /// (+y is forward), each component within the unit disc. [`Vec2::ZERO`] when
    /// no movement pointer is active.
    pub const fn move_vector(&self) -> Vec2 {
        self.move_vector
    }

    /// The yaw delta this frame, about +Y (positive turns left), from the look
    /// drag. Zero when no look pointer is active or it just touched down.
    pub const fn yaw(&self) -> Radians {
        self.yaw
    }

    /// The pitch delta this frame, about local +X (positive looks up), from the
    /// look drag. Zero when no look pointer is active or it just touched down.
    pub const fn pitch(&self) -> Radians {
        self.pitch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radians(value: f32) -> Radians {
        Radians::new(value).unwrap()
    }

    #[test]
    fn accessors_return_the_constructed_fields() {
        let frame = ControlFrame::new(Vec2::new(0.25, -0.5), radians(0.1), radians(-0.2));
        assert_eq!(frame.move_vector(), Vec2::new(0.25, -0.5));
        assert_eq!(frame.yaw(), radians(0.1));
        assert_eq!(frame.pitch(), radians(-0.2));
    }

    #[test]
    fn frame_is_copy_and_equal() {
        let a = ControlFrame::new(Vec2::ZERO, radians(0.0), radians(0.0));
        let b = a;
        assert_eq!(a, b);
    }
}
