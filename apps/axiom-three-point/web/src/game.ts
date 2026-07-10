/*
 * game.ts — THE game, wired to the app's own pure-TypeScript engine. The harness
 * calls `initGame` once (build the scene, bind the keys) and then `updateGame`
 * once per fixed tick: it folds this tick's input into a plain `Intent`,
 * advances the deterministic SDK-free `ThreePointSession`, mirrors the result
 * into the 3D scene, and turns the session's audio cues into procedural tones.
 * It exports `readHud()` for the harness's DOM overlay and `configureViewport()`
 * for touch-gesture projection.
 *
 * Desktop: click the court to grab the pointer · mouse aims · hold SPACE to rise
 * into the shot, release at the top · R restarts (Escape releases the pointer).
 *
 * Touch (the swipe-basketball gesture model): drag anywhere to look; a touch
 * that STARTS in the lower-center zone (on the held ball) is a shot gesture —
 * its samples feed a smoothed `PointerHistory`, and lifting off flicks the ball:
 * upward speed maps to the release progress, sideways to a bounded launch-yaw
 * offset. The camera is never moved by a shot gesture.
 */

import type { TickInput } from "./engine/api.ts";
import type { InputState } from "./engine/input.ts";
import { playTone } from "./engine/audio.ts";
import { type SceneHandles, applyFrame, buildScene } from "./scene.ts";
import type { AudioCue, Hud, Intent, SwipeGesture } from "./types.ts";
import { type Vec2, vec2 } from "./vec.ts";
import { GESTURE_REFERENCE_HEIGHT, SHOT_TUNING, SWIPE_ZONE_HALF_X, SWIPE_ZONE_MIN_Y } from "./constants.ts";
import { swipeIntents } from "./gameplay.ts";
import { PointerHistory } from "./pointer.ts";
import { ThreePointSession } from "./session.ts";

let handles: SceneHandles | undefined;
let session = new ThreePointSession();

// Touch-gesture state (platform-fed, read each fixed tick).
let viewport: Vec2 = vec2(960, 540);
let prevDown = false;
let gesture: "look" | "shot" | null = null;
let lastPointer: Vec2 | null = null;
const history = new PointerHistory();

/** Build the scene and bind the key actions. Called once by the harness, after
 * the renderer is initialized. */
export const initGame = (input: InputState): void => {
  input.bindAction("shoot", ["Space"]);
  input.bindAction("restart", ["KeyR"]);
  handles = buildScene();
  session = new ThreePointSession();
  prevDown = false;
  gesture = null;
  lastPointer = null;
  history.clear();
};

/** Fold this tick's pointer-locked mouse, keyboard, and touch gestures into the
 * session `Intent`. A drag outside the shot zone looks; a drag starting in the
 * lower-center zone is a shot whose lift-off flick launches the ball. */
const readIntent = (input: TickInput, tick: number): Intent => {
  const look = input.look();
  let lookDx = look.x;
  let lookDy = look.y;
  let swipe: SwipeGesture | null = null;

  const sample = input.pointer();
  const down = sample !== undefined ? sample.down : false;
  // Gesture velocities are normalized to the reference height so a flick feels
  // the same on a phone-sized canvas and a desktop one.
  const gestureScale = GESTURE_REFERENCE_HEIGHT / Math.max(1, viewport.y);
  if (down && !prevDown && sample !== undefined) {
    const fx = sample.pos.x / Math.max(1, viewport.x);
    const fy = sample.pos.y / Math.max(1, viewport.y);
    gesture = fy >= SWIPE_ZONE_MIN_Y && Math.abs(fx - 0.5) <= SWIPE_ZONE_HALF_X ? "shot" : "look";
    history.clear();
    if (gesture === "shot") history.push(sample.pos.x * gestureScale, sample.pos.y * gestureScale, tick);
    lastPointer = vec2(sample.pos.x, sample.pos.y);
  } else if (down && sample !== undefined && gesture !== null) {
    if (gesture === "shot") {
      history.push(sample.pos.x * gestureScale, sample.pos.y * gestureScale, tick);
    } else if (lastPointer !== null) {
      lookDx += (sample.pos.x - lastPointer.x) * SHOT_TUNING.touchLookScale;
      lookDy += (sample.pos.y - lastPointer.y) * SHOT_TUNING.touchLookScale;
    }
    lastPointer = vec2(sample.pos.x, sample.pos.y);
  } else if (!down && prevDown) {
    if (gesture === "shot") swipe = swipeIntents(history.releaseVelocity());
    gesture = null;
    lastPointer = null;
    history.clear();
  }
  prevDown = down;

  return {
    lookDx,
    lookDy,
    restartPressed: input.pressed("restart"),
    shootHeld: input.isDown("shoot"),
    shootPressed: input.pressed("shoot"),
    shootReleased: input.released("shoot"),
    swipe,
  };
};

/** Map a session audio cue onto the engine's procedural tone synth. */
const playCue = (cue: AudioCue): void => {
  switch (cue.kind) {
    case "charge":
      playTone({ duration: 0.045, freq: 200 + 420 * cue.level, volume: 0.045, wave: "sine" });
      break;
    case "release":
      playTone({ duration: 0.09, freq: 540, volume: 0.12, wave: "sine" });
      break;
    case "contact": {
      const volume = Math.min(0.3, 0.06 + cue.speed * 0.035);
      if (cue.surface === "rim") playTone({ duration: 0.07, freq: 185, volume, wave: "triangle" });
      else if (cue.surface === "backboard") playTone({ duration: 0.08, freq: 120, volume, wave: "square" });
      else if (cue.surface === "floor") playTone({ duration: 0.1, freq: 85, volume, wave: "sine" });
      else playTone({ duration: 0.07, freq: 100, volume, wave: "square" });
      break;
    }
    case "score": {
      const base = cue.swish ? 740 : 620;
      playTone({ duration: 0.28, freq: base + 40 * Math.min(cue.streak, 6), volume: 0.18, wave: "sine" });
      break;
    }
    case "miss":
      playTone({ duration: 0.14, freq: 150, volume: 0.08, wave: "sine" });
      break;
    case "transition":
      playTone({ duration: 0.32, freq: 300, volume: 0.09, wave: "triangle" });
      break;
    case "results":
      playTone({ duration: 0.5, freq: 500, volume: 0.16, wave: "sine" });
      break;
  }
};

/** One fixed 60 Hz tick: input → session → scene + audio. */
export const updateGame = (input: TickInput, tick: number): void => {
  if (handles === undefined) return;
  session.advance(readIntent(input, tick));
  for (const cue of session.drainAudio()) playCue(cue);
  applyFrame(handles, session.view());
};

/** The HUD the harness reads each frame (draining feedback events). */
export const readHud = (): Hud => session.hud();

/** Report the displayed canvas size (CSS px) for touch-gesture projection. */
export const configureViewport = (width: number, height: number): void => {
  viewport = vec2(width, height);
};
