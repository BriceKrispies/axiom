# Bend It — architecture

`apps/bend-it` (`axiom-bend-it`) is a **composition-leaf Axiom app**: a
mobile-first penalty game with exactly one control.

You **draw the line you want the ball to take**. When you let go, the line
disappears, the kicker reads it, and it takes the closest shot it is actually
capable of. Everything in this document exists to keep one promise:

> The ball goes where you drew, and the only thing that can change its mind is a
> physical contact.

## The one-way flow

```text
pointer + keys
  → DeviceFrame → InputState                       axiom-input (neutral)
  → a drawn line                                   stroke/capture.rs
  → ShotIntent                                     stroke/interpret.rs
  → ONE arc-length-uniform world Trajectory        shot/trajectory.rs
  → fixed-step attempt machine (60 Hz)             play/session/
  → keeper read, dive, capsule interception        play/keeper*.rs, contact.rs
  → camera fitted to this viewport                 camera.rs
  → retained scene submission                      scene/sync.rs
  → screen-space overlay view model                stroke/view.rs
  → SVG painter                                    web/overlay.rs (wasm only)
```

Every arrow is one-way, and three are enforced by `tests/architecture.rs`:

* **`the_drawing_layer_can_only_produce_a_shot_intent`** — nothing under
  `src/stroke/` may name a `Trajectory`, a `Ball` or a `Keeper`. Reading a
  drawing yields a `ShotIntent`; that is its whole vocabulary.
* **`nothing_downstream_of_the_trajectory_can_rewrite_it`** — the ball and the
  keeper may *read* the authored path and may never build one.
* **`the_same_drawing_is_read_the_same_way_every_time`** — the reading may not
  touch a clock, a random source, or an unordered container.

## The mechanic, in three ideas

### 1. Reading a drawing is a fit, not a parse

The line is projected back into the world and **least-squares fitted** onto the
space of legal shots — the two Bézier weights per projection that `shot/curve.rs`
defines. The model is linear in those weights, so the fit is a 2×2
normal-equation solve in closed form: no search, no iteration, no tolerance, and
the same pixels always produce the same kick.

Nothing is ever rejected. A clean banana gives a banana; a shaky line gives the
smooth shot nearest to it; a scribble gives the best single shot that scribble is
evidence for. That is what "the kicker does its best" means, precisely.

Two details carry most of the accuracy, and both exist because the camera looks
almost straight *down* the shot:

* **Progress comes from arc length, not from nearest approach.** On screen,
  "further away" and "higher" are nearly the same direction, so a
  perpendicular-foot ruler reads every bit of lift as distance and the height of
  an arc vanishes — silently, with a perfect residual.
* **Offsets are solved in a local screen basis, never unprojected.** One metre
  across and one metre up are projected *forward* at each point of the flight,
  and the drawn point's offset is solved in that 2×2 basis. No depth is ever
  guessed, so a small depth error cannot masquerade as a bend.

One thing is genuinely unreadable, and is recorded as a test rather than left as
a mystery: a *small* arc on a shot aimed dead centre. The camera sits on that
shot's own centre line, so flat and gently-arced draw the identical picture. A
real lob leaves that line — it climbs above the goal and drops back in — and is
read at better than 90%.

### 2. Where the line finishes is where the ball finishes

The last point of the drawing is cast onto the goal plane and **clamped into the
mouth**. However wildly the line was drawn, the endpoint is legal, so the shot is
valid by construction and nothing downstream has to steer it there.

### 3. The keeper only sees the beginning

It reads once at the end of its reaction, dives, and gets **one lateral
correction** — it can still adjust its line but cannot un-commit its height. It
also remembers where the last few penalties finished and shades both its starting
position and its expected height toward them, so no shape stays a solved answer.
Its *memory* is not hedged the way a single glance is: four penalties into the
same corner is evidence, not a guess.

## Boundaries

| Boundary | Lives in | Rule |
|---|---|---|
| **Tuning** | `tuning.rs` | Every gameplay number, one file. Systems read their own sub-table; no system hides a constant. |
| **Data** | `pitch/`, `figure/model.rs` | Coordinates, pitch geometry, the goal, the humanoid. Built once, never per frame. |
| **Authoring** | `shot/` | Pure data → one deterministic path. Cannot see a pointer. |
| **Simulation** | `play/` | One fixed 60 Hz step, a pure function of `(commands, tick)`. No clock, no randomness. |
| **Presentation** | `camera.rs`, `scene/`, `debug.rs` | Reads the session; can never write it. |
| **Interaction** | `stroke/`, `projection.rs` | Pixels in, one `ShotIntent` out. |
| **Agent** | `agent/` | Perceives (`eyes`), decides through `axiom-agent`, and *draws* (`hand`) — its whole output is pixels. |
| **Platform edge** | `web/` (wasm32) | The only nondeterministic directory. |

## Determinism

The simulation's only time is the tick counter and there is no randomness
anywhere — not in the keeper, not in the result. A replay of the same commands
is the same shootout, which is what makes `agent::play_through` a reproducible
measurement of both the agent and the game's balance.

## Reused from End Zone, and what was left behind

The humanoid and the environment descend from `apps/end-zone`'s arcade
footballer and procedural field:

* **Kept** — the 17-box parented figure, boxes pivoting at the joint and centred
  along the segment, tag-driven materials, the distance-driven gait (phase is
  *metres travelled*, not seconds, so legs slow when the body does), the visual
  body root derived one-way from the gameplay root, the mown-band depth cue, and
  a closed horizon behind the goal.
* **Removed** — the helmet, the facemask and the shoulder-pad slab; every
  football marking; and all football gameplay.
* **Extracted upward** — the parent-chain walk that both apps hand-rolled is
  figure *mechanism*, not football, so it moved into the engine as
  `FigureApi::posed_parts_from_joints`. End Zone's `player/rig.rs` calls it too.

## Running it

```sh
cargo test -p axiom-bend-it                                  # 148 native tests
uv run scripts/localhost_servers.py start-app bend-it        # play it
cargo run -p axiom-bend-it --example playthrough -- 12 1     # the agent plays
cargo run -p axiom-bend-it --example playthrough -- sweep    # balance sweep
```

`F1` toggles the debug view: the sampled 3D path, its two projections drawn on
the turf and up the goal plane, **the world points the last drawing was read as**,
the authored endpoint, the keeper's read and the reach it actually swept, and the
state machine as text — including how far the drawing strayed from the shot
fitted to it.
