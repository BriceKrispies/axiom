
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 15.1);

  // brushing runs along X: heavy shear so the noise stretches into fibres
  let bp = owShear(p, 0.0, 64.0);
  let BP = owShearPer(P, 64.0);
  let brush1 = owFbm01(bp * 2.0, BP * 2.0, 4, 0.5);
  let brush2 = owFbm01(bp * 8.0 + vec2<f32>(3.0), BP * 8.0, 3, 0.5);
  let brush3 = owFbm01(owShear(p * 4.0, 0.0, 24.0), owShearPer(P * 4.0, 24.0), 3, 0.5);
  let brush = brush1 * 0.5 + brush2 * 0.32 + brush3 * 0.18;

  // RENAMED: the source calls this `macro`, which is a WGSL reserved word.
  let macroNoise = owFbm01(p * 0.9, P * 0.9, 3, 0.6);

  var c = owSRGB(vec3<f32>(0.560, 0.565, 0.575));
  c *= 0.93 + 0.13 * brush;
  c *= 0.97 + 0.06 * macroNoise;

  metal = 1.0;
  rough = 0.22 + brush * 0.24 + (macroNoise - 0.5) * 0.06;
  h = 0.78 + (brush - 0.5) * 0.012;
  ao = 1.0;

  // deeper score lines
  let score = owScratches(p * 1.0, P, 40.0, 0.0, 0.60);
  rough += score * 0.22;
  h -= score * 0.006;
  c *= 1.0 - score * 0.05;

  // cross scratches from handling
  let cross = owScratches(p * 3.0, P * 3.0, 8.0, 3.0, 0.70) * 0.7;
  rough += cross * 0.20;
  h -= cross * 0.004;

  // dents: shallow, wide, they break the reflection
  let dent = owFbm01(p * 3.0 + vec2<f32>(7.0), P * 3.0, 3, 0.6);
  h += (dent - 0.5) * 0.05;

  // fingerprints and grease smudges — the thing that sells brushed metal
  let smudge = owSmoothstep(0.58, 0.86, owFbm01(owWarp(p * 2.2 + vec2<f32>(19.0), P * 2.2, 0.7, 3), P * 2.2, 4, 0.55));
  rough += smudge * 0.22;
  c *= 1.0 - smudge * 0.06;
  metal -= smudge * 0.10;

  // grime settling in
  let grime = owSmoothstep(0.66, 0.95, owFbm01(p * 5.0, P * 5.0, 4, 0.55));
  c = owMix3(c, owSRGB(vec3<f32>(0.180, 0.175, 0.165)), grime * 0.35);
  rough += grime * 0.18;
  metal -= grime * 0.35;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.88));
  rough = owClamp(rough, 0.08, 0.95);
  metal = owClamp(metal, 0.0, 1.0);
  ao = owClamp(ao, 0.4, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
