
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
