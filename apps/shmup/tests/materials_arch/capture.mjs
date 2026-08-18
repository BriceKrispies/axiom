/**
 * Golden capture for the architectural `owSurface` generators
 * (`src/materials/glsl/surfaces-arch.js`): concrete, brick, plaster, tile.
 *
 * `surfaces-arch.js` embeds its four generator bodies as GLSL inside a JS
 * template literal (like `noise.js`/`generator.js`) — there is no JS function
 * to `import` and call as ground truth. This script is therefore a
 * **from-scratch, line-by-line transcription** of both the noise library
 * (`noise.js`) and the four `owSurface` bodies (`surfaces-arch.js`) into
 * plain JS doubles, written directly against the GLSL source rather than
 * against `apps/shmup/src/materials/surfaces/arch.rs` — so a mistake
 * made once in the Rust port is not simply re-made here and called agreement.
 * It is still a transcription, with a transcription's error rate; see
 * `docs/work-manifests/claude-of-duty-port/notes/materials-surfaces-arch.md`
 * for the caveat this recipe requires stating.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

// ============================================================================
// noise.js, transcribed to plain JS doubles (noise.js:1-218).
// ============================================================================

const fract = (x) => x - Math.floor(x);
const gmod = (x, y) => x - y * Math.floor(x / y);
const mix = (a, b, t) => a + (b - a) * t;
const clamp = (x, a, b) => Math.min(Math.max(x, a), b);
const smoothstep = (e0, e1, x) => {
  const t = clamp((x - e0) / (e1 - e0), 0, 1);
  return t * t * (3 - 2 * t);
};
const step = (edge, x) => (x < edge ? 0 : 1);
const dot2 = (a, b) => a.x * b.x + a.y * b.y;
const len2 = (v) => Math.sqrt(dot2(v, v));

function owHash11(p) {
  p = fract(p * 0.1031);
  p *= p + 33.33;
  p *= p + p;
  return fract(p);
}
function owHash12(p) {
  let p3x = fract(p.x * 0.1031), p3y = fract(p.y * 0.1031), p3z = fract(p.x * 0.1031);
  const d = p3x * (p3y + 33.33) + p3y * (p3z + 33.33) + p3z * (p3x + 33.33);
  p3x += d; p3y += d; p3z += d;
  return fract((p3x + p3y) * p3z);
}
function owHash22(p) {
  let p3x = fract(p.x * 0.1031), p3y = fract(p.y * 0.1030), p3z = fract(p.x * 0.0973);
  const d = p3x * (p3y + 33.33) + p3y * (p3z + 33.33) + p3z * (p3x + 33.33);
  p3x += d; p3y += d; p3z += d;
  return { x: fract((p3x + p3y) * p3z), y: fract((p3x + p3z) * p3y) };
}
function owHash42(p) {
  let p4x = fract(p.x * 0.1031), p4y = fract(p.y * 0.1030), p4z = fract(p.x * 0.0973), p4w = fract(p.y * 0.1099);
  // p4 += dot(p4, p4.wzxy + 33.33)
  const d = p4x * (p4w + 33.33) + p4y * (p4z + 33.33) + p4z * (p4x + 33.33) + p4w * (p4y + 33.33);
  p4x += d; p4y += d; p4z += d; p4w += d;
  // (p4.xxyz + p4.yzzw) * p4.zywx
  return {
    x: fract((p4x + p4y) * p4z),
    y: fract((p4x + p4z) * p4y),
    z: fract((p4y + p4z) * p4w),
    w: fract((p4z + p4w) * p4x),
  };
}

function owGrad2(i, per) {
  const a = owHash12({ x: gmod(i.x, per.x) + 0.317, y: gmod(i.y, per.y) + 0.317 }) * 6.28318530718;
  return { x: Math.cos(a), y: Math.sin(a) };
}
function owNoise(p, per) {
  const i = { x: Math.floor(p.x), y: Math.floor(p.y) };
  const f = { x: fract(p.x), y: fract(p.y) };
  const fade = (v) => v * v * v * (v * (v * 6 - 15) + 10);
  const ux = fade(f.x), uy = fade(f.y);
  const a = dot2(owGrad2({ x: i.x, y: i.y }, per), { x: f.x, y: f.y });
  const b = dot2(owGrad2({ x: i.x + 1, y: i.y }, per), { x: f.x - 1, y: f.y });
  const c = dot2(owGrad2({ x: i.x, y: i.y + 1 }, per), { x: f.x, y: f.y - 1 });
  const d = dot2(owGrad2({ x: i.x + 1, y: i.y + 1 }, per), { x: f.x - 1, y: f.y - 1 });
  return mix(mix(a, b, ux), mix(c, d, ux), uy) * 1.4142;
}

function owFbm(p, per, oct, gain) {
  let s = 0, a = 0.5, n = 0;
  let pp = { x: p.x, y: p.y }, pper = { x: per.x, y: per.y };
  for (let i = 0; i < 10; i++) {
    if (i >= oct) break;
    s += a * owNoise(pp, pper);
    n += a;
    pp = { x: pp.x * 2, y: pp.y * 2 };
    pper = { x: pper.x * 2, y: pper.y * 2 };
    a *= gain;
  }
  return s / Math.max(n, 1e-4);
}
const owFbm01 = (p, per, oct, gain) => owFbm(p, per, oct, gain) * 0.5 + 0.5;

function owWarp(p, per, amp, oct) {
  const qx = owFbm({ x: p.x + 1.7, y: p.y + 9.2 }, per, oct, 0.5);
  const qy = owFbm({ x: p.x + 8.3, y: p.y + 2.8 }, per, oct, 0.5);
  return { x: p.x + qx * amp, y: p.y + qy * amp };
}

function owWorley(p, per, jitter) {
  const ip = { x: Math.floor(p.x), y: Math.floor(p.y) };
  const fp = { x: fract(p.x), y: fract(p.y) };
  let f1 = 8, f2 = 8, idx = 0, idy = 0;
  for (let y = -1; y <= 1; y++) {
    for (let x = -1; x <= 1; x++) {
      const cell = { x: gmod(ip.x + x, per.x), y: gmod(ip.y + y, per.y) };
      const h = owHash22({ x: cell.x + 0.771, y: cell.y + 0.771 });
      const ox = h.x * jitter + (1 - jitter) * 0.5;
      const oy = h.y * jitter + (1 - jitter) * 0.5;
      const rx = x + ox - fp.x, ry = y + oy - fp.y;
      const d = rx * rx + ry * ry;
      if (d < f1) {
        f2 = f1;
        f1 = d;
        const id = owHash22({ x: cell.x + 3.117, y: cell.y + 3.117 });
        idx = id.x; idy = id.y;
      } else if (d < f2) {
        f2 = d;
      }
    }
  }
  return { f1: Math.sqrt(f1), f2: Math.sqrt(f2), idx, idy };
}

function owVoronoiEdge(p, per, jitter) {
  const ip = { x: Math.floor(p.x), y: Math.floor(p.y) };
  const fp = { x: fract(p.x), y: fract(p.y) };
  const featurePoint = (g) => {
    const cell = { x: gmod(ip.x + g.x, per.x) + 0.771, y: gmod(ip.y + g.y, per.y) + 0.771 };
    const h = owHash22(cell);
    return { x: h.x * jitter + (1 - jitter) * 0.5, y: h.y * jitter + (1 - jitter) * 0.5 };
  };
  let mr = { x: 0, y: 0 }, mg = { x: 0, y: 0 }, md = 8;
  for (let y = -1; y <= 1; y++) {
    for (let x = -1; x <= 1; x++) {
      const g = { x, y };
      const o = featurePoint(g);
      const r = { x: g.x + o.x - fp.x, y: g.y + o.y - fp.y };
      const d = dot2(r, r);
      if (d < md) { md = d; mr = r; mg = g; }
    }
  }
  md = 8;
  for (let y = -2; y <= 2; y++) {
    for (let x = -2; x <= 2; x++) {
      const g = { x: mg.x + x, y: mg.y + y };
      const o = featurePoint(g);
      const r = { x: g.x + o.x - fp.x, y: g.y + o.y - fp.y };
      const diff = { x: r.x - mr.x, y: r.y - mr.y };
      if (dot2(diff, diff) > 1e-5) {
        const avg = { x: (mr.x + r.x) * 0.5, y: (mr.y + r.y) * 0.5 };
        const nlen = len2(diff);
        const ndiff = { x: diff.x / nlen, y: diff.y / nlen };
        md = Math.min(md, dot2(avg, ndiff));
      }
    }
  }
  return md;
}

function owCracks(p, per, jitter, width, breakUp) {
  const wp = owWarp(p, per, 0.20, 3);
  const e = owVoronoiEdge(wp, per, jitter);
  let c = 1 - smoothstep(0, width, e);
  const mask = owFbm01({ x: p.x * 1.7 + 11.3, y: p.y * 1.7 + 11.3 }, { x: per.x * 1.7, y: per.y * 1.7 }, 4, 0.55);
  c *= smoothstep(breakUp, breakUp + 0.28, mask);
  return clamp(c, 0, 1);
}

function owSRGB(c) {
  const decode = (ci) => (ci > 0.04045 ? Math.pow((ci + 0.055) / 1.055, 2.4) : ci / 12.92);
  return { x: decode(c.x), y: decode(c.y), z: decode(c.z) };
}
const owShear = (p, k, stretch) => ({ x: p.x + p.y * k, y: p.y * stretch });
const owShearPer = (per, stretch) => ({ x: per.x, y: per.y * stretch });

// vec3 helpers used only by the surface bodies below (not part of noise.js).
const v3 = (x, y, z) => ({ x, y, z });
const v3mix = (a, b, t) => v3(mix(a.x, b.x, t), mix(a.y, b.y, t), mix(a.z, b.z, t));
const v3scale = (a, s) => v3(a.x * s, a.y * s, a.z * s);
const v3add = (a, b) => v3(a.x + b.x, a.y + b.y, a.z + b.z);
const v3addScalar = (a, s) => v3(a.x + s, a.y + s, a.z + s);
const v3clamp = (a, lo, hi) => v3(clamp(a.x, lo, hi), clamp(a.y, lo, hi), clamp(a.z, lo, hi));

// ============================================================================
// surfaces-arch.js, transcribed line-by-line.
// ============================================================================

function concreteSurface(uv, uSeed, uParam) {
  const P = { x: 8.0, y: 8.0 };
  const p = { x: uv.x * P.x + uSeed * 13.7, y: uv.y * P.y + uSeed * 13.7 };

  const macro = owFbm01({ x: p.x * 0.5, y: p.y * 0.5 }, { x: P.x * 0.5, y: P.y * 0.5 }, 4, 0.58);
  const mid = owFbm01(owWarp({ x: p.x * 2.0, y: p.y * 2.0 }, { x: P.x * 2.0, y: P.y * 2.0 }, 0.7, 3), { x: P.x * 2.0, y: P.y * 2.0 }, 5, 0.5);
  const fine = owFbm01({ x: p.x * 18.0, y: p.y * 18.0 }, { x: P.x * 18.0, y: P.y * 18.0 }, 4, 0.5);
  const micro = owFbm01({ x: p.x * 26.0, y: p.y * 26.0 }, { x: P.x * 26.0, y: P.y * 26.0 }, 3, 0.5);

  const cLight = owSRGB(v3(0.520, 0.512, 0.492));
  const cMid = owSRGB(v3(0.395, 0.392, 0.385));
  const cDark = owSRGB(v3(0.255, 0.253, 0.258));
  let c = v3mix(cMid, cLight, smoothstep(0.35, 0.85, macro));
  c = v3mix(c, cDark, smoothstep(0.55, 0.95, mid) * 0.55);
  c = v3scale(c, 0.93 + 0.14 * fine);
  let pourB = owFbm01(owWarp({ x: p.x * 1.5 + 8.3, y: p.y * 1.5 + 8.3 }, { x: P.x * 1.5, y: P.y * 1.5 }, 0.6, 3), { x: P.x * 1.5, y: P.y * 1.5 }, 4, 0.58);
  pourB = clamp((pourB - 0.5) * 2.5 + 0.5, 0, 1);
  c = v3scale(c, 0.82 + 0.38 * pourB);
  let wash = owFbm01({ x: p.x * 7.0 + 2.0, y: p.y * 7.0 + 2.0 }, { x: P.x * 7.0, y: P.y * 7.0 }, 4, 0.5);
  wash = clamp((wash - 0.5) * 2.2 + 0.5, 0, 1);
  c = v3scale(c, 0.925 + 0.155 * wash);

  let h = 0.62 + (fine - 0.5) * 0.035 + (mid - 0.5) * 0.05;
  let rough = 0.70 + (mid - 0.5) * 0.16 + (micro - 0.5) * 0.07;
  let ao = 1.0;
  let metal = 0.0;

  const agg = owWorley({ x: p.x * 13.0, y: p.y * 13.0 }, { x: P.x * 13.0, y: P.y * 13.0 }, 0.95);
  const aggShape = smoothstep(0.46, 0.10, agg.f1);
  const aggRnd = agg.idx;
  const aggExposed = aggShape * step(0.74, owFbm01({ x: p.x * 3.0 + 5.0, y: p.y * 3.0 + 5.0 }, { x: P.x * 3.0, y: P.y * 3.0 }, 3, 0.5) + aggRnd * 0.35);
  h += aggExposed * 0.022 * (0.5 + aggRnd);
  c = v3mix(c, v3mix(owSRGB(v3(0.335, 0.320, 0.300)), owSRGB(v3(0.560, 0.545, 0.505)), aggRnd), aggExposed * 0.7);
  rough += aggExposed * 0.07 * (aggRnd - 0.5);

  const sand = owWorley({ x: p.x * 20.0, y: p.y * 20.0 }, { x: P.x * 20.0, y: P.y * 20.0 }, 1.0);
  const sandM = smoothstep(0.44, 0.05, sand.f1);
  const sandSel = 0.40 + 0.60 * step(0.30, sand.idx);
  h += sandM * sandSel * 0.028;
  c = v3scale(c, 1.0 + (sandM * sandSel - 0.20) * 0.15);
  rough += (sand.idx - 0.5) * 0.11 + sandM * 0.04;
  ao -= sandM * 0.06;
  const sandTrough = smoothstep(0.52, 0.88, sand.f1);
  c = v3mix(c, v3scale(c, 0.86), sandTrough * 0.34);

  const pores = owWorley({ x: p.x * 22.0, y: p.y * 22.0 }, { x: P.x * 22.0, y: P.y * 22.0 }, 1.0);
  const pore = smoothstep(0.26, 0.0, pores.f1) * step(0.84, pores.idy);
  h -= pore * 0.055;
  ao -= pore * 0.55;
  rough += pore * 0.10;

  const formAmt = uParam.x;
  const jointAmt = uParam.y;

  const boards = uv.y * 4.0;
  const bi = Math.floor(boards);
  const bf = fract(boards);
  let seam = (1 - smoothstep(0, 0.030, bf)) + (1 - smoothstep(0, 0.030, 1 - bf));
  seam = clamp(seam, 0, 1);
  const boardStep = (owHash11(bi + uSeed) - 0.5) * 0.028 * formAmt;
  h += boardStep;
  h -= seam * 0.055 * formAmt;
  ao -= seam * 0.40 * formAmt;
  c = v3scale(c, 1.0 - seam * 0.16 * formAmt);
  const bleed = (1 - smoothstep(0, 0.10, Math.abs(bf - 0.02))) * 0.5 * formAmt;
  c = v3mix(c, v3scale(cLight, 1.05), bleed * 0.35 * owFbm01({ x: p.x * 8.0, y: p.y * 8.0 }, { x: P.x * 8.0, y: P.y * 8.0 }, 3, 0.5));

  const tfx = fract(uv.x * 3.0) - 0.5, tfy = fract(boards * 0.5) - 0.5;
  const tieRnd = owHash12({ x: Math.floor(uv.x * 3.0) + uSeed, y: Math.floor(boards * 0.5) + uSeed });
  const tieLen = Math.sqrt((tfx * 1.0) * (tfx * 1.0) + (tfy * 2.0) * (tfy * 2.0));
  const tie = smoothstep(0.085, 0.05, tieLen) * step(0.45, tieRnd) * formAmt;
  h -= tie * 0.10;
  ao -= tie * 0.5;
  c = v3mix(c, v3scale(cDark, 0.85), tie * 0.6);

  const jdx = Math.abs(fract(uv.x + 0.5) - 0.5);
  const jdy = Math.abs(fract(uv.y + 0.5) - 0.5);
  let joint = Math.max(1 - smoothstep(0.0035, 0.010, jdx), 1 - smoothstep(0.0035, 0.010, jdy));
  joint *= jointAmt;
  h -= joint * 0.10;
  ao -= joint * 0.55;
  c = v3mix(c, v3scale(cDark, 0.62), joint * 0.65);
  const swirl = owFbm01(owWarp({ x: p.x * 1.1 + 3.0, y: p.y * 1.1 + 3.0 }, { x: P.x * 1.1, y: P.y * 1.1 }, 1.4, 3), { x: P.x * 1.1, y: P.y * 1.1 }, 3, 0.6);
  rough -= jointAmt * smoothstep(0.35, 0.85, swirl) * 0.10;
  c = v3scale(c, 1.0 - jointAmt * smoothstep(0.4, 0.9, swirl) * 0.07);

  const crk = owCracks({ x: p.x * 2.6, y: p.y * 2.6 }, { x: P.x * 2.6, y: P.y * 2.6 }, 0.85, 0.028, 0.50);
  const crkFine = owCracks({ x: p.x * 7.0 + 31.0, y: p.y * 7.0 + 31.0 }, { x: P.x * 7.0, y: P.y * 7.0 }, 0.9, 0.020, 0.60) * 0.55;
  const crack = clamp(crk + crkFine, 0, 1);
  h -= crack * 0.12;
  ao -= crack * 0.45;
  c = v3mix(c, v3scale(cDark, 0.80), crack * 0.42);
  rough += crack * 0.12;

  const sp = owWorley({ x: p.x * 1.1 + 7.3, y: p.y * 1.1 + 7.3 }, { x: P.x * 1.1, y: P.y * 1.1 }, 0.9);
  const spallCell = step(0.90, sp.idy);
  const spall = spallCell * smoothstep(0.44, 0.16, sp.f1) * smoothstep(0.42, 0.62, owFbm01({ x: p.x * 4.0 + 2.0, y: p.y * 4.0 + 2.0 }, { x: P.x * 4.0, y: P.y * 4.0 }, 4, 0.5));
  h -= spall * 0.13;
  ao -= spall * 0.35;
  c = v3mix(c, v3scale(v3mix(cDark, cMid, aggRnd), 0.88), spall * 0.8);
  rough += spall * 0.10;
  const spallRim = spall * (1 - spall) * 4.0;
  c = v3scale(c, 1.0 + spallRim * 0.10);

  const ck = owWorley(owWarp({ x: p.x * 5.6 + 19.0, y: p.y * 5.6 + 19.0 }, { x: P.x * 5.6, y: P.y * 5.6 }, 0.6, 3), { x: P.x * 5.6, y: P.y * 5.6 }, 0.95);
  const ckSel = step(0.90, ck.idy);
  const ckSize = 0.20 + 0.16 * ck.idx;
  const ckShape = smoothstep(ckSize, ckSize * 0.3, ck.f1 * (0.72 + 0.56 * owFbm01({ x: p.x * 16.0, y: p.y * 16.0 }, { x: P.x * 16.0, y: P.y * 16.0 }, 3, 0.5)));
  const chip = ckSel * ckShape;
  c = v3mix(c, v3mix(v3scale(c, 0.74), v3mix(cDark, cMid, sand.idx), 0.5), chip * 0.85);
  h -= chip * 0.045;
  ao -= chip * 0.24;
  rough += chip * 0.08;
  const ckLip = Math.max(ckSel * (smoothstep(ckSize * 1.25, ckSize, ck.f1) - ckShape), 0.0);
  c = v3scale(c, 1.0 + ckLip * 0.10);

  const streak = owFbm01({ x: p.x * 6.0, y: p.y * 2.0 }, { x: P.x * 6.0, y: P.y * 2.0 }, 5, 0.55);
  const runoff = smoothstep(0.58, 0.95, streak) * (0.35 + 0.65 * smoothstep(0.2, 0.8, macro));
  c = v3scale(c, 1.0 - runoff * 0.14);
  rough += runoff * 0.05;

  const rustBleed = smoothstep(0.72, 0.98, streak * (0.6 + 0.5 * tieRnd)) * step(0.80, tieRnd);
  c = v3mix(c, owSRGB(v3(0.42, 0.24, 0.12)), rustBleed * 0.45);

  const cavity = 1 - smoothstep(0.42, 0.66, h);
  c = v3mix(c, owSRGB(v3(0.20, 0.19, 0.17)), cavity * 0.35);

  return {
    alb: v3clamp(c, 0.02, 0.85),
    h: clamp(h, 0.0, 1.0),
    rough: clamp(rough, 0.48, 0.98),
    metal,
    ao: clamp(ao, 0.15, 1.0),
  };
}

function brickSurface(uv, uSeed) {
  const P = { x: 8.0, y: 8.0 };
  const COLS = 6.0, ROWS = 18.0;
  const p = { x: uv.x * P.x + uSeed * 9.1, y: uv.y * P.y + uSeed * 9.1 };

  const rowF = uv.y * ROWS;
  const row = Math.floor(rowF);
  const colF = uv.x * COLS + gmod(row, 2.0) * 0.5;
  const col = Math.floor(colF);
  const id = { x: gmod(col, COLS), y: row };
  const f = { x: fract(colF), y: fract(rowF) };

  const rnd = owHash42({ x: id.x + uSeed * 3.0, y: id.y + uSeed * 3.0 });
  const rnd2 = owHash42({ x: id.x * 1.37 + 21.0 + uSeed, y: id.y * 1.37 + 21.0 + uSeed });
  const rnd3 = owHash42({ x: id.x * 0.73 + 7.7 + uSeed * 1.9, y: id.y * 0.73 + 7.7 + uSeed * 1.9 });

  const jitter = { x: (rnd.x - 0.5) * 0.012, y: (rnd.y - 0.5) * 0.030 };
  const fj = { x: f.x + jitter.x, y: f.y + jitter.y };

  const JX = 0.048, JY = 0.135;
  const dxj = Math.min(fj.x, 1.0 - fj.x);
  const dyj = Math.min(fj.y, 1.0 - fj.y);
  const shoulder = 0.74 + 0.16 * rnd3.w;
  const ex = smoothstep(JX * shoulder, JX * 1.02, dxj);
  const ey = smoothstep(JY * shoulder, JY * 1.02, dyj);
  const face = Math.min(ex, ey);

  const bp = { x: fj.x * 3.0 + rnd.z * 17.0, y: fj.y * 1.0 + rnd.w * 17.0 };
  const BP = { x: 24.0, y: 24.0 };

  const mSand = owFbm01({ x: p.x * 20.0, y: p.y * 20.0 }, { x: P.x * 20.0, y: P.y * 20.0 }, 4, 0.5);
  const mGrain = owWorley({ x: p.x * 24.0, y: p.y * 24.0 }, { x: P.x * 24.0, y: P.y * 24.0 }, 1.0);
  const mortarRough = owFbm01({ x: p.x * 20.0, y: p.y * 20.0 }, { x: P.x * 20.0, y: P.y * 20.0 }, 4, 0.55);
  let mortarCol = v3mix(owSRGB(v3(0.400, 0.388, 0.362)), owSRGB(v3(0.278, 0.272, 0.260)), smoothstep(0.3, 0.8, mortarRough));
  mortarCol = v3scale(mortarCol, 0.84 + 0.32 * mSand);
  mortarCol = v3scale(mortarCol, 0.88 + 0.24 * owFbm01({ x: p.x * 6.0, y: p.y * 6.0 }, { x: P.x * 6.0, y: P.y * 6.0 }, 4, 0.6));
  mortarCol = v3mix(mortarCol, owSRGB(v3(0.235, 0.228, 0.215)), smoothstep(0.5, 0.06, mGrain.f1) * 0.40);
  mortarCol = v3mix(mortarCol, owSRGB(v3(0.520, 0.505, 0.470)), smoothstep(0.30, 0.02, owWorley({ x: p.x * 25.0 + 4.0, y: p.y * 25.0 + 4.0 }, { x: P.x * 25.0, y: P.y * 25.0 }, 1.0).f1) * 0.35);

  let jointDepth = 0.10 + 0.05 * owFbm01({ x: p.x * 1.2, y: p.y * 1.2 }, { x: P.x * 1.2, y: P.y * 1.2 }, 3, 0.5);
  const crumble = smoothstep(0.62, 0.86, owFbm01({ x: p.x * 9.0 + 4.0, y: p.y * 9.0 + 4.0 }, { x: P.x * 9.0, y: P.y * 9.0 }, 4, 0.5));
  jointDepth += crumble * 0.09;
  const mortarH = -(mSand - 0.5) * 0.018 - smoothstep(0.5, 0.0, mGrain.f1) * 0.012;

  const faceN = owFbm01({ x: bp.x * 2.2, y: bp.y * 2.2 }, BP, 5, 0.5);
  const faceFine = owFbm01({ x: bp.x * 5.0, y: bp.y * 5.0 }, { x: BP.x * 2.0, y: BP.y * 2.0 }, 4, 0.5);
  const facePore = owWorley({ x: bp.x * 7.0, y: bp.y * 7.0 }, { x: BP.x * 3.5, y: BP.y * 3.5 }, 1.0);
  const poreCluster = smoothstep(0.42, 0.78, owFbm01({ x: bp.x * 3.0 + 8.0, y: bp.y * 3.0 + 8.0 }, { x: BP.x * 1.5, y: BP.y * 1.5 }, 4, 0.55));
  const pore = smoothstep(0.26 + 0.16 * facePore.idx, 0.0, facePore.f1) * step(0.55, facePore.idy) * poreCluster;

  const cA = owSRGB(v3(0.430, 0.238, 0.183));
  const cB = owSRGB(v3(0.318, 0.183, 0.150));
  const cC = owSRGB(v3(0.196, 0.132, 0.120));
  const cD = owSRGB(v3(0.492, 0.392, 0.300));
  const cE = owSRGB(v3(0.372, 0.288, 0.218));

  let brick = v3mix(cA, cB, rnd.z);
  brick = v3mix(brick, cC, step(0.90, rnd.w) * 0.70);
  brick = v3mix(brick, cD, step(0.94, rnd2.x) * 0.62);
  brick = v3mix(brick, cE, step(0.55, rnd2.y) * 0.50);
  brick = v3scale(brick, 0.88 + 0.24 * rnd3.x);
  brick = v3scale(brick, 0.86 + 0.28 * faceN);
  const faceGrain = owFbm01({ x: bp.x * 8.0, y: bp.y * 8.0 }, { x: BP.x * 4.0, y: BP.y * 4.0 }, 4, 0.55);
  brick = v3scale(brick, 0.87 + 0.26 * faceGrain);
  brick = v3mix(brick, v3scale(brick, 1.22), smoothstep(0.55, 0.9, faceFine) * 0.5);
  brick = v3mix(brick, v3scale(brick, 0.62), pore * 0.85);
  brick = v3mix(brick, v3scale(brick, 0.72), smoothstep(0.34, 0.0, facePore.f1) * step(0.86, facePore.idx));
  brick = v3mix(brick, owSRGB(v3(0.62, 0.58, 0.50)), smoothstep(0.86, 0.98, faceFine) * 0.35);

  let faceH = 0.72 + (faceN - 0.5) * 0.05 + (faceFine - 0.5) * 0.025 + (rnd2.z - 0.5) * 0.05;
  faceH -= pore * 0.075;

  const edgeD = Math.min(dxj / JX, dyj / JY);
  const chipNoise = owFbm01({ x: bp.x * 6.0 + 3.0, y: bp.y * 6.0 + 3.0 }, { x: BP.x * 3.0, y: BP.y * 3.0 }, 4, 0.5);
  const chip = smoothstep(1.7, 0.30, edgeD) * smoothstep(0.60, 0.80, chipNoise) * step(0.66, rnd3.z);
  faceH -= chip * 0.17;
  brick = v3mix(brick, v3add(v3scale(brick, 0.72), owSRGB(v3(0.20, 0.13, 0.09))), chip * 0.65);

  const m = face;
  let h = mix(0.72 - jointDepth + mortarH, faceH, m);
  let c = v3mix(mortarCol, brick, m);
  const brickRough = 0.58 + 0.32 * rnd2.z + (rnd3.y - 0.5) * 0.20;
  let rough = mix(0.88 + 0.10 * mSand + 0.06 * (mortarRough - 0.5), brickRough + 0.14 * faceN + 0.10 * (faceGrain - 0.5) + chip * 0.14, m);
  let ao = mix(0.34, 1.0, smoothstep(0.0, 0.75, face));
  ao -= chip * 0.30;
  const metal = 0.0;

  const smear = smoothstep(0.5, 1.0, 1.0 - face) * smoothstep(0.55, 0.9, owFbm01({ x: p.x * 14.0, y: p.y * 14.0 }, { x: P.x * 14.0, y: P.y * 14.0 }, 4, 0.5));
  c = v3mix(c, v3scale(mortarCol, 1.05), smear * 0.5);

  let soilB = owFbm01(owWarp({ x: p.x * 1.8 + 27.0, y: p.y * 1.8 + 27.0 }, { x: P.x * 1.8, y: P.y * 1.8 }, 0.6, 3), { x: P.x * 1.8, y: P.y * 1.8 }, 4, 0.58);
  soilB = clamp((soilB - 0.5) * 2.5 + 0.5, 0, 1);
  c = v3scale(c, 0.845 + 0.33 * soilB);

  let efflo = smoothstep(0.62, 0.96, owFbm01(owWarp({ x: p.x * 2.6, y: p.y * 2.6 }, { x: P.x * 2.6, y: P.y * 2.6 }, 0.8, 3), { x: P.x * 2.6, y: P.y * 2.6 }, 4, 0.5));
  efflo *= mix(1.0, 0.35, m);
  c = v3mix(c, owSRGB(v3(0.66, 0.652, 0.632)), efflo * 0.5);
  rough += efflo * 0.10;

  const streak = owFbm01({ x: p.x * 7.0, y: p.y * 2.3 }, { x: P.x * 7.0, y: P.y * 2.0 }, 5, 0.55);
  const runoff = smoothstep(0.50, 0.92, streak);
  c = v3scale(c, 1.0 - runoff * 0.16);

  const crack = owCracks({ x: p.x * 2.2, y: p.y * 2.2 }, { x: P.x * 2.2, y: P.y * 2.2 }, 0.85, 0.038, 0.58);
  h -= crack * 0.10;
  ao -= crack * 0.45;
  c = v3mix(c, v3scale(c, 0.35), crack * 0.7);

  const cavity = 1 - smoothstep(0.50, 0.74, h);
  c = v3mix(c, owSRGB(v3(0.16, 0.15, 0.14)), cavity * 0.32);

  return {
    alb: v3clamp(c, 0.02, 0.85),
    h: clamp(h, 0.0, 1.0),
    rough: clamp(rough, 0.35, 0.99),
    metal,
    ao: clamp(ao, 0.12, 1.0),
  };
}

function plasterSurface(uv, uSeed) {
  const P = { x: 8.0, y: 8.0 };
  const p = { x: uv.x * P.x + uSeed * 5.3, y: uv.y * P.y + uSeed * 5.3 };

  const sw = owShear({ x: p.x * 1.5, y: p.y * 1.5 }, 1.0, 3.0);
  const trowel = owFbm01(sw, owShearPer({ x: P.x * 1.5, y: P.y * 1.5 }, 3.0), 5, 0.55);
  const skim = owFbm01({ x: p.x * 12.0, y: p.y * 12.0 }, { x: P.x * 12.0, y: P.y * 12.0 }, 5, 0.5);
  const micro = owFbm01({ x: p.x * 24.0, y: p.y * 24.0 }, { x: P.x * 24.0, y: P.y * 24.0 }, 3, 0.5);
  const macro = owFbm01({ x: p.x * 0.6, y: p.y * 0.6 }, { x: P.x * 0.6, y: P.y * 0.6 }, 3, 0.6);

  const cBase = owSRGB(v3(0.598, 0.578, 0.538));
  const cWarm = owSRGB(v3(0.512, 0.462, 0.395));
  const cGrey = owSRGB(v3(0.382, 0.378, 0.372));
  let c = v3mix(cBase, cWarm, smoothstep(0.3, 0.8, macro));
  c = v3scale(c, 0.94 + 0.12 * skim);
  c = v3mix(c, cGrey, smoothstep(0.45, 0.95, trowel) * 0.42);
  c = v3mix(c, v3scale(cBase, 1.10), smoothstep(0.55, 0.15, trowel) * 0.30);

  let h = 0.70 + (trowel - 0.5) * 0.10 + (skim - 0.5) * 0.030 + (micro - 0.5) * 0.012;
  let rough = 0.80 + (skim - 0.5) * 0.12 - smoothstep(0.5, 0.9, trowel) * 0.10;
  let ao = 1.0;
  const metal = 0.0;

  const lapUv = owShear({ x: p.x * 0.7, y: p.y * 0.7 }, 1.0, 1.0);
  const lapF = lapUv.y + owFbm01({ x: p.x * 1.1, y: p.y * 1.1 }, { x: P.x * 1.1, y: P.y * 1.1 }, 3, 0.6) * 1.4;
  const lapI = Math.floor(lapF);
  const lapT = fract(lapF);
  const lapR = owHash11(lapI * 1.71 + uSeed * 2.3);
  c = v3scale(c, 0.885 + 0.240 * lapR);
  rough += (lapR - 0.5) * 0.10;

  let dampB = owFbm01(owWarp({ x: p.x * 1.6 + 3.7, y: p.y * 1.6 + 3.7 }, { x: P.x * 1.6, y: P.y * 1.6 }, 0.7, 3), { x: P.x * 1.6, y: P.y * 1.6 }, 4, 0.58);
  dampB = clamp((dampB - 0.5) * 2.6 + 0.5, 0, 1);
  c = v3scale(c, 0.80 + 0.42 * dampB);
  rough += (dampB - 0.5) * 0.12;
  let soil2 = owFbm01(owWarp({ x: p.x * 3.4 + 21.0, y: p.y * 3.4 + 21.0 }, { x: P.x * 3.4, y: P.y * 3.4 }, 0.55, 3), { x: P.x * 3.4, y: P.y * 3.4 }, 4, 0.55);
  soil2 = clamp((soil2 - 0.5) * 2.4 + 0.5, 0, 1);
  c = v3scale(c, 0.875 + 0.26 * soil2);
  let wash = owFbm01({ x: p.x * 8.0 + 6.0, y: p.y * 8.0 + 6.0 }, { x: P.x * 8.0, y: P.y * 8.0 }, 4, 0.5);
  wash = clamp((wash - 0.5) * 2.2 + 0.5, 0, 1);
  c = v3scale(c, 0.925 + 0.155 * wash);
  const lapEdge = (1 - smoothstep(0.0, 0.05, lapT)) * (0.35 + 0.65 * lapR);
  h += lapEdge * 0.022 - (lapR - 0.5) * 0.014;
  c = v3scale(c, 1.0 + lapEdge * 0.07);

  const tooth = owWorley({ x: p.x * 20.0, y: p.y * 20.0 }, { x: P.x * 20.0, y: P.y * 20.0 }, 1.0);
  const grain = smoothstep(0.46, 0.06, tooth.f1);
  const grainSel = 0.40 + 0.60 * step(0.32, tooth.idx);
  h += grain * grainSel * 0.030;
  ao -= grain * 0.07;
  c = v3scale(c, 1.0 + (grain * grainSel - 0.20) * 0.16);
  rough += (tooth.idx - 0.5) * 0.11 + grain * 0.05;
  const trough = smoothstep(0.52, 0.86, tooth.f1);
  c = v3mix(c, v3scale(c, 0.84), trough * 0.40);

  const ph = owWorley({ x: p.x * 22.0, y: p.y * 22.0 }, { x: P.x * 22.0, y: P.y * 22.0 }, 1.0);
  const hole = smoothstep(0.24, 0.0, ph.f1) * step(0.80, ph.idy);
  h -= hole * 0.06;
  ao -= hole * 0.4;

  let hair = owCracks({ x: p.x * 9.0, y: p.y * 9.0 }, { x: P.x * 9.0, y: P.y * 9.0 }, 0.9, 0.016, 0.52);
  hair += owCracks({ x: p.x * 16.0 + 6.0, y: p.y * 16.0 + 6.0 }, { x: P.x * 16.0, y: P.y * 16.0 }, 0.95, 0.015, 0.62) * 0.5;
  hair = clamp(hair, 0, 1);
  h -= hair * 0.030;
  ao -= hair * 0.18;
  c = v3mix(c, v3scale(c, 0.80), hair * 0.45);

  const crack = owCracks({ x: p.x * 4.5 + 17.0, y: p.y * 4.5 + 17.0 }, { x: P.x * 4.5, y: P.y * 4.5 }, 0.8, 0.018, 0.62);
  h -= crack * 0.16;
  ao -= crack * 0.6;
  c = v3mix(c, owSRGB(v3(0.300, 0.278, 0.250)), crack * 0.8);

  const blowMask = owFbm01(owWarp({ x: p.x * 1.05 + 9.0, y: p.y * 1.05 + 9.0 }, { x: P.x * 1.05, y: P.y * 1.05 }, 1.1, 3), { x: P.x * 1.05, y: P.y * 1.05 }, 4, 0.55);
  const blow = smoothstep(0.775, 0.845, blowMask);
  const blowEdge = smoothstep(0.745, 0.790, blowMask) - blow;
  let substrate = v3mix(owSRGB(v3(0.360, 0.245, 0.195)), owSRGB(v3(0.430, 0.400, 0.360)), owFbm01({ x: p.x * 9.0, y: p.y * 9.0 }, { x: P.x * 9.0, y: P.y * 9.0 }, 4, 0.5));
  substrate = v3scale(substrate, 0.85 + 0.3 * owFbm01({ x: p.x * 20.0, y: p.y * 20.0 }, { x: P.x * 20.0, y: P.y * 20.0 }, 3, 0.5));
  c = v3mix(c, substrate, blow * 0.85);
  h -= blow * 0.13;
  ao -= blow * 0.26;
  rough += blow * 0.10;
  c = v3addScalar(c, blowEdge * 0.06);
  h += blowEdge * 0.02;

  const ck = owWorley(owWarp({ x: p.x * 4.2 + 13.0, y: p.y * 4.2 + 13.0 }, { x: P.x * 4.2, y: P.y * 4.2 }, 0.6, 3), { x: P.x * 4.2, y: P.y * 4.2 }, 0.95);
  const ckSel = step(0.930, ck.idy);
  const ckSize = 0.22 + 0.20 * ck.idx;
  const ckShape = smoothstep(ckSize, ckSize * 0.3, ck.f1 * (0.70 + 0.60 * owFbm01({ x: p.x * 16.0, y: p.y * 16.0 }, { x: P.x * 16.0, y: P.y * 16.0 }, 3, 0.5)));
  const chip = ckSel * ckShape;
  let coat = v3mix(c, owSRGB(v3(0.392, 0.336, 0.284)), 0.52);
  coat = v3scale(coat, 0.90 + 0.20 * owFbm01({ x: p.x * 18.0, y: p.y * 18.0 }, { x: P.x * 18.0, y: P.y * 18.0 }, 3, 0.5));
  c = v3mix(c, coat, chip * 0.58);
  h -= chip * 0.05;
  ao -= chip * 0.26;
  rough += chip * 0.09;
  const ckLip = Math.max(ckSel * (smoothstep(ckSize * 1.25, ckSize, ck.f1) - ckShape), 0.0);
  c = v3scale(c, 1.0 + ckLip * 0.10);
  h += ckLip * 0.010;

  const stain = owFbm01({ x: p.x * 1.6, y: p.y * 3.2 }, { x: P.x * 1.6, y: P.y * 3.0 }, 5, 0.6);
  const tide = smoothstep(0.60, 0.78, stain) * (1 - smoothstep(0.78, 0.94, stain));
  c = v3mix(c, owSRGB(v3(0.400, 0.330, 0.245)), tide * 0.45);
  c = v3scale(c, 1.0 - smoothstep(0.50, 0.95, stain) * 0.34);
  rough += tide * 0.05;

  const mould = smoothstep(0.72, 0.95, owFbm01({ x: p.x * 4.0 + 25.0, y: p.y * 4.0 + 25.0 }, { x: P.x * 4.0, y: P.y * 4.0 }, 5, 0.6)) * smoothstep(0.45, 0.8, stain);
  c = v3mix(c, owSRGB(v3(0.085, 0.090, 0.080)), mould * 0.7);
  rough += mould * 0.08;

  const cavity = 1 - smoothstep(0.48, 0.72, h);
  c = v3mix(c, owSRGB(v3(0.22, 0.21, 0.19)), cavity * 0.30);

  return {
    alb: v3clamp(c, 0.02, 0.88),
    h: clamp(h, 0.0, 1.0),
    rough: clamp(rough, 0.35, 0.99),
    metal,
    ao: clamp(ao, 0.15, 1.0),
  };
}

function tileSurface(uv, uSeed) {
  const P = { x: 8.0, y: 8.0 };
  const N = 6.0;
  const p = { x: uv.x * P.x + uSeed * 4.4, y: uv.y * P.y + uSeed * 4.4 };

  const tp = { x: uv.x * N, y: uv.y * N };
  const id = { x: Math.floor(tp.x), y: Math.floor(tp.y) };
  const f = { x: fract(tp.x), y: fract(tp.y) };
  const rnd = owHash42({ x: id.x + uSeed, y: id.y + uSeed });

  const J = 0.045;
  const dxj = Math.min(f.x, 1.0 - f.x);
  const dyj = Math.min(f.y, 1.0 - f.y);
  const ex = smoothstep(J * 0.70, J * 1.02, dxj);
  const ey = smoothstep(J * 0.70, J * 1.02, dyj);
  const face = Math.min(ex, ey);

  const glaze = owFbm01({ x: f.x * 6.0 + rnd.x * 21.0, y: f.y * 6.0 + rnd.y * 21.0 }, { x: 48.0, y: 48.0 }, 4, 0.5);
  let cTile = v3mix(owSRGB(v3(0.700, 0.690, 0.660)), owSRGB(v3(0.470, 0.500, 0.505)), rnd.z * 0.7);
  cTile = v3scale(cTile, 0.93 + 0.13 * glaze);
  cTile = v3scale(cTile, 0.92 + 0.16 * rnd.y);

  const grout = owFbm01({ x: p.x * 20.0, y: p.y * 20.0 }, { x: P.x * 20.0, y: P.y * 20.0 }, 4, 0.5);
  let cGrout = v3scale(owSRGB(v3(0.400, 0.385, 0.360)), 0.85 + 0.3 * grout);
  cGrout = v3mix(cGrout, owSRGB(v3(0.13, 0.13, 0.12)), 0.45);

  const m = face;
  let h = mix(0.76 - (grout - 0.5) * 0.02, 0.82 + (rnd.w - 0.5) * 0.04, m);
  let c = v3mix(cGrout, cTile, m);
  let rough = mix(0.92, 0.20 + 0.22 * glaze + (rnd.z - 0.5) * 0.14, m);
  let ao = mix(0.40, 1.0, smoothstep(0.0, 0.8, face));
  const metal = 0.0;

  const broken = step(0.90, rnd.x);
  const crack = owCracks({ x: f.x * 3.0 + rnd.y * 9.0, y: f.y * 3.0 + rnd.z * 9.0 }, { x: 24.0, y: 24.0 }, 0.85, 0.04, 0.45) * m;
  c = v3mix(c, v3scale(c, 0.3), crack * 0.8);
  h -= crack * 0.08;
  ao -= crack * 0.5;
  const sub = owSRGB(v3(0.330, 0.300, 0.270));
  c = v3mix(c, sub, broken * m * 0.9);
  h -= broken * m * 0.14;
  rough = mix(rough, 0.95, broken * m);

  const wear = smoothstep(0.45, 0.95, owFbm01({ x: p.x * 2.0, y: p.y * 2.0 }, { x: P.x * 2.0, y: P.y * 2.0 }, 4, 0.55));
  rough += wear * 0.20 * m;
  c = v3scale(c, 1.0 - wear * 0.12);

  const cavity = 1 - smoothstep(0.68, 0.80, h);
  c = v3mix(c, owSRGB(v3(0.14, 0.13, 0.12)), cavity * 0.35);

  return {
    alb: v3clamp(c, 0.02, 0.85),
    h: clamp(h, 0.0, 1.0),
    rough: clamp(rough, 0.12, 0.95),
    metal,
    ao: clamp(ao, 0.15, 1.0),
  };
}

// ============================================================================
// Evaluate over a fixed grid + param variants, matching the seeds/params
// `mod.rs::LIBRARY` actually uses for `concrete` (seed 11, param [1,0,0,0])
// and `concrete_floor` (seed 47, param [0,1,0,0]), plus one extra "neither
// flag" variant to exercise the branch-free middle of the formAmt/jointAmt
// range, and the library seeds for brick (23) / plaster (5) / tile (31).
// ============================================================================

function pts() {
  return [
    { x: 0.02, y: 0.02 },
    { x: 0.13, y: 0.77 },
    { x: 0.42, y: 0.09 },
    { x: 0.65, y: 0.34 },
    { x: 0.91, y: 0.36 },
    { x: 0.99, y: 0.99 },
  ];
}

const out = {};

out.concrete_wall = { seed: 11, param: [1.0, 0.0, 0.0, 0.0], samples: pts().map((uv) => concreteSurface(uv, 11, { x: 1.0, y: 0.0 })) };
out.concrete_floor = { seed: 47, param: [0.0, 1.0, 0.0, 0.0], samples: pts().map((uv) => concreteSurface(uv, 47, { x: 0.0, y: 1.0 })) };
out.concrete_neither = { seed: 11, param: [0.0, 0.0, 0.0, 0.0], samples: pts().map((uv) => concreteSurface(uv, 11, { x: 0.0, y: 0.0 })) };
out.brick = { seed: 23, samples: pts().map((uv) => brickSurface(uv, 23)) };
out.plaster = { seed: 5, samples: pts().map((uv) => plasterSurface(uv, 5)) };
out.tile = { seed: 31, samples: pts().map((uv) => tileSurface(uv, 31)) };

process.stdout.write(JSON.stringify(out));
