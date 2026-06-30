import assert from "node:assert/strict";
import { test } from "node:test";

import { ROOT_STREAM, makeRng } from "./rng.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";

test("next reads a unit float from the root stream", () => {
  const fake = new FakeBridge();
  fake.units = [0.25];
  const rng = makeRng(fake);
  assert.equal(rng.next(), 0.25);
  assert.equal(fake.lastUnitStream, ROOT_STREAM);
});

test("int draws an integer below the bound, threading the bound to the core", () => {
  const fake = new FakeBridge();
  fake.belows = [3];
  assert.equal(makeRng(fake).int(10), 3);
  assert.deepEqual(fake.lastBelow, { maxExclusive: 10, stream: ROOT_STREAM });
});

test("range maps the unit draw onto [min, max)", () => {
  const fake = new FakeBridge();
  fake.units = [0.5];
  assert.equal(makeRng(fake).range(10, 20), 15);
});

test("bool is true exactly when the unit draw is below the probability", () => {
  const low = new FakeBridge();
  low.units = [0.4];
  assert.equal(makeRng(low).bool(0.5), true);
  const high = new FakeBridge();
  high.units = [0.6];
  assert.equal(makeRng(high).bool(0.5), false);
  // No argument exercises the default even-coin probability.
  const def = new FakeBridge();
  def.units = [0.4];
  assert.equal(makeRng(def).bool(), true);
});

test("pick selects the element at the index the core chose", () => {
  const fake = new FakeBridge();
  fake.belows = [2];
  assert.equal(makeRng(fake).pick(["a", "b", "c"]), "c");
  assert.equal(fake.lastBelow!.maxExclusive, 3);
});

test("weighted selects by the weighted index, passing the weights to the core", () => {
  const fake = new FakeBridge();
  fake.weightedIndices = [1];
  assert.equal(makeRng(fake).weighted(["a", "b"], [1, 9]), "b");
  assert.deepEqual(fake.lastWeights, [1, 9]);
});

test("shuffle reorders the author's array in place by the core's permutation", () => {
  const fake = new FakeBridge();
  fake.permutations = [[2, 0, 1]];
  const deck = ["a", "b", "c"];
  makeRng(fake).shuffle(deck);
  // reordered[i] = snapshot[order[i]] = [c, a, b], written back in place.
  assert.deepEqual(deck, ["c", "a", "b"]);
});

test("shuffle with the identity permutation leaves the array unchanged", () => {
  const fake = new FakeBridge();
  const deck = ["a", "b", "c"];
  makeRng(fake).shuffle(deck);
  assert.deepEqual(deck, ["a", "b", "c"]);
});

test("stream descends a named sub-stream and threads its id into draws", () => {
  const fake = new FakeBridge();
  fake.streamIds.set("ai", 99);
  fake.units = [7];
  const sub = makeRng(fake).stream("ai");
  assert.deepEqual(fake.streamCalls, [[ROOT_STREAM, "ai"]]);
  assert.equal(sub.next(), 7);
  assert.equal(fake.lastUnitStream, 99);
});

test("named sub-streams are reproducible and independent", () => {
  const fake = new FakeBridge();
  const rng = makeRng(fake);
  const first = fake.streamIds.get("ai");
  rng.stream("ai");
  const minted = fake.streamIds.get("ai");
  rng.stream("ai");
  const second = fake.streamIds.get("ai");
  assert.equal(first, undefined);
  assert.equal(minted, second);
  rng.stream("loot");
  assert.notEqual(fake.streamIds.get("loot"), minted);
});
