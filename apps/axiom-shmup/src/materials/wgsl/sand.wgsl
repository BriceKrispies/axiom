
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 8.2);

  // ---- wind ripples: sheared sine, gently warped so the crests meander ----
  let rp = owShear(p * 1.0, 1.0, 1.0);
  let warp = owFbm(p * 0.9, P * 0.9, 3, 0.55);
  var ripple = sin((rp.y * 1.0 + warp * 0.55) * 6.28318);
  // asymmetric profile: gentle windward slope, sharp lee crest
  ripple = ripple * 0.5 + 0.5;
  ripple = pow(ripple, 1.7) * 0.75 + ripple * 0.25;
  let rippleAmp = owSmoothstep(0.20, 0.70, owFbm01(p * 0.7, P * 0.7, 3, 0.6));
  let secondary = sin((p.y * 3.0 + p.x * 1.0 + warp * 0.8) * 6.28318) * 0.5 + 0.5;

  let dune = owFbm01(p * 0.5, P * 0.5, 4, 0.6);
  let mid  = owFbm01(p * 5.0, P * 5.0, 5, 0.5);
  let grain = owFbm01(p * 18.0, P * 18.0, 4, 0.55);
  let gcell = owWorley(p * 24.0, P * 24.0, 1.0);

  h = 0.50 + (dune - 0.5) * 0.16 + (mid - 0.5) * 0.05
    + (ripple - 0.5) * 0.26 * rippleAmp + (secondary - 0.5) * 0.06 * rippleAmp
    + (grain - 0.5) * 0.018;

  let cLight = owSRGB(vec3<f32>(0.760, 0.660, 0.480));
  let cMid   = owSRGB(vec3<f32>(0.610, 0.510, 0.360));
  let cDamp  = owSRGB(vec3<f32>(0.360, 0.290, 0.205));
  var c = owMix3(cMid, cLight, owSmoothstep(0.3, 0.8, dune));
  c = owMix3(c, cDamp, owSmoothstep(0.62, 0.28, h) * 0.55);           // damp in the hollows
  // coarse grains collect on the crests, fines in the troughs
  c = owMix3(c, cLight * 1.06, owSmoothstep(0.45, 0.85, ripple) * rippleAmp * 0.35);
  c = owMix3(c, cMid * 0.88, owSmoothstep(0.45, 0.10, ripple) * rippleAmp * 0.30);
  c *= 0.90 + 0.18 * grain;
  // sparkle from quartz grains
  c += vec3<f32>(owSmoothstep(0.22, 0.0, gcell.x) * owStep(0.86, gcell.z) * 0.10);

  rough = 0.90 + (grain - 0.5) * 0.10 - owSmoothstep(0.6, 0.3, h) * 0.12;
  metal = 0.0;
  ao = 1.0 - owSmoothstep(0.55, 0.25, h) * 0.10;

  // ---- pebbles and shell fragments sitting on top ----
  let peb = owWorley(p * 18.0, P * 18.0, 1.0);
  let pebble = owSmoothstep(0.30, 0.10, peb.x) * owStep(0.80, peb.w);
  let pcol = owMix3(owSRGB(vec3<f32>(0.400, 0.370, 0.330)), owSRGB(vec3<f32>(0.690, 0.660, 0.620)), peb.z);
  c = owMix3(c, pcol, pebble * 0.85);
  h += pebble * 0.05;
  rough = owMix(rough, 0.55 + 0.25 * peb.z, pebble * 0.8);
  ao -= owSmoothstep(0.40, 0.30, peb.x) * owStep(0.80, peb.w) * 0.08;

  // ---- scattered dry debris / dark mineral streaks ----
  let streak = owSmoothstep(0.62, 0.88, owFbm01(owShear(p * 2.5, 2.0, 4.0), owShearPer(P * 2.5, 4.0), 4, 0.5));
  c = owMix3(c, cDamp * 1.1, streak * 0.22);

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.82));
  rough = owClamp(rough, 0.35, 0.99);
  ao = owClamp(ao, 0.80, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
