
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let COLS = 6.0;     // bricks across the tile
  let ROWS = 18.0;    // courses up the tile
  let p = uv * P + vec2<f32>(U.seed * 9.1);

  // ---------------- brick lattice, running bond ----------------
  let rowF = uv.y * ROWS;
  let row = floor(rowF);
  let colF = uv.x * COLS + owMod(row, 2.0) * 0.5;
  let col = floor(colF);
  let id = vec2<f32>(owMod(col, COLS), row);
  let f = vec2<f32>(fract(colF), fract(rowF));

  let rnd = owHash42(id + vec2<f32>(U.seed * 3.0));
  let rnd2 = owHash42(id * 1.37 + vec2<f32>(21.0) + vec2<f32>(U.seed));
  let rnd3 = owHash42(id * 0.73 + vec2<f32>(7.7) + vec2<f32>(U.seed * 1.9));

  // Bricks are laid by hand: each one is a hair off square.
  let jitter = (rnd.xy - vec2<f32>(0.5)) * vec2<f32>(0.012, 0.030);
  let fj = f + jitter;

  // joint thickness (10mm of a 225mm x 75mm course). The joint is *raked*: a
  // flat mortar bed with a hard arris at the brick edge. Ramping across the
  // whole joint width is what makes mortar read as a painted line.
  let JX = 0.048;
  let JY = 0.135;
  let dxj = min(fj.x, 1.0 - fj.x);
  let dyj = min(fj.y, 1.0 - fj.y);
  let shoulder = 0.74 + 0.16 * rnd3.w;    // some joints struck flush, some sharp
  let ex = owSmoothstep(JX * shoulder, JX * 1.02, dxj);
  let ey = owSmoothstep(JY * shoulder, JY * 1.02, dyj);
  let face = min(ex, ey);                 // 1 = brick face, 0 = mortar

  // per-brick surface coords so the face texture never repeats
  let bp = vec2<f32>(fj.x, fj.y) * vec2<f32>(3.0, 1.0) + rnd.zw * 17.0;
  let BP = vec2<f32>(24.0);

  // ---------------- mortar ----------------
  let mSand = owFbm01(p * 20.0, P * 20.0, 4, 0.5);
  let mGrain = owWorley(p * 24.0, P * 24.0, 1.0);
  let mortarRough = owFbm01(p * 20.0, P * 20.0, 4, 0.55);
  var mortarCol = owMix3(owSRGB(vec3<f32>(0.400, 0.388, 0.362)), owSRGB(vec3<f32>(0.278, 0.272, 0.260)),
                       owSmoothstep(0.3, 0.8, mortarRough));
  mortarCol *= 0.84 + 0.32 * mSand;
  mortarCol *= 0.88 + 0.24 * owFbm01(p * 6.0, P * 6.0, 4, 0.6);
  mortarCol = owMix3(mortarCol, owSRGB(vec3<f32>(0.235, 0.228, 0.215)), owSmoothstep(0.5, 0.06, mGrain.x) * 0.40);
  mortarCol = owMix3(mortarCol, owSRGB(vec3<f32>(0.520, 0.505, 0.470)), owSmoothstep(0.30, 0.02, owWorley(p * 25.0 + vec2<f32>(4.0), P * 25.0, 1.0).x) * 0.35);

  // some joints are struck flush, some are raked deep, some crumbled out.
  // 0.10-0.15 of a 0.055 m relief = 5-8 mm of real recess.
  var jointDepth = 0.10 + 0.05 * owFbm01(p * 1.2, P * 1.2, 3, 0.5);
  let crumble = owSmoothstep(0.62, 0.86, owFbm01(p * 9.0 + vec2<f32>(4.0), P * 9.0, 4, 0.5));
  jointDepth += crumble * 0.09;
  // the mortar bed itself is not flat — it holds the trowel's sand texture
  let mortarH = -(mSand - 0.5) * 0.018 - owSmoothstep(0.5, 0.0, mGrain.x) * 0.012;

  // ---------------- brick face ----------------
  let faceN = owFbm01(bp * 2.2, BP, 5, 0.5);
  let faceFine = owFbm01(bp * 5.0, BP * 2.0, 4, 0.5);
  let facePore = owWorley(bp * 7.0, BP * 3.5, 1.0);
  // Pits cluster instead of forming an even dot grid, and their size varies.
  let poreCluster = owSmoothstep(0.42, 0.78, owFbm01(bp * 3.0 + vec2<f32>(8.0), BP * 1.5, 4, 0.55));
  let pore = owSmoothstep(0.26 + 0.16 * facePore.z, 0.0, facePore.x) * owStep(0.55, facePore.w) * poreCluster;

  // Colour families: red stock, dark burnt header, pale sand-lime, brown.
  let cA = owSRGB(vec3<f32>(0.430, 0.238, 0.183));   // red stock
  let cB = owSRGB(vec3<f32>(0.318, 0.183, 0.150));   // deep red
  let cC = owSRGB(vec3<f32>(0.196, 0.132, 0.120));   // burnt header
  let cD = owSRGB(vec3<f32>(0.492, 0.392, 0.300));   // sandy
  let cE = owSRGB(vec3<f32>(0.372, 0.288, 0.218));   // brown

  var brick = owMix3(cA, cB, rnd.z);
  brick = owMix3(brick, cC, owStep(0.90, rnd.w) * 0.70);
  brick = owMix3(brick, cD, owStep(0.94, rnd2.x) * 0.62);
  brick = owMix3(brick, cE, owStep(0.55, rnd2.y) * 0.50);
  // every brick came out of the kiln a different shade: +/-12% per brick
  brick *= 0.88 + 0.24 * rnd3.x;
  // within-brick banding from the extrusion
  brick *= 0.86 + 0.28 * faceN;
  // fine sand grain across the face — this is what reads at 0.5 m
  // bp is per-brick, and a brick is only ~170 texels wide, so bp*26 was 78
  // cycles across it — 2.2 texels a cycle. This is the band that has to still
  // be there at 0.5 m, so it is authored at 7 texels and given more contrast.
  let faceGrain = owFbm01(bp * 8.0, BP * 4.0, 4, 0.55);
  brick *= 0.87 + 0.26 * faceGrain;
  brick = owMix3(brick, brick * 1.22, owSmoothstep(0.55, 0.9, faceFine) * 0.5);
  // dark iron spots and sand inclusions
  brick = owMix3(brick, brick * 0.62, pore * 0.85);
  brick = owMix3(brick, brick * 0.72, owSmoothstep(0.34, 0.0, facePore.x) * owStep(0.86, facePore.z));
  brick = owMix3(brick, owSRGB(vec3<f32>(0.62, 0.58, 0.50)), owSmoothstep(0.86, 0.98, faceFine) * 0.35);

  var faceH = 0.72 + (faceN - 0.5) * 0.05 + (faceFine - 0.5) * 0.025
              + (rnd2.z - 0.5) * 0.05;               // each brick sits proud/shy
  faceH -= pore * 0.075;

  // Broken arrises: ~5% of the edge length is knocked off, deep enough to
  // catch a shadow, showing pale raw clay under the fired skin.
  let edgeD = min(dxj / JX, dyj / JY);
  let chipNoise = owFbm01(bp * 6.0 + vec2<f32>(3.0), BP * 3.0, 4, 0.5);
  let chip = owSmoothstep(1.7, 0.30, edgeD) * owSmoothstep(0.60, 0.80, chipNoise) * owStep(0.66, rnd3.z);
  faceH -= chip * 0.17;
  brick = owMix3(brick, brick * 0.72 + owSRGB(vec3<f32>(0.20, 0.13, 0.09)), chip * 0.65);

  // ---------------- combine face + mortar ----------------
  // face is already a shaped profile, so no second smoothstep here: that is
  // what used to smear the arris across the full joint width.
  let m = face;
  h = owMix(0.72 - jointDepth + mortarH, faceH, m);
  var c = owMix3(mortarCol, brick, m);
  // every brick came out of the kiln with a slightly different skin
  let brickRough = 0.58 + 0.32 * rnd2.z + (rnd3.y - 0.5) * 0.20;
  rough = owMix(0.88 + 0.10 * mSand + 0.06 * (mortarRough - 0.5),
              brickRough + 0.14 * faceN + 0.10 * (faceGrain - 0.5) + chip * 0.14, m);
  ao = owMix(0.34, 1.0, owSmoothstep(0.0, 0.75, face));
  ao -= chip * 0.30;
  metal = 0.0;

  // mortar smeared over the brick edge by the trowel
  let smear = owSmoothstep(0.5, 1.0, 1.0 - face) * owSmoothstep(0.55, 0.9, owFbm01(p * 14.0, P * 14.0, 4, 0.5));
  c = owMix3(c, mortarCol * 1.05, smear * 0.5);

  // ---------------- weathering over the whole wall ----------------
  // The 0.1-1 m band — see the long note in PLASTER.
  var soilB = owFbm01(owWarp(p * 1.8 + vec2<f32>(27.0), P * 1.8, 0.6, 3), P * 1.8, 4, 0.58);
  soilB = owClamp((soilB - 0.5) * 2.5 + 0.5, 0.0, 1.0);
  c *= 0.845 + 0.33 * soilB;

  // efflorescence: salt bloom, strongest around joints
  var efflo = owSmoothstep(0.62, 0.96, owFbm01(owWarp(p * 2.6, P * 2.6, 0.8, 3), P * 2.6, 4, 0.5));
  efflo *= owMix(1.0, 0.35, m);
  c = owMix3(c, owSRGB(vec3<f32>(0.66, 0.652, 0.632)), efflo * 0.5);
  rough += efflo * 0.10;

  // soot / rain runoff — short, shallow and only ~3:1 stretched; the long runs
  // are added at runtime where a real ledge sheds water.
  let streak = owFbm01(vec2<f32>(p.x * 7.0, p.y * 2.3), vec2<f32>(P.x * 7.0, P.y * 2.0), 5, 0.55);
  let runoff = owSmoothstep(0.50, 0.92, streak);
  c *= 1.0 - runoff * 0.16;

  // hairline cracks stepping through the joints
  let crack = owCracks(p * 2.2, P * 2.2, 0.85, 0.038, 0.58);
  h -= crack * 0.10;
  ao -= crack * 0.45;
  c = owMix3(c, c * 0.35, crack * 0.7);

  // dirt in every crevice
  let cavity = 1.0 - owSmoothstep(0.50, 0.74, h);
  c = owMix3(c, owSRGB(vec3<f32>(0.16, 0.15, 0.14)), cavity * 0.32);

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.85));
  rough = owClamp(rough, 0.35, 0.99);
  ao = owClamp(ao, 0.12, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
