
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 11.3);

  // ---- substrate: steel with a mill finish ----
  let mill = owFbm01(owShear(p * 5.0, 1.0, 8.0), owShearPer(P * 5.0, 8.0), 4, 0.5);
  let steel = owSRGB(vec3<f32>(0.330, 0.335, 0.345)) * (0.88 + 0.2 * mill);

  // ---- rust that has crept under the paint ----
  let bloom = 1.0 - owBillow(owWarp(p * 1.8, P * 1.8, 1.1, 4), P * 1.8, 5, 0.6);
  let rustField = owSmoothstep(0.60, 0.92, bloom);
  let rustGrain = owFbm01(p * 22.0, P * 22.0, 4, 0.55);
  let rustCol = owRustColour(rustField, rustGrain);

  // ---- paint: an industrial coat with roller texture and orange peel ----
  let peel = owFbm01(p * 22.0, P * 22.0, 4, 0.5);
  let roller = owFbm01(owShear(p * 2.0, 0.0, 3.0), owShearPer(P * 2.0, 3.0), 4, 0.5);
  var paint = U.tint_a * (0.90 + 0.16 * roller);
  paint *= 0.96 + 0.08 * peel;
  // sun-bleached on the up-facing halves
  let bleach = owSmoothstep(0.35, 0.85, owFbm01(p * 0.8, P * 0.8, 3, 0.6));
  paint = owMix3(paint, paint * 1.25 + vec3<f32>(0.03), bleach * 0.5);

  // ---- chipping: paint fails at scratches, impacts and along its own edges ----
  let chipField = owFbm01(owWarp(p * 2.6 + vec2<f32>(4.0), P * 2.6, 0.9, 3), P * 2.6, 5, 0.55);
  let chipEdge = owFbm01(p * 12.0, P * 12.0, 4, 0.5);
  // Paint mostly holds: only the top of the distribution actually fails, and
  // it fails hardest where rust is already lifting it from underneath.
  let chipSrc = chipField * 0.60 + chipEdge * 0.20 + rustField * 0.32 + U.param.z * 0.25;
  var chip = owSmoothstep(0.66, 0.92, chipSrc);
  // small impact chips scattered around
  let dings = owWorley(p * 20.0, P * 20.0, 1.0);
  let ding = owSmoothstep(0.14, 0.03, dings.x) * owStep(0.88, dings.w);
  chip = owClamp(chip + ding, 0.0, 1.0);

  // scratches that cut down to bare metal
  var scr = owScratches(p * 2.5, P * 2.5, 14.0, 1.0, 0.62);
  scr += owScratches(p * 4.0 + vec2<f32>(21.0), P * 4.0, 10.0, -1.0, 0.66) * 0.8;
  scr = owClamp(scr, 0.0, 1.0);

  // ---- layer stack: paint over primer over rust over steel ----
  let primer = owSRGB(vec3<f32>(0.470, 0.300, 0.180));
  let primerBand = owSmoothstep(0.0, 0.35, chip) * (1.0 - owSmoothstep(0.35, 0.6, chip));

  var c = paint;
  var r = 0.42 + (peel - 0.5) * 0.22 + bleach * 0.16;
  var mtl = 0.0;
  h = 0.74 + (roller - 0.5) * 0.02 + (peel - 0.5) * 0.012;
  ao = 1.0;

  c = owMix3(c, primer, primerBand * 0.7);
  c = owMix3(c, rustCol, owSmoothstep(0.35, 0.75, chip) * (0.55 + 0.45 * rustField));
  c = owMix3(c, steel, owSmoothstep(0.75, 0.95, chip) * (1.0 - rustField) * 0.9);
  r = owMix(r, 0.88, owSmoothstep(0.3, 0.8, chip) * (0.4 + 0.6 * rustField));
  r = owMix(r, 0.38, owSmoothstep(0.8, 1.0, chip) * (1.0 - rustField));
  mtl = owMix(0.0, 1.0, owSmoothstep(0.78, 0.96, chip) * (1.0 - owSmoothstep(0.2, 0.7, rustField)));
  h -= owSmoothstep(0.4, 0.8, chip) * 0.16;     // paint has real thickness
  ao -= owSmoothstep(0.35, 0.7, chip) * 0.22;
  // the lip of a chip is a bright hard edge
  let lip = owSmoothstep(0.30, 0.42, chip) * (1.0 - owSmoothstep(0.42, 0.55, chip));
  c *= 1.0 + lip * 0.15;
  h += lip * 0.05;

  // scratches on top of everything
  c = owMix3(c, owSRGB(vec3<f32>(0.500, 0.505, 0.515)), scr * 0.55);
  mtl = owMix(mtl, 1.0, scr * 0.6);
  r = owMix(r, 0.26, scr * 0.55);

  // ---- dirt and rain streaks ----
  let streak = owFbm01(vec2<f32>(p.x * 6.0, p.y * 0.7), vec2<f32>(P.x * 6.0, max(P.y, 1.0)), 5, 0.55);
  let grime = owSmoothstep(0.52, 0.92, streak);
  c *= 1.0 - grime * 0.30;
  r += grime * 0.10;
  mtl *= 1.0 - grime * 0.5;
  // rust bleed running down from the chips
  let bleed = owSmoothstep(0.66, 0.95, streak) * owSmoothstep(0.2, 0.6, rustField);
  c = owMix3(c, owSRGB(vec3<f32>(0.360, 0.190, 0.090)), bleed * 0.45);

  let cavity = 1.0 - owSmoothstep(0.62, 0.78, h);
  c *= 1.0 - cavity * 0.18;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.85));
  rough = owClamp(r, 0.14, 0.99);
  metal = owClamp(mtl, 0.0, 1.0);
  ao = owClamp(ao, 0.2, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
