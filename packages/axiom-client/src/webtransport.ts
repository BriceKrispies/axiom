/*
 * WebTransport over a reliable, ordered HTTP/3 (QUIC) bidirectional stream.
 *
 * This is the platform edge: it binds the browser's WebTransport API and its
 * async stream control flow, so a documented subset of rules (the branch ban,
 * async-await, await-in-loop, no-unsafe-*) is scoped off here and it is
 * coverage-exempt (browser-only; verified via the Playwright path) — exactly as
 * the Rust spine scopes its host/windowing platform layers out. Frames are
 * carried length-prefixed (a stream is a byte stream, not message-framed).
 *
 * `serverCertificateHash` is the sha-256 of the server's DER certificate, letting
 * the browser trust a self-signed dev cert. Omit it for a CA-trusted server.
 */

import type { Transport, TransportHandlers } from "./transport.ts";

const FRAME_HEADER_SIZE = 4;
const AT_START = 0;
const LITTLE_ENDIAN = true;

const concat = (left: Uint8Array, right: Uint8Array): Uint8Array => {
  const out = new Uint8Array(left.length + right.length);
  out.set(left, AT_START);
  out.set(right, left.length);
  return out;
};

// Emit every complete length-prefixed frame in `buffer`; return the leftover.
const drainFrames = (buffer: Uint8Array, handlers: TransportHandlers): Uint8Array => {
  let rest = buffer;
  for (;;) {
    if (rest.length < FRAME_HEADER_SIZE) {
      break;
    }
    const length = new DataView(rest.buffer, rest.byteOffset, rest.byteLength).getUint32(
      AT_START,
      LITTLE_ENDIAN,
    );
    if (rest.length < FRAME_HEADER_SIZE + length) {
      break;
    }
    handlers.onMessage(rest.slice(FRAME_HEADER_SIZE, FRAME_HEADER_SIZE + length));
    rest = rest.slice(FRAME_HEADER_SIZE + length);
  }
  return rest;
};

const readLoop = async (
  reader: ReadableStreamDefaultReader<Uint8Array>,
  handlers: TransportHandlers,
): Promise<void> => {
  let buffer: Uint8Array = new Uint8Array(AT_START);
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      buffer = drainFrames(concat(buffer, value), handlers);
    }
  } catch {
    /* Reader closed. */
  }
  handlers.onClose();
};

/** WebTransport (HTTP/3 / QUIC) transport, selectable by config. */
export class WebTransportTransport implements Transport {
  public readonly reliable = true;
  private transport?: WebTransport;
  private writer?: WritableStreamDefaultWriter<Uint8Array>;
  private readonly url: string;
  private readonly serverCertificateHash?: Uint8Array;

  public constructor(url: string, serverCertificateHash?: Uint8Array) {
    this.url = url;
    this.serverCertificateHash = serverCertificateHash;
  }

  public open(handlers: TransportHandlers): void {
    const options: WebTransportOptions = {};
    if (this.serverCertificateHash) {
      const value = new Uint8Array(this.serverCertificateHash);
      options.serverCertificateHashes = [{ algorithm: "sha-256", value }];
    }
    const transport = new WebTransport(this.url, options);
    this.transport = transport;
    transport.ready
      .then(async (): Promise<void> => {
        const stream = await transport.createBidirectionalStream();
        this.writer = stream.writable.getWriter();
        handlers.onOpen();
        await readLoop(stream.readable.getReader(), handlers);
      })
      .catch((): void => {
        handlers.onClose();
      });
    transport.closed.then(
      (): void => {
        handlers.onClose();
      },
      (): void => {
        handlers.onClose();
      },
    );
  }

  public send(data: Uint8Array): void {
    const framed = new Uint8Array(FRAME_HEADER_SIZE + data.length);
    new DataView(framed.buffer).setUint32(AT_START, data.length, LITTLE_ENDIAN);
    framed.set(data, FRAME_HEADER_SIZE);
    this.writer?.write(framed).catch((): void => {
      /* Write after close: dropped. */
    });
  }

  public close(): void {
    try {
      this.transport?.close();
    } catch {
      /* Already closing. */
    }
  }
}
