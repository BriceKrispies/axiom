// AxiomClient — the browser authoring SDK.
//
// This is browser glue and author ergonomics, NOT engine truth. The server stays
// authoritative; this client sends *intents* and applies *snapshots*. It mirrors
// the concepts of the portable `axiom-client-core` Rust module (connection
// state, a monotonic client sequence, a pending-intent queue, the latest server
// tick) on the browser side, where the WebSocket actually lives.
//
// Deliberately absent in this first version: prediction, rollback, reconnect,
// and compression. The client does not hide server authority and invents no
// gameplay API.

import {
  decodeFrame,
  encodeClientIntent,
  encodeJoinRoom,
  encodeLeaveRoom,
  KIND_REJECTED_INTENT,
  KIND_SERVER_EVENT,
  KIND_SERVER_SNAPSHOT,
  KIND_WELCOME,
  type RejectedIntentMessage,
  type ServerEventMessage,
  type ServerSnapshotMessage,
} from "./protocol.ts";
import {
  WebRtcTransport,
  WebSocketTransport,
  WebTransportTransport,
  type Transport,
  type TransportKind,
  type WebSocketLike,
} from "./transport.ts";

/** Connection status, mirroring `axiom-client-core`'s state machine. */
export type ClientStatus = "disconnected" | "connecting" | "connected";

/** How often to resend JoinRoom over an unreliable transport until welcomed. */
const JOIN_RESEND_MS = 250;

/** Configuration for {@link AxiomClient.connect}. */
export interface ConnectConfig {
  /** The server URL (e.g. `wss://example.com/play` or `https://host/play`). */
  url: string;
  /** An optional opaque authentication token (string is UTF-8 encoded). */
  token?: string | Uint8Array;
  /** The room to join (string is UTF-8 encoded). */
  roomId: string | Uint8Array;
  /** The application protocol version (default `1`). */
  protocolVersion?: number;
  /** Which transport to use (default `"websocket"`). */
  transport?: TransportKind;
  /** For `"webtransport"`: sha-256 of the server's DER cert (self-signed dev). */
  serverCertificateHash?: Uint8Array;
  /** For `"webrtc"`: the SDP offer/answer signaling endpoint (e.g. `/rtc/offer`). */
  signalingUrl?: string;
  /**
   * Optional WebSocket factory, so tests (or non-browser hosts) can inject a
   * fake. Used only for the `"websocket"` transport; defaults to global
   * `WebSocket`.
   */
  socketFactory?: (url: string) => WebSocketLike;
  /** Optional full transport override (takes precedence over `transport`). */
  transportFactory?: (config: ConnectConfig) => Transport;
}

type SnapshotHandler = (snapshot: ServerSnapshotMessage) => void;
type EventHandler = (event: ServerEventMessage) => void;
type StatusHandler = (status: ClientStatus) => void;

function toBytes(value: string | Uint8Array): Uint8Array {
  return typeof value === "string" ? new TextEncoder().encode(value) : value;
}

/** Build the transport for a connect config (override > kind > default). */
function buildTransport(config: ConnectConfig): Transport {
  if (config.transportFactory) return config.transportFactory(config);
  if (config.transport === "webrtc") {
    return new WebRtcTransport(config.signalingUrl ?? config.url);
  }
  if (config.transport === "webtransport") {
    return new WebTransportTransport(config.url, config.serverCertificateHash);
  }
  const factory = config.socketFactory;
  return new WebSocketTransport(
    () => (factory ? factory(config.url) : (new WebSocket(config.url) as unknown as WebSocketLike)),
  );
}

/**
 * A connected (or connecting) browser client. Construct one, register handlers,
 * then {@link connect}. The same instance can be reused after {@link disconnect}.
 */
export class AxiomClient {
  private transport: Transport | null = null;
  private status: ClientStatus = "disconnected";
  private protocolVersion = 1;
  private roomId: Uint8Array = new Uint8Array();
  private nextClientSequence = 1;
  private latestServerTick = 0;
  private lastAckedClientSequence = 0;
  private clientId = 0;
  private pending: number[] = [];

  private joinFrame: Uint8Array | null = null;
  private joinTimer: ReturnType<typeof setInterval> | null = null;

  private snapshotHandlers: SnapshotHandler[] = [];
  private eventHandlers: EventHandler[] = [];
  private statusHandlers: StatusHandler[] = [];

  /** Open a connection and join the room once the transport opens. */
  connect(config: ConnectConfig): void {
    this.protocolVersion = config.protocolVersion ?? 1;
    this.roomId = toBytes(config.roomId);
    const token = config.token === undefined ? new Uint8Array() : toBytes(config.token);
    this.joinFrame = encodeJoinRoom(this.protocolVersion, this.roomId, token);

    const transport = buildTransport(config);
    this.transport = transport;
    this.setStatus("connecting");

    transport.open({
      onOpen: () => this.handleOpen(transport),
      onMessage: (bytes) => this.handleInbound(bytes),
      onClose: () => this.handleClose(),
    });
  }

  private handleOpen(transport: Transport): void {
    // Send JoinRoom as soon as the transport opens. Status stays "connecting"
    // until the server's Welcome arrives — the server, not the transport, decides
    // we are in. Over an UNRELIABLE transport the JoinRoom (or its Welcome) can be
    // dropped, so resend it until we are welcomed.
    const join = this.joinFrame;
    if (join === null) return;
    transport.send(join);
    if (!transport.reliable) {
      this.clearJoinTimer();
      this.joinTimer = setInterval(() => {
        if (this.status === "connected") {
          this.clearJoinTimer();
          return;
        }
        transport.send(join);
      }, JOIN_RESEND_MS);
    }
  }

  private handleClose(): void {
    this.transport = null;
    this.clearJoinTimer();
    this.setStatus("disconnected");
  }

  private clearJoinTimer(): void {
    if (this.joinTimer !== null) {
      clearInterval(this.joinTimer);
      this.joinTimer = null;
    }
  }

  /** Send LeaveRoom (if possible) and close the transport, returning to disconnected. */
  disconnect(): void {
    const transport = this.transport;
    this.clearJoinTimer();
    if (transport !== null) {
      if (this.status === "connected") {
        transport.send(encodeLeaveRoom(this.roomId));
      }
      transport.close();
    }
  }

  /**
   * Send a client intent. Succeeds only while connected (server authority): it
   * assigns the next monotonic client sequence, records it as pending, and
   * sends an encoded `ClientIntent`. Returns the **assigned sequence number** so
   * the caller can pair it with the input for client-side prediction/replay, or
   * `null` if not sent (not connected).
   */
  sendIntent(payload: Uint8Array): number | null {
    if (this.status !== "connected" || this.transport === null) return null;
    const sequence = this.nextClientSequence;
    this.nextClientSequence += 1;
    this.pending.push(sequence);
    // Without a local sim clock, the latest applied server tick is the client's
    // best estimate of "now" for both prediction hint and last-seen fields.
    this.transport.send(
      encodeClientIntent(sequence, this.latestServerTick, this.latestServerTick, payload),
    );
    return sequence;
  }

  /** Register a handler for authoritative snapshots. */
  onSnapshot(handler: SnapshotHandler): void {
    this.snapshotHandlers.push(handler);
  }

  /** Register a handler for authoritative server events. */
  onEvent(handler: EventHandler): void {
    this.eventHandlers.push(handler);
  }

  /** Register a handler for connection-status changes. */
  onStatus(handler: StatusHandler): void {
    this.statusHandlers.push(handler);
  }

  /** The current connection status. */
  getStatus(): ClientStatus {
    return this.status;
  }

  /** The latest authoritative server tick applied. */
  getServerTick(): number {
    return this.latestServerTick;
  }

  /** The server-assigned client id (0 until the `Welcome` arrives). */
  getClientId(): number {
    return this.clientId;
  }

  /** The newest client sequence the server has acknowledged via a snapshot. */
  getLastAckedSequence(): number {
    return this.lastAckedClientSequence;
  }

  /** How many sent intents are still unacknowledged. */
  getPendingIntentCount(): number {
    return this.pending.length;
  }

  private handleInbound(bytes: Uint8Array): void {
    const message = decodeFrame(bytes);
    switch (message.kind) {
      case KIND_WELCOME:
        // Ignore a duplicate Welcome (the unreliable path may resend JoinRoom and
        // get welcomed more than once).
        if (this.status === "connected") return;
        this.latestServerTick = message.serverTick;
        this.clientId = message.clientId;
        this.clearJoinTimer();
        this.setStatus("connected");
        return;
      case KIND_SERVER_SNAPSHOT:
        this.applySnapshot(message);
        return;
      case KIND_SERVER_EVENT:
        this.eventHandlers.forEach((h) => h(message));
        return;
      case KIND_REJECTED_INTENT:
        this.applyRejection(message);
        return;
      default:
        // JoinRoom / LeaveRoom / ClientIntent are client→server; a server should
        // never send them. Ignore rather than crash the client.
        return;
    }
  }

  private applySnapshot(snapshot: ServerSnapshotMessage): void {
    // Reject an older snapshot (an equal tick is allowed and idempotent).
    if (snapshot.serverTick < this.latestServerTick) return;
    this.latestServerTick = snapshot.serverTick;
    this.lastAckedClientSequence = snapshot.lastAcceptedClientSequence;
    this.pending = this.pending.filter((seq) => seq > snapshot.lastAcceptedClientSequence);
    this.snapshotHandlers.forEach((h) => h(snapshot));
  }

  private applyRejection(rejection: RejectedIntentMessage): void {
    this.pending = this.pending.filter((seq) => seq !== rejection.clientSequence);
  }

  private setStatus(status: ClientStatus): void {
    this.status = status;
    this.statusHandlers.forEach((h) => h(status));
  }
}
