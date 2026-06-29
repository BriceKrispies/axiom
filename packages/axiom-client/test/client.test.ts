import assert from "node:assert/strict";
import { mock, test } from "node:test";

import {
  AxiomClient,
  type ConnectConfig,
  decodeClientIntent,
  decodeJoinRoom,
  decodeLeaveRoom,
  encodeClientIntentFor,
  encodeJoinRoom,
  encodeRejectedIntent,
  encodeServerEvent,
  encodeServerSnapshot,
  encodeServerSnapshotFor,
  encodeWelcome,
  KIND_JOIN_ROOM,
  REASON_MALFORMED,
  type Transport,
  type TransportHandlers,
} from "../src/index.ts";
import { FakeSocket } from "./fake-socket.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);
const welcome = (clientId: number, serverTick: number): Uint8Array =>
  encodeWelcome({ clientId, fixedStepNs: 16_666_667, protocolVersion: 1, serverTick });

function connect(overrides: Partial<ConnectConfig> = {}): { client: AxiomClient; socket: FakeSocket } {
  let socket!: FakeSocket;
  const client = new AxiomClient();
  client.connect({
    roomId: "lobby",
    socketFactory: (url): FakeSocket => {
      socket = new FakeSocket(url);
      return socket;
    },
    url: "wss://test/play",
    ...overrides,
  });
  return { client, socket };
}

function connected(): { client: AxiomClient; socket: FakeSocket } {
  const pair = connect();
  pair.socket.open();
  pair.socket.receive(welcome(77, 0));
  return pair;
}

test("connect enters connecting and sends JoinRoom on open", () => {
  const { client, socket } = connect();
  assert.equal(client.getStatus(), "connecting");
  socket.open();
  assert.equal(socket.sent.length, 1);
  const join = decodeJoinRoom(socket.sent[0]!);
  assert.equal(join.kind, KIND_JOIN_ROOM);
  assert.equal(join.protocolVersion, 1);
  assert.deepEqual(join.roomId, new TextEncoder().encode("lobby"));
});

test("connect accepts a Uint8Array roomId and an explicit protocol version", () => {
  const { client, socket } = connect({ protocolVersion: 3, roomId: u8(1, 2, 3), token: u8(9) });
  socket.open();
  const join = decodeJoinRoom(socket.sent[0]!);
  assert.equal(join.protocolVersion, 3);
  assert.deepEqual(join.roomId, u8(1, 2, 3));
  assert.deepEqual(join.token, u8(9));
  assert.equal(client.getStatus(), "connecting");
});

test("Welcome transitions to connected and a duplicate Welcome is ignored", () => {
  const statuses: string[] = [];
  let socket!: FakeSocket;
  const client = new AxiomClient();
  client.onStatus((status): void => void statuses.push(status));
  client.connect({
    roomId: "lobby",
    socketFactory: (url): FakeSocket => {
      socket = new FakeSocket(url);
      return socket;
    },
    url: "wss://test/play",
  });
  socket.open();
  socket.receive(welcome(77, 12));
  assert.equal(client.getStatus(), "connected");
  assert.equal(client.getServerTick(), 12);
  assert.equal(client.getClientId(), 77);
  // A second Welcome (unreliable path) is ignored: tick stays put.
  socket.receive(welcome(88, 99));
  assert.equal(client.getClientId(), 77);
  assert.equal(client.getServerTick(), 12);
  assert.deepEqual(statuses, ["connecting", "connected"]);
});

test("sendIntent returns 0 until connected, then the assigned sequence", () => {
  const fresh = new AxiomClient();
  assert.equal(fresh.sendIntent(u8(1)), 0);
  assert.equal(fresh.getPendingIntentCount(), 0);

  const { client, socket } = connect();
  socket.open(); // connecting, not yet welcomed
  assert.equal(client.sendIntent(u8(1)), 0);

  socket.receive(welcome(1, 0));
  assert.equal(client.sendIntent(u8(4, 5, 6)), 1);
  const intent = decodeClientIntent(socket.sent[socket.sent.length - 1]!);
  assert.equal(intent.clientSequence, 1);
  assert.deepEqual(intent.payload, u8(4, 5, 6));
  assert.equal(client.sendIntent(u8(7)), 2);
  assert.equal(client.getPendingIntentCount(), 2);
});

test("Snapshot fires the handler, acks pending intents, and rejects older ticks", () => {
  const { client, socket } = connected();
  const ticks: number[] = [];
  client.onSnapshot((snapshot): void => void ticks.push(snapshot.serverTick));
  client.sendIntent(u8(1));
  client.sendIntent(u8(2));
  socket.receive(encodeServerSnapshot(5, 1, u8(0xaa)));
  assert.deepEqual(ticks, [5]);
  assert.equal(client.getServerTick(), 5);
  assert.equal(client.getLastAckedSequence(), 1);
  assert.equal(client.getPendingIntentCount(), 1);
  socket.receive(encodeServerSnapshot(4, 0, u8())); // older -> ignored
  assert.equal(client.getServerTick(), 5);
  socket.receive(encodeServerSnapshot(5, 0, u8())); // equal -> allowed
  assert.deepEqual(ticks, [5, 5]);
});

test("Event fires the event handler", () => {
  const { client, socket } = connected();
  const ticks: number[] = [];
  client.onEvent((event): void => void ticks.push(event.serverTick));
  socket.receive(encodeServerEvent(9, u8(1, 2)));
  assert.deepEqual(ticks, [9]);
});

test("RejectedIntent drops the rejected sequence from pending and notifies onRejected", () => {
  const { client, socket } = connected();
  const reasons: number[] = [];
  client.onRejected((reasonCode): void => void reasons.push(reasonCode));
  client.sendIntent(u8(1));
  client.sendIntent(u8(2));
  socket.receive(encodeRejectedIntent(1, REASON_MALFORMED));
  assert.equal(client.getPendingIntentCount(), 1);
  // The registered observer sees the authority's machine-readable reason code.
  assert.deepEqual(reasons, [REASON_MALFORMED]);
});

test("a server-sent client-kind frame is ignored, not fatal", () => {
  const { client, socket } = connected();
  socket.receive(encodeJoinRoom(1, u8(1), u8())); // client->server kind: ignored
  assert.equal(client.getStatus(), "connected");
});

test("per-player frames are ignored by this single-seat client, not fatal", () => {
  const { client, socket } = connected();
  const snapshots: number[] = [];
  client.onSnapshot((snapshot): void => void snapshots.push(snapshot.serverTick));
  socket.receive(encodeClientIntentFor({
    clientSequence: 1,
    lastSeenServerTick: 0,
    payload: u8(),
    player: 2,
    predictedClientTick: 0,
  }));
  socket.receive(encodeServerSnapshotFor(99, [{ player: 2, sequence: 1 }], u8(0xaa)));
  // Neither advances the single-seat snapshot state nor fires a handler.
  assert.deepEqual(snapshots, []);
  assert.equal(client.getServerTick(), 0);
  assert.equal(client.getStatus(), "connected");
});

test("disconnect when connected sends LeaveRoom then closes", () => {
  const { client, socket } = connected();
  client.disconnect();
  const last = socket.sent[socket.sent.length - 1]!;
  assert.deepEqual(decodeLeaveRoom(last).roomId, new TextEncoder().encode("lobby"));
  assert.equal(socket.closed, true);
  assert.equal(client.getStatus(), "disconnected");
});

test("disconnect before connecting is a safe no-op close", () => {
  const fresh = new AxiomClient();
  assert.doesNotThrow(() => {
    fresh.disconnect();
  });
  assert.equal(fresh.getStatus(), "disconnected");
});

test("a transport-driven close returns the client to disconnected", () => {
  const { client, socket } = connected();
  socket.close(); // drives onClose
  assert.equal(client.getStatus(), "disconnected");
});

test("an unreliable transport resends JoinRoom until welcomed, then stops", () => {
  mock.timers.enable({ apis: ["setInterval"] });
  let handlers!: TransportHandlers;
  const sent: Uint8Array[] = [];
  const unreliable: Transport = {
    close: (): void => {
      /* ignore */
    },
    open: (received): void => {
      handlers = received;
    },
    reliable: false,
    send: (data): void => void sent.push(data),
  };
  const client = new AxiomClient();
  client.connect({ roomId: "lobby", transportFactory: (): Transport => unreliable, url: "u" });
  handlers.onOpen();
  assert.equal(sent.length, 1); // initial JoinRoom
  mock.timers.tick(250);
  assert.equal(sent.length, 2); // resent while still connecting
  handlers.onMessage(welcome(1, 0));
  assert.equal(client.getStatus(), "connected");
  mock.timers.tick(250); // first tick after welcome stops the timer
  mock.timers.tick(250); // and no further resends happen
  assert.equal(sent.length, 2);
  mock.timers.reset();
});
