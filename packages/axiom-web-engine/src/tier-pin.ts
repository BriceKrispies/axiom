/*
 * tier-pin.ts — the PURE codec behind the two things `override.ts` persists: a
 * pinned tier (`?render=webgl2` remembered for the session) and the crash
 * sentinel (the tier an attempt was made at, cleared once a frame is drawn).
 *
 * Both are "a tier, valid only in the environment it was written in", so both
 * use one codec. Keeping it here rather than inside the storage boundary is
 * deliberate: the decision to REJECT a stored value — a stamp mismatch, a
 * version bump, a corrupted string, a tier name that no longer exists — is
 * exactly the logic that has to be right and exactly the logic a browser-only
 * file could never prove. `override.ts` is left holding nothing but
 * `sessionStorage.getItem` / `setItem` in a try/catch.
 *
 * Why an environment stamp at all: a pin follows the SESSION, and a VDI /
 * published-application user's session can be re-hosted onto different hardware
 * mid-flight. A tier pinned on a GPU-backed host is actively harmful on a
 * software-rendered one. Stamping the pin with the environment it was chosen in
 * makes a stale pin self-invalidating rather than sticky.
 *
 * Why sessionStorage and not localStorage: a bad pin must not be permanent.
 * Closing the tab is a remedy every user already knows.
 */

import { type Tier, isTier } from "./tier.ts";
import { absentProbe, both, orElse, pick } from "./branchless.ts";

/** Bump when the encoded shape changes; older payloads then decode as absent
 * instead of being misread. */
const PIN_VERSION = "v1";

/** Newlines cannot occur in a user agent, a screen size, or a tier name, so a
 * newline join is unambiguous without escaping. */
const PIN_SEPARATOR = "\n";

const PIN_FIELDS = 3;
const VERSION_INDEX = 0;
const STAMP_INDEX = 1;
const TIER_INDEX = 2;

const ABSENT_TIER = absentProbe<Tier>();

/** The environment a pin was chosen in. Everything here changes when a session
 * is re-hosted onto different hardware, and nothing here changes during normal
 * use of one page. */
export interface EnvironmentFingerprint {
  readonly devicePixelRatio: number;
  readonly screenHeight: number;
  readonly screenWidth: number;
  readonly userAgent: string;
}

/** A stable string identifying the backing host a tier was chosen on. */
export const environmentStamp = (env: EnvironmentFingerprint): string =>
  `${env.userAgent}|${env.screenWidth}x${env.screenHeight}@${env.devicePixelRatio}`;

/** Encode a tier together with the environment it is valid in. */
export const encodePin = (tier: Tier, stamp: string): string => [PIN_VERSION, stamp, tier].join(PIN_SEPARATOR);

/**
 * Decode a stored pin, yielding the tier ONLY when the payload is well-formed,
 * current, and was written in this same environment. Every other case — absent,
 * truncated, wrong version, foreign stamp, unknown tier name — is absent, so a
 * corrupted or stale value can never steer the ladder.
 */
export const decodePin = (raw: string | undefined, stamp: string): Tier | undefined => {
  const fields = orElse(raw, "").split(PIN_SEPARATOR);
  const valid = [fields]
    .filter((parts) => parts.length === PIN_FIELDS)
    .filter((parts) => both(pick(parts, VERSION_INDEX) === PIN_VERSION, pick(parts, STAMP_INDEX) === stamp))
    .map((parts) => pick(parts, TIER_INDEX))
    .filter((name): name is Tier => isTier(name));
  return pick([ABSENT_TIER, ...valid], valid.length);
};
