//! One opaque, deterministic frame-local command record.

/// A single frame-local command.
///
/// Layer 04 deliberately does not invent a command alphabet — that is the
/// concern of higher engine layers. A [`FrameCommand`] is the smallest
/// shape every future system can agree on: a monotonic sequence number
/// assigned by the queue, an opaque `u32` kind code chosen by the
/// producer, and an owned byte payload that may be empty.
///
/// The byte payload is plain data (no kernel `BinaryWriter`-typed bytes
/// here yet; this layer does not need them). Two commands compare equal
/// iff their sequence, kind, and payload bytes are byte-identical.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameCommand {
    sequence: u64,
    kind: u32,
    payload: Vec<u8>,
}

impl FrameCommand {
    /// Construct a command directly. Normally produced by
    /// [`crate::FrameCommandQueue::push`].
    pub fn new(sequence: u64, kind: u32, payload: Vec<u8>) -> Self {
        FrameCommand {
            sequence,
            kind,
            payload,
        }
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn kind(&self) -> u32 {
        self.kind
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let c = FrameCommand::new(7, 42, vec![1, 2, 3]);
        assert_eq!(c.sequence(), 7);
        assert_eq!(c.kind(), 42);
        assert_eq!(c.payload(), &[1, 2, 3]);
    }

    #[test]
    fn equality_requires_all_three_fields() {
        let a = FrameCommand::new(1, 7, vec![9]);
        let b = FrameCommand::new(1, 7, vec![9]);
        assert_eq!(a, b);
        assert_ne!(a, FrameCommand::new(2, 7, vec![9]));
        assert_ne!(a, FrameCommand::new(1, 8, vec![9]));
        assert_ne!(a, FrameCommand::new(1, 7, vec![9, 0]));
    }

    #[test]
    fn empty_payload_is_supported() {
        let c = FrameCommand::new(0, 0, Vec::new());
        assert!(c.payload().is_empty());
    }
}
