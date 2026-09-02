
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(6.0);
  let p = uv * P + vec2<f32>(U.seed * 3.0);
  let a = owFbm01(p * 0.5, P * 0.5, 4, 0.62);
  let b = owFbm01(owWarp(p * 1.0, P, 1.1, 3), P, 4, 0.58);
  let c = owFbm01(p * 2.5, P * 2.5, 4, 0.55);
  let d = owFbm01(p * 7.0, P * 7.0, 4, 0.5);
  alb = vec3<f32>(a, b, c);
  h = d;
  rough = 0.5; metal = 0.0; ao = 1.0;
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
