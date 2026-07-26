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
- `tests/ai_overseer.rs`, `tests/ai_overseer_sim.rs` — the defensive overseer:
  the tactical scoring reads football evidence (base when nothing's clear,
  pressure on a held stable pocket, suppressed pressure into a deep-TD danger,
  deep/middle/outside/contain/bracket cues, the bracket's personnel cost), the
  directive carries no movement command (defenders never teleport), decisions
  replay identically, possession memory resets at the boundary; and through the
  real sim — throw→catch-point→swarm, rollout→contain→run-response, the run
  response's distinct pursuit roles, the emergency override at the goal, mode
  hysteresis, the readable exposed-region tradeoff, and a stable bracket target.
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

## The decision-window attempt loop

- `tests/attempt_loop.rs` — the prototype's load-bearing guarantees, over the
  real simulation: the attempt opens pre-snap and snaps itself; the play
  develops for ~1 s before anything is asked; a window opens within the deadline
  on **every** seed; the window dilates time without pausing; a declined window
  closes back to full speed with the play still live; later windows are shorter;
  the window budget is respected; a press outside a window (or a second press)
  is rejected as stale; each of the three keys throws to the receiver it names;
  a moving receiver is thrown a lead; scrambling hands over the quarterback and
  the defense sees a runner immediately; the stick is ignored while the play
  develops; **declining every window usually ends in a sack**; ten consecutive
  attempts resolve with no skipped or repeated index; and a reset leaves no
  stale marker, throwable, possession, duplicated entity or time dilation.
- `tests/autopilot.rs` — the headless driver AND the balance instrument. Ten
  attempts with no human; every attempt offers a window; a session replays
  bit-for-bit; **no read is a trap or a gimme** (each completes between 15% and
  95% of the time — this is the check that caught the original 22-yard post at
  4%); and the reads come open in order (short before deep).
  `patience_sweep -- --ignored --nocapture` prints yards/attempt, disaster rate
  and per-read hit rate for an impatient, a balanced and a greedy quarterback:
  the numbers that say whether waiting is a real trade.
- `tests/frontend_hud.rs` — `HudView` from the live attempt loop: the attempt
  counter, the session line, the three numbered read prompts and the scramble
  caption, a draining window timer, the result card with signed yards, and the
  guarantee (by exhaustive destructuring) that **the prompt never reports how
  open a read is**.

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
