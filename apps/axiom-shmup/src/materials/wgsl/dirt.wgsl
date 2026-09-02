
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 3.4);

  let macro_ = owFbm01(p * 0.6, P * 0.6, 4, 0.62);
  let clump = owBillow(owWarp(p * 3.0, P * 3.0, 0.6, 3), P * 3.0, 5, 0.55);
  let fine  = owFbm01(p * 14.0, P * 14.0, 4, 0.5);
  let micro = owFbm01(p * 22.0, P * 22.0, 3, 0.5);

  let cDry  = owSRGB(vec3<f32>(0.430, 0.350, 0.255));
  let cWet  = owSRGB(vec3<f32>(0.185, 0.140, 0.100));
  let cPale = owSRGB(vec3<f32>(0.560, 0.490, 0.385));
  var c = owMix3(cDry, cPale, owSmoothstep(0.45, 0.9, macro_));
  c = owMix3(c, cWet, owSmoothstep(0.55, 0.15, macro_) * 0.8);
  // Halved high-frequency albedo contrast; the read moves into height/roughness.
  c *= 0.94 + 0.11 * fine;
  c *= 0.975 + 0.05 * micro;

  h = 0.55 + (macro_ - 0.5) * 0.14 + (clump - 0.5) * 0.16 + (fine - 0.5) * 0.075;
  rough = 0.88 + (fine - 0.5) * 0.14 + (micro - 0.5) * 0.10;
  metal = 0.0;
  ao = 1.0;

  // dried mud cracks in the flat pans
  let pan = owSmoothstep(0.35, 0.65, macro_);
  let mud = owCracks(p * 2.4, P * 2.4, 0.85, 0.045, 0.35) * pan;
  h -= mud * 0.16;
  ao -= mud * 0.32;
  c = owMix3(c, cWet * 0.7, mud * 0.75);
  // the mud plates curl up at their edges
  let plateLift = owSmoothstep(0.10, 0.0, mud) * pan;
  h += plateLift * 0.01;

  // stones of two grades
  let st = owWorley(p * 11.0, P * 11.0, 1.0);
  let stone = owSmoothstep(0.30, 0.11, st.x) * owStep(0.62, st.w);
  let scol = owMix3(owSRGB(vec3<f32>(0.330, 0.315, 0.295)), owSRGB(vec3<f32>(0.600, 0.575, 0.540)), st.z);
  c = owMix3(c, scol, stone * 0.6);
  h += stone * 0.085;
  rough = owMix(rough, 0.52 + 0.28 * st.z, stone * 0.8);
  ao -= owSmoothstep(0.36, 0.28, st.x) * owStep(0.62, st.w) * 0.10;

  let grit = owWorley(p * 22.0, P * 22.0, 1.0);
  let gritM = owSmoothstep(0.26, 0.08, grit.x) * owStep(0.55, grit.w);
  c = owMix3(c, owMix3(scol, cPale, grit.z), gritM * 0.4);
  h += gritM * 0.015;

  // dead grass / organic litter
  var litter = owSmoothstep(0.70, 0.86, owFbm01(owShear(p * 8.0, 1.0, 5.0), owShearPer(P * 8.0, 5.0), 4, 0.5));
  litter *= owSmoothstep(0.4, 0.8, macro_);
  c = owMix3(c, owSRGB(vec3<f32>(0.330, 0.290, 0.160)), litter * 0.5);
  h += litter * 0.012;
  rough += litter * 0.05;

  // sparse moss in the damp low spots
  let moss = owSmoothstep(0.74, 0.92, owFbm01(p * 4.5 + vec2<f32>(19.0), P * 4.5, 5, 0.6)) * owSmoothstep(0.5, 0.1, macro_);
  c = owMix3(c, owSRGB(vec3<f32>(0.150, 0.185, 0.105)), moss * 0.65);

  let cavity = 1.0 - owSmoothstep(0.40, 0.70, h);
  ao -= cavity * 0.14;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.72));
  rough = owClamp(rough, 0.45, 0.99);
  ao = owClamp(ao, 0.72, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
