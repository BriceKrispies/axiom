//! A single keyframe: a local transform sampled at a deterministic tick.

use axiom_kernel::Tick;
use axiom_math::Transform;

/// One sample on an animation track: the bone's local [`Transform`] at a fixed
/// [`Tick`]. Time is an integer engine tick — never wall-clock — so a clip
/// sampled at the same tick always yields the same pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    time: Tick,
    transform: Transform,
}

impl Keyframe {
    /// A keyframe placing `transform` at `time`.
    pub const fn new(time: Tick, transform: Transform) -> Self {
        Keyframe { time, transform }
    }

    /// The tick this keyframe is anchored at.
    pub const fn time(self) -> Tick {
        self.time
    }

    /// The local transform at this keyframe.
    pub const fn transform(self) -> Transform {
        self.transform
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    #[test]
    fn keyframe_keeps_time_and_transform() {
        let xf = Transform::from_translation(Vec3::new(1.0, 0.0, 0.0));
        let key = Keyframe::new(Tick::new(5), xf);
        assert_eq!(key.time(), Tick::new(5));
        assert_eq!(key.transform(), xf);
    }
}
