//! WGSL transcription of Claude-of-Duty `src/materials/glsl/surfaces-organic.js`.
//!
//! Source header (`surfaces-organic.js:1-6`):
//!
//! > Wood, fabric, sandbag/burlap, foliage, rubber, glass.
//! > Foliage writes its cutout mask into the height channel's companion — see
//! > generator.js, which routes `h` to albedo.a for parallax on most surfaces but
//! > to the alpha-test mask for `foliage`.
//!
//! Each constant holds the WGSL body of that block's `owSurface`. GLSL `out`
//! parameters become `ptr<function, …>`; a prologue copies each one into a local
//! `var` of the same name and an epilogue writes them back, so every statement
//! between them is the source line-for-line (this also reproduces `metal` being
//! both a local and an out-param, exactly as GLSL has it).

/// `WOOD` (`surfaces-organic.js:8-120`). Planked, weathered timber: staggered
/// butt joints, warped grain rings with knots, splits, saw marks, nails with a
/// rust weep, and ground-in soil.
pub const WOOD: &str = r#"
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
"#;

/// `FABRIC` (`surfaces-organic.js:122-199`). Plain-weave cloth tinted by
/// `uTintA`/`uTintB`: warp-over-weft cells, fuzz and slubs, a drape-fold field,
/// threadbare wear, pulled threads, stains and dust.
pub const FABRIC: &str = r#"
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
"#;

/// `BURLAP` (`surfaces-organic.js:201-257`). Coarse hessian sacking: per-thread
/// irregular thickness, jute/pale/soil colouring, sun rot, loose standing
/// fibres, and spilled sand caught in the weave.
pub const BURLAP: &str = r#"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let THREADS = 34.0;      // hessian is coarse
  let p = uv * P + vec2<f32>(U.seed * 4.7);

  let t = uv * THREADS;
  let cell = floor(t);
  let f = fract(t) - vec2<f32>(0.5);
  let over = owMod(cell.x + cell.y, 2.0);

  // hessian threads are irregular: each one has its own thickness
  let twx = 0.62 + 0.30 * owHash12(vec2<f32>(cell.x, 0.0) + vec2<f32>(U.seed));
  let twy = 0.62 + 0.30 * owHash12(vec2<f32>(0.0, cell.y) + vec2<f32>(U.seed * 1.7));
  let warpP = cos(owClamp(f.x / twx, -0.5, 0.5) * 3.14159);
  let weftP = cos(owClamp(f.y / twy, -0.5, 0.5) * 3.14159);
  let top = owMix(warpP, weftP, over);
  let bot = owMix(weftP, warpP, over) * 0.40;
  let weave = max(top, bot);

  let fibre = owFbm01(owShear(p * 12.0, 0.0, 8.0), owShearPer(P * 12.0, 8.0), 3, 0.5);
  let macroF = owFbm01(p * 1.0, P * 1.0, 4, 0.62);   // GLSL name was `macro` (reserved in WGSL)
  let dirt  = owFbm01(owWarp(p * 2.5, P * 2.5, 0.8, 3), P * 2.5, 5, 0.55);

  let cJute = owSRGB(vec3<f32>(0.520, 0.430, 0.275));
  let cPale = owSRGB(vec3<f32>(0.640, 0.560, 0.400));
  let cSoil = owSRGB(vec3<f32>(0.230, 0.180, 0.120));
  var c = owMix3(cJute, cPale, owHash12(cell + vec2<f32>(3.0)) * 0.5 + fibre * 0.15);
  c *= 0.855 + 0.235 * (weave * 0.5 + 0.5);
  c *= 0.90 + 0.18 * macroF;
  c = owMix3(c, cSoil, owSmoothstep(0.42, 0.85, dirt) * 0.60);

  h = 0.50 + weave * 0.38 + (fibre - 0.5) * 0.05;
  rough = 0.90 + (1.0 - weave) * 0.06;
  metal = 0.0;
  ao = owMix(0.74, 1.0, owSmoothstep(-0.4, 0.9, weave));

  // sun rot: bleached and frayed on the exposed side
  let rot = owSmoothstep(0.55, 0.9, owFbm01(p * 0.7 + vec2<f32>(11.0), P * 0.7, 3, 0.6));
  c = owMix3(c, cPale * 1.15, rot * 0.4);
  rough += rot * 0.05;

  // loose fibres standing off the surface
  let loose = owScratches(p * 4.0, P * 4.0, 10.0, 2.0, 0.70);
  h += loose * 0.06;
  c = owMix3(c, cPale, loose * 0.3);

  // spilled sand caught in the weave
  let sand = owSmoothstep(0.5, 0.85, owFbm01(p * 12.0, P * 12.0, 4, 0.5)) * (1.0 - owSmoothstep(0.2, 0.7, weave));
  c = owMix3(c, owSRGB(vec3<f32>(0.640, 0.545, 0.390)), sand * 0.45);

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.80));
  rough = owClamp(rough, 0.6, 0.99);
  ao = owClamp(ao, 0.2, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
"#;

/// `FOLIAGE` (`surfaces-organic.js:259-328`). One serrated elliptical leaf per
/// cell, sampled over the 3x3 neighbourhood so leaves overlap; the nearest
/// (highest-`depth`) leaf wins. As the file header notes, `h` here doubles as
/// the alpha-test cutout mask rather than a height (see `generator.js`).
pub const FOLIAGE: &str = r#"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let CELLS = 5.0;
  let p = uv * P + vec2<f32>(U.seed * 5.9);

  // Each cell holds one leaf, rotated and scaled by its hash. Sampling the
  // 3x3 neighbourhood lets leaves overlap into their neighbours' cells.
  let lp = uv * CELLS;
  let ip = floor(lp);
  let fp = fract(lp);

  var bestCover = 0.0;
  var bestDepth = -1.0;
  var bestCol = vec3<f32>(0.0);
  var bestH = 0.0;
  var bestVein = 0.0;

  for (var y: i32 = -1; y <= 1; y = y + 1){
    for (var x: i32 = -1; x <= 1; x = x + 1){
      let g = vec2<f32>(f32(x), f32(y));
      let cell = owMod2(ip + g, vec2<f32>(CELLS));
      let r = owHash42(cell + vec2<f32>(U.seed * 2.0));
      let r2 = owHash42(cell * 1.7 + vec2<f32>(9.0) + vec2<f32>(U.seed));
      let centre = g + vec2<f32>(0.15) + r.xy * 0.7 - fp;
      let ang = r.z * 6.28318;
      let q = owRot(centre, ang);
      // leaf shape: an ellipse pinched at both ends
      let s = vec2<f32>(0.30 + r.w * 0.16, 0.13 + r2.x * 0.07);
      let e = q / s;
      let d = length(e);
      let pinch = 1.0 - 0.55 * abs(e.x) * 0.5;
      // DEAD IN SOURCE: this `cover` is overwritten by the serrated form below
      // before any read.
      var cover = owSmoothstep(1.02, 0.86, d / max(pinch, 0.3));
      // serrated edge
      let serr = sin(atan2(e.y, e.x) * 26.0) * 0.03;
      cover = owSmoothstep(1.02 + serr, 0.88 + serr, d / max(pinch, 0.3));
      if (cover > 0.01){
        let depth = r2.y;
        if (depth > bestDepth){
          var vein = 1.0 - owSmoothstep(0.0, 0.05, abs(e.y * s.y));
          let sideV = owSmoothstep(0.75, 1.0, abs(fract(e.x * 5.0 + e.y * 2.0) * 2.0 - 1.0));
          vein = owClamp(vein + sideV * 0.45 * cover, 0.0, 1.0);
          let cYoung = owSRGB(vec3<f32>(0.180, 0.330, 0.090));
          let cOld   = owSRGB(vec3<f32>(0.095, 0.185, 0.060));
          let cDry   = owSRGB(vec3<f32>(0.390, 0.320, 0.110));
          var lc = owMix3(cYoung, cOld, r2.z);
          lc = owMix3(lc, cDry, owSmoothstep(0.55, 1.0, r2.w) * 0.8);
          // blotches and mildew spots
          let spots = owFbm01(p * 22.0, P * 22.0, 3, 0.5);
          lc *= 0.85 + 0.30 * spots;
          lc = owMix3(lc, cDry * 0.7, owSmoothstep(0.78, 0.95, spots) * 0.5);
          lc = owMix3(lc, lc * 1.35, vein * 0.5);
          bestDepth = depth;
          bestCover = cover;
          bestCol = lc;
          // DEAD IN SOURCE: `bestH` is accumulated but never read after the loop.
          bestH = 0.45 + depth * 0.35 + (1.0 - owSmoothstep(0.0, 1.0, d)) * 0.12 + vein * 0.05;
          bestVein = vein;
        }
      }
    }
  }

  let fine = owFbm01(p * 12.0, P * 12.0, 3, 0.5);
  alb = owClamp3(bestCol * (0.955 + 0.085 * fine), vec3<f32>(0.02), vec3<f32>(0.7));
  // h doubles as the cutout mask for foliage (see generator.js)
  h = bestCover;
  rough = owClamp(0.62 + (1.0 - bestVein) * 0.14 + (fine - 0.5) * 0.10, 0.35, 0.95);
  metal = 0.0;
  ao = owClamp(0.55 + bestDepth * 0.45, 0.3, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
"#;

/// `RUBBER` (`surfaces-organic.js:330-380`). Moulded pebble-grain rubber: a
/// Worley pebble field, a mould seam, chalky abrasion scuffs, ozone cracking,
/// and settled dust.
pub const RUBBER: &str = r#"
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
"#;

/// `GLASS` (`surfaces-organic.js:382-415`). Near-black albedo whose look comes
/// from the roughness channel: wiped smear, dust film, water spots, and fine
/// scratches.
pub const GLASS: &str = r#"
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
"#;
