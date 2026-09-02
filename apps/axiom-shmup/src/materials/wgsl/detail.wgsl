
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed);
  // ~10 mm swell, ~3.5 mm tooth
  let a = owFbm01(p * 3.0, P * 3.0, 4, 0.55);
  let b = owFbm01(p * 9.0, P * 9.0, 4, 0.52);
  // 3.9 mm pits and 1.6 mm grains — both wide enough to survive two mip levels
  let pores = owWorley(p * 8.0, P * 8.0, 1.0);
  let grit  = owWorley(p * 20.0, P * 20.0, 1.0);
  let scr = owScratches(p * 2.5, P * 2.5, 16.0, 1.0, 0.66)
          + owScratches(p * 4.0 + vec2<f32>(5.0), P * 4.0, 11.0, -2.0, 0.70) * 0.8;
  // Proud grains: a solid, rounded bump rather than a threshold speck.
  let gritA = owSmoothstep(0.34, 0.08, pores.x) * owStep(0.38, pores.z);
  let gritB = owSmoothstep(0.30, 0.06, grit.x) * owStep(0.34, grit.z);
  let pit   = owSmoothstep(0.26, 0.0, pores.x) * owStep(0.72, pores.w);
  h = 0.5 + (a - 0.5) * 0.34 + (b - 0.5) * 0.26;
  h = h - pit * 0.38;
  h = h + gritA * 0.26 * (0.5 + grit.z) + gritB * 0.20;
  h = h - owClamp(scr, 0.0, 1.0) * 0.18;
  // Albedo tracks the grain so a proud grain reads light and its trough reads
  // dark; the shader scales this by the per-surface detail albedo amount.
  alb = vec3<f32>(0.5 + (a - 0.5) * 0.22 + (b - 0.5) * 0.15
           + gritA * 0.16 + gritB * 0.10 - pit * 0.14);
  rough = 0.5 + (b - 0.5) * 0.5;
  metal = 0.0;
  ao = 1.0 - pit * 0.45 - gritB * 0.10;
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
