//! One player input scheduled to execute at a tick.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

/// A player input: a `kind` tag plus an opaque `payload`.
///
/// This mirrors the engine's existing command shape (`FrameCommand` /
/// `RuntimeCommand` are `{ kind: u32, payload: bytes }`). Netcode treats the
/// payload as opaque bytes — only the app interprets it — and tags each command
/// with its `(peer, tick)` when it enters the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetCommand {
    kind: u32,
    payload: Vec<u8>,
}

impl NetCommand {
    /// Construct a command from a kind tag and payload bytes.
    pub fn new(kind: u32, payload: Vec<u8>) -> Self {
        NetCommand { kind, payload }
    }

    /// The command kind tag.
    pub fn kind(&self) -> u32 {
        self.kind
    }

    /// The opaque payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Serialize as a little-endian `u32` kind then a length-prefixed payload.
    pub fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.kind);
        writer.write_byte_slice(&self.payload);
    }

    /// Read a command previously written with [`Self::write_to`].
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        let kind = reader.read_u32()?;
        let payload = reader.read_byte_slice()?.to_vec();
        Ok(NetCommand { kind, payload })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip() {
        let c = NetCommand::new(5, vec![1, 2, 3]);
        assert_eq!(c.kind(), 5);
        assert_eq!(c.payload(), &[1, 2, 3]);
    }

    #[test]
    fn serialization_round_trips() {
        let c = NetCommand::new(9, vec![0xAA, 0xBB]);
        let mut w = BinaryWriter::new();
        c.write_to(&mut w);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(NetCommand::read_from(&mut r).unwrap(), c);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn empty_payload_round_trips() {
        let c = NetCommand::new(0, Vec::new());
        let mut w = BinaryWriter::new();
        c.write_to(&mut w);
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(NetCommand::read_from(&mut r).unwrap(), c);
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        // Decoding any prefix shorter than the full frame must fail — this walks
        // the `?` error arm of every field read (kind, then payload).
        let mut w = BinaryWriter::new();
        NetCommand::new(9, vec![1, 2, 3]).write_to(&mut w);
        let bytes = w.into_bytes();
        for k in 0..bytes.len() {
            let mut r = BinaryReader::new(&bytes[..k]);
            assert!(
                NetCommand::read_from(&mut r).is_err(),
                "prefix len {k} must fail"
            );
        }
        assert!(NetCommand::read_from(&mut BinaryReader::new(&bytes)).is_ok());
    }

    #[test]
    fn equality_requires_same_kind_and_payload() {
        assert_ne!(NetCommand::new(1, vec![0]), NetCommand::new(2, vec![0]));
        assert_ne!(NetCommand::new(1, vec![0]), NetCommand::new(1, vec![1]));
    }
}
