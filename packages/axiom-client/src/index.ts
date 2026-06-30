/*
 * Public entry point for the `@axiom/client` browser SDK.
 *
 * Re-exports the ergonomic client and the low-level protocol codec/constants so
 * game authors can use the high-level `AxiomClient` or drop down to the wire
 * format. The server stays authoritative; this package is glue and ergonomics,
 * not engine truth.
 */

export { AxiomClient } from "./client.ts";
export type { ClientStatus, ConnectConfig } from "./client-config.ts";

export { WebSocketTransport } from "./transport.ts";
export type { SocketLike, Transport, TransportHandlers, TransportKind } from "./transport.ts";
export { DatagramTransport } from "./datagram-transport.ts";
export type { DatagramLink, DatagramLinkFactory } from "./datagram-transport.ts";
export { WebTransportTransport } from "./webtransport.ts";
export { WebRtcTransport } from "./webrtc.ts";

export { ProtocolError } from "./protocol-error.ts";

export {
  decodeClientIntent,
  decodeFrame,
  decodeJoinRoom,
  decodeLeaveRoom,
  decodeRejectedIntent,
  decodeServerEvent,
  decodeServerSnapshot,
  decodeWelcome,
  encodeClientIntent,
  encodeJoinRoom,
  encodeLeaveRoom,
  encodeRejectedIntent,
  encodeServerEvent,
  encodeServerSnapshot,
  encodeWelcome,
  peekKind,
} from "./codec.ts";
export type { ClientIntentFields, WelcomeFields } from "./codec.ts";

export {
  decodeClientIntentFor,
  decodeServerSnapshotFor,
  encodeClientIntentFor,
  encodeServerSnapshotFor,
} from "./per-player-codec.ts";
export type { ClientIntentForFields } from "./per-player-codec.ts";

export {
  applySnapshot,
  decodeServerSnapshotForDelta,
  diffSnapshot,
  encodeServerSnapshotForDelta,
} from "./snapshot-delta.ts";
export type { ServerSnapshotForDeltaFields } from "./snapshot-delta.ts";

export {
  KIND_CLIENT_INTENT,
  KIND_CLIENT_INTENT_FOR,
  KIND_JOIN_ROOM,
  KIND_LEAVE_ROOM,
  KIND_REJECTED_INTENT,
  KIND_SERVER_EVENT,
  KIND_SERVER_SNAPSHOT,
  KIND_SERVER_SNAPSHOT_FOR,
  KIND_SERVER_SNAPSHOT_FOR_DELTA,
  KIND_WELCOME,
  MAX_ACKS,
  MAX_PAYLOAD_LEN,
  MAX_ROOM_ID_LEN,
  REASON_MALFORMED,
  REASON_NOT_IN_ROOM,
  REASON_OUT_OF_ORDER,
  REASON_UNSPECIFIED,
  WIRE_MAJOR,
  WIRE_MINOR,
} from "./messages.ts";
export type {
  ClientIntentForMessage,
  ClientIntentMessage,
  DecodedKind,
  DecodedMessage,
  JoinRoomMessage,
  LeaveRoomMessage,
  PlayerAck,
  RejectedIntentMessage,
  ServerEventMessage,
  ServerSnapshotForDeltaMessage,
  ServerSnapshotForMessage,
  ServerSnapshotMessage,
  WelcomeMessage,
} from "./messages.ts";
