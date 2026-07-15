/*
 * definition.ts — the contract one casino game presents to the application:
 * identity + catalog metadata, a validated default configuration, and a
 * `mount` that runs the game on a canvas through @axiom/web-engine's
 * `runGame`. The registry (registry.ts) is the single source of truth for
 * which definitions exist; the catalog and the game screen are both built
 * from it.
 */

import type { BackendChoice, InputState } from "@axiom/web-engine";
import type { CasinoGameConfig, Rarity } from "../configuration/schema.ts";
import type { ConfigIssue } from "../configuration/validation.ts";
import type { SessionAuditRecord } from "../diagnostics/audit.ts";
import type { ChanceResultSource } from "../outcomes/result-source.ts";
import type { GamePhase } from "../sessions/phases.ts";

export type RenderMode = "2d" | "3d";

/** Catalog filter families. A game may belong to several. */
export type GameCategory = "choice" | "machine" | "physical" | "reveal";

export type MechanicKind = "choice-population" | "destination" | "combination" | "single-reveal";

/** The player-facing settings the application resolves and hands to a mount. */
export interface PresentationSettings {
  readonly masterVolume: number;
  readonly sfxVolume: number;
  readonly reducedMotion: boolean;
  readonly particleScale: number;
  readonly cameraShake: boolean;
  readonly highContrast: boolean;
}

/** The per-frame HUD projection a mounted game reports to the DOM shell. */
export interface CasinoHud {
  readonly phase: GamePhase;
  readonly instruction: string;
  /** Result banner text once revealed, else null. */
  readonly resultText: string | null;
  readonly win: boolean | null;
  readonly tierId: string | null;
  readonly rarity: Rarity | null;
  readonly inputLocked: boolean;
  readonly round: number;
  readonly audit: SessionAuditRecord;
}

/** Everything the application supplies when mounting one game. */
export interface GameRuntime<TSpec> {
  readonly config: CasinoGameConfig<TSpec>;
  readonly seed: number;
  readonly round: number;
  readonly source: ChanceResultSource;
  readonly settings: PresentationSettings;
  readonly backend?: BackendChoice;
  /** Freeze the simulation at this tick (?shot=N deterministic screenshots). */
  readonly freezeAtTick?: number;
  /** Pin `view`'s wall clock (deterministic captures). */
  readonly pinnedNowMs?: number;
  /** Scripted synthetic input, run before each tick's snapshot. */
  readonly script?: (tick: number, input: InputState) => void;
  /** Called every rendered frame with the HUD projection. */
  readonly onHud: (hud: CasinoHud) => void;
}

/** A live mounted game (wraps the engine's RunningGame behind the hud shape). */
export interface RunningCasinoGame {
  readonly stop: () => void;
  readonly readHud: () => CasinoHud;
  readonly input: InputState;
}

/** Palette + glyph the catalog uses to draw the game's procedural thumbnail. */
export interface ThumbnailSpec {
  readonly top: string;
  readonly bottom: string;
  readonly accent: string;
  readonly glyph: "chest" | "card" | "wheel" | "dice" | "door" | "globe" | "dial" | "ticket" | "gift" | "rocket" | "bobber" | "claw" | "elevator" | "fountain" | "map" | "portal" | "capsule" | "lantern" | "gem" | "token";
}

export interface CasinoGameDefinition<TSpec = unknown> {
  readonly id: string;
  readonly displayName: string;
  readonly shortDescription: string;
  /** The concise on-screen instruction shown in the game chrome. */
  readonly instruction: string;
  readonly renderMode: RenderMode;
  /** Interaction badge for the catalog card (e.g. "aim + release"). */
  readonly interaction: string;
  readonly categories: readonly GameCategory[];
  readonly mechanic: MechanicKind;
  /** Machine-camera rule: true ⇒ the camera sits INSIDE the machine. */
  readonly machineInterior: boolean;
  readonly thumbnail: ThumbnailSpec;
  readonly defaultConfig: () => CasinoGameConfig<TSpec>;
  /** Game-specific validation of `gameSpecific` (shared shape is validated
   * centrally by `validateConfig`). */
  readonly validateSpec: (spec: TSpec) => readonly ConfigIssue[];
  readonly mount: (canvas: HTMLCanvasElement, runtime: GameRuntime<TSpec>) => RunningCasinoGame;
}
