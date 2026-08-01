# Burnt Rubber — Architecture

An original third-person arcade racing framework and demonstration game, built
as a **composition-leaf app** on the Axiom engine.

---

## 1. Responsibilities

This app owns, end to end:

| Concern | Where it lives |
|---|---|
| Course generation (seeded, constrained, paced) | `track/generate.rs`, `track/section.rs` |
| Spline sampling and the arc-length table | `track/spline.rs`, `track/mod.rs` |
| The arcade car model | `sim/car.rs`, `sim/controller.rs` |
| Where the mass sits, and what it costs | `sim/chassis.rs` |
| Racing collision response | `sim/collision.rs` |
| Deterministic traffic | `sim/traffic.rs` |
| Boost, near misses, the reward loop | `sim/boost.rs`, `sim/collision.rs` |
| Race flow (countdown, progress, finish, reset) | `sim/mod.rs` |
| The chase camera | `camera.rs` |
| Road geometry and chunk lifecycle | `render/road_mesh.rs`, `render/chunks.rs` |
| Roadside scenery and its pool | `render/scenery.rs`, `render/scenery_pool.rs` |
| Car and traffic visuals | `render/car_model.rs` |
| Speed effects | `render/effects.rs` |
| The racing HUD | `hud.rs` |
| Audio cues | `audio_cues.rs` |
| Telemetry | `diagnostics.rs` |
| Visual debugging | `debug_view.rs` |
| Deterministic scripting and capture | `script.rs`, `capture.rs` |
| Tuning | `tuning.rs` |

### Why all of this is app-local

Every one of those is a *game design decision wearing an engineering hat*.

The car model is the clearest case. It is not a vehicle simulation; it is an
authored forward/lateral velocity split with a hand-picked grip curve and a
hand-picked steering-authority curve, chosen so that *this* car on *this* road
feels the way this game wants. A "generic vehicle physics module" would have to
be tunable for every game that used it, and the moment it were, it would stop
being tunable for this one. The same argument applies to the boost economy, the
near-miss window, the chase camera's heading blend, and the course's pacing
curve: they are not capabilities, they are opinions.

Nothing racing-shaped was added to the kernel, no new ordered layer was created,
and no existing engine module was modified.

### What the engine actually provides

| Engine capability | What it does here |
|---|---|
| `axiom` (umbrella) | Scene, meshes, materials, lights, camera, the render tick, `Visible`/`Transform` component writes |
| `axiom_kernel::DeterministicRng` | The seeded source behind the course, the scenery and the traffic |
| `axiom_math` | Every vector, quaternion, matrix, AABB |
| `axiom_frame::FrameAccumulator` | Banking a variable browser frame into whole 60 Hz steps |
| `axiom_input::InputState` | The action-binding table keys and pad buttons are folded through |
| `axiom_visibility::VisibilityApi` | Frustum culling (`visible_mask`) and distance-band LOD (`lod_levels`) for the scenery pool |
| `axiom_audio::AudioApi` | The neutral mixer the engine/wind/tyre/impact cues are scheduled into |
| `axiom_windowing` (wasm) | The live presentation loop |

**No engine change was required.** Every capability the app needed already
existed with a usable public surface.

---

## 2. The deterministic boundary

```
                    ┌──────────────── deterministic ────────────────┐
browser clock ──▶ FrameAccumulator ──▶ N × RaceSim::step(DriveCommand)
                                          │  track, car, traffic,
                                          │  boost, camera, events
                                          ▼
                    └───────────────────────────────────────────────┘
                                          │  (read only)
                    ┌──────────────── presentation ─────────────────┐
                    RaceScene::pose(alpha) ──▶ RunningApp::tick ──▶ pixels
                    └───────────────────────────────────────────────┘
```

* Elapsed real time enters at exactly **one** place: `LiveState::elapsed_nanos`
  in `web.rs`. It is immediately converted into an integer step count. Nothing
  below the app root ever sees a duration.
* That time is **clamped** to `MAX_FRAME_NANOS` before it reaches the
  accumulator. `FrameAccumulator` banks everything it is handed, and whole steps
  clamped away by `max_steps` "also stay banked (never dropped)" — the right
  contract for an accumulator, but it means the step cap limits the *rate* of
  catch-up and not the *total debt*. A two-second hitch otherwise banks 120 steps
  and the next two dozen frames each run the full five, so the whole world moves
  at **five times real time** until it drains. A racing game is real-time: time
  lost to a stall is lost, not replayed at quintuple speed.
* `RaceSim::step` reads a `DriveCommand` and nothing else. No clock, no ambient
  randomness, no globals.
* Presentation *reads* simulation state and interpolates between the last two
  steps. It writes nothing back. Rendering twice produces the same frame;
  rendering zero times changes nothing about the race.
* The camera is **inside** the deterministic half. Its springs advance on fixed
  steps, so a replay reproduces the framing, not just the physics.
* The effect phase (`render/effects.rs`) also advances on the fixed step, so a
  144 Hz browser does not get faster tyre smoke than a 30 Hz one.

Given the same seed, starting state, step count and ordered command sequence,
the app reproduces: the generated track, the car state, the traffic, the boost
meter, the collision events, the camera pose and the progress state. This is
asserted directly in `sim::tests::an_identical_command_sequence_replays_identically`.

---

## 3. The car controller

One fixed step, in this order:

1. **Ramp the steering input** toward the command.
2. **Rotate the chassis.** Yaw rate = `steer × steering_authority(speed) ×
   pivot × direction × handbrake × airborne`, plus a counter-steer assist while
   drifting.
3. **Decompose the (unchanged) velocity** into the new chassis frame.
4. **Longitudinal:** throttle against a tapering acceleration curve, braking,
   reverse, boost, drag, off-road penalties, a hard speed clamp.
5. **Lateral grip:** bleed the lateral component exponentially, at a rate set by
   the surface and the handbrake.
6. **Integrate position** in two bounded sub-moves, each with a barrier check.
7. **Settle onto the road**: gravity always applies and the road is a floor.
8. **Classify surface**, update the drift flag with hysteresis, age the impact.

**Rotating the chassis is what creates a slide.** Nothing models a tyre. When
the wheel is flicked, the nose swings and the velocity does not; the difference
*is* the lateral component; grip decides how fast it goes away. Turn the grip
down — handbrake, or dirt — and the difference survives, which is a drift.

Step 3 is the whole model, and it is easy to leave out: the velocity is *stored*
in the chassis frame, so if the chassis rotates and those two numbers are left
alone, the velocity has silently rotated with the car. The nose and the direction
of travel can then never disagree and there is no such thing as a slide — the car
just pivots. Re-projecting the unchanged world velocity onto the new axes is what
makes the disagreement exist. (It was missing here at first, and every "drift" in
the game was really a barrier bounce; a wide enough road that the car stopped
reaching the barriers is what exposed it. `turning_the_chassis_leaves_the_velocity_behind`
now pins it.)

The consequence worth knowing when tuning: a turn at speed `v` with yaw rate `ω`
settles at roughly `v·ω/grip` of lateral slide. `grip` therefore has to be set
against the *steering authority*, not in isolation — high enough that a plain
hard turn stays under `drift_threshold`, or the car drifts whenever it corners
and the handbrake stops meaning anything. That relationship is asserted in
`tuning::tests`.

Consequences worth stating:

* The model cannot explode. Every quantity is a bounded velocity, never an
  accumulated force, so a bad contact can slow the car or shove it sideways but
  cannot hand the next step a number that grows.
* Airborne behaviour is free: gravity always applies and the road is a floor,
  so a crest where the road falls away faster than gravity pulls launches the
  car with no "jump" case anywhere.
* Steering authority falls hyperbolically with speed, with a floor
  (`steer_authority_floor`). Without the falloff, a flick at 320 km/h is a spin;
  without the floor, top speed is a rail.

---

## 4. The chase camera

The heading is a **blend**, and the blend is the design:

* the chassis nose — so the camera is attached to the car;
* the direction of travel (`velocity_heading_blend`) — so a drift stays readable;
* the road ahead (`track_anticipation`) — so a corner opens up slightly early;
* the steering input (`STEER_LEAD`) — so turning in leads rather than trails.

Everything else is a bounded, smoothed function of speed, boost and impact:
field of view (65° → 88° → 96°), chase distance (6.5 m → 8.5 m, +1.1 m on
boost), look-ahead (5 m → 14 m), accel/brake pull (±1.6 m), turn roll (±4°),
and a three-layer shake — a quadratic-in-speed vibration, a boost addition, and
a decaying directional impulse after an impact.

Camera obstruction is handled with a floor: the eye never drops below the road
surface behind the car, plus `min_ground_clearance`. That is deliberately not a
general camera-collision system — the only geometry a chase camera here can be
pushed into is the road it is following, and the road's height is a table lookup
the frame already does.

---

## 5. The procedural track

### Representation

A `Track` is an immutable, **arc-length-uniform** table of `TrackSample`s at 2 m
spacing (~4 600 entries for the 9 km course), each carrying position, an
orthonormal banked frame (tangent/right/up), distance, heading, curvature,
grade, bank, half-width and section.

It is the app's **single source of spatial truth**: the road mesh, the props,
the traffic lanes, the collision boundary, the reset points, the camera's
anticipation and the HUD progress bar all read this one table. Two
representations of a racing line drift, and when they drift the car drives
through the scenery.

**Lanes live here too** (`Track::lane_count` / `Track::lane_lateral`), for the
same reason and not by coincidence: the painted dividers and the lanes the
traffic holds have to be the *same* lanes. Computing them separately in the road
mesh and in the traffic is how you get cars driving down the middle of a painted
line — and it is exactly what happened here until the road was widened and the
two definitions diverged.

Arc-length uniformity is load-bearing: every downstream consumer addresses the
course by *metres travelled*. A parameter-uniform table would make lane dashes
bunch in corners and "4 200 m along" not a distance.

### Generation

1. **Plan** — nine authored sections with an ordered pacing curve (opening
   straight → coastal sweepers → ridge crests → esses → tunnel → long haul →
   canyon → final sweep → finish), each with its own curviness, hilliness,
   width and event-length envelope.
2. **Author** — per control point (40 m apart), a heading-step and grade signal
   emitted as *events*: smooth half-sine bumps over several points, separated by
   straights. A bend therefore has an entry, an apex and an exit rather than
   being per-point noise.
3. **Correct** — clamp magnitudes, then run exactly `correction_passes` bounded
   forward+backward relaxation sweeps limiting how fast the heading step and
   grade may change. Every loop has a compile-time bound; there is no
   "retry until valid".
4. **Integrate** — walk the corrected signals into world positions. Constraints
   hold on the *signal*, and the geometry is built from the signal, so a
   position can never encode an illegal turn.
5. **Sample** — Catmull-Rom through the control points (C¹ continuous, passes
   *through* its points), densified to 1 m and resampled at exactly 2 m.
6. **Frame** — central-difference tangents, curvature from heading change,
   banking from clamped curvature smoothed over a fixed number of passes.

Enforced and asserted: minimum turn radius, maximum curvature change between
adjacent samples, maximum grade and grade change, maximum banking, minimum and
maximum road width, no inverted road, no non-finite geometry.

---

## 6. Chunk lifecycle

The course divides into ~92 chunks of 100 m. Each chunk becomes four meshes
(surface / paint / rail / verge) — one per material, so a visible chunk costs
four draw calls rather than one per lane marking.

**Everything is built once, at install, and streaming is purely a visibility
decision.** Chunks entering the active range (`CHUNKS_BEHIND = 2` …
`CHUNKS_AHEAD = 14`) are shown; chunks leaving are hidden. Nothing is rebuilt,
despawned or re-uploaded, and the update early-outs unless the range actually
changed (about once a second at racing speed).

That shape is not a shortcut, it is required: the live browser backend sizes its
vertex and instance buffers from the mesh set captured at startup, so a mesh
registered after the render loop begins would never reach the GPU.

### Why chunk boundaries cannot crack

Chunk `n` spans samples `[n·k, (n+1)·k]` **inclusive at both ends**. The last row
of chunk `n` is generated from the *same table entry* as the first row of chunk
`n+1` — not an equivalent sample, the same one. Their boundary vertices are
therefore bit-identical, and no floating-point difference can open a seam.

---

## 7. Scenery

Priorities, in order: *frequent small things close to the road*, *occasional
large landmarks*, *anything on the horizon* last.

* **Reflector posts** are placed by distance (every 8 m, both sides, at the
  shoulder edge) with no randomness at all. Their whole value is the regularity:
  at 90 m/s that is eleven a second past each shoulder, which the eye reads
  directly as speed. They sit *inside* the barrier line on purpose — close to
  the camera is the point.
* **Tunnel lights** every 11 m on the ceiling, so an enclosed section strobes.
* **Zone props** (trees, rocks, poles, signs, buildings) are drawn from a
  per-zone vocabulary at 14 m slots, most of which are deliberately empty so the
  filled ones read as landmarks rather than wallpaper.
* **Distant hills** are generated once for the whole course and never streamed;
  they are visible from everywhere, so streaming them would be pure overhead.

A prop is a pure function of `(seed, chunk, side, slot)`. Chunk 41 regenerates
identical trees whether you reach it in the first minute or after three resets.

Per-kind entity pools are the hard instance ceiling. Each frame the cached props
are distance-rejected, then frustum-culled and LOD-banded through
`axiom_visibility`, and the survivors are written into pool slots. The engine
answers "what can the camera see, and how finely"; the app answers "which
archetypes, how far is each worth drawing, what does a reduced tier look like".

---

## 8. Traffic

An infinite ordered list of **slots** along the course, slot `k` at
`k · traffic_spacing`. A slot's lane, speed, variant and lane-wander phase are a
pure function of `(seed, k)`. A bounded pool of live cars is recycled through
those slots.

The consequence is the property the tests pin: **recycling a pool entry cannot
change what a slot contains.** The spawn cursor also skips consumed slots
*arithmetically* rather than one per loop iteration, so a player who jumped
forward (a capture, a reset, the finish) does not find an empty road while the
cursor crawls up to them.

Traffic has no AI, no pathfinding and no awareness of the player. The
interesting agent in a game about threading traffic is the player; traffic that
reacted would remove exactly the judgement being asked for.

---

## 9. Collision response

Both barriers and traffic resolve **positionally, then in velocity**, entirely
in bounded velocities. A contact removes the motion heading into the obstacle,
reflects a fraction, and scrubs forward speed proportionally to how square the
hit was.

The design goal is specific: a collision must **hurt momentum without stopping
the demo**.

* Position is integrated in two sub-moves per step, each with a barrier check;
  at the boosted top speed that is under a metre per check, far shorter than the
  car, the traffic or the barrier.
* A **scrape alignment** turns a car pressed against a barrier to run along it.
  This is load-bearing rather than decorative: the chassis has no yaw authority
  of its own at low speed and the contact response only touches velocity, so
  without it a car that nosed into a wall would grind there forever with nothing
  in the model ever pointing it back down the road.
* A rear-ender can never leave the player slower than the car in front.
* A side-swipe always shoves, whether or not the player was still closing.
* Grazes below a threshold are still resolved but not *reported*, so running a
  wall does not strobe the camera, the audio and the HUD.

---

## 10. Performance strategy

| Measure | How |
|---|---|
| No per-frame allocation in the fixed step | The controller writes only into existing state |
| No per-frame mesh rebuild | All geometry built once at install |
| Bounded draw calls | ≤17 chunks × 4 meshes; one material per pooled kind |
| Bounded instances | Hard per-kind pool capacities, asserted against the generator's worst case |
| Bounded traffic | Fixed pool, recycled |
| Bounded particles | Fixed pools; a slot's position is a function of index and phase, not a simulation |
| Culling and LOD | `axiom_visibility`, plus a cheap distance reject first |
| Scratch reuse | The scenery cache keeps chunks that stayed in range and reuses its scratch buffers |
| Early-outs | Chunk visibility and the scenery cache only do work when the range changes |

Telemetry (`diagnostics.rs`) reports active chunks, total road triangles, drawn
scenery instances, cached scenery chunks, live traffic, effect instances,
simulation steps, speed and progress — as **structured values**, never printed.

---

## 11. Extension points

* **A second car** — `VehicleTuning` is a value; `RaceSim::new` takes a `Tuning`.
* **A second course** — `RaceSim::new` takes a seed; the pacing plan is
  `SectionKind::ALL` and its profiles.
* **A new section kind** — add a variant, a profile and a zone; the generator,
  the mesh builder, the scenery and the HUD all pick it up.
* **A new prop archetype** — add a `PropKind` variant with a capacity, extents,
  draw distance and a mesh/material mapping.
* **Rebindable controls** — `Controls::new` is a binding table; the simulation
  reads action ids, never keys.
* **Recorded replays** — a `Vec<DriveCommand>` plus a seed is a complete replay;
  `BurntRubber::advance_steps` is the player.

---

## 11a. Input: one path for three devices

Keyboard, gamepad and the on-screen touch pad all converge on the same
[`axiom_input::InputState`] action table before the simulation sees anything:

* the **keyboard** supplies key tokens directly;
* the **gamepad**'s face buttons are folded into synthetic key tokens, and its
  triggers and stick arrive as analogue channels;
* the **touch pad** (`touch.rs`) does exactly the same — its five buttons
  present themselves as the key tokens their physical equivalents use, and its
  joystick supplies analogue steering.

So a touch player and a keyboard player are indistinguishable to `RaceSim`, and
there is one binding table to change rather than three input paths to keep in
sync. Where two sources disagree on an analogue channel, the one asking for more
wins, so a pad and a keyboard can be used together.

`touch.rs` is the **model**, not browser glue: layout, hit testing, the
joystick's deadzone and its clamping are all native-testable, and `web.rs` is
left with "attach three pointer listeners and draw some circles". `HeldKeys`
(in `controls.rs`) is the same idea for the keyboard, and for the same reason —
see below.

### Input state is the one thing a reset cannot fix

Everything else the player can get wrong lives in the car, and `R` puts the car
back. **Held input does not**: if a key is recorded as pressed and never
recorded as released, it is held forever, and resetting the car just puts it
back on the road to be immediately told to turn again. That makes stuck-input
bugs uniquely nasty — they look like physics bugs and they survive every
recovery the game offers.

Three rules follow, and all three are in `controls.rs` where they can be tested:

1. **A key has exactly one identity, and it is `code`.** A browser event carries
   both `code` ("KeyD", the physical key, never changes) and `key` ("d", the
   character produced, *changes with the modifiers*). Recording both leaves a
   phantom the moment a modifier is involved: press D, press Shift, release D,
   and the browser reports the release as `"D"` — so the `"d"` recorded on the
   way down is never cleared. In a game where Shift is boost and D is steer,
   that is a car that steers right forever. `key` is used only as a fallback for
   synthetic events, which have no physical key to name.
2. **Focus loss releases everything.** A browser delivers `keydown` and then
   simply never delivers the `keyup` if focus moved away in between. `blur`,
   `pagehide` and `visibilitychange` all clear the held set and the touch pad.
3. **A mouse is not a thumb.** Pointer events from a mouse never create a
   virtual joystick — otherwise clicking the canvas to focus it plants a stick
   in the lower-left and starts steering, with no visible pad to explain why. The joystick
is deliberately **dynamic** — it appears centred on wherever the thumb lands in
the steering zone, because a fixed on-screen stick has to be found without
looking.

---

## 11b. Two engine facts this app is designed around

Both were found by looking at the running game, and both are recorded here
because they are invisible from the source alone.

### World `+X` is on the **left** of the screen

`Mat4::look_at` builds its screen-right axis as `forward × up`, which is the
opposite handedness from the usual `up × forward`. The consequence is direct:
increasing the car's `yaw` swings its nose from `+Z` toward `+X`, which the
player sees as turning **left**. Steering right is therefore a *decreasing* yaw
(`sim/controller.rs`), and the front wheels steer by `yaw − steer_angle`.

## The centre of gravity

`sim/chassis.rs` holds the car's mass geometry — wheelbase, track width, the
height of the centre of gravity and where it sits along the wheelbase. It is
deliberately **not** a tyre, drivetrain or suspension model; it is the handful of
real rigid-body relations a centre of gravity genuinely determines, each wired to
a handling knob that already existed:

| Real quantity | What it drives |
|---|---|
| CoG offset from the wheelbase midpoint | the point the chassis yaws about (`rotate_chassis`) |
| Static front load fraction | turn-in authority, and braking |
| `cog_height / half-track` — the rollover ratio | lateral load transfer, and so grip |
| `cog_height / wheelbase` | longitudinal transfer under braking |

Cornering throws load onto the outside wheels, and a tyre's grip grows less than
linearly with the load on it, so an unevenly loaded pair grips less than an
evenly loaded one. That is why a low centre of gravity corners better here, for
the same reason it does in the world — and why the shipping car carries its mass
at 30 cm on a 1.9 m track, 56% of it on the front axle.

Two bounds in this area are load-bearing and easy to get wrong: the rollover
threshold must sit **clear of the car's real cornering load**, and the braking
clamp clear of what the shipping car actually reaches. A limit the ordinary case
is pinned against silently deletes the effect it was meant to bound — an earlier
draft saturated both, and the geometry stopped discriminating at exactly the
moment it should have mattered most.

Every world-space test passes with this sign either way round, which is exactly
how a game ships with inverted controls. So the test that pins it
(`steering_turns_the_car_in_the_direction_the_player_sees`) derives screen-right
the same way the view matrix does, and asserts against *that*.

The rest of the app's world chirality is left alone: a mirrored world is still a
valid world, and the banking, the lane order and the collision sides are all
self-consistent under mirroring. Only the place where a *player-relative* term
("right") meets the world needed correcting.

### Ground geometry has to be continuous, not just correct where you look

The verge originally started at the *barrier*, while the paved surface stopped at
the *shoulder* — leaving the metres of dirt in between with no geometry at all.
That strip is exactly where a car that has run wide is sitting, so the hole was
invisible on the racing line and unmissable off it. Widening the road made it
worse. `the_ground_is_continuous_from_the_tarmac_to_the_scenery_line` pins it.

### The depth range is a quality setting, not a formality

The road, its shoulder, its verge and its paint are four nearly-coplanar surfaces
stacked within centimetres. With a `0.35 m` near plane and a `1800 m` far plane
they z-fight into shimmering bands a few hundred metres ahead — most of what the
player is looking at. The near plane is the end of the ratio worth moving (the
camera never holds less than 6.5 m of car), and the far plane should reach the
furthest drawn chunk and stop.

### `Material::with_emissive` never reaches the GPU

Emissive is carried from the umbrella down through `axiom-render`'s
`RenderMaterial` — and stops there. The backend never reads it and `DrawData`
exposes only `color()`, so on the live path an emissive term contributes
nothing. Every material here that is meant to glow therefore carries its
brightness in `base_color`; the emissive is kept only as a declaration of intent
for a backend that grows support. See the note in `render/palette.rs`.

---

## 12. Intentionally omitted

* **Tyre, drivetrain and suspension simulation.** The visual suspension is
  presentation layered over simulation state and feeds nothing back.
* **Takedowns, crash cinematics, vehicle destruction.**
* **Traffic AI.** Lane-holding only, by design.
* **Multiplayer, an editor, an open world, a general procedural-world module.**
* **Full-screen motion blur.** Speed comes from optical flow, field of view and
  geometry, not from a post-process.
* **In-scene 3D text.** `axiom-text` produces neutral glyph batches with no path
  into the renderer's draw list; building that bridge would be a general engine
  capability added in an app to show a speedometer. The HUD is a DOM overlay,
  the established pattern in this repository.
