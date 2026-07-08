/*
 * THE GAME — a retro 32-bit soccer penalty shootout authored ENTIRELY in
 * TypeScript on @axiom/game. No Rust in this app: the fixed diorama, the aim +
 * power interaction, the physics-arc ball flight, the diving goalie with save
 * volumes, goal/save/miss/post resolution, the 5-round scoring session, the
 * impact effects, and the run-up-and-strike kicker are all the modules in this
 * folder, driving the shared engine through the SDK's 3D scene, input, and camera
 * surfaces. The engine renders the retained scene; this file wires the sim to it.
 *
 * Controls: ←/→ or A/D aim horizontally · ↑/↓ or W/S aim vertically · hold
 * Space/K to charge power, release to shoot · Enter continue · R reset.
 */

import { type Sim, bindAction, onFixedUpdate } from "@axiom/game";
import { type FrameSnapshot, type SceneHandles, applyFrame, buildScene, setCamera } from "./scene.ts";
import { type SessionState, sessionAdvance, sessionCameraOffset, sessionEffectDescriptor, sessionNew } from "./session.ts";
import { type PenaltyInputIntent, makeIntent } from "./input.ts";
import { type Vec3, vec3 } from "./engine.ts";
import { ballPose } from "./interaction.ts";
import { goalieRenderWorld } from "./goalie.ts";
import { kickerFrameTick } from "./kicker.ts";
import { type HudModel, readHudModel } from "./hud.ts";
import { BALL_RADIUS } from "./scene-constants.ts";

const bindKeys = (): void => {
  bindAction("aimLeft", ["ArrowLeft", "KeyA"]);
  bindAction("aimRight", ["ArrowRight", "KeyD"]);
  bindAction("aimUp", ["ArrowUp", "KeyW"]);
  bindAction("aimDown", ["ArrowDown", "KeyS"]);
  bindAction("shoot", ["Space", "KeyK"]);
  bindAction("reset", ["KeyR"]);
  bindAction("continue", ["Enter"]);
};

const readIntent = (sim: Sim): PenaltyInputIntent =>
  makeIntent({
    aimXAxis: (Number(sim.input.isDown("aimRight")) - Number(sim.input.isDown("aimLeft"))) * 100,
    aimYAxis: (Number(sim.input.isDown("aimUp")) - Number(sim.input.isDown("aimDown"))) * 100,
    chargePressed: sim.input.isDown("shoot"),
    releasePressed: sim.input.released("shoot"),
    resetPressed: sim.input.pressed("reset"),
    continuePressed: sim.input.pressed("continue"),
  });

/** The flight progress [0,1] the kicker's strike-through maps against. */
const flightProgress = (session: SessionState): number => {
  const flight = session.shot.flight;
  return flight ? Math.min(flight.elapsedTicks / flight.trajectory.totalTicks, 1) : 0;
};

/** Build the per-frame scene snapshot from the session state. */
const snapshotOf = (session: SessionState): FrameSnapshot => {
  const shot = session.shot;
  const base = ballPose(shot);
  const effect = sessionEffectDescriptor(session);

  // Save/Miss drift the ball off its trajectory endpoint; override its position.
  let ball = base;
  if (effect?.ballDeflection) {
    const p = effect.ballDeflection.current;
    const height = Math.max(p.y - BALL_RADIUS, 0);
    const factor = 1 / (1 + height * 0.5);
    ball = {
      position: p,
      radius: BALL_RADIUS,
      shadowCenter: vec3(p.x, 0.03, p.z),
      shadowRadiusX: BALL_RADIUS * 1.1 * factor,
      shadowRadiusZ: BALL_RADIUS * factor,
      trail: [],
      trailLen: 0,
    };
  }

  const flash = effect?.foreground[0] ?? null;

  return {
    ball,
    goalieWorld: goalieRenderWorld(shot.goalie),
    kickerTick: kickerFrameTick({ state: shot.state, chargeTicks: shot.chargeTicks, flightProgress: flightProgress(session) }),
    frameShake: effect?.frameShake ?? null,
    saveFlash: flash ? { position: flash.position, size: flash.size, alpha: flash.alpha } : null,
  };
};

// Live game state — undefined until the first fixed tick builds the scene (which
// needs the host channel, bound by `boot` before the first advance).
let handles: SceneHandles | undefined;
let session: SessionState = sessionNew();

onFixedUpdate((sim: Sim): void => {
  if (handles === undefined) {
    bindKeys();
    handles = buildScene();
    session = sessionNew();
  }
  session = sessionAdvance(session, readIntent(sim));
  const offset: Vec3 = sessionCameraOffset(session);
  setCamera(offset);
  applyFrame(handles, snapshotOf(session));
});

/** The HUD the harness reads each frame to update the DOM overlay. */
export const readHud = (): HudModel => readHudModel(session);
