
// ---------------------------------------------------------------- hashes ----
fn owHash11(p0 : f32) -> f32 {
  var p = fract(p0 * 0.1031);
  p = p * (p + 33.33);
  p = p * (p + p);
  return fract(p);
}
fn owHash12(p : vec2<f32>) -> f32 {
  var p3 = fract(p.xyx * 0.1031);
  p3 = p3 + vec3<f32>(dot(p3, p3.yzx + vec3<f32>(33.33)));
  return fract((p3.x + p3.y) * p3.z);
}
fn owHash22(p : vec2<f32>) -> vec2<f32> {
  var p3 = fract(p.xyx * vec3<f32>(0.1031, 0.1030, 0.0973));
  p3 = p3 + vec3<f32>(dot(p3, p3.yzx + vec3<f32>(33.33)));
  return fract((p3.xx + p3.yz) * p3.zy);
}
fn owHash32(p : vec2<f32>) -> vec3<f32> {
  var p3 = fract(p.xyx * vec3<f32>(0.1031, 0.1030, 0.0973));
  p3 = p3 + vec3<f32>(dot(p3, p3.yxz + vec3<f32>(33.33)));
  return fract((p3.xxy + p3.yzz) * p3.zyx);
}
fn owHash42(p : vec2<f32>) -> vec4<f32> {
  var p4 = fract(p.xyxy * vec4<f32>(0.1031, 0.1030, 0.0973, 0.1099));
  p4 = p4 + vec4<f32>(dot(p4, p4.wzxy + vec4<f32>(33.33)));
  return fract((p4.xxyz + p4.yzzw) * p4.zywx);
}

// ------------------------------------------------------- gradient noise ----
fn owGrad2(i : vec2<f32>, per : vec2<f32>) -> vec2<f32> {
  let a = owHash12(owMod2(i, per) + vec2<f32>(0.317)) * 6.28318530718;
  return vec2<f32>(cos(a), sin(a));
}

// Periodic gradient (Perlin) noise. Returns ~[-1,1].
fn owNoise(p : vec2<f32>, per : vec2<f32>) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let u = f * f * f * (f * (f * 6.0 - vec2<f32>(15.0)) + vec2<f32>(10.0));
  let a = dot(owGrad2(i + vec2<f32>(0.0, 0.0), per), f - vec2<f32>(0.0, 0.0));
  let b = dot(owGrad2(i + vec2<f32>(1.0, 0.0), per), f - vec2<f32>(1.0, 0.0));
  let c = dot(owGrad2(i + vec2<f32>(0.0, 1.0), per), f - vec2<f32>(0.0, 1.0));
  let d = dot(owGrad2(i + vec2<f32>(1.0, 1.0), per), f - vec2<f32>(1.0, 1.0));
  return owMix(owMix(a, b, u.x), owMix(c, d, u.x), u.y) * 1.4142;
}
fn owNoise01(p : vec2<f32>, per : vec2<f32>) -> f32 { return owNoise(p, per) * 0.5 + 0.5; }

// Periodic value noise — blockier, good for cell-ish tint variation.
fn owValue(p : vec2<f32>, per : vec2<f32>) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let u = f * f * (vec2<f32>(3.0) - 2.0 * f);
  let a = owHash12(owMod2(i + vec2<f32>(0.0, 0.0), per) + vec2<f32>(1.7));
  let b = owHash12(owMod2(i + vec2<f32>(1.0, 0.0), per) + vec2<f32>(1.7));
  let c = owHash12(owMod2(i + vec2<f32>(0.0, 1.0), per) + vec2<f32>(1.7));
  let d = owHash12(owMod2(i + vec2<f32>(1.0, 1.0), per) + vec2<f32>(1.7));
  return owMix(owMix(a, b, u.x), owMix(c, d, u.x), u.y);
}

// ------------------------------------------------------------------ fbm ----
fn owFbm(p0 : vec2<f32>, per0 : vec2<f32>, oct : i32, gain : f32) -> f32 {
  var p = p0;
  var per = per0;
  var s = 0.0;
  var a = 0.5;
  var n = 0.0;
  for (var i : i32 = 0; i < 10; i = i + 1) {
    if (i >= oct) { break; }
    s = s + a * owNoise(p, per);
    n = n + a;
    p = p * 2.0; per = per * 2.0; a = a * gain;
  }
  return s / max(n, 1e-4);
}
fn owFbm01(p : vec2<f32>, per : vec2<f32>, oct : i32, gain : f32) -> f32 { return owFbm(p, per, oct, gain) * 0.5 + 0.5; }

// Ridged fbm — sharp creases, good for cracks / rock. Returns [0,1].
fn owRidged(p0 : vec2<f32>, per0 : vec2<f32>, oct : i32, gain : f32) -> f32 {
  var p = p0;
  var per = per0;
  var s = 0.0;
  var a = 0.5;
  var n = 0.0;
  for (var i : i32 = 0; i < 10; i = i + 1) {
    if (i >= oct) { break; }
    let v = 1.0 - abs(owNoise(p, per));
    s = s + a * v * v;
    n = n + a;
    p = p * 2.0; per = per * 2.0; a = a * gain;
  }
  return s / max(n, 1e-4);
}

// Billowy fbm — puffy clumps, good for rust blooms and clay.
fn owBillow(p0 : vec2<f32>, per0 : vec2<f32>, oct : i32, gain : f32) -> f32 {
  var p = p0;
  var per = per0;
  var s = 0.0;
  var a = 0.5;
  var n = 0.0;
  for (var i : i32 = 0; i < 10; i = i + 1) {
    if (i >= oct) { break; }
    s = s + a * abs(owNoise(p, per));
    n = n + a;
    p = p * 2.0; per = per * 2.0; a = a * gain;
  }
  return s / max(n, 1e-4);
}

// --------------------------------------------------------- domain warp -----
fn owWarp(p : vec2<f32>, per : vec2<f32>, amp : f32, oct : i32) -> vec2<f32> {
  let q = vec2<f32>(owFbm(p + vec2<f32>(1.7, 9.2), per, oct, 0.5),
                    owFbm(p + vec2<f32>(8.3, 2.8), per, oct, 0.5));
  return p + amp * q;
}

// -------------------------------------------------------------- worley -----
// Periodic Worley/Voronoi.
//  .x = F1 distance, .y = F2 distance, .z = hash id of the F1 cell,
//  .w = second hash of the F1 cell.
fn owWorley(p : vec2<f32>, per : vec2<f32>, jitter : f32) -> vec4<f32> {
  let ip = floor(p);
  let fp = fract(p);
  var f1 = 8.0;
  var f2 = 8.0;
  var id = vec2<f32>(0.0);
  for (var y : i32 = -1; y <= 1; y = y + 1) {
    for (var x : i32 = -1; x <= 1; x = x + 1) {
      let g = vec2<f32>(f32(x), f32(y));
      let cell = owMod2(ip + g, per);
      let o = owHash22(cell + vec2<f32>(0.771)) * jitter + vec2<f32>((1.0 - jitter) * 0.5);
      let r = g + o - fp;
      let d = dot(r, r);
      if (d < f1) { f2 = f1; f1 = d; id = owHash22(cell + vec2<f32>(3.117)); }
      else if (d < f2) { f2 = d; }
    }
  }
  return vec4<f32>(sqrt(f1), sqrt(f2), id);
}

// Distance to the *edge* of the Voronoi cell (Quilez two-pass). Much better
// looking crack networks than F2-F1. Returns [0, ~0.7].
fn owVoronoiEdge(p : vec2<f32>, per : vec2<f32>, jitter : f32) -> f32 {
  let ip = floor(p);
  let fp = fract(p);
  var mr = vec2<f32>(0.0);
  var mg = vec2<f32>(0.0);
  var md = 8.0;
  for (var y : i32 = -1; y <= 1; y = y + 1) {
    for (var x : i32 = -1; x <= 1; x = x + 1) {
      let g = vec2<f32>(f32(x), f32(y));
      let o = owHash22(owMod2(ip + g, per) + vec2<f32>(0.771)) * jitter + vec2<f32>((1.0 - jitter) * 0.5);
      let r = g + o - fp;
      let d = dot(r, r);
      if (d < md) { md = d; mr = r; mg = g; }
    }
  }
  md = 8.0;
  for (var y : i32 = -2; y <= 2; y = y + 1) {
    for (var x : i32 = -2; x <= 2; x = x + 1) {
      let g = mg + vec2<f32>(f32(x), f32(y));
      let o = owHash22(owMod2(ip + g, per) + vec2<f32>(0.771)) * jitter + vec2<f32>((1.0 - jitter) * 0.5);
      let r = g + o - fp;
      let diff = r - mr;
      if (dot(diff, diff) > 1e-5) {
        md = min(md, dot(0.5 * (mr + r), normalize(diff)));
      }
    }
  }
  return md;
}

// Crack network: warped voronoi edges, thinned and broken up so lines
// terminate instead of forming a perfect mesh. Returns [0,1], 1 = deep crack.
fn owCracks(p : vec2<f32>, per : vec2<f32>, jitter : f32, width : f32, breakUp : f32) -> f32 {
  let wp = owWarp(p, per, 0.20, 3);
  let e = owVoronoiEdge(wp, per, jitter);
  var c = 1.0 - owSmoothstep(0.0, width, e);
  // Break the network so it reads as damage, not as a net.
  let mask = owFbm01(p * 1.7 + vec2<f32>(11.3), per * 1.7, 4, 0.55);
  c = c * owSmoothstep(breakUp, breakUp + 0.28, mask);
  return owClamp(c, 0.0, 1.0);
}

// ------------------------------------------------------------ utilities ----
fn owSat(x : f32) -> f32 { return owClamp(x, 0.0, 1.0); }
fn owSat3(x : vec3<f32>) -> vec3<f32> { return owClamp3(x, vec3<f32>(0.0), vec3<f32>(1.0)); }
fn owRemap(x : f32, a : f32, b : f32, c : f32, d : f32) -> f32 {
  return c + (d - c) * owClamp((x - a) / max(b - a, 1e-5), 0.0, 1.0);
}
fn owRot(p : vec2<f32>, a : f32) -> vec2<f32> {
  let s = sin(a);
  let c = cos(a);
  return mat2x2<f32>(vec2<f32>(c, -s), vec2<f32>(s, c)) * p;
}
// sRGB hex-ish helper: authoring colours in gamma space, output linear.
fn owSRGB(c : vec3<f32>) -> vec3<f32> {
  return owMix3v(pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c / 12.92, owStep3(c, vec3<f32>(0.04045)));
}
// Anisotropic shear that preserves tileability: 'k' and 'stretch' must be
// integers so the lattice still wraps on 'per'.
fn owShear(p : vec2<f32>, k : f32, stretch : f32) -> vec2<f32> {
  return vec2<f32>(p.x + p.y * k, p.y * stretch);
}
fn owShearPer(per : vec2<f32>, stretch : f32) -> vec2<f32> {
  return vec2<f32>(per.x, per.y * stretch);
}

// Scratch lines: long thin streaks running along a sheared axis. [0,1].
fn owScratches(p : vec2<f32>, per : vec2<f32>, stretch : f32, k : f32, thin : f32) -> f32 {
  let q = owShear(p, k, stretch);
  let qper = owShearPer(per, stretch);
  let n = owFbm01(q, qper, 4, 0.5);
  return owSmoothstep(thin, thin + 0.06, n) * (1.0 - owSmoothstep(thin + 0.06, thin + 0.2, n));
}
