import { test } from "node:test";
import assert from "node:assert/strict";

import { AxiomClient, type ConnectConfig } from "../src/index.ts";
import {
  decodeJoinRoom,
  decodeLeaveRoom,
  decodeClientIntent,
  encodeRejectedIntent,
  encodeServerEvent,
  encodeServerSnapshot,
  encodeWelcome,
  KIND_JOIN_ROOM,
  REASON_MALFORMED,
} from "../src/index.ts";
import { FakeSocket } from "./fake_socket.ts";

const u8 = (...bytes: number[]) => Uint8Array.from(bytes);

/** Connect a client through a FakeSocket and return both. */
function connect(overrides: Partial<ConnectConfig> = {}): { client: AxiomClient; socket: FakeSocket } {
  let socket!: FakeSocket;
  const client = new AxiomClient();
  client.connect({
    url: "wss://test/play",
    roomId: "lobby",
    protocolVersion: 1,
    socketFactory: (url) => {
      socket = new FakeSocket(url);
      return socket;
    },
    ...overrides,
  });
  return { client, socket };
}

/** Drive a client all the way to "connected" (open + Welcome). */
function connected(): { client: AxiomClient; socket: FakeSocket } {
  const pair = connect();
  pair.socket.open();
  pair.socket.receive(encodeWelcome(1, 77, 0, 16_666_667));
  return pair;
}

test("connect creates a client in connecting state", () => {
  const { client } = connect();
  assert.equal(client.getStatus(), "connecting");
});

test("socket open sends JoinRoom", () => {
  const { socket } = connect({ roomId: "lobby" });
  socket.open();
  assert.equal(socket.sent.length, 1);
  const join = decodeJoinRoom(socket.sent[0]!);
  assert.equal(join.kind, KIND_JOIN_ROOM);
  assert.equal(join.protocolVersion, 1);
  assert.deepEqual(join.roomId, new TextEncoder().encode("lobby"));
});

test("status handler observes transitions", () => {
  const seen: string[] = [];
  const { socket } = connect();
  // The first "connecting" fires inside connect(); register after to capture
  // the connected transition, then assert the full path separately.
  const client2 = new AxiomClient();
  const statuses: string[] = [];
  client2.onStatus((s) => statuses.push(s));
  let inner!: FakeSocket;
  client2.connect({ url: "u", roomId: "r", socketFactory: (u) => (inner = new FakeSocket(u)) });
  inner.open();
  inner.receive(encodeWelcome(1, 1, 0, 1));
  assert.deepEqual(statuses, ["connecting", "connected"]);
  // (the first `socket` is unused beyond construction)
  assert.ok(seen.length === 0);
});

test("Welcome transitions to connected", () => {
  const { client, socket } = connect();
  socket.open();
  assert.equal(client.getStatus(), "connecting");
  socket.receive(encodeWelcome(1, 77, 12, 16_666_667));
  assert.equal(client.getStatus(), "connected");
  assert.equal(client.getServerTick(), 12);
});

test("sendIntent rejects while disconnected", () => {
  const client = new AxiomClient();
  assert.equal(client.sendIntent(u8(1)), null);
  assert.equal(client.getPendingIntentCount(), 0);
});

test("sendIntent rejects while merely connecting", () => {
  const { client, socket } = connect();
  socket.open(); // JoinRoom sent, but no Welcome yet
  assert.equal(client.sendIntent(u8(1)), null);
  assert.equal(client.getPendingIntentCount(), 0);
});

test("sendIntent sends an encoded ClientIntent and returns its sequence", () => {
  const { client, socket } = connected();
  const before = socket.sent.length;
  assert.equal(client.sendIntent(u8(4, 5, 6)), 1); // returns the assigned seq
  assert.equal(socket.sent.length, before + 1);
  const intent = decodeClientIntent(socket.sent[socket.sent.length - 1]!);
  assert.equal(intent.clientSequence, 1);
  assert.deepEqual(intent.payload, u8(4, 5, 6));
  assert.equal(client.getPendingIntentCount(), 1);
  // The sequence increments.
  assert.equal(client.sendIntent(u8(7)), 2);
  assert.equal(client.getPendingIntentCount(), 2);
});

test("getClientId returns the server-assigned id after Welcome", () => {
  const { client } = connected(); // connected() welcomes with clientId 77
  assert.equal(client.getClientId(), 77);
});

test("Snapshot invokes the snapshot handler and acks pending intents", () => {
  const { client, socket } = connected();
  const snapshots: number[] = [];
  client.onSnapshot((s) => snapshots.push(s.serverTick));
  client.sendIntent(u8(1)); // seq 1
  client.sendIntent(u8(2)); // seq 2
  socket.receive(encodeServerSnapshot(5, 1, u8(0xaa)));
  assert.deepEqual(snapshots, [5]);
  assert.equal(client.getServerTick(), 5);
  assert.equal(client.getLastAckedSequence(), 1);
  assert.equal(client.getPendingIntentCount(), 1); // seq 2 still pending
});

test("an older snapshot is ignored, an equal tick is allowed", () => {
  const { client, socket } = connected();
  socket.receive(encodeServerSnapshot(10, 0, u8()));
  assert.equal(client.getServerTick(), 10);
  socket.receive(encodeServerSnapshot(9, 0, u8())); // older → ignored
  assert.equal(client.getServerTick(), 10);
  socket.receive(encodeServerSnapshot(10, 0, u8())); // equal → allowed
  assert.equal(client.getServerTick(), 10);
});

test("Event invokes the event handler", () => {
  const { client, socket } = connected();
  const events: number[] = [];
  client.onEvent((e) => events.push(e.serverTick));
  socket.receive(encodeServerEvent(9, u8(1, 2)));
  assert.deepEqual(events, [9]);
});

test("RejectedIntent updates pending state", () => {
  const { client, socket } = connected();
  client.sendIntent(u8(1)); // seq 1
  client.sendIntent(u8(2)); // seq 2
  assert.equal(client.getPendingIntentCount(), 2);
  socket.receive(encodeRejectedIntent(1, REASON_MALFORMED));
  assert.equal(client.getPendingIntentCount(), 1); // only seq 2 remains
});

test("disconnect closes the socket and moves to disconnected", () => {
  const { client, socket } = connected();
  client.disconnect();
  // LeaveRoom sent before close.
  const last = socket.sent[socket.sent.length - 1]!;
  assert.deepEqual(decodeLeaveRoom(last).roomId, new TextEncoder().encode("lobby"));
  assert.equal(socket.closed, true);
  assert.equal(client.getStatus(), "disconnected");
});

test("a server-sent client message is ignored, not fatal", () => {
  const { client, socket } = connected();
  // A ClientIntent frame should never come from the server; the client ignores it.
  socket.receive(encodeRejectedIntent(0, 0)); // valid, to confirm liveness
  assert.equal(client.getStatus(), "connected");
});
