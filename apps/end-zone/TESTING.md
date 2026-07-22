# End Zone — testing

Run the app's tests with `cargo test -p axiom-end-zone`. The frontend is pure
and native-testable; the simulation and score-attack drive are deterministic and
driven headlessly.

## Simulation & framework (unchanged deterministic core)

- `tests/determinism.rs` — the full showcase replays bit-for-bit (state digest,
  events, trajectory, possession, intents, camera modes/poses); a second seed
  changes only seeded presentation variation.
- `tests/football.rs`, `tests/ai.rs`, `tests/camera.rs`, `tests/field.rs` —
  the ball state machine, the AI pipeline, the camera director, and the
  field coordinate system.
- `tests/ai_situation.rs`, `tests/ai_coverage.rs`, `tests/ai_engagement.rs` —
  the foundational player-AI pass: the derived `BallSituation` + scramble
  detection, decision determinism and commitment locking; a scrambling
  quarterback becoming the priority, non-duplicated pursuit responsibilities,
  deep leverage, predictive catch-point / intercept / tackle-angle reactions and
  the shared loose-ball response; and the line engagement — an offensive lineman
  squaring and anchoring, a block that doesn't oscillate, the rush advantage
  building, the eventual shed/sack, and a strong blocker delaying it.
- `tests/controls.rs` — a zero stick reproduces the scripted showcase
  bit-for-bit; user steering only overrides the ball holder's AI intent.

## Locomotion (distance-driven, planted-foot)

- `tests/locomotion.rs` — direct tests for `presentation::locomotion`:
  - **Leg IK** — the two-bone solver reaches reachable ankle targets (FK
    round-trips the solve), bends the knee forward (never inverts), and clamps
    unreachable targets without stretching; all outputs finite.
  - **Distance-driven phase** — identical displacement advances the phase
    identically; zero displacement does not advance it; **blocked movement**
    (requested velocity but zero actual displacement) does not cycle the legs;
    faster actual movement advances faster; teleport/reset does not advance the
    gait; replaying the same displacement history is bit-identical.
  - **Stride / cadence** — both stay within configured bounds; sprint stride >
    jog stride; startup expands stride over time; stopping converges to a stable
    idle settled on a foot; sharp turns shorten the stride.
  - **Foot locking** — a planted foot holds its world position (zero slide) while
    the body advances; the lock error (foot reaches its target) stays small and
    planting alternates deterministically; airborne / teleport invalidate both
    locks; every generated joint and foot position is finite.
  - **Pose composition** — the carry hold does not remove lower-body locomotion;
    fall/action overrides suppress locomotion; composition is deterministic for
    the same input and gait; a locomotion state routed to `override_pose`
    defensively yields the neutral base.
  - **Determinism** — a full scripted showcase sequence (acceleration, sprint,
    contact, turning, stopping, reset, carrying, tackle) replays the whole
    per-player pose + gait history bit-for-bit through the real `ShowcaseRun`.

  The authoritative-movement-vs-animation split (animation never mutates the
  sim) is still guarded by `tests/camera.rs`, which the locomotion animator
  obeys by construction (it reads only the snapshot).

## Score-attack drive

- `tests/drive.rs` — over the real simulation: a fresh run starts 1st & 10 with
  zeroed stats; an unassisted run turns the ball over on downs and ends (the
  dead-ball play clock bounds every play); the run summary matches the final
  drive state; a fresh run resets all statistics; a run replays identically from
  the same config; and `DriveState::resolve` awards the expected
  first-down / touchdown / run-over events.
- `tests/frontend_hud.rs` — `HudView` from authoritative `DriveState`: down
  display, yards-to-go derived from state, first-down reset, line-to-gain
  movement, touchdown scoring, bounded heat, `GOAL` near the end zone, and the
  HUD shape carrying only the five required read-outs.

## Frontend

- `tests/frontend_flow.rs` — the six-state flow: title confirm starts gameplay
  immediately, pause/resume preserves the run, restart launches fresh,
  settings/controls return to pause, return-to-title disposes the run, game over
  offers play again / return to title, play again uses a fresh seed, and
  identical input scripts replay identically.
- `tests/frontend_pause.rs` — over the composed shell: the simulation does not
  advance while paused, resume produces no time jump, restart rebuilds a fresh
  simulation, and return-to-title disposes the run.
- `tests/frontend_settings.rs` — valid defaults, bounded volume, screen-shake
  driving real camera amplitude (`OFF` = 0, `LOW` scales), reduced motion
  suppressing nonessential movement, a persistence round-trip, safe fallback on
  malformed input, and no removed setting in the persisted shape.
- `tests/frontend_teams.rs` — exactly two fixed teams, distinct and valid,
  always used by the run bootstrap, with no user-facing selection.

## Architecture & reduction guards

- `tests/architecture.rs` — the deterministic core is browser-free and
  wall-clock-free, no placeholder/console macros or junk-drawer modules, no
  `unwrap`/`expect` in production, every core source file stays under 300 lines,
  and no engine layer/module depends on this app.
- `tests/frontend_reduction.rs` — precise, comment-stripped source checks that
  the removed concepts do not return (`MainMenu`, `TeamSelect`, `MatchSetup`,
  `Credits`, `TeamCard`, `MatchLaunchConfig`, difficulty/camera/game-speed
  settings, control rebind, attract), that the deleted screen files are gone,
  and that exactly the six screen states exist.

## Browser verification

The `wasm32` presentation arm (the live `wgpu`/`web-sys` render) is verified in
a real browser: build with `make end-zone-build`, serve `apps/end-zone/web`, and
drive it with `scripts/playwright_controller.py`. Headless browsers need
`?backend=canvas2d` (the WebGL2 path lacks `VERTEX_STORAGE` there).
