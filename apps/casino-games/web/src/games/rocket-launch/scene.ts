/*
 * scene.ts — Rocket Launch presentation: a cheerful toy pad below a ring of
 * destination planets (sized/ringed/colored by tier for wins, gray-blue moons
 * for losses — none highlighted as chosen before launch). The rocket idles with
 * a tiny wobble and blinking pad lights; a held countdown ignites three lights
 * and grows a flame; the reveal flies the rocket up, orbits it onto the committed
 * planet, docks with a flag pop, and reveals the reward. The camera target rises
 * with the rocket. Pure view: returns a Scene value.
 */

import type { EngineVec3, GameResources, MaterialSpec, Rgba, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { RARITY_COLORS, REWARD_MATERIALS, rewardBeam, rewardProp } from "../../presentation/rewards/tiers.ts";
import { bob, clamp01, easeOutBack, lerp, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatRoll, rotateByQuat, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { RocketSpec, RocketState } from "./game.ts";
import {
  chargeStrength,
  committedPlanetIndex,
  dockPoint,
  exhaustOrigin,
  flightPhaseAt,
  flightTimeline,
  ignitedLights,
  PAD,
  planetPosition,
  rocketHeading,
  rocketPosition,
} from "./game.ts";

const SPACE_CLEAR: Rgba = [0.06, 0.08, 0.16, 1];

const rarityColorOf = (runtime: GameRuntime<RocketSpec>, tierId: string | null): Rgba => {
  const tier = runtime.config.rewardTiers.find((t) => t.id === tierId);
  return tier === undefined ? [0.55, 0.62, 0.75, 1] : RARITY_COLORS[tier.rarity];
};

/** Declared once per mount: primitives + per-planet body/ring materials. */
export const rocketResources = (runtime: GameRuntime<RocketSpec>): GameResources => {
  const spec = runtime.config.gameSpecific;
  const materials: Record<string, MaterialSpec> = {
    ...STAGE_MATERIALS,
    ...REWARD_MATERIALS,
    ...CONFETTI_MATERIALS,
    Fin: { baseColor: [0.95, 0.4, 0.35, 1], emissive: [0.2, 0.06, 0.05, 1] },
    Flag: { baseColor: [1, 0.85, 0.35, 1], emissive: [0.4, 0.3, 0.08, 1] },
    FlagPole: { baseColor: [0.85, 0.88, 0.95, 1] },
    Flame: { baseColor: [1, 0.7, 0.25, 1], emissive: [1, 0.55, 0.15, 1] },
    FlameCore: { baseColor: [1, 0.95, 0.6, 1], emissive: [1, 0.9, 0.5, 1] },
    Moon: { baseColor: [0.55, 0.62, 0.75, 1], emissive: [0.1, 0.12, 0.16, 1] },
    PadBase: { baseColor: [0.5, 0.55, 0.68, 1], emissive: [0.06, 0.07, 0.1, 1] },
    PadLightOff: { baseColor: [0.4, 0.42, 0.5, 1], emissive: [0.04, 0.04, 0.05, 1] },
    PadLightOn: { baseColor: [1, 0.85, 0.4, 1], emissive: [1, 0.65, 0.2, 1] },
    RocketBody: { baseColor: [0.96, 0.97, 1, 1], emissive: [0.22, 0.24, 0.3, 1] },
    RocketNose: { baseColor: [1, 0.55, 0.42, 1], emissive: [0.35, 0.14, 0.1, 1] },
    Smoke: { baseColor: [0.85, 0.88, 0.95, 1], emissive: [0.2, 0.22, 0.26, 1], opacity: 0.4 },
  };
  spec.planets.forEach((planet, i) => {
    const color = planet.tierId === null ? ([0.55, 0.62, 0.75, 1] as Rgba) : rarityColorOf(runtime, planet.tierId);
    materials[`planet${i}`] = { baseColor: color, emissive: [color[0] * 0.4, color[1] * 0.4, color[2] * 0.4, 1] };
  });
  return {
    materials,
    meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
  };
};

/** The rocket's current world position + heading for the active phase. */
interface RocketPose {
  readonly position: EngineVec3;
  readonly heading: number;
  readonly age: number;
}

const rocketPose = (state: RocketState, index: number, count: number, seed: number): RocketPose => {
  const session = state.session;
  const timeline = flightTimeline(session.config.presentationSpeed, false);
  if (session.phase === "revealing") {
    const age = phaseAge(session);
    return {
      age,
      heading: rocketHeading(age, index, count, seed, session.round, timeline),
      position: rocketPosition(age, index, count, seed, session.round, timeline),
    };
  }
  if (session.phase === "celebrating" || session.phase === "complete") {
    return { age: timeline.total, heading: Math.PI / 2, position: dockPoint(index, count) };
  }
  // Idle on the pad with a tiny wobble.
  const wobble = v3(bob(session.tick, 90) * 0.04, 0, 0);
  return { age: -1, heading: Math.PI / 2, position: addV3(PAD, wobble) };
};

export const rocketScene = (runtime: GameRuntime<RocketSpec>, state: RocketState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const count = spec.planets.length;
  const index = committedPlanetIndex(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const timeline = flightTimeline(session.config.presentationSpeed, false);
  const pose = rocketPose(state, index, count, seed);
  const phase = pose.age >= 0 ? flightPhaseAt(pose.age, timeline) : null;

  // Planets: distinct by tier, none marked as chosen before launch.
  const planets: SceneInstance[] = [];
  spec.planets.forEach((planet, i) => {
    const at = planetPosition(i, count);
    const win = planet.tierId !== null;
    const size = win ? 0.62 + 0.12 * i / Math.max(1, count - 1) : 0.5;
    planets.push({
      key: `planet${i}`,
      material: win ? `planet${i}` : "Moon",
      mesh: "sphere",
      transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(size, size, size) },
    });
    if (win) {
      planets.push({
        key: `ring${i}`,
        material: "StageGold",
        mesh: "cylinder",
        transform: { position: at, rotation: quatRoll(0.5 + bob(session.tick, 200, i) * 0.1), scale: v3(size * 2.4, 0.04, size * 2.4) },
      });
    }
  });

  // The pad + three countdown lights.
  const pad: SceneInstance[] = [
    { key: "pad:base", material: "PadBase", mesh: "cylinder", transform: { position: v3(PAD.x, 0.12, PAD.z), rotation: QUAT_IDENTITY, scale: v3(1.1, 0.24, 1.1) } },
    { key: "pad:mastL", material: "PadBase", mesh: "box", transform: { position: v3(-0.42, 0.7, 0), rotation: QUAT_IDENTITY, scale: v3(0.08, 1.1, 0.08) } },
    { key: "pad:mastR", material: "PadBase", mesh: "box", transform: { position: v3(0.42, 0.7, 0), rotation: QUAT_IDENTITY, scale: v3(0.08, 1.1, 0.08) } },
  ];
  const charge = state.extra.chargeTicks;
  const idleBlink = session.phase === "ready" && charge === 0;
  for (let i = 0; i < 3; i += 1) {
    const lit = session.phase === "ready"
      ? (charge > 0 ? i < ignitedLights(charge) : (i + Math.floor(session.tick / 20)) % 3 === 0)
      : idleBlink;
    pad.push({
      key: `padlight${i}`,
      material: lit ? "PadLightOn" : "PadLightOff",
      mesh: "sphere",
      transform: { position: v3(-0.42 + i * 0.42, 0.28, 0.55), rotation: QUAT_IDENTITY, scale: v3(0.12, 0.12, 0.12) },
    });
  }

  // The rocket (body + nose + two fins), oriented along its heading.
  const q = quatRoll(pose.heading - Math.PI / 2);
  const part = (suffix: string, local: EngineVec3, scale: EngineVec3, material: string): SceneInstance => ({
    key: `rocket:${suffix}`,
    material,
    mesh: suffix === "nose" ? "sphere" : suffix.startsWith("fin") ? "box" : "cylinder",
    transform: { position: addV3(pose.position, rotateByQuat(local, q)), rotation: q, scale },
  });
  const rocket: SceneInstance[] = [
    part("body", v3(0, 0, 0), v3(0.3, 0.85, 0.3), "RocketBody"),
    part("nose", v3(0, 0.5, 0), v3(0.32, 0.32, 0.32), "RocketNose"),
    part("finL", v3(-0.22, -0.35, 0), v3(0.16, 0.28, 0.06), "Fin"),
    part("finR", v3(0.22, -0.35, 0), v3(0.16, 0.28, 0.06), "Fin"),
  ];

  // Flame + smoke: countdown flame while charging, exhaust during liftoff.
  const effects: SceneInstance[] = [];
  const chargingFlame = session.phase === "ready" && charge > 0 ? chargeStrength(charge) : 0;
  const flying = phase === "liftoff" || phase === "orbit";
  const flameScale = chargingFlame > 0 ? 0.2 + chargingFlame * 0.5 : flying ? (phase === "liftoff" ? 0.7 : 0.35) : 0;
  if (flameScale > 0) {
    const origin = exhaustOrigin(pose.position, pose.heading);
    for (let k = 0; k < 4; k += 1) {
      const s = flameScale * (1 - k * 0.2);
      effects.push({
        key: `flame${k}`,
        material: k === 0 ? "FlameCore" : "Flame",
        mesh: "sphere",
        transform: {
          position: addV3(origin, rotateByQuat(v3(0, -k * 0.18 * flameScale, 0), q)),
          rotation: QUAT_IDENTITY,
          scale: v3(s * (0.24 + pulse((session.tick % 8) / 8) * 0.06), s * 0.3, s * 0.24),
        },
      });
    }
  }
  if (phase === "liftoff") {
    effects.push(...sparkleRing("smoke", v3(PAD.x, 0.3, PAD.z), 6, seed, pose.age, timeline.liftoff).map((puff) => ({ ...puff, material: "Smoke" })));
  }

  // Reward reveal + flag pop at the docked planet.
  const reveal: SceneInstance[] = [];
  const plan = session.committed;
  if (plan !== null && (phase === "dock" || phase === "reveal" || session.phase === "celebrating" || session.phase === "complete")) {
    const rarity = outcomeRarity(session);
    const dockAge = pose.age - (timeline.liftoff + timeline.orbit);
    const pop = easeOutBack(clamp01(dockAge / Math.max(1, timeline.dock)));
    const at = dockPoint(index, count);
    reveal.push(
      { key: "flag:pole", material: "FlagPole", mesh: "cylinder", transform: { position: v3(at.x + 0.18, at.y + pop * 0.25, at.z), rotation: QUAT_IDENTITY, scale: v3(0.03, 0.5 * pop, 0.03) } },
      { key: "flag:cloth", material: plan.win ? "Flag" : "TryAgain", mesh: "box", transform: { position: v3(at.x + 0.32, at.y + 0.32 * pop, at.z), rotation: QUAT_IDENTITY, scale: v3(0.26 * pop, 0.16 * pop, 0.02) } },
    );
    if (plan.win && rarity !== "loss") {
      reveal.push(rewardBeam("beam", v3(at.x, at.y + 0.2, at.z), pop), ...rewardProp("reward", rarity, v3(at.x, at.y + 0.5, at.z), pop, session.tick, 0.85));
    } else {
      reveal.push({ key: "spacerock", material: "Moon", mesh: "box", transform: { position: v3(at.x, at.y + 0.35, at.z), rotation: quatRoll(bob(session.tick, 24) * 0.4), scale: v3(0.26, 0.24, 0.24) } });
    }
  }

  // Celebration at the planet.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(dockPoint(index, count), v3(0, 0.5, 0.2));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Camera rises with the rocket (less follow under reduced motion).
  const follow = runtime.settings.reducedMotion ? 0.35 : 0.7;
  const camY = lerp(2.2, pose.position.y, follow);
  const lights: SceneLight[] = [...stageLights(v3(0, camY, 1.5), 0.7)];
  if (phase === "reveal" || session.phase === "celebrating") {
    const at = dockPoint(index, count);
    lights.push({ key: "light:dock", light: { color: [1, 0.9, 0.6, 1], intensity: 1.2, kind: "point", position: v3(at.x, at.y + 0.4, at.z + 0.8) } });
  }

  return {
    camera: showcaseCamera(v3(0, camY, 0), 8, 0.4, 0.92),
    clearColor: session.phase === "intro" || session.phase === "ready" ? SKY_CLEAR : SPACE_CLEAR,
    instances: [...stageRoom(20), ...planets, ...pad, ...rocket, ...effects, ...reveal, ...celebration],
    lights,
  };
};
