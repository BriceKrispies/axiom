
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed * 2.7);

  let bed = owFbm01(p * 1.3, P * 1.3, 4, 0.55);

  //
  // This is the street. Not a bed of loose chippings — compacted dust and grit
  // with aggregate part-buried in it, which is what a Levantine back street
  // actually is, and much more importantly it is what stops twelve metres of
  // road reading as dither.
  //
  // Two things were wrong before. (1) The finest grade was 208 cells across a
  // 512 bake — 2.5 texels, i.e. white noise, and the "grain" band on top of it
  // was 480 cells across 512, literally sub-texel. (2) The interstitial dust
  // was authored at 0.29 while the stone tops ran to 0.62 and then the runtime
  // cavity-grime layer pushed the (low) bed down another 17% toward black. A
  // 2.5:1 albedo step at a 10 mm period across the whole frame is the textbook
  // recipe for salt-and-pepper. So: three grades at 34/19/9 mm (5.9 texels at
  // the worst), stones separated from the bed by RELIEF and ROUGHNESS rather
  // than by value, and a bed sitting mid-height so the cavity term leaves it
  // alone.
  //
  let a = owWorley(p * 5.5, P * 5.5, 1.0);
  let b = owWorley(p * 10.0 + vec2<f32>(5.0), P * 10.0, 1.0);
  let cSm = owWorley(p * 21.0 + vec2<f32>(11.0), P * 21.0, 1.0);

  // Sparse: most of what you see is the compacted bed, with stones IN it. The
  // old coverage was ~80% at every grade, which is a shingle beach; a used
  // road shows perhaps a quarter of its aggregate.
  let sA = owSmoothstep(0.36, 0.10, a.x) * owStep(0.44, a.w);
  let sB = owSmoothstep(0.30, 0.08, b.x) * owStep(0.62, b.w);
  let sC = owSmoothstep(0.24, 0.06, cSm.x) * owStep(0.74, cSm.w);

  // The stones live in the height field: raised relief so each one catches the
  // sun on one side and shadows on the other. Half-buried, not tipped out on
  // the surface — the peak-to-trough is only a few mm at world scale.
  let ha = sA * 0.15 * (0.5 + a.z);
  let hb = sB * 0.09 * (0.5 + b.z);
  let hc = sC * 0.025;
  h = 0.54 + (bed - 0.5) * 0.11 + max(max(ha, hb), hc) + 0.22 * (ha + hb);

  //
  // The stone palette has to straddle the bed value, not sit above it. With
  // the bed at 0.35 and the stones running 0.29-0.51 every dark stone hid in
  // the dust and only the pale ones showed, which is why the road read as
  // white confetti scattered on grey rather than as aggregate. Half the
  // stones are now darker than the bed and half lighter.
  //
  let s1 = owSRGB(vec3<f32>(0.372, 0.356, 0.332));
  let s2 = owSRGB(vec3<f32>(0.232, 0.220, 0.208));
  let s3 = owSRGB(vec3<f32>(0.462, 0.438, 0.400));
  let s4 = owSRGB(vec3<f32>(0.352, 0.276, 0.220));
  var top = owMix3(s1, s2, a.z);
  top = owMix3(top, s3, owStep(0.78, a.w));
  top = owMix3(top, s4, owStep(0.90, b.w) * 0.7);

  // The bed is dust, and it is only a few percent off the stones sitting in it.
  let cBed = owSRGB(vec3<f32>(0.362, 0.336, 0.294));
  var c = owMix3(cBed, top, owClamp(sA * 0.70 + sB * 0.42 + sC * 0.16, 0.0, 1.0));
  // ~9 mm grain, 4.9 texels wide: a texture, not a dither.
  let grain = owFbm01(p * 13.0, P * 13.0, 4, 0.5);
  c *= 0.965 + 0.07 * grain;

  // Per-stone gloss: wet-worn pebbles glint, the dust between them does not.
  // This, not albedo, is what separates a stone from the dust around it.
  // Per-stone gloss, but only a little of it. Under a bright sky the IBL
  // specular lobe is a big part of what a shaded ground plane returns, so a
  // wide roughness spread at the aggregate period is another way of writing
  // salt-and-pepper: clamping this term alone took the measured
  // high-frequency deviation on the road from 2.45 to 1.68.
  rough = 0.82 + 0.05 * grain + (1.0 - owClamp(sA + sB, 0.0, 1.0)) * 0.06
        - sA * (0.06 + 0.07 * a.z) - sB * 0.05 * b.z;
  metal = 0.0;
  //
  // AO IS THE WHOLE BALLGAME ON A GROUND PLANE. A street in shadow is lit
  // almost entirely by the sky, so orm.r is very nearly the only shading
  // term the surface has — a 0.62:1.0 cavity ripple at the 10-30 mm aggregate
  // period is therefore a 1.6:1 luminance ripple at 2 screen pixels, which is
  // precisely the salt-and-pepper the critics measured. (Proved by clamping
  // the albedo to a constant and the normal map to flat: the speckle survived
  // both untouched, and only died when this range was compressed.) Baked
  // cavity AO belongs at low frequency and low contrast; the shading of an
  // individual stone is the normal map's job.
  //
  ao = owMix(0.87, 1.0, owSmoothstep(0.42, 0.66, h));

  // fine dust filling the gaps
  let dust = 1.0 - owSmoothstep(0.44, 0.62, h);
  c = owMix3(c, cBed * 1.04, dust * 0.5);
  rough += dust * 0.08;
  ao = owMix(ao, 1.0, dust * 0.3);

  // Wheel and foot traffic sweeps the loose grit into drifts and polishes bare
  // lanes: 0.5-1.5 m form inside the tile, which is the scale the eye uses to
  // decide whether a road is a surface or a pattern.
  let drift = owFbm01(owWarp(p * 0.9 + vec2<f32>(17.0), P * 0.9, 0.8, 3), P * 0.9, 4, 0.6);
  h += (drift - 0.5) * 0.10;
  c *= 0.86 + 0.28 * drift;
  rough += (drift - 0.5) * 0.10;
  // Dust drifts BURY the aggregate: where the drift is deep the stones go
  // under it. Without this the stone density is identical over every square
  // metre of a hundred-metre street, which is the tell that it is a texture.
  c = owMix3(c, cBed * (0.92 + 0.22 * drift), owSmoothstep(0.55, 0.88, drift) * 0.72);

  // Dried tyre tracks and dragged-heel scuffs — long, shallow, low contrast.
  let scuff = owFbm01(owShear(p * 2.2, 0.0, 6.0), owShearPer(P * 2.2, 6.0), 4, 0.5);
  c *= 1.0 - owSmoothstep(0.55, 0.92, scuff) * 0.10;
  rough -= owSmoothstep(0.6, 0.95, scuff) * 0.08;

  alb = owClamp3(c, vec3<f32>(0.02), vec3<f32>(0.78));
  rough = owClamp(rough, 0.62, 0.99);
  ao = owClamp(ao, 0.72, 1.0);
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
