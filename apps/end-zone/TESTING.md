# End Zone — testing

All app tests are native (`rlib`) and deterministic; nothing here needs a
browser or GPU.

```sh
cargo test -p axiom-end-zone      # the app's own suites
cargo test --workspace            # everything (CI parity)
cargo xtask check-architecture    # Layer/Module/App laws
cargo fmt -p axiom-end-zone -- --check
```

## Deterministic replay expectations

`tests/determinism.rs` runs the complete showcase twice with the same seed and
fixed-step count and compares, **bit-for-bit**:

- the final authoritative state digest (`SimState::digest`, f32 bit patterns);
- the ordered simulation event stream;
- per-tick football trajectory samples;
- the possession history;
- every player's per-tick AI intent history;
- the camera mode history and per-tick final camera poses.

A second-seed run proves the split: the authoritative simulation and event
stream are seed-independent, while the explicitly seeded presentation
variation (camera impulse phases) changes. Approximate equality is used only
where a test checks a derived quantity (an epsilon of `1e-4` on conversions);
replay comparisons are exact.

## Direct subsystem tests

- `tests/field.rs` — dimensions (120 × 53⅓ yd), midfield `Z = 0`, end-zone
  boundaries, reversible yard↔world conversion in both drive directions,
  offense-relative mirroring, and **finite-geometry validation** of every
  generated field piece and merged marking/number mesh (finite values,
  in-range triangle indices).
- `tests/football.rs` — held ball follows the sim carry socket exactly;
  bit-exact deterministic release; identical trajectories across runs; flight
  advances every fixed tick with no teleport; deterministic spin; catch
  evaluation success/failure cases (volume, timing, action state); possession
  event ordering; the loose → grounded → incomplete path (driven by a
  data-only roster change).
- `tests/ai.rs` — identical intents per seed/inputs; deterministic route
  progress; route mirroring across drive direction; configured reaction delay
  obeyed (and shortened via data); acceleration/turn-rate limits on the
  steering update; behavior changed through roster data with unchanged code;
  stable id ordering.
- `tests/camera.rs` — fixed-step camera proofs: throw → PassFlight,
  catch attempt → CatchResolve, transfer → BallCarrierFollow, ground impact →
  Impact + impulse; impulses decay EXACTLY to zero (the final sample is the
  zero envelope); after expiry the final pose equals the impulse-free base
  bit-for-bit (shake never drifts the rig); replay-identical poses. Plus the
  seeded-effect proofs: bounded pools, clamped amplitudes, identical effects
  for identical events, full expiry, and sim-inertness of all presentation
  input (forced cameras + debug toggles leave the digest unchanged).
- `tests/architecture.rs` — app hygiene: `app.toml` lists exactly the consumed
  layers/modules; the deterministic core is browser-free, wall-clock-free, and
  ambient-randomness-free (only `web.rs` touches the DOM); no placeholder or
  console macros; no `unwrap()`/`expect(` in production paths; no junk-drawer
  modules; no engine layer/module depends on this app; every source file stays
  under the repo's 300-line app-placement heuristic.

## Engine regression added with this app

The showcase exposed a generic `axiom-physics` defect (an immovable-immovable
contact NaN'd the solver and permanently wedged the world via step rollback).
The fix lives in the engine with direct tests:

- `modules/axiom-physics/src/contact_solver.rs` — unit test
  `immovable_pair_contact_is_a_finite_no_op`;
- `modules/axiom-physics/tests/determinism_poison.rs` —
  `overlapping_kinematic_bodies_never_wedge_the_world`.

## Browser verification

The wasm arm is verified by serving the app (`make end-zone-build`,
`make end-zone`) and driving it with the repo's Playwright controller
(`uv run scripts/playwright_controller.py goto/console/screenshot`), checking
for console errors and capturing the formation, pass-flight, post-catch, and
ground-impact moments.
