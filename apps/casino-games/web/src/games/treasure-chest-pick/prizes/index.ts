/*
 * index.ts — the prize catalog: which treasure a chest yields, and how to draw it.
 *
 * ── how a chest decides what it holds ──────────────────────────────────────
 * It does not. The round does, before the player can touch anything: the
 * choice-population adapter draws a REWARD TIER for every slot at commit time
 * (`planChoicePopulation`), and the tier ids are exactly the `PrizeKind`s. So
 * the prize is a pure read of the committed plan — the presentation never picks
 * it, never re-rolls it, and cannot leak it, which is the same fairness property
 * the idle dance and the flight already hold.
 *
 * `prizeKindOf` therefore takes a tier id and returns a treasure. The lookup is
 * exact for the five tiers this game ships, and falls back through RARITY for
 * anything else — the reward ladder is editable from the Set Up panel, so a
 * config naming tiers this file has never heard of is a normal state, not a bug.
 * Each rarity has a canonical treasure (a coin is what "common" looks like, a
 * ring is what "jackpot" looks like), so any ladder a player builds still opens
 * chests full of real objects instead of falling back to a placeholder.
 */

import type { EngineQuat, MaterialSpec, SceneInstance } from "@axiom/web-engine";
import type { Rarity } from "../../../chance-engine/configuration/schema.ts";
import { quatMul, quatPitch, quatYaw } from "../../../presentation/stage/vectors.ts";
import type { Prize, PrizeFrame, PrizeKind } from "./prize.ts";
import { PRIZE_KINDS, PRIZE_METAL_MATERIALS, prizePlace } from "./prize.ts";
import { CRAB_BRIDE } from "./crab-bride.ts";
import { GOLD_BAR } from "./gold-bar.ts";
import { GOLD_COIN } from "./gold-coin.ts";
import { LEATHER_BOOT } from "./leather-boot.ts";
import { WEDDING_RING } from "./wedding-ring.ts";

const CATALOG: Readonly<Record<PrizeKind, Prize>> = {
  "crab-bride": CRAB_BRIDE,
  "gold-bar": GOLD_BAR,
  "gold-coin": GOLD_COIN,
  "leather-boot": LEATHER_BOOT,
  "wedding-ring": WEDDING_RING,
};

/** What each rarity looks like when a config names a tier this file does not
 * know. Also the honest ranking of the five: a coin is a small win, a boot is
 * the joke, a bar is a real haul, and the ring is the jackpot. */
const CANONICAL: Readonly<Record<Rarity, PrizeKind>> = {
  common: "gold-coin",
  jackpot: "wedding-ring",
  rare: "gold-bar",
  uncommon: "crab-bride",
};

const isPrizeKind = (id: string): id is PrizeKind => PRIZE_KINDS.includes(id as PrizeKind);

/** The treasure a committed tier yields — exact by id, else by rarity. */
export const prizeKindOf = (tierId: string | null, rarity: Rarity): PrizeKind =>
  tierId !== null && isPrizeKind(tierId) ? tierId : CANONICAL[rarity];

/** Every material the five treasures need, merged into the scene's resources. */
export const PRIZE_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...PRIZE_METAL_MATERIALS,
  ...PRIZE_KINDS.reduce<Record<string, MaterialSpec>>((all, kind) => ({ ...all, ...CATALOG[kind].materials }), {}),
};

/** How far the widest treasure reaches from its centre, in prize-local units.
 * The reveal's framing test sizes its headroom budget from this, so it can
 * never fall out of step with whichever prize is actually the biggest. */
export const PRIZE_EXTENT = Math.max(...PRIZE_KINDS.map((kind) => CATALOG[kind].extent));

/** How far ONE treasure reaches from its centre, in prize-local units — what
 * the staging needs to sit a glow underneath it without clipping through it. */
export const prizeExtentOf = (kind: PrizeKind): number => CATALOG[kind].extent;

/** How fast a turntable treasure revolves, in radians per tick. */
const TURNTABLE_RATE = 0.04;
/** A face-on treasure's rock: how far it swings, and how slowly. */
const ROCK_SWING = 0.38;
const ROCK_RATE = 0.021;

/**
 * The turn a treasure holds this tick, given how far the camera looks DOWN on
 * it (`elevation`, radians above the horizontal).
 *
 * Leaning by exactly `-elevation` swings the prize's local +Z onto the vector
 * pointing back at the camera and its local +Y onto screen-up — so at full lean
 * a prize author's "toward the camera" and "up" mean precisely that, and a coin
 * built standing in its local XY plane presents its face square to the lens. A
 * partial lean is the same rotation, scaled: an upright object dipped toward
 * the viewer rather than laid out for it. See `PrizePresentation`.
 */
export const prizeSpin = (kind: PrizeKind, elevation: number, tick: number): EngineQuat => {
  const prize = CATALOG[kind];
  const faceOn = prize.presentation === "faces-camera";
  const lean = quatPitch(-elevation * (faceOn ? 1 : prize.lean));
  const turn = faceOn ? quatYaw(Math.sin(tick * ROCK_RATE) * ROCK_SWING) : quatYaw(tick * TURNTABLE_RATE);
  // Turn about the object's OWN vertical first, then tip the whole thing back —
  // so the lean is a property of the shot and the turn a property of the object,
  // and neither one drifts the other.
  return quatMul(lean, turn);
};

/** Draw one treasure. `keyPrefix` namespaces its instance keys. */
export const prizeInstances = (kind: PrizeKind, keyPrefix: string, frame: PrizeFrame): readonly SceneInstance[] =>
  CATALOG[kind].build(prizePlace(keyPrefix, frame), frame);

export type { PrizeFrame, PrizeKind } from "./prize.ts";
export { PRIZE_KINDS, PRIZE_SIZE } from "./prize.ts";
