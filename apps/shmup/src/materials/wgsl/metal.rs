//! Metals. The single most important physical rule here: bare metal is
//! metalness 1, and every oxide/paint/dirt layer on top of it is metalness 0.
//! Blending metalness through the rust and chip masks is what makes these read
//! as real steel rather than as grey plastic.
//!
//! WGSL transcription of `Claude-of-Duty/src/materials/glsl/surfaces-metal.js`.

/// `RUST_HELPERS` (`surfaces-metal.js:9-21`).
/// Shared: layered iron oxide. Returns rust amount \[0,1\] and its colour.
pub const RUST_HELPERS: &str = r#"
fn owRustColour(t: f32, grain: f32) -> vec3<f32> {
  // young rust is orange, mature rust is dark red-brown, old rust is near-black
  let c1 = owSRGB(vec3<f32>(0.560, 0.290, 0.110));   // fresh orange
  let c2 = owSRGB(vec3<f32>(0.380, 0.180, 0.085));   // mid
  let c3 = owSRGB(vec3<f32>(0.190, 0.100, 0.060));   // mature
  let c4 = owSRGB(vec3<f32>(0.640, 0.400, 0.190));   // powdery bloom
  var c = owMix3(c1, c2, owSmoothstep(0.15, 0.6, t));
  c = owMix3(c, c3, owSmoothstep(0.55, 1.0, t));
  c = owMix3(c, c4, owSmoothstep(0.55, 0.95, grain) * 0.45);
  return c * (0.82 + 0.36 * grain);
}
"#;

/// `METAL_RUST` (`surfaces-metal.js:23-88`).
/// Steel with layered rust blooms, flaking plates, pitting and scratches.
pub const METAL_RUST: &str = r#"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 7.7);

  // ---- base steel ----
  let mill = owFbm01(owShear(p * 4.0, 1.0, 6.0), owShearPer(P * 4.0, 6.0), 4, 0.5);
  let fine = owFbm01(p * 22.0, P * 22.0, 4, 0.5);
  let steel = owSRGB(vec3<f32>(0.330, 0.335, 0.345)) * (0.90 + 0.18 * mill);
  var c = steel;
  h = 0.72 + (mill - 0.5) * 0.02 + (fine - 0.5) * 0.01;
  rough = 0.40 + (mill - 0.5) * 0.16 + (fine - 0.5) * 0.08;
  metal = 1.0;
  ao = 1.0;

  // ---- rust blooms: warped billow clusters, hard-edged where they flake ----
  let wp = owWarp(p * 1.4, P * 1.4, 1.2, 4);
  var bloom = owBillow(wp, P * 1.4, 5, 0.6);
  bloom = 1.0 - bloom;                              // clusters, not veins
  let spread = owFbm01(p * 0.7 + vec2<f32>(12.0), P * 0.7, 3, 0.6);
  let rust = owSmoothstep(0.36, 0.72, bloom * (0.55 + 0.85 * spread));
  let rustGrain = owFbm01(p * 26.0, P * 26.0, 4, 0.55);
  let pit = owFbm01(p * 24.0, P * 24.0, 3, 0.5);

  // flaking scale: the rust lifts in plates near the edge of a bloom
  let scale = owWorley(p * 16.0, P * 16.0, 1.0).x;
  let flake = owSmoothstep(0.30, 0.10, scale) * owSmoothstep(0.25, 0.55, rust) * (1.0 - owSmoothstep(0.8, 1.0, rust));

  // Rust *colour* is driven by how old the patch is, not by how much of it
  // there is — otherwise every heavily rusted area collapses to the same brown.
  let rustAge = owFbm01(p * 0.85 + vec2<f32>(21.0), P * 0.85, 4, 0.62);
  let rustCol = owRustColour(rustAge * 0.8 + rust * 0.3, rustGrain);
  c = owMix3(c, rustCol, rust);
  metal = owMix(1.0, 0.0, owSmoothstep(0.15, 0.55, rust));
  rough = owMix(rough, 0.86 + 0.10 * rustGrain, owSmoothstep(0.1, 0.6, rust));
  h += rust * 0.11 * (0.4 + rustGrain) + flake * 0.13;
  h -= owSmoothstep(0.5, 0.95, rust) * pit * 0.14;      // deep pitting under old rust
  ao -= flake * 0.30 + owSmoothstep(0.6, 1.0, rust) * 0.15;

  // ---- pitting straight into the steel where rust has eaten through ----
  let pits = owWorley(p * 22.0, P * 22.0, 1.0);
  let deep = owSmoothstep(0.22, 0.0, pits.x) * owStep(0.72, pits.w) * owSmoothstep(0.3, 0.8, rust);
  h -= deep * 0.22;
  ao -= deep * 0.45;
  c = owMix3(c, rustCol * 0.35, deep * 0.7);

  // ---- scratches through everything, exposing bright metal ----
  var scr = owScratches(p * 3.0, P * 3.0, 12.0, 1.0, 0.60);
  scr += owScratches(p * 5.0 + vec2<f32>(8.0), P * 5.0, 9.0, -2.0, 0.66) * 0.7;
  scr = owClamp(scr, 0.0, 1.0) * 0.6;
  c = owMix3(c, owSRGB(vec3<f32>(0.480, 0.485, 0.495)), scr * 0.8);
  metal = owMix(metal, 1.0, scr * 0.85);
  rough = owMix(rough, 0.24, scr * 0.7);
  h -= scr * 0.010;

  // ---- grime ----
  let grime = owSmoothstep(0.55, 0.9, owFbm01(vec2<f32>(p.x * 5.0, p.y * 0.8), vec2<f32>(P.x * 5.0, max(P.y, 1.0)), 5, 0.55));
  c *= 1.0 - grime * 0.25;
  rough += grime * 0.08;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.80));
  rough = owClamp(rough, 0.12, 0.99);
  ao = owClamp(ao, 0.15, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
"#;

/// `METAL_PAINTED` (`surfaces-metal.js:90-178`).
/// Industrial paint over primer over rust over steel, with chipping and bleed.
pub const METAL_PAINTED: &str = r#"
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
"#;

/// `METAL_BRUSHED` (`surfaces-metal.js:180-237`).
/// Brushed steel: X-aligned fibres, score lines, dents, smudges and grime.
pub const METAL_BRUSHED: &str = r#"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 15.1);

  // brushing runs along X: heavy shear so the noise stretches into fibres
  let bp = owShear(p, 0.0, 64.0);
  let BP = owShearPer(P, 64.0);
  let brush1 = owFbm01(bp * 2.0, BP * 2.0, 4, 0.5);
  let brush2 = owFbm01(bp * 8.0 + vec2<f32>(3.0), BP * 8.0, 3, 0.5);
  let brush3 = owFbm01(owShear(p * 4.0, 0.0, 24.0), owShearPer(P * 4.0, 24.0), 3, 0.5);
  let brush = brush1 * 0.5 + brush2 * 0.32 + brush3 * 0.18;

  // RENAMED: the source calls this `macro`, which is a WGSL reserved word.
  let macroNoise = owFbm01(p * 0.9, P * 0.9, 3, 0.6);

  var c = owSRGB(vec3<f32>(0.560, 0.565, 0.575));
  c *= 0.93 + 0.13 * brush;
  c *= 0.97 + 0.06 * macroNoise;

  metal = 1.0;
  rough = 0.22 + brush * 0.24 + (macroNoise - 0.5) * 0.06;
  h = 0.78 + (brush - 0.5) * 0.012;
  ao = 1.0;

  // deeper score lines
  let score = owScratches(p * 1.0, P, 40.0, 0.0, 0.60);
  rough += score * 0.22;
  h -= score * 0.006;
  c *= 1.0 - score * 0.05;

  // cross scratches from handling
  let cross = owScratches(p * 3.0, P * 3.0, 8.0, 3.0, 0.70) * 0.7;
  rough += cross * 0.20;
  h -= cross * 0.004;

  // dents: shallow, wide, they break the reflection
  let dent = owFbm01(p * 3.0 + vec2<f32>(7.0), P * 3.0, 3, 0.6);
  h += (dent - 0.5) * 0.05;

  // fingerprints and grease smudges — the thing that sells brushed metal
  let smudge = owSmoothstep(0.58, 0.86, owFbm01(owWarp(p * 2.2 + vec2<f32>(19.0), P * 2.2, 0.7, 3), P * 2.2, 4, 0.55));
  rough += smudge * 0.22;
  c *= 1.0 - smudge * 0.06;
  metal -= smudge * 0.10;

  // grime settling in
  let grime = owSmoothstep(0.66, 0.95, owFbm01(p * 5.0, P * 5.0, 4, 0.55));
  c = owMix3(c, owSRGB(vec3<f32>(0.180, 0.175, 0.165)), grime * 0.35);
  rough += grime * 0.18;
  metal -= grime * 0.35;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.88));
  rough = owClamp(rough, 0.08, 0.95);
  metal = owClamp(metal, 0.0, 1.0);
  ao = owClamp(ao, 0.4, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
"#;

/// `CORRUGATED` (`surfaces-metal.js:239-323`).
/// Galvanised corrugated sheet: ridge profile, panel laps, spangle, rust,
/// perforations, hex fixings with washers and weeping rust streaks.
pub const CORRUGATED: &str = r#"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let RIDGES = 12.0;
  let p = uv * P + vec2<f32>(U.seed * 6.1);

  // ---- the profile: sinusoidal ridges with a flat-ish crown ----
  let t = uv.x * RIDGES * 6.28318530718;
  let wave = sin(t);
  let profile = owSign(wave) * pow(abs(wave), 0.72) * 0.5 + 0.5;
  // panel joints every 4 ridges: one sheet laps over the next
  let panel = uv.x * RIDGES / 4.0;
  let panelId = floor(panel);
  let lap = owSmoothstep(0.0, 0.06, fract(panel)) * owSmoothstep(0.0, 0.06, 1.0 - fract(panel));
  let panelStep = (owHash11(panelId + U.seed) - 0.5) * 0.05;

  let dents = owFbm01(p * 2.2, P * 2.2, 4, 0.6);
  let fine = owFbm01(p * 11.0, P * 11.0, 4, 0.5);

  h = 0.18 + profile * 0.62 + panelStep + (dents - 0.5) * 0.07 + (fine - 0.5) * 0.012;
  h -= (1.0 - lap) * 0.06;

  // ---- galvanised zinc: crystalline spangle ----
  let sp = owWorley(p * 7.0, P * 7.0, 1.0);
  let spangle = owSmoothstep(0.55, 0.05, sp.x);
  let zinc = owSRGB(vec3<f32>(0.520, 0.535, 0.545));
  var c = owMix3(zinc * 0.86, zinc * 1.12, spangle * (0.3 + 0.7 * sp.z));
  c *= 0.94 + 0.12 * fine;
  metal = 1.0;
  rough = 0.34 + (1.0 - spangle) * 0.16 + (fine - 0.5) * 0.08;
  ao = 1.0;

  // ---- rust, heavier in the valleys and at the bottom of the sheet ----
  let valley = 1.0 - profile;
  let rustField = owSmoothstep(0.62, 0.98,
      (1.0 - owBillow(owWarp(p * 1.6, P * 1.6, 1.0, 4), P * 1.6, 5, 0.6)) *
      (0.58 + 0.40 * valley) + (1.0 - uv.y) * 0.16);
  let rustGrain = owFbm01(p * 22.0, P * 22.0, 4, 0.55);
  let rustCol = owRustColour(rustField, rustGrain);
  c = owMix3(c, rustCol, rustField);
  metal = owMix(metal, 0.0, owSmoothstep(0.15, 0.6, rustField));
  rough = owMix(rough, 0.88 + 0.08 * rustGrain, owSmoothstep(0.1, 0.6, rustField));
  h += rustField * 0.02 * rustGrain;

  // holes rusted right through
  let hole = owWorley(p * 5.0 + vec2<f32>(31.0), P * 5.0, 0.95);
  let perf = owSmoothstep(0.10, 0.02, hole.x) * owStep(0.94, hole.w) * owSmoothstep(0.5, 0.9, rustField);
  h -= perf * 0.5;
  ao -= perf * 0.7;
  c = owMix3(c, rustCol * 0.25, perf);

  // ---- fixings: hex screws with a rubber washer, two rows, on the crowns ----
  let crown = owSmoothstep(0.72, 0.95, profile);
  let fx = vec2<f32>(fract(uv.x * RIDGES) - 0.5, fract(uv.y * 3.0) - 0.5);
  let fd = length(fx * vec2<f32>(1.0, RIDGES / 3.0));
  let screwRnd = owHash12(floor(vec2<f32>(uv.x * RIDGES, uv.y * 3.0)) + vec2<f32>(U.seed));
  let screw = owSmoothstep(0.16, 0.11, fd) * crown * owStep(0.25, screwRnd);
  let washer = owSmoothstep(0.24, 0.18, fd) * crown * owStep(0.25, screwRnd);
  h += washer * 0.02 + screw * 0.035;
  c = owMix3(c, owSRGB(vec3<f32>(0.120, 0.115, 0.110)), washer * 0.8);
  c = owMix3(c, owMix3(owSRGB(vec3<f32>(0.400, 0.405, 0.410)), rustCol, rustField), screw);
  rough = owMix(rough, 0.85, washer * 0.8);
  rough = owMix(rough, 0.42 + rustField * 0.4, screw);
  metal = owMix(metal, 0.0, washer * 0.9);
  metal = owMix(metal, 1.0 - rustField, screw);
  ao -= (washer - screw) * 0.35;
  // rust streak weeping from each fixing
  // NOTE: `washer * 0.0` is dead computation in the source; ported verbatim.
  let weep = washer * 0.0 + owSmoothstep(0.34, 0.20, fd) * owStep(0.25, screwRnd) * crown *
               owSmoothstep(0.0, 0.5, fract(uv.y * 3.0) - 0.5);
  c = owMix3(c, owSRGB(vec3<f32>(0.330, 0.170, 0.080)), owClamp(weep, 0.0, 1.0) * 0.5);

  // ---- dirt collecting in the valleys ----
  let dirt = valley * owSmoothstep(0.35, 0.8, owFbm01(p * 3.0, P * 3.0, 4, 0.55));
  c = owMix3(c, owSRGB(vec3<f32>(0.200, 0.185, 0.160)), dirt * 0.40);
  rough += dirt * 0.14;
  metal *= 1.0 - dirt * 0.5;
  ao -= valley * 0.18;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.85));
  rough = owClamp(rough, 0.14, 0.99);
  metal = owClamp(metal, 0.0, 1.0);
  ao = owClamp(ao, 0.15, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
"#;
