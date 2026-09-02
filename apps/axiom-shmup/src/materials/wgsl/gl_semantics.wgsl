
// GLSL `mod(x, y)` = x - y * floor(x / y). NOT a truncated remainder: the
// lattice wrap feeds this negative coordinates.
fn owMod(x : f32, y : f32) -> f32 {
  return x - y * floor(x / y);
}
fn owMod2(p : vec2<f32>, q : vec2<f32>) -> vec2<f32> {
  return p - q * floor(p / q);
}

// GLSL `mix(a, b, t)` = a * (1 - t) + b * t.
fn owMix(a : f32, b : f32, t : f32) -> f32 {
  return a * (1.0 - t) + b * t;
}
fn owMix3(a : vec3<f32>, b : vec3<f32>, t : f32) -> vec3<f32> {
  return a * (1.0 - t) + b * t;
}
fn owMix3v(a : vec3<f32>, b : vec3<f32>, t : vec3<f32>) -> vec3<f32> {
  return a * (vec3<f32>(1.0) - t) + b * t;
}

// GLSL `clamp(x, lo, hi)` = min(max(x, lo), hi).
fn owClamp(x : f32, lo : f32, hi : f32) -> f32 {
  return min(max(x, lo), hi);
}
fn owClamp3(x : vec3<f32>, lo : vec3<f32>, hi : vec3<f32>) -> vec3<f32> {
  return min(max(x, lo), hi);
}

// GLSL `step(edge, x)` = 0 when x < edge, else 1.
fn owStep(edge : f32, x : f32) -> f32 {
  return select(1.0, 0.0, x < edge);
}
fn owStep3(edge : vec3<f32>, x : vec3<f32>) -> vec3<f32> {
  return select(vec3<f32>(1.0), vec3<f32>(0.0), x < edge);
}

// GLSL `smoothstep(e0, e1, x)`: t = clamp((x - e0) / (e1 - e0), 0, 1);
// return t * t * (3 - 2 * t).
fn owSmoothstep(e0 : f32, e1 : f32, x : f32) -> f32 {
  let t = owClamp((x - e0) / (e1 - e0), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}

// GLSL `sign(x)`: -1 below zero, 0 AT zero, +1 above.
fn owSign(x : f32) -> f32 {
  return select(0.0, -1.0, x < 0.0) + select(0.0, 1.0, x > 0.0);
}
