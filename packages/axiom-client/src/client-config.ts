/*
 * Public configuration and status types for AxiomClient, plus the status
 * constants its state machine compares against. Kept beside client.ts so the
 * client file stays within the line budget.
 */

import type { ServerEventMessage, ServerSnapshotMessage } from "./messages.ts";
import type { SocketLike, Transport, TransportKind } from "./transport.ts";

export const STATUS_DISCONNECTED = "disconnected";
export const STATUS_CONNECTING = "connecting";
export const STATUS_CONNECTED = "connected";

/** Connection status, mirroring `axiom-client-core`'s state machine. */
export type ClientStatus =
  | typeof STATUS_CONNECTED
  | typeof STATUS_CONNECTING
  | typeof STATUS_DISCONNECTED;

/** Configuration for `AxiomClient.connect`. */
export interface ConnectConfig {
  /** The server URL (e.g. `wss://example.com/play` or `https://host/play`). */
  readonly url: string;
  /** An optional opaque authentication token (string is UTF-8 encoded). */
  readonly token?: string | Uint8Array;
  /** The room to join (string is UTF-8 encoded). */
  readonly roomId: string | Uint8Array;
  /** The application protocol version (default `1`). */
  readonly protocolVersion?: number;
  /** Which transport to use (default `"websocket"`). */
  readonly transport?: TransportKind;
  /** For `"webtransport"`: sha-256 of the server's DER cert (self-signed dev). */
  readonly serverCertificateHash?: Uint8Array;
  /** For `"webrtc"`: the SDP offer/answer signaling endpoint. */
  readonly signalingUrl?: string;
  /** Optional WebSocket factory, so tests can inject a fake. */
  readonly socketFactory?: (url: string) => SocketLike;
  /** Optional full transport override (takes precedence over `transport`). */
  readonly transportFactory?: (config: ConnectConfig) => Transport;
}

export type SnapshotHandler = (snapshot: ServerSnapshotMessage) => void;
export type EventHandler = (event: ServerEventMessage) => void;
export type StatusHandler = (status: ClientStatus) => void;
/** Observes a server-rejected intent, carrying the machine-readable `REASON_*` code. */
export type RejectionHandler = (reasonCode: number) => void;
