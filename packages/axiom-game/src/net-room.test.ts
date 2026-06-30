import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type Match,
  type Matchmaker,
  type MatchmakeOptions,
  type Room,
  type RoomConfig,
  type RoomHostFactory,
  bindMatchmaker,
  bindRoomHost,
  hostRoom,
  matchmake,
} from "./net-room.ts";
import type { PlayerId } from "./vocabulary.ts";

const sampleConfig: RoomConfig = { fixedHz: 60, maxPlayers: 2, seed: 7n };

// Runs FIRST, before any bind: hostRoom/matchmake use the inert seam — the lobby
// surface is total before the runtime binds a real authority / matchmaker. This
// exercises INERT_ROOM_HOST + every INERT_ROOM member + INERT_MATCHMAKER ->
// INERT_MATCH. The test returns the matchmake promise (no banned async/await
// keyword) so node:test awaits it.
test("before binding, hostRoom yields an inert closed room and matchmake an empty match", () => {
  const room = hostRoom(sampleConfig);
  assert.equal(room.id, "");
  assert.deepEqual(room.players(), []);
  assert.doesNotThrow(() => {
    room.close();
  });

  return matchmake().then((match) => {
    assert.deepEqual(match, { roomId: "", url: "" });
  });
});

test("bindRoomHost installs the authority factory hostRoom forwards to", () => {
  const seats: PlayerId[] = [1, 2];
  let closed = 0;
  let seenConfig: RoomConfig | undefined;
  const room: Room = {
    close: () => {
      closed += 1;
    },
    id: "duel-1",
    players: () => seats,
  };
  const factory: RoomHostFactory = (config) => {
    seenConfig = config;
    return room;
  };
  bindRoomHost(factory);

  const hosted = hostRoom(sampleConfig);
  assert.deepEqual(seenConfig, sampleConfig);
  assert.equal(hosted.id, "duel-1");
  assert.deepEqual(hosted.players(), [1, 2]);
  hosted.close();
  assert.equal(closed, 1);
});

test("a RoomConfig may carry the optional botFill seam", () => {
  // botFill is the optional bot-backfill window (SPEC-13 §16.3); it threads through
  // the config the bound authority reads.
  let seen: RoomConfig | undefined;
  bindRoomHost((config): Room => {
    seen = config;
    return {
      close: (): void => {
        // no-op room
      },
      id: "r",
      players: (): readonly PlayerId[] => [],
    };
  });
  hostRoom({ botFill: { afterTicks: 90 }, fixedHz: 30, maxPlayers: 4, seed: 11n });
  assert.ok(seen);
  assert.deepEqual(seen.botFill, { afterTicks: 90 });
});

test("bindMatchmaker installs the matchmaker matchmake forwards options to", () => {
  const placement: Match = { roomId: "queued-room", url: "wss://authority" };
  let seenOptions: MatchmakeOptions | undefined;
  const matchmaker: Matchmaker = (options) => {
    seenOptions = options;
    return Promise.resolve(placement);
  };
  bindMatchmaker(matchmaker);

  return matchmake({ mode: "ranked" }).then((match) => {
    assert.deepEqual(seenOptions, { mode: "ranked" });
    assert.deepEqual(match, placement);
  });
});
