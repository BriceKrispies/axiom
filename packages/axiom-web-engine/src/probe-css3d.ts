/*
 * probe-css3d.ts — the terminal rung: `backend-css.ts` draws the scene as DOM
 * elements under CSS 3D transforms and never acquires a drawing context at all.
 *
 * This probe therefore asks the smallest possible question — can a style be set
 * and read back? — and it is deliberately generous about the answer. CSS3D is
 * the fail-safe: `chooseTier` lands here when every rung above it failed, so a
 * strict verdict here would leave the engine with nowhere at all to render.
 * Losing `preserve-3d` costs depth sorting, not the picture, so it is reported
 * as `degraded` rather than a failure.
 *
 * Platform edge: browser-API boundary — ordinary control flow, coverage-exempt.
 */

import type { TierProbe } from "./tier.ts";

const TRANSLATE_3D = "translate3d(1px, 2px, 3px)";

/** `CSS.supports` where it exists, otherwise the older evidence: assign the
 * declaration and see whether the engine kept it. */
const supports = (property: string, value: string): boolean => {
  const api = globalThis.CSS as { supports?: (property: string, value: string) => boolean } | undefined;
  if (typeof api?.supports === "function") {
    return api.supports(property, value);
  }
  const probe = document.createElement("div");
  probe.style.setProperty(property, value);
  return probe.style.getPropertyValue(property) !== "";
};

export const probeCss3d = (): TierProbe => {
  try {
    const transforms = supports("transform", TRANSLATE_3D);
    const depth = supports("transform-style", "preserve-3d");
    if (transforms && depth) {
      return { accelerated: false, detail: "3D transforms + preserve-3d supported", outcome: "pass" };
    }
    return {
      accelerated: false,
      detail: `DOM rendering available; 3D transforms=${transforms}, preserve-3d=${depth}`,
      outcome: "degraded",
    };
  } catch (error) {
    return { accelerated: false, detail: `css3d probe threw: ${String(error)}`, outcome: "fail" };
  }
};
