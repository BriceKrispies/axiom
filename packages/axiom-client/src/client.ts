/*
 * AxiomClient — the browser authoring SDK.
 *
 * Browser glue and author ergonomics, NOT engine truth: the server stays
 * authoritative; this client sends *intents* and applies *snapshots*. It mirrors
 * the portable `axiom-client-core` Rust module (connection state, a monotonic
 * client sequence, a pending-intent queue, the latest server tick).
 *
 * The state machine is branchless: status guards and the inbound dispatch select
 * actions through table lookup (`pick`) and a per-kind handler record, and absent
 * collaborators use null-objects/defaults instead of presence checks. Browser
 * transport wiring lives at the platform edge (build-transport.ts).
 *
 * Deliberately absent: prediction, rollback, reconnect, compression.
 */

import {
  type ClientStatus,
  type ConnectConfig,
  type EventHandler,
  type RejectionHandler,
  STATUS_CONNECTED,
  STATUS_CONNECTING,
  STATUS_DISCONNECTED,
  type SnapshotHandler,
  type StatusHandler,
} from "./client-config.ts";
import {
  type DecodedKind,
  type DecodedMessage,
  KIND_CLIENT_INTENT,
  KIND_JOIN_ROOM,
  KIND_LEAVE_ROOM,
  KIND_REJECTED_INTENT,
  KIND_SERVER_EVENT,
  KIND_SERVER_SNAPSHOT,
  KIND_WELCOME,
  type ServerSnapshotMessage,
  type WelcomeMessage,
} from "./messages.ts";
import { NULL_TRANSPORT, type Transport } from "./transport.ts";
import { decodeFrame, encodeClientIntent, encodeJoinRoom, encodeLeaveRoom } from "./codec.ts";
import { each, pick } from "./control-flow.ts";
import { assert } from "./protocol-error.ts";
import { buildTransport } from "./build-transport.ts";
import { toBytes } from "./text.ts";

export type { ClientStatus, ConnectConfig } from "./client-config.ts";

const DEFAULT_PROTOCOL_VERSION = 1;
const FIRST_SEQUENCE = 1;
const SEQUENCE_STEP = 1;
const ZERO = 0;
const NOT_SENT = 0;
const JOIN_RESEND_MS = 250;

const NOOP = (): void => {
  /* Intentionally empty. */
};

/**
 * A connected (or connecting) browser client. Construct one, register handlers,
 * then {@link connect}. The same instance can be reused after {@link disconnect}.
 */
export class AxiomClient {
  private transport: Transport = NULL_TRANSPORT;
  private status: ClientStatus = STATUS_DISCONNECTED;
  private protocolVersion = DEFAULT_PROTOCOL_VERSION;
  private roomId: Uint8Array = new Uint8Array();
  private nextClientSequence = FIRST_SEQUENCE;
  private latestServerTick = ZERO;
  private lastAckedClientSequence = ZERO;
  private clientId = ZERO;
  private pending: number[] = [];
  private joinFrame: Uint8Array = new Uint8Array();
  private readonly joinTimers: ReturnType<typeof setInterval>[] = [];
  private readonly snapshotHandlers: SnapshotHandler[] = [];
  private readonly eventHandlers: EventHandler[] = [];
  private readonly statusHandlers: StatusHandler[] = [];
  private readonly rejectionHandlers: RejectionHandler[] = [];

  /** Open a connection and join the room once the transport opens. */
  public connect(config: ConnectConfig): void {
    /*
     * Destructuring defaults supply the optional fields branchlessly (they apply
     * exactly when the field is `undefined`) — no `??`/`?:` under the Branchless Law.
     */
    const { protocolVersion = DEFAULT_PROTOCOL_VERSION, token = new Uint8Array() } = config;
    this.protocolVersion = protocolVersion;
    this.roomId = toBytes(config.roomId);
    const tokenBytes = toBytes(token);
    this.joinFrame = encodeJoinRoom(this.protocolVersion, this.roomId, tokenBytes);
    const transport = buildTransport(config);
    this.transport = transport;
    this.setStatus(STATUS_CONNECTING);
    transport.open({
      onClose: (): void => {
        this.handleClose();
      },
      onMessage: (bytes): void => {
        this.handleInbound(bytes);
      },
      onOpen: (): void => {
        this.handleOpen(transport);
      },
    });
  }

  /** Send LeaveRoom (when connected) and close the transport. */
  public disconnect(): void {
    const { transport } = this;
    this.clearJoinTimer();
    const sendLeave = (): void => {
      transport.send(encodeLeaveRoom(this.roomId));
    };
    pick([NOOP, sendLeave], Number(this.status === STATUS_CONNECTED))();
    transport.close();
  }

  /**
   * Send a client intent. Succeeds only while connected (server authority): it
   * assigns the next monotonic client sequence, records it pending, and sends an
   * encoded `ClientIntent`. Returns the assigned sequence (>= 1), or `0` if not
   * sent (not connected).
   */
  public sendIntent(payload: Uint8Array): number {
    const sequence = this.nextClientSequence;
    const commit = (): number => {
      this.nextClientSequence += SEQUENCE_STEP;
      this.pending.push(sequence);
      this.transport.send(
        encodeClientIntent({
          clientSequence: sequence,
          lastSeenServerTick: this.latestServerTick,
          payload,
          predictedClientTick: this.latestServerTick,
        }),
      );
      return sequence;
    };
    const skip = (): number => NOT_SENT;
    return pick([skip, commit], Number(this.status === STATUS_CONNECTED))();
  }

  /** Register a handler for authoritative snapshots. */
  public onSnapshot(handler: SnapshotHandler): void {
    this.snapshotHandlers.push(handler);
  }

  /** Register a handler for authoritative server events. */
  public onEvent(handler: EventHandler): void {
    this.eventHandlers.push(handler);
  }

  /** Register a handler for connection-status changes. */
  public onStatus(handler: StatusHandler): void {
    this.statusHandlers.push(handler);
  }

  /** Register a handler for a server-rejected intent (the authority's reason code). */
  public onRejected(handler: RejectionHandler): void {
    this.rejectionHandlers.push(handler);
  }

  /** The current connection status. */
  public getStatus(): ClientStatus {
    return this.status;
  }

  /** The latest authoritative server tick applied. */
  public getServerTick(): number {
    return this.latestServerTick;
  }

  /** The server-assigned client id (0 until the `Welcome` arrives). */
  public getClientId(): number {
    return this.clientId;
  }

  /** The newest client sequence the server has acknowledged via a snapshot. */
  public getLastAckedSequence(): number {
    return this.lastAckedClientSequence;
  }

  /** How many sent intents are still unacknowledged. */
  public getPendingIntentCount(): number {
    return this.pending.length;
  }

  private handleOpen(transport: Transport): void {
    transport.send(this.joinFrame);
    const startResend = (): void => {
      this.clearJoinTimer();
      this.joinTimers.push(
        setInterval((): void => {
          this.resendTick(transport);
        }, JOIN_RESEND_MS),
      );
    };
    // Resend JoinRoom over an unreliable transport until welcomed.
    pick([startResend, NOOP], Number(transport.reliable))();
  }

  private resendTick(transport: Transport): void {
    /*
     * The resend timer only ticks while connecting: becoming connected (or
     * closing) clears it synchronously, so there is no "stop" arm to guard here.
     */
    transport.send(this.joinFrame);
  }

  private handleClose(): void {
    this.transport = NULL_TRANSPORT;
    this.clearJoinTimer();
    this.setStatus(STATUS_DISCONNECTED);
  }

  private clearJoinTimer(): void {
    each(this.joinTimers.splice(ZERO), (timer): void => {
      clearInterval(timer);
    });
  }

  private handleInbound(bytes: Uint8Array): void {
    const message = decodeFrame(bytes);
    const handlers: Readonly<Record<DecodedKind, (decoded: DecodedMessage) => void>> = {
      [KIND_JOIN_ROOM]: NOOP,
      [KIND_LEAVE_ROOM]: NOOP,
      [KIND_CLIENT_INTENT]: NOOP,
      [KIND_WELCOME]: (decoded): void => {
        this.onWelcome(decoded);
      },
      [KIND_SERVER_SNAPSHOT]: (decoded): void => {
        this.onSnapshotMessage(decoded);
      },
      [KIND_SERVER_EVENT]: (decoded): void => {
        this.onEventMessage(decoded);
      },
      [KIND_REJECTED_INTENT]: (decoded): void => {
        this.onRejection(decoded);
      },
    };
    handlers[message.kind](message);
  }

  private onWelcome(message: DecodedMessage): void {
    assert(message.kind === KIND_WELCOME, "dispatch guarantees a welcome message");
    // Ignore a duplicate Welcome (the unreliable path may be welcomed twice).
    const accept = (): void => {
      this.acceptWelcome(message);
    };
    pick([NOOP, accept], Number(this.status !== STATUS_CONNECTED))();
  }

  private acceptWelcome(message: WelcomeMessage): void {
    this.latestServerTick = message.serverTick;
    this.clientId = message.clientId;
    this.clearJoinTimer();
    this.setStatus(STATUS_CONNECTED);
  }

  private onSnapshotMessage(message: DecodedMessage): void {
    assert(message.kind === KIND_SERVER_SNAPSHOT, "dispatch guarantees a snapshot");
    this.applySnapshot(message);
  }

  private applySnapshot(snapshot: ServerSnapshotMessage): void {
    // Reject an older snapshot (an equal tick is allowed and idempotent).
    const accept = (): void => {
      this.acceptSnapshot(snapshot);
    };
    pick([NOOP, accept], Number(snapshot.serverTick >= this.latestServerTick))();
  }

  private acceptSnapshot(snapshot: ServerSnapshotMessage): void {
    this.latestServerTick = snapshot.serverTick;
    this.lastAckedClientSequence = snapshot.lastAcceptedClientSequence;
    this.pending = this.pending.filter((seq): boolean => seq > snapshot.lastAcceptedClientSequence);
    each(this.snapshotHandlers, (handler): void => { handler(snapshot); });
  }

  private onEventMessage(message: DecodedMessage): void {
    assert(message.kind === KIND_SERVER_EVENT, "dispatch guarantees an event");
    each(this.eventHandlers, (handler): void => { handler(message); });
  }

  private onRejection(message: DecodedMessage): void {
    assert(message.kind === KIND_REJECTED_INTENT, "dispatch guarantees a rejection");
    this.pending = this.pending.filter((seq): boolean => seq !== message.clientSequence);
    each(this.rejectionHandlers, (handler): void => { handler(message.reasonCode); });
  }

  private setStatus(status: ClientStatus): void {
    this.status = status;
    each(this.statusHandlers, (handler): void => { handler(status); });
  }
}
