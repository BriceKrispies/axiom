
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
