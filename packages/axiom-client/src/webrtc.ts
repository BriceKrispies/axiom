/*
 * WebRTC DataChannel transport — true unreliable, unordered "UDP in the browser"
 * (`{ordered:false, maxRetransmits:0}`). WebRTC negotiates a direct UDP socket
 * between peers, so unlike WebTransport it needs no HTTP/3 and no cert.
 *
 * This is the platform edge: it binds the browser's RTCPeerConnection API and its
 * async negotiation control flow, so a documented subset of rules (the branch
 * ban, async-await, no-unsafe-*) is scoped off here and it is coverage-exempt
 * (browser-only; verified via the Playwright path) — exactly as the Rust spine
 * scopes its host/windowing platform layers out.
 *
 * Signaling is a single HTTP POST of the SDP offer to `signalingUrl`, returning
 * the answer (non-trickle ICE). Unreliable, so the client resends JoinRoom.
 */

import { type Transport, type TransportHandlers, asUint8Array } from "./transport.ts";

const NO_RETRANSMITS = 0;
const DEAD_STATES: ReadonlySet<string> = new Set(["failed", "disconnected", "closed"]);

const iceGatheringComplete = async (peer: RTCPeerConnection): Promise<void> => {
  if (peer.iceGatheringState === "complete") {
    return;
  }
  await new Promise<void>((resolve): void => {
    const check = (): void => {
      if (peer.iceGatheringState === "complete") {
        peer.removeEventListener("icegatheringstatechange", check);
        resolve();
      }
    };
    peer.addEventListener("icegatheringstatechange", check);
  });
};

const negotiate = async (
  peer: RTCPeerConnection,
  signalingUrl: string,
  handlers: TransportHandlers,
): Promise<void> => {
  try {
    const offer = await peer.createOffer();
    await peer.setLocalDescription(offer);
    await iceGatheringComplete(peer);
    const response = await fetch(signalingUrl, {
      body: JSON.stringify({ sdp: peer.localDescription?.sdp, type: peer.localDescription?.type }),
      headers: { "content-type": "application/json" },
      method: "POST",
    });
    await peer.setRemoteDescription(await response.json());
  } catch {
    handlers.onClose();
  }
};

/** WebRTC DataChannel (unreliable, unordered) transport, selectable by config. */
export class WebRtcTransport implements Transport {
  public readonly reliable = false;
  private peer?: RTCPeerConnection;
  private channel?: RTCDataChannel;
  private readonly signalingUrl: string;

  public constructor(signalingUrl: string) {
    this.signalingUrl = signalingUrl;
  }

  public open(handlers: TransportHandlers): void {
    const peer = new RTCPeerConnection();
    this.peer = peer;
    const channel = peer.createDataChannel("game", { maxRetransmits: NO_RETRANSMITS, ordered: false });
    channel.binaryType = "arraybuffer";
    this.channel = channel;
    channel.addEventListener("open", (): void => {
      handlers.onOpen();
    });
    channel.addEventListener("message", (event): void => {
      handlers.onMessage(asUint8Array(event.data));
    });
    channel.addEventListener("close", (): void => {
      handlers.onClose();
    });
    peer.addEventListener("connectionstatechange", (): void => {
      if (DEAD_STATES.has(peer.connectionState)) {
        handlers.onClose();
      }
    });
    negotiate(peer, this.signalingUrl, handlers).catch((): void => {
      /* Negotiation handles its own failure. */
    });
  }

  public send(data: Uint8Array): void {
    const { channel } = this;
    if (channel?.readyState === "open") {
      channel.send(new Uint8Array(data));
    }
  }

  public close(): void {
    try {
      this.peer?.close();
    } catch {
      /* Already closing. */
    }
  }
}
