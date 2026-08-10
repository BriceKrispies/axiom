# Axiom — Sky Drop

A pure-TypeScript `@axiom/game` leaf app. There is no `Cargo.toml`/`app.toml`/
`package.json`: this is a pure-TypeScript app over the shared engine (the
`@axiom/game` SDK + the `axiom-game-runtime` wasm core), so it is not a cargo
workspace member and `cargo xtask check-architecture` does not classify it.

## The game

You are **180 metres up**, standing off to one side of a target painted on the ground
far below, with a rack of **eight balls**. **Pick one up, swing it, let go.** It leaves
your hand at whatever speed you were actually moving it — and the next ball is in your
hand immediately, so you throw the whole rack as fast as you like, with several balls
falling at once. Press **R** for a fresh rack.

There is no aiming widget, no power meter, and no predicted-landing marker. The throw
is physical: the game reads the ball's own speed and direction off the motion you made
and lets the physics take it from there.

Three things follow from that, and each is load-bearing:

- **The camera never moves.** It sits at the stand for the whole round. A ball you
  throw falls *away* from you and shrinks toward a target that stays put, which is what
  makes 180 m read as 180 m. A camera that chased a ball would hold it at constant size
  against a growing target — the ground rising, not the ball falling — and with several
  balls in the air, chasing any one of them would yank the frame off the one still in
  your hand.
- **Nothing is scored until the rack is down.** No per-throw verdict, no points popup,
  no running total. A round is one continuous act of throwing, and interrupting it eight
  times with a scorecard breaks the rhythm.
- **But you are not throwing blind.** Balls stay where they land, so your grouping is
  visible on the ground the whole time. You can see you are throwing long, or that the
  wind is walking you left — you are simply never told in points.

The catch is the **wind**. It is the same for the whole rack — shown top-left as a
drift speed and an arrow — and it blows for the entire fall, enough to push an
uncorrected throw most of the way off the target. Reading one crosswind across eight
throws, and watching your grouping walk into the centre as you correct, is the game.
(Re-rolling the wind per ball would make that unlearnable, which is why `conditions.ts`
is keyed on the round.)

The only feedback while a ball is in your hand is a glow that swells with how fast you
are swinging it: it tells you about the *throw*, which is the thing you control, and
says nothing about where the ball will end up, which is the thing you are being asked
to judge.

## Lineage

This was `axiom-swipe-basketball`, an arcade cabinet game. What survives is its
mechanic: grab the ball, carry it, let go, and the ball keeps the motion you gave it —
plus the deterministic fixed-step ball simulator (`physics.ts`) and the
triangular-weighted release smoothing (now in `motion.ts`). The cabinet, the hoop, the
rack and the one-way scoring rule are gone.

Two attempts to "improve" the throw were tried and reverted, and the tests that guard
against them are still there:

1. A **pointer-velocity flick** (the cabinet's own model, measured in screen pixels).
   Careful aiming means dragging out and then holding still to line the shot up, which
   drove the measured velocity to zero — so the most considered throws came out as
   dead drops. §2c/§2e guard the replacement.
2. A **press-to-release displacement** with a predicted-landing reticle. That fixed the
   decay, and replaced throwing with operating a targeting widget. Precise, and not a
   throw at all.

What both got wrong is that they *interpreted* a gesture into a launch. The mechanic
that works does not interpret anything: the ball is a physical object you are moving,
and its velocity when you let go is the throw.

## Architecture

The gameplay core imports **nothing** from `@axiom/game`, so the whole game is
constructible in a bare `node --test` process:

- `constants.ts` — every tuning number. The comments record *why* each one is what it
  is; several are bounded at both ends by the framing geometry rather than by taste.
- `vec.ts` — the linear algebra everything else is built on.
- `conditions.ts` — the deterministic per-ROUND setup (where you stand + the wind) as a
  pure hash of `(seed, round, field)`. No RNG object, no hidden state.
- `motion.ts` — the held ball's recent positions, in metres, and the smoothed velocity
  it carries when you let go. This *is* the throw; nothing maps a gesture onto it.
- `projection.ts` — the camera math the grab and the carry need (project the ball to
  test a grab, unproject the finger onto the drag plane).
- `selection.ts` — is the pointer on the ball?
- `physics.ts` — gravity + wind + linear drag + the ground plane, plus a landing
  predictor the tuning tests use to prove every stand is throwable.
- `target.ts` — the scoring bands. The scene draws its rings from this same list.
- `round.ts` — the rack state machine (throwing → settling → results), and the rule
  that landings accumulate in silence.
- `viewpoint.ts` — the fixed camera, solved as a pure function of where you stand, plus
  the horizontal basis the HUD orients the wind arrow in.
- `session.ts` — the deterministic core that drives all of the above from `Intent`s.
  Balls are an ARRAY here, not a field: several are airborne at once and nothing in the
  update loop may assume otherwise.

Only `scene.ts` touches the engine, and `harness.ts` is the browser/DOM edge.

### Why the camera does what it does

The framing is the hardest constraint in the game and most of `viewpoint.ts` exists to
satisfy it. A target 180 m below and 26–48 m sideways sits within ~15° of straight
down, so the *only* angle that shows the stand and the target together is near-vertical.
That is also why the camera sits 30 m back: any closer and a 1.2 m ball fills as much of
the frame as the 30 m target beneath it, and the shot reads as a ball resting on a field
instead of one high above a landscape.

Because the camera is fixed, a thrown ball is a few pixels within a second. The **ground
shadows** do the real work of showing where each ball is: cast on the ground beside the
rings, widening with altitude, so they double as a crude altimeter — a tight dark disc
means that ball is about to land.

`sky-drop.test.ts` §7 asserts that the stand, the target, and the entire rim of arm's
reach all project inside the canvas, for every round.

## Running it

```sh
uv run scripts/localhost_servers.py start-app sky-drop --port 8090
```

## Tests

```sh
node --test web/src/sky-drop.test.ts
```

46 tests, no wasm and no DOM. Beyond the usual unit coverage, four groups are
load-bearing:

- **§6 tuning** pins the physics consequences the design rests on — the fall lasts
  ~2.8 s (bounded at both ends), every stand is reachable well inside the speed range,
  maximum wind is worth compensating without being an automatic miss, and no ball can
  be scored by releasing it without a throw. Retune gravity or drag and these fail
  loudly instead of quietly making the game unplayable.
- **§7 framing** asserts the camera constraint above, which is invisible in a
  screenshot right up until it is already broken.
- **§8 feel** guards the throw mechanic: a ball must be touched to be picked up, it must
  visibly follow the finger, a harder carry must fly further, the camera must **never**
  move — not while carrying, not while balls fall — the next ball must be in hand the
  instant one leaves it, several balls must be able to fly at once *on their own
  trajectories*, and landed balls must stay on the ground.
- **§9 silence** guards the rule that no score exists on screen until the rack is down:
  the round must not change phase when a ball lands, and the scoreboard must wait for
  every ball to be both thrown *and* settled. This is the constraint most likely to be
  eroded one convenience at a time.

## Two bugs this app flushed out, and where they actually lived

Neither was in this app's code, and neither was where it first appeared to be.

**A stale prebuilt wasm.** On a WebGL2 device the page rendered nothing but the
canvas's blue CSS background; on WebGPU there was a ~40 m dark square sitting on the
target. Both had the same cause: `apps/axiom-game-runtime/web/pkg/` — the shared wasm
engine every `@axiom/game` app loads — had been built on 2026-07-10 and never rebuilt.
The skinning-palette capability gate that stops the WebGL2 crash landed 2026-07-21;
the change that anchors the shadow volume on the view instead of the world origin
landed 2026-08-06. Both fixes were in the source, tested and passing, and neither was
in the binary being served. The dark square was the *old* origin-anchored shadow box,
which is 40 m across and centred exactly where this game paints its target.

The root cause was in `tools/axiom-serve`: `ensure_game_runtime_pkg` returned early
whenever the pkg *existed*, so it was effectively write-once. It now asks `cargo`
whether the runtime is up to date on every start (cargo being the only thing that can
answer, given the runtime pulls in most of the workspace) and re-runs `wasm-bindgen`
only when the binary actually changed.

If a browser app ever disagrees with engine source you can see is correct, suspect the
pkg before you suspect the engine.

**Z-fighting on the target rings.** The painted rings are six flat meshes stacked at
the same spot, and they were separated by 12 mm. At a 180 m viewing distance with a
0.08 m near plane the depth buffer resolves about 43 mm, so the rings interleaved into
speckle — invisible on WebGPU, glaring on WebGL2. Fixed here, in this app, by pushing
the near plane out to 0.5 m (nothing is ever nearer than 1.2 m) and separating the
layers by `GROUND_LAYER_STEP`. See the comments on `CAMERA_NEAR`.

## Backend differences worth knowing

- **WebGL2 has no vertex-stage storage buffers**, so the engine skips its skinned pass
  there. This game has no skinned geometry, so it is unaffected.
- WebGL2 renders noticeably lighter and less saturated than WebGPU. That is a
  backend-level difference in the post/tonemap chain, not something this app sets.
