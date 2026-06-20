/*
 * Pluggable byte-frame transports. The client speaks plain byte frames; how those
 * bytes travel is a config choice. This file owns the transport contract, the
 * inbound-data coercion, and the default reliable transport (WebSocket). The
 * unreliable browser transports live at the platform edge (webtransport.ts,
 * webrtc.ts).
 */

import { assert } from "./protocol-error.ts";

/** Which transport to use. */
export type TransportKind = "websocket" | "webtransport" | "webrtc";

/** Callbacks a transport drives as its connection lives and carries bytes. */
export interface TransportHandlers {
  readonly onOpen: () => void;
  readonly onMessage: (data: Uint8Array) => void;
  readonly onClose: () => void;
}

/** A byte-frame transport. */
export interface Transport {
  /**
   * Whether delivery is reliable + ordered. WebSocket is; WebRTC datagrams are
   * not, so the client compensates (JoinRoom resend, newer-wins snapshots).
   */
  readonly reliable: boolean;
  readonly open: (handlers: TransportHandlers) => void;
  readonly send: (data: Uint8Array) => void;
  readonly close: () => void;
}

/** Events the client listens for on a socket. */
interface SocketEventMap {
  readonly open: unknown;
  readonly message: { readonly data: unknown };
  readonly close: unknown;
  readonly error: unknown;
}

/** The minimal slice of the browser `WebSocket` interface this client uses. */
export interface SocketLike {
  binaryType: string;
  readonly addEventListener: <Name extends keyof SocketEventMap>(
    type: Name,
    listener: (event: SocketEventMap[Name]) => void,
  ) => void;
  readonly send: (data: Uint8Array) => void;
  readonly close: () => void;
}

const isBinary = (data: unknown): data is ArrayBuffer | Uint8Array =>
  [Uint8Array, ArrayBuffer].some((constructor): boolean => data instanceof constructor);

/** Coerce inbound socket data (ArrayBuffer or Uint8Array) to bytes. */
export const asUint8Array = (data: unknown): Uint8Array => {
  assert(isBinary(data), "expected binary message data (ArrayBuffer or Uint8Array)");
  return new Uint8Array(data);
};

/** The post-open operations the transport keeps a handle to after `open`. */
interface SocketSink {
  readonly send: (data: Uint8Array) => void;
  readonly close: () => void;
}

/*
 * A null-object sink: the field is always a valid SocketSink, so send/close need
 * no presence check (no branch) before the real socket is opened. Only send/close
 * are reachable before open, so the null object carries exactly those.
 */
const NULL_SINK: SocketSink = {
  close: (): void => {
    /* No socket yet. */
  },
  send: (): void => {
    /* No socket yet. */
  },
};

/*
 * A null-object transport: a client's transport field is always a valid Transport,
 * so the state machine needs no presence check before connect (or after close).
 */
export const NULL_TRANSPORT: Transport = {
  close: (): void => {
    /* No transport yet. */
  },
  open: (): void => {
    /* No transport yet. */
  },
  reliable: true,
  send: (): void => {
    /* No transport yet. */
  },
};

/** Reliable, ordered transport over a `WebSocket` (or an injected fake). */
export class WebSocketTransport implements Transport {
  public readonly reliable = true;
  private sink: SocketSink = NULL_SINK;
  private readonly factory: () => SocketLike;

  public constructor(factory: () => SocketLike) {
    this.factory = factory;
  }

  public open(handlers: TransportHandlers): void {
    const socket = this.factory();
    socket.binaryType = "arraybuffer";
    socket.addEventListener("open", (): void => {
      handlers.onOpen();
    });
    socket.addEventListener("message", (event): void => {
      handlers.onMessage(asUint8Array(event.data));
    });
    socket.addEventListener("close", (): void => {
      handlers.onClose();
    });
    socket.addEventListener("error", (): void => {
      handlers.onClose();
    });
    this.sink = socket;
  }

  public send(data: Uint8Array): void {
    this.sink.send(data);
  }

  public close(): void {
    this.sink.close();
  }
}
