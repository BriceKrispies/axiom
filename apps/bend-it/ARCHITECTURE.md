# Bend It — architecture

`apps/bend-it` (`axiom-bend-it`) is a **composition-leaf Axiom app**: a
mobile-first penalty-kick game whose entire mechanic is a trajectory editor.

The player does not aim a kick. They **draw the shape of one** — a point inside
the goal, a bend, a height — and then watch the kicker execute exactly that.
Everything in this document exists to keep one promise:

> The ball goes where you drew, and the only thing that can change its mind is a
> physical contact.

## The one-way flow

```text
pointer + keys
  → DeviceFrame → InputState                       axiom-input (neutral)
  → DragTracker                                    editor/drag.rs
  → Grab on a SculptPanel                          editor/sculpt.rs
  → EditorCommand                                  the ONLY thing gestures may say
  → ShotIntent (target + two BendCurves)           shot/intent.rs
  → ONE arc-length-uniform world Trajectory        shot/trajectory.rs
  → fixed-step attempt machine (60 Hz)             play/session/
  → keeper read, dive, capsule interception        play/keeper*.rs, contact.rs
  → camera fitted to this viewport                 camera.rs
  → retained scene submission                      scene/sync.rs
  → screen-space overlay view model                editor/view.rs
  → SVG painter                                    web/overlay.rs (wasm only)
```

Every arrow is one-way, and two of them are enforced by tests
(`tests/architecture.rs`):

* **`gesture_code_can_only_speak_in_commands`** — nothing under `src/editor/`
  may name a `Trajectory`, a `Ball` or a `Keeper`. Gesture code writes an
  `EditorCommand`; that is its whole vocabulary.
* **`nothing_downstream_of_the_trajectory_can_rewrite_it`** — `play/ball.rs` and
  the keeper may *read* the authored path and may never build one. A second
  producer is exactly how "the ball follows what you drew" quietly becomes "the
  ball follows what you drew, mostly".

## The mechanic, in three ideas

### 1. A shot is four numbers

One projection's editable state is a **cubic Bézier offset** away from the
straight line from ball to target, with both ends pinned to zero:

```text
offset(u) = 3(1-u)²u · w1  +  3(1-u)u² · w2
```

Two weights per projection, four in total. Direction, magnitude and *where the
curve breaks* are all read out of them rather than stored — which is why the
player never sees a control for any of them. A loop, a cusp, a path that leaves
the ball or misses the target are not rejected; they are unrepresentable.

### 2. The whole panel is the handle

Grab anywhere in the sculpt panel. The position *along* the shot becomes where
the curve peaks; the movement *across* it becomes how far it bends, one to one
and relative to whatever was already there. There is no handle to find and no
pixel to hit — which is the entire mobile-UX answer.

### 3. The keeper only sees the beginning

The keeper takes **one reading** at the end of its reaction, extrapolates it
ballistically to the goal plane, and dives. A beat later it gets **one lateral
correction** — it can still adjust its line, but it cannot un-commit its height.
Then it is executing, and whatever the ball does next it does unopposed.

That single limitation is why two shots to the same point are not the same shot,
and why *where* you put the peak of a curve is the central decision. The keeper
also remembers the last few finishes and shades its starting position and its
expected height toward them, so no one authored shape stays a solved answer.

## Boundaries

| Boundary | Lives in | Rule |
|---|---|---|
| **Tuning** | `tuning.rs` | Every gameplay number, one file. Systems read their own sub-table; no system hides a constant. |
| **Data** | `pitch/`, `figure/model.rs` | Coordinates, pitch geometry, the goal, the humanoid. Built once, never per frame. |
| **Authoring** | `shot/` | Pure data → one deterministic path. Cannot see a pointer. |
| **Simulation** | `play/` | One fixed 60 Hz step, a pure function of `(commands, tick)`. No clock, no randomness. |
| **Presentation** | `camera.rs`, `scene/`, `debug.rs` | Reads the session; can never write it. |
| **Interaction** | `editor/`, `projection.rs` | Screen space in, `EditorCommand` out. |
| **Agent** | `agent.rs` | Perceives the session, decides through `axiom-agent`, emits the same commands. |
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
  football marking; and all football gameplay. The pad girdle became an ordinary
  shoulder yoke, the head became a head, and the figure wears a kit.
* **Extracted upward** — the parent-chain walk that both apps hand-rolled is
  figure *mechanism*, not football, so it moved into the engine as
  `FigureApi::posed_parts_from_joints`. End Zone's `player/rig.rs` now calls it
  too.

## Running it

```sh
cargo test -p axiom-bend-it                                  # 140 native tests
uv run scripts/localhost_servers.py start-app bend-it        # play it
cargo run -p axiom-bend-it --example playthrough -- 12 1     # the agent plays
cargo run -p axiom-bend-it --example playthrough -- sweep    # balance sweep
```

`F1` toggles the debug view: the sampled 3D path, its two projections drawn on
the turf and up the goal plane, the authored endpoint, the keeper's read and the
reach it actually swept, and the state machine as text.
