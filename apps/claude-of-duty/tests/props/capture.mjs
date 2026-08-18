/**
 * Golden capture for the prop prototype library (`src/world/props.js`).
 *
 * Runs the ORIGINAL `C:/dev/Claude-of-Duty/src/world/props.js` under Node,
 * via a real `Assembler` (materials/render stubbed to `null` — `props.js`
 * never touches either; it only calls `A.proto()`), and dumps what
 * `tests/props_port.rs` compares against:
 *
 *   - every registered prototype's id/key/vertCount/triCount/metadata table
 *     (tilt, sink, skirt, maxDist, chunk, castShadow, receiveShadow)
 *   - full position/normal/uv/color/index buffers for a handful of
 *     prototypes chosen to exercise every geometry primitive props.js uses
 *     (chamfered box + slats, cylinder + rib + warp, icosahedron rock,
 *     Lp-ball sphere sack, lathe tyre, extrude polyPrism, the hand-rolled
 *     dust-skirt cone and pock bowl)
 *
 * `registerProps` shares one Rng stream with the whole level build
 * (`kit.js:979`) but a standalone capture is free to seed however it likes,
 * as long as this script and `tests/props_port.rs` agree — see SEED below.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

import { Rng } from 'file:///C:/dev/Claude-of-Duty/src/core/rng.js';
import { Assembler } from 'file:///C:/dev/Claude-of-Duty/src/world/builder.js';
import { registerProps } from 'file:///C:/dev/Claude-of-Duty/src/world/props.js';

// Arbitrary, fixed — only needs to match `tests/props_port.rs`.
const SEED = 20260818;

function dumpGeo(g) {
  const pa = g.getAttribute('position');
  const na = g.getAttribute('normal');
  const ua = g.getAttribute('uv');
  const ca = g.getAttribute('color');
  return {
    pos: Array.from(pa.array),
    normal: na ? Array.from(na.array) : null,
    uv: ua ? Array.from(ua.array) : null,
    color: ca ? Array.from(ca.array) : null,
    index: g.index ? Array.from(g.index.array) : null,
  };
}

const rng = new Rng(SEED);
const A = new Assembler({ materials: null, rng, render: null });
registerProps(A, rng);

const out = { seed: SEED, prototypes: {} };

// Every registered prototype, structural facts only.
for (const [id, p] of A._protos) {
  const g = p.geo;
  const pa = g.getAttribute('position');
  out.prototypes[id] = {
    key: p.key,
    vertCount: pa.count,
    triCount: g.index ? g.index.count / 3 : pa.count / 3,
    tilt: p.tilt,
    sink: p.sink,
    skirt: p.skirt,
    maxDist: p.maxDist,
    chunk: p.chunk,
    castShadow: p.castShadow,
    receiveShadow: p.receiveShadow,
  };
}

// Full buffers for a handful of prototypes chosen for primitive coverage.
out.geo = {};
for (const id of [
  'crate_a', // chamferBox + slats/posts/lid, a real chamfer + hand-authored wear/grime
  'barrel_rust', // cylinder + ribs + warpGeometry
  'rock_a', // icosahedron + fbm displacement
  'sandbag_a', // Lp-ball sphere sack + analytic mask paint
  'tyre', // lathe profile + tread/groove displacement
  'slab_shard', // polyPrism (extrude) + rebar cylinders
  'dust_skirt', // hand-rolled ragged cone
  'pock', // hand-rolled chipped bowl
]) {
  out.geo[id] = dumpGeo(A._protos.get(id).geo);
}

process.stdout.write(JSON.stringify(out));
