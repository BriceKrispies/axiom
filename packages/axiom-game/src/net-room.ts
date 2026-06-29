/*
 * The room hosting + matchmaking authoring entry (SPEC-13 §16.3 / §16.6). These
 * are the lobby surface around the per-client `joinRoom` (`net.ts`):
 *   - `hostRoom(RoomConfig) → Room` stands up a server-authoritative room running
 *     the authored sim. Per SPEC-13 §3.5 the *authority itself* (the socket accept
 *     loop, wall-clock ticks) is app/tool tier (`tools/axiom-netplay-server`), NOT
 *     a reusable spine module — so this projection is a thin handle over a
 *     `RoomHostFactory` seam the runtime binds (its `@axiom/client` / authority-
 *     harness binding), exactly as `net.ts` binds the client `NetTransport` seam.
 *   - `matchmake(opts?) → Promise<Match>` asks the host to place the player into a
 *     room, returning its `{ roomId, url }` for `joinRoom`. Matchmaking is a host
 *     HTTP policy (SPEC-13 §9), so it too is a bound `Matchmaker` seam.
 *
 * Both surfaces are total before the runtime binds: the inert room host yields an
 * empty closed room and the inert matchmaker resolves to an empty match, so the
 * authoring API is callable (and fully covered) without a server or `@axiom/client`
 * — the same inert-until-bound discipline as `net.ts`'s transport. Everything is
 * pure forwarding through the bound seam (no control flow).
 */

import type { PlayerId, Ticks } from "./vocabulary.ts";

/** A room identity, stable within the host (SPEC-13 §16.3). */
export type RoomId = string;

/** The room configuration `hostRoom` stands an authoritative sim up from (SPEC-13 §16.3). */
export interface RoomConfig {
  /** The number of player seats the room admits. */
  readonly maxPlayers: number;
  /** The deterministic seed every deployment of this room shares. */
  readonly seed: bigint;
  /** The fixed simulation rate the authority steps at. */
  readonly fixedHz: number;
  /** Fill empty seats with engine-driven bots after this many idle ticks (optional). */
  readonly botFill?: { readonly afterTicks: Ticks };
}

/** A live authoritative room handle (SPEC-13 §16.3). */
export interface Room {
  /** The room's stable id. */
  readonly id: RoomId;
  /** The seated players, in stable order. */
  readonly players: () => readonly PlayerId[];
  /** Close the room and release its authority. */
  readonly close: () => void;
}

/** Stands up a `Room` for a `hostRoom` call — the runtime's authority binding point. */
export type RoomHostFactory = (config: RoomConfig) => Room;

/** A matchmaking placement (SPEC-13 §16.6): the room to join and the authority URL. */
export interface Match {
  /** The room the player was placed into. */
  readonly roomId: RoomId;
  /** The authority endpoint URL `joinRoom` connects to. */
  readonly url: string;
}

/** The matchmaking request options (SPEC-13 §16.6); `mode` selects a queue. */
export interface MatchmakeOptions {
  /** The match mode / queue to place into. */
  readonly mode?: string;
}

/** Places a player into a room — the runtime's matchmaking binding point. */
export type Matchmaker = (options?: MatchmakeOptions) => Promise<Match>;

/** No seats — the empty list an inert / closed room reports. */
const NO_PLAYERS: readonly PlayerId[] = [];

/*
 * The inert room before `bindRoomHost`: empty id, no seats, `close` a no-op. Keeps
 * `hostRoom` total before the runtime binds a real authority.
 */
const INERT_ROOM: Room = {
  close: (): void => {
    // No-op until a room host is bound
  },
  id: "",
  players: (): readonly PlayerId[] => NO_PLAYERS,
};

/** The inert room-host factory before `bindRoomHost` — every host yields the inert room. */
const INERT_ROOM_HOST: RoomHostFactory = (): Room => INERT_ROOM;

/** The inert match before `bindMatchmaker` — empty room id, empty URL. */
const INERT_MATCH: Match = { roomId: "", url: "" };

/** The inert matchmaker before `bindMatchmaker` — resolves to the inert match. */
const INERT_MATCHMAKER: Matchmaker = (): Promise<Match> => Promise.resolve(INERT_MATCH);

/** The mutable room session: the bound authority factory and the bound matchmaker. */
const roomSession: { host: RoomHostFactory; matchmaker: Matchmaker } = {
  host: INERT_ROOM_HOST,
  matchmaker: INERT_MATCHMAKER,
};

/** Install the runtime's real authority factory (its `@axiom/client` / harness binding), once at boot. */
export const bindRoomHost = (host: RoomHostFactory): void => {
  roomSession.host = host;
};

/** Install the runtime's real matchmaker (its host HTTP binding), once at boot. */
export const bindMatchmaker = (matchmaker: Matchmaker): void => {
  roomSession.matchmaker = matchmaker;
};

/** Host a server-authoritative room over the bound authority (SPEC-13 §16.3). */
export const hostRoom = (config: RoomConfig): Room => roomSession.host(config);

/** Request a matchmaking placement over the bound matchmaker (SPEC-13 §16.6). */
export const matchmake = (options?: MatchmakeOptions): Promise<Match> => roomSession.matchmaker(options);
