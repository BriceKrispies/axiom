
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let PLANKS = 5.0;
  let p = uv * P + vec2<f32>(U.seed * 12.9);

  // ---- plank layout: rows running along X, staggered butt joints ----
  let rowF = uv.y * PLANKS;
  let row = floor(rowF);
  let rf = fract(rowF);
  let stagger = owHash11(row + U.seed * 2.0);
  let lenF = uv.x * 2.0 + stagger;             // 2 boards per row lengthwise
  let board = floor(lenF);
  let lf = fract(lenF);
  let rnd = owHash42(vec2<f32>(board, row) + vec2<f32>(U.seed));

  // gaps between boards
  let GY = 0.035;
  let GX = 0.010;
  let ey = min(owSmoothstep(0.0, GY, rf), owSmoothstep(0.0, GY, 1.0 - rf));
  let ex = min(owSmoothstep(0.0, GX, lf), owSmoothstep(0.0, GX, 1.0 - lf));
  let face = min(ex, ey);

  // ---- grain: rings stretched along the board, warped, with knots ----
  let gp = vec2<f32>(lf * 2.0 + rnd.x * 13.0, rf + rnd.y * 7.0);
  let GP = vec2<f32>(16.0, 8.0);
  let warp = owFbm(vec2<f32>(gp.x * 3.0, gp.y * 12.0), vec2<f32>(GP.x * 3.0, GP.y * 12.0), 4, 0.55);
  var ringCoord = gp.y * (14.0 + rnd.z * 12.0) + warp * 2.2 + rnd.w * 5.0;

  // knots pull the rings into a tight radial swirl
  let knotP = vec2<f32>(0.25 + rnd.x * 0.5, 0.35 + rnd.y * 0.3);
  let kd = length((vec2<f32>(lf, rf) - knotP) * vec2<f32>(2.2, 1.0));
  let hasKnot = owStep(0.68, rnd.z);
  let knotPull = hasKnot * exp(-kd * 9.0);
  ringCoord = owMix(ringCoord, kd * 42.0, owClamp(knotPull * 1.6, 0.0, 1.0));

  let rings = fract(ringCoord);
  let ringDark = owSmoothstep(0.42, 0.5, rings) * (1.0 - owSmoothstep(0.5, 0.62, rings));
  let latewood = owSmoothstep(0.30, 0.52, rings);

  // fine fibre along the grain
  let fibre = owFbm01(owShear(p * 6.0, 0.0, 40.0), owShearPer(P * 6.0, 40.0), 4, 0.5);
  let micro = owFbm01(p * 22.0, P * 22.0, 3, 0.5);

  // ---- colour ----
  let wLight = owSRGB(vec3<f32>(0.505, 0.408, 0.290));
  let wMid   = owSRGB(vec3<f32>(0.362, 0.272, 0.180));
  let wDark  = owSRGB(vec3<f32>(0.205, 0.142, 0.092));
  let wGrey  = owSRGB(vec3<f32>(0.372, 0.355, 0.328));   // weathered silver-grey
  var c = owMix3(wLight, wMid, rnd.w * 0.8 + latewood * 0.5);
  c = owMix3(c, wDark, ringDark * 0.65);
  c *= 0.90 + 0.18 * fibre;
  c = owMix3(c, wDark * 0.7, owClamp(knotPull * 2.2, 0.0, 1.0) * 0.8);

  // weathering: UV-bleached, silvered, worst on the exposed boards
  let weather = owSmoothstep(0.20, 0.85, owFbm01(p * 0.8, P * 0.8, 3, 0.6)) * (0.4 + 0.6 * rnd.x);
  c = owMix3(c, wGrey, weather * 0.68);

  var faceH = 0.74 - ringDark * 0.02 - latewood * 0.012 + (fibre - 0.5) * 0.03 + (micro - 0.5) * 0.008;
  faceH += (rnd.y - 0.5) * 0.035;              // boards cup and sit at different heights
  faceH -= owClamp(knotPull * 1.5, 0.0, 1.0) * 0.03;

  // splits and checks running along the grain
  let split = owScratches(vec2<f32>(p.x, p.y) * 2.0, P * 2.0, 30.0, 0.0, 0.66) * weather;
  faceH -= split * 0.10;
  c = owMix3(c, wDark * 0.45, split * 0.7);

  // saw marks across the board
  let saw = owFbm01(owShear(p * 3.0, 0.0, 1.0) * vec2<f32>(30.0, 1.0), vec2<f32>(P.x * 90.0, P.y * 3.0), 3, 0.5);
  faceH += (saw - 0.5) * 0.012;

  // rounded / bashed board edges
  let edgeD = min(min(rf, 1.0 - rf) / GY, min(lf, 1.0 - lf) / GX);
  let bevel = 1.0 - owSmoothstep(0.0, 2.4, edgeD);
  faceH -= bevel * 0.035;
  c *= 1.0 - bevel * 0.10;
  c = owMix3(c, wLight * 1.15, bevel * owSmoothstep(0.5, 0.9, owFbm01(p * 20.0, P * 20.0, 3, 0.5)) * 0.35);

  // ---- gap between boards: dark, deep ----
  let m = owSmoothstep(0.05, 0.7, face);
  h = owMix(0.44, faceH, m);
  c = owMix3(wDark * 0.25, c, m);
  rough = owMix(0.95, 0.62 + 0.22 * fibre + weather * 0.20 + split * 0.15, m);
  ao = owMix(0.25, 1.0, owSmoothstep(0.0, 0.5, face)) - bevel * 0.12 * m;
  metal = 0.0;

  // ---- nails ----
  let nf = vec2<f32>(fract(lf * 3.0 + 0.5) - 0.5, (rf - 0.5));
  // DEAD IN SOURCE: this `nd` is overwritten by the next line before any read.
  var nd = length(nf * vec2<f32>(3.0, 1.0) / vec2<f32>(3.0, 1.0) * vec2<f32>(1.0, 1.0));
  nd = length(vec2<f32>(fract(lf * 3.0 + 0.5) - 0.5, rf - 0.22) * vec2<f32>(1.4, 1.0));
  let nail = owSmoothstep(0.055, 0.030, nd) * m * owStep(0.3, rnd.w);
  h -= nail * 0.02;
  c = owMix3(c, owSRGB(vec3<f32>(0.230, 0.200, 0.170)), nail * 0.85);
  rough = owMix(rough, 0.55, nail);
  metal = owMix(metal, 0.85, nail * 0.7);
  ao -= nail * 0.25;
  // rust weep under the nail
  let weep = owSmoothstep(0.11, 0.05, nd) * owStep(0.3, rnd.w) * owSmoothstep(0.0, 0.6, rf - 0.22) * m;
  c = owMix3(c, owSRGB(vec3<f32>(0.330, 0.185, 0.095)), owClamp(weep, 0.0, 1.0) * 0.4);

  // grime
  let cavity = 1.0 - owSmoothstep(0.55, 0.78, h);
  c = owMix3(c, owSRGB(vec3<f32>(0.120, 0.106, 0.088)), cavity * 0.45);
  // ground-in dirt over the whole board
  let soil = owSmoothstep(0.40, 0.88, owFbm01(owWarp(p * 2.2 + vec2<f32>(5.0), P * 2.2, 0.9, 3), P * 2.2, 5, 0.6));
  c = owMix3(c, owSRGB(vec3<f32>(0.185, 0.160, 0.128)), soil * 0.40);
  rough += soil * 0.08;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.80));
  rough = owClamp(rough, 0.25, 0.99);
  ao = owClamp(ao, 0.12, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
