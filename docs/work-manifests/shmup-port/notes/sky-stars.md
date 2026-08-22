# `sky/stars.js` — audit and pinning

Source: `C:/dev/Claude-of-Duty/src/sky/stars.js` (165 lines, `STARS_GLSL`).
Target: `apps/shmup/src/sky/stars.rs` (167 lines, already wired in).
Golden: `apps/shmup/tests/sky_stars/{capture.mjs,golden.json}` (330 KB).
Test: `apps/shmup/tests/sky_stars_port.rs`.

Written as a **separate** golden set (`sky_stars/`, not `sky/`) because two
sibling agents had just finished in `tests/sky/capture.mjs`,
`tests/sky/golden.json`, `tests/sky_port.rs`, `src/sky/dome.rs` and
`src/sky/clouds.rs`. None of those were touched, and neither was
`src/sky/mod.rs`, `lib.rs` or `Cargo.toml`.

`stars.rs` was nobody's slice for most of this port. It was flagged for
assignment by the `dome`/`clouds` audit (§8 of `notes/sky-dome-clouds.md`),
which observed that the star functions were exercised only *indirectly*, via
`dome::sample` → `skNightSky`, and never pinned individually — so a bug in one
surfaced only if it happened to move a `dome.sample` row.

**Verdict: the port was complete and, with one exception, faithful. The
exception is the exact defect class the brief said to hunt for, and it was
present identically on both sides.**

## 1. Method — and why the order matters

The two sibling slices between them found ten last-bit faithfulness defects,
every one of them present *identically* in the Rust and in a JS
"transcription" that had been written by reading the Rust. A second reading of
the same wrong thing proves nothing.

So this slice inverted the order, mechanically:

1. Read `stars.js` (and the two `noise.js` functions it calls) as GLSL text.
2. Write `tests/sky_stars/capture.mjs` from that text alone — `stars.rs` was
   not opened at all until the golden had been generated.
3. Diff the two, operator by operator, and investigate every disagreement
   from the algorithm before changing either side.
4. Second pass: re-read the GLSL grepping every `/` and every multi-factor
   product, then go looking for that operator in the Rust. (This is the pass
   that found four of the `dome`/`clouds` eight; here it found nothing the
   first pass had missed, which is itself worth recording.)

Then, as a third reading, the star transcriptions already sitting in
`tests/sky/capture.mjs` were compared against both. §3 is what that found.

## 2. Function-by-function inventory

| source | status before | now |
|---|---|---|
| `SK_STAR_TINT` (40) | ported | pinned |
| `skBlackbody` (43-55) | ported, **one defect** | fixed, pinned (22 rows) |
| `skAirmass` (58-61) | ported | pinned (17 rows) |
| `skStarLayer` (68-96) | ported | pinned (336 rows) |
| `SK_GAL_POLE`/`SK_GAL_CORE` (99-100) | ported | pinned exactly |
| `skMilkyWay` (102-127) | ported | pinned (300 rows) |
| `skNightSky` (133-161) | ported | pinned (600 rows) |

Nothing was missing, and nothing was filed under an out-of-scope
justification — unlike `dome` (two shader `main`s) and `volumetrics` (three of
four claims). `uStarParams` and `uCelestial` are uniforms and are correctly
taken as parameters (`StarParams`, `Mat3`).

## 3. The defect: `skBlackbody`'s normalising divide

`stars.js:54`:

```glsl
return c / max( 1e-4, dot( c, vec3( 0.2126, 0.7152, 0.0722 ) ) );
```

`stars.rs` had:

```rust
c.scale(1.0 / c.dot(Vec3::new(0.2126, 0.7152, 0.0722)).max(1e-4))
```

Multiplying by a rounded reciprocal is not the same operation as dividing.
Fixed to `c.div(Vec3::splat(...))`.

**The same defect is in `tests/sky/capture.mjs:814`** —
`scale(c, 1 / Math.max(1e-4, dot(c, [...])))` — so the two readings that
existed before this slice shared it, and any golden built from that pair would
have blessed it forever. That is the exact failure mode the brief described,
found once more, in the one file left unassigned. It is the whole argument for
transcribing from the source text.

Not fixed here, because `tests/sky/` is outside this slice's assigned paths and
a sibling agent had just finished in it. **For the orchestrator:**
`tests/sky/capture.mjs:814` should get the same `divS` treatment, and
`tests/sky/golden.json` regenerated. It moves `dome.sample` rows in the last
bits only, well inside that file's own `1e-9` pin, so it is a faithfulness fix,
not a failing test.

### A second, smaller divergence in the same file

`tests/sky/capture.mjs:819` writes GLSL `degrees(x)` as `(x * 180) / Math.PI`.
That is a different operation from `x * (180 / Math.PI)`, which is what
`stars.rs` does (`f64::to_degrees`) and what this slice's capture does.
Measured: it moves the worst airmass value by **8.2e-14** relative, peaking
right at the `96.07995` cliff. Same recommendation.

For the record, `180 / Math.PI` in V8 is **bit-identical** to the constant
Rust's `f64::to_degrees` multiplies by, so this slice's two sides agree
exactly.

## 4. Everything else, checked and found faithful

Every divide in the source, with where it lands in the Rust:

| `stars.js` | expression | verdict |
|---|---|---|
| 44 | `clamp(kelvin, 1200, 40000) / 100.0` | divide ✓ |
| 54 | `c / max(1e-4, dot(...))` | **was a reciprocal-multiply — fixed** |
| 60 | `1.0 / ( max(cosZenith,0) + ... )` | divide ✓ |
| 76 | `normalize(...)` | see §5 |
| 87 | `-( d * d ) / ( sigma * sigma )` | divide, negation before it ✓ |
| 88 | `-d / ( sigma * 3.4 )` | divide ✓ |
| 105/106 | `abs(lat) / 0.048`, `/ 0.165` | divide ✓ |
| 111 | `max(0, 1 - toCore) / 0.22` | divide ✓ |
| 140 | `abs(mw) / 0.16` | divide ✓ |

Every vector-by-scalar chain:

- `stars.js:95` `tint * ( flux * ( core + skirt ) * max( 0.0, tw ) )` — the
  source parenthesises the three scalars **together**, so this is **one**
  vector multiply. `tint.scale(flux * (core + skirt) * tw.max(0.0))` is right;
  splitting it into three `.scale()` calls would have been the defect, and this
  is the shape `notes/sky-dome-clouds.md` §8 specifically flagged for a look.
  It was already correct.
- `stars.js:126` `tint * ( density * gain )` — one multiply ✓.
- `stars.js:158` `vec3( 0.55, 1.0, 0.78 ) * 0.00030` — one ✓.
- `stars.js:160` `col * ( uStarParams.x * ext )` — one ✓.

Every scalar grouping (`83`, `90-91`, `107`, `121-122`, `137`, `145`) matches
left-to-right. The three-statement form `density = ...; density *= ...`
(`121-122`) is preserved as two statements, not folded.

## 5. `normalize` — the one judgement call

GLSL `normalize(v)` is reference-defined as `v / length(v)`;
`atmosphere.rs::Vec3::normalize` is `v * (1 / length(v))`; real hardware uses
`rsqrt` and agrees with neither. Unlike the §3 sites, the source does not
*write* a `/` here, so the "the source's operator is the specification"
argument does not straightforwardly apply.

`Vec3::normalize` is shared by five sky modules and was not flagged by either
sibling audit, so **it was left alone** and `capture.mjs` transcribes the same
convention, documented at both sites. The two sides therefore agree bit-exactly.
Measured cost of the choice: rewriting the capture to divide moves the worst
golden value by **2.0e-14** relative — four to five orders under the pin, so
nothing here depends on it. Flagged rather than changed; if it is ever
normalised across the crate it belongs in `atmosphere.rs`, in one edit.

The same reasoning applies to `gl_mix` (`a + (b - a) * t`, not the spec's
`a*(1-t) + b*t`), which `atmosphere.rs` already documents as a deliberate,
fixed crate-wide convention. The capture follows it.

## 6. Traps checked by name

- **`Float32Array`** — `grep` over `stars.js` and `noise.js`: no match. Nothing
  in this file stores anything; it is shader text end to end. `f64`
  throughout, matching the rest of `src/sky/`.
- **`sign` is not `signum`** — no `sign()` anywhere in `stars.js`. It uses
  `step()` once (`stars.js:72`), which the Rust turns into the equivalent
  comparison `h.x < 1.0 - keep`, correctly and with a comment.
- **`Math.hypot` is not `sqrt(x*x+y*y+z*z)`** — no `hypot` in the source. The
  one length here is GLSL `length( cross( dir, starDir ) )`, which *is*
  `sqrt(dot)`; `crate::jsmath::hypot3` would have been the wrong function, and
  is correctly not used.
- **Float associativity** — §3, §4. One defect, everything else already
  faithful.
- **Matrix storage order** — `uCelestial` is a `mat3` uniform fed from
  `THREE.Matrix3.elements`, which is **column-major**; `celestial::Mat3` stores
  logical rows. The test transposes once, in `mat3_from_three_elements`, and
  then checks that result against `Celestial::set_hour().celestial_matrix()` —
  so the transposition itself is pinned, not assumed. Reading the elements
  row-major would silently transpose the whole sky rotation.
- **Euler order** — none in this file.
- **Enum as a table index** — none.
- **Dead computation** — none in `stars.js`; every value computed is used.
- **A matching count is not proof / your comparator can be the bug** — the
  relevant form here is §3: two independent-looking readings that share a tidy.
  Also, every value table asserts that *both* arms of its early-out are
  populated, so a capture that silently started emitting all-zeros fails
  instead of passing.

## 7. What is pinned, and at what tolerance

`REL = 1e-9`, **relative**, floored only at `1e-300`.

This is stricter than `sky_port.rs`'s helper, which floors at `1.0`. It has to
be: a star's radiance is legitimately `1e-3`, a diffraction skirt is
legitimately `1e-40`, and a pin that floors at 1.0 says nothing about either.
The consequence is that a golden `0.0` demands an exact `0.0` from the Rust —
which is correct here, since every zero comes from an early return of
`vec3(0.0)` or a multiply by exactly `0.0`, and §8 shows both sides always take
the same branch.

Why `1e-9` and not tighter: every `+ - * /` and `sqrt` is IEEE-exact and
therefore **bit-identical** between V8 and Rust, so the arguments handed to
`exp`/`log`/`pow`/`sin`/`acos` are bit-identical too, and only each
transcendental's own rounding (≤1 ULP in both libms) diverges. The two places
that compound are `airmass` (an `acos` feeding a `pow` through a subtraction
that can cancel) and `milky_way`'s `exp(-pow(...))` (argument up to ~745 before
underflow); both stay near `1e-13`. Two measured calibration points, both from
deliberately rewriting one operator into an equally defensible alternative:
`normalize` div-vs-reciprocal moves the worst value `2.0e-14`, and the
`degrees` form moves the worst airmass `8.2e-14`. So `1e-9` sits four to five
orders above the arithmetic noise, and a real algebraic regression moves these
numbers by decimal *digits*.

### Tables

| table | rows | what the inputs cover |
|---|---|---|
| `blackbody` | 22 | both sides of the 1200 K and 40000 K clamps, the `t <= 19` (`b = 0`) arm and 0.0001 K past it (where the `clamp` lower bound bites), `t == 66` exactly and just past it — where **all three** of `r`, `g`, `b` switch arms — and the full 2600–22000 K range `skStarLayer` actually draws from |
| `airmass` | 17 | zenith to nadir, both sides of the `z = 96.07995` cliff, `cosZenith = 0` (the 37.9 peak) |
| `starLayer` | 300 | the three live layers (`stars.js:152-154`) × 50 directions (a 40-point golden-angle sphere cover + 10 named: zenith, nadir, both horizons, either side of the murk edge, both galactic poles, the core, two on-plane) × both uniform sets. 250 rows take the `exist` early-out and 19 are conspicuous stars; both counts are asserted |
| `starLayerExtreme` | 36 | `twinkle` at 0 / 1.5 / 4.0 with `keep = 1.0`. `max( 0.0, tw )` **cannot** clip under live uniforms (the largest `twinkle` `skNightSky` produces is `0.55 * 0.85 = 0.4675` against a bracket bounded by ±1.6, so `tw >= 0.252`), so the clipped arm is only reachable by driving it directly. With `keep = 1` every cell exists, so an all-zero row can *only* be the clip — which is what makes the assertion sharp |
| `milkyWay` | 300 | 50 directions × gains 0.16 / 0.064 × oct 2 / 3 / 5. `dome.js:235` only ever passes 3 or 5; oct 2 is there because it is the only input that exercises `max( 2, oct - 1 )` (`stars.js:116`) — a port writing `oct - 1` without the `max` agrees on every live input and diverges only there. 126 rows take the `band < 0.002` silhouette early-out, 174 do not |
| `celestial` | 3 | **genuine oracle** — real `Celestial` + `THREE.Matrix3` at hours 1.5, 4.0, 19.2, in THREE's own column-major element order |
| `nightSky` | 600 | 3 hours × 2 uniform sets × quality 0/1 (`(3, false)` / `(5, true)`, exactly `dome.js:235`) × 50 directions |
| `pureAirglow` | 86 | indices of the `nightSky` rows where the Milky Way is outside its silhouette *and* all three layers came back empty, so the sample is the airglow term alone. Gives a direct pin on `vec3( 0.55, 1.0, 0.78 )`, `0.00030` and the final `col * ( uStarParams.x * ext )` — literals that live inside the function body and are otherwise only implicit in a value table |

### Behavioural assertions, not just value comparisons

- `blackbody_is_normalised_to_unit_luminance` — the function's whole contract
  (`stars.js:8-9`: temperature and magnitude independent). Checks the Rec.709
  luminance of every row is 1. The `max(1e-4, ...)` floor on that divide is
  unreachable across the fit's whole domain (`r` never drops below 0.62, so the
  luminance never falls below ~0.075); it is defensive in the source and is
  preserved as such, not exercised.
- `night_sky_is_never_black_above_the_murk_and_always_black_below_it` — the
  source's own stated purpose for the airglow (`stars.js:157`). All 324
  above-horizon rows are strictly positive on all three channels; all 276
  below-horizon rows are exactly zero.
- `the_points_gate_is_the_only_thing_that_adds_the_star_lattice` — toggles
  `points` with `mwOctaves` **held at 5**. Comparing the golden's quality-0 and
  quality-1 rows directly does *not* isolate the gate: quality also moves
  `mwOctaves` 3 → 5, which re-rolls `skFbm3`'s clumps, and 46 of those 300
  pairs are legitimately darker at quality 1. Held fixed, 86 directions gain a
  star, 10 of them by more than 1.5×, and none loses light.

### Two source quirks, pinned by name

- **`airmass_pins_the_zenith_and_the_below_horizon_collapse`.** The doc comment
  says "1 overhead, ~38 at the horizon". Straight up the formula actually
  returns **0.99971** — the `0.50572 * (96.07995 - z)^-1.6364` term is still
  worth 2.9e-4 at `z = 0`. And past `z = 96.07995` deg the `max(0, ...)` floor
  makes the base exactly 0, `pow(0, -1.6364)` is `+Infinity`, and the airmass
  collapses to **exactly 0** — having *fallen* from 37.9 at the horizon through
  1.7 on the way. Nonphysical, and harmless only because `skNightSky`
  multiplies by `smoothstep(-0.03, 0.10, dir.y)`, which is already 0 there.
  Ported as written.
- **`blackbody_2600k_is_warmer_than_the_source_comment_claims`.**
  `SK_STAR_TINT`'s 12-line doc comment (`stars.js:28-39`) justifies the 0.11
  mix by asserting 2600 K normalises to "roughly 1.6 / 0.7 / 0.25" and so lands
  "under 0.15 HSV saturation". The code returns **2.0611 / 0.7697 / 0.1565**,
  and the resulting tint's HSV saturation is **0.1876**, not under 0.15. The
  comment's arithmetic corresponds to some earlier normalisation. A comment is
  not behaviour, so the code is what is ported and what is pinned; changing the
  constant to satisfy the prose would change the render. Recorded, not "fixed".

## 8. Discontinuity margins — measured, not assumed

Four branches here are decided by a computed float. Following the method
`sky_volumetrics_port.rs` established for its `step()` cliff, each fixture's
margin was measured:

| branch | site | min margin over the fixtures |
|---|---|---|
| `max(0, 96.07995 - z)` → `pow(0, -1.6364)` = +inf | 60 | 0.81 deg in `z` |
| `band < 0.002` early-out | 108 | 2.3e-4 |
| `t <= 66` / `t >= 66` blackbody arms | 47-52 | 0.80 in `t` |
| `t <= 19` blackbody arm | 52 | 7.0 in `t` |

against arithmetic noise around `1e-14`: twelve to thirteen orders of margin.
The `airmass` fixtures deliberately straddle the cliff at ±0.9 deg rather than
landing on it — a golden pinned *on* a discontinuity is brittle to a 1-ULP libm
difference, and this one is violent (finite on one side, exactly 0 on the
other).

The fifth branch, `step( 1.0 - keep, h.x )` (`stars.js:72`), carries no risk at
all: `h` comes out of `skHash33`, which is pure `* + fract`, so both sides get a
bit-identical `h.x` and always take the same arm. Same for the `kelvin` fed to
`skBlackbody`'s branches — reached through one `pow`, whose 1-ULP uncertainty is
thirteen orders below the 0.80 margin.

## 9. Reproducibility

`golden.json` is regenerated with `node capture.mjs > golden.json` from
`apps/shmup/tests/sky_stars/`; two consecutive runs are byte-identical (checked
with `cmp`). Node 24, and the only import from the source tree is
`celestial.js` + three, by absolute `file:///` URL — the pattern
`tests/sky/capture.mjs` established. `C:/dev/Claude-of-Duty` was not modified.

## 10. Wiring, and things outside this slice

- **No wiring needed.** `apps/shmup/src/sky/mod.rs` already declares
  `pub mod stars;`. No `lib.rs`, `Cargo.toml` or `app.toml` change. The new test
  target `tests/sky_stars_port.rs` is auto-discovered by Cargo, and
  `tests/sky_stars/` is a data directory, not a target.
- **Not built or tested** (per the fan-out brief): no `cargo` command of any
  kind was run. The golden is real — it comes from running JavaScript, which
  needs no Rust — but the Rust test has never been compiled.
- **For the orchestrator, three things outside the assigned paths:**
  1. `tests/sky/capture.mjs:814` carries the same `skBlackbody` reciprocal
     multiply fixed here (§3), and `:819` the `(x * 180) / Math.PI` form
     (§3). Both are last-bit; both should be brought into line and
     `tests/sky/golden.json` regenerated.
  2. `src/sky/celestial.rs:82-87`'s doc on `Mat3` says the starfield is "not
     ported in this slice". That is now false — `stars::night_sky` is its
     consumer and `Mat3::mul_vec3` exists specifically for it.
  3. `atmosphere.rs::Vec3::normalize` is a reciprocal-multiply where GLSL's
     reference definition divides (§5). Deliberately left alone; it is a
     five-module shared helper and a crate-wide decision, not a stars one.
