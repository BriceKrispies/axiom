//! The versioned wire frame peers exchange.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
    SchemaVersion,
};

use crate::net_command::NetCommand;
use crate::peer_id::PeerId;

/// The wire format version. Compatibility is by major (see [`SchemaVersion`]),
/// so a peer rejects a frame from an incompatible major.
const WIRE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

const TAG_INPUT: u8 = 0;
const TAG_HASH_BEACON: u8 = 1;

/// One frame on the wire: either a peer's input for a tick, or a peer's state
/// hash for a confirmed tick.
///
/// Every frame is prefixed with [`WIRE_VERSION`] and a one-byte discriminant,
/// then decoded with bounds checks — a truncated or version-mismatched or
/// unknown-tag frame fails with a precise kernel error rather than panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetMessage {
    /// A peer's input scheduled at `tick`.
    Input {
        /// The peer that authored the input.
        peer: PeerId,
        /// The tick the input executes at.
        tick: u64,
        /// The input itself.
        command: NetCommand,
    },
    /// A peer's state-hash report for a confirmed `tick`.
    HashBeacon {
        /// The peer reporting the hash.
        peer: PeerId,
        /// The confirmed tick the hash is for.
        tick: u64,
        /// The 256-bit state fingerprint.
        hash: [u8; 32],
    },
}

impl NetMessage {
    /// Encode this frame to bytes (version header, tag, then fields).
    pub fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        WIRE_VERSION.write_to(&mut w);
        match self {
            NetMessage::Input {
                peer,
                tick,
                command,
            } => {
                w.write_u8(TAG_INPUT);
                peer.write_to(&mut w);
                w.write_u64(*tick);
                command.write_to(&mut w);
            }
            NetMessage::HashBeacon { peer, tick, hash } => {
                w.write_u8(TAG_HASH_BEACON);
                peer.write_to(&mut w);
                w.write_u64(*tick);
                for &byte in hash {
                    w.write_u8(byte);
                }
            }
        }
        w.into_bytes()
    }

    /// Decode a frame previously produced by [`Self::encode`].
    ///
    /// Fails with `SchemaVersionMismatch` for an incompatible major,
    /// `InvalidDiscriminant` for an unknown tag, or the reader's
    /// `OutOfBounds` / `TruncatedData` for a short buffer.
    pub fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        let version = SchemaVersion::read_from(&mut r)?;
        if !version.is_compatible_with(WIRE_VERSION) {
            return Err(KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::SchemaVersionMismatch,
                "netcode wire version is incompatible with this peer",
            ));
        }
        let tag = r.read_u8()?;
        match tag {
            TAG_INPUT => {
                let peer = PeerId::read_from(&mut r)?;
                let tick = r.read_u64()?;
                let command = NetCommand::read_from(&mut r)?;
                Ok(NetMessage::Input {
                    peer,
                    tick,
                    command,
                })
            }
            TAG_HASH_BEACON => {
                let peer = PeerId::read_from(&mut r)?;
                let tick = r.read_u64()?;
                let mut hash = [0u8; 32];
                for byte in &mut hash {
                    *byte = r.read_u8()?;
                }
                Ok(NetMessage::HashBeacon { peer, tick, hash })
            }
            _ => Err(KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::InvalidDiscriminant,
                "unknown netcode message tag",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> NetMessage {
        NetMessage::Input {
            peer: PeerId::from_raw(3),
            tick: 7,
            command: NetCommand::new(2, vec![9, 9, 9]),
        }
    }

    fn beacon() -> NetMessage {
        NetMessage::HashBeacon {
            peer: PeerId::from_raw(4),
            tick: 12,
            hash: [0xAB; 32],
        }
    }

    #[test]
    fn input_round_trips() {
        assert_eq!(NetMessage::decode(&input().encode()).unwrap(), input());
    }

    #[test]
    fn hash_beacon_round_trips() {
        assert_eq!(NetMessage::decode(&beacon().encode()).unwrap(), beacon());
    }

    #[test]
    fn unknown_tag_is_invalid_discriminant() {
        let mut w = BinaryWriter::new();
        WIRE_VERSION.write_to(&mut w);
        w.write_u8(99); // not a known tag
        let err = NetMessage::decode(&w.into_bytes()).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Binary);
        assert_eq!(err.code(), KernelErrorCode::InvalidDiscriminant);
    }

    #[test]
    fn incompatible_major_is_rejected() {
        let mut w = BinaryWriter::new();
        SchemaVersion::new(WIRE_VERSION.major() + 1, 0).write_to(&mut w);
        w.write_u8(TAG_INPUT);
        let err = NetMessage::decode(&w.into_bytes()).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::SchemaVersionMismatch);
    }

    #[test]
    fn every_truncated_prefix_of_a_frame_is_rejected() {
        // Walks the `?` error arm of every field read in both variants (version,
        // tag, peer, tick, command / hash) — including the empty buffer (k = 0).
        for msg in [input(), beacon()] {
            let bytes = msg.encode();
            for k in 0..bytes.len() {
                assert!(
                    NetMessage::decode(&bytes[..k]).is_err(),
                    "prefix len {k} must fail to decode"
                );
            }
            assert!(NetMessage::decode(&bytes).is_ok());
        }
    }
}
