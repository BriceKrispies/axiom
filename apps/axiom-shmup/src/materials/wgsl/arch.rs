//! WGSL transcription of Claude-of-Duty `src/materials/glsl/surfaces-arch.js`.
//!
//! From the source file's own header (`surfaces-arch.js:1-13`):
//!
//! > Architectural surfaces: concrete, brick, plaster, stucco, ceramic tile.
//! >
//! > Every surface implements:
//! >   `void owSurface(vec2 uv, out vec3 alb, out float h, out float rough,`
//! >   `               out float metal, out float ao)`
//! > 'uv' is [0,1) across the tile, 'h' is 0..1 (0.5 ≈ the nominal surface
//! > plane), 'alb' is LINEAR albedo (authored via `owSRGB()` so the numbers read
//! > like paint swatches), and 'ao' is a baked cavity term, not a lighting term.
//! >
//! > uSeed shifts the noise lattice so two variants of the same surface never
//! > line up. Shifting the argument of a periodic function keeps it periodic.
//!
//! Each `pub const` holds the WGSL transcription of one exported GLSL block's
//! `owSurface` body, in the source's order. WGSL has no `out` parameters, so
//! each body is wrapped in a `ptr<function, _>` signature with a `var` copy
//! prologue and a write-back epilogue; that wrapper is the only structural
//! change — every statement between them is line-for-line the source.
//!
//! Uniform renames: `uSeed` -> `U.seed`, `uTintA` -> `U.tint_a`,
//! `uTintB` -> `U.tint_b`, `uParam` -> `U.param`.
//!
//! One identifier rename: GLSL `macro` (in `CONCRETE` and `PLASTER`) is a WGSL
//! reserved word, so it is spelled `macro_` here. Flagged at each site.

/// `CONCRETE` (`surfaces-arch.js:15-179`) — cast concrete: pour variation,
/// exposed aggregate, the coarse sand fraction, bug holes, board-formed
/// formwork (`uParam.x`) or saw-cut control joints (`uParam.y`), structural
/// cracks, spalling, chips, and rain/soot/rust staining.
pub const CONCRETE: &str = r#"
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
"#;

/// `BRICK` (`surfaces-arch.js:181-336`) — a running-bond brick wall: 6 bricks
/// across by 18 courses, per-brick jitter and kiln shade, a raked mortar joint
/// with a hard arris, face pores and broken arrises, then efflorescence, soot
/// runoff and hairline cracks over the whole wall.
pub const BRICK: &str = r#"
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
"#;

/// `PLASTER` (`surfaces-arch.js:338-499`) — trowelled plaster/stucco: sheared
/// trowel sweeps, skim-coat laps, the three 0.1-1 m weathering bands, the sand
/// tooth of the finish coat, pinholes, crazing and structural cracks, blown
/// patches down to the substrate, chipped flakes, water tide marks and mould.
pub const PLASTER: &str = r#"
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
"#;

/// `TILE` (`surfaces-arch.js:501-563`) — a 6x6 grid of glazed ceramic tiles on
/// a flat grout bed with a hard arris, per-tile batch shade and glaze noise,
/// cracked/broken tiles showing the bed underneath, and traffic wear.
pub const TILE: &str = r#"
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
"#;
