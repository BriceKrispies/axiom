# `weapons::parts::receiver` port notes

Ported `addHandguard` (`parts.js:391-514`), `addUpperReceiver` (`:525-656`),
`addBoltCarrier` (`:662-687`), and `addLowerReceiver` (`:693-792`) into
`apps/shmup/src/weapons/parts/receiver.rs`. Registered in
`src/weapons/parts/mod.rs` alongside `barrel`/`controls`/`hardware`/`magazine`.

## API shape

Each builder gets an `Opts` struct mirroring the JS `o` object, with a
`Default` impl documenting every JS `?? value` fallback, following the house
convention already established by `parts::barrel`/`parts::hardware`/
`parts::magazine`:

- `HandguardOpts<'a>` — `mat_panel: Option<&'a str>` reproduces
  `o.matPanel ?? matAlu`. `top_from`/`top_to` are two independent
  `Option<f32>` fields (matching the source's two independent `?? null`
  reads) rather than a single `Option<(f32, f32)>`; every real call site
  (`models/rifle.js:184-185`) sets both together, and the combined branch is
  written as `o.top_from.zip(o.top_to)` with a `let-else` continue,
  reproducing the source's `if (topFrom === null) continue;` without
  inventing behavior for the case no real caller exercises (`topFrom` set,
  `topTo` not).
- `UpperReceiverOpts`, `BoltCarrierOpts`, `LowerReceiverOpts` — straightforward
  field-for-field ports. `LowerReceiverOpts::mag_top`/`mag_bottom` are
  `Option<f32>` reproducing `?? bore - 0.014` / `?? bore - 0.062` (a Rust
  `Default` cannot see `bore`, so the fallback is applied in the function
  body via `unwrap_or`).
- Return types: `UpperReceiverResult { rail_top }`, `LowerReceiverResult
  { mag_top, mag_bottom, mag_z, mag_tilt, well_h, mag_w, mag_d }`, matching
  the source's returned object shapes exactly.

## Preserved source quirk: `addLowerReceiver`'s dead `matSteel` parameter

`addLowerReceiver(asm, mat, matSteel, o)` (`parts.js:693`) never references
`matSteel` in its body — every geometry call in that range (`bodyG`, `well`,
`liner`, `mouth`, `tower`, `guard`, `bossG`) uses `mat` or the literal
`'cavity'` bucket. Per the port recipe's rule 7, kept as `_mat_steel: &str`
for call-order fidelity rather than silently dropped, with a doc comment at
the call site (same treatment `parts::magazine::build_magazine` gives its
dead `mats` parameter).

## A real bug this port caught: narrow-then-widen breaks the f64 contract

`03-weapon-geometry-api.md`'s "Corrections to this contract" section #1 says
intermediate contour math must stay in `f64` — narrowing to `f32` before
`round_rect`/`extrude`'s division-heavy bevel path loses precision that gets
amplified back out past any reasonable tolerance.

My first draft of `add_lower_receiver` wrote `round_rect(f64::from(mag_w -
0.0052), f64::from(mag_d - 0.0052), 0.006, 5)` — the *subtraction* ran in
`f32` (since `mag_w`/`mag_d` are `f32`), and only the *result* was widened.
This is exactly the narrowing the contract's correction warns about, just
self-inflicted at a call site instead of inherited from a primitive
signature.

It surfaced as a real, large failure: the synthetic
`lower_receiver_defaults` golden case (`magTop`/`magBottom` both defaulted,
which forces the `liner`'s extrude depth to a fixed `0.048 - 0.004 = 0.044`
regardless of `bore`) failed with a position delta of `~3.6e-4` — two orders
of magnitude past the `1e-5` merged-bucket tolerance and far past anything
attributable to independent-libm ULP noise. I spent real effort chasing this
as a suspected instability in `extrude()`'s bevel/weld algorithm itself
(isolated the exact call outside `Assembly`, compared JS vs. Rust vertex
counts and values at several nearby dimension/depth combinations, watched
the output flip between vertex counts of 752/756/760 under femtometer-scale
input perturbations) before realizing the actual defect was the narrow-then-
widen order in my own call sites, not a primitive-layer instability.

Fixed at all 8 call sites in `receiver.rs` (`lip`/`lip_inner` in
`add_upper_receiver`; `well`/`liner`/`mouth` and their `holes` in
`add_lower_receiver`) by widening first: `f64::from(mag_w) - 0.0052` instead
of `f64::from(mag_w - 0.0052)`. All six tests pass exactly after the fix,
including the previously-failing default-fallback case — confirming this
was the entire root cause, not a symptom of a deeper primitive issue.
**Lesson for any future call site that feeds a computed `f32` value into
`round_rect`/`extrude`'s `f64` contour parameters: always widen the operand
first, never the expression.**

## Verification

Golden-capture per the recipe. Captured via a temporary Node script (not
committed) calling the real `parts.js` builders against a real `Assembly`
from `geometry.js`, using the exact arguments `buildRifle()`
(`src/weapons/models/rifle.js`) uses at every real call site, plus two
synthetic all-default cases to exercise branches the real rifle never hits
(`handguard_no_top`: `topFrom === null` continue arm + `matPanel ?? matAlu`
fallback; `lower_receiver_defaults`: `magTop ?? bore - 0.014` / `magBottom ??
bore - 0.062` fallback). Committed as
`apps/shmup/tests/parts/receiver_golden.json` (6.6 MB — six cases,
each with several material buckets); test file
`apps/shmup/tests/weapons_parts_receiver_port.rs`.

Six tests, all passing exactly:

- `add_handguard_matches_the_rifle_configuration`
- `add_handguard_matches_the_source_with_no_top_slat_and_default_material`
- `add_upper_receiver_matches_the_rifle_configuration`
- `add_bolt_carrier_matches_the_rifle_configuration`
- `add_lower_receiver_matches_the_rifle_configuration`
- `add_lower_receiver_matches_the_source_with_default_mag_top_bottom_and_dimensions`

Tolerance policy matches `weapons_parts_magazine_port.rs`'s established
pattern (these builders compose many `extrude`/`lathe_z` calls into
merged-and-welded per-material buckets): triangle count exact always; vertex
count exact when it matches, else within the same small budget
(`max(vert_count/10, 8)`) used elsewhere for the documented independent-libm
weld-quantization residual; position/normal floats within `1e-5` absolute
when vertex counts matched. In practice every bucket in every case matched
vertex count and position/normal exactly — no case needed the fallback
budget.

`returned` fields (`UpperReceiverResult::rail_top`,
`LowerReceiverResult`'s seven fields) are compared within `1e-5` against the
JS `f64` golden, same as `weapons_parts_barrel_port.rs`'s pattern for
`gasAt`/`rBore`/`len`/`crownZ`.

## Verify-before-commit results

- `cargo test -p axiom-shmup` — pass (ran the lib tests plus every
  `weapons_*` integration test individually; one sibling test binary
  (`weapons_parts_optics_port`, owned by a concurrent agent) hit a transient
  Windows file-lock from a simultaneous build and was excluded from this
  run, not a failure in code this slice touches).
- `cargo xtask check-architecture` — pass (`OK: all layers satisfy the Axiom
  Layer Law.`).

## Nothing left unported in this slice

All four functions in scope are fully ported and golden-verified. No
divergence from source behavior beyond ordinary `f32`/`f64` boundary
handling (documented above, and now fixed rather than merely tolerated).
