/*
 * tier-pin.test.ts — `node --test` coverage for the persisted-tier codec. The
 * cases that matter are the REJECTIONS: a pin written on a different backing
 * host, an older payload shape, and a corrupted string must all decode as
 * absent, because each of them, believed, would pin a VDI user to a backend
 * their current host cannot render.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { decodePin, encodePin, environmentStamp } from "./tier-pin.ts";

const HOST = {
  devicePixelRatio: 1,
  screenHeight: 1080,
  screenWidth: 1920,
  userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/141.0.0.0",
};

const stamp = environmentStamp(HOST);

test("the stamp changes with every part of the environment", () => {
  assert.notEqual(stamp, environmentStamp({ ...HOST, devicePixelRatio: 2 }));
  assert.notEqual(stamp, environmentStamp({ ...HOST, screenWidth: 1280 }));
  assert.notEqual(stamp, environmentStamp({ ...HOST, screenHeight: 800 }));
  assert.notEqual(stamp, environmentStamp({ ...HOST, userAgent: "Mozilla/5.0 (X11; Linux x86_64)" }));
  assert.equal(stamp, environmentStamp({ ...HOST }), "and is stable for the same environment");
});

test("a pin round-trips within its own environment", () => {
  assert.equal(decodePin(encodePin("webgl2", stamp), stamp), "webgl2");
  assert.equal(decodePin(encodePin("css3d", stamp), stamp), "css3d");
});

test("a pin written on a different backing host is rejected", () => {
  const elsewhere = environmentStamp({ ...HOST, devicePixelRatio: 2 });
  assert.equal(decodePin(encodePin("webgpu", elsewhere), stamp), undefined, "a re-hosted VDI session must not inherit the pin");
});

test("a missing, corrupt, or foreign payload decodes as absent", () => {
  assert.equal(decodePin(undefined, stamp), undefined);
  assert.equal(decodePin("", stamp), undefined);
  assert.equal(decodePin("webgl2", stamp), undefined, "a bare tier name is not a pin");
  assert.equal(decodePin(`v0\n${stamp}\nwebgl2`, stamp), undefined, "a superseded payload version");
  assert.equal(decodePin(`v1\n${stamp}\nwebgl2\nextra`, stamp), undefined, "too many fields");
  assert.equal(decodePin(`v1\n${stamp}\nvulkan`, stamp), undefined, "a tier name that does not exist");
  assert.equal(decodePin(`v1\n${stamp}\nauto`, stamp), undefined, "auto is not a tier");
});
