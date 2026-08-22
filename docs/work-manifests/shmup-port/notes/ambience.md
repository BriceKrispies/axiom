# `fx/ambience.js` → `apps/shmup/src/fx/ambience.rs`

The last unported file of the source's `src/fx/` tree — the **visual** ambience
(dust motes, heat shimmer, smoke columns), not `audio/ambience.js` (already
ported as `src/audio/ambience.rs`, a different file entirely).

| what | where |
|---|---|
| module | `apps/shmup/src/fx/ambience.rs` (340 JS lines → ~800 Rust incl. docs/tests) |
| golden capture | `apps/shmup/tests/ambience/capture.mjs` |
| golden | `apps/shmup/tests/ambience/golden.json` (165 KB) |
| test | `apps/shmup/tests/ambience_port.rs` |

## Wiring the orchestrator must do

```
apps/shmup/src/fx/mod.rs: pub mod ambience;
```

and, in `mod.rs`'s table, a row `| [`ambience`] | `fx/ambience.js` |` plus the
removal of the "every file except `ambience.js`" caveat in its module doc.

Three further edits belong to `src/fx/system.rs`, a file this slice may not
touch. They are optional (nothing breaks without them) but they close the gap
that file's own module doc documents:

1. `FxSystem` gains `pub ambience: Ambience`, built in `FxSystem::new` right
   after `ShellSystem::new` with `AmbienceInit { motes: Some(mote as f64),
   box_size: None, shimmer: Some(particle_budget >= 4000) }` — `index.js:
   121-124`. **No RNG re-ordering is needed:** the constructor spends none
   (below).
2. `FxSystem::add_smoke_column` stops being a no-op:
   `self.ambience.add_column(x, y, z, &ColumnOpts::from(opts))` — the
   `From<SmokeColumnOpts>` impl already exists in `ambience.rs`. Add
   `add_smoke_source`/`remove_smoke_source` alongside (`index.js:625-637`).
3. `FxSystem::update` ends with
   `self.ambience.sun_factor = self.sun_factor; self.ambience.update(...)`
   (`index.js:786-787`). This needs a split borrow (`ambience` and the rest of
   `self`) — `std::mem::take`-style, or make `ambience` an argument the way
   this port already does. Taking it as an argument is why `Ambience::update`
   is shaped `(&mut self, fx: &mut FxSystem, ...)`.

**Deferral expiry:** if (1)–(3) are skipped, the expiry condition is "a caller
wants smoke columns from explosions, or motes in a frame". The file that must
change is `apps/shmup/src/fx/system.rs`; the symbols are
`FxSystem::add_smoke_column` (currently `{}`) and `FxSystem::update`.

## The RNG-divergence question `system.rs` raised, answered

`src/fx/system.rs`'s module doc flags `ambience.js` as "**the one place this
port's RNG stream can diverge from the real game's**, if `Ambience`'s
constructor draws". It does not. Golden-pinned: `construction[*].rngBefore ==
construction[*].rngAfter` for six different opts bags — `new Ambience(fx,
opts)` is pure state initialisation, no `fork()`, no literal seed, no draw.
`Ambience::new` therefore takes no `Rng` at all. That paragraph in `system.rs`
can be retired when (1) lands.

## Two source defects, ported faithfully and pinned

### 1. The `resetSpawn()` aliasing bug (`ambience.js:170-172`)

`resetSpawn()` returns the **single module-level `SP` object**
(`particles.js:56-71`), not a fresh one. `_puff` builds the smoke puff in `s`,
emits it, then — for the ember spark — calls `resetSpawn()` again into `t`.
`s` and `t` are the same object, so the second call has already zeroed `s.x`
and `s.z` by the time `t.x = s.x; t.z = s.z` runs.

**Every ember spark in the game spawns at world `x = 0, z = 0`**, never above
the smoke that produced it. Only `t.y = e.y` survives, because `e` is the
emitter — a different object.

Confirmed by running the original: in the golden's "ember always fires" block
the emitter sits at `(1.25, 0.5, -2.75)` and all three sparks land at
`(0, 0.5, 0)`.

This port's `reset_spawn()` returns a *fresh* `ParticleSpawn`, so a literal
transcription would silently **fix** the bug and diverge. `ambience.rs` writes
`t.x = 0.0; t.z = 0.0;` with the reason at the site. Pinned by
`ambience_port::ember_sparks_spawn_at_the_world_origin_reset_spawn_aliasing`.

Audited the rest of `src/fx/*.js` for the same shape: `ambience.js` is the
**only** file where two spawn descriptors are live at once (grep for two
`resetSpawn()` bindings in one scope with a cross-read; every other file's
second spawn reads `p`/`n`/`V`/`d`, none of which is `SP`). No other site is
affected.

### 2. The dead mote-delay branch (`ambience.js:236`)

```js
s.delay = this._warm <= 2 ? -rng.float() * s.life * 0.95 : -rng.float() * dt;
```

`_warm` is only incremented inside `if (this._warm < 2)`, so it **saturates at
2** — and the ternary tests `<= 2`. The condition is therefore always true and
the `dt` arm is unreachable: every mote, warm fill and steady-state trickle
alike, has its birth spread across `-life * 0.95`, not across one frame.

Both arms draw exactly one float, so the RNG stream is unaffected — only the
value differs, which is why nothing else in the source notices. Proved from
the original's numbers: in the 60 Hz trickle block the last mote (frame 5, long
past the warm fill) carries a delay of −4.2 s against a `dt` of 1/60. Pinned by
`ambience_port::mote_delay_always_uses_the_life_spread_dead_dt_branch`. Kept
written out, dead arm and all — dead computation in the source is part of the
source.

## Traps checked, by name

* **`rng.fork()` / literal seeds** — there are none in `ambience.js`. The
  constructor draws nothing (pinned).
* **Draw order** — the load-bearing property here; nothing else in this file
  is. Every block in the golden ends on an exact, zero-tolerance
  `fx.rng.float()`. `_puff` is 17 draws + 1 conditional ember roll (+7) + 1
  conditional haze roll (+1 inside `fx.haze`); `_motes` is 14 per mote;
  `_shimmer` is 8 + 1. Documented per-function against the source lines.
* **JS argument evaluation order** — `_shimmer`'s five RNG draws sit *inside*
  the `fx.haze(...)` argument list (`ambience.js:279-288`) and are drawn
  left-to-right before the call. They are hoisted into named locals in Rust so
  the order is explicit rather than incidental.
* **`Float32Array` storage width** — `ParticleLayer.array` is a
  `Float32Array` (`particles.js:265`) and the Rust `ParticleLayer` already
  narrows with `as f32`. The golden compares the **raw interleaved records**,
  32 floats per particle, so the narrowing is pinned, not assumed.
* **`Math.hypot` vs `Vector3.length()`** — `ambience.js` calls neither
  directly. The one normalisation it reaches is inside
  `Camera.getWorldDirection`, which is `Vector3.normalize()` =
  `divideScalar(length() || 1)` — the plain root, not hypot. `crate::jsmath`
  is correctly *not* used here.
* **`Camera.getWorldDirection` negates AFTER normalising**
  (`Camera.js:100-103` overrides `Object3D`'s). `camera_world_direction`
  reproduces that order; negating first and normalising second is different
  rounding.
* **`Math.round` ties toward `+Infinity`** — no `Math.round` in this file.
  `Math.floor` (mote accumulator) and `Math.min` are, and are ported as
  `f64::floor` / `f64::min`.
* **`for (let i = 0; i < rng.int(a,b); i++)` re-drawing per iteration** — no
  such loop here. The two loops with an RNG-influenced bound (`_motes`'s
  `for (i < n)` and `_puff`'s `while (acc >= 1 && guard-- > 0)`) both compute
  their bound **once**, into a local, before the loop. Checked explicitly.
* **`_acquire`'s `age / duration`** — `Infinity` duration yields `0`, so a
  pool full of persistent sources always recycles slot 0. Pinned by the
  `acquire` block, which ages 24 emitters unevenly and then overflows by 4
  (the original recycled slots 0, 3, 6, 9 — the shortest-duration ones).
* **`remove(0)`** — the source matches on tag with no active check, and every
  untouched emitter's tag is `0`, so `remove(0)` clears all 24. `_tag` starts
  at 1 so no issued tag is ever 0. Pinned.
* **`opts.shimmer !== false`** — a *strict* compare: `undefined` enables. Not
  `??`. Modelled as `Option<bool> != Some(false)`.
* **`_scan`'s defaults beat `addSource`'s** — three of them differ (`rate` 4
  vs 4.5, `ember` 0.2 vs 0.25, `haze` 0.3 vs 0.35), and `_scan` passes neither
  `duration` nor `growth`, so those two *do* fall through. Pinned by the scan
  block against a real `THREE.Scene`.

## Event payloads

`ambience.js` consumes **no events** — it is driven directly by
`FxSystem.update`. No payload type was added, so the audio/ui/weapons/fx fork
is unchanged at four. Nothing was missing.

## The two seams

`ambience.js` touches `THREE` in exactly two places, both named as traits/types
rather than reimplemented:

* **The camera** — `CameraFrame` (the matrix pair `fx/system.rs` already
  takes) plus `camera_world_direction`.
* **The scene graph** — `AmbienceScene`, three methods: `smoke_sources()`
  (`scene.traverse` filtered to `userData.fxSmoke`, **in traversal order** —
  the order decides emitter acquisition and therefore every subsequent draw),
  `attached()` (`!!o.parent`), `world_position()`
  (`setFromMatrixPosition(o.matrixWorld)`). Precedent: `fx::world::FxWorld`.

**One documented narrowing.** The source holds `e.object` as a direct
`Object3D` reference, so following it does not need the `scene` argument; here
it does. With `scene == None` an object-bound emitter keeps its last position
instead of following or deactivating. The game always passes `ctx.scene`
(`index.js:787`), so this is only reachable from a test that deliberately
passes `None`. Closing it properly would mean an object-resolver seam separate
from the scene seam — two traits where the source has one reference — which
did not seem worth the surface. Flagged rather than hidden.

## The golden

`capture.mjs` imports the **real** `Ambience` and the **real** `FxSystem`,
runs the real `async init(ctx)` against a stub WebGL surface, and drives real
`Ambience` instances frame by frame. Every particle in the golden was written
by the real `ParticleLayer.emit` into the real `Float32Array`; every RNG value
came from the real `src/core/rng.js`. Nothing is transcribed — there is no
GLSL in this file, so the `sky/`-style hand-transcription escape hatch does
not apply and was not used.

Three fixtures are computed rather than fabricated, each declared in the golden
so both sides read the same constants:

* a real posed `THREE.PerspectiveCamera` (its `matrixWorld` goes into the
  golden, so the Rust side is fed byte-identical input);
* an analytic ground ramp `0.4 + 0.05x − 0.03z` that returns `NaN` below
  `x = −30`, the only way to exercise `_shimmer`'s `Number.isFinite(h)` guard
  — both the finite and the NaN side are covered;
* a real `THREE.Scene` with parented tagged/untagged objects, whose traversal
  order and world positions the Rust `AmbienceScene` fixture replays.

Blocks: `camera`, `construction` (6 opts bags) + `builtin` (4 quality
presets), `tags` (defaults / `remove` / `remove(0)`), `acquire` (24-fill +
overflow), `puffs` (6 scenarios incl. the `guard = 8` clamp and mid-stream
expiry), `motes` (6, incl. the `min(64, …)` clamp and the `n <= 0` early
return), `shimmer` (6, incl. every gate and both ground paths), `scan` +
`scanFollow` (discovery, following, detach), and `integration` (300 frames of
motes + shimmer + two emitters + scanning at once).

## Tolerances (**unverified** — nothing was built or run in this wave)

* `rngNext`: **exact, zero tolerance.** Integer-arithmetic generator; this
  should hold bit-for-bit. If it does not, the port has a draw-order bug, not
  a tolerance problem — do not widen it.
* Counters, tags, flags, emitter indices: **exact.**
* Emitter scalars and the 32-float particle records: `REL = 1e-12` relative.
  The records are `f32` on both sides (≈1e-7 relative spacing), so 1e-12 is
  five orders below the storage grid — tight enough to pin the exact `f32`,
  loose enough to absorb a 1-ULP JSON decimal round-trip (a known
  `serde_json` wrinkle). If the integration block fails at 1e-12 but passes at
  1e-7, that is the round-trip, not the maths.

## Not done in this wave, per `12-final-wave-brief.md`

No build, no `cargo test`, no gate, no commit, and `mod.rs` untouched. The
capture script *was* run (Node, against the read-only `C:/dev/Claude-of-Duty`)
— that is the oracle, not a build. Both Rust files were parse-checked with a
standalone `rustfmt --emit stdout` (read-only, no target dir); they parse, but
they have **not been type-checked**.
