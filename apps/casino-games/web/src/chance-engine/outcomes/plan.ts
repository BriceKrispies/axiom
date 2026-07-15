/*
 * plan.ts — the committed-outcome vocabulary. An `OutcomePlan` is the material
 * result of one round, resolved at a clear commitment point BEFORE its reveal
 * animation. After commitment nothing may change it: not the win state, not
 * the tier, not the selected object — animation only manifests it.
 */

import type { RewardDefinition } from "../configuration/schema.ts";

/** Typed, meaningful player action supplied at commitment. */
export interface OutcomeResolutionContext {
  /** Choice games: which visible object the player selected. */
  readonly selectedIndex?: number;
  /** Prize Drop: horizontal release position in [0, 1]. */
  readonly dropPosition?: number;
  /** Claw Grab: index of the prize currently under the claw. */
  readonly targetedPrizeIndex?: number;
  /** Fishing Cast: index of the visual region the cast landed in. */
  readonly castRegion?: number;
  /** Capsule Conveyor: belt position (capsule index) at stop time. */
  readonly stopPosition?: number;
  /** Prize Wheel: launch strength in [0, 1]. */
  readonly launchStrength?: number;
  /** Coin Fountain / aimed games: normalized aim point. */
  readonly aim?: { readonly x: number; readonly y: number };
}

/** How the committed result manifests mechanically, per adapter family. */
export type MechanicManifestation =
  | {
      readonly kind: "choice";
      readonly winnersByIndex: readonly (string | null)[];
      readonly selectedIndex: number;
      readonly winnerCount: number;
    }
  | { readonly kind: "destination"; readonly destinationIndex: number; readonly destinationId: string }
  | { readonly kind: "combination"; readonly combination: readonly number[] }
  | { readonly kind: "single"; readonly focusIndex: number };

/** One round's committed, immutable material outcome. */
export interface OutcomePlan {
  /** Stable round identity: `seed#round` (seeded) or the injected round id. */
  readonly roundId: string;
  readonly win: boolean;
  /** The resolved reward tier id — null on a non-winning round. */
  readonly tierId: string | null;
  readonly reward: RewardDefinition | null;
  /** Drives reveal/celebration animation streams; part of the commitment. */
  readonly presentationSeed: number;
  readonly manifestation: MechanicManifestation;
}
