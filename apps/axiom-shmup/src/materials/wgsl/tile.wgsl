
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let N = 6.0;
  let p = uv * P + vec2<f32>(U.seed * 4.4);

  let tp = uv * N;
  let id = floor(tp);
  let f = fract(tp);
  let rnd = owHash42(id + vec2<f32>(U.seed));

  // Flat grout bed with a hard arris at the tile edge: a full-width ramp is
  // what makes a joint read as a drawn line instead of a recess.
  let J = 0.045;
  let dxj = min(f.x, 1.0 - f.x);
  let dyj = min(f.y, 1.0 - f.y);
  let ex = owSmoothstep(J * 0.70, J * 1.02, dxj);
  let ey = owSmoothstep(J * 0.70, J * 1.02, dyj);
  let face = min(ex, ey);

  let glaze = owFbm01(f * 6.0 + rnd.xy * 21.0, vec2<f32>(48.0), 4, 0.5);
  var cTile = owMix3(owSRGB(vec3<f32>(0.700, 0.690, 0.660)), owSRGB(vec3<f32>(0.470, 0.500, 0.505)), rnd.z * 0.7);
  cTile *= 0.93 + 0.13 * glaze;
  cTile *= 0.92 + 0.16 * rnd.y;                                 // per-tile batch shade

  let grout = owFbm01(p * 20.0, P * 20.0, 4, 0.5);
  var cGrout = owSRGB(vec3<f32>(0.400, 0.385, 0.360)) * (0.85 + 0.3 * grout);
  cGrout = owMix3(cGrout, owSRGB(vec3<f32>(0.13, 0.13, 0.12)), 0.45);   // grout is always filthy

  let m = face;
  // 0.06 of a 0.03 m relief = 1.8 mm of grout recess.
  h = owMix(0.76 - (grout - 0.5) * 0.02, 0.82 + (rnd.w - 0.5) * 0.04, m);
  var c = owMix3(cGrout, cTile, m);
  // glazed tile has to stay glossy enough to actually catch a highlight
  rough = owMix(0.92, 0.20 + 0.22 * glaze + (rnd.z - 0.5) * 0.14, m);
  ao = owMix(0.40, 1.0, owSmoothstep(0.0, 0.8, face));
  metal = 0.0;

  // chipped / cracked / missing tiles
  let broken = owStep(0.90, rnd.x);
  let crack = owCracks(f * 3.0 + rnd.yz * 9.0, vec2<f32>(24.0), 0.85, 0.04, 0.45) * m;
  c = owMix3(c, c * 0.3, crack * 0.8);
  h -= crack * 0.08;
  ao -= crack * 0.5;
  let sub = owSRGB(vec3<f32>(0.330, 0.300, 0.270));
  c = owMix3(c, sub, broken * m * 0.9);
  h -= broken * m * 0.14;
  rough = owMix(rough, 0.95, broken * m);

  // scuffs and traffic wear
  let wear = owSmoothstep(0.45, 0.95, owFbm01(p * 2.0, P * 2.0, 4, 0.55));
  rough += wear * 0.20 * m;
  c *= 1.0 - wear * 0.12;

  let cavity = 1.0 - owSmoothstep(0.68, 0.80, h);
  c = owMix3(c, owSRGB(vec3<f32>(0.14, 0.13, 0.12)), cavity * 0.35);

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.85));
  rough = owClamp(rough, 0.12, 0.95);
  ao = owClamp(ao, 0.15, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
