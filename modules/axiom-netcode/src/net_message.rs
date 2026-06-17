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

/// The `kind` discriminant for an `Input` frame — identical to its wire tag, so
/// the in-memory kind and the on-wire tag never drift.
pub(crate) const KIND_INPUT: u8 = TAG_INPUT;
/// The `kind` discriminant for a `HashBeacon` frame — identical to its wire tag.
pub(crate) const KIND_HASH_BEACON: u8 = TAG_HASH_BEACON;

/// The placeholder command stored in a `HashBeacon` frame, where no input
/// command exists. Constructors set it deterministically so two equal frames of
/// the same kind always compare equal under the derived `Eq` (the `command`
/// field is read only when `kind == KIND_INPUT`).
fn absent_command() -> NetCommand {
    NetCommand::new(0, Vec::new())
}

/// The placeholder hash stored in an `Input` frame, where no state hash exists.
/// Constructors set it deterministically for the same equality reason (the
/// `hash` field is read only when `kind == KIND_HASH_BEACON`).
const ABSENT_HASH: [u8; 32] = [0u8; 32];

/// One frame on the wire: either a peer's input for a tick, or a peer's state
/// hash for a confirmed tick. Every frame carries an ed25519 [`Signature`] over
/// its canonical body, so the author cannot be forged.
///
/// This is a **tagged struct**, not an enum: `kind` selects which logical frame
/// this is ([`KIND_INPUT`] or [`KIND_HASH_BEACON`]), `peer` and `signature` are
/// common to both kinds (read with no branch), and the remaining fields are the
/// payload of one kind or the other. A field that does not belong to the active
/// kind holds a deterministic placeholder and is never read for that kind, so
/// payload extraction is a kind-gated field read rather than a `match`.
///
/// Every frame is prefixed with [`WIRE_VERSION`] and a one-byte discriminant,
/// then decoded with bounds checks — a truncated or version-mismatched or
/// unknown-tag frame fails with a precise kernel error rather than panicking.
/// The signature is **structural only** here (its bytes round-trip); whether it
/// is *valid* for a given author is decided by the session against its roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetMessage {
    /// Which kind of frame this is: [`KIND_INPUT`] or [`KIND_HASH_BEACON`].
    kind: u8,
    /// The peer this frame claims as its author (common to both kinds).
    peer: PeerId,
    /// The author's signature over this frame's canonical body (common to both
    /// kinds).
    signature: Signature,
    /// The tick this frame is for — the execution tick of an input, or the
    /// confirmed tick of a beacon (common to both kinds).
    tick: u64,
    /// The input command — meaningful only when `kind == KIND_INPUT`.
    command: NetCommand,
    /// The 256-bit state fingerprint — meaningful only when
    /// `kind == KIND_HASH_BEACON`.
    hash: [u8; 32],
}

impl NetMessage {
    /// Construct an `Input` frame: a peer's `command` scheduled at `tick`,
    /// authenticated by `signature`. The `hash` field is the absent placeholder.
    pub(crate) fn input(
        peer: PeerId,
        tick: u64,
        command: NetCommand,
        signature: Signature,
    ) -> Self {
        NetMessage {
            kind: KIND_INPUT,
            peer,
            signature,
            tick,
            command,
            hash: ABSENT_HASH,
        }
    }

    /// Construct a `HashBeacon` frame: a peer's state `hash` for confirmed
    /// `tick`, authenticated by `signature`. The `command` field is the absent
    /// placeholder.
    pub(crate) fn hash_beacon(
        peer: PeerId,
        tick: u64,
        hash: [u8; 32],
        signature: Signature,
    ) -> Self {
        NetMessage {
            kind: KIND_HASH_BEACON,
            peer,
            signature,
            tick,
            command: absent_command(),
            hash,
        }
    }

    /// This frame's kind discriminant ([`KIND_INPUT`] or [`KIND_HASH_BEACON`]).
    /// Common to both kinds — a plain field read, the basis for kind-gated
    /// payload extraction.
    pub(crate) fn kind(&self) -> u8 {
        self.kind
    }

    /// The input command, present (`Some`) only for an `Input` frame; `None` for
    /// a beacon, whose `command` field is the absent placeholder.
    pub(crate) fn command(&self) -> Option<&NetCommand> {
        (self.kind == KIND_INPUT).then_some(&self.command)
    }

    /// The state hash, present (`Some`) only for a `HashBeacon` frame; `None` for
    /// an input, whose `hash` field is the absent placeholder.
    pub(crate) fn hash(&self) -> Option<&[u8; 32]> {
        (self.kind == KIND_HASH_BEACON).then_some(&self.hash)
    }

    /// The tick this frame is for (common to both kinds).
    pub(crate) fn tick(&self) -> u64 {
        self.tick
    }

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
        hash.iter().for_each(|&byte| w.write_u8(byte));
        w.into_bytes()
    }

    /// The peer this frame claims as its author (to be verified against a
    /// roster). Common to both kinds — a plain field read, no branch.
    pub(crate) fn peer(&self) -> PeerId {
        self.peer
    }

    /// This frame's signature. Common to both kinds — a plain field read.
    pub(crate) fn signature(&self) -> &Signature {
        &self.signature
    }

    /// The canonical bytes this frame's signature must cover. The signing
    /// payload is per-kind, so the kind selects which payload to build: an
    /// `Input` frame signs over its command, a `HashBeacon` over its hash. The
    /// `then(..).unwrap_or_else(..)` builds exactly one payload — the EXACT bytes
    /// that kind has always produced — with no `match`.
    pub(crate) fn signed_bytes(&self) -> Vec<u8> {
        (self.kind == KIND_INPUT)
            .then(|| Self::input_signing_payload(self.peer, self.tick, &self.command))
            .unwrap_or_else(|| Self::beacon_signing_payload(self.peer, self.tick, &self.hash))
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
        SchemaVersion::read_from(&mut r)
            .and_then(|version| {
                // `then_some`/`ok_or_else` is the branchless form of the
                // incompatible-major guard: compatible -> Ok(()), else the
                // SchemaVersionMismatch error.
                version
                    .is_compatible_with(WIRE_VERSION)
                    .then_some(())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorScope::Binary,
                            KernelErrorCode::SchemaVersionMismatch,
                            "netcode wire version is incompatible with this peer",
                        )
                    })
            })
            .and_then(|()| r.read_u8())
            .and_then(|tag| Self::decode_body(&mut r, tag))
    }

    /// Decode the frame body for a known `tag`, or fail with
    /// `InvalidDiscriminant`. The tag dispatch is a value comparison (not an
    /// enum match): `then`/`or_else` selects exactly one decode path so the
    /// reader is consumed by only the matching arm, and an unknown tag falls
    /// through to the discriminant error.
    fn decode_body(r: &mut BinaryReader<'_>, tag: u8) -> KernelResult<Self> {
        (tag == TAG_INPUT)
            .then(|| Self::decode_input(r))
            .or_else(|| (tag == TAG_HASH_BEACON).then(|| Self::decode_beacon(r)))
            .unwrap_or_else(|| {
                Err(KernelError::new(
                    KernelErrorScope::Binary,
                    KernelErrorCode::InvalidDiscriminant,
                    "unknown netcode message tag",
                ))
            })
    }

    /// Decode an `Input` body (the fields after the tag), as a `?`-free chain of
    /// fallible field reads.
    fn decode_input(r: &mut BinaryReader<'_>) -> KernelResult<Self> {
        PeerId::read_from(r).and_then(|peer| {
            r.read_u64().and_then(|tick| {
                NetCommand::read_from(r).and_then(|command| {
                    Signature::read_from(r)
                        .map(|signature| NetMessage::input(peer, tick, command, signature))
                })
            })
        })
    }

    /// Decode a `HashBeacon` body (the fields after the tag), as a `?`-free chain
    /// of fallible field reads.
    fn decode_beacon(r: &mut BinaryReader<'_>) -> KernelResult<Self> {
        PeerId::read_from(r).and_then(|peer| {
            r.read_u64().and_then(|tick| {
                Self::read_hash(r).and_then(|hash| {
                    Signature::read_from(r)
                        .map(|signature| NetMessage::hash_beacon(peer, tick, hash, signature))
                })
            })
        })
    }

    /// Read the 32 hash bytes in order, failing on the first short read. The
    /// `try_fold` writes each byte into its slot and threads the reader's
    /// `OutOfBounds`/`TruncatedData` error out of the fold — the branchless form
    /// of the per-byte `?` loop.
    fn read_hash(r: &mut BinaryReader<'_>) -> KernelResult<[u8; 32]> {
        (0..32usize).try_fold([0u8; 32], |mut hash, i| {
            r.read_u8().map(|byte| {
                hash[i] = byte;
                hash
            })
        })
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
        NetMessage::input(peer, tick, command, signature)
    }

    fn beacon() -> NetMessage {
        let peer = PeerId::from_raw(4);
        let tick = 12;
        let hash = [0xAB; 32];
        let signature = key().sign(&NetMessage::beacon_signing_payload(peer, tick, &hash));
        NetMessage::hash_beacon(peer, tick, hash, signature)
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
    fn kind_gated_accessors_select_the_active_payload() {
        // An input frame: kind is INPUT, `command()` is Some, `hash()` is None.
        let i = input();
        assert_eq!(i.kind(), KIND_INPUT);
        assert_eq!(i.tick(), 7);
        assert_eq!(i.command(), Some(&NetCommand::new(2, vec![9, 9, 9])));
        assert_eq!(i.hash(), None, "an input carries no hash");
        // A beacon frame: kind is HASH_BEACON, `hash()` is Some, `command()` None.
        let b = beacon();
        assert_eq!(b.kind(), KIND_HASH_BEACON);
        assert_eq!(b.tick(), 12);
        assert_eq!(b.hash(), Some(&[0xAB; 32]));
        assert_eq!(b.command(), None, "a beacon carries no command");
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
