/*
 * Transport wiring for connect(). This is the platform edge: it constructs the
 * concrete browser transports (including the default `new WebSocket(...)`), which
 * are browser-only and so coverage-exempt and lint-relaxed (branch ban, no-unsafe)
 * — exactly as the Rust spine scopes its host/windowing platform layers out. The
 * spine (client state machine, codec) stays fully branchless and covered; only
 * this browser-construction wiring lives here.
 */

import { type SocketLike, type Transport, type TransportKind, WebSocketTransport } from "./transport.ts";
import type { ConnectConfig } from "./client-config.ts";
import { WebRtcTransport } from "./webrtc.ts";
import { WebTransportTransport } from "./webtransport.ts";

const DEFAULT_TRANSPORT: TransportKind = "websocket";

const browserSocket = (url: string): SocketLike => new WebSocket(url) as unknown as SocketLike;

const builders: Record<TransportKind, (config: ConnectConfig) => Transport> = {
  webrtc: (config): Transport => new WebRtcTransport(config.signalingUrl ?? config.url),
  websocket: (config): Transport => {
    const factory = config.socketFactory ?? browserSocket;
    return new WebSocketTransport((): SocketLike => factory(config.url));
  },
  webtransport: (config): Transport =>
    new WebTransportTransport(config.url, config.serverCertificateHash),
};

/** Build the transport for a connect config (override > kind > default). */
export const buildTransport = (config: ConnectConfig): Transport => {
  const override = config.transportFactory;
  if (override) {
    return override(config);
  }
  return builders[config.transport ?? DEFAULT_TRANSPORT](config);
};
