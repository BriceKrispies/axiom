import assert from "node:assert/strict";
import { test } from "node:test";

import {
  loadSound,
  playMusic,
  playSound,
  playTone,
  scheduleSound,
  setMasterVolume,
  setMuted,
  stopVoice,
} from "../src/sound.ts";
import { bindNative } from "../src/host-binding.ts";
import { FakeHost } from "./fake-host.ts";

test("loadSound registers a URL and returns a handle", () => {
  const host = new FakeHost();
  bindNative(host);
  const id = loadSound("blip.wav");
  assert.deepEqual(host.loadedUrls, ["blip.wav"]);
  assert.equal(id, 1);
});

test("playSound forwards the voice options (present and absent)", () => {
  const host = new FakeHost();
  bindNative(host);
  const sound = loadSound("blip.wav");
  playSound(sound, { loop: true, pitch: 1.5, volume: 0.5 });
  playSound(sound);
  assert.deepEqual(host.playedSounds, [
    [sound, { loop: true, pitch: 1.5, volume: 0.5 }],
    [sound, undefined],
  ]);
});

test("stopVoice forwards the voice handle", () => {
  const host = new FakeHost();
  bindNative(host);
  stopVoice(42);
  assert.deepEqual(host.stoppedVoices, [42]);
});

test("playMusic forwards the playlist and options", () => {
  const host = new FakeHost();
  bindNative(host);
  playMusic(["a.ogg", "b.ogg"], { crossfadeSeconds: 2, loop: true });
  assert.deepEqual(host.playedMusic, [[["a.ogg", "b.ogg"], { crossfadeSeconds: 2, loop: true }]]);
});

test("playTone forwards the synthesis spec", () => {
  const host = new FakeHost();
  bindNative(host);
  const spec = {
    duration: 0.25,
    envelope: { attack: 0.01, decay: 0.05, release: 0.1, sustain: 0.7 },
    freq: 220,
    wave: "square",
  } as const;
  playTone(spec);
  assert.deepEqual(host.playedTones, [spec]);
});

test("scheduleSound forwards the time and options", () => {
  const host = new FakeHost();
  bindNative(host);
  const sound = loadSound("hit.wav");
  scheduleSound(sound, 1.5, { volume: 0.8 });
  assert.deepEqual(host.scheduledSounds, [[sound, 1.5, { volume: 0.8 }]]);
});

test("setMasterVolume and setMuted forward to the host", () => {
  const host = new FakeHost();
  bindNative(host);
  setMasterVolume(0.6);
  setMuted(true);
  assert.deepEqual(host.masterVolumes, [0.6]);
  assert.deepEqual(host.muteStates, [true]);
});
