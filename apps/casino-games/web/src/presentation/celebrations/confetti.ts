/*
 * confetti.ts — stateless celebration particles. Every particle's pose is an
 * ANALYTIC function of (presentationSeed, particle index, age): a ballistic
 * arc with spin, drawn from the PARTICLES stream only. Nothing is allocated
 * per frame beyond the instance list the view already returns, no particle
 * state is stored, counts are bounded by `celebrationOf`, and — because the
 * stream is decorative — confetti can never influence an outcome.
 */

import type { SceneInstance } from "@axiom/web-engine";
import type { EngineVec3 } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { MaterialSpec } from "@axiom/web-engine";
import { quatAxisAngle, v3 } from "../stage/vectors.ts";

/** Spread into a game's materials to use `confettiBurst` / `sparkleRing`. */
export const CONFETTI_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ConfettiCoral: { baseColor: [1, 0.45, 0.4, 1], emissive: [0.45, 0.14, 0.12, 1] },
  ConfettiGold: { baseColor: [1, 0.82, 0.3, 1], emissive: [0.5, 0.38, 0.1, 1] },
  ConfettiLavender: { baseColor: [0.75, 0.6, 1, 1], emissive: [0.3, 0.22, 0.5, 1] },
  ConfettiMint: { baseColor: [0.5, 1, 0.75, 1], emissive: [0.16, 0.45, 0.28, 1] },
  ConfettiSky: { baseColor: [0.45, 0.8, 1, 1], emissive: [0.14, 0.3, 0.45, 1] },
  Sparkle: { baseColor: [1, 1, 0.9, 1], emissive: [1, 0.95, 0.7, 1] },
};

const CONFETTI_NAMES = ["ConfettiCoral", "ConfettiGold", "ConfettiLavender", "ConfettiMint", "ConfettiSky"];

/**
 * A confetti burst from `origin`. `ageTicks` counts since the burst began;
 * pieces launch upward in a cone, fall under gravity, spin, and vanish after
 * `lifeTicks`. Count is already resolved/bounded by the caller.
 */
export const confettiBurst = (
  keyPrefix: string,
  origin: EngineVec3,
  count: number,
  presentationSeed: number,
  ageTicks: number,
  lifeTicks = 110,
  spread = 1,
): readonly SceneInstance[] => {
  if (ageTicks < 0 || ageTicks > lifeTicks || count <= 0) {
    return [];
  }
  const t = ageTicks / 60;
  return Array.from({ length: count }, (_, i) => {
    const angle = sample01(presentationSeed, "particles", i, 0) * Math.PI * 2;
    const speed = 1.4 + sample01(presentationSeed, "particles", i, 1) * 2.2;
    const up = 2.2 + sample01(presentationSeed, "particles", i, 2) * 2.4;
    const spinAxis = v3(
      sample01(presentationSeed, "particles", i, 3) - 0.5,
      sample01(presentationSeed, "particles", i, 4) - 0.5,
      sample01(presentationSeed, "particles", i, 5) - 0.5,
    );
    const axisLen = Math.sqrt(spinAxis.x ** 2 + spinAxis.y ** 2 + spinAxis.z ** 2) || 1;
    const fade = 1 - ageTicks / lifeTicks;
    return {
      key: `${keyPrefix}:${i}`,
      material: CONFETTI_NAMES[i % CONFETTI_NAMES.length] as string,
      mesh: "box",
      transform: {
        position: v3(
          origin.x + Math.cos(angle) * speed * t * spread,
          origin.y + (up * t - 4.2 * t * t) * spread,
          origin.z + Math.sin(angle) * speed * t * spread,
        ),
        rotation: quatAxisAngle(
          v3(spinAxis.x / axisLen, spinAxis.y / axisLen, spinAxis.z / axisLen),
          ageTicks * (0.12 + sample01(presentationSeed, "particles", i, 6) * 0.2),
        ),
        scale: v3(0.075 * fade + 0.01, 0.11 * fade + 0.01, 0.02),
      },
    };
  });
};

/** A gentle expanding ring of sparkles (selection feedback, loss response). */
export const sparkleRing = (
  keyPrefix: string,
  origin: EngineVec3,
  count: number,
  presentationSeed: number,
  ageTicks: number,
  lifeTicks = 40,
): readonly SceneInstance[] => {
  if (ageTicks < 0 || ageTicks > lifeTicks || count <= 0) {
    return [];
  }
  const t = ageTicks / lifeTicks;
  const radius = 0.25 + t * 0.9;
  return Array.from({ length: count }, (_, i) => {
    const angle = (i / count) * Math.PI * 2 + sample01(presentationSeed, "particles", i, 7) * 0.5;
    const size = 0.05 * (1 - t) + 0.012;
    return {
      key: `${keyPrefix}:${i}`,
      material: "Sparkle",
      mesh: "sphere",
      transform: {
        position: v3(origin.x + Math.cos(angle) * radius, origin.y + t * 0.5, origin.z + Math.sin(angle) * radius),
        rotation: [0, 0, 0, 1],
        scale: v3(size, size, size),
      },
    };
  });
};
