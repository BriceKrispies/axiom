//! `Seconds` — a finite span of presentation time, in seconds.

use crate::binary_reader::BinaryReader;
use crate::binary_writer::BinaryWriter;
use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::reflect::Reflect;
use crate::result::KernelResult;
use crate::type_schema::TypeSchema;

/// A span of **presentation** time, in seconds.
///
/// A kernel quantity primitive: the typed boundary where a raw `f32` becomes a
/// dimensioned duration, so the presentation layers above stop passing naked
/// floats whose unit a caller has to guess. The inner scalar is always finite —
/// [`Seconds::new`] is the only constructor and it rejects NaN / infinity.
///
/// `Seconds` is deliberately **not** a simulation tick. It is the *real
/// frame-delta* clock the presentation side runs on — the `dt` an `onRender`
/// pass advances a visual-only system (particles, easing, flip-books) by. It is
/// distinct from:
///
/// - [`crate::Tick`] / [`crate::TickDelta`] — the deterministic, fixed-step
///   *simulation* clock. A `Seconds` value can never stand in for a tick: ticks
///   are authoritative and replayable, presentation seconds are wall-clock-fed
///   and never feed sim back.
/// - an audio-clock second (the `AudioSeconds` newtype in `axiom-audio`) — a
///   *scheduling* time on the audio device's own clock, again a separate wall so
///   a render delta can never be mistaken for an audio cue time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seconds(f32);

impl Seconds {
    /// Construct a duration, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        [
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Seconds must be finite",
            )),
            Ok(Seconds(value)),
        ][value.is_finite() as usize]
    }

    /// The underlying scalar value, in seconds.
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Reflect for Seconds {
    const SCHEMA: TypeSchema = TypeSchema::new("Seconds", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.0.reflect_write(writer);
    }

    /// Read a duration, re-validating finiteness (a non-finite scalar in the
    /// byte stream is rejected exactly as [`Seconds::new`] would).
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f32::reflect_read(reader).and_then(Seconds::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Seconds::new(0.016).unwrap().get(), 0.016);
    }

    #[test]
    fn new_rejects_nan() {
        let e = Seconds::new(f32::NAN).unwrap_err();
        assert_eq!(e.scope(), KernelErrorScope::Scalar);
        assert_eq!(e.code(), KernelErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_infinity() {
        assert_eq!(
            Seconds::new(f32::INFINITY).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn reflect_round_trips_rejects_truncation_and_nonfinite() {
        let s = Seconds::new(0.016).unwrap();
        let mut w = BinaryWriter::new();
        s.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Seconds::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            s
        );
        assert!(Seconds::reflect_read(&mut BinaryReader::new(&[])).is_err());
        let mut bad = BinaryWriter::new();
        bad.write_f32(f32::INFINITY);
        assert!(Seconds::reflect_read(&mut BinaryReader::new(&bad.into_bytes())).is_err());
        assert_eq!(<Seconds as Reflect>::SCHEMA.name(), "Seconds");
    }
}
