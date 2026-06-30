import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type AxiomClientLike,
  axiomNetFactory,
  decodeIntent,
  encodeIntent,
  netTransportFromClient,
} from "./axiom-net.ts";
import type { ConnStatus, Intent, JoinConfig } from "./net.ts";
import type { PlayerId } from "./vocabulary.ts";

const noSeats: PlayerId[] = [];

/*
 * A fake `AxiomClient` (no socket): it records what the transport drives and lets a
 * test fire the registered status / rejection observers, so the whole adapter is
 * covered without `@axiom/client` or a browser.
 */
class FakeClient implements AxiomClientLike {
  public status: ConnStatus = "connecting";
  public clientId = 0;
  public readonly sent: Uint8Array[] = [];
  public disconnects = 0;
  public statusObserver: (status: ConnStatus) => void = (): void => {
    // replaced by onStatus
  };
  public rejectedObserver: (reasonCode: number) => void = (): void => {
    // replaced by onRejected
  };

  public getStatus(): ConnStatus {
    return this.status;
  }
  public getClientId(): number {
    return this.clientId;
  }
  public sendIntent(payload: Uint8Array): number {
    this.sent.push(payload);
    return this.sent.length;
  }
  public onStatus(handler: (status: ConnStatus) => void): void {
    this.statusObserver = handler;
  }
  public onRejected(handler: (reasonCode: number) => void): void {
    this.rejectedObserver = handler;
  }
  public disconnect(): void {
    this.disconnects += 1;
  }
}

test("encodeIntent/decodeIntent round-trip every field type, order-independent", () => {
  // A boolean, a number, and a string round-trip exactly — covering all three
  // VALUE_ENCODERS / VALUE_DECODERS and every typeIndex branch.
  const intent: Intent = { dash: 3.5, fire: true, weapon: "rail" };
  const bytes = encodeIntent(intent);
  assert.deepEqual(decodeIntent(bytes), intent);
  // Built in a DIFFERENT key order at runtime (not a literal, so sort-keys can't
  // reorder it): sorted-key encoding (byKey's `<` and `>` arms) makes the bytes
  // independent of insertion order.
  const shuffled: [string, boolean | number | string][] = [
    ["weapon", "rail"],
    ["fire", true],
    ["dash", 3.5],
  ];
  assert.deepEqual(encodeIntent(Object.fromEntries(shuffled)), bytes);
});

test("encodeIntent/decodeIntent handle a false boolean and an empty record", () => {
  // A false boolean encodes its 0 value byte and decodes back to `false`.
  assert.deepEqual(decodeIntent(encodeIntent({ crouch: false })), { crouch: false });
  // The empty intent encodes to a zero field-count and decodes back to {} (the
  // bounded Array.from map visits zero fields).
  assert.deepEqual(decodeIntent(encodeIntent({})), {});
});

test("netTransportFromClient forwards status, intent, and leave to the client", () => {
  const fake = new FakeClient();
  fake.status = "connected";
  const transport = netTransportFromClient(fake);
  assert.equal(transport.status(), "connected");

  // sendIntent encodes the author intent through encodeIntent before the client ships it.
  transport.sendIntent({ dash: 1 });
  assert.deepEqual(fake.sent, [encodeIntent({ dash: 1 })]);

  transport.leave();
  assert.equal(fake.disconnects, 1);
});

test("localPlayer is absent until admitted, then the client id", () => {
  const fake = new FakeClient();
  const transport = netTransportFromClient(fake);
  // clientId 0 = not yet welcomed -> the absent seat (localPlayerOf's absent thunk).
  assert.equal(transport.localPlayer(), noSeats.at(0));
  fake.clientId = 5;
  // A non-zero id -> the present seat (localPlayerOf's present thunk).
  assert.equal(transport.localPlayer(), 5);
});

test("onStatus forwards the observer and onRejected maps the reason code to text", () => {
  const fake = new FakeClient();
  const transport = netTransportFromClient(fake);
  const statuses: ConnStatus[] = [];
  const reasons: string[] = [];
  transport.onStatus((status): void => void statuses.push(status));
  transport.onRejected((reason): void => void reasons.push(reason));

  fake.statusObserver("connected");
  fake.rejectedObserver(1); // REASON_MALFORMED -> in-range reasonText
  fake.rejectedObserver(99); // out of range -> the unspecified fallback (Math.min clamp)
  assert.deepEqual(statuses, ["connected"]);
  assert.deepEqual(reasons, ["malformed", "unspecified"]);
});

test("axiomNetFactory opens a client per config and wraps it as a transport", () => {
  const fake = new FakeClient();
  fake.clientId = 7;
  let seenConfig: JoinConfig | undefined;
  const factory = axiomNetFactory((config): AxiomClientLike => {
    seenConfig = config;
    return fake;
  });
  const config: JoinConfig = { roomId: "duel", token: "jwt", url: "wss://authority" };
  const transport = factory(config);
  assert.deepEqual(seenConfig, config);
  assert.equal(transport.localPlayer(), 7);
});
