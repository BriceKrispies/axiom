# End Zone — architecture

`apps/end-zone` (`axiom-end-zone`) is a **composition-leaf Axiom app**: the
reusable engine framework for an original arcade-football game, proven by a
deterministic systems showcase (one data-driven play: formation → snap →
drop-back → routes → blocking → pass → catch → pursuit → tackle → ground
impact → reset), now fronted by a complete production menu shell (title,
menus, team selection, settings, pause — see `FRONTEND.md`). Still not the
finished game: no scoring, downs, or playbook.

## App-local boundaries and the one-way flow

```text
input commands (diagnostic keys → DeviceFrame → InputState)
  → fixed-step deterministic simulation      src/state.rs (+ subsystem stages)
  → ordered simulation events                src/events.rs
  → immutable presentation snapshot          src/presentation/snapshot.rs
  → camera director + presentation effects   src/camera/*, src/presentation/*
  → Axiom scene/render submission            src/scene.rs, src/scene_sync.rs, src/web.rs
```

Four boundaries:

1. **Data** (`src/data/*`, `src/config.rs`, `src/identity.rs`) — declarative
   teams, palettes, rosters, archetypes, formations, plays, routes, and every
   tuning knob (behavior, camera, juice). Typed ids (`TeamId`, `PlayerId`,
   `BallId`, `PlayId`, `AssignmentId`, `CameraTargetId`); players are always
   resolved in ascending `PlayerId` order over fixed arrays — never hash-map
   iteration order. `SimState.rosters` holds the live roster data every
   (re)formation is rebuilt from, so behavior changes are data mutations.
2. **Simulation** (`src/state.rs`, `src/physics_rig.rs`, `src/ai/*`,
   `src/football/{state,flight,possession,sim}.rs`,
   `src/player/{controller,contact,lineup}.rs`, `src/events.rs`) — one fixed
   60 Hz step per tick, a pure function of `(command stream)`; the explicit
   seed exists for presentation variation only. No wall clock, no ambient
   randomness, no scene/render types. Subsystem stages are `impl SimState`
   blocks owned by their subsystem (`ai::stage`, `football::sim`).
3. **Presentation** (`src/presentation/*`, `src/player/{model,rig,animation}.rs`,
   `src/football/model.rs`, `src/camera/*`, `src/debug.rs`) — consumes the
   immutable `PresentationSnapshot` plus the ordered `SimEvent`s. It cannot
   mutate the simulation; removing it changes nothing authoritative (proven in
   `tests/camera.rs::presentation_effects_do_not_mutate_simulation_state`).
4. **Composition** (`src/app.rs`, `src/scene.rs`, `src/scene_sync.rs`,
   `src/showcase.rs`, `src/shell.rs`, `src/web/`) — wires the headless
   `ShowcaseRun` to the engine `RunningApp`, retained entities, and the
   browser edge.

Two shell boundaries sit on top (details in `FRONTEND.md`):

5. **Frontend** (`src/frontend/*`) — the pure, browser-free menu machine:
   explicit screen states, device-independent actions, deterministic focus,
   typed settings + versioned persistence codec, theme, transitions, and the
   typed `SceneView` view model. It communicates ONLY through drained
   `FrontendCommand`s and the immutable `MatchLaunchConfig` boundary
   (`src/launch.rs`); it never mutates a running simulation.
6. **Platform edge** (`src/web/`, wasm32-only) — the sanctioned
   nondeterministic directory: DOM presenter for the view model, the
   `web_sys::Storage` adapter behind the app-local `ProfileStore` trait,
   gamepad polling, menu tone synthesis, and the in-match touch controls.
   `src/shell.rs` composes frontend over game: it applies launch / restart /
   return / pause commands and steps the sim per the frontend's
   `SimDirective` (menu-ambient, live, frozen).

## User control (touch + keyboard)

The player steers the OFFENSE's ball holder (quarterback pre-throw, carrier
after the catch) through `SimState::user_stick`, an offense-relative `[-1,1]²`
input stream sampled once per tick. A live stick replaces that one player's AI
intent with a movement intent at the AI stage — the controller still applies
every acceleration/turn-rate/boundary limit, so steered movement obeys the
same physics as AI movement, and a zero stick reproduces the scripted
showcase bit-for-bit (`tests/controls.rs`). The contextual
`DiagnosticCommand::PrimaryAction` (touch A / `Enter`) snaps pre-snap, throws
while the quarterback holds the ball, and restarts after the whistle. The
quarterback NEVER throws on his own — the showcase controller auto-starts and
auto-snaps, but the throw is exclusively user input (the deterministic replay
harness injects one scripted throw press at `TRACE_THROW_TICK` to stand in
for it). The platform edge (`web.rs`) mounts a pointer-event virtual joystick
and two buttons for mobile; `WASD`/arrows and `Enter` are the keyboard twin.

## Deterministic stepping

- Fixed step: 60 Hz (`FIXED_STEP_NANOS = 16_666_667`), one sim tick per
  animation frame (the repo's live-loop convention).
- The only time is the tick counter; the only variation source is
  `EndZoneConfig::seed`, used exclusively as `seed ^ stable event id` for
  presentation (camera impulse phases, dust directions).
- `tests/determinism.rs` replays the full showcase bit-for-bit (state digest,
  events, trajectory, possession, intents, camera modes/poses) and proves a
  second seed changes only the seeded presentation variation.

## Coordinate conventions

One system, defined once in `src/field/coordinates.rs`:
X sideline↔sideline (|X| ≤ 26.667), Y up (surface at Y = 0), Z end zone↔end
zone (|Z| ≤ 60), origin at midfield, **one world unit = one yard**; 120-yd
total length, 53⅓-yd width, goal lines at Z = ±50. All conversions (yard line,
normalized, offense-relative `OffenseFrame` that mirrors correctly in either
drive direction) live there — no scattered sign inversions.

## Procedural-content policy

Everything visible is generated at runtime — no imported textures, meshes,
fonts, sprites, or motion data:

- **Field** (`src/field/{generator,markings}.rs`): built ONCE — alternating
  turf bands and team-colored end zones as scaled engine planes; all line work
  (boundary, goal lines, five-yard lines, one-yard ticks, hash marks, an
  original midfield diamond) and block field numbers (a seven-segment quad
  table, no font) as two merged `MeshData` meshes; goalposts from engine
  cylinders.
- **Players** (`src/player/model.rs`): an original 17-box `axiom-figure`
  rig — oversized helmet + facemask bar, shoulder-pad slab, sturdy torso,
  arms/hands, legs/feet — with exaggerated arcade proportions. Part tags map
  to `TeamPalette` slots; construction has zero team branches.
- **Animation** (`src/player/animation.rs`): procedural poses from explicit
  state (ready stance, idle, jog, sprint, drop-back, throw, catch, block,
  tackle, hit reaction, stumble, airborne fall, ground impact, recovery). The
  leg cycle is keyed to accumulated stride DISTANCE, so feet do not slide.
- **Football** (`src/football/model.rs`): the engine's unit sphere scaled into
  a prolate silhouette plus a procedural lace-ridge box; tucked when carried,
  spiraling about the flight axis in the air.

## Football state machine

`Dead → Snap → Held(QB) → Airborne → Held(receiver) → …` plus
`Airborne → Loose → Grounded` for the incompletion path (`src/football/state.rs`,
transitions in `src/football/sim.rs`). The held-ball position IS the sim's
carry socket (a pure function of the carrier's pose). The throw solves a
deterministic release velocity (`flight.rs`) and hands it to the physics body:
flight and bounce are REAL integration through `axiom-physics`, never a
teleport. Catch evaluation is deterministic: catch volume radius + arrival
timing tolerance (archetype data) + the receiver's action state.

## AI model

Three stages (`src/ai/`): **assignment evaluation** (`assignment.rs` resolves
the play's per-slot data — routes compiled to world waypoints through the
offense frame), **intent** (`brain.rs` dispatch + `offense.rs`/`defense.rs`
role machines emit typed `PlayerIntent`s), **execution**
(`player/controller.rs`, the only writer of player movement, under
acceleration/turn-rate limits with teammate-only separation and boundary
clamping). Defenders read a DELAYED perception ring (per-archetype reaction
delay) and pursue with bounded, aggressiveness-scaled prediction — no perfect
mirroring. All tuning lives in `data/tuning.rs` and the archetypes.

## Contact framework

`src/player/contact.rs`: blocking engagements (strength contest resists the
defender), deterministic tackle evaluation (range + closing speed + strength
vs mass → normalized impact strength), hit impulse, balance, and a controlled
procedural fall (stumble or airborne arc → ground impact → recovery) — no
ragdoll. A strong tackle emits `TackleContact` (tackler, target, point,
direction, relative speed, strength, airborne) and later `GroundImpact`; both
drive the presentation juice.

## Camera director

`src/camera/`: six modes (`FormationWide`, `QuarterbackFollow`,
`BallCarrierFollow` with velocity look-ahead + yaw-lag clamp, `PassFlight`
framing ball + arrival, `CatchResolve` blending to the catcher, `Impact` with
automatic return), driven ONLY by typed events + the snapshot. Critically
damped springs at fixed ticks (`rig.rs`);
`final pose = smoothed base + additive impulse stack` (`impulse.rs`) — the
stack is bounded (8), clamped, seeded, and every impulse's final sample is
exactly zero, so shake can never drift the base. Diagnostic keys can force
modes; `5` returns to automatic.

## Event-driven juice

`src/presentation/{juice,particles}.rs`: dust bursts, impact rings, speed
streaks, ball trail, catch flash, throw pulse, field-plane wobble, player
squash — all spawned only by events, with bounded lifetimes, clamped
amplitudes, fixed pools, exact decay to zero, and variation derived solely
from `seed ^ event id`.

## Consumed Axiom capabilities

| Capability | Facade |
|---|---|
| Scene/render/camera/lights/meshes | `axiom` umbrella (`RunningApp`, `MeshData`, `set_camera`) |
| Deterministic physics (ball flight/bounce, player bodies) | `axiom-physics` (`PhysicsApi`) |
| Procedural box figures | `axiom-figure` (`FigureApi::posed_parts`) |
| Deterministic input sampling | `axiom-input` (`InputState`/`DeviceFrame`) |
| Seeded randomness (presentation only) | `axiom-kernel` (`DeterministicRng`) |
| Scalars/math | `axiom-kernel` (`Meters`/`Radians`/`Ratio`), `axiom-math` |
| Runtime step vocabulary | `axiom-runtime` (`RuntimeStep`) |
| Ambient/sky | `axiom-host` (`FrameAmbient`) |
| Live browser loop + overlay (wasm32 only) | `axiom-windowing`, `axiom-debug-overlay` |

Engine constraints inherited and documented here: the physics narrow phase has
no prolate/convex collider (the football flies and bounces as a sphere; the
silhouette is visual scale) and no joints/compound bodies (each player is one
kinematic sphere mirrored from the controller; the 17-box figure is render
pose only) — the same compromises as the repo's sports-physics lab.

## Why football-specific systems stay in the app

Plays, routes, assignments, catch rules, tackle rules, camera grammar, and
juice recipes are game-design vocabulary, not engine capability. The Module
Law reserves layers/modules for reusable, game-agnostic capabilities; a
football framework is exactly the kind of composition an app tier exists for.
The one engine change this app motivated — the physics contact solver's
immovable-pair gate — was generic (any two kinematic bodies), and was
therefore fixed in `axiom-physics` with direct tests, not worked around here.
