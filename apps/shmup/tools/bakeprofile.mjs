#!/usr/bin/env node
/**
 * BAKE PROFILER — function-level attribution for the worker bakes.
 *
 * WHY THIS EXISTS SEPARATELY. `tools/bootprofile.mjs` profiles the main thread.
 * The bakes do not run there any more — they were moved to a worker pool
 * precisely so they would not — and the boot profile can therefore only report
 * them as a total: "3.9 s of worker CPU". Once the rest of boot got fast enough
 * that `fx:atlases.await` started sitting idle waiting on those workers, that
 * total became the thing to attack, and a total is not attribution.
 *
 * Profiling a worker over CDP is possible but awkward (flat-mode auto-attach
 * needs per-session routing that the driver does not expose). It is also
 * unnecessary: every baker in `src/bakers.js` is a pure function from a seed to
 * typed arrays, with no DOM, no WebGL and no browser API of any kind. That is
 * exactly what makes them worker-safe, and it also makes them RUNNABLE IN NODE,
 * on the same V8, under a real sampling profiler.
 *
 *   node --cpu-prof --cpu-prof-dir=.bakeprof tools/bakeprofile.mjs
 *   node tools/bakeprofile.mjs --report .bakeprof/<file>.cpuprofile
 *
 * Or just `node tools/bakeprofile.mjs`, which times each baker without
 * sampling — enough to see which one dominates.
 */
import { performance } from 'node:perf_hooks';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';

import { BAKERS } from '../src/bakers.js';
import { SOLDIER_SHARDS } from '../src/ai/bake.js';

const args = Object.fromEntries(
  process.argv.slice(2).map((a) => {
    const m = a.match(/^--([^=]+)(?:=(.*))?$/);
    return m ? [m[1], m[2] ?? true] : [a, true];
  })
);

/**
 * The exact jobs boot queues, with the sizes it uses.
 *
 * Seeds are arbitrary here — the cost of a bake does not depend on the seed,
 * only on the size and the surface — but they are fixed so two runs of this
 * tool are comparable.
 */
const JOBS = [
  { kind: 'fx:particle-atlas', payload: { seed: 1, size: 1024 } },
  { kind: 'fx:decal-atlas', payload: { seed: 2, size: 1024 } },
  ...SOLDIER_SHARDS.map((only, i) => ({
    kind: 'ai:soldier-sets',
    label: `ai:soldier-sets[${i}]`,
    payload: { nzSeed: 3, size: 512, camo: ['arid', 'woodland', 'urban'], only },
  })),
];

// ---------------------------------------------------------------- reporting --
/** Fold a V8 `.cpuprofile` into self time per function. */
function report(file) {
  const cp = JSON.parse(readFileSync(file, 'utf8'));
  const byId = new Map(cp.nodes.map((n) => [n.id, n]));
  const self = new Map();
  let total = 0;
  for (let i = 0; i < cp.samples.length; i++) {
    const dt = (cp.timeDeltas[i] ?? 0) / 1000;
    const n = byId.get(cp.samples[i]);
    if (!n) continue;
    const f = n.callFrame;
    const file_ = (f.url || '').replace(/^file:\/+/, '').split(/[\\/]/).slice(-2).join('/');
    const key = `${f.functionName || '(anonymous)'}  ${file_ || '(native)'}`;
    self.set(key, (self.get(key) ?? 0) + dt);
    total += dt;
  }
  console.log(`\nBAKE PROFILE — ${total.toFixed(0)}ms sampled\n`);
  console.log('  self time by function');
  for (const [k, v] of [...self.entries()].sort((a, b) => b[1] - a[1]).slice(0, 22)) {
    if (v < 2) break;
    console.log(`  ${v.toFixed(0).padStart(6)}ms ${((v / total) * 100).toFixed(1).padStart(5)}%  ${k}`);
  }
}

if (args.report) {
  report(args.report === true ? newestProfile() : String(args.report));
  process.exit(0);
}

function newestProfile(dir = '.bakeprof') {
  if (!existsSync(dir)) {
    console.error(`no ${dir}/ — run: node --cpu-prof --cpu-prof-dir=${dir} tools/bakeprofile.mjs`);
    process.exit(1);
  }
  const files = readdirSync(dir).filter((f) => f.endsWith('.cpuprofile'));
  if (!files.length) {
    console.error(`${dir}/ has no .cpuprofile`);
    process.exit(1);
  }
  return join(dir, files.sort().at(-1));
}

// ------------------------------------------------------------------- timing --
const rounds = Number(args.rounds ?? 1);
const results = [];
for (let r = 0; r < rounds; r++) {
  for (const job of JOBS) {
    const t0 = performance.now();
    const out = BAKERS[job.kind](job.payload);
    const ms = performance.now() - t0;
    const bytes = countBytes(out);
    if (r === rounds - 1) results.push({ label: job.label ?? job.kind, ms, bytes });
  }
}

function countBytes(v, seen = new Set()) {
  if (!v) return 0;
  if (ArrayBuffer.isView(v)) return seen.has(v.buffer) ? 0 : (seen.add(v.buffer), v.byteLength);
  if (Array.isArray(v)) return v.reduce((a, x) => a + countBytes(x, seen), 0);
  if (typeof v === 'object') return Object.values(v).reduce((a, x) => a + countBytes(x, seen), 0);
  return 0;
}

const total = results.reduce((a, r) => a + r.ms, 0);
console.log(`\nBAKE COST — ${results.length} jobs, ${total.toFixed(0)}ms of CPU total\n`);
for (const r of results.sort((a, b) => b.ms - a.ms)) {
  console.log(
    `  ${r.ms.toFixed(0).padStart(6)}ms ${((r.ms / total) * 100).toFixed(1).padStart(5)}%  ` +
      `${r.label.padEnd(22)} ${(r.bytes / (1024 * 1024)).toFixed(1)}MB out`
  );
}
console.log(
  `\n  Wall time across a pool of N workers is roughly the largest shard, not the sum:\n` +
    `  slowest job ${Math.max(...results.map((r) => r.ms)).toFixed(0)}ms.\n`
);
console.log('  For function-level attribution:');
console.log('    node --cpu-prof --cpu-prof-dir=.bakeprof tools/bakeprofile.mjs');
console.log('    node tools/bakeprofile.mjs --report\n');
