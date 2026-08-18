/**
 * Golden capture for the `kit.js` modular-building-kit port.
 *
 * Runs the ORIGINAL `C:/dev/Claude-of-Duty/src/world/kit.js` (plus its
 * `util.js`/`builder.js`/`palette.js` dependencies) under Node and dumps:
 *
 *  - `solidSlabs` output rectangles for four opening layouts.
 *  - `windowState`'s selection distribution across a sweep of
 *    floor/damage/allowLit combinations (500 draws per combination, from a
 *    fixed per-combination seed) — the recipe's "the distribution matters".
 *  - Each element builder's (`facadeWall`, `windowUnit` at every state,
 *    `doorUnit`, `shopfront`, `balcony`, `parapet`, `stairRun`,
 *    `stripedCloth`, `awning`, `drainpipe`, `rubbleMound`) per-palette-key
 *    vertex/triangle counts, for a fixed panel matrix and a fixed seed.
 *  - `pockGeometry`/`spallPatch` raw geometry dumps (position/normal/color).
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

import * as THREE from 'file:///C:/dev/Claude-of-Duty/node_modules/three/build/three.module.js';
import { Rng } from 'file:///C:/dev/Claude-of-Duty/src/core/rng.js';
import { trs as trsUtil } from 'file:///C:/dev/Claude-of-Duty/src/world/util.js';
import { Assembler } from 'file:///C:/dev/Claude-of-Duty/src/world/builder.js';
import {
  facadeWall,
  windowState,
  windowUnit,
  doorUnit,
  shopfront,
  balcony,
  parapet,
  stairRun,
  stripedCloth,
  awning,
  drainpipe,
  pockGeometry,
  spallPatch,
  rubbleMound,
} from 'file:///C:/dev/Claude-of-Duty/src/world/kit.js';
import { solidSlabs } from 'file:///C:/dev/Claude-of-Duty/src/world/util.js';

const out = {};

function newAssembler(seed) {
  const materials = { get: () => new THREE.MeshBasicMaterial() };
  return new Assembler({ materials, rng: new Rng(seed), render: null });
}

/** Dump `A.finalize(root, null)`'s per-key static-mesh vertex/tri counts. */
function finalizeBuckets(A) {
  const root = new THREE.Group();
  A.finalize(root, null);
  const buckets = root.children
    .filter((c) => c.isMesh)
    .map((c) => ({
      key: c.name.replace(/^world_/, ''),
      verts: c.geometry.getAttribute('position').count,
      tris: c.geometry.index ? c.geometry.index.count / 3 : c.geometry.getAttribute('position').count / 3,
    }));
  buckets.sort((a, b) => a.key.localeCompare(b.key));
  return { buckets, stats: A.stats };
}

function dumpGeo(g) {
  const pa = g.getAttribute('position');
  const na = g.getAttribute('normal');
  const ca = g.getAttribute('color');
  return {
    pos: Array.from(pa.array),
    normal: na ? Array.from(na.array) : null,
    color: ca ? Array.from(ca.array) : null,
    index: g.index ? Array.from(g.index.array) : null,
  };
}

// The fixed panel matrix shared by every element that takes `pm`: a
// non-trivial (translated + yawed) panel->level transform, so the ported
// code's own panel-space composition is genuinely exercised.
function fixedPanelMatrix() {
  const m = new THREE.Matrix4();
  trsUtil(m, 1.2, 0.4, 3.4, 0.3, 1, 1, 1, 0, 0);
  return m;
}

// =============================================================== solidSlabs ==
out.solid_slabs = {
  no_holes: solidSlabs(2.0, 3.0, []),
  one_centered_hole: solidSlabs(2.0, 3.0, [{ x: 0, y: 1.5, w: 0.6, h: 0.8 }]),
  two_holes: solidSlabs(4.0, 3.0, [
    { x: -1.0, y: 1.0, w: 0.8, h: 1.4 },
    { x: 1.0, y: 1.0, w: 0.8, h: 1.4 },
  ]),
  hole_touching_edge: solidSlabs(2.0, 2.0, [{ x: -1.0, y: 1.0, w: 1.0, h: 2.0 }]),
};

// ============================================================== windowState ==
{
  const floors = [-1, 0, 1, 2];
  const damages = [0.0, 0.2, 0.5, 0.8];
  const allowLitOptions = [true, false];
  const N = 500;
  const distribution = [];
  for (const floor of floors) {
    for (const damage of damages) {
      for (const allowLit of allowLitOptions) {
        const rng = new Rng(0xc0ffee);
        const counts = {};
        for (let i = 0; i < N; i++) {
          const s = windowState(rng, floor, damage, { allowLit });
          counts[s] = (counts[s] ?? 0) + 1;
        }
        distribution.push({ floor, damage, allowLit, counts });
      }
    }
  }
  out.window_state_distribution = distribution;
}

// ================================================================ facadeWall ==
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  const openings = [
    { x: -1.0, y: 1.5, w: 0.7, h: 1.0 },
    { x: 1.0, y: 1.5, w: 0.7, h: 1.0, arch: 0.5 },
  ];
  facadeWall(A, pm, { w: 4.0, h: 3.2, t: 0.3, key: 'plaster_cream', openings, rng: new Rng(2), warp: 0.018, bevel: 0.022, top: 'flat' });
  out.facade_wall = finalizeBuckets(A);
}

// ================================================================ windowUnit ==
{
  const states = ['boarded', 'open', 'shuttered', 'ajar', 'curtain', 'lit', 'glazed'];
  const perState = {};
  for (const state of states) {
    const A = newAssembler(1);
    const pm = fixedPanelMatrix();
    const o = { x: 0, y: 1.5, w: 1.0, h: 1.4 };
    windowUnit(A, pm, o, new Rng(3), { state, sill: true, lintel: true, grille: true, shutters: true, curtain: state === 'curtain' });
    perState[state] = finalizeBuckets(A);
  }
  out.window_unit = perState;
}

// =================================================================== doorUnit ==
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  const o = { x: 0, y: 1.05, w: 1.0, h: 2.1 };
  doorUnit(A, pm, o, new Rng(4), { open: 0.4 });
  out.door_unit = finalizeBuckets(A);
}

// =============================================================== shopfront ==
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  const o = { x: 0, y: 1.1, w: 3.0, h: 2.2 };
  shopfront(A, pm, o, new Rng(5), { drop: 0.5 });
  out.shopfront = finalizeBuckets(A);
}

// ================================================================= balcony ==
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  balcony(A, pm, 0.0, 3.0, 1.8, new Rng(6), { railing: 'metal' });
  out.balcony_metal = finalizeBuckets(A);
}
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  balcony(A, pm, 0.0, 3.0, 1.8, new Rng(6), { railing: 'concrete' });
  out.balcony_concrete = finalizeBuckets(A);
}

// ================================================================= parapet ==
{
  const A = newAssembler(1);
  const top = parapet(A, 'roof_screed', 0.0, 0.0, 6.0, 4.0, 8.0, new Rng(7), {});
  out.parapet = { ...finalizeBuckets(A), top };
}

// =================================================================== stairs ==
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  const result = stairRun(A, pm, 0.0, 0.0, 0.0, 1.2, 6, 0.18, 0.28, { railing: true });
  out.stair_run = { ...finalizeBuckets(A), result };
}

// ============================================================ stripedCloth ==
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  stripedCloth(A, ['fabric_red', 'fabric_cream'], pm, 2.0, 1.0, { bands: 4, skipBand: 1, rng: new Rng(8) });
  out.striped_cloth = finalizeBuckets(A);
}

// ================================================================== awning ==
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  const result = awning(A, pm, 0.0, 2.2, 2.0, new Rng(9), { legs: true });
  out.awning = { ...finalizeBuckets(A), result };
}

// =============================================================== drainpipe ==
{
  const A = newAssembler(1);
  const pm = fixedPanelMatrix();
  drainpipe(A, pm, 0.0, 5.0, 4.8, new Rng(10), {});
  out.drainpipe = finalizeBuckets(A);
}

// ============================================================== rubbleMound ==
{
  const A = newAssembler(1);
  const result = rubbleMound(A, new Rng(11), 0.0, 0.0, 0.0, 2.0, 12, {});
  out.rubble_mound = { ...finalizeBuckets(A), result: result ?? null };
}

// ============================================================ pockGeometry ==
out.pock_geometry = dumpGeo(pockGeometry(new Rng(12), 0.05));

// ============================================================== spallPatch ==
out.spall_patch = dumpGeo(spallPatch(new Rng(13), 1.0, 0.8, 0.03));

console.log(JSON.stringify(out));
