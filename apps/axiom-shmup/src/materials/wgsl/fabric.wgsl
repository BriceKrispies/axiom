
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let THREADS = 96.0;
  let p = uv * P + vec2<f32>(U.seed * 3.9);

  // ---- plain weave: warp over weft on alternating cells ----
  let t = uv * THREADS;
  let cell = floor(t);
  let f = fract(t) - vec2<f32>(0.5);
  let over = owMod(cell.x + cell.y, 2.0);   // 0 -> warp on top, 1 -> weft on top

  let warpProfile = cos(f.x * 3.14159) ;
  let weftProfile = cos(f.y * 3.14159);
  let top = owMix(warpProfile, weftProfile, over);
  let bot = owMix(weftProfile, warpProfile, over) * 0.45;
  let weave = max(top, bot);
  let threadId = owHash12(cell + vec2<f32>(U.seed));

  // ---- fuzz and slubs ----
  let fuzz = owFbm01(p * 12.0, P * 12.0, 3, 0.55);
  let slub = owFbm01(p * 14.0, P * 14.0, 4, 0.5);
  let macroF = owFbm01(p * 1.2, P * 1.2, 4, 0.6);   // GLSL name was `macro` (reserved in WGSL)

  let cA = U.tint_a;
  let cB = U.tint_b;
  var c = owMix3(cA, cB, threadId * 0.6 + slub * 0.4);
  c *= 0.865 + 0.215 * (weave * 0.5 + 0.5);
  c *= 0.960 + 0.075 * fuzz;
  c *= 0.90 + 0.20 * macroF;

  h = 0.55 + weave * 0.30 + (fuzz - 0.5) * 0.03 + (slub - 0.5) * 0.05;
  rough = 0.86 + (1.0 - weave) * 0.08 + (fuzz - 0.5) * 0.06;
  metal = 0.0;
  ao = owMix(0.82, 1.0, owSmoothstep(-0.4, 0.9, weave));

  // ---- drape folds ---------------------------------------------------------
  // Cloth under tension gathers into soft parallel ridges roughly a hand's width
  // apart, wandering as they run. At the 0.26 m mapping the awnings use, 2.6
  // cycles across the tile is a ~10 cm fold. A weave alone reads as printed
  // canvas; the fold field is what gives a canopy its shape between its poles.
  let foldC = uv.y * 2.6 + uv.x * 0.55 + owFbm01(p * 0.9, P * 0.9, 3, 0.62) * 2.2;
  let foldT = abs(fract(foldC) - 0.5) * 2.0;          // 0 at crest, 1 in trough
  let crest = 1.0 - foldT;
  let foldR = owHash11(floor(foldC) * 2.13 + U.seed);
  let fold = crest * crest * (0.55 + 0.75 * foldR);
  h += (fold - 0.30) * 0.115;
  c *= 0.895 + 0.21 * fold;
  ao -= (1.0 - crest) * 0.14;
  // the crease line itself is polished by handling and holds the dust
  let creaseLine = 1.0 - owSmoothstep(0.0, 0.10, foldT);
  rough -= creaseLine * 0.06;
  c *= 1.0 + creaseLine * 0.05;

  // ---- wear: threadbare patches, fraying, pulled threads ----
  let wearField = owSmoothstep(0.58, 0.82, owFbm01(owWarp(p * 2.0, P * 2.0, 0.8, 3), P * 2.0, 4, 0.55));
  c = owMix3(c, c * 1.35 + vec3<f32>(0.02), wearField * 0.5);
  rough += wearField * 0.06;
  h -= wearField * 0.05;

  let pulled = owScratches(p * 3.0, P * 3.0, 18.0, 1.0, 0.68);
  h += pulled * 0.05;
  c *= 1.0 - pulled * 0.10;

  // ---- stains and dust ----
  let stain = owSmoothstep(0.55, 0.9, owFbm01(owWarp(p * 1.5 + vec2<f32>(7.0), P * 1.5, 1.0, 3), P * 1.5, 5, 0.6));
  c = owMix3(c, c * 0.42 + owSRGB(vec3<f32>(0.09, 0.08, 0.06)), stain * 0.55);
  rough += stain * 0.05;

  let dust = owSmoothstep(0.4, 0.85, owFbm01(p * 6.0, P * 6.0, 4, 0.5));
  c = owMix3(c, owSRGB(vec3<f32>(0.400, 0.375, 0.335)), dust * 0.14);

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.85));
  rough = owClamp(rough, 0.5, 0.99);
  ao = owClamp(ao, 0.25, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
