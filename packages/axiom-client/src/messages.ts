/*
 * Decoded message shapes and the wire constants that discriminate them.
 *
 * This mirrors the `axiom-net-protocol` Rust module byte-for-byte: the same
 * little-endian framing, the same one-byte kind discriminants, and the same size
 * bounds. The Rust module owns the canonical contract; this is its browser twin.
 */

/** Wire-codec format version (the encoding version; compatibility is by major). */
export const WIRE_MAJOR = 1;
export const WIRE_MINOR = 0;

/** Stable one-byte message-kind discriminants (must match the Rust `frame` module). */
export const KIND_JOIN_ROOM = 0;
export const KIND_LEAVE_ROOM = 1;
export const KIND_CLIENT_INTENT = 2;
export const KIND_WELCOME = 3;
export const KIND_SERVER_SNAPSHOT = 4;
export const KIND_SERVER_EVENT = 5;
export const KIND_REJECTED_INTENT = 6;
/** Per-player-addressed client intent (carries the originating player id). */
export const KIND_CLIENT_INTENT_FOR = 7;
/** Per-player-addressed server snapshot (carries per-player acknowledgements). */
export const KIND_SERVER_SNAPSHOT_FOR = 8;

/** Documented size bounds (must match the Rust module). */
export const MAX_ROOM_ID_LEN = 64;
export const MAX_PAYLOAD_LEN = 65_536;
/** The maximum number of per-player acknowledgements a single snapshot may carry. */
export const MAX_ACKS = 4096;

/** Well-known machine-readable reject reasons. */
export const REASON_UNSPECIFIED = 0;
export const REASON_MALFORMED = 1;
export const REASON_OUT_OF_ORDER = 2;
export const REASON_NOT_IN_ROOM = 3;

/** The set of valid decoded message kinds (the discriminant union). */
export type DecodedKind =
  | typeof KIND_JOIN_ROOM
  | typeof KIND_LEAVE_ROOM
  | typeof KIND_CLIENT_INTENT
  | typeof KIND_WELCOME
  | typeof KIND_SERVER_SNAPSHOT
  | typeof KIND_SERVER_EVENT
  | typeof KIND_REJECTED_INTENT
  | typeof KIND_CLIENT_INTENT_FOR
  | typeof KIND_SERVER_SNAPSHOT_FOR;

export interface JoinRoomMessage {
  readonly kind: typeof KIND_JOIN_ROOM;
  readonly protocolVersion: number;
  readonly roomId: Uint8Array;
  readonly token: Uint8Array;
}
export interface LeaveRoomMessage {
  readonly kind: typeof KIND_LEAVE_ROOM;
  readonly roomId: Uint8Array;
}
export interface ClientIntentMessage {
  readonly kind: typeof KIND_CLIENT_INTENT;
  readonly clientSequence: number;
  readonly predictedClientTick: number;
  readonly lastSeenServerTick: number;
  readonly payload: Uint8Array;
}
export interface WelcomeMessage {
  readonly kind: typeof KIND_WELCOME;
  readonly protocolVersion: number;
  readonly clientId: number;
  readonly serverTick: number;
  readonly fixedStepNs: number;
}
export interface ServerSnapshotMessage {
  readonly kind: typeof KIND_SERVER_SNAPSHOT;
  readonly serverTick: number;
  readonly lastAcceptedClientSequence: number;
  readonly payload: Uint8Array;
}
export interface ServerEventMessage {
  readonly kind: typeof KIND_SERVER_EVENT;
  readonly serverTick: number;
  readonly payload: Uint8Array;
}
export interface RejectedIntentMessage {
  readonly kind: typeof KIND_REJECTED_INTENT;
  readonly clientSequence: number;
  readonly reasonCode: number;
}
/** One per-player acknowledgement carried by a {@link ServerSnapshotForMessage}. */
export interface PlayerAck {
  readonly player: number;
  readonly sequence: number;
}
export interface ClientIntentForMessage {
  readonly kind: typeof KIND_CLIENT_INTENT_FOR;
  readonly player: number;
  readonly clientSequence: number;
  readonly predictedClientTick: number;
  readonly lastSeenServerTick: number;
  readonly payload: Uint8Array;
}
export interface ServerSnapshotForMessage {
  readonly kind: typeof KIND_SERVER_SNAPSHOT_FOR;
  readonly serverTick: number;
  readonly acks: readonly PlayerAck[];
  readonly payload: Uint8Array;
}

export type DecodedMessage =
  | JoinRoomMessage
  | LeaveRoomMessage
  | ClientIntentMessage
  | WelcomeMessage
  | ServerSnapshotMessage
  | ServerEventMessage
  | RejectedIntentMessage
  | ClientIntentForMessage
  | ServerSnapshotForMessage;
