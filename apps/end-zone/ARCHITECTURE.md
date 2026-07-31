# End Zone — architecture

`apps/end-zone` (`axiom-end-zone`) is a **composition-leaf Axiom app**: an
original arcade football game built on a reusable football-systems framework.

The game layer on top is the **decision-window prototype**. It exists to answer
one design question — *is observing a simulated play and intervening at a
dramatic decision window more enjoyable than continuously controlling a
conventional football play?* — so it is deliberately small: one formation, three
receivers, no playbook, no downs, no progression. The play snaps itself, drops
back, runs its routes and collapses its pocket while the player watches; at the
moment the read is worth making, time dilates and the player picks one of four
things (throw read 1, 2, or 3, or scramble) inside a couple of real seconds.
Then the attempt resolves and the next one begins, ~8–12 seconds a cycle.

The football simulation underneath is unchanged: the same deterministic,
data-driven play engine (formation → snap → drop-back → routes → blocking →
pass → catch → pursuit → tackle → ground impact). The **attempt layer**
(`src/attempt/`) sits on top of it and owns pacing, the decision window, and the
result. It replaced the previous score-attack drive layer (`src/drive.rs`:
downs, line to gain, heat, game over) and its separate pre-snap huddle SCREEN.
The play call itself came back, but on the field rather than in a menu: the
offense stands at the line with the card up until you call one, and the call is
what starts the play.

## App-local boundaries and the one-way flow

```text
input commands (keys → DeviceFrame → InputState)
  → fixed-step deterministic simulation      src/state.rs (+ subsystem stages)
  → decision-window attempt loop             src/attempt/* (window/choice/result/reset)
  → ordered simulation events                src/events.rs
  → immutable presentation snapshot          src/presentation/snapshot.rs (+ attempt view)
  → camera director + presentation effects   src/camera/*, src/presentation/*
  → HUD view model                           src/presentation/hud.rs
  → Axiom scene/render submission            src/scene.rs, src/scene_sync.rs, src/web/
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
3. **Presentation** (`src/presentation/*` — including the `locomotion/*`
   animator, `src/player/{model,rig,animation}.rs`,
   `src/football/model.rs`, `src/camera/*`, `src/debug.rs`) — consumes the
   immutable `PresentationSnapshot` plus the ordered `SimEvent`s. It cannot
   mutate the simulation; removing it changes nothing authoritative (proven in
   `tests/camera.rs::presentation_effects_do_not_mutate_simulation_state`).
4. **Composition** (`src/app.rs`, `src/scene.rs`, `src/scene_sync.rs`,
   `src/showcase.rs`, `src/shell.rs`, `src/web/`) — wires the headless
   `ShowcaseRun` to the engine `RunningApp`, retained entities, and the
   browser edge.

Two shell boundaries sit on top (details in `FRONTEND.md`):

5. **Frontend** (`src/frontend/*`) — the pure, browser-free **six-state**
   screen machine (`Title`, `InGame`, `Paused`, `Settings`, `Controls`,
   `GameOver`): device-independent actions, deterministic focus, three typed
   settings + a compact versioned persistence codec, theme, snap transitions,
   and the typed `SceneView` view model. It communicates ONLY through drained
   `FrontendCommand`s (`LaunchRun{seed}` / `RestartRun` / `ReturnToTitle` /
   `SetPaused`); the shell resolves a launch seed into the immutable
   `RunConfig` (`src/launch.rs`). The frontend never mutates a running run and
   never queries it (the shell pushes the run-over summary in via
   `enter_game_over`).
6. **Platform edge** (`src/web/`, wasm32-only) — the sanctioned
   nondeterministic directory: DOM presenter for the view model, a separate
   **HUD DOM layer** rendered from the authoritative `HudView`, the
   `web_sys::Storage` adapter behind the app-local `ProfileStore` trait,
   gamepad polling, menu tone synthesis, and the in-game touch controls.
   `src/shell.rs` composes frontend over game: it applies launch / restart /
   return / pause commands, steps the run per the frontend's `SimDirective`
   (menu-ambient, live, frozen), and reports a finished run back as the
   game-over screen.

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
quarterback NEVER throws on his own — the AMBIENT showcase controller
auto-starts and auto-snaps, but in a real session the snap follows the player's
play call, and the throw is always user input (the deterministic replay harness
injects one scripted throw press at `TRACE_THROW_TICK` to stand in for it). On
mobile there is no virtual joystick: the on-screen prompts are themselves the
buttons, read by one delegated pointer listener (`web/touch.rs`).

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
- **Animation** (`src/player/animation.rs` + `src/presentation/locomotion/*`):
  procedural poses from explicit state. Normal on-feet locomotion (idle / jog /
  sprint / start / stop / turn / backpedal) is owned by the app-local
  **locomotion animator** (`presentation::locomotion`): a distance-driven,
  planted-foot system detailed under "Locomotion" below. The remaining
  self-posing states (throw, catch, block, tackle, dive, hit reaction, stumble,
  airborne fall, ground impact, recovery) are the OVERRIDE poses in
  `animation::override_pose`; the carry / throw-ready arm overlay is
  `animation::apply_hold`.
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

## The decision-window attempt loop

`src/attempt/` is the app-local gameplay layer. It adds no football rule the
simulation does not already produce; it owns *pacing*, *when the player is
asked*, and *how the answer is measured*.

```text
PlayCall ──call 1|2|3──▶ Shifting ──offense set──▶ Developing ──trigger──▶ DecisionWindow
                                                       ▲                        │
                                                       └──── no choice ─────────┤
                                  ┌── throw 1|2|3 ─────────────────────────────┤
                                  ▼                             └── scramble ──┐
                             PassInFlight ──┐                            Scrambling
                                            ├─▶ Resolving ─▶ Result ─▶ Resetting
                                            └─────────────────┬──────────────┘
                                                              ▼
                                                          PlayCall
```

- **The pre-snap is two phases, and both ends belong to the player.**
  `PlayCall` has **no clock**: it holds until a play is called, so an attempt
  never runs a play nobody chose. `Shifting` then ends on a *fact about the
  field* — every offensive player standing on his spot (`setup::offense_is_set`)
  — so the ball goes because the offense is ready, not because a timer expired.
  `SHIFT_STALL_TICKS` is a stall guard behind that, never the normal path.

- **`AttemptPhase`** (`phase.rs`) is the whole state — never a pile of booleans,
  and every field a state needs is carried *in* the state (a `DecisionWindow`
  cannot exist without knowing when it closes). It also owns `time_scale()`,
  `accepts_choice()` and `steerable()`.
- **`PlayRead`** (`read.rs`) is what the player is being asked to judge: per
  read, how far the route has developed, how much separation it has and whether
  it has broken; plus how close the rush is. A pure function of simulation
  state. `window_trigger` decides whether *this tick* is the dramatic moment —
  a collapsing pocket forces the question before a pretty read does, and a
  **develop deadline** guarantees a window opens at least once per attempt, so
  the prototype can never silently fail to ask.
- **`AttemptController`** (`controller.rs`) steps the machine, issues
  `SimCommand`s, and latches the player's choice (input arrives per render
  frame, and is consumed once per simulation tick). A press outside a live ball
  — or a second press after one is committed — is stale and dropped. A throw is
  a single press: the pass is always solved onto the receiver, so the decision
  is *which receiver and when*, never how hard.
- **Re-arming.** Declining a window is a real decision, not a dismissal: full
  speed resumes for `WINDOW_COOLDOWN_TICKS`, then a shorter window opens
  (`WINDOW_TICKS` decays by `WINDOW_DECAY_TICKS` each time, floor
  `WINDOW_MIN_TICKS`), up to `MAX_WINDOWS`. After the last one nobody asks
  again — the rush gets home and it is a sack. That is the "I can wait a little
  longer, but I may lose everything" tension, expressed as timing.
- **`AttemptLedger`** (`ledger.rs`) is bookkeeping ONLY: attempts, completions,
  interceptions, sacks, yards. It never scales the defense or unlocks anything.
- **`setup.rs`** builds each attempt: always the same offensive concept
  (`data::prototype::triple_read`), spotted at `PROTOTYPE_LINE`, against a
  defensive call drawn from the existing deterministic selector keyed on
  `(seed, attempt index)`. Fixed `PROTOTYPE_HEAT` — the coverage varies, the
  aggression never escalates.
- **`view.rs`** is the one-way window presentation reads the loop through: a
  plain `Copy` `AttemptStep` hung on the immutable snapshot, so the
  "presentation cannot mutate simulation" boundary survives having a gameplay
  layer worth looking at.

**Time dilation lives in composition, not simulation.** `ShowcaseRun::time_scale`
reports the phase's scale and `EndZoneApp::advance` buys simulation ticks with
fractional *credit* — at 1.0× that is exactly one tick per frame as before; at
0.16× the sim advances once every ~6 rendered frames. The simulation itself
never learns about wall-clock time and stays a pure fixed-step function of its
command stream.

**Presentation.** The HUD (`presentation/hud.rs`) formats the attempt counter,
the session line, the state caption, the decision prompt and the result card —
and deliberately never reports how open a read is, because judging that is the
game. In-world, each read wears a coloured ring plus a stack of cubes whose
COUNT is its number (`presentation/receiver_ring.rs`). The camera's
`DecisionRead` mode pulls high and back for the window and springs back
afterwards, which is the most legible signal that the game is asking something.

A session is deterministic in `RunConfig`; restarting rebuilds the identical
initial state. There is no game over — the loop is endless, and END SESSION in
the pause menu is what produces the summary.

## AI model

The AI is a football-specific decision layer over a single shared play model, in
one direction (`src/ai/`):

1. **Assignment** (`assignment.rs`) resolves the play's per-slot data — routes
   compiled to world waypoints through the offense frame.
2. **Situation + perception** (`football/situation.rs`, `ai/perception.rs`): each
   tick derives a `BallSituation` (an AI-facing *view* over the authoritative
   `BallState` — adding a *committed-to-run* quarterback and a *contested* catch
   window; never a second state machine) and builds one read-only
   `PlayPerception` — the shared, delay-invariant play facts (situation, ball /
   catch geometry, pocket, run commitment, and the coordinated responsibilities).
   `ai/coordination.rs` is a stateless geometric pass that hands each defender one
   pursuit responsibility (primary / contain / cutback / deep, or intercept /
   contest / tackle-angle on a thrown ball) so the team keeps shape without
   duplicating an angle.
3. **Candidate generation + arbitration** (`brain.rs`, `offense.rs`,
   `protection.rs`, `defense.rs`, `action.rs`, `commitment.rs`): every role emits
   a few scored `ScoredAction`s on one shared priority scale (ball threat →
   prevent score → assignment → leverage → recover); the arbiter picks one under
   **commitment locking** (hysteresis) so players don't thrash. Positional
   identity lives in *which* actions a role offers; the machinery is one scored
   contest, not per-role conditionals.
4. **Execution** (`player/controller.rs`, the only writer of player movement,
   under acceleration/turn-rate limits with teammate-only separation and boundary
   clamping).

### Defensive overseer

Above the individual defenders sits a **`DefensiveOverseer`** (`ai/overseer.rs`) —
an adaptive coordinator for the opponent team. It watches the whole play through
the shared perception + an aggregated **`DefensiveRead`** (`ai/field_read.rs`:
pocket integrity, pressure, rollout, most-dangerous receiver by observable
separation, deep/crossing/sideline threats, scoring danger), runs at a
deterministic **cadence** (every 6 ticks, or immediately on a ball-state change /
touchdown emergency — never per player), scores candidate **`TacticalMode`s**
(`ai/tactics.rs`: base / pressure / contain-qb / qb-run / protect deep-middle-
outside / bracket / catch-point / swarm / emergency), and — under **commitment
locking** (a mode holds for its window unless the ball state forces a change or a
touchdown emergency appears) — issues ONE compact **`DefensiveDirective`**
(`ai/directive.rs`): a mode, coverage/rush emphasis, per-defender assignment
**overrides** (spy / blitz / bracket / contain, chosen by `ai/personnel.rs`), a
risk tolerance, a confidence, and the **exposed region** the adjustment gives up.

The overseer **never** sets a position, velocity, or steering vector. The
directive rides in `PlayPerception`; the geometric responsibilities come from
`coordination` as before; each defender's own arbitration reads its override +
the emphasis (`ai/directed.rs`) and converts them to movement — so every
adjustment is a real, exploitable tradeoff (pressure gives up the underneath
middle; a bracket single-covers a backside receiver; contain slows the rush).
Lightweight **possession memory** (scramble/hold/deep-drop/targeting tendencies)
nudges confidence and resets at the possession boundary (a touchdown). The one
persistent field is `overseer` on `SimState`.

Defenders still read a DELAYED perception ring (per-archetype reaction delay) for
the opponent geometry they chase, so the *shared situation* makes the team
coherent while individual reaction latency is preserved. **Line engagements**
(`ai/engagement.rs`) model each block as a deterministic contest whose *advantage*
builds over time (a strong blocker delays, but the rush eventually sheds and gets
home — never a global speed boost); the contact stage (`player/contact_stage.rs`)
advances it and applies the physical resist + pocket-compressing displacement. A
fresh catch has a brief **catch-secure** window before it can be tackled, so a
contested catch is a catch-and-step, not an instant swarm. Interceptions are
**break-up only** this pass (a defender at the catch point knocks the pass
incomplete — no possession change). The persistent AI state is two parallel
fields on `SimState` (`ai_memory`, `engagements`); everything else is a per-tick
local. All tuning lives in `data/tuning.rs` and the archetypes.

## Contact framework

`src/player/contact.rs`: blocking engagements (strength contest resists the
defender), deterministic tackle evaluation (range + closing speed + strength
vs mass → normalized impact strength), hit impulse, balance, and a controlled
procedural fall (stumble or airborne arc → ground impact → recovery) — no
ragdoll. A strong tackle emits `TackleContact` (tackler, target, point,
direction, relative speed, strength, airborne) and later `GroundImpact`; both
drive the presentation juice.

## Locomotion (distance-driven, planted-foot)

`src/presentation/locomotion/*` is the **single owner** of normal running
animation. It is a presentation system — it reads the immutable snapshot and
this tick's events and produces poses; it can never touch authoritative state.

**Authoritative movement owns world position; animation only explains it.** The
one-way flow is:

```text
authoritative movement (state.rs: AI → controller → collision → bounds)
  → resolved planar displacement + velocity   (PlayerView in the snapshot)
  → LocomotionInput                            (per player, per tick)
  → distance-driven GaitState                  (gait.rs)
  → planted-foot targets                       (foot.rs)
  → two-bone leg IK + whole-body pose          (leg.rs, pose.rs)
  → final PlayerPose                           (mod.rs)
```

- **Distance-driven gait phase.** The gait cycle advances by *actual resolved
  displacement*, `phase += planar_length(pos − prev_pos) / effective_stride`,
  measured from the snapshot's world positions AFTER collision de-penetration and
  boundary clamping. It is **not** driven by requested/AI velocity or a wall
  clock. Blocked movement (zero real displacement) does not advance the legs; a
  stop settles the phase to a foot-down; a teleport/reset (a discontinuity beyond
  `teleport_distance`, or a `PlayReset`/`PlayStarted` event) re-anchors the feet
  and does not advance the cycle. This is the root fix for the old skating: the
  legacy `player.stride += speed·dt` accumulated *intended* velocity in the
  simulation, so blocked/clamped players kept cycling. `stride` no longer exists
  on `PlayerSim`.
- **Planted-foot locking (`foot.rs`).** Each foot cycles Swing → Landing →
  Planted → PushOff. At foot-strike its world ground contact is latched and held
  fixed while the body travels over it — so a stance foot has ~zero world
  velocity (proven in `tests/locomotion.rs`). Feet plant a small,
  reach-bounded `stance_reach` ahead of the hip, and the planted fraction shrinks
  with stride (`2·stance_reach / stride`) so a world-locked foot is released into
  swing *before* the (short, arcade) leg over-extends — brief ground contact at a
  sprint, never a slide. The correction is visual only; it never moves the sim
  body.
- **Two-bone leg IK (`leg.rs`).** A small, explicit thigh+shin solver (law of
  cosines, knee bent toward facing so it never inverts) reaches each foot's world
  target; unreachable targets clamp instead of stretching; all outputs are
  finite. It is NOT a general IK engine.
- **Stride / cadence (`gait.rs`, tuning in `data/locomotion_tuning.rs`).** Stride
  blends short→long from speed and is bounded; cadence is capped
  (`max_cadence`) by *lengthening* the stride, never by blurring the legs.
  Startup ramps stride up from a stand; stopping shortens and settles; turns
  shorten and widen the stance and bank the torso.
- **Locomotion modes.** Explicit `Idle / Starting / Jogging / Sprinting /
  Stopping / Turning`, chosen from the ramps and resolved speed — not raw speed
  thresholds alone.
- **Whole-body motion (`pose.rs`).** Pelvis bob + yaw toward the leading leg,
  torso counter-rotation, forward lean from resolved acceleration, lateral bank
  from turning, shoulder counter-rotation, opposite arm swing, and a landing dip
  — all bounded by tuning.
- **Pose composition order (one explicit boundary, keyed on `AnimState`):**
  1. base rig pose (neutral)
  2. locomotion pose (legs by IK + pelvis/torso/arms) — for the holdable states
     `{Idle, ReadyStance, Jog, Sprint, DropBack}`; **or** an action / fall /
     recovery override (`animation::override_pose`) for every other state
  3. football carry / quarterback throw-ready arm overlay (`apply_hold`)
  4. presentation-only impact compression (juice squash) — applied at render in
     `rig::body_transform`.
  `AnimState` IS the animation-priority discriminant: a player is either running
  (locomotion owns the body) or in a self-posing action/fall (the override owns
  it); they never fight over a joint. On leaving an override the gait re-anchors,
  so no stale planted-foot target survives.
- **Determinism.** The gait bank is persistent presentation state advanced once
  per sim tick inside `ShowcaseRun::step` (never per render frame, so a paused
  frame re-presents the same poses). It is a pure function of the snapshot
  history, so the whole pose/gait history replays bit-for-bit
  (`tests/locomotion.rs::locomotion_replays_bit_for_bit_over_a_full_scripted_sequence`).
- **Diagnostics.** With the overlay on (F1), the selected player shows
  authoritative vs requested speed, actual distance moved, mode, gait phase,
  stride, cadence, planted foot, both foot states, both foot-lock errors, and any
  override reason; debug markers draw each planted-foot lock, each solved foot,
  the next intended landing, and the resolved movement vector. Debug rendering
  cannot affect the sim or the pose.

## Camera director

`src/camera/`: seven modes — `DecisionRead` (high and pulled back, framing the
pocket and all three routes for an open decision window), plus
(`FormationWide`, `QuarterbackFollow`,
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
