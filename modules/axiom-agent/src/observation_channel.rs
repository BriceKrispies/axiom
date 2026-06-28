//! The kind of perception an observation entry came from.

/// A declared perception channel for an [`crate::AgentApi`] observation.
///
/// These are *labels*, not implementations. In particular `ScreenSample` marks
/// data a future app/tool may sample from a rendered frame — this module does
/// **not** implement machine vision; it only reserves the channel so observation
/// data can declare its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ObservationChannel {
    /// Symbolic facts the app already knows (the default, fully-deterministic).
    Semantic = 1,
    /// Spatial/geometric measurements.
    Geometric = 2,
    /// A sample taken from a rendered frame (filled by a future app/tool).
    ScreenSample = 3,
    /// Data sourced from a recorded replay.
    Replay = 4,
    /// Diagnostic data for inspection only.
    Debug = 5,
}

impl ObservationChannel {
    /// The stable numeric code of this channel.
    pub const fn code(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_distinct() {
        assert_eq!(ObservationChannel::Semantic.code(), 1);
        assert_eq!(ObservationChannel::Geometric.code(), 2);
        assert_eq!(ObservationChannel::ScreenSample.code(), 3);
        assert_eq!(ObservationChannel::Replay.code(), 4);
        assert_eq!(ObservationChannel::Debug.code(), 5);
    }

    #[test]
    fn derives_are_exercised() {
        let c = ObservationChannel::Geometric;
        let d = c;
        assert_eq!(c, d);
        assert_ne!(c, ObservationChannel::Replay);
        assert!(format!("{c:?}").contains("Geometric"));
    }
}
