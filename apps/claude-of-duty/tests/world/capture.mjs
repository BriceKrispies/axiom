/**
 * Golden capture for the world Assembler/ground port.
 *
 * Runs the ORIGINAL `C:/dev/Claude-of-Duty/src/world/{util,builder,ground}.js`
 * under Node and dumps what `tests/world_port.rs` compares against: chamferBox
 * and patchGeometry vertex buffers, a wallPanel triangle-count/position
 * sample across three opening shapes, the road camber/wear/rut profile
 * sampled across the street width, and Assembler.finalize()'s per-key
 * vertex/triangle/instance counts for a small fixed scene.
 *
 * `seam()` is not exported by ground.js (it is a closure private to
 * `buildGround`) so it cannot be imported directly; the function body below
 * is copy-pasted verbatim from `ground.js:158-209` (not retyped) so the
 * capture still runs the source's own algorithm rather than a
 * transcription of it.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

import * as THREE from 'file:///C:/dev/Claude-of-Duty/node_modules/three/build/three.module.js';
import { Rng } from 'file:///C:/dev/Claude-of-Duty/src/core/rng.js';
import { chamferBox, patchGeometry, wallPanel, weatherProp, trs as trsUtil } from 'file:///C:/dev/Claude-of-Duty/src/world/util.js';
import { Assembler } from 'file:///C:/dev/Claude-of-Duty/src/world/builder.js';
import { buildGround } from 'file:///C:/dev/Claude-of-Duty/src/world/ground.js';
import { STREET, ALLEYS } from 'file:///C:/dev/Claude-of-Duty/src/world/layout.js';

const out = {};

// ---------------------------------------------------------------- chamferBox --
function dumpGeo(g) {
  const pa = g.getAttribute('position');
  const na = g.getAttribute('normal');
  const ua = g.getAttribute('uv');
  const ca = g.getAttribute('color');
  return {
    pos: Array.from(pa.array),
    normal: Array.from(na.array),
    uv: ua ? Array.from(ua.array) : null,
    color: ca ? Array.from(ca.array) : null,
    index: g.index ? Array.from(g.index.array) : null,
  };
}

out.chamfer_box_unit = dumpGeo(chamferBox(1, 1, 1, 0.012));
out.chamfer_box_soft = dumpGeo(chamferBox(2, 1.5, 0.8, 0.03));

// -------------------------------------------------------------- patchGeometry --
{
  const rng = new Rng(1);
  out.patch_geometry_default = dumpGeo(patchGeometry(rng, 1.0, { lobes: 9, wobble: 0.45, sag: 0.0 }));
}
{
  const rng = new Rng(7);
  out.patch_geometry_sagged = dumpGeo(patchGeometry(rng, 2.3, { lobes: 12, wobble: 0.3, sag: 0.15 }));
}

// ------------------------------------------------------------------ weatherProp --
{
  const g = chamferBox(1, 1, 1, 0.01);
  weatherProp(g, { base: 0.1, wear: 0.85, grime: 0.5, down: 0.6, height: 1 });
  out.weather_prop_on_chamfer_box = dumpGeo(g);
}

// ---------------------------------------------------------------------- trs --
{
  const m = new THREE.Matrix4();
  trsUtil(m, 1.5, -2.25, 3.0, 0.3, 1.2, 0.9, 1.1, -0.4, 0.7);
  out.trs_sample = Array.from(m.elements);
}

// ----------------------------------------------------------------- wallPanel --
function dumpPanel(w, h, t, holes, opts) {
  const g = wallPanel(w, h, t, holes, opts);
  return dumpGeo(g);
}
out.wall_panel_no_holes = dumpPanel(2.0, 3.0, 0.3, [], { bevel: 0.02, top: 'flat', curveSegments: 6 });
out.wall_panel_rect_hole = dumpPanel(2.0, 3.0, 0.3, [{ x: 0, y: 1.5, w: 0.6, h: 0.8 }], {
  bevel: 0.02,
  top: 'flat',
  curveSegments: 6,
});
out.wall_panel_arch_hole = dumpPanel(2.0, 3.0, 0.3, [{ x: 0, y: 1.0, w: 0.8, h: 1.6, arch: 0.6 }], {
  bevel: 0.02,
  top: 'flat',
  curveSegments: 6,
});

// -------------------------------------------------------------- road profile --
// Only the two purely-x-dependent terms of the road height field
// (`ground.js:50,53`) — `wear` also depends on z (via fbm3) so sampling
// "across the width" alone can't isolate it, and fbm3 itself is already
// golden-pinned independently (`tests/world_port.rs` / `crate::world::noise`).
{
  const HW = STREET.halfWidth;
  const samples = [];
  for (let i = 0; i <= 20; i++) {
    const x = -HW + (i / 20) * (HW * 2);
    const camber = (1 - (x / HW) ** 2) * 0.055;
    const rut = -Math.exp(-((Math.abs(x) - 1.6) ** 2) / 0.5) * 0.022;
    samples.push({ x, camber, rut });
  }
  out.road_camber_rut_profile = samples;
}

// ----------------------------------------------------------- Assembler.finalize --
{
  const materials = { get: () => new THREE.MeshBasicMaterial() };
  const rng = new Rng(11);
  const A = new Assembler({ materials, rng, render: null });
  const root = new THREE.Group();

  A.add('concrete', chamferBox(1, 1, 1, 0.012), null, { masks: [0.2, 0.3, 0.1] });
  A.add('concrete', chamferBox(1, 1, 1, 0.012), trsUtil(new THREE.Matrix4(), 2, 0, 0), null);
  A.add('sand', chamferBox(1, 1, 1, 0.012), null, null);

  const protoGeo = chamferBox(0.5, 0.5, 0.5, 0.01);
  A.proto('barrel', { geo: protoGeo, key: 'metal_rust', tilt: 0, sink: 0, skirt: 0 });
  for (let i = 0; i < 5; i++) {
    A.put('barrel', i * 1.0, 0, 0, 0, 1, null);
  }

  A.box('dirt', 0, 0, 0, 2, 1, 2);
  A.box('concrete', 3, 0, 0, 1, 1, 1);

  A.finalize(root, null);

  out.assembler_finalize_stats = A.stats;
  out.assembler_finalize_mesh_names = root.children.map((c) => c.name).sort();
}

// -------------------------------------------------------------------- seam --
// Copy-pasted verbatim from ground.js:158-209 (a closure, not exported) so
// the capture runs the source's own algorithm.
function seam(A, sr, ax, az, bx, bz, keyA, keyB, y) {
  const len = Math.hypot(bx - ax, bz - az);
  const n = Math.max(6, Math.round(len / 1.15));
  const tx = (bx - ax) / len;
  const tz = (bz - az) / len;
  const nxs = -tz;
  const nzs = tx;
  for (let i = 0; i < n; i++) {
    const t = ((i + sr.range(0.15, 0.85)) / n) * len;
    const px = ax + tx * t;
    const pz = az + tz * t;
    for (const [key, side] of [
      [keyA, -1],
      [keyB, 1],
    ]) {
      if (sr.float() < 0.22) continue;
      const off = side * sr.range(-0.12, 0.62);
      const g = patchGeometry(sr, sr.range(0.3, 0.62), { lobes: 10, wobble: 0.6 });
      const m = new THREE.Matrix4();
      trsUtil(
        m,
        px + nxs * off,
        y + 0.006 + sr.range(0, 0.004),
        pz + nzs * off,
        sr.float() * 6.28,
        1,
        1,
        sr.range(0.55, 1.0)
      );
      A.addOnce(key, g, m, { masks: [0.15, sr.range(0.3, 0.8), sr.range(0.2, 0.5)] });
    }
    if (A.has('rock_b')) {
      for (let k = 0; k < sr.int(1, 3); k++) {
        const off = sr.range(-0.55, 0.55);
        A.put(
          sr.float() < 0.68 ? 'rock_b' : 'rock_a',
          px + nxs * off + sr.range(-0.2, 0.2),
          y + 0.01,
          pz + nzs * off + sr.range(-0.2, 0.2),
          sr.float() * 6.28,
          sr.range(0.45, 1.0),
          [1, sr.range(1.0, 1.5), 1]
        );
      }
    }
  }
}
{
  const materials = { get: () => new THREE.MeshBasicMaterial() };
  const A = new Assembler({ materials, rng: new Rng(1), render: null });
  const sr = new Rng(0x5ea31d);
  seam(A, sr, 0.0, 0.0, 6.0, 0.0, 'sand', 'dirt', 0.0);
  A.finalize(new THREE.Group(), null);
  out.seam_stats = A.stats;
}

// ------------------------------------------------------------ buildGround --
{
  const materials = { get: () => new THREE.MeshBasicMaterial() };
  const A = new Assembler({ materials, rng: new Rng(0), render: null });
  buildGround(A, new Rng(2));
  A.finalize(new THREE.Group(), null);
  out.build_ground_stats = A.stats;
}

console.log(JSON.stringify(out));
