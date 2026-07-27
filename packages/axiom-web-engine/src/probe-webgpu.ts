/*
 * probe-webgpu.ts — the top rung. This is the only ASYNCHRONOUS probe, and the
 * only one that can cost real time, so every rule here exists to bound it:
 *
 *   - `requestAdapter()` and `requestDevice()` each get their OWN timeout. Both
 *     can hang indefinitely behind a wedged GPU process; a single shared budget
 *     would let a slow adapter starve the device request of any chance.
 *   - An adapter can be returned whose DEVICE creation then fails — a genuine
 *     state on machines with a blocklisted driver. The adapter alone proves
 *     nothing, so the tier only passes once a device exists.
 *   - `device.lost` is a promise that NEVER rejects and may stay pending
 *     forever. It is latched with a `.then`, never awaited: the probe checks the
 *     latch after a microtask turn, so an immediately-lost device is caught
 *     without the probe ever being able to hang on it.
 *   - The whole probe is SKIPPED when the synchronous rungs already proved
 *     there is no hardware acceleration. Chrome disables WebGPU whenever
 *     hardware acceleration is off, so on exactly the machines where boot time
 *     hurts most — a remote-desktop / published-application session — probing
 *     could only ever burn the full adapter + device budget to be told what the
 *     WebGL probe already reported for free.
 *
 * There is no WebGPU BACKEND in this engine yet: the tier renders through
 * WebGL2 (see `renderer.ts`). The probe still runs, and is still reported
 * honestly, because "this machine has WebGPU" is a fact an app or a harness may
 * legitimately want — and because the day a backend lands, the ladder above it
 * is already correct.
 *
 * Platform edge: browser-API boundary — ordinary control flow, coverage-exempt.
 */

import type { TierProbe } from "./tier.ts";

/** The slice of the WebGPU API this probe touches, declared structurally: the
 * DOM lib these packages compile against has no WebGPU types, and pulling in a
 * whole type package to name four methods would be the larger dependency. */
interface GpuDevice {
  readonly destroy: () => void;
  readonly lost: Promise<unknown>;
}

interface GpuAdapter {
  readonly requestDevice: () => Promise<GpuDevice | null>;
}

interface Gpu {
  readonly requestAdapter: () => Promise<GpuAdapter | null>;
}

/** Per-stage budget. Two stages, so the probe's worst case is 2 x this. */
const STAGE_TIMEOUT_MS = 1200;

/** One microtask turn — long enough for a device that is lost on arrival to
 * settle its `lost` promise, short enough to be free. */
const settle = async (): Promise<void> => {
  await Promise.resolve();
};

/** Race `work` against a timer. A timeout resolves to `null` rather than
 * rejecting: a stage that never answered is indistinguishable, for our
 * purposes, from a stage that answered "no". */
const withTimeout = async <Value>(work: Promise<Value | null>, ms: number): Promise<Value | null> => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<null>((resolve) => {
    timer = setTimeout(() => {
      resolve(null);
    }, ms);
  });
  try {
    return await Promise.race([work, timeout]);
  } finally {
    clearTimeout(timer);
  }
};

/** The probe plus a way to release the device it created. */
export interface WebgpuProbe {
  readonly probe: TierProbe;
  /** Destroy the probe device. The caller releases it unless the webgpu tier is
   * the one being rendered with. */
  readonly release: () => void;
}

const noRelease = (): void => {
  // Nothing was created, so nothing needs destroying.
};

const withoutDevice = (detail: string, outcome: TierProbe["outcome"]): WebgpuProbe => ({
  probe: { accelerated: false, detail, outcome },
  release: noRelease,
});

const gpuOf = (): Gpu | undefined => (globalThis.navigator as { gpu?: Gpu } | undefined)?.gpu;

/**
 * Probe the WebGPU rung. `skip` is the caller's decision (see
 * `shouldProbeWebgpu` in `tier.ts`) that the budget is not worth spending;
 * passing it reports `skipped`, which is a result, not a failure.
 */
export const probeWebgpu = async (skip: boolean): Promise<WebgpuProbe> => {
  if (skip) {
    return withoutDevice("skipped: no hardware acceleration, so WebGPU is disabled too", "skipped");
  }
  try {
    const gpu = gpuOf();
    if (!gpu) {
      return withoutDevice("navigator.gpu is not present", "fail");
    }
    const adapter = await withTimeout(gpu.requestAdapter(), STAGE_TIMEOUT_MS);
    if (!adapter) {
      return withoutDevice(`no adapter within ${STAGE_TIMEOUT_MS}ms`, "fail");
    }
    const device = await withTimeout(adapter.requestDevice(), STAGE_TIMEOUT_MS);
    if (!device) {
      return withoutDevice(`an adapter was returned but no device within ${STAGE_TIMEOUT_MS}ms`, "fail");
    }
    let lost = false;
    void device.lost.then(() => {
      lost = true;
      return null;
    });
    await settle();
    const release = (): void => {
      try {
        device.destroy();
      } catch {
        // A device that refuses to be destroyed is already gone.
      }
    };
    if (lost) {
      release();
      return withoutDevice("the device was lost immediately after creation", "fail");
    }
    return { probe: { accelerated: true, detail: "adapter + device acquired", outcome: "pass" }, release };
  } catch (error) {
    return withoutDevice(`webgpu probe threw: ${String(error)}`, "fail");
  }
};
