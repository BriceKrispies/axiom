
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 5.3);

  // trowel: broad sweeps, anisotropic, with a fine skim on top
  let sw = owShear(p * 1.5, 1.0, 3.0);
  let trowel = owFbm01(sw, owShearPer(P * 1.5, 3.0), 5, 0.55);
  let skim   = owFbm01(p * 12.0, P * 12.0, 5, 0.5);
  let micro  = owFbm01(p * 24.0, P * 24.0, 3, 0.5);
  let macro_ = owFbm01(p * 0.6, P * 0.6, 3, 0.6);   // GLSL name: macro (WGSL reserved word)

  let cBase = owSRGB(vec3<f32>(0.598, 0.578, 0.538));
  let cWarm = owSRGB(vec3<f32>(0.512, 0.462, 0.395));
  let cGrey = owSRGB(vec3<f32>(0.382, 0.378, 0.372));
  var c = owMix3(cBase, cWarm, owSmoothstep(0.3, 0.8, macro_));
  c *= 0.94 + 0.12 * skim;
  c = owMix3(c, cGrey, owSmoothstep(0.45, 0.95, trowel) * 0.42);
  c = owMix3(c, cBase * 1.10, owSmoothstep(0.55, 0.15, trowel) * 0.30);

  h = 0.70 + (trowel - 0.5) * 0.10 + (skim - 0.5) * 0.030 + (micro - 0.5) * 0.012;
  rough = 0.80 + (skim - 0.5) * 0.12 - owSmoothstep(0.5, 0.9, trowel) * 0.10;
  ao = 1.0;
  metal = 0.0;

  // ---- skim-coat laps ------------------------------------------------------
  // A plasterer works the wall in ~40 cm passes, and every pass sets a hair
  // lighter or darker than the one before with a faint arris where the trowel
  // lifted off. This is the mid-frequency signal that separates plaster from
  // paint at 2-5 m — without it the wall is one value plus a sprinkle of specks.
  let lapUv = owShear(p * 0.7, 1.0, 1.0);
  let lapF = lapUv.y + owFbm01(p * 1.1, P * 1.1, 3, 0.6) * 1.4;
  let lapI = floor(lapF);
  let lapT = fract(lapF);
  let lapR = owHash11(lapI * 1.71 + U.seed * 2.3);
  c *= 0.885 + 0.240 * lapR;
  rough += (lapR - 0.5) * 0.10;

  //
  // THE 0.1-1 m BAND. A wall seen from 2-3 m fills the frame with about half a
  // metre of itself, which is a hole in the frequency budget: the macro layer
  // varies over 4-12 m and the detail map over 10 mm, so between them the
  // surface has nothing and measures a standard deviation of 5 over a
  // 260x240 patch — a flat colour with a sprinkle of specks. These three
  // bands (damp bloom, hand-height soiling, and a soft dirt wash) sit at
  // 15-90 cm and are what actually makes a plastered wall read as plaster.
  //
  // NB the contrast expansion. A 4-octave fbm01 spans about 0.3-0.7, never
  // 0-1, so writing 0.86 + 0.30 * n gives a +/-6% wash and not the +/-20%
  // the numbers suggest — the same trap the macro layer documents. Every band
  // here is re-centred and expanded before it is used.
  var dampB = owFbm01(owWarp(p * 1.6 + vec2<f32>(3.7), P * 1.6, 0.7, 3), P * 1.6, 4, 0.58);
  dampB = owClamp((dampB - 0.5) * 2.6 + 0.5, 0.0, 1.0);
  c *= 0.80 + 0.42 * dampB;
  rough += (dampB - 0.5) * 0.12;
  var soil2 = owFbm01(owWarp(p * 3.4 + vec2<f32>(21.0), P * 3.4, 0.55, 3), P * 3.4, 4, 0.55);
  soil2 = owClamp((soil2 - 0.5) * 2.4 + 0.5, 0.0, 1.0);
  c *= 0.875 + 0.26 * soil2;
  var wash = owFbm01(p * 8.0 + vec2<f32>(6.0), P * 8.0, 4, 0.5);
  wash = owClamp((wash - 0.5) * 2.2 + 0.5, 0.0, 1.0);
  c *= 0.925 + 0.155 * wash;
  let lapEdge = (1.0 - owSmoothstep(0.0, 0.05, lapT)) * (0.35 + 0.65 * lapR);
  h += lapEdge * 0.022 - (lapR - 0.5) * 0.014;
  c *= 1.0 + lapEdge * 0.07;

  // ---- sand tooth: the 0.5-2 mm grain of the finish coat, with a matching
  //      height channel. Without this the wall is paint, not plaster.
  // 6-9 mm float grain. The finer 1-2 mm tooth is the shared detail map's
  // job: at 2.2 m over 1024 texels one texel is 2.1 mm, so anything past
  // K = 22 here is a sub-texel hash that bakes as dither and mips to grey.
  let tooth = owWorley(p * 20.0, P * 20.0, 1.0);
  let grain = owSmoothstep(0.46, 0.06, tooth.x);
  let grainSel = 0.40 + 0.60 * owStep(0.32, tooth.z);
  h += grain * grainSel * 0.030;
  ao -= grain * 0.07;
  c *= 1.0 + (grain * grainSel - 0.20) * 0.16;
  rough += (tooth.z - 0.5) * 0.11 + grain * 0.05;
  // dust and shadow sit in the troughs between grains
  let trough = owSmoothstep(0.52, 0.86, tooth.x);
  c = owMix3(c, c * 0.84, trough * 0.40);

  // pinholes from the float
  let ph = owWorley(p * 22.0, P * 22.0, 1.0);
  let hole = owSmoothstep(0.24, 0.0, ph.x) * owStep(0.80, ph.w);
  h -= hole * 0.06;
  ao -= hole * 0.4;

  // hairline crazing — a fine, wide-spread net
  var hair = owCracks(p * 9.0, P * 9.0, 0.9, 0.016, 0.52);
  hair += owCracks(p * 16.0 + vec2<f32>(6.0), P * 16.0, 0.95, 0.015, 0.62) * 0.5;
  hair = owClamp(hair, 0.0, 1.0);
  h -= hair * 0.030;
  ao -= hair * 0.18;
  c = owMix3(c, c * 0.80, hair * 0.45);

  // structural cracks — few, wide, branching
  let crack = owCracks(p * 4.5 + vec2<f32>(17.0), P * 4.5, 0.8, 0.018, 0.62);
  h -= crack * 0.16;
  ao -= crack * 0.6;
  c = owMix3(c, owSRGB(vec3<f32>(0.300, 0.278, 0.250)), crack * 0.8);

  // blown plaster: patches spalled off, revealing render/brick beneath
  let blowMask = owFbm01(owWarp(p * 1.05 + vec2<f32>(9.0), P * 1.05, 1.1, 3), P * 1.05, 4, 0.55);
  let blow = owSmoothstep(0.775, 0.845, blowMask);
  let blowEdge = owSmoothstep(0.745, 0.790, blowMask) - blow;
  var substrate = owMix3(owSRGB(vec3<f32>(0.360, 0.245, 0.195)), owSRGB(vec3<f32>(0.430, 0.400, 0.360)),
                       owFbm01(p * 9.0, P * 9.0, 4, 0.5));
  substrate *= 0.85 + 0.3 * owFbm01(p * 20.0, P * 20.0, 3, 0.5);
  c = owMix3(c, substrate, blow * 0.85);
  h -= blow * 0.13;
  ao -= blow * 0.26;
  rough += blow * 0.10;
  // the lip of the blown patch is bright and sharp
  c += vec3<f32>(blowEdge * 0.06);
  h += blowEdge * 0.02;

  // ---- chipped patches: 6-9 cm flakes knocked off the skim, showing the darker
  //      browncoat. Deliberately FEWER and LARGER than a fine speckle: a dense
  //      sprinkle of 3 cm dark dots on a facade reads as fly dirt, not as damage,
  //      and it is the one thing that survives at every distance and so gives the
  //      whole wall a screen-space texture.
  let ck = owWorley(owWarp(p * 4.2 + vec2<f32>(13.0), P * 4.2, 0.6, 3), P * 4.2, 0.95);
  let ckSel = owStep(0.930, ck.w);
  let ckSize = 0.22 + 0.20 * ck.z;
  let ckShape = owSmoothstep(ckSize, ckSize * 0.3,
                             ck.x * (0.70 + 0.60 * owFbm01(p * 16.0, P * 16.0, 3, 0.5)));
  let chip = ckSel * ckShape;
  // The browncoat is the same family as the finish, just darker and coarser —
  // a chip is a shallow flake, not a hole punched in the wall.
  var coat = owMix3(c, owSRGB(vec3<f32>(0.392, 0.336, 0.284)), 0.52);
  coat *= 0.90 + 0.20 * owFbm01(p * 18.0, P * 18.0, 3, 0.5);
  c = owMix3(c, coat, chip * 0.58);
  h -= chip * 0.05;
  ao -= chip * 0.26;
  rough += chip * 0.09;
  let ckLip = max(ckSel * (owSmoothstep(ckSize * 1.25, ckSize, ck.x) - ckShape), 0.0);
  c *= 1.0 + ckLip * 0.10;
  h += ckLip * 0.010;

  // water staining: tide marks and slow brown bleed
  let stain = owFbm01(vec2<f32>(p.x * 1.6, p.y * 3.2), vec2<f32>(P.x * 1.6, P.y * 3.0), 5, 0.6);
  let tide = owSmoothstep(0.60, 0.78, stain) * (1.0 - owSmoothstep(0.78, 0.94, stain));
  c = owMix3(c, owSRGB(vec3<f32>(0.400, 0.330, 0.245)), tide * 0.45);
  c *= 1.0 - owSmoothstep(0.50, 0.95, stain) * 0.34;
  rough += tide * 0.05;

  // black mould in the damp corners
  let mould = owSmoothstep(0.72, 0.95, owFbm01(p * 4.0 + vec2<f32>(25.0), P * 4.0, 5, 0.6)) *
                owSmoothstep(0.45, 0.8, stain);
  c = owMix3(c, owSRGB(vec3<f32>(0.085, 0.090, 0.080)), mould * 0.7);
  rough += mould * 0.08;

  // grime in recesses
  let cavity = 1.0 - owSmoothstep(0.48, 0.72, h);
  c = owMix3(c, owSRGB(vec3<f32>(0.22, 0.21, 0.19)), cavity * 0.30);

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.88));
  rough = owClamp(rough, 0.35, 0.99);
  ao = owClamp(ao, 0.15, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
