// Pluggable transports for the SDK. The client speaks plain byte frames; *how*
// those bytes travel is a config choice, swappable without touching the engine
// modules (which never learn the transport). Two are provided:
//
//   - WebSocketTransport  — reliable, ordered, over TCP. The default.
//   - WebTransportTransport — HTTP/3 over QUIC datagrams: unreliable + unordered
//     (the "UDP in the browser" path). The client resends JoinRoom until welcomed
//     and treats snapshots as newer-wins, so loss is tolerated.

/** Which transport to use. */
export type TransportKind = "websocket" | "webtransport" | "webrtc";

/** Callbacks a transport drives as its connection lives and carries bytes. */
export interface TransportHandlers {
  onOpen: () => void;
  onMessage: (data: Uint8Array) => void;
  onClose: () => void;
}

/** A byte-frame transport. */
export interface Transport {
  /**
   * Whether delivery is reliable + ordered. WebSocket is; WebTransport datagrams
   * are not, so the client compensates (JoinRoom resend, newer-wins snapshots).
   */
  readonly reliable: boolean;
  open(handlers: TransportHandlers): void;
  send(data: Uint8Array): void;
  close(): void;
}

/** The minimal slice of the browser `WebSocket` interface this client uses. */
export interface WebSocketLike {
  binaryType: string;
  send(data: Uint8Array): void;
  close(): void;
  onopen: ((this: unknown, ev: unknown) => unknown) | null;
  onmessage: ((this: unknown, ev: { data: unknown }) => unknown) | null;
  onclose: ((this: unknown, ev: unknown) => unknown) | null;
  onerror: ((this: unknown, ev: unknown) => unknown) | null;
}

/** Coerce inbound socket data (ArrayBuffer or Uint8Array) to bytes. */
export function asUint8Array(data: unknown): Uint8Array {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  throw new TypeError("expected binary message data (ArrayBuffer or Uint8Array)");
}

/** Reliable, ordered transport over a `WebSocket` (or an injected fake). */
export class WebSocketTransport implements Transport {
  readonly reliable = true;
  private socket: WebSocketLike | null = null;
  private readonly factory: () => WebSocketLike;

  constructor(factory: () => WebSocketLike) {
    this.factory = factory;
  }

  open(handlers: TransportHandlers): void {
    const socket = this.factory();
    socket.binaryType = "arraybuffer";
    this.socket = socket;
    socket.onopen = () => handlers.onOpen();
    socket.onmessage = (ev) => handlers.onMessage(asUint8Array(ev.data));
    socket.onclose = () => handlers.onClose();
    socket.onerror = () => handlers.onClose();
  }

  send(data: Uint8Array): void {
    this.socket?.send(data);
  }

  close(): void {
    this.socket?.close();
  }
}

/**
 * WebTransport over a reliable, ordered HTTP/3 (QUIC) bidirectional stream.
 *
 * .NET's public WebTransport API exposes streams (not datagrams), so frames are
 * carried length-prefixed (a stream is a byte stream, not message-framed). This
 * gives a real HTTP/3/QUIC transport selectable by config — reliable, so no
 * JoinRoom resend is needed. (True UDP-like datagrams await a datagram-capable
 * server.)
 *
 * `serverCertificateHash` is the sha-256 of the server's DER certificate, letting
 * the browser trust a self-signed dev cert (the .NET example exposes it at
 * `/cert-hash`). Omit it for a CA-trusted server.
 */
export class WebTransportTransport implements Transport {
  readonly reliable = true;
  private transport: WebTransport | null = null;
  private writer: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private readonly url: string;
  private readonly serverCertificateHash?: Uint8Array;

  constructor(url: string, serverCertificateHash?: Uint8Array) {
    this.url = url;
    this.serverCertificateHash = serverCertificateHash;
  }

  open(handlers: TransportHandlers): void {
    const options: WebTransportOptions = {};
    if (this.serverCertificateHash) {
      // Copy into a fresh ArrayBuffer-backed view (BufferSource wants ArrayBuffer).
      const value = new Uint8Array(this.serverCertificateHash);
      options.serverCertificateHashes = [{ algorithm: "sha-256", value }];
    }
    const transport = new WebTransport(this.url, options);
    this.transport = transport;
    transport.ready
      .then(async () => {
        const stream = await transport.createBidirectionalStream();
        this.writer = stream.writable.getWriter();
        handlers.onOpen();
        void this.readLoop(stream.readable.getReader(), handlers);
      })
      .catch(() => handlers.onClose());
    transport.closed.then(() => handlers.onClose(), () => handlers.onClose());
  }

  private async readLoop(
    reader: ReadableStreamDefaultReader<Uint8Array>,
    handlers: TransportHandlers,
  ): Promise<void> {
    let buffer: Uint8Array = new Uint8Array(0);
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        if (!value) continue;
        buffer = concat(buffer, value);
        // Extract every complete u32-LE length-prefixed frame.
        for (;;) {
          if (buffer.length < 4) break;
          const view = new DataView(buffer.buffer, buffer.byteOffset, buffer.byteLength);
          const len = view.getUint32(0, true);
          if (buffer.length < 4 + len) break;
          handlers.onMessage(buffer.slice(4, 4 + len));
          buffer = buffer.slice(4 + len);
        }
      }
    } catch {
      /* reader closed */
    }
    handlers.onClose();
  }

  send(data: Uint8Array): void {
    const framed = new Uint8Array(4 + data.length);
    new DataView(framed.buffer).setUint32(0, data.length, true);
    framed.set(data, 4);
    void this.writer?.write(framed).catch(() => {});
  }

  close(): void {
    try {
      this.transport?.close();
    } catch {
      /* already closing */
    }
  }
}

/**
 * WebRTC DataChannel transport — true unreliable, unordered UDP in the browser
 * (`{ordered:false, maxRetransmits:0}`). WebRTC negotiates a direct UDP socket
 * between the peers (over loopback when same-host), so unlike WebTransport it
 * needs no HTTP/3, no QUIC OS support, and no cert (DTLS is self-negotiated).
 *
 * Signaling is a single HTTP POST of the SDP offer to `signalingUrl`, returning
 * the answer (non-trickle ICE). Unreliable, so the client resends JoinRoom until
 * welcomed and treats snapshots as newer-wins.
 */
export class WebRtcTransport implements Transport {
  readonly reliable = false;
  private pc: RTCPeerConnection | null = null;
  private channel: RTCDataChannel | null = null;
  private readonly signalingUrl: string;

  constructor(signalingUrl: string) {
    this.signalingUrl = signalingUrl;
  }

  open(handlers: TransportHandlers): void {
    const pc = new RTCPeerConnection();
    this.pc = pc;
    const channel = pc.createDataChannel("game", { ordered: false, maxRetransmits: 0 });
    channel.binaryType = "arraybuffer";
    this.channel = channel;
    channel.onopen = () => handlers.onOpen();
    channel.onmessage = (e) => handlers.onMessage(new Uint8Array(e.data as ArrayBuffer));
    channel.onclose = () => handlers.onClose();
    pc.onconnectionstatechange = () => {
      const s = pc.connectionState;
      if (s === "failed" || s === "disconnected" || s === "closed") handlers.onClose();
    };
    void this.negotiate(pc, handlers);
  }

  private async negotiate(pc: RTCPeerConnection, handlers: TransportHandlers): Promise<void> {
    try {
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      await iceGatheringComplete(pc);
      const res = await fetch(this.signalingUrl, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ type: pc.localDescription!.type, sdp: pc.localDescription!.sdp }),
      });
      const answer = await res.json();
      await pc.setRemoteDescription(answer);
    } catch {
      handlers.onClose();
    }
  }

  send(data: Uint8Array): void {
    // Send an ArrayBuffer-backed copy (DOM's send wants ArrayBufferView<ArrayBuffer>).
    if (this.channel && this.channel.readyState === "open") this.channel.send(new Uint8Array(data));
  }

  close(): void {
    try {
      this.pc?.close();
    } catch {
      /* already closing */
    }
  }
}

function iceGatheringComplete(pc: RTCPeerConnection): Promise<void> {
  if (pc.iceGatheringState === "complete") return Promise.resolve();
  return new Promise((resolve) => {
    const check = () => {
      if (pc.iceGatheringState === "complete") {
        pc.removeEventListener("icegatheringstatechange", check);
        resolve();
      }
    };
    pc.addEventListener("icegatheringstatechange", check);
  });
}

function concat(a: Uint8Array, b: Uint8Array): Uint8Array {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
}
