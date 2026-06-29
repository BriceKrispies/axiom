/*
 * The multiplayer authoring projection (SPEC-13 §4.2, the server-authoritative
 * §16 stack). It projects four shapes onto the same boundary pattern the rest of
 * the SDK uses:
 *   - `NetSim` — the SPEC-00 `Sim` widened with player addressing
 *     (`players`/`inputOf`/`joinedThisTick`/`leftThisTick`); built by `makeNetSim`
 *     over a `NetParticipants` seam the runtime feeds from the authority.
 *   - `Intent` — the author's one flat per-tick command record (§16.2); the engine
 *     owns the wire codec derived from it, so no hand-written twin lives here.
 *   - `joinRoom(JoinConfig) → NetClient` — the client connection surface (§16.4),
 *     a thin wrapper over a `NetTransport` seam. The runtime binds the real
 *     transport (over `@axiom/client`) once via `bindNetTransport`, exactly as it
 *     binds the native host via `bindNative`; tests bind a fake. The covered spine
 *     depends only on the seam, never on a live socket — so it stays fully
 *     branchless and 100% covered without `@axiom/client` or a browser.
 *   - `configureNet(NetConfig)` — prediction/interpolation are CONFIGURED, not
 *     authored (§16.5). Physics prediction defaults OFF: the default config
 *     predicts nothing and interpolates nothing — authority/non-physics state only.
 *     NOTE (2026-06-29): the cross-target `f32` determinism golden came back GREEN
 *     (commit 5980a0f), which is the §17.6 prerequisite local-player prediction was
 *     blocked on — so prediction is now UNBLOCKED for a future opt-in. It stays OFF
 *     by default here; turning it on is a deliberate later change (resimulation
 *     wiring + a reconciliation drift=0 proof), not flipped on in this surface.
 *   - `onSnapshot(() => Uint8Array)` / `onRestore(bytes => void)` — the extra
 *     authoritative-state hooks (§16.5): the author appends bytes the authority
 *     packs into each snapshot, and restores them on a client reconcile. Stored on
 *     the session; the runtime drains them via `boundNetSnapshot`/`boundNetRestore`.
 *
 * Everything is pure forwarding (no control flow): `NetClient`/`NetSim` re-expose
 * the seam's per-tick reads, and the inert pre-bind transport returns neutral
 * values so the surface is total before the runtime binds. The sibling lobby
 * surface (`hostRoom`/`matchmake`) lives in `net-room.ts`, and the authority-
 * snapshot participant decoder (`makeNetParticipants`) in `net-participants.ts`.
 */

import type { PlayerId, Result, Ticks } from "./vocabulary.ts";
import { type Sim, type SimContext, makeSim } from "./sim.ts";
import type { Input } from "./input.ts";

/** The connection status the authority (not the socket) decides (SPEC-13 §4.2). */
export type ConnStatus = "connected" | "connecting" | "disconnected";

/** One flat per-tick command record (SPEC-13 §4.2 / §16.2); the engine derives its wire codec. */
export type Intent = Record<string, boolean | number | string>;

/** The room-join configuration (SPEC-13 §4.2); `token` is an opaque JWT. */
export interface JoinConfig {
  /** The authority endpoint URL. */
  readonly url: string;
  /** The room to join. */
  readonly roomId: string;
  /** An opaque auth token forwarded to the authority. */
  readonly token?: string;
}

/** Prediction/interpolation configuration (SPEC-13 §4.2 / §16.5) — configured, never authored. */
export interface NetConfig {
  /** Predict the local player's authored sim ahead of the authority. */
  readonly predictLocalPlayer: boolean;
  /** Smooth remote entities between snapshots (presentation-only). */
  readonly interpolateRemote: boolean;
  /** Render-time interpolation delay in ticks. */
  readonly interpolationDelayTicks?: number;
}

/** The client handle `joinRoom` returns (SPEC-13 §4.2). */
export interface NetClient {
  /** The current connection status. */
  readonly status: () => ConnStatus;
  /** The local player's seat, or the empty value until admitted. */
  readonly localPlayer: () => Result<PlayerId>;
  /** Send the local player's intent for the running tick. */
  readonly sendIntent: (intent: Intent) => void;
  /** Observe connection status changes. */
  readonly onStatus: (callback: (status: ConnStatus) => void) => void;
  /** Observe a rejected join/intent, with the authority's reason. */
  readonly onRejected: (callback: (reason: string) => void) => void;
  /** Leave the room and close the connection. */
  readonly leave: () => void;
}

/*
 * The transport seam the runtime implements over `@axiom/client` (the netcode
 * SDK) and a test fakes. It is the SPEC-13 analogue of `NativeBridge`/`HostBridge`:
 * the covered spine projects `NetClient` over THIS, never over a live socket.
 */
export interface NetTransport {
  /** The current connection status. */
  readonly status: () => ConnStatus;
  /** The local player's seat, or the empty value until admitted. */
  readonly localPlayer: () => Result<PlayerId>;
  /** Encode and send the local player's intent. */
  readonly sendIntent: (intent: Intent) => void;
  /** Register a status-change observer. */
  readonly onStatus: (callback: (status: ConnStatus) => void) => void;
  /** Register a rejection observer. */
  readonly onRejected: (callback: (reason: string) => void) => void;
  /** Close the connection. */
  readonly leave: () => void;
}

/** Opens a transport for a `joinRoom` call — the runtime's `@axiom/client` binding point. */
export type NetTransportFactory = (config: JoinConfig) => NetTransport;

/*
 * The per-tick networked facts the authority feeds the loop. `inputOf` returns the
 * per-player `Input` over the running tick's intent snapshot; in `local` mode
 * `inputOf(localPlayer)` is the single-player `Sim.input`.
 */
export interface NetParticipants {
  /** The seated players this tick, in stable order. */
  readonly players: (tick: Ticks) => readonly PlayerId[];
  /** A player's per-tick input snapshot. */
  readonly inputOf: (player: PlayerId, tick: Ticks) => Input;
  /** The players that joined on this tick. */
  readonly joinedThisTick: (tick: Ticks) => readonly PlayerId[];
  /** The players that left on this tick. */
  readonly leftThisTick: (tick: Ticks) => readonly PlayerId[];
}

/** The SPEC-00 `Sim` widened by player addressing (SPEC-13 §4.2). */
export interface NetSim extends Sim {
  /** The seated players this tick, in stable order. */
  readonly players: () => readonly PlayerId[];
  /** A player's per-tick input (its authored intents). */
  readonly inputOf: (player: PlayerId) => Input;
  /** The players that joined this tick. */
  readonly joinedThisTick: () => readonly PlayerId[];
  /** The players that left this tick. */
  readonly leftThisTick: () => readonly PlayerId[];
}

/** Build the networked `Sim` for `tick` from the game's `context` and the authority's `participants`. */
export const makeNetSim = (
  context: SimContext,
  participants: NetParticipants,
  tick: Ticks,
): NetSim => {
  const base = makeSim(context, tick);
  return {
    add: base.add,
    dt: base.dt,
    input: base.input,
    inputOf: (player: PlayerId): Input => participants.inputOf(player, tick),
    joinedThisTick: (): readonly PlayerId[] => participants.joinedThisTick(tick),
    leftThisTick: (): readonly PlayerId[] => participants.leftThisTick(tick),
    physics: base.physics,
    players: (): readonly PlayerId[] => participants.players(tick),
    rng: base.rng,
    tick: base.tick,
    time: base.time,
    tweens: base.tweens,
    world: base.world,
  };
};

/** No seats — the empty list whose missing first element is the absent value (no banned `undefined` literal). */
const NO_SEATS: readonly PlayerId[] = [];

/** The empty seat an inert / un-admitted client reports (`undefined`, read as the missing first seat). */
const absentPlayer = (): Result<PlayerId> => NO_SEATS.at(0);

/*
 * The inert transport before `bindNetTransport`: disconnected, no seat, every
 * signal a no-op. Keeps `joinRoom` total before the runtime binds a real transport.
 */
const INERT_TRANSPORT: NetTransport = {
  leave: (): void => {
    // No-op until a transport is bound
  },
  localPlayer: (): Result<PlayerId> => absentPlayer(),
  onRejected: (): void => {
    // No-op until a transport is bound
  },
  onStatus: (): void => {
    // No-op until a transport is bound
  },
  sendIntent: (): void => {
    // No-op until a transport is bound
  },
  status: (): ConnStatus => "disconnected",
};

/** The inert factory before `bindNetTransport` — every join yields the inert transport. */
const INERT_FACTORY: NetTransportFactory = (): NetTransport => INERT_TRANSPORT;

/** The default network configuration — predict nothing, interpolate nothing (authority state only). */
const DEFAULT_NET_CONFIG: NetConfig = { interpolateRemote: false, predictLocalPlayer: false };

/** The empty author-snapshot bytes the default `onSnapshot` contributes (no extra author state). */
const NO_SNAPSHOT_BYTES = new Uint8Array();

/** The default author-snapshot hook — contributes no extra bytes until `onSnapshot` is set. */
const DEFAULT_SNAPSHOT: () => Uint8Array = (): Uint8Array => NO_SNAPSHOT_BYTES;

/** The default author-restore hook — a no-op until `onRestore` is set. */
const DEFAULT_RESTORE: (bytes: Uint8Array) => void = (): void => {
  // No author restore hook until onRestore is called
};

/** The mutable net session: the bound transport factory, the config, and the snapshot/restore hooks. */
const netSession: {
  factory: NetTransportFactory;
  config: NetConfig;
  snapshot: () => Uint8Array;
  restore: (bytes: Uint8Array) => void;
} = {
  config: DEFAULT_NET_CONFIG,
  factory: INERT_FACTORY,
  restore: DEFAULT_RESTORE,
  snapshot: DEFAULT_SNAPSHOT,
};

/** Install the runtime's real transport factory (its `@axiom/client` binding), once at boot. */
export const bindNetTransport = (factory: NetTransportFactory): void => {
  netSession.factory = factory;
};

/** Configure prediction/interpolation (SPEC-13 §4.2 / §16.5). */
export const configureNet = (config: NetConfig): void => {
  netSession.config = config;
};

/** The current network configuration (the default until `configureNet`). */
export const boundNetConfig = (): NetConfig => netSession.config;

/** Register the author's extra-state snapshot hook (SPEC-13 §16.5): bytes the authority appends per snapshot. */
export const onSnapshot = (callback: () => Uint8Array): void => {
  netSession.snapshot = callback;
};

/** Register the author's extra-state restore hook (SPEC-13 §16.5): applied on a client reconcile. */
export const onRestore = (callback: (bytes: Uint8Array) => void): void => {
  netSession.restore = callback;
};

/** Collect the author's extra snapshot bytes — the runtime appends these to the authoritative snapshot. */
export const boundNetSnapshot = (): Uint8Array => netSession.snapshot();

/** Deliver restored snapshot bytes to the author's hook — the runtime calls this on reconcile. */
export const boundNetRestore = (bytes: Uint8Array): void => {
  netSession.restore(bytes);
};

/** Join a room, returning a `NetClient` over the bound transport (SPEC-13 §4.2). */
export const joinRoom = (config: JoinConfig): NetClient => {
  const transport = netSession.factory(config);
  return {
    leave: (): void => {
      transport.leave();
    },
    localPlayer: (): Result<PlayerId> => transport.localPlayer(),
    onRejected: (callback: (reason: string) => void): void => {
      transport.onRejected(callback);
    },
    onStatus: (callback: (status: ConnStatus) => void): void => {
      transport.onStatus(callback);
    },
    sendIntent: (intent: Intent): void => {
      transport.sendIntent(intent);
    },
    status: (): ConnStatus => transport.status(),
  };
};
