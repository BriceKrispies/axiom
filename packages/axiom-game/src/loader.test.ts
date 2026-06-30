import assert from "node:assert/strict";
import { test } from "node:test";

import { FakeHost } from "./fake-host.testkit.ts";
import { bindNative } from "./host-binding.ts";
import { loadFont, loadTexture } from "./loader.ts";

test("loadTexture forwards the url to the bound host and returns its handle", () => {
  const host = new FakeHost();
  bindNative(host);
  const handle = loadTexture("player.png");
  assert.deepEqual(host.loadedTextureUrls, ["player.png"]);
  assert.equal(handle, 1);
  // The same surface dedupes at the native layer; the free function just forwards,
  // so a second call records a second url here (the host owns identity).
  loadTexture("enemy.png");
  assert.deepEqual(host.loadedTextureUrls, ["player.png", "enemy.png"]);
});

test("loadFont forwards the url to the bound host and returns its FontSpec", () => {
  const host = new FakeHost();
  host.fontReturn = { family: "monospace", size: 24 };
  bindNative(host);
  const spec = loadFont("game.ttf");
  assert.deepEqual(host.loadedFontUrls, ["game.ttf"]);
  assert.deepEqual(spec, { family: "monospace", size: 24 });
});
