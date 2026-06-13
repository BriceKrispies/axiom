//! The versioned, signed wire frame peers exchange.

use axiom_crypto::Signature;
use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
    SchemaVersion,
};

use crate::net_command::NetCommand;
use crate::peer_id::PeerId;

/// The wire format version. Compatibility is by major (see [`SchemaVersion`]),
/// so a peer rejects a frame from an incompatible major.
const WIRE_VERSION: SchemaVersion = SchemaVersion::new(2, 0);

const TAG_INPUT: u8 = 0;
const TAG_HASH_BEACON: u8 = 1;

/// One frame on the wire: either a peer's input for a tick, or a peer's state
/// hash for a confirmed tick. Every frame carries an ed25519 [`Signature`] over
/// its canonical body, so the author cannot be forged.
///
/// Every frame is prefixed with [`WIRE_VERSION`] and a one-byte discriminant,
/// then decoded with bounds checks — a truncated or version-mismatched or
/// unknown-tag frame fails with a precise kernel error rather than panicking.
/// The signature is **structural only** here (its bytes round-trip); whether it
/// is *valid* for a given author is decided by the session against its roster.
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
        /// The author's signature over this frame's canonical body.
        signature: Signature,
    },
    /// A peer's state-hash report for a confirmed `tick`.
    HashBeacon {
        /// The peer reporting the hash.
        peer: PeerId,
        /// The confirmed tick the hash is for.
        tick: u64,
        /// The 256-bit state fingerprint.
        hash: [u8; 32],
        /// The author's signature over this frame's canonical body.
        signature: Signature,
    },
}

impl NetMessage {
    /// The canonical bytes an `Input` author signs (everything but the
    /// signature). Shared by signing (in the session) and verification, so the
    /// two can never drift.
    pub(crate) fn input_signing_payload(peer: PeerId, tick: u64, command: &NetCommand) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        WIRE_VERSION.write_to(&mut w);
        w.write_u8(TAG_INPUT);
        peer.write_to(&mut w);
        w.write_u64(tick);
        command.write_to(&mut w);
        w.into_bytes()
    }

    /// The canonical bytes a `HashBeacon` author signs (everything but the
    /// signature).
    pub(crate) fn beacon_signing_payload(peer: PeerId, tick: u64, hash: &[u8; 32]) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        WIRE_VERSION.write_to(&mut w);
        w.write_u8(TAG_HASH_BEACON);
        peer.write_to(&mut w);
        w.write_u64(tick);
        for &byte in hash {
            w.write_u8(byte);
        }
        w.into_bytes()
    }

    /// The peer this frame claims as its author (to be verified against a
    /// roster).
    pub(crate) fn peer(&self) -> PeerId {
        match self {
            NetMessage::Input { peer, .. } | NetMessage::HashBeacon { peer, .. } => *peer,
        }
    }

    /// This frame's signature.
    pub(crate) fn signature(&self) -> &Signature {
        match self {
            NetMessage::Input { signature, .. } | NetMessage::HashBeacon { signature, .. } => {
                signature
            }
        }
    }

    /// The canonical bytes this frame's signature must cover.
    pub(crate) fn signed_bytes(&self) -> Vec<u8> {
        match self {
            NetMessage::Input {
                peer,
                tick,
                command,
                ..
            } => Self::input_signing_payload(*peer, *tick, command),
            NetMessage::HashBeacon {
                peer, tick, hash, ..
            } => Self::beacon_signing_payload(*peer, *tick, hash),
        }
    }

    /// Encode this frame to bytes: the canonical signed body, then the signature.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = self.signed_bytes();
        let mut w = BinaryWriter::new();
        self.signature().write_to(&mut w);
        bytes.extend_from_slice(&w.into_bytes());
        bytes
    }

    /// Decode a frame previously produced by [`Self::encode`].
    ///
    /// Fails with `SchemaVersionMismatch` for an incompatible major,
    /// `InvalidDiscriminant` for an unknown tag, or the reader's
    /// `OutOfBounds` / `TruncatedData` for a short buffer. A successful decode
    /// proves the frame is *well-formed*, not that its signature is *valid* —
    /// that is the session's job.
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
                let signature = Signature::read_from(&mut r)?;
                Ok(NetMessage::Input {
                    peer,
                    tick,
                    command,
                    signature,
                })
            }
            TAG_HASH_BEACON => {
                let peer = PeerId::read_from(&mut r)?;
                let tick = r.read_u64()?;
                let mut hash = [0u8; 32];
                for byte in &mut hash {
                    *byte = r.read_u8()?;
                }
                let signature = Signature::read_from(&mut r)?;
                Ok(NetMessage::HashBeacon {
                    peer,
                    tick,
                    hash,
                    signature,
                })
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
    use axiom_crypto::SigningKey;

    fn key() -> SigningKey {
        SigningKey::from_seed([11u8; 32])
    }

    fn input() -> NetMessage {
        let peer = PeerId::from_raw(3);
        let tick = 7;
        let command = NetCommand::new(2, vec![9, 9, 9]);
        let signature = key().sign(&NetMessage::input_signing_payload(peer, tick, &command));
        NetMessage::Input {
            peer,
            tick,
            command,
            signature,
        }
    }

    fn beacon() -> NetMessage {
        let peer = PeerId::from_raw(4);
        let tick = 12;
        let hash = [0xAB; 32];
        let signature = key().sign(&NetMessage::beacon_signing_payload(peer, tick, &hash));
        NetMessage::HashBeacon {
            peer,
            tick,
            hash,
            signature,
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
    fn the_signature_covers_the_genuine_body() {
        // The decoded signature verifies against the frame's signed bytes under
        // the author's key — and fails under a different key.
        for msg in [input(), beacon()] {
            assert!(key()
                .verifying_key()
                .verify(&msg.signed_bytes(), msg.signature()));
            let other = SigningKey::from_seed([99u8; 32]).verifying_key();
            assert!(!other.verify(&msg.signed_bytes(), msg.signature()));
        }
    }

    #[test]
    fn peer_and_signature_accessors_match_each_variant() {
        assert_eq!(input().peer(), PeerId::from_raw(3));
        assert_eq!(beacon().peer(), PeerId::from_raw(4));
        // Accessor returns the same signature the frame was built with.
        assert!(key()
            .verifying_key()
            .verify(&input().signed_bytes(), input().signature()));
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
        // tag, peer, tick, command/hash, signature) — including the empty buffer.
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
