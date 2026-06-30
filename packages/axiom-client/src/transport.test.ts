import assert from "node:assert/strict";
import { test } from "node:test";

import { asUint8Array, NULL_TRANSPORT, WebSocketTransport } from "./transport.ts";
import { FakeSocket } from "./fake-socket.testkit.ts";
import { ProtocolError } from "./protocol-error.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);

const noHandlers = {
  onClose: (): void => {
    /* ignore */
  },
  onMessage: (): void => {
    /* ignore */
  },
  onOpen: (): void => {
    /* ignore */
  },
};

test("asUint8Array coerces Uint8Array and ArrayBuffer, rejects other data", () => {
  assert.deepEqual(asUint8Array(u8(1, 2, 3)), u8(1, 2, 3));
  assert.deepEqual(asUint8Array(u8(4, 5).buffer), u8(4, 5));
  assert.throws(() => asUint8Array("not binary"), ProtocolError);
  assert.throws(() => asUint8Array(123), ProtocolError);
});

test("WebSocketTransport before open is a safe no-op (null sink)", () => {
  let built = 0;
  const transport = new WebSocketTransport(() => {
    built += 1;
    return new FakeSocket("ws://test");
  });
  // Send/close before open touch the null sink: no socket is built, no throw.
  transport.send(u8(1));
  transport.close();
  assert.equal(built, 0);
  assert.equal(transport.reliable, true);
});

test("WebSocketTransport wires open/message/close/error and send/close", () => {
  let socket!: FakeSocket;
  const transport = new WebSocketTransport(() => {
    socket = new FakeSocket("ws://test");
    return socket;
  });
  const events: string[] = [];
  const received: Uint8Array[] = [];
  transport.open({
    onClose: (): void => void events.push("close"),
    onMessage: (data): void => void received.push(data),
    onOpen: (): void => void events.push("open"),
  });
  assert.equal(socket.binaryType, "arraybuffer");

  socket.open();
  socket.receive(u8(7, 8, 9));
  transport.send(u8(1, 2));
  assert.deepEqual(socket.sent, [u8(1, 2)]);
  assert.deepEqual(received, [u8(7, 8, 9)]);

  socket.fail(); // error -> onClose
  transport.close();
  assert.equal(socket.closed, true);
  assert.deepEqual(events, ["open", "close", "close"]);
});

test("NULL_TRANSPORT is a safe no-op transport", () => {
  assert.equal(NULL_TRANSPORT.reliable, true);
  // Every operation is a no-op and must not throw or invoke handlers.
  assert.doesNotThrow(() => {
    NULL_TRANSPORT.open(noHandlers);
    NULL_TRANSPORT.send(u8(1));
    NULL_TRANSPORT.close();
  });
});
