
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
