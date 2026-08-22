# `sky/dome.rs` + `sky/clouds.rs` — audit and completion

Slice: `apps/shmup/src/sky/dome.rs` (from `src/sky/dome.js`, 377 lines) and
`apps/shmup/src/sky/clouds.rs` (from `src/sky/clouds.js`, 374 lines). Both were
flagged **borderline** in `06-parallel-port-plan.md`, in the same category as
the four ports that were stopped mid-slice and committed looking finished.

**Verdict: the code was in much better shape than the flag suggested — but the
*pins* did not exist at all.** One real gap in `dome.rs` (two shader `main`s
filed under a justification that did not cover them), two faithfulness defects
shared by both files, and zero golden coverage for either module.

## 1. The headline finding: the sky goldens were never emitted

`tests/sky/capture.mjs` already contained a complete, careful, line-referenced
hand-transcription of `CLOUDS_GLSL`, `SKY_BODY`, `STARS_GLSL` and the
volumetrics shaders — roughly 500 lines of it — and **assigned none of it to
`out`**. The functions were defined and then dead. `golden.json`'s top-level
keys before this slice were:

```
constants phaseFunctions raySphere medium lutUv transmittanceToSpace luminance
raymarchSkySegment transmittanceLut multiscatterLut skyViewLut skyViewLookup
ambientProbe derivedConstants noise celestial
```

— i.e. `atmosphere.js`, `luts.js`, `noise.js` and `celestial.js` only, and
`tests/sky_port.rs` had no test naming `dome`, `clouds`, `stars` or
`volumetrics`. So `dome.rs` and `clouds.rs` — ~650 lines of transcribed
shader — were unverified. That is exactly the "compiles, wired in, looks
finished" hazard the plan warns about, one layer further in: the *harness*
looked finished too.

Nothing was structurally wrong with the transcriptions; they just never ran —
which is precisely how eight last-bit faithfulness defects (§4) survived in
them.

### Sweep of the rest of the capture script

The `volumetrics` agent hit the identical shape and asked for a sweep of the
whole file. Done, mechanically (strip comments, list every top-level
`function`/`const`, count references):

- **Dead transcription found: the entire `volumetrics.js` block.** `raymarchFog`
  and `compositeAnalytic` were called by nothing, so `skFogAmbient`,
  `skFogPhase`, `skFogInscatterPhase`, `skFogNearRamp`, `skFogDensity`,
  `skHeightIntegral` and `skVogel` were all transitively dead too. Volumetrics
  now has its own capture script and golden set under
  `apps/shmup/tests/sky_volumetrics/`, so this copy was both dead and
  duplicated. **Removed** — not commented out, so nothing here can read as
  coverage again — and replaced by a comment block saying what was there, why
  it was dangerous, and where the live version lives.
- **`stars.js`'s transcriptions are now live**, via `skSample` → `skNightSky`,
  which this slice emits. They are exercised but not *individually* pinned; see
  §8.
- **After the sweep, zero dead top-level definitions remain** in
  `tests/sky/capture.mjs`.
- **Golden keys with no Rust consumer: one**, `derivedConstants.zenithSkyRgb`.
  It is a pre-existing atmosphere/luts field that only feeds the capture's own
  `zenithSkyLuminance` self-check, which the Rust does assert (as an
  order-of-magnitude bound). Harmless, outside this slice, flagged rather than
  changed.

## 2. Function-by-function inventory

### `clouds.js`

| source | kind | status before | now |
|---|---|---|---|
| `skCloudMacro` (53-58) | GLSL | ported (`cloud_macro`) | pinned, **real oracle** |
| `skSmoothRidge2` (74-84) | GLSL | ported | pinned |
| `skCirrusBand` (126-149) | GLSL | ported | pinned |
| `skCumulusDensity` (152-172) | GLSL | ported | pinned |
| `skCumulusLight` (179-186) | GLSL | ported | pinned |
| `skClouds` (195-327) | GLSL | ported | pinned |
| `skCloudShadow` (334-341) | GLSL | ported | pinned |
| `cloudMacro` (351-356) | **plain JS** | ported (same fn as `skCloudMacro`) | pinned, **real oracle** |
| `cloudSunOcclusion` (364-374) | **plain JS** | ported | pinned, **real oracle** |
| `SK_CUMULUS_KM` / `SK_CIRRUS_KM` | const | ported | — |

`clouds.js` was **functionally complete**. Every named GLSL function and both
exported JS functions were present, and re-reading each against the source
line by line found no algorithmic divergence. `uCloudParams.z` (`detail_gain`)
really is read by nothing in `CLOUDS_GLSL`; the existing comment saying so is
correct, and it is carried per the "dead computation is still part of the
source" rule.

### `dome.js`

| source | kind | status before | now |
|---|---|---|---|
| `owSkLum` (57) | GLSL | not a separate fn — `atmosphere::luminance` | documented; identical Rec.709 sum, same term order |
| `skAmbientSky` / `skAmbientHorizon` (62-63) | GLSL | folded into the `ambient: [Vec3; 2]` parameter | correct — see below |
| `skSunDisc` (70-82) | GLSL | ported | pinned |
| `skAureole` (99-117) | GLSL | ported | pinned |
| `skRolloff` (140-154) | GLSL | ported | pinned |
| `skMoonDisc` (156-188) | GLSL | ported | pinned |
| `skSample` (194-273) | GLSL | ported | pinned |
| `DOME_VERT` `main` (276-290) | GLSL | **ABSENT** | ported as `screen_ray`, pinned |
| `ENV_FRAG` `main` (303-315) | GLSL | **ABSENT** | ported as `equirect_direction`, pinned |
| `DOME_FRAG` `main` (292-300) | GLSL | absent | genuinely trivial: `sample(screen_ray(..).normalize(), 1)` |
| `SkyDome` class (317-377) | JS + THREE | not ported | correct — see below |

## 3. Judging the "deliberately not ported" claims

The module doc claimed two things were out of scope. One claim was right, one
was doing too much work.

**`SkyDome`'s ShaderMaterial wiring — a real boundary.** `constructor` builds
two `THREE.ShaderMaterial`s, a full-screen-triangle `Mesh`, an `onBeforeRender`
that copies `camera.projectionMatrixInverse`/`matrixWorld` into uniform
objects, and sets `renderOrder = -10000`, `depthTest/depthWrite = false`,
`owNoPrepass`/`owNoShadow`. There is no arithmetic in any of it. That is host
plumbing, correctly deferred to a WGSL/render-integration slice.

**The two shader `main`s were swept up in the same sentence, and should not
have been.** `DOME_VERT` computes the screen-pixel → world-ray map:
inverse-project an NDC point, divide through by `w`, renormalise onto the
`z = -1` plane, rotate by the camera basis. `ENV_FRAG` computes the
equirectangular-texel → world-ray map. Both are pure, portable, testable
arithmetic whose only GPU-shaped input is two matrices — and `skSample`, the
thing this module exists to reference-implement, is useless without knowing
which ray a pixel corresponds to. They are now `dome::screen_ray` and
`dome::equirect_direction`. This is the "unfinished work wearing a
justification" the brief asked me to look for; it was one sentence of module
doc away from being invisible.

Two details that make `screen_ray` a faithful per-pixel port rather than an
approximation of an interpolated varying: the source explicitly divides by
`-vd.z` *so that* the quantity is linear in screen space (`dome.js:284-286`),
which means evaluating per-pixel reproduces the rasteriser's interpolation
exactly; and `THREE.Matrix4.elements` is **column-major**, so the element
indices in the port are columns. A row-major reading compiles and silently
transposes the projection — the matrix-storage-order trap, named in the brief.

**`fwidth` — a real boundary, but it was a dead end.** `skSunDisc` and
`skMoonDisc` anti-alias their edge with `fwidth(theta)` / `fwidth(r2)`, the
screen-space derivative over a 2×2 fragment quad. A CPU sample genuinely
cannot know which quad the hardware picked, so taking the derivative as an
explicit parameter is right (and matches how `raymarch_sky` already handles
`uTransmittanceLut`). But leaving it *only* as a parameter means the disc
anti-aliasing is never pinned against anything real — every test value would
be invented, which is precisely what the recipe forbids. So:

- `dome::fwidth(v, v_at_x_plus_1, v_at_y_plus_1)` is GLSL's own definition,
  `abs(dFdx) + abs(dFdy)`; and
- with `screen_ray` now present, `tests/sky_port.rs` builds the two
  neighbouring-pixel rays a real 1920×1080 fragment quad would have, takes
  `safe_acos` against the sun at all three, and pins the resulting derivative.

The parameter stays a parameter (the quad choice is still the caller's), but
it is now reachable end to end with no invented numbers anywhere in the chain.

**`skAmbientSky`/`skAmbientHorizon` — correctly folded.** They are
`texture(uSkyAmbientLut, vec2(0.25, 0.5))` and `vec2(0.75, 0.5)` on a 2×1
texture, i.e. exactly texel 0 and texel 1 at their centres, which is exactly
what `luts::bake_ambient` already returns as `[Vec3; 2]`. Nothing lost.

**`owSkLum` — correctly reused.** `dome.js` declares its own luminance helper
rather than reusing `atmosphere.js`'s, but the two are the same Rec.709 dot
product in the same term order, so `atmosphere::luminance` stands in exactly.
`owSkLum` has no other caller in the source. Now documented at `rolloff`.

## 4. Faithfulness defects found and fixed (both files) — eight sites

All eight are the same class — a division turned into a reciprocal-multiply, or
a chain of multiplies re-associated — and **every one was present in *both* the
Rust and the capture script's JS**. A shared tidy, not a divergence, so no
golden would ever have caught them. They are last-bit-scale, not behavioural;
the reason to fix them is the brief's rule, and the fact that the identical
class of bug was just found living in `volumetrics`' dead transcription for
exactly the same reason.

Method that found them: after the first pass I re-read `dome.js` and
`clouds.js` as GLSL text alone — grepping every `/` and every multi-factor
product — rather than reading the Rust and checking it looked right. The first
pass (reading Rust against GLSL) caught four; the second pass (reading GLSL
first, then looking for the operator in the Rust) caught four more. That
asymmetry is the whole argument for the discipline.

**A. `x / SK_PI` written as `x * (1.0 / PI)`** — multiplying by a rounded
reciprocal is a different operation from dividing:

- `clouds.rs` cirrus `col`: `( sunHigh * fwd + moonHigh * … ) / SK_PI`
- `clouds.rs` cumulus: `direct / SK_PI + fill`
- `clouds.rs` composite: `outC /= outA`
- `dome.rs` ground bounce: `uSunIrradiance * max(0, uSunDir.y) / SK_PI`
- `dome.rs` `sun_disc`: `… * skTransmittance(…) / ( uDisc.z * uDisc.z )`

**B. Grouping changed.** The ground-bounce site above also had the *grouping*
wrong: GLSL evaluates `(irradiance * cosine) / pi` componentwise, the port had
`irradiance * (cosine / pi)`. A different expression, not a rearrangement.

**C. Vector-by-scalar chains folded into one multiply.** GLSL multiplies the
vector once per factor, left to right; folding the scalars first re-associates
the product:

- `dome.rs` `moon_disc`:
  `uMoonDiscRadiance * ( albedo / 0.13 ) * ( shade + earthshine ) * cover`
  — three separate vector multiplies, was one.
- `clouds.rs` cumulus `fill`:
  `ambient * mix( 0.50, 1.5, … ) * ( 0.32 + 0.68 * lit )` — two, was one.
- `clouds.rs` composite:
  `… + cumulus.rgb * cumulus.a * ( 1.0 - cirrus.a )` — the cumulus term is
  scaled twice, was once by the folded product. (`outA` on the line above
  genuinely *is* the folded scalar, because there the source folds it itself —
  worth noting, because the two lines look symmetric and are not.)

All eight are now `Vec3::div(Vec3::splat(…))` / separate `.scale(…)` steps in
Rust, and a new `divS` helper plus nested `scale` in the capture script,
matching the source's operator and grouping literally.

For the record, `SK_PI` is `3.141592653589793` (`atmosphere.js:110`), which is
exactly `std::f64::consts::PI` and exactly `Math.PI`, so nothing else was
hiding there.

Everything else checked and found already faithful: `s / max(n, 1e-4)` in
`skSmoothRidge2`, `1.0 / max(0.4, lenKM)` and `pr.y * fa * aniso` in
`skCirrusBand`, `0.20 / max(0.12, abs(lightDir.y))` and the three separate
`tau +=` statements in `skCumulusLight`, `0.85 * dBase / max(0.10, rayDir.y)`
and `0.09 / (abs(rayDir.y) + 0.09)` in `skClouds`, `SK_MIE_S * uMieScale *
0.0012 / max(...)` in `skAureole`, `pow(l/knee, p) * knee / l` in `skRolloff`,
`theta / R` and `dot(rayDir, mr) / R` in the two discs, and
`(uv.x - 0.5) * 2.0 * SK_PI` in `ENV_FRAG`.

**Doc drift, corrected:** `cirrus_band`'s Rust doc said the fibre term
modulates density "0.35..1.4". The code is `0.35 + 1.05 * f`; the *source's*
comment (`clouds.js:145-147`) says "between 0.35 and 1.2". The doc now cites
the code and notes the source's rounding, rather than quietly inventing a
third figure.

## 5. Traps checked by name

- **`sign` is not `signum`** — neither `dome.js` nor `clouds.js` calls GLSL
  `sign` at all (the one sky user is `luts::sky_view_lookup`, which already
  routes through `atmosphere::gl_sign`). Nothing to do, but checked.
- **`Float32Array`** — `grep` over both source files: no match. Neither file
  stores anything; both are pure shader text plus two scalar JS functions.
- **`Math.hypot`** — no match in either file. `length`/`normalize` in the port
  are `sqrt(dot)`, matching GLSL's `length`, which is what the source uses.
- **Float associativity** — the eight fixes in §4; and the three-tap
  self-shadow accumulation in `skCumulusLight` and the fbm accumulations in
  `skSmoothRidge2` were left in exactly the source's clumsy left-to-right
  order (`tau += a; tau += b; tau += c` as three statements, not one sum).
  This was by far the highest-yield trap in this slice: eight sites, all
  invisible to a golden because both readings shared them.
- **Euler order / matrix storage** — `screen_ray` is the only matrix consumer
  here and is column-major; see §3.
- **Enum used as a table index** — none in this slice.
- **Dead computation is still part of the source** — `uCloudParams.z` and
  `skAureole`'s unused `lightDir` parameter are both preserved-as-documented
  rather than deleted or silently reproduced.
- **A matching count is not proof / your comparator can be the bug** — the
  relevant version here: an *emitted* golden with no consumer proves nothing,
  and that is exactly what was on `main`. Every new table below has at least
  one assertion that some row is non-trivial, so a capture that silently
  starts producing all-zeros fails instead of passing.

## 6. What is pinned, and at what tolerance

All new goldens are in `apps/shmup/tests/sky/golden.json` (regenerate with
`node capture.mjs > golden.json` from `apps/shmup/tests/sky/`; verified
byte-reproducible across runs, and the diff against the previous file is
**purely additive** — every pre-existing key is byte-identical).

Tolerance is `REL = 1e-9` **relative**, the figure the rest of `sky_port.rs`
already uses, with an absolute floor of `1e-9` for values under 1. It is
looser than `core_port.rs`'s `1e-12` because these chains run `sin`, `cos`,
`exp`, `pow`, `sqrt` and `acos` dozens deep and those are not bit-guaranteed
between V8 and Rust's libm. A real regression here (a dropped `pi`, a
transposed matrix, a flipped smoothstep edge) moves these numbers by decimal
digits.

### Genuine oracle — `out.cloudsOracle`

The capture script imports and calls the **original** `clouds.js`:

| table | rows | inputs |
|---|---|---|
| `cloudMacro` | 8 | points across the four analytic waves |
| `cloudSunOcclusion` | 5 | incl. `sunDir.y` below the `0.1` floor, and `coverage = 0` |

A failure in these two is unambiguously a bug in `clouds.rs`.

### No oracle — `out.clouds` (CLOUDS_GLSL) and `out.dome` (SKY_BODY)

Everything else in both files is WebGL2 shader source held in a JS template
string. There is no JavaScript form to call, so `capture.mjs` hand-transcribes
each named GLSL function into plain JS, line-referenced against the source, and
`src/sky/{clouds,dome}.rs` transcribes the same GLSL independently. **These
tests pin the Rust against a second careful reading of the shader. They cannot
catch a mistake both readings share.** Auditing means reading three things side
by side: the GLSL, `capture.mjs`, and the Rust. (§4's eight defects are live
examples of exactly that blind spot: both readings tidied them the same way,
and only re-reading the GLSL text on its own caught them. Four of the eight
were caught only on the *second* pass, after switching from "read the Rust,
check it against the GLSL" to "read the GLSL, then go find that operator in
the Rust".)

| table | rows | what the inputs cover |
|---|---|---|
| `clouds.smoothRidge2` | 21 | 7 deck points × oct 1/2/3 |
| `clouds.cirrusBand` | 63 | 7 points × cov 0 / 0.21 / 0.8 × both real families + a `lenKM` under the `0.4` floor. All `cov = 0` rows must be exactly 0 (the silhouette early-out) and at least one row must be non-zero — both asserted. |
| `clouds.cumulusDensity` | 84 | 7 points × coverage 0 / 0.30 / 0.85 / 1.0 × oct 3 / 4 / 6. oct 3 keeps the cauliflower-ridge branch off; 4 and 6 turn it on. Both arms asserted present. |
| `clouds.cumulusLight` | 40 | 5 points × 4 light directions (incl. `|y|` under the `0.12` floor and a below-horizon light) × oct 2/4 |
| `clouds.cloudShadow` | 8 | 4 world points × 2 sun directions |
| `clouds.skClouds` | 18 | 9 rays × quality 0/1: below `-0.008` (exact-zero early-out, asserted), the `-0.008..0` sliver, grazing, a low ray with real cumulus (α 0.88), the cirrus zenith roll-off ramp, zenith, straight at the sun, straight at the moon. At least one α > 0.5 asserted. |
| `dome.aureole` | 18 | 6 `cosTheta` (incl. the `0.9135` cutoff itself, which is on the zero side) × 3 `rayDirY` (incl. below the horizon, exercising the `max(0.055, y + 0.055)` floor on both sides) |
| `dome.rolloff` | 16 | 4 colours × {knee 0, knee −1, knee 0.30 exp 1.5, knee 0.30 exp 0.38}. `knee <= 0` asserted **bit-identical** identity, not merely close. |
| `dome.sunDisc` | 16 | 8 θ from 0 out past the edge × 2 `fwidth`. Pins a genuine source quirk (below). |
| `dome.moonDisc` | 32 | 2 moon directions × 8 θ × oct 2/4. Both arms of `abs(uMoonDir.y) > 0.97 ? +Z : +Y` are driven — the reference axis the gnomonic frame is built from, whose wrong branch only shows for a moon near the zenith. |
| `dome.fwidth` | 4 | the arithmetic on its own |
| `dome.screenRay` | 7 NDC | matrices from a **real `THREE.PerspectiveCamera`** (63°, 16:9, its own `projectionMatrixInverse` and `matrixWorld` elements) — so the matrix half of this is a genuine oracle |
| `dome.fwidthFromRays` | 7 pixels | `screen_ray` at a fragment and its two quad neighbours → `safe_acos` → `fwidth`, end to end |
| `dome.equirectDirection` | 7 uv | including both poles and the seam |
| `dome.sample` | 24 | 12 rays × quality 0/1: inside the sun disc, the rim exactly at `R`, the AA band at fractional cover, inside and just outside the aureole cutoff, mid-sky, near zenith, a low ray with cumulus, horizon murk, two below-horizon ground-bounce rays, inside the moon disc |

`dome.uniforms` and `clouds.lighting` carry the whole uniform state and the
four per-deck irradiances into the golden, so the Rust test reads them rather
than restating them.

### Two behavioural assertions, not just value comparisons

- **`the_sun_disc_goes_in_after_the_roll_off`** — `skSample`'s own comment
  insists the discs go in *after* `skRolloff` because they are the only thing
  in the sky meant to clip and bloom. Checked as behaviour: the same ray at
  quality 1 comes back at luminance 665.6 against a roll-off knee of 0.30
  (>1000× the knee), while quality 0 — which skips the disc — sits at 0.68. If
  a future edit moved the roll-off after the discs, this notices.
- **`sun_disc_matches_the_transcribed_glsl`** pins a real source quirk: at
  `theta == R` exactly the coverage smoothstep is 0.5 — the disc *is* half
  covered — but `r` clamps to 1, `mu` is 0, and the limb term `pow(0, 0.32)`
  zeroes the radiance. Limb darkening takes the rim to black, so the outermost
  lit sample is strictly inside `R`. Every row at or beyond `R` is asserted
  exactly zero.

## 7. Byte-reproducibility note

`golden.json` is regenerated with `node capture.mjs > golden.json` and two
consecutive runs produce identical bytes. One wrinkle worth knowing: the
previously committed file was written through a PowerShell redirect and is
CRLF on disk, while a bash redirect writes LF. The path has no `.gitattributes`
entry and `core.autocrlf = true`, so git normalises both to the same LF blob —
the *committed* bytes are stable either way — but `ls -la` sizes will differ by
one byte per line between the two shells. Do not read that as a content change.

## 8. Not done / out of scope

- `SkyDome`'s `THREE.ShaderMaterial` + mesh + `onBeforeRender` wiring, and
  `envMaterial`. Host plumbing; belongs to a WGSL/render-integration slice.
- `fullscreen.js` (`SKY_VERT`, `fullScreenGeometry`, `blit`, `SkyPass`,
  `hdrTarget`, `floatTarget`) is a separate file and a separate slice
  (`06-parallel-port-plan.md` lists it at 101 lines).
- **`stars.rs` is nobody's slice right now and is only pinned indirectly.**
  `capture.mjs` already holds complete transcriptions of `skBlackbody`,
  `skAirmass`, `skStarLayer`, `skMilkyWay` and `skNightSky`, and this slice
  makes them *live* — `out.dome.sample` runs `skNightSky` — but no test pins
  the star functions individually, so a bug in one of them only surfaces if it
  happens to move a `sample` row. Worth assigning as a small slice: the
  transcriptions exist, only the emission and the Rust assertions are missing.
  Given §4, whoever takes it should re-read `stars.js` as GLSL text first and
  check every division and multiply chain in `stars.rs` before trusting either
  transcription. (`skStarLayer`'s `flux * (core + skirt) * max(0, tw)` and
  `skMilkyWay`'s `density * gain` are the shapes to look at.)
- `volumetrics` is another agent's live slice with its own golden set under
  `tests/sky_volumetrics/`. I did not touch `volumetrics.rs` or that
  directory; I did remove the dead duplicate transcription from
  `tests/sky/capture.mjs` (see §1).

## 9. Wiring

**None needed.** `apps/shmup/src/sky/mod.rs` already declares `pub mod dome;`
and `pub mod clouds;`. No `lib.rs`, `Cargo.toml` or `app.toml` change.
