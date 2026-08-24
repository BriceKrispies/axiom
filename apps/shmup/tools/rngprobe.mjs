#!/usr/bin/env node
/**
 * RNG STREAM PROBE — which subsystem's randomness moved.
 *
 * Every procedural thing in this game hangs off one deterministic stream: the
 * engine's root `Rng`, from which each subsystem forks exactly once, in
 * dependency order. A refactor that changes the NUMBER or ORDER of draws from
 * that root stream repaints the entire game — different camo, different debris,
 * different recoil — and the pixel gate reports it as "everything changed",
 * which is true and completely unhelpful for finding the cause.
 *
 * This prints the state of every subsystem's fork, so a diff of two runs names
 * the exact subsystem whose stream moved and, because forks are ordered, the
 * first one that differs is the one that gained or lost a draw.
 *
 *   node tools/rngprobe.mjs --compare=tools/rng-golden.json   # the gate
 *   node tools/rngprobe.mjs --trace                          # who forks, in order
 *   node tools/rngprobe.mjs --out=rng-new.json               # re-baseline
 *
 * `tools/rng-golden.json` is a committed snapshot of the capture-mode streams.
 * It is pure JavaScript arithmetic from a fixed seed, so it is identical on
 * every machine and every GPU — unlike the pixel gate, which needs a reference
 * capture per machine and six minutes to run. Check this first: it catches a
 * reseed in well under a minute, and a reseed is what makes all eleven shots
 * change at once. If it fails, `--trace` names the line.
 *
 * Runs in capture mode, where the root seed is fixed, so two runs of the same
 * tree must agree exactly.
 */
import { chromium } from 'playwright';
import { writeFileSync, readFileSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';

const args = Object.fromEntries(
  process.argv.slice(2).map((a) => {
    const m = a.match(/^--([^=]+)(?:=(.*))?$/);
    return m ? [m[1], m[2] ?? true] : [a, true];
  })
);

const PORT = Number(args.port ?? 5173);
const ANGLE = String(args.angle ?? (process.platform === 'darwin' ? 'metal' : 'gl'));

const browser = await chromium.launch({
  headless: true,
  args: [`--use-angle=${ANGLE}`, '--ignore-gpu-blocklist', '--mute-audio'],
});
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
// `--trace` asks the engine to record a stack for every fork of the ROOT
// stream. That is what turns "everything differs from fork N onward" into the
// one line of code that added, removed or moved a fork.
const EXTRA = `${args.trace ? '&rngtrace=1' : ''}${args.query ? `&${args.query}` : ''}`;
await page.goto(`http://127.0.0.1:${PORT}/?capture=1&lockstep=1${EXTRA}`, {
  waitUntil: 'domcontentloaded',
  timeout: 180000,
});
await page.waitForFunction('window.__READY__ === true', null, { timeout: 300000 });

const snap = await page.evaluate(() => {
  const e = window.__ENGINE__;
  const state = (r) => (r ? [r.s0 >>> 0, r.s1 >>> 0, r.s2 >>> 0, r.s3 >>> 0].join(',') : null);
  const out = { root: state(e.rng), systems: {} };
  for (const sys of e.registry.ordered) {
    const id = sys.constructor.id;
    out.systems[id] = state(sys.rng);
  }
  // World and AI publish counts that move the instant their stream does, which
  // makes an accidental reseed obvious even without a second run to diff.
  const w = e.ctx.peek('world');
  const ai = e.ctx.peek('ai');
  out.witness = {
    staticTris: w?.stats?.staticTris ?? null,
    instances: w?.stats?.instances ?? null,
    drawCalls: w?.stats?.drawCalls ?? null,
    agents: ai?.stats?.agents ?? null,
    camoMean: ai?.materials?.camoStats?.arid?.mean ?? null,
  };
  out.forkTrace = (e.rngForkTrace ?? []).map((t) =>
    t.replace(/https?:\/\/[^/]+\//g, '').replace(/\?t=\d+/g, '')
  );
  return out;
});

await browser.close();

if (args.compare) {
  const p = resolve(String(args.compare));
  if (!existsSync(p)) throw new Error(`--compare: no such file ${p}`);
  const base = JSON.parse(readFileSync(p, 'utf8'));
  const rows = [];
  const cmp = (name, a, b) => rows.push({ what: name, before: b, after: a, same: a === b });
  cmp('root(after boot)', snap.root, base.root);
  for (const id of new Set([...Object.keys(snap.systems), ...Object.keys(base.systems)])) {
    cmp(`fork:${id}`, snap.systems[id], base.systems[id]);
  }
  for (const k of Object.keys(snap.witness)) {
    cmp(`witness:${k}`, String(snap.witness[k]), String(base.witness[k]));
  }
  console.table(rows);
  const bad = rows.filter((r) => !r.same);
  console.log(bad.length ? `\n${bad.length} DIFFER — first is the culprit: ${bad[0].what}` : '\nidentical');
  process.exit(bad.length ? 1 : 0);
}

if (args.trace) {
  console.log(`ROOT FORKS: ${snap.forkTrace.length}`);
  snap.forkTrace.forEach((t, i) => {
    const site = (t.match(/at ([\w.]+|new \w+) \(([^)]+)\)/) ?? [])[0] ?? t;
    console.log(`  ${String(i + 1).padStart(2)}  ${site}`);
  });
  console.log('');
}

const text = JSON.stringify(snap, null, 2);
if (args.out) {
  writeFileSync(resolve(String(args.out)), text);
  console.error(`wrote ${args.out}`);
} else {
  console.log(text);
}
