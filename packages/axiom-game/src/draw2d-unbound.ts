/*
 * The inert 2D drawing surface used before `bindNative` — every draw is a no-op
 * and every id-returning verb mints the null handle / empty list. Kept in its own
 * module so `draw2d-binding.ts` stays within the 300-line budget, the same
 * partition reason `unbound-host.ts` was split out of `host-binding.ts`. The free
 * surface composes `UNBOUND_DRAW2D` onto `UNBOUND_HOST_BASE` so `boundHost()` is a
 * total `HostBridge` (no `null` channel to branch on) before any host binds.
 */

import type { Draw2dBridge, Paint, TextMetrics } from "./draw2d-binding.ts";
import type { Handle, Rect } from "./vocabulary.ts";

/** The handle returned by the inert handle-minting 2D reads before a host binds (a null handle). */
const UNBOUND_HANDLE = 0;

/** The inert sub-rect the unbound flip-book sampler returns (nothing to draw). */
const INERT_RECT: Rect = { height: 0, width: 0, x: 0, y: 0 };

/** The inert extent the unbound `measureText` returns before a host binds. */
const INERT_METRICS: TextMetrics = { height: 0, width: 0 };

/*
 * The inert 2D surface used before `bindNative`: every draw is a no-op and every
 * id-returning verb mints the null handle / empty list. Composed into `UNBOUND_HOST`
 * so the free surface stays total (no `null` channel to branch on).
 */
export const UNBOUND_DRAW2D: Draw2dBridge = {
  draw2dAdvanceParticles: (): void => {
    // No-op until a host is bound
  },
  draw2dBeginTarget: (): void => {
    // No-op until a host is bound
  },
  draw2dCamera2d: (): void => {
    // No-op until a host is bound
  },
  draw2dCircle: (): void => {
    // No-op until a host is bound
  },
  draw2dCreateEmitter: (): Handle => UNBOUND_HANDLE,
  draw2dCreateRenderTarget: (): Handle => UNBOUND_HANDLE,
  draw2dEllipse: (): void => {
    // No-op until a host is bound
  },
  draw2dEmit: (): void => {
    // No-op until a host is bound
  },
  draw2dEndTarget: (): void => {
    // No-op until a host is bound
  },
  draw2dFinish: (): readonly number[] => [],
  draw2dLine: (): void => {
    // No-op until a host is bound
  },
  draw2dLinearGradient: (): Paint => UNBOUND_HANDLE,
  draw2dMeasureText: (): TextMetrics => INERT_METRICS,
  draw2dPath: (): void => {
    // No-op until a host is bound
  },
  draw2dRadialGradient: (): Paint => UNBOUND_HANDLE,
  draw2dRect: (): void => {
    // No-op until a host is bound
  },
  draw2dSampleAnimation: (): Rect => INERT_RECT,
  draw2dSprite: (): void => {
    // No-op until a host is bound
  },
  draw2dTargetTexture: (): Handle => UNBOUND_HANDLE,
  draw2dText: (): void => {
    // No-op until a host is bound
  },
};
