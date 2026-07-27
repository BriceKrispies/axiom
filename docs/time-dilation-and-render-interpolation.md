# Time dilation and render interpolation

A recipe for slowing a **fixed-timestep** simulation down for dramatic effect
without the result looking like a dropped-frame stutter.

Built for End Zone's decision window (a slow-motion beat where the player picks
a receiver), proven in the browser, then **switched off** in that game because
the design moved away from slow motion. The machinery is still in the tree and
still compiled — see *Re-arming it* — so this is a parked technique, not a
deleted one.

Status: **inert in End Zone** (`DECISION_TIME_SCALE = 1.0`). Nothing else uses it
yet.

---

## The problem it solves

A fixed-timestep simulation cannot be slowed by shrinking `dt` — the whole point
of a fixed step is that `dt` never changes, and everything downstream
(determinism, replay, physics stability) depends on that. So you slow it by
**spending ticks more slowly**: accumulate fractional credit each rendered frame
and step only when a whole tick is owed.

```rust
sim_credit += time_scale;              // e.g. 0.13
while sim_credit >= 1.0 {
    sim_credit -= 1.0;
    sim.step();
}
```

That is correct, deterministic, and — on its own — **looks broken**.

At `0.13×` a tick fires once every ~7.7 rendered frames. The ~7 frames in
between re-present a byte-identical previous frame. The screen holds still, then
jumps a whole tick's motion in one frame. That is an 8 fps slideshow played back
at 60 Hz, and the eye reads the *hold-and-jump cadence* as stutter no matter how
small each step is.

The trap worth naming: **making the dilation shallower does not help.** It only
trades stutter frequency for stutter size. We tried it; it looked equally wrong.

Measured, by hashing a strip of the canvas every frame and counting
byte-identical consecutive frames over ~35 s of play:

| | duplicate frames during dilation |
|---|---|
| tick-credit alone | ~87% (6.7 of every 7.7) |
| with interpolation | **0.2%** (3 of 1454) |

## The fix: draw *between* ticks

Keep the previous and current simulation states and render the frame at
`alpha = leftover tick credit`, blending the two. Every rendered frame becomes a
distinct, evenly-spaced pose and the motion is continuous at display rate.

This is the standard fixed-timestep remedy (Gaffer on Games, *Fix Your
Timestep*). The value here is the four rules that make it survive contact with a
real game.

### Rule 1 — blend continuous state, never discrete state

Positions, velocities, rotations, camera pose: blend. Animation state,
possession, roles, ball-hold, phase enums: **take the current value unblended**.
Interpolating an enum invents a state the simulation never had — a half-caught
ball is not a thing.

### Rule 2 — blend the skeleton too, not just the root

A smoothly gliding torso with limbs snapping at 8 Hz is a *different, weirder*
artifact, not a fix. Blend the joints as well. `nlerp` is correct here: per-tick
joint deltas are small enough that it is visually identical to `slerp` and much
cheaper.

### Rule 3 — treat large jumps as discontinuities, not motion

A reset, a re-spot, a teleport must not be smeared across the screen. Past a
threshold (End Zone used 3 yards/tick, ~20× a sprint) the new value simply wins:

```rust
if (curr.pos - prev.pos).length() > TELEPORT_THRESHOLD { continue; }
```

Without this, a formation reset drags every player across the field over several
frames.

### Rule 4 — interpolation costs one tick of latency, so skip it at full speed

Interpolating renders one tick *in the past*. That is inherent: the alternative,
extrapolating, overshoots and jitters on every direction change. At 60 Hz it is
~16 ms — irrelevant during a slow-motion beat where nothing needs frame-accurate
input, and unwanted during normal play.

So gate it:

```rust
let dilated = time_scale < 1.0;
if !dilated { return newest_step; }   // zero added latency
```

## Where it lives

| Piece | File |
|---|---|
| Tick credit + the render gate | `apps/end-zone/src/app.rs` (`advance`, `presented`) |
| Blending (all four rules) | `apps/end-zone/src/presentation/interpolate.rs` |
| The scale itself | `apps/end-zone/src/attempt/phase.rs` (`AttemptPhase::time_scale`) |

Landed in `e6da01ad` (*fix the decision window's slow motion reading as frame
stutter*); the tick-credit half arrived with the decision-window prototype in
`a62c8195`.

## Re-arming it

One constant. Set `DECISION_TIME_SCALE` in `apps/end-zone/src/attempt/mod.rs`
back to `0.13` and the whole path wakes up — the credit loop starts skipping
ticks and `presented()` starts blending, because both are already keyed on
`time_scale < 1.0`.

**Then re-tune the window durations.** They are expressed in *simulation ticks*,
so their real-time length is `ticks / (60 × time_scale)`. Dilating without
re-tuning shortens every window by the dilation factor, and vice versa. End
Zone's current values are sized for `1.0`.

## If it ever becomes an engine module

It is deliberately app-tier today. The tick-credit half is trivially generic; the
blending half is not — it reaches into this game's snapshot, pose and joint
types. Generalising it needs an interpolable-state abstraction the engine does
not currently have, and it would then owe the spine's branchless and
100%-coverage laws. Worth doing when a second game wants it, not before.
