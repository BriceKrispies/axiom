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

## The mechanic, in four ideas

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

Its dive is **integrated, not parameterised**: a position and a velocity,
accelerating toward whatever it is committed to, capped at its own top speed. The
mid-flight correction moves the *target* and never touches the body. That is the
fix for a real defect — the dive used to be an eased curve over a re-settable
interval, and the correction reset both ends of it. A smoothstep begins at zero
velocity, so a keeper mid-dive at full speed was stopped dead and made to
accelerate again: it committed a bit, stopped, then carried on. A body with
momentum cannot do that whatever its target does.

Its **arms are solved onto the point it is throwing its hands at**, and the
capsule the ball is tested against is built from those same solved fingertips —
one description of the reach instead of two that could drift apart. It reaches for
its *read*, never for the ball: a keeper that could aim its hands at the real ball
would save everything it could get near, and the whole reason a shaped shot beats
a keeper is that its read is wrong. The dive aims the **hips** an arm's stretch
short and lets the arm cover the rest, which is what a keeper does — and that
stretch is *measured* off a full-stretch pose rather than written down, because
the arms are bounded by the figure's ranges of motion and only the skeleton knows
how far they really get.


It reads once at the end of its reaction, dives, and gets **one lateral
correction** — it can still adjust its line but cannot un-commit its height. It
also remembers where the last few penalties finished and shades both its starting
position and its expected height toward them, so no shape stays a solved answer.
Its *memory* is not hedged the way a single glance is: four penalties into the
same corner is evidence, not a guess.

And it has **nerves**. Everything about one penalty that is not the same twice —
how fast it reacts, how far out its judgement is, how completely it follows
through, whether it abandons the read and simply picks a side, whether it gets
its correction at all — is drawn once, up front, from the session's seeded
generator (`play/nerve.rs`). Nothing during the flight rolls anything.

Drawing it all up front buys three things: the tick loop stays a pure function
of `(nerve, trajectory, t)` and is testable at a fixed nerve; a penalty replays
exactly, because the whole of its luck is five numbers; and the variation is
*inspectable* — `F1` prints whether the keeper read you or guessed.

`Session::steady` faces the average keeper with no roll in it at all. That is
what the mechanic tests play against, so "a bent shot beats a keeper that read it
straight" is a claim about the mechanic rather than about a lucky roll. Nothing a
player ever meets is steady.

### 4. The kick is solved, not scheduled

*How fast* and *how* you drew are read alongside *where* — the tempo of the line
becomes a `Pace` (`stroke/pace.rs`), its shape becomes a `KickDrive`
(`figure/strike.rs`) — and between them they drive the body.

**Speed is the authored quantity; time is derived.** A shot names the speed the
ball *leaves at*, between 100 km/h and 160 (`FlightTuning::slow_launch` /
`fast_launch`), and the flight time falls out of it — the path length, the launch
speed and the fixed exponential bleed determine each other, so nobody chooses how
long a penalty takes. That is the right way round: "how hard was it hit" is a
number a person can check against a real penalty, and "1.1 seconds to cross 11
metres" is how the ball ended up floating in at 35 km/h.

Measured over the coarse matrix: **launch 100–151 km/h, arriving at 86–129, in
0.28–0.47 s**, keeping ~85% of its pace to the line — which is a real ball's
drag, not a ball on a string.

The game shows the figure, and shows it **twice**, in the same place. The readout
under the score is faint while a line is under the finger — what the shot *would*
leave at if it were let go now, read with the very same call the finished line
gets — and solid from the moment of contact, taken off the ball rather than read
back off what the shot was authored at. A promise, then the fact.

That is what makes the tempo mechanic legible. The speed of your hand has decided
how hard the ball is hit since the pace work, but you could only find out
afterwards; now you watch your own tempo become a number while you are still
drawing. A preview that could disagree with the kick would be worse than none at
all — it would teach you something untrue about your own hand — so a test draws a
real shot in real pixels, keeps what the screen promised, runs it to the strike
and asserts the two agree within 3 km/h
(`the_speed_the_line_promises_is_the_speed_the_ball_leaves_at`).

The striking leg is a **driven pendulum**: the hip applies a torque, the leg has
inertia and damping, and the swing is integrated (at eight substeps a tick — the
downswing is only about six ticks long, so tick resolution would swing the boot
straight through where the ball was). So the tick the ball leaves is not a
constant anywhere; it is whatever the integration produces.

And the torque is not a free number either — it is **derived from the speed the
shot has to leave at**:

```text
v_boot = launch / ball_off_boot     ω = v_boot / leg_length     τ = I·ω² / (2·Δθ)
```

so a 160 km/h shot is *visibly* a harder swing than a 100 km/h one: more torque,
a leg that reaches the ball sooner, a follow-through carrying the speed it
genuinely had. The animation and the flight cannot drift apart because there is
only one number, and a test (`the_leg_is_moving_as_fast_as_the_ball_it_sends_away`)
asserts the boot really does arrive at 20–35 m/s to match.

The joints that put the boot on the arc are **solved**, not posed
(`figure/ik.rs`): a two-bone IK for the striking leg onto a target on the swing
arc, and another for the support leg onto a foot that is planted in the world and
stays there. That is what lets the rest of the body vary at all — a bent shot
plants wider and opens the hips through the ball, a lofted one plants further
behind it and leans away, a hard one commits forward — while the boot still meets
the ball, because meeting the ball is a geometric fact rather than a tuned number.

### 5. A joint only does what a joint can do

A solve produces a *rotation*, and a rotation is unbounded. The IK will hand back
125° of hip abduction if that is what the arithmetic says; a knee will bend
backwards; a shin will roll about its own length. That is not the solve
misbehaving — it is the solve doing what it was asked, and nobody having said
what a leg is.

`figure/joints.rs` says. Each joint declares a **range** — how far the limb may
swing from rest in each of four directions, and how far it may twist about its own
length — and every pose is put through it before anything draws or tests against
it. The numbers are the ordinary clinical ones: a hip flexes 120° and extends 22°,
a knee flexes 140° and does not hyperextend, an ankle plantarflexes 50°.

A rotation splits into a **swing** (where the limb points, bounded by the joint
capsule) and a **twist** (the limb rotating about its own length, bounded by the
ligaments — the one nobody thinks about and the one that reads as *broken* rather
than merely strained). Each is clamped against its own budget, the swing against
an ellipse blended from the four directional limits, and they are put back
together. The split is exact, so a legal rotation comes back untouched: the limits
cost nothing until something asks for the impossible. A hinge needs no special
case — a knee is just a joint with a large backward budget and zero of everything
else.

Two structural bugs came out of building it, both of the "the symptom was in the
animation and the cause was three layers down" kind:

* The IK derived its hinge axis by **crossing the reach with a fixed "the knee
  points forward" pole** — the textbook formulation, with a singularity exactly
  where a kick lives. When the leg comes up past horizontal the reach is parallel
  to "forward", the cross product collapses, and the hip rolls through 125° on its
  way to nothing in particular. It now projects a *stable* axis (a knee turns
  about the body's lateral axis wherever the leg is), whose own singularity — a
  leg pointing straight out sideways — is nowhere a leg goes.
* The swing's contact snap was being **undone by the remaining substeps of the
  same tick**, so the one frame anyone actually sees of the strike had the boot
  already through the ball by a hand's width. The leg now rests on the ball for
  the frame it strikes it, which is what a real ~10 ms contact does anyway.

The one thing deliberately held fixed is the **frame the swing is solved in**: the
strike's root lean, roll and lift are a function of the drive alone and do not
grow through the swing. A lean that accumulated mid-swing would move the hip out
from under its own arc and put the boot *through* the ball instead of on it.

The hip also runs out of travel (`follow_through_limit`) rather than carrying on
over the top. An unbounded integrator is happy to swing the leg over the kicker's
head; a hip is not a windmill.

## How often does the keeper save it?

`src/matrix/` sweeps the whole authorable space — every corner, every bend,
every arc, every place a curve can break — against a run of seeded keepers, and
counts. It is the game's tuning instrument as much as its test: change a keeper
number, run the sweep, and see what it did to *every* shot rather than to the
three you thought to try.

```sh
cargo test  -p axiom-bend-it --test keeper_sweep            # the coarse sweep
cargo run --release -p axiom-bend-it --example keeper_report -- 96
```

Over 11,115 shapes × 96 keepers (1,067,040 penalties) the keeper saves **≈ 38%**
— and that is a *uniform* average over the shape space, not over how anyone
actually shoots. Aim at a corner and shape the ball and it falls to under 20%.

| | saved |
|---|---|
| everything | 38% |
| flat, no arc | 52% |
| shaped | 35% |
| down the middle | 76% |
| into a corner | 20% |

The keeper is calibrated **against the flight time, not against a feel**: at
0.3–0.5 s it reacts in 0.09 s, commits, and gets one lateral correction 0.13 s
later. Those are a real keeper's numbers, and they are why the shape of the
line still buys something — most of a curve happens after the correction — while
a shot down the middle is still the easiest thing in the game to save.

**A seed is a keeper.** Each one produces exactly one nerve for a cold attempt,
so a handful of seeds is a handful of keepers and aliases hard: at eight seeds
the report came back visibly lopsided — one keeper that guessed right handed
every right-aimed shot in the matrix a free save. The number only settles past
about 96.

## Boundaries

| Boundary | Lives in | Rule |
|---|---|---|
| **Tuning** | `tuning.rs` | Every gameplay number, one file. Systems read their own sub-table; no system hides a constant. |
| **Data** | `pitch/`, `figure/model.rs` | Coordinates, pitch geometry, the goal, the humanoid. Built once, never per frame. |
| **Authoring** | `shot/` | Pure data → one deterministic path. Cannot see a pointer. |
| **Simulation** | `play/` | One fixed 60 Hz step, a pure function of `(commands, tick, seed)`. No clock; the only randomness is the kernel's seeded generator. |
| **Presentation** | `camera.rs`, `scene/`, `debug.rs` | Reads the session; can never write it. |
| **Interaction** | `stroke/`, `projection.rs` | Pixels in, one `ShotIntent` out. |
| **Agent** | `agent/` | Perceives (`eyes`), decides through `axiom-agent`, and *draws* (`hand`) — its whole output is pixels. |
| **Measurement** | `matrix.rs` | Every shot vs the keeper, headless and reproducible. |
| **Platform edge** | `web/` (wasm32) | The only nondeterministic directory. |

## Determinism

The simulation's only time is the tick counter, and its only randomness is the
kernel's seeded generator — which reads no entropy, no clock and no global state.
A session is `(commands, seed)` and nothing else, so the same pair is the same
shootout on any machine. That is what makes both `agent::play_through` and the
whole shot matrix reproducible measurements rather than anecdotes: a surprising
cell in a million-penalty sweep can be replayed exactly from the one
`(shape, seed)` pair that produced it.

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
cargo test -p axiom-bend-it                                  # 205 native tests
uv run scripts/localhost_servers.py start-app bend-it        # play it
cargo run -p axiom-bend-it --example playthrough -- 12 1     # the agent plays
cargo run -p axiom-bend-it --example playthrough -- sweep    # balance sweep
```

`F1` toggles the debug view: the sampled 3D path, its two projections drawn on
the turf and up the goal plane, **the world points the last drawing was read as**,
the authored endpoint, the keeper's read and the reach it actually swept, and the
state machine as text — including how far the drawing strayed from the shot
fitted to it.
