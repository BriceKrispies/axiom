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

import type { TickInput } from "@axiom/web-engine";
import type { InputState } from "@axiom/web-engine";
import { playTone, setAmbienceLevel, startAmbience } from "@axiom/web-engine";
import { type SceneHandles, applyFrame, buildScene, sceneNodeCount } from "./scene.ts";
import type { GameEvent, Hud, Intent, SwipeGesture } from "./types.ts";
import { type Vec2, vec2 } from "./vec.ts";
import { DEBUG_COUNTERS, GESTURE_REFERENCE_HEIGHT, POLISH_TUNING, SHOT_TUNING, SWIPE_ZONE_HALF_X, SWIPE_ZONE_MIN_Y } from "./constants.ts";
import { swipeIntents } from "./gameplay.ts";
import { impactPitch, impactVolume } from "./polish.ts";
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
  startAmbience(POLISH_TUNING.ambientCrowdVolume);
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

let audioPlays = 0;

const tone = (spec: Parameters<typeof playTone>[0]): void => {
  audioPlays += 1;
  playTone(spec);
};

/**
 * The unified event → sound map. Impact volume and pitch come from the shared
 * `impactVolume`/`impactPitch` mappings (normalized collision speed, clamped by
 * POLISH_TUNING) — a soft graze and a hard clank never sound identical. All
 * variation is deterministic (speed, slot, streak); no randomness.
 */
const playEventAudio = (event: GameEvent): void => {
  switch (event.kind) {
    case "ballPickupStarted":
      tone({ duration: 0.05, freq: 300 * (1 + event.slot * 0.04), volume: 0.05, wave: "triangle" });
      break;
    case "chargeTick":
      tone({ duration: 0.045, freq: 200 + 420 * event.level, volume: 0.045, wave: "sine" });
      break;
    case "ballReleased":
      tone({ duration: 0.09, freq: 520 + 60 * event.progress, volume: 0.12, wave: "sine" });
      break;
    case "rimHit":
      tone({ duration: 0.07, freq: 185 * impactPitch(event.speed), volume: impactVolume(event.speed), wave: "triangle" });
      break;
    case "backboardHit":
      tone({ duration: 0.08, freq: 120 * impactPitch(event.speed), volume: impactVolume(event.speed), wave: "square" });
      break;
    case "floorHit":
      tone({ duration: 0.1, freq: 85 * impactPitch(event.speed), volume: impactVolume(event.speed), wave: "sine" });
      break;
    case "basketMade":
      if (!event.swish) {
        tone({ duration: 0.26, freq: 620 + 40 * Math.min(event.streak, 6), volume: 0.17, wave: "sine" });
      }
      // The score-award blip rides every make.
      tone({ delay: 0.06, duration: 0.06, freq: 1046, volume: 0.06, wave: "sine" });
      break;
    case "swishMade":
      // The clean-net figure: a warm body plus a bright, short sparkle.
      tone({ duration: 0.3, freq: 740, volume: 0.2, wave: "sine" });
      tone({ delay: 0.05, duration: 0.08, freq: 1480, volume: 0.07, wave: "triangle" });
      break;
    case "shotMissed":
      tone({ duration: 0.14, freq: 150, volume: 0.08, wave: "sine" });
      break;
    case "streakIncreased":
      if (event.streak >= 2) {
        tone({ duration: 0.07, freq: 500 + 70 * Math.min(event.streak, 7), volume: 0.07, wave: "sine" });
      }
      break;
    case "streakBroken":
      tone({ duration: 0.12, freq: 392, volume: 0.09, wave: "sine" });
      tone({ delay: 0.09, duration: 0.16, freq: 262, volume: 0.07, wave: "sine" });
      break;
    case "stationTransitionStarted":
      tone({ duration: 0.32, freq: 300, volume: 0.09, wave: "triangle" });
      break;
    case "stationTransitionCompleted":
      tone({ duration: 0.12, freq: 523, volume: event.final ? 0.13 : 0.1, wave: "sine" });
      if (event.final) tone({ delay: 0.1, duration: 0.16, freq: 659, volume: 0.12, wave: "sine" });
      break;
    case "gameCompleted":
      tone({ duration: 0.2, freq: 523, volume: 0.14, wave: "sine" });
      tone({ delay: 0.15, duration: 0.35, freq: 784, volume: 0.15, wave: "sine" });
      break;
    case "rackCompleted":
    case "gameRestarted":
    default:
      break;
  }
};

/** One fixed 60 Hz tick: input → session → events (audio) → scene. */
export const updateGame = (input: TickInput, tick: number): void => {
  if (handles === undefined) return;
  session.advance(readIntent(input, tick));
  for (const event of session.drainGameEvents()) playEventAudio(event);
  const view = session.view();
  // Quiet arena bed, swelling gently with the crowd reaction.
  setAmbienceLevel(POLISH_TUNING.ambientCrowdVolume * (1 + 1.6 * view.crowdPulse));
  applyFrame(handles, view);
  if (DEBUG_COUNTERS && tick % 60 === 0) {
    console.log(
      `three-point counters: effects=${session.activeEffects()} trails=${session.activeTrailSamples()} ` +
        `audio/s=${audioPlays} nodes=${sceneNodeCount()}`,
    );
    audioPlays = 0;
  }
};

/** The HUD the harness reads each frame (draining feedback events). */
export const readHud = (): Hud => session.hud();

/** Report the displayed canvas size (CSS px) for touch-gesture projection. */
export const configureViewport = (width: number, height: number): void => {
  viewport = vec2(width, height);
};
