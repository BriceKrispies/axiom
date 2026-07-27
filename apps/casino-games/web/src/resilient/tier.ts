/*
 * tier.ts — WHICH RUNG OF THE ONE LADDER THIS PAGE IS ACTUALLY ON.
 *
 * There is a single ladder, top to bottom, and this page implements all of it:
 *
 *     webgpu → webgl2 → webgl1 → canvas2d → css3d → form
 *
 * The first five rungs belong to `@axiom/web-engine`. Its `detectTier()` walks
 * them by PAINTING a known pattern on each rung and classifying the pixels that
 * come back, so a drawing context that exists but renders nothing — a real
 * state on locked-down and remote-desktop machines — is rejected rather than
 * trusted. This file no longer owns a stand-in for that judgement: the old
 * `CSS.supports("transform-style", "preserve-3d")` probe was a claim about an
 * API, never evidence about a frame, and two ladders in one product is exactly
 * the drift this build exists to avoid.
 *
 * The bottom rung is the one the engine cannot have an opinion about, because
 * it is the one that needs no script at all: `form` is the served document —
 * nine `<button type="submit">` controls inside a real `<form method="POST">`.
 * Nothing SETS it in the happy path. It is where the page already is before
 * this module runs, and where it returns to the moment anything above it fails.
 *
 * So the page keeps exactly two judgements of its own, and both earn their
 * place:
 *
 *   - `cssApplied` — did OUR stylesheet actually apply? Not "is CSS
 *     supported": whether THIS page got its rules, which an enterprise policy
 *     can strip. Layering geometry over a form that has lost its layout is
 *     strictly worse than the plain form, so a stripped stylesheet pins the
 *     page at `form`.
 *   - which rung is drawn HOW. `css3d` is drawn by this page's own CSS 3D
 *     chests, built INSIDE the buttons (`chests-3d.ts`); every rung above it is
 *     drawn by the engine on a canvas layered UNDER the buttons. Both keep the
 *     form controls as the only interaction, which is the invariant the whole
 *     build is for.
 *
 * Everything here is a pure function of a probe so the ladder's rules are
 * testable under bare `node --test`, with no DOM and no GPU.
 */

import type { Tier } from "@axiom/web-engine";

/** A rung of the product ladder: the engine's five, plus the document itself. */
export type PageTier = Tier | "form";

/** The bottom rung — the served form, working with zero script. */
export const FORM_TIER: PageTier = "form";

/** How a rung is drawn. `none` is the form: nothing is drawn over it. */
export type RenderRung = "engine" | "css3d" | "none";

export const rungFor = (tier: PageTier): RenderRung =>
  tier === "form" ? "none" : tier === "css3d" ? "css3d" : "engine";

/** What the page observed about the environment it woke up in. */
export interface PageProbe {
  /** Did our stylesheet actually apply? */
  readonly cssApplied: boolean;
  /** The engine's probed render tier (already honours `?render=`). */
  readonly renderTier: Tier;
}

/** The ladder, as one expression: the engine's verdict, floored at `form` when
 * this page has no styling to hang a richer presentation off. */
export const chooseTier = (probe: PageProbe): PageTier => (probe.cssApplied ? probe.renderTier : FORM_TIER);

/**
 * The next rung DOWN to try when the current one fails to mount. An engine rung
 * falls to the CSS 3D chests — a canvas that would not come up says nothing
 * about whether the DOM can composite a transform tree — and `css3d` falls to
 * the form, which cannot fail because it is what the server already sent.
 */
export const demoteRender = (tier: PageTier): PageTier => (rungFor(tier) === "engine" ? "css3d" : FORM_TIER);

/**
 * True when `tier` intercepts the form and posts in place. Every rung above the
 * form does; the form itself is a native browser navigation.
 *
 * Note what is NOT probed to decide this: whether a transport EXISTS. A managed
 * browser hands you a `fetch` that is present, callable and rejects, so the only
 * honest answer comes from having tried (`transport.ts`). The page climbs
 * optimistically and drops to `FORM_TIER` when the attempt actually fails.
 */
export const postsInPlace = (tier: PageTier): boolean => tier !== FORM_TIER;

/**
 * Read the tier-2 sentinel. `resilient.css` sets `--resilient-css: 1` on the
 * root element, and reading it back is the only honest test:
 * `document.styleSheets.length` counts sheets the browser has a RECORD of, not
 * sheets whose rules are being applied, and a policy that neuters styling can
 * leave the former intact.
 */
export const probeCss = (view: Window): boolean =>
  view.getComputedStyle(view.document.documentElement).getPropertyValue("--resilient-css").trim() === "1";
