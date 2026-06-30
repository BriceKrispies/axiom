/*
 * DatagramTransport — a reusable UNRELIABLE (datagram / lossy) byte-frame transport.
 *
 * The reliable default is `WebSocketTransport`; this is its unreliable twin, for
 * carriers that may drop and reorder frames (WebRTC unreliable data channels,
 * WebTransport datagrams, a native UDP bridge). It declares `reliable = false`, so
 * the client compensates exactly as it already does for `WebRtcTransport`: it
 * resends `JoinRoom` until welcomed, and applies snapshots newest-wins (the
 * per-player wire carries the `server_tick` / sequence the receiver orders on, so
 * a stale or duplicated datagram is dropped and the newest wins).
 *
 * Like `WebSocketTransport` takes a socket *factory*, this takes a `DatagramLink`
 * *factory*: the transport logic (send/close routing, the unreliable flag, the
 * null-object pre-open link) is dependency-free and lives here, fully covered; the
 * concrete browser binding of a real datagram link is the platform edge
 * (constructed in `build-transport.ts` / `webrtc.ts`), coverage-exempt and verified
 * via the Playwright path. The same split the Rust spine draws at `host`/`windowing`.
 */

import type { Transport, TransportHandlers } from "./transport.ts";

/** A minimal unreliable duplex byte link the {@link DatagramTransport} drives. */
export interface DatagramLink {
  readonly send: (data: Uint8Array) => void;
  readonly close: () => void;
}

/**
 * Opens a datagram link, wiring its lifecycle/inbound datagrams to `handlers`
 * (`onOpen` when ready, `onMessage` per inbound datagram, `onClose` on close/error).
 */
export type DatagramLinkFactory = (handlers: TransportHandlers) => DatagramLink;

/*
 * A null-object link: the field is always a valid DatagramLink, so send/close need
 * no presence check (no branch) before the real link is opened.
 */
const NULL_DATAGRAM_LINK: DatagramLink = {
  close: (): void => {
    /* No link yet. */
  },
  send: (): void => {
    /* No link yet. */
  },
};

/** An unreliable (datagram) transport, selectable via config or `transportFactory`. */
export class DatagramTransport implements Transport {
  public readonly reliable = false;
  private link: DatagramLink = NULL_DATAGRAM_LINK;
  private readonly factory: DatagramLinkFactory;

  public constructor(factory: DatagramLinkFactory) {
    this.factory = factory;
  }

  public open(handlers: TransportHandlers): void {
    this.link = this.factory(handlers);
  }

  public send(data: Uint8Array): void {
    this.link.send(data);
  }

  public close(): void {
    this.link.close();
  }
}
