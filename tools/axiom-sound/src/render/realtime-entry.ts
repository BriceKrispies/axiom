// The in-browser REALTIME capture entry (non-deterministic sibling of
// browser-entry.ts). Where browser-entry renders through an OfflineAudioContext,
// this plays the pattern through a *real* AudioContext in wall-clock time and
// captures the master mix as PCM. Realtime rendering matters for effects whose
// decaying tails generate denormal floats (e.g. a distorted signal into `.room()`
// reverb): Chrome's realtime audio thread flushes denormals to zero, so it does
// not suffer the pathological slowdown the offline render thread does. The trade
// is determinism — realtime uses live noise sources and wall-clock timing — which
// is why this path is a documented, opt-in escape from the deterministic tool.

import { transpiler } from '@strudel/transpiler';
import { evaluate, evalScope } from '@strudel/core';
import { initAudio, registerSynthSounds, superdough, setAudioContext } from 'superdough';

import type { RenderRequest, RenderResult } from './protocol.ts';

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

// A minimal recorder AudioWorkletProcessor: posts a copy of its (stereo) input
// block to the main thread every render quantum. Registered from an inline data
// URL so it needs no network, exactly like superdough's own worklets.
const RECORDER_SRC = [
  'class R extends AudioWorkletProcessor {',
  '  process(inputs) {',
  '    const inp = inputs[0];',
  '    if (inp && inp.length) {',
  '      const l = inp[0] ? inp[0].slice() : new Float32Array(128);',
  '      const r = inp[1] ? inp[1].slice() : l;',
  '      this.port.postMessage([l, r]);',
  '    }',
  '    return true;',
  '  }',
  '}',
  'registerProcessor("axiom-recorder", R);',
].join('\n');

function float32ToBase64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

function concat(chunks: readonly Float32Array[], frames: number): Float32Array {
  const out = new Float32Array(frames);
  let offset = 0;
  for (const c of chunks) {
    const n = Math.min(c.length, frames - offset);
    if (n <= 0) {
      break;
    }
    out.set(c.subarray(0, n), offset);
    offset += n;
  }
  return out;
}

/**
 * Play `req.code` through a realtime AudioContext for `req.seconds` and capture
 * the master mix as interleaved-free per-channel PCM. Returns the same shape as
 * the offline renderer so the Node side can encode it identically.
 */
async function realtimeRender(req: RenderRequest): Promise<RenderResult> {
  await scopeReady;
  const ctx = new AudioContext({ sampleRate: req.sampleRate });
  await ctx.resume();
  setAudioContext(ctx);
  await initAudio();
  registerSynthSounds();

  let pattern: Pattern;
  try {
    pattern = ((await evaluate(req.code, transpiler)) as { pattern: Pattern }).pattern;
  } catch (err) {
    return { ok: false, diagnostic: { phase: 'evaluate', message: String((err as Error)?.message ?? err) } };
  }
  const haps = pattern.queryArc(0, req.seconds * req.cps);

  const recorderUrl = `data:text/javascript;base64,${btoa(RECORDER_SRC)}`;
  await ctx.audioWorklet.addModule(recorderUrl);
  const recorder = new AudioWorkletNode(ctx, 'axiom-recorder', {
    numberOfInputs: 1,
    numberOfOutputs: 1,
    outputChannelCount: [2],
    channelCount: 2,
    channelCountMode: 'explicit',
    channelInterpretation: 'speakers',
  });
  const left: Float32Array[] = [];
  const right: Float32Array[] = [];
  recorder.port.onmessage = (e: MessageEvent) => {
    const [l, r] = e.data as [Float32Array, Float32Array];
    left.push(l);
    right.push(r);
  };

  // Tee the master bus into the recorder: superdough connects its destinationGain
  // to ctx.destination with native `connect`, so intercepting connections whose
  // target is ctx.destination captures the full mix. Skip the recorder's own
  // connection to avoid a feedback loop.
  const proto = AudioNode.prototype as unknown as {
    connect: (this: AudioNode, dest: AudioNode, ...rest: unknown[]) => AudioNode;
  };
  const realConnect = proto.connect;
  proto.connect = function (this: AudioNode, dest: AudioNode, ...rest: unknown[]): AudioNode {
    const result = realConnect.call(this, dest, ...rest) as AudioNode;
    (dest === ctx.destination && this !== recorder ? [1] : []).forEach(() =>
      realConnect.call(this, recorder),
    );
    return result;
  };
  const tConnect = ctx.currentTime;
  realConnect.call(recorder, ctx.destination);

  // Pre-compute the schedule (absolute AudioContext times), sorted by onset.
  const base = tConnect + 0.3;
  const events = haps
    .filter((hap) => hap.hasOnset() && hap.whole)
    .map((hap) => {
      const whole = hap.whole as NonNullable<Hap['whole']>;
      const begin = whole.begin.valueOf();
      const end = whole.end.valueOf();
      return { at: base + begin / req.cps, dur: (end - begin) / req.cps, value: hap.value, cycle: begin };
    })
    .sort((a, b) => a.at - b.at);

  // Lookahead scheduler: only realize the nodes for events within LOOKAHEAD of
  // the playhead, then sleep. This spreads node creation across the realtime
  // playback (rather than a one-shot upfront burst of thousands of nodes) and
  // keeps wall-clock ≈ the sound's own length.
  const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));
  const LOOKAHEAD = 0.5;
  const endTime = base + req.seconds;
  let idx = 0;
  while (ctx.currentTime < endTime + 0.3) {
    const horizon = ctx.currentTime + LOOKAHEAD;
    while (idx < events.length && events[idx].at <= horizon) {
      const ev = events[idx];
      idx += 1;
      await superdough(ev.value, ev.at, ev.dur, req.cps, ev.cycle);
    }
    await sleep(50);
  }
  proto.connect = realConnect;

  const frames = Math.max(1, Math.round(req.seconds * req.sampleRate));
  const startIdx = Math.max(0, Math.round((base - tConnect) * req.sampleRate));
  const fullL = concat(left, startIdx + frames).subarray(startIdx, startIdx + frames);
  const fullR = concat(right, startIdx + frames).subarray(startIdx, startIdx + frames);
  const channels = req.channels === 1 ? [fullL] : [fullL, fullR];

  // Peak guard: a full mix can sum past full scale, which the encoder rejects as
  // clipping. Attenuate — never boost — the captured mix uniformly so its peak
  // sits just under full scale, preserving the mix balance. (Absolute loudness is
  // controlled by the consuming app.)
  const CEILING = 0.97;
  let peak = 0;
  for (const ch of channels) {
    for (let i = 0; i < ch.length; i++) {
      const a = Math.abs(ch[i]);
      if (a > peak) {
        peak = a;
      }
    }
  }
  if (peak > CEILING) {
    const scale = CEILING / peak;
    for (const ch of channels) {
      for (let i = 0; i < ch.length; i++) {
        ch[i] *= scale;
      }
    }
  }

  const channelsB64 = channels.map(float32ToBase64);
  return { ok: true, channelsB64, sampleRate: req.sampleRate, frames };
}

declare global {
  interface Window {
    __realtimeRender(req: RenderRequest): Promise<RenderResult>;
  }
}

window.__realtimeRender = realtimeRender;
