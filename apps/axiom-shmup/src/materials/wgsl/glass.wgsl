
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 2.2);

  let smear = owFbm01(owShear(p * 3.0, 1.0, 6.0), owShearPer(P * 3.0, 6.0), 4, 0.5);
  let dustF = owFbm01(p * 5.0, P * 5.0, 5, 0.55);
  let spots = owWorley(p * 24.0, P * 24.0, 1.0).x;
  let fine = owFbm01(p * 12.0, P * 12.0, 3, 0.5);

  // glass itself is almost black in albedo; the look comes from reflections
  var c = owSRGB(vec3<f32>(0.045, 0.050, 0.052));

  let dirty = owSmoothstep(0.45, 0.85, dustF);
  c = owMix3(c, owSRGB(vec3<f32>(0.300, 0.290, 0.265)), dirty * 0.35);

  rough = 0.045 + smear * 0.10 * owSmoothstep(0.3, 0.9, dustF) + dirty * 0.22;
  rough += owSmoothstep(0.30, 0.05, spots) * 0.25;             // water spots
  rough += (fine - 0.5) * 0.02;

  // fine scratches
  let scr = owScratches(p * 2.0, P * 2.0, 24.0, 1.0, 0.70);
  rough += scr * 0.25;
  c += vec3<f32>(scr * 0.02);

  h = 0.5 + (smear - 0.5) * 0.004;
  metal = 0.0;
  ao = 1.0 - dirty * 0.1;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.5));
  rough = owClamp(rough, 0.02, 0.7);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
