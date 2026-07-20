// The in-browser Strudel entry. esbuild bundles this (with the pinned @strudel/*
// + superdough packages) into a single inline <script> the harness serves. It
// runs inside headless Chromium and exposes two functions on `window`:
//   __strudelCheck  — transpile → evaluate → assert Pattern → query (no audio)
//   __strudelRender — the above, then an OfflineAudioContext bounce → PCM
//
// This assembled offline exporter is what the task calls the "minimal exporter":
// there is no public one-call Strudel bounce API, so we drive the render from
// public exports — superdough's `setAudioContext` injects an OfflineAudioContext
// so every internal getAudioContext() renders offline, then we schedule each hap
// through `superdough` and read the rendered AudioBuffer. Pure oscillators need
// no samples and no network; test/exporter.test.ts pins this against the exact
// package versions.

import { transpiler } from '@strudel/transpiler';
import { evaluate, evalScope } from '@strudel/core';
import { initAudio, registerSynthSounds, superdough, setAudioContext } from 'superdough';

import type {
  CheckRequest,
  CheckResult,
  Diagnostic,
  Phase,
  RenderRequest,
  RenderResult,
} from './protocol.ts';

// Hoist Strudel's control functions (note, s, gain, scale, …) into global scope
// so evaluated source can reference them, exactly as the Strudel REPL does.
const scopeReady: Promise<unknown> = evalScope(
  import('@strudel/core'),
  import('@strudel/mini'),
  import('@strudel/tonal'),
  import('@strudel/webaudio'),
);

interface Hap {
  readonly whole?: { begin: { valueOf(): number }; end: { valueOf(): number } };
  hasOnset(): boolean;
  readonly value: unknown;
}

interface Pattern {
  queryArc(begin: number, end: number): Hap[];
}

function diag(phase: Phase, err: unknown): Diagnostic {
  const anyErr = err as { message?: string; loc?: { line?: number; column?: number } } | undefined;
  const message = anyErr?.message ?? String(err);
  const line = anyErr?.loc?.line;
  // acorn columns are 0-based; present 1-based to match editors.
  const column = typeof anyErr?.loc?.column === 'number' ? anyErr.loc.column + 1 : undefined;
  return { phase, message, line, column };
}

/** Run transpile → evaluate → assert Pattern → query, returning the pattern + haps. */
async function toPattern(
  req: CheckRequest,
): Promise<{ pattern: Pattern; haps: Hap[] } | { diagnostic: Diagnostic }> {
  await scopeReady;

  try {
    transpiler(req.code, { emitMiniLocations: true, addReturn: true });
  } catch (err) {
    return { diagnostic: diag('transpile', err) };
  }

  let pattern: unknown;
  try {
    ({ pattern } = await evaluate(req.code, transpiler));
  } catch (err) {
    return { diagnostic: diag('evaluate', err) };
  }

  const asPattern = pattern as Partial<Pattern> | null | undefined;
  if (!asPattern || typeof asPattern.queryArc !== 'function') {
    return {
      diagnostic: {
        phase: 'pattern',
        message: 'source did not evaluate to a Strudel pattern',
      },
    };
  }

  const cycles = req.seconds * req.cps;
  try {
    const haps = (asPattern as Pattern).queryArc(0, cycles);
    return { pattern: asPattern as Pattern, haps };
  } catch (err) {
    return { diagnostic: diag('query', err) };
  }
}

async function check(req: CheckRequest): Promise<CheckResult> {
  const outcome = await toPattern(req);
  return 'diagnostic' in outcome
    ? { ok: false, diagnostic: outcome.diagnostic }
    : { ok: true, hapCount: outcome.haps.length };
}

async function render(req: RenderRequest): Promise<RenderResult> {
  const outcome = await toPattern(req);
  if ('diagnostic' in outcome) {
    return { ok: false, diagnostic: outcome.diagnostic };
  }

  const frames = Math.max(1, Math.ceil(req.seconds * req.sampleRate));
  const ctx = new OfflineAudioContext(req.channels, frames, req.sampleRate);
  setAudioContext(ctx);
  // Worklets ON: superdough's worklet effects (distort, shape, coarse, crush, …)
  // construct AudioWorkletNodes unconditionally, so they need the worklet modules
  // registered first. superdough/supradough ship those modules as inline
  // `data:text/javascript;base64` URLs baked into their built bundles, so
  // `audioWorklet.addModule` resolves them in-memory — no network, offline-safe.
  await initAudio();
  registerSynthSounds();

  for (const hap of outcome.haps) {
    if (!hap.hasOnset() || !hap.whole) {
      continue;
    }
    const begin = hap.whole.begin.valueOf();
    const end = hap.whole.end.valueOf();
    const tSec = begin / req.cps;
    const durSec = (end - begin) / req.cps;
    try {
      await superdough(hap.value, tSec, durSec, req.cps, begin);
    } catch (err) {
      // A synthesis-time failure (e.g. a bad control) fails the render as an
      // invalid pattern rather than silently producing partial audio.
      return { ok: false, diagnostic: diag('query', err) };
    }
  }
  const buffer = await ctx.startRendering();
  const channelsB64: string[] = [];
  for (let c = 0; c < buffer.numberOfChannels; c++) {
    channelsB64.push(float32ToBase64(buffer.getChannelData(c)));
  }
  return {
    ok: true,
    channelsB64,
    sampleRate: buffer.sampleRate,
    frames: buffer.length,
  };
}

function float32ToBase64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

declare global {
  interface Window {
    __strudelCheck(req: CheckRequest): Promise<CheckResult>;
    __strudelRender(req: RenderRequest): Promise<RenderResult>;
  }
}

window.__strudelCheck = check;
window.__strudelRender = render;
