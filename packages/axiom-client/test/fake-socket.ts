// A tiny in-memory WebSocket stand-in for tests: it records what the client
// sends and lets a test drive the open/message/close/error lifecycle by hand.

import type { SocketLike } from "../src/transport.ts";

type Listener = (event: { data: unknown }) => void;

export class FakeSocket implements SocketLike {
  public binaryType = "blob";
  public readonly sent: Uint8Array[] = [];
  public closed = false;
  public readonly url: string;
  private readonly listeners = new Map<string, Listener[]>();

  public constructor(url: string) {
    this.url = url;
  }

  public addEventListener(type: string, listener: Listener): void {
    const list = this.listeners.get(type) ?? [];
    list.push(listener);
    this.listeners.set(type, list);
  }

  public send(data: Uint8Array): void {
    this.sent.push(data);
  }

  public close(): void {
    this.closed = true;
    this.dispatch("close");
  }

  // --- test drivers ---

  public open(): void {
    this.dispatch("open");
  }

  public receive(bytes: Uint8Array): void {
    // Mirror a browser socket with binaryType="arraybuffer": event.data is an
    // ArrayBuffer.
    this.dispatch("message", bytes.slice().buffer);
  }

  public fail(): void {
    this.dispatch("error");
  }

  private dispatch(type: string, data?: unknown): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener({ data });
    }
  }
}
