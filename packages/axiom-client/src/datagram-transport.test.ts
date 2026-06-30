import assert from "node:assert/strict";
import { test } from "node:test";

import { DatagramTransport } from "./datagram-transport.ts";
import { encodeServerSnapshot, encodeWelcome } from "./codec.ts";
import { AxiomClient } from "./client.ts";
import { FakeDatagramLink } from "./fake-datagram-link.testkit.ts";
import { STATUS_CONNECTED } from "./client-config.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);

test("a datagram transport is unreliable and routes send/close to its link", () => {
  const link = new FakeDatagramLink();
  const transport = new DatagramTransport(link.factory);
  assert.equal(transport.reliable, false);

  // Before open the null link absorbs send/close without throwing (no presence check).
  transport.send(u8(9));
  transport.close();
  assert.deepEqual(link.sent, []);
  assert.equal(link.closed, false);

  const opened: string[] = [];
  transport.open({
    onClose: (): void => {
      opened.push("close");
    },
    onMessage: (): void => {
      opened.push("message");
    },
    onOpen: (): void => {
      opened.push("open");
    },
  });
  link.open();
  link.deliver(u8(1, 2, 3));
  link.fail();
  assert.deepEqual(opened, ["open", "message", "close"]);

  transport.send(u8(7, 7));
  transport.close();
  assert.deepEqual(link.sent, [u8(7, 7)]);
  assert.equal(link.closed, true);
});

test("the client converges newest-wins over a lossy, reordering datagram link", () => {
  const link = new FakeDatagramLink();
  const client = new AxiomClient();
  const applied: number[] = [];
  client.onSnapshot((snapshot): void => {
    applied.push(snapshot.serverTick);
  });

  client.connect({
    roomId: "lobby",
    transportFactory: (): DatagramTransport => new DatagramTransport(link.factory),
    url: "datagram://test",
  });
  link.open();

  // The authority admits us (this also clears the JoinRoom resend timer).
  link.deliver(encodeWelcome({ clientId: 1, fixedStepNs: 16_666_667, protocolVersion: 1, serverTick: 0 }));
  assert.equal(client.getStatus(), STATUS_CONNECTED);

  const body = u8(0xaa, 0xbb);
  // Datagrams arrive dropped + reordered: 5, then a stale 3, a jump to 8 (6,7 lost),
  // then a late straggler 4. Newest-wins must keep only the forward progress.
  link.deliver(encodeServerSnapshot(5, 0, body));
  link.deliver(encodeServerSnapshot(3, 0, body)); // stale → ignored
  link.deliver(encodeServerSnapshot(8, 2, body)); // jump forward (6,7 dropped)
  link.deliver(encodeServerSnapshot(4, 0, body)); // late straggler → ignored

  // Converged to the newest snapshot, applying only the forward steps; no crash.
  assert.equal(client.getServerTick(), 8);
  assert.deepEqual(applied, [5, 8]);
  assert.equal(client.getLastAckedSequence(), 2);

  client.disconnect();
});
