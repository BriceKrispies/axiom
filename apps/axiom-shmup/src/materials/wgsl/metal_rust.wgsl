
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
