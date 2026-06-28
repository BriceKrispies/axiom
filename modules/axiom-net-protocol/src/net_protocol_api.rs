//! The single public facade of the `axiom-net-protocol` module.

use axiom_kernel::KernelResult;

use crate::client_intent::ClientIntent;
use crate::client_intent_for::ClientIntentFor;
use crate::frame;
use crate::join_room::JoinRoom;
use crate::leave_room::LeaveRoom;
use crate::opaque_payload::MAX_PAYLOAD_LEN;
use crate::rejected_intent::{
    RejectedIntent, REASON_MALFORMED, REASON_NOT_IN_ROOM, REASON_OUT_OF_ORDER, REASON_UNSPECIFIED,
};
use crate::room_id::MAX_ROOM_ID_LEN;
use crate::server_event::ServerEvent;
use crate::server_snapshot::ServerSnapshot;
use crate::server_snapshot_for::{ServerSnapshotFor, MAX_ACKS};
use crate::welcome::Welcome;

/// The decoded fields of a per-player `ServerSnapshotFor`: the authoritative
/// `server_tick`, the `(player, sequence)` acknowledgements, and the opaque
/// snapshot `payload`. A transparent alias so the facade still returns plain
/// primitives while keeping the nested-tuple signature readable.
type DecodedServerSnapshotFor = (u64, Vec<(u64, u64)>, Vec<u8>);

/// The multiplayer wire contract — the only public export of `axiom-net-protocol`.
///
/// This is a stateless codec namespace: every method is an associated function
/// that validates and encodes, or decodes and validates, one protocol message.
/// Because a module exposes a single nameable type, messages cross this boundary
/// as plain primitives (`u32` / `u64` / `&[u8]` / `Vec<u8>`) — the same shape a
/// socket sees — so an app or the TypeScript package can own the transport
/// without naming a protocol type.
///
/// Encoders return `KernelResult<Vec<u8>>` and fail on invalid input (a zero
/// protocol version, an empty/over-long room id, an over-size payload, a zero
/// fixed step). `RejectedIntent` has nothing to validate, so its encoder is
/// infallible. Decoders return the message's fields as a tuple, or a
/// `KernelError` for an incompatible version, an unknown/unexpected kind, a
/// truncated body, or an over-size payload.
#[derive(Debug, Clone, Copy, Default)]
pub struct NetProtocolApi;

impl NetProtocolApi {
    /// Stable message-kind discriminant of a `JoinRoom` frame.
    pub const KIND_JOIN_ROOM: u8 = frame::KIND_JOIN_ROOM;
    /// Stable message-kind discriminant of a `LeaveRoom` frame.
    pub const KIND_LEAVE_ROOM: u8 = frame::KIND_LEAVE_ROOM;
    /// Stable message-kind discriminant of a `ClientIntent` frame.
    pub const KIND_CLIENT_INTENT: u8 = frame::KIND_CLIENT_INTENT;
    /// Stable message-kind discriminant of a `Welcome` frame.
    pub const KIND_WELCOME: u8 = frame::KIND_WELCOME;
    /// Stable message-kind discriminant of a `ServerSnapshot` frame.
    pub const KIND_SERVER_SNAPSHOT: u8 = frame::KIND_SERVER_SNAPSHOT;
    /// Stable message-kind discriminant of a `ServerEvent` frame.
    pub const KIND_SERVER_EVENT: u8 = frame::KIND_SERVER_EVENT;
    /// Stable message-kind discriminant of a `RejectedIntent` frame.
    pub const KIND_REJECTED_INTENT: u8 = frame::KIND_REJECTED_INTENT;
    /// Stable message-kind discriminant of a per-player `ClientIntentFor` frame.
    pub const KIND_CLIENT_INTENT_FOR: u8 = frame::KIND_CLIENT_INTENT_FOR;
    /// Stable message-kind discriminant of a per-player `ServerSnapshotFor` frame.
    pub const KIND_SERVER_SNAPSHOT_FOR: u8 = frame::KIND_SERVER_SNAPSHOT_FOR;

    /// The maximum room-id length, in bytes.
    pub const MAX_ROOM_ID_LEN: usize = MAX_ROOM_ID_LEN;
    /// The maximum opaque-payload length, in bytes.
    pub const MAX_PAYLOAD_LEN: usize = MAX_PAYLOAD_LEN;
    /// The maximum number of per-player acks a `ServerSnapshotFor` may carry.
    pub const MAX_ACKS: usize = MAX_ACKS;

    /// Reject reason: unspecified / generic refusal.
    pub const REASON_UNSPECIFIED: u32 = REASON_UNSPECIFIED;
    /// Reject reason: the intent was malformed or violated the protocol.
    pub const REASON_MALFORMED: u32 = REASON_MALFORMED;
    /// Reject reason: the intent arrived out of order or too late to apply.
    pub const REASON_OUT_OF_ORDER: u32 = REASON_OUT_OF_ORDER;
    /// Reject reason: the client is not a member of the room it addressed.
    pub const REASON_NOT_IN_ROOM: u32 = REASON_NOT_IN_ROOM;
}

impl NetProtocolApi {
    /// Peek the message kind of an encoded frame without decoding its body.
    /// Validates the wire version and rejects an unknown kind, returning one of
    /// the `KIND_*` discriminants so a dispatcher can route the frame.
    pub fn message_kind(bytes: &[u8]) -> KernelResult<u8> {
        frame::peek_kind(bytes)
    }

    /// Encode a `JoinRoom` (client → server).
    pub fn encode_join_room(
        protocol_version: u32,
        room_id: &[u8],
        token: &[u8],
    ) -> KernelResult<Vec<u8>> {
        JoinRoom::new(protocol_version, room_id, token).map(|m| m.encode())
    }

    /// Encode a `LeaveRoom` (client → server).
    pub fn encode_leave_room(room_id: &[u8]) -> KernelResult<Vec<u8>> {
        LeaveRoom::new(room_id).map(|m| m.encode())
    }

    /// Encode a `ClientIntent` (client → server).
    pub fn encode_client_intent(
        client_sequence: u64,
        predicted_client_tick: u64,
        last_seen_server_tick: u64,
        payload: &[u8],
    ) -> KernelResult<Vec<u8>> {
        ClientIntent::new(
            client_sequence,
            predicted_client_tick,
            last_seen_server_tick,
            payload,
        )
        .map(|m| m.encode())
    }

    /// Encode a `Welcome` (server → client).
    pub fn encode_welcome(
        protocol_version: u32,
        client_id: u64,
        server_tick: u64,
        fixed_step_ns: u64,
    ) -> KernelResult<Vec<u8>> {
        Welcome::new(protocol_version, client_id, server_tick, fixed_step_ns).map(|m| m.encode())
    }

    /// Encode a `ServerSnapshot` (server → client).
    pub fn encode_server_snapshot(
        server_tick: u64,
        last_accepted_client_sequence: u64,
        payload: &[u8],
    ) -> KernelResult<Vec<u8>> {
        ServerSnapshot::new(server_tick, last_accepted_client_sequence, payload).map(|m| m.encode())
    }

    /// Encode a `ServerEvent` (server → client).
    pub fn encode_server_event(server_tick: u64, payload: &[u8]) -> KernelResult<Vec<u8>> {
        ServerEvent::new(server_tick, payload).map(|m| m.encode())
    }

    /// Encode a `RejectedIntent` (server → client). Infallible: any sequence and
    /// any machine-readable reason code is representable.
    pub fn encode_rejected_intent(client_sequence: u64, reason_code: u32) -> Vec<u8> {
        RejectedIntent::new(client_sequence, reason_code).encode()
    }

    /// Decode a `JoinRoom`, returning `(protocol_version, room_id, token)`.
    pub fn decode_join_room(bytes: &[u8]) -> KernelResult<(u32, Vec<u8>, Vec<u8>)> {
        JoinRoom::decode(bytes).map(|m| {
            (
                m.protocol_version(),
                m.room_id().to_vec(),
                m.token().to_vec(),
            )
        })
    }

    /// Decode a `LeaveRoom`, returning the `room_id`.
    pub fn decode_leave_room(bytes: &[u8]) -> KernelResult<Vec<u8>> {
        LeaveRoom::decode(bytes).map(|m| m.room_id().to_vec())
    }

    /// Decode a `ClientIntent`, returning `(client_sequence,
    /// predicted_client_tick, last_seen_server_tick, payload)`.
    pub fn decode_client_intent(bytes: &[u8]) -> KernelResult<(u64, u64, u64, Vec<u8>)> {
        ClientIntent::decode(bytes).map(|m| {
            (
                m.client_sequence(),
                m.predicted_client_tick(),
                m.last_seen_server_tick(),
                m.payload().to_vec(),
            )
        })
    }

    /// Decode a `Welcome`, returning `(protocol_version, client_id, server_tick,
    /// fixed_step_ns)`.
    pub fn decode_welcome(bytes: &[u8]) -> KernelResult<(u32, u64, u64, u64)> {
        Welcome::decode(bytes).map(|m| {
            (
                m.protocol_version(),
                m.client_id(),
                m.server_tick(),
                m.fixed_step_ns(),
            )
        })
    }

    /// Decode a `ServerSnapshot`, returning `(server_tick,
    /// last_accepted_client_sequence, payload)`.
    pub fn decode_server_snapshot(bytes: &[u8]) -> KernelResult<(u64, u64, Vec<u8>)> {
        ServerSnapshot::decode(bytes).map(|m| {
            (
                m.server_tick(),
                m.last_accepted_client_sequence(),
                m.payload().to_vec(),
            )
        })
    }

    /// Decode a `ServerEvent`, returning `(server_tick, payload)`.
    pub fn decode_server_event(bytes: &[u8]) -> KernelResult<(u64, Vec<u8>)> {
        ServerEvent::decode(bytes).map(|m| (m.server_tick(), m.payload().to_vec()))
    }

    /// Decode a `RejectedIntent`, returning `(client_sequence, reason_code)`.
    pub fn decode_rejected_intent(bytes: &[u8]) -> KernelResult<(u64, u32)> {
        RejectedIntent::decode(bytes).map(|m| (m.client_sequence(), m.reason_code()))
    }

    /// Encode a per-player `ClientIntentFor` (client → server): the anonymous
    /// `ClientIntent` body prefixed with the originating `player` seat. Fails on
    /// an over-size payload.
    pub fn encode_client_intent_for(
        player: u64,
        client_sequence: u64,
        predicted_client_tick: u64,
        last_seen_server_tick: u64,
        payload: &[u8],
    ) -> KernelResult<Vec<u8>> {
        ClientIntentFor::new(
            player,
            client_sequence,
            predicted_client_tick,
            last_seen_server_tick,
            payload,
        )
        .map(|m| m.encode())
    }

    /// Decode a `ClientIntentFor`, returning `(player, client_sequence,
    /// predicted_client_tick, last_seen_server_tick, payload)`.
    pub fn decode_client_intent_for(
        bytes: &[u8],
    ) -> KernelResult<(u64, u64, u64, u64, Vec<u8>)> {
        ClientIntentFor::decode(bytes).map(|m| {
            (
                m.player(),
                m.client_sequence(),
                m.predicted_client_tick(),
                m.last_seen_server_tick(),
                m.payload().to_vec(),
            )
        })
    }

    /// Encode a per-player `ServerSnapshotFor` (server → client): the snapshot
    /// body with a bounded list of per-player `(player, sequence)` acks. Fails on
    /// an over-size payload or an ack list longer than [`Self::MAX_ACKS`].
    pub fn encode_server_snapshot_for(
        server_tick: u64,
        acks: &[(u64, u64)],
        payload: &[u8],
    ) -> KernelResult<Vec<u8>> {
        ServerSnapshotFor::new(server_tick, acks, payload).map(|m| m.encode())
    }

    /// Decode a `ServerSnapshotFor`, returning `(server_tick, acks, payload)`.
    pub fn decode_server_snapshot_for(
        bytes: &[u8],
    ) -> KernelResult<DecodedServerSnapshotFor> {
        ServerSnapshotFor::decode(bytes)
            .map(|m| (m.server_tick(), m.acks().to_vec(), m.payload().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelErrorCode;

    #[test]
    fn join_room_round_trips_through_the_facade() {
        let bytes = NetProtocolApi::encode_join_room(1, b"lobby", b"tok").unwrap();
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_JOIN_ROOM
        );
        assert_eq!(
            NetProtocolApi::decode_join_room(&bytes).unwrap(),
            (1, b"lobby".to_vec(), b"tok".to_vec())
        );
    }

    #[test]
    fn leave_room_round_trips_through_the_facade() {
        let bytes = NetProtocolApi::encode_leave_room(b"lobby").unwrap();
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_LEAVE_ROOM
        );
        assert_eq!(
            NetProtocolApi::decode_leave_room(&bytes).unwrap(),
            b"lobby".to_vec()
        );
    }

    #[test]
    fn client_intent_round_trips_through_the_facade() {
        let bytes = NetProtocolApi::encode_client_intent(5, 100, 98, b"in").unwrap();
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_CLIENT_INTENT
        );
        assert_eq!(
            NetProtocolApi::decode_client_intent(&bytes).unwrap(),
            (5, 100, 98, b"in".to_vec())
        );
    }

    #[test]
    fn welcome_round_trips_through_the_facade() {
        let bytes = NetProtocolApi::encode_welcome(1, 77, 42, 16_666_667).unwrap();
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_WELCOME
        );
        assert_eq!(
            NetProtocolApi::decode_welcome(&bytes).unwrap(),
            (1, 77, 42, 16_666_667)
        );
    }

    #[test]
    fn server_snapshot_round_trips_through_the_facade() {
        let bytes = NetProtocolApi::encode_server_snapshot(42, 5, b"st").unwrap();
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_SERVER_SNAPSHOT
        );
        assert_eq!(
            NetProtocolApi::decode_server_snapshot(&bytes).unwrap(),
            (42, 5, b"st".to_vec())
        );
    }

    #[test]
    fn server_event_round_trips_through_the_facade() {
        let bytes = NetProtocolApi::encode_server_event(9, b"ev").unwrap();
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_SERVER_EVENT
        );
        assert_eq!(
            NetProtocolApi::decode_server_event(&bytes).unwrap(),
            (9, b"ev".to_vec())
        );
    }

    #[test]
    fn rejected_intent_round_trips_through_the_facade() {
        let bytes = NetProtocolApi::encode_rejected_intent(5, NetProtocolApi::REASON_OUT_OF_ORDER);
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_REJECTED_INTENT
        );
        assert_eq!(
            NetProtocolApi::decode_rejected_intent(&bytes).unwrap(),
            (5, NetProtocolApi::REASON_OUT_OF_ORDER)
        );
    }

    #[test]
    fn client_intent_for_round_trips_through_the_facade() {
        let bytes = NetProtocolApi::encode_client_intent_for(7, 5, 100, 98, b"in").unwrap();
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_CLIENT_INTENT_FOR
        );
        assert_eq!(
            NetProtocolApi::decode_client_intent_for(&bytes).unwrap(),
            (7, 5, 100, 98, b"in".to_vec())
        );
    }

    #[test]
    fn server_snapshot_for_round_trips_through_the_facade() {
        let bytes =
            NetProtocolApi::encode_server_snapshot_for(42, &[(7, 5), (9, 3)], b"st").unwrap();
        assert_eq!(
            NetProtocolApi::message_kind(&bytes).unwrap(),
            NetProtocolApi::KIND_SERVER_SNAPSHOT_FOR
        );
        assert_eq!(
            NetProtocolApi::decode_server_snapshot_for(&bytes).unwrap(),
            (42, vec![(7, 5), (9, 3)], b"st".to_vec())
        );
    }

    #[test]
    fn per_player_encoders_surface_validation_failures() {
        let big = vec![0u8; NetProtocolApi::MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            NetProtocolApi::encode_client_intent_for(1, 1, 0, 0, &big)
                .unwrap_err()
                .code(),
            KernelErrorCode::OutOfBounds
        );
        let too_many: Vec<(u64, u64)> =
            (0..=NetProtocolApi::MAX_ACKS as u64).map(|p| (p, p)).collect();
        assert_eq!(
            NetProtocolApi::encode_server_snapshot_for(0, &too_many, b"")
                .unwrap_err()
                .code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decoding_a_per_player_frame_as_the_wrong_kind_is_rejected() {
        let intent = NetProtocolApi::encode_client_intent_for(1, 1, 0, 0, b"x").unwrap();
        assert_eq!(
            NetProtocolApi::decode_server_snapshot_for(&intent)
                .unwrap_err()
                .code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn encoders_surface_validation_failures() {
        assert_eq!(
            NetProtocolApi::encode_join_room(0, b"r", b"")
                .unwrap_err()
                .code(),
            KernelErrorCode::InvalidId
        );
        assert_eq!(
            NetProtocolApi::encode_leave_room(b"").unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
        let big = vec![0u8; NetProtocolApi::MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            NetProtocolApi::encode_server_event(0, &big)
                .unwrap_err()
                .code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decoding_the_wrong_kind_is_rejected() {
        let welcome = NetProtocolApi::encode_welcome(1, 1, 0, 1).unwrap();
        // A Welcome frame is server→client; decoding it as a client→server
        // ClientIntent must fail on the kind discriminant.
        assert_eq!(
            NetProtocolApi::decode_client_intent(&welcome)
                .unwrap_err()
                .code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn message_kind_rejects_an_unknown_frame() {
        assert!(NetProtocolApi::message_kind(&[]).is_err());
    }

    #[test]
    fn the_default_facade_is_constructible() {
        // The facade is a zero-sized namespace; constructing it is harmless and
        // keeps it usable as a value where a handle is expected. Exercise the
        // derived Default/Clone/Copy so they are covered.
        let api = <NetProtocolApi as Default>::default();
        let copied = api;
        let cloned = Clone::clone(&api);
        assert_eq!(format!("{copied:?}"), format!("{cloned:?}"));
    }
}
