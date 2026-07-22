/*
 * harness.ts — the browser boot / platform edge for Arena Forge. It sizes the
 * single gameplay canvas (device-pixel-ratio aware, resize-aware), wires pointer
 * (touch + mouse, one path) into the game, and drives the fixed-step loop from
 * Axiom's `@axiom/web-engine` (`startLoop`). This is the ONLY file that touches
 * the DOM and the wall clock; the game and the whole simulation below it are
 * deterministic and platform-free. The two dev-server anchors (the versioned
 * import and the `/events` SSE channel) match the other Axiom apps so the
 * packager/hot-reload can rewrite them.
 */

import { initRenderer, resizeRenderer, startLoop } from "@axiom/web-engine";
import type { BackendChoice } from "@axiom/web-engine";
import { FIXED_HZ } from "./sim/tuning.ts";
import { ArenaForgeGame } from "./game.ts";

const CANVAS_ID = "arena-canvas";
const SCENE_ID = "arena-scene";
const MAX_STEPS = 8;

/** `?backend=canvas2d|webgl2|auto` pins the 3D backend (canvas2d = the
 * deterministic baseline the filmstrip test uses). */
const backendFromUrl = (): BackendChoice => {
  const q = new URLSearchParams(globalThis.location?.search ?? "").get("backend");
  return q === "canvas2d" || q === "webgl2" ? q : "auto";
};

const boot = (): void => {
  const canvas = document.getElementById(CANVAS_ID) as HTMLCanvasElement | null;
  const sceneCanvas = document.getElementById(SCENE_ID) as HTMLCanvasElement | null;
  if (canvas === null || sceneCanvas === null) {
    return;
  }
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    return;
  }
  // The base canvas hosts the engine's 3D scene (WebGL2 or the Canvas2D baseline).
  initRenderer(sceneCanvas, backendFromUrl());

  // A seed chosen at the host edge (the wall clock never reaches the sim).
  const seed = (Math.floor(Date.now() % 1_000_000) ^ 0x5f3d) >>> 0;
  const game = new ArenaForgeGame(seed);
  // A handle for the deterministic browser interaction test (dev only).
  (globalThis as unknown as { __arena: ArenaForgeGame }).__arena = game;

  let cssW = 0;
  let cssH = 0;
  const resize = (): void => {
    const rect = canvas.getBoundingClientRect();
    const dpr = Math.min(3, globalThis.devicePixelRatio || 1);
    cssW = Math.max(320, rect.width);
    cssH = Math.max(240, rect.height);
    canvas.width = Math.floor(cssW * dpr);
    canvas.height = Math.floor(cssH * dpr);
    sceneCanvas.width = canvas.width;
    sceneCanvas.height = canvas.height;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    resizeRenderer(sceneCanvas.width, sceneCanvas.height);
  };
  resize();
  globalThis.addEventListener("resize", resize);

  // Pointer: down on the canvas; move/up on the window so a drag can leave it.
  // Two active pointers become a PINCH (zoom); a single pointer is a drag/tap.
  const local = (e: PointerEvent): [number, number] => {
    const rect = canvas.getBoundingClientRect();
    return [e.clientX - rect.left, e.clientY - rect.top];
  };
  const pointers = new Map<number, { x: number; y: number }>();
  let pinchDist = 0;
  const twoFingerDist = (): number => {
    const p = [...pointers.values()];
    return p.length >= 2 ? Math.hypot((p[0] as { x: number }).x - (p[1] as { x: number }).x, (p[0] as { y: number }).y - (p[1] as { y: number }).y) : 0;
  };
  canvas.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    const [x, y] = local(e);
    pointers.set(e.pointerId, { x, y });
    if (pointers.size === 1) {
      game.onPointerDown(x, y);
    } else if (pointers.size === 2) {
      game.onPointerUp(x, y); // cancel any single-finger drag before pinching
      pinchDist = twoFingerDist();
    }
  });
  globalThis.addEventListener("pointermove", (e) => {
    const [x, y] = local(e);
    if (pointers.has(e.pointerId)) {
      pointers.set(e.pointerId, { x, y });
    }
    if (pointers.size >= 2) {
      const d = twoFingerDist();
      if (pinchDist > 0 && d > 0) {
        game.onPinch(d / pinchDist);
      }
      pinchDist = d;
    } else {
      game.onPointerMove(x, y);
    }
  });
  const endPointer = (e: PointerEvent): void => {
    const [x, y] = local(e);
    pointers.delete(e.pointerId);
    if (pointers.size === 0) {
      game.onPointerUp(x, y);
    } else {
      pinchDist = 0; // a lifted finger during pinch resets the pinch baseline
    }
  };
  globalThis.addEventListener("pointerup", endPointer);
  globalThis.addEventListener("pointercancel", endPointer);
  // Keyboard: printable characters + the few named keys a canvas text field needs.
  // Modifier chords are left to the browser (never swallow Ctrl/Cmd shortcuts).
  globalThis.addEventListener("keydown", (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) {
      return;
    }
    const named = e.key === "Backspace" || e.key === "Escape" || e.key === "Enter";
    if (named || e.key.length === 1) {
      if (named) {
        e.preventDefault();
      }
      game.onKey(e.key);
    }
  });

  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    game.onWheel(e.deltaY);
  }, { passive: false });

  startLoop({
    fixedHz: FIXED_HZ,
    maxCatchUpSteps: MAX_STEPS,
    update: () => game.update(),
    render: () => {
      game.renderScene3D();
      game.render(ctx, cssW, cssH);
    },
  });
};

boot();
