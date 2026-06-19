// Public entry point for the `@axiom/client` browser SDK.
//
// Re-exports the ergonomic client and the low-level protocol codec/constants so
// game authors can use the high-level `AxiomClient` or drop down to the wire
// format when they need to. The server stays authoritative; this package is glue
// and ergonomics, not engine truth.

export { AxiomClient } from "./client.ts";
export type { ClientStatus, ConnectConfig } from "./client.ts";
export {
  WebSocketTransport,
  WebTransportTransport,
  WebRtcTransport,
} from "./transport.ts";
export type {
  Transport,
  TransportKind,
  TransportHandlers,
  WebSocketLike,
} from "./transport.ts";

export {
  ProtocolError,
  WIRE_MAJOR,
  WIRE_MINOR,
  KIND_JOIN_ROOM,
  KIND_LEAVE_ROOM,
  KIND_CLIENT_INTENT,
  KIND_WELCOME,
  KIND_SERVER_SNAPSHOT,
  KIND_SERVER_EVENT,
  KIND_REJECTED_INTENT,
  MAX_ROOM_ID_LEN,
  MAX_PAYLOAD_LEN,
  REASON_UNSPECIFIED,
  REASON_MALFORMED,
  REASON_OUT_OF_ORDER,
  REASON_NOT_IN_ROOM,
  peekKind,
  decodeFrame,
  encodeJoinRoom,
  encodeLeaveRoom,
  encodeClientIntent,
  encodeWelcome,
  encodeServerSnapshot,
  encodeServerEvent,
  encodeRejectedIntent,
  decodeJoinRoom,
  decodeLeaveRoom,
  decodeClientIntent,
  decodeWelcome,
  decodeServerSnapshot,
  decodeServerEvent,
  decodeRejectedIntent,
} from "./protocol.ts";

export type {
  DecodedMessage,
  JoinRoomMessage,
  LeaveRoomMessage,
  ClientIntentMessage,
  WelcomeMessage,
  ServerSnapshotMessage,
  ServerEventMessage,
  RejectedIntentMessage,
} from "./protocol.ts";
