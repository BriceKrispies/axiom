/*
 * detect.ts — the ORCHESTRATOR: it runs the probes in ladder order and produces
 * the `DetectionReport` the renderer acts on.
 *
 * The order is not arbitrary. The control probe (`probe-readback.ts`) runs
 * FIRST and unconditionally, because every later probe's pixel evidence is only
 * admissible if readback itself can be trusted. Then the ladder is walked from
 * the crash-guard ceiling downward, LAZILY: the first rung that passes stops the
 * walk, and every rung below it is recorded as `skipped`. That laziness is a
 * correctness property, not an optimisation — every WebGL probe consumes one of
 * the ~16 GL contexts a page is allowed, so probing rungs nobody will use is a
 * way to lose the game's own context.
 *
 * Two things this file must never do:
 *
 *   - Gate anything on `requestAnimationFrame`. rAF does not fire in a hidden
 *     or minimised tab, which is the normal state of a Citrix published
 *     application while it starts. A detector that waits for a frame there waits
 *     forever. Only microtasks and timers are used.
 *   - Let a probe take the boot down. Every probe is wrapped; a throw is that
 *     tier's failure and nothing more. Detection itself is capped
 *     (`DETECT_BUDGET_MS`) and always produces a report, even if that report
 *     says only "css3d, because nothing else answered in time".
 *
 * WebGPU is the one asynchronous rung, so there are two entry points.
 * `detectTierSync` is what a synchronous `initRenderer` can use; it records
 * webgpu as `skipped`, which is honest — it was never asked. `detectTier`
 * awaits the full ladder and caches its report, so a later `initRenderer(canvas,
 * "auto")` reuses it rather than probing twice.
 *
 * Platform edge: browser-API boundary — ordinary control flow, coverage-exempt.
 * Every decision it makes is pure and covered in `tier.ts`.
 */

import {
  type DetectionReport,
  type Tier,
  type TierProbe,
  type TierProbes,
  ceilingAfterCrash,
  chooseTier,
  isSelectable,
  ladderFrom,
  outcomesOf,
  rank,
  shouldProbeWebgpu,
} from "./tier.ts";
import type { ReadbackTrust } from "./probe-pattern.ts";
import { probeCanvas2d } from "./probe-canvas2d.ts";
import { probeCss3d } from "./probe-css3d.ts";
import { probeReadback } from "./probe-readback.ts";
import { probeWebgl } from "./probe-webgl.ts";
import { probeWebgpu } from "./probe-webgpu.ts";
import { readCrashSentinel, readTierOverride } from "./override.ts";

/** The whole detection's hard cap. The synchronous rungs cost microseconds; the
 * budget exists for the two WebGPU stages, which are bounded at 1200ms each. */
const DETECT_BUDGET_MS = 2500;

const SKIPPED: TierProbe = { accelerated: false, detail: "not reached: a better tier already passed", outcome: "skipped" };

const ABOVE_CEILING: TierProbe = { accelerated: false, detail: "above the crash-guard ceiling", outcome: "skipped" };

/** Every rung except the asynchronous one. */
type SyncTier = Exclude<Tier, "webgpu">;

/** The synchronous rungs. WebGPU is absent by construction, not by omission. */
const SYNC_PROBES: Readonly<Record<SyncTier, (trust: ReadbackTrust) => TierProbe>> = {
  canvas2d: probeCanvas2d,
  css3d: (): TierProbe => probeCss3d(),
  webgl1: (trust): TierProbe => probeWebgl("webgl1", trust),
  webgl2: (trust): TierProbe => probeWebgl("webgl2", trust),
};

/** A monotonic clock where one exists. `performance` is typed as always
 * present but is genuinely absent in some embedded hosts, so the fallback is a
 * catch rather than a condition the compiler would call unreachable. */
const now = (): number => {
  try {
    return performance.now();
  } catch {
    return Date.now();
  }
};

/** Walk the ladder from `ceiling`, probing lazily: stop at the first rung that
 * can render. Rungs above the ceiling and below the winner are `skipped`. */
const walkSync = (ceiling: Tier, trust: ReadbackTrust): TierProbes => {
  const probes: Record<Tier, TierProbe> = {
    canvas2d: ABOVE_CEILING,
    css3d: ABOVE_CEILING,
    webgl1: ABOVE_CEILING,
    webgl2: ABOVE_CEILING,
    webgpu: ABOVE_CEILING,
  };
  for (const tier of ladderFrom(ceiling)) {
    if (tier === "webgpu") {
      probes.webgpu = { accelerated: false, detail: "not probed on the synchronous path", outcome: "skipped" };
      continue;
    }
    probes[tier] = SYNC_PROBES[tier](trust);
    if (isSelectable(probes[tier].outcome)) {
      for (const lower of ladderFrom(tier).slice(1)) {
        probes[lower] = SKIPPED;
      }
      break;
    }
  }
  return probes;
};

/** Everything a report is assembled from. */
interface Detection {
  readonly ceiling: Tier;
  readonly probes: TierProbes;
  readonly startedMs: number;
  readonly trust: ReadbackTrust;
}

const report = ({ ceiling, probes, startedMs, trust }: Detection): DetectionReport => {
  const override = readTierOverride();
  const detected = chooseTier(outcomesOf(probes), ceiling);
  const elapsedMs = now() - startedMs;
  // An explicit ?render= is the user's escape hatch and outranks everything,
  // including the crash guard — without that, a machine that crashed once could
  // never be asked to try that tier again.
  if (override.source === "url" && override.tier) {
    return { ceiling, elapsedMs, probes, readback: trust, source: "url", tier: override.tier };
  }
  // A remembered SESSION pin is only a convenience, so it stays capped by the
  // crash-guard ceiling.
  if (override.source === "session" && override.tier && rank(override.tier) >= rank(ceiling)) {
    return { ceiling, elapsedMs, probes, readback: trust, source: "session", tier: override.tier };
  }
  return { ceiling, elapsedMs, probes, readback: trust, source: "probe", tier: detected };
};

let cached: DetectionReport | undefined;

/** The last completed detection, if any. */
export const latestDetection = (): DetectionReport | undefined => cached;

/** Forget the cached detection (tests, and an app that wants to re-detect after
 * a context loss). */
export const resetDetection = (): void => {
  cached = undefined;
};

/**
 * Run the synchronous ladder: control probe, then webgl2 → webgl1 → canvas2d →
 * css3d until one passes. WebGPU is reported as `skipped` — see `detectTier`
 * for the full ladder.
 */
export const detectTierSync = (): DetectionReport => {
  const started = now();
  const ceiling = ceilingAfterCrash(readCrashSentinel());
  const trust = probeReadback().trust;
  cached = report({ ceiling, probes: walkSync(ceiling, trust), startedMs: started, trust });
  return cached;
};

/**
 * Run the full ladder, including the asynchronous WebGPU rung. Capped at
 * `DETECT_BUDGET_MS`; the synchronous result is always available as a fail-safe,
 * so the cap can only ever cost the WebGPU upgrade, never the report.
 */
export const detectTier = async (): Promise<DetectionReport> => {
  const started = now();
  const base = detectTierSync();
  const remaining = DETECT_BUDGET_MS - (now() - started);
  const skip = !shouldProbeWebgpu(base.probes, base.ceiling) || remaining <= 0;
  const gpu = await probeWebgpu(skip);
  // No WebGPU BACKEND exists yet — the webgpu tier renders through WebGL2 — so
  // the probe device is never handed on and is released here in every case.
  gpu.release();
  const probes: TierProbes = {
    canvas2d: base.probes.canvas2d,
    css3d: base.probes.css3d,
    webgl1: base.probes.webgl1,
    webgl2: base.probes.webgl2,
    webgpu: gpu.probe,
  };
  cached = report({ ceiling: base.ceiling, probes, startedMs: started, trust: base.readback });
  return cached;
};
