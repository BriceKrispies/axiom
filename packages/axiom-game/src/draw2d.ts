/*
 * The pure flip-book sampler (SPEC-04 §10.2) — the one draw2d FREE function. The
 * draw VERBS are immediate-mode and live on `Frame` (`sim.ts`); `sampleAnimation`
 * is different: a pure function of `(anim, elapsed, loop)` that returns the atlas
 * sub-rect to show at presentation time `elapsed`, computing nothing visible.
 *
 * The frame-index math (`floor(elapsed * fps)`, wrap-vs-clamp) runs NATIVE-side
 * via the bound host's `draw2dSampleAnimation` — the single deterministic source
 * of truth, never recomputed in TS. Only the `loop` default is resolved here at
 * the facade (the contract's `loop?` defaults to `true`, the flip-book
 * convention), branchlessly via `orElse`.
 */

import type { Rect, Seconds } from "./vocabulary.ts";
import type { SpriteAnimation } from "./draw2d-binding.ts";
import { boundHost } from "./host-binding.ts";
import { orElse } from "./control-flow.ts";

/** The contract's `loop?` default (SPEC-04 §10.2): a flip-book wraps unless told otherwise. */
const LOOP_BY_DEFAULT = true;

/**
 * Sample `anim`'s atlas sub-rect at presentation time `elapsed`. The index is
 * `floor(elapsed * anim.fps)`; when `loop` (default `true`) it wraps modulo the
 * frame count, otherwise it clamps to the last frame. An empty animation has no
 * frame to show, so it samples the inert zero-`Rect`. Pure: the same inputs
 * always yield the same `Rect` (SPEC-04 §6).
 */
export const sampleAnimation = (anim: SpriteAnimation, elapsed: Seconds, loop?: boolean): Rect =>
  boundHost().draw2dSampleAnimation(anim, elapsed, orElse(loop, LOOP_BY_DEFAULT));
