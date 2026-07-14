//! A single keyframe: a local transform sampled at a deterministic tick.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, Tick};
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

    /// Append the keyframe's bytes: the tick as a `u64` then the transform.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u64(self.time.raw());
        self.transform.write_to(writer);
    }

    /// Read a keyframe written by [`Keyframe::write_to`].
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Keyframe> {
        reader.read_u64().and_then(|raw| {
            Transform::read_from(reader).map(|transform| Keyframe {
                time: Tick::new(raw),
                transform,
            })
        })
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

    #[test]
    fn keyframe_round_trips_through_bytes() {
        let key = Keyframe::new(
            Tick::new(42),
            Transform::from_translation(Vec3::new(1.0, -2.0, 3.0)),
        );
        let mut w = BinaryWriter::new();
        key.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Keyframe::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            key
        );
        assert!(Keyframe::read_from(&mut BinaryReader::new(&bytes[..2])).is_err());
    }
}
