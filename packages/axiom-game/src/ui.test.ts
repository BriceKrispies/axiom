import assert from "node:assert/strict";
import { test } from "node:test";

import { makeUi } from "./ui.ts";
import { type SimContext, makeSim } from "./sim.ts";
import { bindNative } from "./host-binding.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";
import { FakeHost } from "./fake-host.testkit.ts";
import { TickPump } from "./pump.ts";
import type { Rgba } from "./vocabulary.ts";

const WHITE: Rgba = [1, 1, 1, 1];
const BLACK: Rgba = [0, 0, 0, 1];

test("makeUi forwards every verb to the bound host", () => {
  const host = new FakeHost();
  bindNative(host);
  host.uiViewportReturn = { height: 240, width: 320 };
  host.uiDrawListReturn = Uint8Array.from([1, 2, 3]);
  const ui = makeUi();

  ui.beginFrame({ height: 240, width: 320 }, { x: 5, y: 6 }, true);
  ui.rect({ height: 4, width: 3, x: 1, y: 2 }, { fill: WHITE, stroke: BLACK, strokeWidth: 2 });
  ui.text("score", { color: WHITE, font: { family: "monospace", size: 12 }, pos: { x: 8, y: 8 } });
  ui.sprite(7, { pos: { x: 10, y: 10 } });
  // The pointer (5, 6) installed by beginFrame lies inside this button on its
  // press edge, so the fake's faithful hit-test reports it activated.
  const activated = ui.button({ height: 20, width: 40, x: 0, y: 0 }, "ok", { fill: WHITE });

  assert.deepEqual(host.uiBeginFrames, [
    { pointer: { x: 5, y: 6 }, pressed: true, viewport: { height: 240, width: 320 } },
  ]);
  assert.deepEqual(host.uiRects, [
    { bounds: { height: 4, width: 3, x: 1, y: 2 }, style: { fill: WHITE, stroke: BLACK, strokeWidth: 2 } },
  ]);
  // text/sprite carry the SPEC-04 `TextOpts`/`SpriteOpts` records, forwarded unchanged.
  assert.deepEqual(host.uiTexts, [
    { opts: { color: WHITE, font: { family: "monospace", size: 12 }, pos: { x: 8, y: 8 } }, value: "score" },
  ]);
  assert.deepEqual(host.uiSprites, [{ opts: { pos: { x: 10, y: 10 } }, texture: 7 }]);
  assert.deepEqual(host.uiButtons, [
    { bounds: { height: 20, width: 40, x: 0, y: 0 }, label: "ok", style: { fill: WHITE } },
  ]);
  assert.equal(activated, true);
  // `viewport` is a readonly property (not a method): reading it returns this
  // frame's installed viewport snapshot.
  assert.deepEqual(ui.viewport, { height: 240, width: 320 });
  assert.deepEqual([...ui.drawList()], [1, 2, 3]);
});

// SPEC-09 §4.2 (~line 152): `Ui.text`/`Ui.sprite` reuse the full SPEC-04
// `TextOpts`/`SpriteOpts` style records UNCHANGED — every styling field
// (`font`/`align`/`layer`/`alpha` for text; `rotation`/`scale`/`anchor`/`tint`/
// `flip`/`source`/`layer`/`alpha` for a sprite) reaches the bound host with no
// field dropped, the contract author surface the gap audit (SPEC-09 finding #1/#2)
// requires.
test("text/sprite carry every SPEC-04 styling field through to the host, none dropped", () => {
  const host = new FakeHost();
  bindNative(host);
  const ui = makeUi();

  const textOpts = {
    align: "center" as const,
    alpha: 0.75,
    color: WHITE,
    font: { family: "serif", size: 18, weight: 700 },
    layer: 3,
    pos: { x: 4, y: 5 },
  };
  const spriteOpts = {
    alpha: 0.5,
    anchor: { x: 0.5, y: 0.5 },
    flipX: true,
    flipY: false,
    layer: 2,
    pos: { x: 10, y: 20 },
    rotation: 1.5,
    scale: { x: 2, y: 3 },
    source: { height: 16, width: 8, x: 1, y: 2 },
    tint: BLACK,
  };
  ui.text("hp", textOpts);
  ui.sprite(9, spriteOpts);

  // The full records arrive at the host verbatim — the path is lossless.
  assert.deepEqual(host.uiTexts, [{ opts: textOpts, value: "hp" }]);
  assert.deepEqual(host.uiSprites, [{ opts: spriteOpts, texture: 9 }]);
});

test("button with no style supplies the default transparent-fill style (the orElse default)", () => {
  const host = new FakeHost();
  bindNative(host);
  const ui = makeUi();

  // No beginFrame ⇒ no press edge ⇒ the fake reports the button inactive.
  const defaulted = ui.button({ height: 20, width: 40, x: 0, y: 0 }, "x");

  assert.equal(defaulted, false);
  assert.deepEqual(host.uiButtons, [
    { bounds: { height: 20, width: 40, x: 0, y: 0 }, label: "x", style: { fill: [0, 0, 0, 0] } },
  ]);
});

// SPEC-09 §7: the button hit-test truth table — activated iff the pointer is
// inside `bounds` on its press edge (the same `bounds.contains(pointer) &
// pressed_edge` the native `UiSurface::button` proves). Driven across the
// in/out × edge/no-edge combinations, plus an inclusive boundary coordinate.
test("button activation truth table: pointer in/out of bounds × press edge present/absent", () => {
  const host = new FakeHost();
  bindNative(host);
  const ui = makeUi();
  const bounds = { height: 20, width: 20, x: 10, y: 10 };
  const viewport = { height: 100, width: 100 };
  const activate = (x: number, y: number, pressed: boolean): boolean => {
    ui.beginFrame(viewport, { x, y }, pressed);
    return ui.button(bounds, "ok");
  };

  // Inside (15,15) + press edge ⇒ activated.
  assert.equal(activate(15, 15, true), true);
  // Inside but no press edge ⇒ not activated.
  assert.equal(activate(15, 15, false), false);
  // Press edge but pointer outside ⇒ not activated.
  assert.equal(activate(0, 0, true), false);
  // Outside and no edge ⇒ not activated.
  assert.equal(activate(0, 0, false), false);
  // Boundary coordinate (the inclusive top-left corner) on a press edge ⇒ activated.
  assert.equal(activate(10, 10, true), true);
});

// SPEC-09 §7: the presentation-leak structural proof — no presentation (Ui /
// tween / draw) state is reachable through any `Sim` accessor. A HUD reflects sim
// state but never feeds it back (SPEC-09 §6): the surfaces a sim can reach expose
// only write/forwarding verbs, with no read-back getter a fixed update could call.
test("presentation-leak: no Sim/tween accessor returns Ui/tween/draw state to the sim", () => {
  const bridge = new FakeBridge();
  const context: SimContext = { bridge, fixedHz: 60, pump: new TickPump(bridge, 60) };
  const sim = makeSim(context, 0);

  // The Sim exposes exactly its simulation members — no `ui`/`frame`/`draw`
  // surface and no presentation read-back (`drawList`/`viewport`).
  assert.deepEqual(
    Object.keys(sim).toSorted(),
    ["add", "dt", "input", "physics", "rng", "tick", "time", "tweens", "world"],
  );
  const leaks = ["ui", "frame", "draw", "drawList", "viewport"];
  assert.ok(
    leaks.every((name) => !Object.keys(sim).includes(name)),
    "a Sim must expose no presentation accessor",
  );

  // The tween surface a sim holds is push-only: `add`/`cancel` (the eased value is
  // delivered through the author's `onUpdate` closure, never a sim-callable getter).
  assert.deepEqual(Object.keys(sim.tweens).toSorted(), ["add", "cancel"].toSorted());

  // The Ui surface lives behind its own factory, off the sim graph: building one
  // takes the explicit `makeUi()`, and its value-returning members (`viewport` /
  // `drawList`) are the platform edge's read-backs, unreachable from the Sim above.
  const ui = makeUi();
  assert.ok(!Object.values(sim).includes(ui), "the Ui surface is not wired into the Sim");
});
