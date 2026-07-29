/*
 * prize.ts — the contract every treasure a chest can yield is written against.
 *
 * A prize is a small assembly of the engine's box / sphere / cylinder primitives
 * that rises out of the opened chest and hovers as the frame's subject. There
 * are five of them (see `PRIZE_KINDS`), and which one a chest holds is decided
 * by the round's reward tier — never by the presentation. This file owns only
 * the SHAPE of that job: local space, the placement helper, the shared metal
 * palette, and what a prize must declare about itself. The prizes themselves
 * live one per file beside it.
 *
 * ── prize-local space ──────────────────────────────────────────────────────
 * Every prize is authored in a unit box centered on the origin: roughly ±1 on
 * each axis, +Y up, +Z toward the camera. `place` is the ONLY way a part gets
 * into the world — it scales local units by the frame's `size`, welds the part
 * to the prize's slow turn, and drops it at the frame's `center`. So a prize
 * author writes plain readable numbers ("the bar's base is 1.5 wide and 0.34
 * tall") and never touches world coordinates, the hero scale, or the spin.
 *
 * The unit box is a real budget, not a suggestion: `extent` is how far the
 * assembly actually reaches, and the reveal's framing test multiplies it by the
 * hero scale to prove the prize stays on screen through the overshoot of its
 * climb. A prize that reaches past what it declares will fail that test.
 */

import type { EngineQuat, EngineVec3, MaterialSpec, Rgba, SceneInstance } from "@axiom/web-engine";
import { QUAT_IDENTITY, addV3, quatMul, rotateByQuat, scaleV3, v3 } from "../../../presentation/stage/vectors.ts";

/** The five treasures a chest can hold. The id doubles as the reward tier id
 * that selects it (see `definition.ts`), so this list IS the prize vocabulary. */
export type PrizeKind = "gold-bar" | "crab-bride" | "wedding-ring" | "leather-boot" | "gold-coin";

export const PRIZE_KINDS: readonly PrizeKind[] = ["gold-bar", "crab-bride", "wedding-ring", "leather-boot", "gold-coin"];

/**
 * World units one prize-local unit is worth, at hero scale, before the reveal's
 * own damping. This is what makes the catalog interchangeable: every treasure is
 * authored inside the same unit box and arrives on screen at the same size, so
 * swapping a boot for a ring changes the object and nothing about the shot.
 *
 * Set against the faceted gem this catalog replaced, which reached ~0.32 of the
 * same units from its centre — a treasure filling the box now reads a little
 * over a third larger, which is where a hand-sized object (a bar, a boot, a
 * ring) wants to sit in this framing.
 */
export const PRIZE_SIZE = 0.52;

/** The meshes a prize may draw with — exactly the primitives the chest scene
 * already declares as resources. A prize introduces no new mesh. */
export type PrizeMesh = "box" | "sphere" | "cylinder";

/** Where and how big a prize is drawn this frame, and the clocks it may move on. */
export interface PrizeFrame {
  /** World point the prize hovers at (the chest's mouth plus its climb). */
  readonly center: EngineVec3;
  /** World units one prize-local unit is worth. */
  readonly size: number;
  /** The prize's slow presentation turn. `place` welds every part to it. */
  readonly spin: EngineQuat;
  /** The session tick — the only clock a prize may animate on. */
  readonly tick: number;
  /** 0 while the prize is still climbing out, 1 once it has arrived. Idle
   * flourishes (sparkles, a claw wave) ramp in on this so nothing twinkles
   * while the object is still emerging. */
  readonly settle: number;
}

/**
 * Put one part of a prize into the world. `offset` and `scale` are in
 * PRIZE-LOCAL units; `rotation` is the part's own tilt in prize-local space and
 * is composed with (not replaced by) the prize's turn.
 */
export type PrizePlace = (
  suffix: string,
  mesh: PrizeMesh,
  material: string,
  offset: EngineVec3,
  scale: EngineVec3,
  rotation?: EngineQuat,
) => SceneInstance;

/**
 * How a treasure is presented to a camera that looks DOWN on it.
 *
 * This game's camera sits ~50° above the horizontal, so an object authored
 * standing upright in +Y is seen largely from ABOVE — which is how the first
 * pass of this catalog failed: the ring lay nearly edge-on and read as a
 * lollipop, and the crab bride read as a pink blob seen from the top. Leaning
 * every prize back into the camera's own elevation is the fix, and it belongs
 * HERE rather than in five separate files, because it is a fact about the shot,
 * not about any of the objects.
 *
 * The two modes differ in what the lean is protecting:
 *
 *   * `turntable` — a solid object (a bar, a boot, a crab). It reads from every
 *     side, so it turns slowly all the way round, and leans back only PARTLY
 *     (`lean`), because tipping a standing object all the way into the lens
 *     makes it look like it is falling over rather than being presented.
 *   * `faces-camera` — an object whose whole subject is one FACE (a struck coin,
 *     a ring seen through its band). It leans fully into the camera and does not
 *     revolve at all: a full turntable yaw would carry it edge-on for half of
 *     every revolution, and an object that vanishes twice a second is a bug
 *     wearing a flourish. It rocks gently instead, which shows the relief moving
 *     under the light without ever losing the face.
 */
export type PrizePresentation = "turntable" | "faces-camera";

/** What one prize declares about itself. */
export interface Prize {
  /** How this treasure meets the camera — see `PrizePresentation`. */
  readonly presentation: PrizePresentation;
  /** For a `turntable` prize, the share of the camera's downward look it leans
   * back into, in [0, 1]: 0 stands bolt upright, 1 lies face-on to the lens.
   * Ignored by a `faces-camera` prize, which is always fully leaned. */
  readonly lean: number;
  /** The materials this prize needs, merged into the scene's resource set.
   * Names are prefixed `Prize…` so a prize can never shadow a chest or beach
   * material — the merge is asserted collision-free by the prize test. */
  readonly materials: Readonly<Record<string, MaterialSpec>>;
  /** How far the assembly reaches from its centre, in prize-local units. The
   * framing test holds the prize to this, so declare the real reach. */
  readonly extent: number;
  readonly build: (place: PrizePlace, frame: PrizeFrame) => readonly SceneInstance[];
}

/** The placement helper for one prize instance — see "prize-local space". */
export const prizePlace = (keyPrefix: string, frame: PrizeFrame): PrizePlace =>
  (suffix, mesh, material, offset, scale, rotation = QUAT_IDENTITY): SceneInstance => ({
    key: `${keyPrefix}:${suffix}`,
    material,
    mesh,
    transform: {
      position: addV3(frame.center, rotateByQuat(scaleV3(offset, frame.size), frame.spin)),
      rotation: quatMul(frame.spin, rotation),
      scale: scaleV3(scale, frame.size),
    },
  });

// ── the shared metal palette ───────────────────────────────────────────────
/*
 * Two prizes are gold (the bar and the coin) and a third is set with a diamond,
 * so the metals live here rather than being re-invented per file — a bar and a
 * coin that disagree about what gold looks like would read as two different
 * substances in the same chest.
 *
 * All of it is authored for the REVEAL rig, which is the only light a prize is
 * ever seen under: the sun, the sky, and one warm kiss, summing to ~1.25 on an
 * up-facing surface. That is the constraint that sets the top of the ladder —
 * an albedo above ~0.8 multiplies past the tone curve's knee, and once two
 * channels clip together the hue is gone by construction and the gold reads as
 * white. So the brightest rung sits at 0.78 and the value STEP between rungs
 * (top face → front → side → deep) does the work of making it read as metal,
 * exactly as the chest's own gilding ladder does.
 *
 * The sparkle is the one exception and is deliberately a light, not a surface:
 * a black albedo carrying a hot emissive, so a glint is the same brightness
 * wherever it lands and cannot be modulated by the face it sits on.
 */
export const PRIZE_METAL_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  /** Gold, value-stepped by facing. `Top` catches the key, `Deep` is a recess. */
  PrizeGoldTop: { baseColor: [0.78, 0.6, 0.2, 1] },
  PrizeGold: { baseColor: [0.66, 0.5, 0.16, 1] },
  PrizeGoldSide: { baseColor: [0.5, 0.37, 0.12, 1] },
  PrizeGoldDeep: { baseColor: [0.34, 0.25, 0.08, 1] },
  /** A struck line: the stamped lettering on a bar, the rim milling on a coin. */
  PrizeGoldEtch: { baseColor: [0.26, 0.18, 0.055, 1] },
  /** A glint. A light, not a surface — see the note above. */
  PrizeSparkle: { baseColor: [0, 0, 0, 1], emissive: [1, 0.94, 0.72, 1] },
};

/**
 * A twinkle envelope in [0, 1] for sparkle number `index` at `tick`.
 *
 * Sparkles read as light catching an edge only if they are SHARP — on for a
 * few frames, off for many, and never in unison. So each one runs its own slow
 * cycle (a different period per index, spaced by an irrational step so they
 * never re-phase) and is raised to a high power, which leaves a narrow spike at
 * the top of each cycle and near-zero everywhere else. Pure in (index, tick),
 * like every other cosmetic in this game.
 */
export const sparkleAt = (index: number, tick: number, settle: number): number => {
  const period = 34 + ((index * 7) % 19);
  const phase = ((tick / period + index * 0.6180339887) % 1) * Math.PI;
  return Math.sin(phase) ** 14 * settle;
};

/** Convenience for a prize that wants a plain solid color of its own. */
export const solid = (color: Rgba): MaterialSpec => ({ baseColor: color });

/** A local-space vector, re-exported so a prize file needs one import. */
export { v3 };
