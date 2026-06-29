import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type ConnStatus,
  type Intent,
  type JoinConfig,
  type NetParticipants,
  type NetTransport,
  type NetTransportFactory,
  bindNetTransport,
  boundNetConfig,
  configureNet,
  joinRoom,
  makeNetSim,
} from "../src/net.ts";
import type { SimContext } from "../src/sim.ts";
import { TickPump } from "../src/pump.ts";
import { makeInput } from "../src/input.ts";
import type { PlayerId } from "../src/vocabulary.ts";
import { FakeBridge } from "./fake-bridge.ts";

const fixedHz = 60;
const noSeats: PlayerId[] = [];

const onStatusNoop = (): void => {
  // no-op status observer
};
const onRejectedNoop = (): void => {
  // no-op rejection observer
};

const makeContext = (): SimContext => {
  const bridge = new FakeBridge();
  return { bridge, fixedHz, pump: new TickPump(bridge, fixedHz) };
};

// Runs FIRST, before bindNetTransport: joinRoom uses the inert transport and the
// default (predict-nothing) config — the surface is total before the runtime binds.
test("before a transport is bound, joinRoom is inert and the config predicts nothing", () => {
  assert.deepEqual(boundNetConfig(), { interpolateRemote: false, predictLocalPlayer: false });
  const client = joinRoom({ roomId: "r1", url: "wss://x" });
  assert.equal(client.status(), "disconnected");
  assert.equal(client.localPlayer(), noSeats.at(0));
  assert.doesNotThrow(() => {
    client.sendIntent({ fire: true });
    client.onStatus(onStatusNoop);
    client.onRejected(onRejectedNoop);
    client.leave();
  });
});

test("makeNetSim widens the Sim with player addressing scoped to the tick", () => {
  const context = makeContext();
  const seenInputOf: [PlayerId, number][] = [];
  const seenPlayers: number[] = [];
  const participants: NetParticipants = {
    inputOf: (player, tick) => {
      seenInputOf.push([player, tick]);
      return makeInput(context.bridge, tick);
    },
    joinedThisTick: () => [1],
    leftThisTick: () => [2],
    players: (tick) => {
      seenPlayers.push(tick);
      return [1, 2, 3];
    },
  };
  const sim = makeNetSim(context, participants, 7);
  // Base Sim members are retained unchanged.
  assert.equal(sim.tick, 7);
  assert.equal(sim.dt, 1 / fixedHz);
  assert.equal(typeof sim.world.spawn, "function");
  assert.equal(typeof sim.rng.next, "function");
  // Player addressing forwards to the participants, scoped to the running tick.
  assert.deepEqual(sim.players(), [1, 2, 3]);
  assert.deepEqual(seenPlayers, [7]);
  assert.deepEqual(sim.joinedThisTick(), [1]);
  assert.deepEqual(sim.leftThisTick(), [2]);
  assert.equal(typeof sim.inputOf(2).isDown, "function");
  assert.deepEqual(seenInputOf, [[2, 7]]);
});

test("joinRoom wraps the bound transport and forwards the NetClient surface", () => {
  const sent: Intent[] = [];
  const statusCbs: ((status: ConnStatus) => void)[] = [];
  const rejectedCbs: ((reason: string) => void)[] = [];
  let leaves = 0;
  let seenConfig: JoinConfig | undefined;
  const transport: NetTransport = {
    leave: () => {
      leaves += 1;
    },
    localPlayer: () => 5,
    onRejected: (cb) => {
      rejectedCbs.push(cb);
    },
    onStatus: (cb) => {
      statusCbs.push(cb);
    },
    sendIntent: (intent) => {
      sent.push(intent);
    },
    status: () => "connected",
  };
  const factory: NetTransportFactory = (config) => {
    seenConfig = config;
    return transport;
  };
  bindNetTransport(factory);

  const config: JoinConfig = { roomId: "duel", token: "jwt", url: "wss://authority" };
  const client = joinRoom(config);
  assert.deepEqual(seenConfig, config);
  assert.equal(client.status(), "connected");
  assert.equal(client.localPlayer(), 5);

  client.sendIntent({ dash: 1 });
  client.onStatus(onStatusNoop);
  client.onRejected(onRejectedNoop);
  client.leave();
  assert.deepEqual(sent, [{ dash: 1 }]);
  assert.deepEqual(statusCbs, [onStatusNoop]);
  assert.deepEqual(rejectedCbs, [onRejectedNoop]);
  assert.equal(leaves, 1);
});

test("configureNet replaces the active network config", () => {
  configureNet({ interpolateRemote: true, interpolationDelayTicks: 3, predictLocalPlayer: true });
  assert.deepEqual(boundNetConfig(), {
    interpolateRemote: true,
    interpolationDelayTicks: 3,
    predictLocalPlayer: true,
  });
});
