/*
 * override.ts — the two pieces of STATE the tier ladder needs across a reload:
 * the user's explicit `?render=` choice, and the crash guard.
 *
 * The override. `?render=webgl2` forces a rung; `?render=auto` clears the
 * force. The choice is remembered so it survives the in-app navigations and
 * reloads a support conversation involves ("open it with ?render=canvas2d" has
 * to keep working after the next click). It is remembered in **sessionStorage,
 * never localStorage**: a wrong pin must expire when the tab closes, which is a
 * remedy every user already knows, instead of following them forever. It is
 * also stamped with the environment it was chosen in (see `tier-pin.ts`), so a
 * VDI session re-hosted onto different hardware does not inherit a pin chosen
 * for the old host.
 *
 * The crash guard. `beginAttempt(tier)` writes a sentinel BEFORE a tier is
 * initialised; `confirmFirstFrame(tier)` clears it once a frame has actually
 * been drawn. A sentinel still present at boot therefore means exactly one
 * thing: the last attempt at that tier died before it rendered anything — a
 * driver crash, a killed GPU process, a context lost during init. The next boot
 * caps the ladder one rung lower (`ceilingAfterCrash`), so the machine reaches
 * a working renderer on its second load instead of crashing identically
 * forever.
 *
 * Every storage access is wrapped: enterprise policy, private modes, and
 * partitioned third-party contexts can all make `sessionStorage` throw on mere
 * ACCESS, not just on write. Storage being unavailable degrades the override
 * and the crash guard to no-ops — it must never take the engine down with it.
 *
 * Platform edge: browser-API boundary — ordinary control flow, coverage-exempt.
 * The decisions it persists (what a valid pin is, what a stale one is) are pure
 * and fully covered in `tier-pin.ts`.
 */

import { type Tier, type TierChoice, parseTierChoice } from "./tier.ts";
import { decodePin, encodePin, environmentStamp } from "./tier-pin.ts";

/** The `?render=` query parameter. */
const QUERY_KEY = "render";

/** Session keys. Namespaced so an app's own storage cannot collide. */
const PIN_KEY = "axiom.render.pin";
const ATTEMPT_KEY = "axiom.render.attempt";

/** Where the running tier's choice came from. */
export type OverrideSource = "url" | "session" | "none";

export interface TierOverride {
  readonly source: OverrideSource;
  readonly tier: Tier | undefined;
}

const NO_OVERRIDE: TierOverride = { source: "none", tier: undefined };

const store = (): Storage | undefined => {
  try {
    return globalThis.sessionStorage ?? undefined;
  } catch {
    return undefined;
  }
};

const readKey = (key: string): string | undefined => {
  try {
    return store()?.getItem(key) ?? undefined;
  } catch {
    return undefined;
  }
};

const writeKey = (key: string, value: string): void => {
  try {
    store()?.setItem(key, value);
  } catch {
    // Storage is unavailable (policy, private mode, partitioning). The override
    // and the crash guard degrade to no-ops; detection still works.
  }
};

const dropKey = (key: string): void => {
  try {
    store()?.removeItem(key);
  } catch {
    // See writeKey.
  }
};

/** The environment a pin is valid in. Read defensively: a headless or embedded
 * host can be missing `screen` entirely. */
const stamp = (): string => {
  const screen = globalThis.screen as { height?: number; width?: number } | undefined;
  return environmentStamp({
    devicePixelRatio: globalThis.devicePixelRatio ?? 1,
    screenHeight: screen?.height ?? 0,
    screenWidth: screen?.width ?? 0,
    userAgent: globalThis.navigator?.userAgent ?? "",
  });
};

const queryChoice = (): TierChoice | undefined => {
  try {
    const search = globalThis.location?.search;
    if (!search) {
      return undefined;
    }
    return parseTierChoice(new URLSearchParams(search).get(QUERY_KEY) ?? undefined);
  } catch {
    return undefined;
  }
};

/** Forget any pinned tier (what `?render=auto` means). */
export const clearTierOverride = (): void => {
  dropKey(PIN_KEY);
};

/**
 * The tier the user asked for, if any. A `?render=` in the URL wins and is
 * pinned for the session; otherwise a pin from earlier in this session is
 * honoured, but only if it was written on this same environment.
 */
export const readTierOverride = (): TierOverride => {
  const choice = queryChoice();
  if (choice === "auto") {
    clearTierOverride();
    return NO_OVERRIDE;
  }
  if (choice) {
    writeKey(PIN_KEY, encodePin(choice, stamp()));
    return { source: "url", tier: choice };
  }
  const pinned = decodePin(readKey(PIN_KEY), stamp());
  return pinned ? { source: "session", tier: pinned } : NO_OVERRIDE;
};

/** The tier a previous attempt died in, if it never reached a frame. */
export const readCrashSentinel = (): Tier | undefined => decodePin(readKey(ATTEMPT_KEY), stamp());

/** Record that `tier` is about to be initialised. Called BEFORE the backend is
 * constructed, so a crash inside construction leaves the sentinel behind. */
export const beginAttempt = (tier: Tier): void => {
  writeKey(ATTEMPT_KEY, encodePin(tier, stamp()));
};

/**
 * Record that `tier` reached a rendered frame. Only clears a sentinel written
 * for this same tier: a stale sentinel from a DIFFERENT tier's failed attempt
 * is still evidence, and must not be erased by an unrelated success.
 */
export const confirmFirstFrame = (tier: Tier): void => {
  if (readCrashSentinel() === tier) {
    dropKey(ATTEMPT_KEY);
  }
};
