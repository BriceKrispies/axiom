
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  let P = vec2<f32>(8.0);
  let p = uv * P;
  *albOut = vec3<f32>(owHash12(p * 37.0),
                      owFbm01(p * 3.0, P * 3.0, 4, 0.55),
                      owWorley(p * 5.0, P * 5.0, 1.0).x);
  *hOut = owVoronoiEdge(p * 4.0, P * 4.0, 1.0);
  *roughOut = 0.5;
  *metalOut = 0.0;
  *aoOut = 1.0;
}
