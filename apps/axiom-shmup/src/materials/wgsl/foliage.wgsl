
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
