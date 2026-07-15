/*
 * settings.ts — the player-facing settings: volumes, reduced motion, particle
 * density, camera shake, high contrast, text scale. Persisted to localStorage
 * and resolved (together with a config's reducedMotion mode and the OS media
 * query) into the `PresentationSettings` a game mount receives.
 */

import type { ReducedMotionMode } from "../chance-engine/configuration/schema.ts";
import type { PresentationSettings } from "../chance-engine/registry/definition.ts";

export interface PlayerSettings {
  readonly masterVolume: number;
  readonly sfxVolume: number;
  readonly muted: boolean;
  readonly reducedMotion: "system" | "on" | "off";
  readonly particleDensity: "full" | "low";
  readonly cameraShake: boolean;
  readonly highContrast: boolean;
  readonly textScale: "normal" | "large";
}

export const DEFAULT_SETTINGS: PlayerSettings = {
  cameraShake: true,
  highContrast: false,
  masterVolume: 0.9,
  muted: false,
  particleDensity: "full",
  reducedMotion: "system",
  sfxVolume: 0.9,
  textScale: "normal",
};

const STORAGE_KEY = "casino-games:settings";

export const loadSettings = (): PlayerSettings => {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) {
      return DEFAULT_SETTINGS;
    }
    return { ...DEFAULT_SETTINGS, ...(JSON.parse(raw) as Partial<PlayerSettings>) };
  } catch {
    return DEFAULT_SETTINGS;
  }
};

export const saveSettings = (settings: PlayerSettings): void => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Storage may be unavailable (private mode) — settings just don't persist.
  }
};

const systemPrefersReducedMotion = (): boolean =>
  typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Resolve player settings + the config's reduced-motion mode into the flat
 * settings a mount consumes. The config may force reduced motion on/off for
 * one game; "system" defers to the player setting, which defers to the OS. */
export const resolveSettings = (settings: PlayerSettings, configMode: ReducedMotionMode): PresentationSettings => {
  const playerReduced = settings.reducedMotion === "system" ? systemPrefersReducedMotion() : settings.reducedMotion === "on";
  return {
    cameraShake: settings.cameraShake,
    highContrast: settings.highContrast,
    masterVolume: settings.muted ? 0 : settings.masterVolume,
    particleScale: settings.particleDensity === "low" ? 0.35 : 1,
    reducedMotion: configMode === "system" ? playerReduced : configMode === "on",
    sfxVolume: settings.muted ? 0 : settings.sfxVolume,
  };
};
