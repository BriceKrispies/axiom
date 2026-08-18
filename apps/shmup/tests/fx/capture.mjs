/**
 * Golden capture for the Claude-of-Duty fx port.
 *
 * Runs the ORIGINAL `C:/dev/Claude-of-Duty/src/fx/*.js` under Node (with the
 * source's own `three` dependency resolved from its `node_modules`) and dumps
 * the values `tests/fx_port.rs` pins against:
 *
 *   - particles: raw interleaved records written by the real `ParticleLayer`
 *     for a fixed spawn sequence and a fixed `now` schedule.
 *   - decalsEviction: the real `DecalSystem`'s ring-buffer cursor/wrapped
 *     state and the first written vertex, at and past its budget.
 *   - particleAtlas / decalAtlas: the full byte buffer of a small (32px)
 *     bake from the real `buildParticleAtlas`/`buildDecalAtlas`.
 *   - impacts: for each of the 12 physics/audio surfaces, the sequence of
 *     particle-tile ids `spawnImpact` emits (tagged by additive/lit layer)
 *     and every decal it adds, recorded through a stub `fx` that mirrors
 *     `axiom_shmup::fx::system::FxSystem`'s emit/addDecal contract
 *     instead of a live THREE scene.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 *
 * Reads the source by absolute path and writes nothing but stdout, so it is
 * safe to re-run at any time.
 */

import { Rng } from 'file:///C:/dev/Claude-of-Duty/src/core/rng.js';
import { ParticleLayer, resetSpawn, STRIDE } from 'file:///C:/dev/Claude-of-Duty/src/fx/particles.js';
import { DecalSystem } from 'file:///C:/dev/Claude-of-Duty/src/fx/decals.js';
import { buildParticleAtlas, buildDecalAtlas, P, D } from 'file:///C:/dev/Claude-of-Duty/src/fx/atlas.js';
import { spawnImpact } from 'file:///C:/dev/Claude-of-Duty/src/fx/impacts.js';

const out = {};

/* ------------------------------------------------------------------ */
/* particles: emission + raw record layout                             */
/* ------------------------------------------------------------------ */
{
  const rng = new Rng(12345);
  const layer = new ParticleLayer({ capacity: 8, mode: 'additive', atlas: {}, cols: 4 });
  const nowSchedule = [0.0, 0.016, 0.033, 0.1, 0.5, 1.0];
  const records = [];
  for (let i = 0; i < 6; i++) {
    const s = resetSpawn();
    s.x = rng.range(-2, 2);
    s.y = rng.range(0, 3);
    s.z = rng.range(-2, 2);
    s.vx = rng.range(-5, 5);
    s.vy = rng.range(-5, 5);
    s.vz = rng.range(-5, 5);
    s.size0 = rng.range(0.01, 0.2);
    s.size1 = rng.range(0.01, 0.2);
    s.life = rng.range(0.2, 2.0);
    s.delay = rng.range(0, 0.1);
    s.drag = rng.range(0.5, 5);
    s.gravity = rng.range(-20, 5);
    s.seed = rng.float();
    const slot = layer.emit(s, nowSchedule[i]);
    const b = slot * STRIDE;
    records.push(Array.from(layer.array.slice(b, b + STRIDE)));
  }
  out.particles = { seed: 12345, nowSchedule, records };
}

/* ------------------------------------------------------------------ */
/* decals: ring-buffer eviction at budget                              */
/* ------------------------------------------------------------------ */
{
  const sys = new DecalSystem({ capacity: 8, albedo: {}, normal: {}, orm: {}, cols: 4 });
  const V = new (await import('file:///C:/dev/Claude-of-Duty/node_modules/three/build/three.module.js')).Vector3();
  const N = new (await import('file:///C:/dev/Claude-of-Duty/node_modules/three/build/three.module.js')).Vector3(0, 1, 0);
  const states = [];
  for (let i = 0; i < 9; i++) {
    V.set(i, 0, 0);
    const ok = sys.add({ point: V, normal: N, size: 0.2, tile: 0, now: i, world: null });
    states.push({
      i,
      ok,
      cursor: sys.cursor,
      wrapped: sys._wrapped,
      count: sys.count,
      firstVertexX: sys.pos[0],
    });
  }
  out.decalsEviction = states;
}

/* ------------------------------------------------------------------ */
/* atlases: full byte buffers of a small bake                          */
/* ------------------------------------------------------------------ */
{
  const size = 32;
  const pAtlas = buildParticleAtlas(new Rng(777), size);
  const dAtlas = buildDecalAtlas(new Rng(888), size);
  out.particleAtlas = {
    seed: 777,
    size,
    cols: pAtlas.cols,
    bytes: Array.from(pAtlas.texture.image.data),
  };
  out.decalAtlas = {
    seed: 888,
    size,
    cols: dAtlas.cols,
    albedo: Array.from(dAtlas.albedo.image.data),
    normal: Array.from(dAtlas.normal.image.data),
    orm: Array.from(dAtlas.orm.image.data),
  };
}

/* ------------------------------------------------------------------ */
/* impacts: per-surface selection, recorded through a stub fx          */
/* ------------------------------------------------------------------ */
{
  const SURFACES = [
    'concrete', 'metal', 'wood', 'dirt', 'sand', 'glass',
    'water', 'foliage', 'fabric', 'flesh', 'rubber', 'plaster',
  ];
  const impacts = {};
  for (let idx = 0; idx < SURFACES.length; idx++) {
    const surface = SURFACES[idx];
    const rng = new Rng(1000 + idx);
    const addTiles = [];
    const litTiles = [];
    const decalCalls = [];
    const fxStub = {
      rng,
      pScale: 1.0,
      physics: undefined,
      ctx: {},
      emitAdd(s) {
        addTiles.push(s.tile);
      },
      emitLit(s) {
        litTiles.push(s.tile);
      },
      addDecal(p, n, opts) {
        decalCalls.push({
          tile: opts.tile,
          size: Number(opts.size.toFixed(6)),
          life: opts.life ?? null,
          maxAngle: opts.maxAngle ?? null,
        });
        return true;
      },
      haze() {},
      sunWorld() {
        return { x: 0, y: 1, z: 0 };
      },
      bloodSpatterBehind() {},
    };
    const point = { x: 0, y: 1, z: 0 };
    const normal = { x: 0, y: 1, z: 0 };
    const incident = { x: 0, y: -1, z: 0 };
    spawnImpact(fxStub, point, normal, incident, surface, 1.0);
    impacts[surface] = {
      addCount: addTiles.length,
      litCount: litTiles.length,
      addTiles,
      litTiles,
      decalCalls,
    };
  }
  out.impacts = impacts;
}

process.stdout.write(JSON.stringify(out));
