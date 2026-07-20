# End Zone — agent routing

This is the routing file for `apps/end-zone` (`axiom-end-zone`), a
composition-leaf Axiom app: an arcade **score-attack football** game built on a
reusable, deterministic play-simulation framework that lives entirely inside
this app. Read the repo-root `CLAUDE.md` first — the Layer/Module/Branchless/
Coverage laws still apply. This file tells you **where** things live so you fix
them at the right boundary instead of guessing.

## Start here — pick your doc

| If you're changing… | Read | Then look in |
|---|---|---|
| The big picture / boundaries / data flow | `ARCHITECTURE.md` | `src/lib.rs` (module map) |
| Menus, screen states, HUD shape, teams, persistence | `FRONTEND.md` | `src/frontend/*` |
| What buttons do (keyboard / gamepad / touch) | `CONTROLS.md` | `src/frontend/bindings.rs`, `src/web/touch.rs` |
| Settings (the three: volume / shake / reduced-motion) | `SETTINGS.md` | `src/frontend/settings.rs`, `src/launch.rs` |
| How to test / what's covered / browser verify | `TESTING.md` | `tests/*` |

If a change spans several of these, skim `ARCHITECTURE.md` for the boundary map
before touching code — the app is deliberately cut into one-way layers and a fix
in the wrong one is debt.

## The one-way flow (which file owns what)

```text
input (keys → DeviceFrame → InputState)
  → fixed-step 60 Hz simulation          src/state.rs, src/ai/*, src/football/*, src/player/*
  → score-attack drive loop              src/drive.rs        (downs / score / heat / game over)
  → ordered SimEvents                    src/events.rs
  → immutable PresentationSnapshot       src/presentation/snapshot.rs
  → camera director + juice              src/camera/*, src/presentation/*
  → HUD view model                       src/presentation/hud.rs
  → Axiom scene/render submission        src/scene.rs, src/scene_sync.rs, src/web/*
```

- **Data** (`src/data/*`, `src/config.rs`, `src/identity.rs`) — declarative
  teams, rosters, archetypes, formations, plays, and every tuning knob
  (`src/data/tuning.rs`). Behavior changes are usually **data edits here**, not
  new code.
- **Simulation** — pure function of the command stream, one 60 Hz tick, no wall
  clock, no scene/render types. The seed varies presentation only.
- **Presentation** — consumes the immutable snapshot + events; **must not**
  mutate the sim (guarded by `tests/camera.rs`).
- **Composition / platform edge** (`src/app.rs`, `src/shell.rs`, `src/web/*`) —
  the only place browser/nondeterminism is allowed; `src/web/` is wasm32-only.

## Where does my change go? (quick router)

- **New gameplay rule / down logic / heat / game-over** → `src/drive.rs`. It
  measures play outcomes and keeps score-attack bookkeeping; it adds no football
  rule the sim doesn't already produce.
- **Player/defender behavior** → data first (`src/data/tuning.rs`, archetypes),
  then the AI stage that owns it: `src/ai/assignment.rs` (route→waypoints),
  `src/ai/{brain,offense,defense}.rs` (intent), `src/player/controller.rs`
  (execution — the *only* writer of player movement).
- **Ball flight / catch / tackle / fall** → `src/football/*` (state machine +
  flight through real physics), `src/player/contact.rs` (tackle/block/fall).
- **Running / walking / foot-plant animation (skating)** →
  `src/presentation/locomotion/*` (the distance-driven, planted-foot animator:
  `gait.rs` phase, `foot.rs` plant locking, `leg.rs` two-bone IK, `pose.rs`
  whole-body pose, `mod.rs` composition), tuned in
  `src/data/locomotion_tuning.rs`. Non-locomotion (throw/catch/fall) poses are
  `src/player/animation.rs::override_pose`. Authoritative movement owns world
  position; the animator only explains it — it never mutates the sim. See
  `ARCHITECTURE.md` § Locomotion.
- **Camera feel** → `src/camera/*` (modes in `modes.rs`, springs in `rig.rs`,
  shake in `impulse.rs`). Driven only by events + snapshot.
- **Screen for a visible thing (dust, ring, flash, streak)** → juice, spawned
  only by events: `src/presentation/{juice,particles}.rs`.
- **Menus / focus / a new screen state** → `src/frontend/*` (seven states in
  `screen.rs`: Title → Menu (PLAY/SETTINGS) → InGame, plus Paused/Settings/
  Controls/GameOver; the frontend talks to the shell ONLY via `FrontendCommand`).
- **Field geometry / markings / numbers** → `src/field/{generator,markings}.rs`
  (all procedural; no imported assets).
- **Anything touching the DOM, storage, gamepad, audio, touch** → `src/web/`
  (wasm32-only, the sanctioned nondeterministic edge). Never elsewhere.

## App-local invariants (do not break these)

1. **Determinism.** The sim is a pure function of input; only time is the tick
   counter, only variation is `seed ^ stable event id` for presentation.
   `tests/determinism.rs` replays the whole showcase bit-for-bit — a change that
   makes it diverge is a bug, not a new baseline.
2. **Presentation cannot mutate simulation.** Enforced by `tests/camera.rs`.
3. **One coordinate system**, defined once in `src/field/coordinates.rs` (X
   sideline, Y up, Z end-zone-to-end-zone, 1 unit = 1 yard). No ad-hoc sign
   flips — use `OffenseFrame` for drive-direction mirroring.
4. **Everything visible is procedural** — no imported textures, meshes, fonts,
   sprites, or motion clips.
5. **The quarterback never throws on his own.** The throw is exclusively user
   input; the replay harness injects one scripted press at `TRACE_THROW_TICK`.
6. **Reduction guards.** The old menu-shell concepts are gone and must stay gone
   — `tests/frontend_reduction.rs` fails on the substrings (`MainMenu`,
   `TeamSelect`, `MatchSetup`, `Credits`, `MatchLaunchConfig`, difficulty/
   camera/game-speed settings, rebind, attract). Don't reintroduce those names.
7. **Every core source file stays under 300 lines** (`tests/architecture.rs`);
   no `unwrap`/`expect` in production, no console/placeholder macros, no
   junk-drawer modules.
8. **No football-specific systems leak into the engine.** Plays, routes, catch/
   tackle rules, camera grammar, and juice are game vocabulary — they live here,
   not in a layer or module. The one engine change this app motivated (physics
   immovable-pair gate) was generic and fixed in `axiom-physics`.

## Build / test / run

```sh
cargo test -p axiom-end-zone          # native tests (sim, drive, frontend, guards)
make end-zone-build                    # wasm build for the browser
cargo run -p axiom-serve -- end-zone   # local hot-reload dev server
```

Browser verification (the wasm `wgpu`/`web-sys` arm the native gate can't
exercise): serve `apps/end-zone/web` and drive it with
`scripts/playwright_controller.py`. Headless browsers need `?backend=canvas2d`
(the WebGL2 path lacks `VERTEX_STORAGE` there).

`cargo fmt -p axiom-end-zone` is fine here (it is an app, not a spine crate) —
but never `cargo fmt -p` a spine layer/module crate.
