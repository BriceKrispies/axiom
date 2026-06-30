// A tiny in-memory unreliable datagram link for tests: it records what the client
// sends and lets a test drive the open/inbound/close lifecycle by hand — and, by
// delivering frames in any order (or not at all), simulate drop and reorder.

import type { DatagramLink, DatagramLinkFactory } from "./datagram-transport.ts";
import type { TransportHandlers } from "./transport.ts";

const NOOP = (): void => {
  /* before the factory is invoked */
};
const DETACHED: TransportHandlers = { onClose: NOOP, onMessage: NOOP, onOpen: NOOP };

export class FakeDatagramLink {
  public readonly sent: Uint8Array[] = [];
  public closed = false;
  private handlers: TransportHandlers = DETACHED;

  /** Pass `link.factory` to `new DatagramTransport(...)`. */
  public readonly factory: DatagramLinkFactory = (handlers): DatagramLink => {
    this.handlers = handlers;
    return {
      close: (): void => {
        this.closed = true;
      },
      send: (data): void => {
        this.sent.push(data);
      },
    };
  };

  /** Drive the link open (the transport's `onOpen`). */
  public open(): void {
    this.handlers.onOpen();
  }

  /** Deliver one inbound datagram (the transport's `onMessage`). */
  public deliver(bytes: Uint8Array): void {
    this.handlers.onMessage(bytes);
  }

  /** Drive a link close/error (the transport's `onClose`). */
  public fail(): void {
    this.handlers.onClose();
  }
}
