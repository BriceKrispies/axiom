
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 6.9);

  // ---- binder: dark, slightly blue-grey, sun-bleached in patches ----
  let macro_ = owFbm01(p * 0.55, P * 0.5, 4, 0.6);
  let mid   = owFbm01(p * 3.0, P * 3.0, 5, 0.5);
  let fine  = owFbm01(p * 16.0, P * 16.0, 4, 0.5);

  let cFresh = owSRGB(vec3<f32>(0.115, 0.115, 0.122));
  let cWorn  = owSRGB(vec3<f32>(0.300, 0.298, 0.295));
  var c = owMix3(cFresh, cWorn, owSmoothstep(0.25, 0.85, macro_) * 0.85);
  // Half the old fine-grain albedo contrast: the stone read belongs in the
  // height/normal channels, not in a high-frequency albedo dither.
  c *= 0.94 + 0.12 * fine;

  h = 0.60 + (mid - 0.5) * 0.06;
  rough = 0.78 + (mid - 0.5) * 0.10 + (fine - 0.5) * 0.14;
  metal = 0.0;
  ao = 1.0;

  // ---- aggregate: dense angular chippings, three grades ----
  // Angularity comes from warping the worley domain: round cells become
  // faceted, which is what separates asphalt from a pebble beach.
  let ap = owWarp(p, P, 0.10, 3);
  let big = owWorley(ap * 12.0, P * 12.0, 1.0);
  let bigM = owSmoothstep(0.40, 0.16, big.x);
  let bigExposed = bigM * owSmoothstep(0.30, 0.62, owFbm01(p * 2.2 + vec2<f32>(3.0), P * 2.0, 4, 0.5) + big.w * 0.5);
  let small = owWorley(ap * 22.0 + vec2<f32>(7.0), P * 22.0, 1.0);
  let smallM = owSmoothstep(0.36, 0.10, small.x);
  let smallExposed = smallM * owStep(0.30, small.w);
  let grit = owWorley(ap * 28.0 + vec2<f32>(3.0), P * 28.0, 1.0);
  let gritM = owSmoothstep(0.32, 0.06, grit.x) * owStep(0.45, grit.z);

  let stoneA = owSRGB(vec3<f32>(0.400, 0.392, 0.378));
  let stoneB = owSRGB(vec3<f32>(0.210, 0.200, 0.192));
  let stoneC = owSRGB(vec3<f32>(0.560, 0.520, 0.470));
  var stone = owMix3(stoneA, stoneB, big.z);
  stone = owMix3(stone, stoneC, owStep(0.90, big.w));

  // Stones are read by their relief and their specular, not by their tint:
  // colour contrast is roughly halved and the height contribution raised.
  c = owMix3(c, stone, bigExposed * 0.52);
  c = owMix3(c, owMix3(stoneA, stoneC, small.z), smallExposed * 0.22);
  c = owMix3(c, owMix3(stoneB, stoneA, grit.z), gritM * 0.14);
  h += bigExposed * 0.15 * (0.6 + 0.6 * big.z) + smallExposed * 0.065 + gritM * 0.022;
  rough += bigExposed * (0.10 - 0.22 * big.z) + smallExposed * (0.06 - 0.14 * small.z);

  // voids between the aggregate — where the binder has ravelled out
  let voidM = owSmoothstep(0.50, 0.85, big.x) * owSmoothstep(0.28, 0.6, small.x);
  h -= voidM * 0.10;
  ao -= voidM * 0.14;

  // ---- tyre polish: two smooth bands where wheels track ----
  let lane = abs(fract(uv.x * 1.0 + 0.25) - 0.5) * 2.0;
  let polish = (1.0 - owSmoothstep(0.10, 0.62, lane)) *
                 owSmoothstep(0.25, 0.65, owFbm01(vec2<f32>(p.x * 0.7, p.y * 5.0), vec2<f32>(P.x, P.y * 5.0), 4, 0.5));
  rough -= polish * 0.16;
  h -= polish * 0.012;
  c = owMix3(c, c * 0.78 + owSRGB(vec3<f32>(0.045, 0.045, 0.048)), polish * 0.45);

  // ---- patch repairs: darker rectangles-ish with a seam ----
  let rep = owWorley(owWarp(p * 0.5 + vec2<f32>(13.0), P * 0.5, 1.6, 3), P * 0.5, 0.9);
  let inPatch = owStep(0.72, rep.w);
  let patchEdge = (1.0 - owSmoothstep(0.0, 0.06, rep.y - rep.x)) * inPatch;
  c = owMix3(c, cFresh * (0.85 + 0.35 * fine), inPatch * 0.20);
  rough = owMix(rough, 0.84, inPatch * 0.22);
  h -= patchEdge * 0.07;
  ao -= patchEdge * 0.20;
  c = owMix3(c, cFresh * 0.5, patchEdge * 0.35);
  // tar bleeding out of the seam, glossy
  let tar = patchEdge * owSmoothstep(0.4, 0.7, owFbm01(p * 6.0, P * 6.0, 3, 0.5));
  rough -= tar * 0.35;
  c = owMix3(c, owSRGB(vec3<f32>(0.055, 0.055, 0.058)), tar * 0.7);

  // ---- alligator cracking + long thermal cracks ----
  let gator = owCracks(p * 3.4, P * 3.4, 0.9, 0.032, 0.56);
  let thermal = owCracks(p * 0.9 + vec2<f32>(41.0), P * 0.9, 0.75, 0.05, 0.70);
  let crack = owClamp(gator + thermal, 0.0, 1.0);
  h -= crack * 0.16;
  ao -= crack * 0.30;
  c = owMix3(c, owSRGB(vec3<f32>(0.045, 0.043, 0.042)), crack * 0.85);
  rough += crack * 0.12;

  // ---- oil stains, dark and slightly glossy ----
  let oil = owSmoothstep(0.68, 0.90, owFbm01(owWarp(p * 1.8 + vec2<f32>(31.0), P * 1.8, 0.9, 3), P * 1.8, 4, 0.55));
  c = owMix3(c, owSRGB(vec3<f32>(0.045, 0.043, 0.046)), oil * 0.6);
  rough -= oil * 0.16;

  // ---- dust settled in the low spots ----
  let dust = owSmoothstep(0.55, 0.30, h) * owSmoothstep(0.35, 0.75, macro_);
  c = owMix3(c, owSRGB(vec3<f32>(0.420, 0.390, 0.340)), dust * 0.35);
  rough += dust * 0.10;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.75));
  rough = owClamp(rough, 0.44, 0.99);
  // see the AO note in GRAVEL: on a ground plane this channel is the shading
  ao = owClamp(ao, 0.68, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
