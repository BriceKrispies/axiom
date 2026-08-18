/**
 * Golden capture for the Claude-of-Duty audio port.
 *
 * Runs the ORIGINAL `C:/dev/Claude-of-Duty/src/audio/*.js` under Node against a
 * recording stub of `BaseAudioContext`, and dumps every number the Rust port
 * must reproduce: the DSP scalars, the noise fills, the waveshaper curves, the
 * envelope automation, whole rendered impulse responses, `classifySpace` over
 * seven room shapes, the lookup tables — and, for two dozen voices, the ENTIRE
 * graph the JavaScript builds for a given seed: node list in creation order
 * with every constructed parameter, every connection, every automation event
 * and every source start.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 *
 * It reads the source by absolute path and writes nothing but stdout, so it is
 * safe to re-run at any time. If the output differs from the committed
 * `golden.json`, either the source changed or Node did — investigate before
 * committing the new file, because `tests/audio_port.rs` is pinned to it.
 *
 * The recording stub below is a faithful but minimal `BaseAudioContext`: every
 * factory the source calls, `AudioParam` with its five automation methods and a
 * `value` setter, `connect` to a node or a param, and `start`/`stop`. It records
 * rather than renders, which is exactly what `audio::graph::AudioGraph` does on
 * the Rust side — the two are directly comparable by construction.
 */

import { Rng } from 'file:///C:/dev/Claude-of-Duty/src/core/rng.js';
import {
  fillNoise, NoiseBank, hit, ad, adsr, sweep, saturationCurve, limiterCurve,
  airCutoff, semis, dbToGain, clamp, lerp, SPEED_OF_SOUND, struckResonator,
} from 'file:///C:/dev/Claude-of-Duty/src/audio/dsp.js';
import { IR_SPECS, generateIR, classifySpace, SPACE_KEYS } from 'file:///C:/dev/Claude-of-Duty/src/audio/ir.js';
import { WEAPON_PROFILES, weaponShot, bulletWhizz, dryFire, resolveProfile } from 'file:///C:/dev/Claude-of-Duty/src/audio/weapons.js';
import {
  surfaceImpact, footstep, shellCasing, reloadPhase, explosion, bodyFall,
  uiSound, heartbeat, cloth,
} from 'file:///C:/dev/Claude-of-Duty/src/audio/foley.js';
import { bark, BARKS, barkFor } from 'file:///C:/dev/Claude-of-Duty/src/audio/vox.js';
import { ambientOneShot, ONE_SHOTS } from 'file:///C:/dev/Claude-of-Duty/src/audio/ambience.js';

const SR = 48000;

/* ------------------------------------------------------------------ */
/* Recording stub of BaseAudioContext                                  */
/* ------------------------------------------------------------------ */

let G = null;

function newGraph() {
  return { nodes: [], conns: [], autos: [], sched: [], buffers: [], curves: [], waves: [] };
}

class Param {
  constructor(node, name, v) { this.node = node; this.name = name; this._v = v; }
  get value() { return this._v; }
  set value(v) { this._v = v; G.nodes[this.node].params[this.name] = v; }
  setValueAtTime(v, t) { G.autos.push([this.node, this.name, 'set', v, t]); return this; }
  exponentialRampToValueAtTime(v, t) { G.autos.push([this.node, this.name, 'expo', v, t]); return this; }
  linearRampToValueAtTime(v, t) { G.autos.push([this.node, this.name, 'lin', v, t]); return this; }
  setTargetAtTime(v, t, tc) { G.autos.push([this.node, this.name, 'target', v, t, tc]); return this; }
  cancelScheduledValues(t) { G.autos.push([this.node, this.name, 'cancel', 0, t]); return this; }
}

class Node {
  constructor(kind, params, fields) {
    this.id = G.nodes.length;
    this.kind = kind;
    this.params = { ...params };
    G.nodes.push({ id: this.id, kind, params: this.params, fields: fields ?? {} });
    for (const k in params) this[k] = new Param(this.id, k, params[k]);
    this._rec = G.nodes[this.id];
  }
  connect(target) {
    if (target instanceof Param) G.conns.push([this.id, 'param', target.node, target.name]);
    else G.conns.push([this.id, 'node', target.id, '']);
    return target;
  }
  disconnect() { }
  start(when, offset, duration) { G.sched.push([this.id, 'start', when ?? 0, offset ?? null, duration ?? null]); }
  stop(when) { G.sched.push([this.id, 'stop', when ?? 0, null, null]); }
}

function fieldProxy(node) {
  // Mirror plain (non-AudioParam) assignments into the record.
  return new Proxy(node, {
    set(t, k, v) {
      if (typeof k === 'string' && !(t[k] instanceof Param) && k !== 'id' && k !== 'kind' && k !== 'params' && k !== '_rec') {
        if (k === 'buffer') t._rec.fields.buffer = v?.__id ?? null;
        // Curve identity is recorded as its LENGTH, not an index. dsp.js's
        // CURVE_CACHE is module-global and keyed on `drive.toFixed(2)`, so two
        // different runs of this script (and two different drives that round to
        // the same key) legitimately share one array; an index would compare
        // capture-run bookkeeping rather than anything the port must reproduce.
        // The curve VALUES are pinned exactly, separately, in `out.curves`.
        else if (k === 'curve') t._rec.fields.curve = v.length;
        else t._rec.fields[k] = v;
      }
      t[k] = v;
      return true;
    },
  });
}

const CURVE_IDS = new Map();
function curveId(c) {
  if (!c) return null;
  if (CURVE_IDS.has(c)) return CURVE_IDS.get(c);
  const id = CURVE_IDS.size;
  CURVE_IDS.set(c, id);
  return id;
}

class Ctx {
  constructor(sr = SR) { this.sampleRate = sr; this.currentTime = 0; this.destination = null; }
  createBuffer(ch, len, sr) {
    const data = [];
    for (let i = 0; i < ch; i++) data.push(new Float32Array(len));
    const id = G.buffers.length;
    const b = { __id: id, numberOfChannels: ch, length: len, sampleRate: sr, duration: len / sr, getChannelData: (i) => data[i] };
    G.buffers.push(b);
    return b;
  }
  createBufferSource() { return fieldProxy(new Node('bufferSource', { playbackRate: 1, detune: 0 })); }
  createGain() { return fieldProxy(new Node('gain', { gain: 1 })); }
  createBiquadFilter() { return fieldProxy(new Node('biquad', { frequency: 350, Q: 1, gain: 0, detune: 0 })); }
  createOscillator() {
    const n = fieldProxy(new Node('oscillator', { frequency: 440, detune: 0 }));
    n.setPeriodicWave = (w) => { n._rec.fields.wave = w.__id; };
    return n;
  }
  createWaveShaper() { return fieldProxy(new Node('waveShaper', {})); }
  createConvolver() { return fieldProxy(new Node('convolver', {})); }
  createDynamicsCompressor() { return fieldProxy(new Node('compressor', { threshold: -24, knee: 30, ratio: 12, attack: 0.003, release: 0.25 })); }
  createStereoPanner() { return fieldProxy(new Node('stereoPanner', { pan: 0 })); }
  createPanner() { return fieldProxy(new Node('panner', { positionX: 0, positionY: 0, positionZ: 0 })); }
  createPeriodicWave(real, imag) {
    const id = G.waves.length;
    G.waves.push({ __id: id, real: [...real], imag: [...imag] });
    return G.waves[id];
  }
}

function record(fn) {
  const prev = G;
  G = newGraph();
  const ctx = new Ctx();
  const r = fn(ctx);
  const out = G;
  G = prev;
  return { graph: out, ret: r };
}

/** Reduce a recorded graph to a compact, comparable JSON shape. */
function slim(g, ret) {
  return {
    nodes: g.nodes.map((n) => ({ i: n.id, k: n.kind, p: round(n.params), f: round(n.fields) })),
    conns: g.conns,
    autos: g.autos.map((a) => a.map((x) => (typeof x === 'number' ? r12(x) : x))),
    sched: g.sched.map((a) => a.map((x) => (typeof x === 'number' ? r12(x) : x))),
    ret: ret ? { end: r12(ret.end), send: r12(ret.send), node: ret.node.id } : null,
  };
}
// Full precision: JSON.stringify already emits the shortest round-tripping
// decimal for an f64, so the goldens are EXACT. Rounding here would quietly
// weaken every comparison downstream — an f32 sample needs 17 significant
// digits to survive the trip through f64.
function r12(x) { return Number.isFinite(x) ? x : String(x); }
function round(o) { const q = {}; for (const k in o) q[k] = typeof o[k] === 'number' ? r12(o[k]) : o[k]; return q; }

/* ------------------------------------------------------------------ */
/* Goldens                                                             */
/* ------------------------------------------------------------------ */

const out = {};

/* -- dsp: scalar helpers ------------------------------------------- */
out.speedOfSound = SPEED_OF_SOUND;
out.airCutoff = [0, 1, 5, 12.5, 50, 100, 300, 1000, 5000].map((d) => [d, r12(airCutoff(d))]);
out.semis = [-12, -3.5, -1.1, 0, 0.45, 1, 7, 12].map((n) => [n, r12(semis(n))]);
out.dbToGain = [-60, -22, -6, 0, 6].map((d) => [d, r12(dbToGain(d))]);
out.clamp = [[-1, 0, 1, r12(clamp(-1, 0, 1))], [0.5, 0, 1, r12(clamp(0.5, 0, 1))], [4, 0, 1, r12(clamp(4, 0, 1))]];
out.lerp = [[2200, 700, 0.3, r12(lerp(2200, 700, 0.3))]];

/* -- dsp: noise fills ---------------------------------------------- */
out.noise = {};
for (const kind of ['white', 'pink', 'brown', 'crackle']) {
  const n = kind === 'crackle' ? 4096 : 64;
  const buf = new Float32Array(n);
  fillNoise(buf, kind, new Rng(0x1234abcd));
  // Report the head, plus a checksum over the whole buffer.
  out.noise[kind] = {
    n,
    head: [...buf.slice(0, 24)].map((v) => r12(v)),
    tail: [...buf.slice(n - 8)].map((v) => r12(v)),
    sum: r12([...buf].reduce((s, v) => s + v, 0)),
    absSum: r12([...buf].reduce((s, v) => s + Math.abs(v), 0)),
  };
}
// A default/unknown kind falls through to white.
{
  const a = new Float32Array(16); fillNoise(a, 'nonsense', new Rng(7));
  const b = new Float32Array(16); fillNoise(b, 'white', new Rng(7));
  out.noiseDefaultIsWhite = [...a].every((v, i) => v === b[i]);
}

/* -- dsp: curves ---------------------------------------------------- */
out.curves = {};
for (const [drive, asym] of [[4, 0], [6, 0.35], [2.5, 0.2], [14, 0.7], [1.6, 0.35]]) {
  const c = saturationCurve(drive, asym);
  out.curves[`sat:${drive}:${asym}`] = {
    n: c.length,
    samples: [0, 1, 256, 512, 1023, 1024, 1536, 2046, 2047].map((i) => [i, r12(c[i])]),
  };
}
// The two-decimal cache key: two different drives that round to the same key
// share one curve, and the FIRST caller's exact drive is the one that shapes it.
out.curveCacheShares = saturationCurve(6.144, 0.351) === saturationCurve(6.1449, 0.3512);
out.curveCacheDistinct = saturationCurve(6.144, 0.351) === saturationCurve(6.149, 0.351);
{
  const c = limiterCurve();
  out.curves.limiter = {
    n: c.length,
    samples: [0, 1, 512, 1024, 2047, 2048, 3072, 4094, 4095].map((i) => [i, r12(c[i])]),
  };
}

/* -- dsp: envelopes ------------------------------------------------- */
function envCase(fn) {
  const calls = [];
  const p = {
    setValueAtTime: (v, t) => calls.push(['set', r12(v), r12(t)]),
    exponentialRampToValueAtTime: (v, t) => calls.push(['expo', r12(v), r12(t)]),
    setTargetAtTime: (v, t, tc) => calls.push(['target', r12(v), r12(t), r12(tc)]),
  };
  const ret = fn(p);
  return { calls, ret: r12(ret) };
}
out.env = {
  hit: envCase((p) => hit(p, 0.02, 0.9, 0.0075)),
  hitTinyPeak: envCase((p) => hit(p, 0.02, 1e-9, 0.01)),
  hitNaN: envCase((p) => hit(p, NaN, 0.5, 0.01)),
  hitNegT: envCase((p) => hit(p, -0.001, 0.5, 0.01)),
  adLongAttack: envCase((p) => ad(p, 0.02, 0.8, 0.012, 0.13)),
  adShortAttack: envCase((p) => ad(p, 0.02, 0.8, 0.0005, 0.13)),
  adsr: envCase((p) => adsr(p, 0.02, 0.5, 0.014, 0.03, 0.07, 0.72, 0.055)),
  sweep: envCase((p) => sweep(p, 0.02, 620, 190, 0.28)),
  sweepFloor: envCase((p) => sweep(p, 0.02, 0.0001, 0, 0.0)),
  sweepBadTo: envCase((p) => sweep(p, 0.02, 100, NaN, 0.1)),
};

/* -- ir: generateIR ------------------------------------------------- */
out.irSpecs = IR_SPECS;
out.spaceKeys = SPACE_KEYS;
out.ir = {};
for (const key of SPACE_KEYS) {
  const ctx = new Ctx();
  G = newGraph();
  const buf = generateIR(ctx, new Rng(0x1234 + key.length), IR_SPECS[key]);
  G = null;
  const ch0 = buf.getChannelData(0), ch1 = buf.getChannelData(1);
  const probe = (d) => {
    let peak = 0, sum = 0, sq = 0;
    for (let i = 0; i < d.length; i++) { const a = Math.abs(d[i]); if (a > peak) peak = a; sum += d[i]; sq += d[i] * d[i]; }
    return { peak: r12(peak), sum: r12(sum), rms: r12(Math.sqrt(sq / d.length)) };
  };
  out.ir[key] = {
    length: buf.length,
    ch0: probe(ch0), ch1: probe(ch1),
    // Exact samples at fixed indices — the strongest possible pin.
    samples0: [0, 1, 100, 480, 1000, 4800, 9600, buf.length - 1].map((i) => [i, r12(ch0[i])]),
    samples1: [0, 1, 100, 480, 1000, 4800, 9600, buf.length - 1].map((i) => [i, r12(ch1[i])]),
  };
}
// A tiny synthetic spec, so a Rust test can compare EVERY sample.
{
  const spec = {
    seconds: 0.01, rt60: 0.05, predelay: 0.001, hfDamp: 0.5, bright: 0.5,
    diffusion: 0.6, width: 0.5, taps: [0.002, 0.004], tapGain: 0.7, slaps: 2, slapTime: 0.003,
  };
  const ctx = new Ctx();
  G = newGraph();
  const buf = generateIR(ctx, new Rng(99), spec);
  G = null;
  out.irTiny = {
    spec,
    length: buf.length,
    ch0: [...buf.getChannelData(0)].map((v) => r12(v)),
    ch1: [...buf.getChannelData(1)].map((v) => r12(v)),
  };
}

/* -- ir: classifySpace ---------------------------------------------- */
out.classify = {};
{
  const cases = {
    smallRoom: [new Array(9).fill(3.5).map((v, i) => (i === 8 ? 2.6 : v)), 40],
    street: [[4, 30, 40, 30, 4, 30, 40, 30, 40], 40],
    open: [new Array(9).fill(40), 40],
    corridor: [[1.8, 12, 38, 12, 1.8, 12, 38, 12, 2.4], 40],
    infinite: [[Infinity, Infinity, Infinity, Infinity, Infinity, Infinity, Infinity, Infinity, Infinity], 40],
    degenerate: [[0, 0, 0, 0, 0, 0, 0, 0, 0], 40],
    evenHoriz: [[3, 4, 5, 6, 7, 8, 2.5], 40],
  };
  for (const k in cases) {
    const [hits, max] = cases[k];
    const w = classifySpace(hits, max, null);
    out.classify[k] = round(w);
  }
}

/* -- voices: full recorded graphs ------------------------------------ */
out.voices = {};
const voice = (name, seed, fn) => {
  const { graph, ret } = record((ctx) => {
    const rng = new Rng(seed);
    const bank = new NoiseBank(ctx, rng.fork(), 1.2);
    return fn(ctx, bank, rng);
  });
  out.voices[name] = slim(graph, ret);
};

// A fresh module-level `_rr` cache per profile would leak between cases, so we
// clear it: the port builds the round robin per WeaponAudio instance.
const clearRR = () => { for (const k in WEAPON_PROFILES) { delete WEAPON_PROFILES[k]._rr; delete WEAPON_PROFILES[k]._rrIndex; } };

clearRR();
voice('shot:rifle@2m', 0xA0D10, (c, b, r) => weaponShot(c, b, r, WEAPON_PROFILES.rifle, { when: 0.02, distance: 2, firstPerson: true }));
clearRR();
voice('shot:rifle@120m', 0xA0D17, (c, b, r) => weaponShot(c, b, r, WEAPON_PROFILES.rifle, { when: 0.02, distance: 120 }));
clearRR();
voice('shot:shotgun@2m', 0xA0D1E, (c, b, r) => weaponShot(c, b, r, WEAPON_PROFILES.shotgun, { when: 0.02, distance: 2, firstPerson: true }));
clearRR();
voice('shot:suppressed@1m', 0xA0D25, (c, b, r) => weaponShot(c, b, r, WEAPON_PROFILES.suppressed, { when: 0.02, distance: 1, firstPerson: true }));
clearRR();
// Two consecutive rifle shots — pins the round-robin advance.
voice('shot:rifle:x2', 0xA0D2C, (c, b, r) => {
  weaponShot(c, b, r, WEAPON_PROFILES.rifle, { when: 0.02, distance: 2, firstPerson: true });
  return weaponShot(c, b, r, WEAPON_PROFILES.rifle, { when: 0.12, distance: 2, firstPerson: true });
});
voice('whizz', 0xA0D33, (c, b, r) => bulletWhizz(c, b, r, { when: 0.02, miss: 1.2 }));
voice('dryfire', 0xA0D3A, (c, b, r) => dryFire(c, b, r, { when: 0.02 }));

for (const s of ['concrete', 'metal', 'wood', 'dirt', 'sand', 'glass', 'water', 'foliage', 'fabric', 'flesh', 'rubber', 'plaster']) {
  voice(`impact:${s}`, 0xB0000 + s.length * 977, (c, b, r) => surfaceImpact(c, b, r, { when: 0.02, surface: s, energy: 1 }));
}
for (const gait of ['walk', 'run', 'sprint', 'crouch', 'land']) {
  voice(`step:concrete:${gait}`, 0xC0000 + gait.length * 977, (c, b, r) => footstep(c, b, r, { when: 0.02, surface: 'concrete', gait }));
}
voice('step:metal:run', 0xC1234, (c, b, r) => footstep(c, b, r, { when: 0.02, surface: 'metal', gait: 'run' }));
voice('shell:concrete', 0xD0001, (c, b, r) => shellCasing(c, b, r, { when: 0.02, surface: 'concrete' }));
voice('shell:dirt', 0xD0002, (c, b, r) => shellCasing(c, b, r, { when: 0.02, surface: 'dirt' }));
for (const ph of ['start', 'magout', 'magin', 'end']) {
  voice(`reload:${ph}`, 0xE0000 + ph.length * 977, (c, b, r) => reloadPhase(c, b, r, ph, { when: 0.02 }));
}
voice('explosion@5m', 0xF0001, (c, b, r) => explosion(c, b, r, { when: 0.02, distance: 5, radius: 8 }));
voice('explosion@180m', 0xF0002, (c, b, r) => explosion(c, b, r, { when: 0.02, distance: 180, radius: 12 }));
voice('bodyfall', 0xF0003, (c, b, r) => bodyFall(c, b, r, { when: 0.02 }));
voice('cloth', 0xF0004, (c, b, r) => cloth(c, b, r, { when: 0.02 }));
voice('heartbeat', 0xF0005, (c, b, r) => heartbeat(c, b, r, { when: 0.02 }));
for (const k of ['hitmarker', 'headshot', 'kill', 'damage', 'armour', 'grenade_warn', 'regen', 'lowhealth', 'blip']) {
  voice(`ui:${k}`, 0x10000 + k.length * 977, (c, b, r) => uiSound(c, b, r, k, { when: 0.02 }));
}
for (const k of Object.keys(BARKS)) {
  voice(`bark:${k}`, 0x20000 + k.length * 977, (c, b, r) => bark(c, b, r, { when: 0.02, bark: k }));
}
voice('bark:radio', 0x2FFFF, (c, b, r) => bark(c, b, r, { when: 0.02, bark: 'contact', radio: true }));
for (const k of ONE_SHOTS) {
  voice(`ambient:${k}`, 0x30000 + k.length * 977, (c, b, r) => ambientOneShot(c, b, r, k, { when: 0.02 }));
}
voice('resonator', 0x40001, (c, b, r) => ({
  node: struckResonator(c, b, r, 0.02, [
    { f: 1750, q: 34, g: 0.42, decay: 0.28 },
    { f: 3120 },
  ], 0.0035),
  end: 0, send: 0,
}));

/* -- NoiseBank ------------------------------------------------------- */
{
  const { graph } = record((ctx) => {
    const rng = new Rng(0x51ee7);
    const bank = new NoiseBank(ctx, rng, 1.2);
    const s = bank.source('pink', rng, 1.3, true);
    const s2 = bank.source('nope', null, 1, false);
    return { node: s, end: s._offset, send: s2._offset };
  });
  out.bank = {
    buffers: graph.buffers.map((b) => ({ ch: b.numberOfChannels, len: b.length, dur: r12(b.duration) })),
    nodes: graph.nodes.map((n) => ({ k: n.kind, p: round(n.params), f: round(n.fields) })),
  };
  // Recompute the offsets deterministically for the pin.
  const rng = new Rng(0x51ee7);
  const c = new Ctx();
  G = newGraph();
  const bank = new NoiseBank(c, rng, 1.2);
  const s = bank.source('pink', rng, 1.3, true);
  out.bank.offset = r12(s._offset);
  out.bank.duration = r12(bank.buffers.pink.duration);
  G = null;
}

/* -- resolveProfile / barkFor ---------------------------------------- */
out.resolveProfile = {};
for (const n of ['rifle', 'AK', 'akm', 'scar', 'mp5', 'UMP', 'glock', 'deagle', 'spas', 'awp', 'm249', 'silenced_mp7', 'nonsense', '']) {
  const p = resolveProfile(n);
  out.resolveProfile[n || '<empty>'] = Object.keys(WEAPON_PROFILES).find((k) => WEAPON_PROFILES[k] === p);
}
out.resolveProfileNull = Object.keys(WEAPON_PROFILES).find((k) => WEAPON_PROFILES[k] === resolveProfile(null));
out.barkFor = {};
{
  const rng = new Rng(0x8a12);
  for (const k of ['spot', 'reload', 'grenade', 'flank', 'suppress', 'advance', 'hurt', 'death', 'copy', 'unknown']) {
    out.barkFor[k] = [barkFor(k, rng), barkFor(k, rng), barkFor(k, rng)];
  }
}
out.weaponProfiles = WEAPON_PROFILES;
out.barks = BARKS;
out.oneShots = ONE_SHOTS;

process.stdout.write(JSON.stringify(out, null, 1));
