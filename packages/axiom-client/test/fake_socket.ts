// A tiny in-memory WebSocket stand-in for tests: it records what the client
// sends and lets a test drive the open/message/close lifecycle by hand. This is
// how the client tests run with no real server and no real browser.

import type { WebSocketLike } from "../src/transport.ts";

export class FakeSocket implements WebSocketLike {
  binaryType = "blob";
  readonly sent: Uint8Array[] = [];
  closed = false;
  onopen: ((this: unknown, ev: unknown) => unknown) | null = null;
  onmessage: ((this: unknown, ev: { data: unknown }) => unknown) | null = null;
  onclose: ((this: unknown, ev: unknown) => unknown) | null = null;
  onerror: ((this: unknown, ev: unknown) => unknown) | null = null;
  readonly url: string;

  constructor(url: string) {
    this.url = url;
  }

  send(data: Uint8Array): void {
    this.sent.push(data);
  }

  close(): void {
    this.closed = true;
    this.onclose?.call(this, {});
  }

  // --- test drivers ---

  open(): void {
    this.onopen?.call(this, {});
  }

  receive(bytes: Uint8Array): void {
    // Mirror a browser socket with binaryType="arraybuffer": event.data is an
    // ArrayBuffer.
    const buffer = bytes.slice().buffer;
    this.onmessage?.call(this, { data: buffer });
  }
}
