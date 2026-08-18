# Port notes — `weapons/parts/controls.rs`

Source: `C:\dev\Claude-of-Duty\src\weapons\parts.js` — `selectorPart` (:795-828),
`triggerPart` (:838-866), `addPistolGrip` (:876-956), `addCarbineStock`
(:962-1071), `chargingHandlePart` (:1781-1854), `addForeGrip` (:1857-1879).

Target: `apps/shmup/src/weapons/parts/controls.rs`.
Test: `apps/shmup/tests/weapons_parts_controls_port.rs`, golden at
`apps/shmup/tests/parts/controls_golden.json`.

## What was ported

All six functions, faithfully, including:

- `selectorPart`'s dead `matSteel` parameter — declared, never read in the
  body (the return is always `{ geo, mat: matAlu }`). Kept as `_mat_steel`
  for call-order fidelity, same convention as `parts::magazine`'s dead
  `mats` parameter.
- `chargingHandlePart`'s `wR.rotateZ(0)` — a literal no-op in the source too.
  Ported as a real (no-op) `rotate_z(&mut w_r, 0.0)` call rather than dropped,
  to keep call order recognisable against the source.
- `addCarbineStock`'s detent-notch loop `break` when
  `z > zRear - 0.02` — a real early exit, covered by a dedicated test case
  sized to trigger it after the first iteration.

## Local helpers

Added `translate`/`rotate_y`/`rotate_z` to `controls.rs`, computing the
rotation `sin`/`cos` in `f64` and building the `Mat4` by hand — the same
precision convention `parts::magazine`'s `rotate_x`/`rotate_y` established
(not `axiom_math::Quat::from_axis_angle`, which only accepts `f32` and
truncates the angle before the trig). `rotate_z` has no prior sibling to copy;
it was hand-derived from the same right-handed-rotation convention as
`rotate_x`/`rotate_y` and cross-checked against `parts::barrel`'s
already-tested `Quat`-based `rotate_z` (same result, different construction).

All point lists fed to `extrude` are `Vec<[f64; 2]>`, built from `f32` option
fields widened at the point-list boundary (`f64::from(...)`) — the same
convention `parts::magazine` uses, not full-`f64` computation of every
intermediate (which would be truer to `03-weapon-geometry-api.md`'s
"Corrections" wording but would diverge from the already-shipped, already-
golden-verified sibling convention).

## Options structs

`PistolGripOpts` (`len`/`w`/`angle`/`y`/`z`, all with real JS `??` defaults),
`CarbineStockOpts` (`bore`/`z_front`/`z_rear` mandatory, `y: Option<f32>`
reproducing `o.y ?? bore - 0.012`), `ForeGripOpts` (`len`/`angle` real
defaults, `y`/`z` zeroed by `Default` only for struct-update convenience, not
a JS default — no real call site omits them, and JS would produce `NaN` if it
did).

## The one real residual — diagnosed, not papered over

Every bucket that merges an `extrude()` piece with `box_geo()`/`blob()`
pieces comes back with a vertex count off by a handful (2-10 vertices) despite
an **exactly matching triangle/index topology**:

- `trigger_default` (blade extrude + 6 serration boxes)
- `pistol_grip_*`'s `polymer` bucket (core extrude + beaver/cap blobs)
- `carbine_stock_*`'s `polymer` bucket (shell extrude + cheek/scallop blobs)
- `charging_handle` (box_geo/rod_z interleaved with extrude throughout)

This was verified, not assumed:

1. `tri_count()` matches the golden's `index.len() / 3` exactly in every
   affected case (checked directly, not just inferred from the vertex-count
   pass/fail).
2. A bounding-box check on `trigger_default`'s raw positions (not committed —
   a throwaway debug assertion, per the port recipe) landed within `1e-8` of
   the golden's bounding box on all six components — the shape and placement
   are correct, not merely plausible.
3. The **unaffected** buckets in the exact same calls prove the cause:
   `carbine_stock_*`'s `alu` bucket (`tube_z`/`lathe_z`/`box_geo` only, no
   `extrude`) and `rubber` bucket (`blob`/`box_geo` only), and
   `pistol_grip_*`'s `rubber` bucket (`blob`/`box_geo` only), all match
   **exactly** — vertex-for-vertex, in the same test run, same merge/weld
   machinery. Only a bucket that merges `extrude()`'s bevelled output with
   `box_geo`/`blob`'s rounded-corner output shows the residual.

This is the same root cause `weapons_geometry_primitives_port.rs`'s
`assert_geo_topology_matches` and `primitives::extrude`'s module doc already
diagnose: `get_bevel_vec`'s corner construction divides by a near-zero
denominator, and independent `f64::sin`/`f64::cos` implementations (Rust's
libm vs V8's) can nudge the result past the `1e-6` weld-quantization grid —
here tipping the tie for a handful of `extrude` vertices that land close to a
neighbouring `box_geo`/`blob` piece's vertices, rather than at a
`round_rect`-style tangent corner within one contour. Same mechanism, new
trigger condition (cross-piece proximity instead of within-contour tangency).

Per the port recipe ("measure the residual and state the cause; do not
silently widen or drop to topology-only"), the affected buckets use a new
`assert_geo_topology_matches`/`assert_bucket_topology_matches` pair in the
test file (triangle count exact; vertex count exact-or-bounded to
`max(10%, 8)`, the same budget `weapons_geometry_primitives_port.rs` and
`weapons_parts_hardware_port.rs` already use for this class), while every
bucket that doesn't mix `extrude` with `box_geo`/`blob` keeps the strict
exact-count, `1e-5`-tolerance comparison.

## Verification

- `cargo test -p axiom-shmup` — pass (all test binaries green,
  including the new `weapons_parts_controls_port` with 11 tests).
- `cargo xtask check-architecture` — pass, exit 0.

## Nothing was left unported

All six functions in this slice are fully ported with no gaps, no
topology-only fallback beyond the diagnosed residual above, and no dropped
comments.
