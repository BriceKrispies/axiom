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
- `tests/controls.rs` — user-control proofs: a zero stick reproduces the
  autonomous showcase exactly (digest + events); the stick steers only the
  offensive ball holder and respects the archetype's speed limit; the
  contextual primary action snaps early, orders the throw, and restarts a
  finished play.
- `tests/architecture.rs` — app hygiene: `app.toml` lists exactly the consumed
  layers/modules; the deterministic core is browser-free, wall-clock-free, and
  ambient-randomness-free (only the `src/web/` edge touches the DOM); no
  placeholder or console macros; no `unwrap()`/`expect(` in production paths;
  no junk-drawer modules; no engine layer/module depends on this app; every
  core source file stays under the repo's 300-line app-placement heuristic.

## Frontend shell tests

The menu shell is pure and native-testable (see `FRONTEND.md`); its suites
drive `FrontendApp`/`EndZoneShell` with synthetic input frames:

- `tests/frontend_flow.rs` — the explicit screen machine: happy path to a
  launched match, consistent backward cancel, pause/resume, the
  return-to-menu dialog, settings returning to its exact origin with focus
  restored, credits, attract entry/exit (and never deep in the menus), and
  identical-input replay determinism.
- `tests/frontend_focus.rs` — deterministic focus (first-enabled default,
  grid movement, disabled skipping, memory restore), hover/pointer
  activation, the navigation repeat delay + cadence, edge-triggered confirm,
  gamepad token translation, stable device hints, and modal focus
  confinement.
- `tests/frontend_teams.rs` — six complete original teams (unique identity,
  bounded ratings, valid emblems, distinct strength profiles), rating→roster
  scaling as pure data, total league lookup, the opponent cursor never
  reaching the locked player team, and selection persistence.
- `tests/frontend_settings.rs` — all five categories carry real fields;
  bounded stepping/cycling; the working-vs-committed editor (APPLY commits +
  requests persistence, RESET DEFAULTS resets the working copy, dirty BACK
  raises the discard dialog with the safe option focused); live preview via
  the theme fingerprint; reduced-motion transition swaps; rebind capture
  (rebinds, times out, leaves committed bindings untouched) and conflict
  reporting.
- `tests/frontend_persistence.rs` — versioned encode/decode round-trips,
  per-field fallback on corrupt values, hostile-input safety, the
  distinct-teams invariant, binding-token validation, the
  `MemoryStore` load/save/clear cycle, and legacy salvage.
- `tests/frontend_launch.rs` — `MatchLaunchConfig` validation, difficulty /
  camera / effects profiles as pure data (including the accessibility
  scales), deterministic game-speed pacing, seed-exact match reproduction,
  and roster shaping from the selected teams.
- `tests/frontend_shell.rs` — the composed shell: menus run the ambient
  showcase, launch swaps in the real match, pause freezes the authoritative
  digest exactly, restart replays the frozen config byte-for-byte,
  return-to-menu restores the ambient loop, attract runs the live sim, and
  menu input never reaches the background simulation.

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
