
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 13.7);

  // ---- base tone: pour variation, wet/dry patches, cement bloom ----
  let macro_ = owFbm01(p * 0.5, P * 0.5, 4, 0.58);   // GLSL name: macro (WGSL reserved word)
  let mid   = owFbm01(owWarp(p * 2.0, P * 2.0, 0.7, 3), P * 2.0, 5, 0.5);
  let fine  = owFbm01(p * 18.0, P * 18.0, 4, 0.5);
  let micro = owFbm01(p * 26.0, P * 26.0, 3, 0.5);

  let cLight = owSRGB(vec3<f32>(0.520, 0.512, 0.492));
  let cMid   = owSRGB(vec3<f32>(0.395, 0.392, 0.385));
  let cDark  = owSRGB(vec3<f32>(0.255, 0.253, 0.258));
  var c = owMix3(cMid, cLight, owSmoothstep(0.35, 0.85, macro_));
  c = owMix3(c, cDark, owSmoothstep(0.55, 0.95, mid) * 0.55);
  c *= 0.93 + 0.14 * fine;
  // The 0.1-1 m band — see the long note in PLASTER. Pour blotching and the
  // wash of dirt that runs over any concrete left outdoors.
  // contrast-expanded: see the note in PLASTER
  var pourB = owFbm01(owWarp(p * 1.5 + vec2<f32>(8.3), P * 1.5, 0.6, 3), P * 1.5, 4, 0.58);
  pourB = owClamp((pourB - 0.5) * 2.5 + 0.5, 0.0, 1.0);
  c *= 0.82 + 0.38 * pourB;
  var wash = owFbm01(p * 7.0 + vec2<f32>(2.0), P * 7.0, 4, 0.5);
  wash = owClamp((wash - 0.5) * 2.2 + 0.5, 0.0, 1.0);
  c *= 0.925 + 0.155 * wash;

  h = 0.62 + (fine - 0.5) * 0.035 + (mid - 0.5) * 0.05;
  rough = 0.70 + (mid - 0.5) * 0.16 + (micro - 0.5) * 0.07;
  ao = 1.0;
  metal = 0.0;

  // ---- exposed aggregate: stone chips sitting just under the skin ----
  let agg = owWorley(p * 13.0, P * 13.0, 0.95);
  let aggShape = owSmoothstep(0.46, 0.10, agg.x);
  let aggRnd = agg.z;
  // Only some chips break the surface.
  let aggExposed = aggShape * owStep(0.74, owFbm01(p * 3.0 + vec2<f32>(5.0), P * 3.0, 3, 0.5) + aggRnd * 0.35);
  h += aggExposed * 0.022 * (0.5 + aggRnd);
  c = owMix3(c, owMix3(owSRGB(vec3<f32>(0.335, 0.320, 0.300)), owSRGB(vec3<f32>(0.560, 0.545, 0.505)), aggRnd), aggExposed * 0.7);
  rough += aggExposed * 0.07 * (aggRnd - 0.5);

  // ---- coarse sand fraction: the 5-8 mm grit of the cement skin ----
  // The 0.5-2 mm tooth is NOT authored here. At 2.5 m over a 1024 bake one
  // texel is 2.4 mm, so a 1 mm grain is a sub-texel hash: it bakes as white
  // noise, dithers at mip 0 and is gone by mip 1. That band belongs to the
  // shared detail map, which is tiled ten times finer. What lives here is the
  // grit you can actually resolve, at real amplitude.
  let sand = owWorley(p * 20.0, P * 20.0, 1.0);
  let sandM = owSmoothstep(0.44, 0.05, sand.x);
  let sandSel = 0.40 + 0.60 * owStep(0.30, sand.z);
  h += sandM * sandSel * 0.028;
  c *= 1.0 + (sandM * sandSel - 0.20) * 0.15;
  rough += (sand.z - 0.5) * 0.11 + sandM * 0.04;
  ao -= sandM * 0.06;
  let sandTrough = owSmoothstep(0.52, 0.88, sand.x);
  c = owMix3(c, c * 0.86, sandTrough * 0.34);

  // ---- air pockets / bug holes from the pour ----
  let pores = owWorley(p * 22.0, P * 22.0, 1.0);
  let pore = owSmoothstep(0.26, 0.0, pores.x) * owStep(0.84, pores.w);
  h -= pore * 0.055;
  ao -= pore * 0.55;
  rough += pore * 0.10;

  // uParam.x = board-formed wall (1) vs poured slab (0)
  // uParam.y = saw-cut control joints, for floors
  let formAmt = U.param.x;
  let jointAmt = U.param.y;

  // ---- formwork: horizontal board lines + tie-rod holes ----
  let boards = uv.y * 4.0;
  let bi = floor(boards);
  let bf = fract(boards);
  var seam = (1.0 - owSmoothstep(0.0, 0.030, bf)) + (1.0 - owSmoothstep(0.0, 0.030, 1.0 - bf));
  seam = owClamp(seam, 0.0, 1.0);
  // Boards are never perfectly aligned: each course steps a fraction of a mm.
  let boardStep = (owHash11(bi + U.seed) - 0.5) * 0.028 * formAmt;
  h += boardStep;
  h -= seam * 0.055 * formAmt;
  ao -= seam * 0.40 * formAmt;
  c *= 1.0 - seam * 0.16 * formAmt;
  // cement bled along the seam and set lighter
  let bleed = (1.0 - owSmoothstep(0.0, 0.10, abs(bf - 0.02))) * 0.5 * formAmt;
  c = owMix3(c, cLight * 1.05, bleed * 0.35 * owFbm01(p * 8.0, P * 8.0, 3, 0.5));

  // tie holes, plugged, one every other board
  let tf = fract(vec2<f32>(uv.x * 3.0, boards * 0.5)) - vec2<f32>(0.5);
  let tieRnd = owHash12(floor(vec2<f32>(uv.x * 3.0, boards * 0.5)) + vec2<f32>(U.seed));
  let tie = owSmoothstep(0.085, 0.05, length(tf * vec2<f32>(1.0, 2.0))) * owStep(0.45, tieRnd) * formAmt;
  h -= tie * 0.10;
  ao -= tie * 0.5;
  c = owMix3(c, cDark * 0.85, tie * 0.6);

  // ---- saw-cut control joints (slabs) + power-float polish ----
  let jd = abs(fract(uv + vec2<f32>(0.5)) - vec2<f32>(0.5));
  var joint = max(1.0 - owSmoothstep(0.0035, 0.010, jd.x), 1.0 - owSmoothstep(0.0035, 0.010, jd.y));
  joint *= jointAmt;
  h -= joint * 0.10;
  ao -= joint * 0.55;
  c = owMix3(c, cDark * 0.62, joint * 0.65);
  // trowel arcs left by the power float
  let swirl = owFbm01(owWarp(p * 1.1 + vec2<f32>(3.0), P * 1.1, 1.4, 3), P * 1.1, 3, 0.6);
  rough -= jointAmt * owSmoothstep(0.35, 0.85, swirl) * 0.10;
  c *= 1.0 - jointAmt * owSmoothstep(0.4, 0.9, swirl) * 0.07;

  // ---- structural cracks: branch from the seams and corners ----
  let crk = owCracks(p * 2.6, P * 2.6, 0.85, 0.028, 0.50);
  let crkFine = owCracks(p * 7.0 + vec2<f32>(31.0), P * 7.0, 0.9, 0.020, 0.60) * 0.55;
  let crack = owClamp(crk + crkFine, 0.0, 1.0);
  h -= crack * 0.12;
  ao -= crack * 0.45;
  c = owMix3(c, cDark * 0.80, crack * 0.42);
  rough += crack * 0.12;

  // ---- spalling: a chunk of the skin has broken off, aggregate showing ----
  let sp = owWorley(p * 1.1 + vec2<f32>(7.3), P * 1.1, 0.9);
  let spallCell = owStep(0.90, sp.w);
  let spall = spallCell * owSmoothstep(0.44, 0.16, sp.x) *
                owSmoothstep(0.42, 0.62, owFbm01(p * 4.0 + vec2<f32>(2.0), P * 4.0, 4, 0.5));
  h -= spall * 0.13;
  ao -= spall * 0.35;
  c = owMix3(c, owMix3(cDark, cMid, aggRnd) * 0.88, spall * 0.8);
  rough += spall * 0.10;
  // rim of the spall catches light
  let spallRim = spall * (1.0 - spall) * 4.0;
  c *= 1.0 + spallRim * 0.10;

  // ---- small chips: 2-5 cm bites out of the skin showing darker, wetter
  //      concrete plus the sand fraction underneath (~3% of the surface) ----
  let ck = owWorley(owWarp(p * 5.6 + vec2<f32>(19.0), P * 5.6, 0.6, 3), P * 5.6, 0.95);
  let ckSel = owStep(0.90, ck.w);
  let ckSize = 0.20 + 0.16 * ck.z;
  let ckShape = owSmoothstep(ckSize, ckSize * 0.3,
                             ck.x * (0.72 + 0.56 * owFbm01(p * 16.0, P * 16.0, 3, 0.5)));
  let chip = ckSel * ckShape;
  c = owMix3(c, owMix3(c * 0.74, owMix3(cDark, cMid, sand.z), 0.5), chip * 0.85);
  h -= chip * 0.045;
  ao -= chip * 0.24;
  rough += chip * 0.08;
  let ckLip = max(ckSel * (owSmoothstep(ckSize * 1.25, ckSize, ck.x) - ckShape), 0.0);
  c *= 1.0 + ckLip * 0.10;

  // ---- staining: rain runoff, soot, rust bleed from rebar ----
  // Only ~3:1 stretched and shallow: the long runs come from the runtime
  // weather layer, which knows where the sills and ledges are. A 10:1 stretch
  // baked into the tile at full strength just reads as wood veneer.
  let streak = owFbm01(vec2<f32>(p.x * 6.0, p.y * 2.0), vec2<f32>(P.x * 6.0, P.y * 2.0), 5, 0.55);
  let runoff = owSmoothstep(0.58, 0.95, streak) * (0.35 + 0.65 * owSmoothstep(0.2, 0.8, macro_));
  c *= 1.0 - runoff * 0.14;
  rough += runoff * 0.05;

  let rustBleed = owSmoothstep(0.72, 0.98, streak * (0.6 + 0.5 * tieRnd)) * owStep(0.80, tieRnd);
  c = owMix3(c, owSRGB(vec3<f32>(0.42, 0.24, 0.12)), rustBleed * 0.45);

  // dirt collects in every recess
  let cavity = 1.0 - owSmoothstep(0.42, 0.66, h);
  c = owMix3(c, owSRGB(vec3<f32>(0.20, 0.19, 0.17)), cavity * 0.35);

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.85));
  rough = owClamp(rough, 0.48, 0.98);
  ao = owClamp(ao, 0.15, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
