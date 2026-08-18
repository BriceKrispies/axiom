/**
 * Golden capture for the `buildings.js` facade-programme port.
 *
 * Runs the ORIGINAL `C:/dev/Claude-of-Duty/src/world/buildings.js` (plus its
 * `builder.js`/`layout.js` dependencies) under Node and dumps:
 *
 *  - `Assembler.finalize()`'s per-palette-key vertex/triangle counts and
 *    overall stats for one COMPLETE building.
 *  - the full anchor set `buildBuilding` returns (floorY/roofY/top, and
 *    every door/window/balcony/awning — which fully encodes the bay-kind
 *    selection: every non-blank, non-ragged bay shows up here with its
 *    floor/side/position/state).
 *
 * Building choice: **W1**, not W2/E1/E3. W2/E1/E3 are `enterable: true`,
 * which makes the real `buildBuilding` call into `buildInterior` ->
 * `furnishRoom` (`src/world/interiors.js`) — a concurrent, not-yet-ported
 * slice (see `src/world/buildings.rs`'s module doc). Furniture geometry
 * would inflate the JS-side triangle counts in a way the Rust port (which
 * defers furnishing) cannot match, corrupting an otherwise-clean
 * apples-to-apples comparison. W1 is `enterable: false` (the dark-core path)
 * and exercises setback, arches, balconies, doorBays, string
 * course/cornice, damage/weathering and the drainpipe — the large majority
 * of `buildings.js`'s logic — with a completely clean `rng` stream.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

import * as THREE from 'file:///C:/dev/Claude-of-Duty/node_modules/three/build/three.module.js';
import { Rng } from 'file:///C:/dev/Claude-of-Duty/src/core/rng.js';
import { Assembler } from 'file:///C:/dev/Claude-of-Duty/src/world/builder.js';
import { buildBuilding } from 'file:///C:/dev/Claude-of-Duty/src/world/buildings.js';
import { BUILDINGS } from 'file:///C:/dev/Claude-of-Duty/src/world/layout.js';

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
  return { buckets, stats: { ...A.stats } };
}

const w1 = BUILDINGS.find((b) => b.id === 'W1');
if (!w1) throw new Error('W1 not found in layout.js BUILDINGS');
if (w1.enterable) throw new Error('W1 is enterable now — pick a different non-enterable golden target');

const A = newAssembler(1);
const rng = new Rng(0xc0ffee);
const info = buildBuilding(A, rng, w1);

out.w1_building = {
  ...finalizeBuckets(A),
  info: {
    floorY: info.floorY,
    roofY: info.roofY,
    top: info.top,
    doors: info.doors.map((d) => ({ side: d.side, x: d.x, wp: d.wp })),
    windows: info.windows.map((w) => ({ side: w.side, f: w.f, x: w.x, y: w.y, w: w.w, h: w.h, state: w.state })),
    balconies: info.balconies.map((b) => ({ side: b.side, x: b.x, y: b.y, w: b.w, d: b.d })),
    awnings: info.awnings.map((a) => ({ side: a.side, x: a.x, y: a.y, w: a.w })),
  },
};

process.stdout.write(JSON.stringify(out, null, 2));
