
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 9.6);

  // moulded pebble grain
  let pb = owWorley(p * 12.0, P * 12.0, 1.0);
  let pebble = owSmoothstep(0.42, 0.10, pb.x);
  let fine = owFbm01(p * 12.0, P * 12.0, 3, 0.5);
  let macroF = owFbm01(p * 1.5, P * 1.5, 4, 0.6);   // GLSL name was `macro` (reserved in WGSL)

  h = 0.60 + pebble * 0.10 + (fine - 0.5) * 0.02 + (macroF - 0.5) * 0.03;
  // 0.20 sRGB ~= 0.031 linear. Anything darker lands under the 0.02 albedo
  // floor applied below, which clamps the entire surface flat (a black,
  // detail-free rubber that violates the "no flat surfaces" bar).
  var c = owSRGB(vec3<f32>(0.200, 0.200, 0.206));
  c *= 0.85 + 0.25 * (pebble * 0.5 + 0.5);
  c *= 0.94 + 0.10 * fine;

  rough = 0.88 - pebble * 0.06 + (fine - 0.5) * 0.08;
  metal = 0.0;
  ao = owMix(0.6, 1.0, pebble * 0.5 + 0.5);

  // mould seam
  let seam = 1.0 - owSmoothstep(0.0, 0.012, abs(fract(uv.y * 2.0 + 0.5) - 0.5));
  h += seam * 0.03;
  c *= 1.0 + seam * 0.35;
  rough -= seam * 0.10;

  // scuffs: rubber goes chalky-grey where it abrades
  let scuff = owSmoothstep(0.55, 0.88, owFbm01(owWarp(p * 3.0, P * 3.0, 0.8, 3), P * 3.0, 4, 0.55));
  c = owMix3(c, owSRGB(vec3<f32>(0.220, 0.218, 0.212)), scuff * 0.45);
  rough += scuff * 0.06;
  h -= scuff * 0.015;

  // cracking from ozone / age
  let crack = owCracks(p * 7.0, P * 7.0, 0.9, 0.028, 0.62);
  h -= crack * 0.06;
  c *= 1.0 - crack * 0.35;
  ao -= crack * 0.35;

  // dust
  let dust = owSmoothstep(0.5, 0.9, owFbm01(p * 8.0, P * 8.0, 4, 0.5));
  c = owMix3(c, owSRGB(vec3<f32>(0.290, 0.275, 0.250)), dust * 0.16);

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.35));
  rough = owClamp(rough, 0.55, 0.99);
  ao = owClamp(ao, 0.3, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
