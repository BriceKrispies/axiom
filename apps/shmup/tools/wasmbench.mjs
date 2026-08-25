#!/usr/bin/env node
/**
 * JS vs wasm on the bake hot path — the measurement that decides whether
 * porting the rest is worth doing.
 *
 * `tools/bakeprofile.mjs` found that 54% of the 4 s of worker bake CPU is three
 * functions: `fbm`, `ridge` and the loop around them. This runs the same
 * arithmetic both ways and reports the ratio, plus whether the two agree BIT
 * FOR BIT — which is the harder and more important question. A 3x speedup that
 * needs the pixel gate re-baselined is a much worse deal than it looks, because
 * every future comparison against that baseline is then comparing against a
 * number nobody verified.
 *
 *   node tools/wasmbench.mjs [--n=400000] [--reps=3]
 */
import { readFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { TileNoise } from '../src/ai/bake.js';
import { Rng } from '../src/core/rng.js';

const here = dirname(fileURLToPath(import.meta.url));
const WASM = join(here, '..', 'bake-rs', 'target', 'wasm32-unknown-unknown', 'release', 'shmup_bake.wasm');

const args = Object.fromEntries(
  process.argv.slice(2).map((a) => {
    const m = a.match(/^--([^=]+)(?:=(.*))?$/);
    return m ? [m[1], m[2] ?? true] : [a, true];
  })
);
const N = Number(args.n ?? 400_000);
const REPS = Number(args.reps ?? 3);
const SEED = 12345;

let wasm;
try {
  const mod = new WebAssembly.Module(readFileSync(WASM));
  wasm = new WebAssembly.Instance(mod, {}).exports;
} catch (err) {
  console.error(`could not load ${WASM}\n  ${err.message}`);
  console.error('  build it:  cargo build --manifest-path bake-rs/Cargo.toml --target wasm32-unknown-unknown --release');
  process.exit(1);
}

/** The JS side, written to mirror the wasm entry point exactly. */
const jsBench = (kind, seed, n, period, oct) => {
  const nz = new TileNoise(new Rng(seed));
  let acc = 0;
  const inv = 1 / n;
  for (let i = 0; i < n; i++) {
    const u = i * inv;
    const v = (i * 0.61803398875) % 1;
    acc += kind === 'fbm' ? nz.fbm(u, v, period, oct) : nz.ridge(u, v, period, oct);
  }
  return acc;
};

const best = (fn) => {
  let ms = Infinity;
  let out;
  for (let r = 0; r < REPS; r++) {
    const t = performance.now();
    out = fn();
    ms = Math.min(ms, performance.now() - t);
  }
  return { ms, out };
};

console.log(`\nJS vs WASM — ${N.toLocaleString()} samples, best of ${REPS}\n`);
let allExact = true;

for (const [kind, period, oct] of [['fbm', 24, 4], ['fbm', 96, 2], ['ridge', 32, 3]]) {
  const js = best(() => jsBench(kind, SEED, N, period, oct));
  const rs = best(() =>
    kind === 'fbm' ? wasm.bench_fbm(SEED, N, period, oct) : wasm.bench_ridge(SEED, N, period, oct)
  );
  // Bit-exact, not approximately equal: compare the IEEE payloads.
  const a = new Float64Array([js.out]);
  const b = new Float64Array([rs.out]);
  const exact = new BigUint64Array(a.buffer)[0] === new BigUint64Array(b.buffer)[0];
  allExact &&= exact;
  console.log(
    `  ${kind}(period=${period}, oct=${oct})`.padEnd(30) +
      `js ${js.ms.toFixed(0).padStart(5)}ms   wasm ${rs.ms.toFixed(0).padStart(5)}ms   ` +
      `${(js.ms / rs.ms).toFixed(2)}x   ` +
      (exact ? 'bit-exact' : `MISMATCH js=${js.out} wasm=${rs.out}`)
  );
}

console.log(
  `\n  ${allExact ? 'Every result is bit-identical to the JavaScript.' : 'DIVERGENCE — the port is not a drop-in.'}\n`
);
