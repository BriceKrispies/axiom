# `ui/index.js` → `apps/shmup/src/ui/system.rs`

Slice: the HUD subsystem facade — the integration layer that turns the eleven
already-ported widgets in `apps/shmup/src/ui/` into a running HUD.

| | |
|---|---|
| source | `C:/dev/Claude-of-Duty/src/ui/index.js:1-613` (whole file) |
| target | `apps/shmup/src/ui/system.rs` |
| test | `apps/shmup/tests/ui_system_port.rs` |
| golden | `apps/shmup/tests/ui_system/golden.json` (425 KB) |
| capture | `apps/shmup/tests/ui_system/capture.mjs` |

---

## 1. The golden is a real browser, not a DOM stub

`ui/index.js` cannot run under bare Node: `init()` builds a DOM overlay and
every widget constructor calls `document.createElement`. The brief forbids
faking a DOM and calling it an oracle, and rightly — a stub that answers
whatever the source happens to ask proves nothing.

So the capture runs the **original, unmodified `src/ui/index.js` in headless
Chromium**. Both ingredients were already installed in the source repo:
`playwright@1.61.1` (with Chromium binaries present under
`~/AppData/Local/ms-playwright`) and `three@0.180`. `capture.mjs`:

1. serves `C:/dev/Claude-of-Duty` over a throwaway `node:http` server, with an
   import map pointing the bare `three` specifier at
   `/node_modules/three/build/three.module.js`;
2. opens a page that imports the real `src/ui/index.js`, `src/core/rng.js`,
   `src/core/registry.js` (`EventBus`) and `src/core/config.js`;
3. builds a `ctx` with a real `THREE.PerspectiveCamera`, a real canvas, a real
   `EventBus`, a real `Rng(0x5eed1234)`, and a scripted clock/input;
4. `await ui.init(ctx)`, then drives 120 frames of a scripted scenario;
5. dumps the state, the derived scalars and an effect journal per frame.

**The DOM, the CSSOM, the canvas, `three`, and all eleven widgets are the real
things.** What is scripted is exactly what the source itself documents as
optional and duck-typed (`index.js:48-58`): the peer subsystems `weapons`,
`player`, `ai`, `audio`, plus the engine's clock and input. The clock advances
exactly as `core/engine.js:step` advances it (`dt = rawDt * time.scale`,
`elapsed += dt`, `raw += rawDt`), so the pause-freeze is real too.

Re-running the capture produces a **byte-identical** file (verified).

### What the golden covers

Per frame, after `lateUpdate`:

* the whole of `this.state` — all 26 fields, strings included;
* `hudVisible`, `hudTarget`, `k`, `vw`, `vh`, `_regenTimer`, `_lastKillAt`,
  `_lastRaw`, `_hadPointerLock`, `_bakeFrame`, `_prevPos`, `rawDt`;
* the normalised camera basis (`rx, rz, fx, fz`) and the compass heading;
* `_blipCount` and every live `_blips` entry;
* `_objectives` ids, `_compassObjs` (bearing/label/colour), and the whole
  `_mmState`;
* the live slot indices of all five pools (hit, arcs, killfeed, grenades,
  damage numbers) plus the objective pool population;
* `menu.open` and `menu.shown`;
* the camera's `matrixWorld` / `projectionMatrix * matrixWorldInverse` / eye /
  fov, so the Rust replay is fed the *same* camera rather than recomputing one;
* and the **effect journal** — every outward call the facade made into a widget
  or into `audio`, in order, with its arguments.

The journal is the heart of it. A facade *is* what it calls and when, and this
records `hit.spawn('head', slot 0)` → `crosshair.onHit()` → `sfx('hit_head',
0.7)` → `markers.spawnDamage([3,1.5,-4], 37, 'hs', slot 0)` as an ordered list.
It is collected by wrapping the methods on the live widget instances after
`init()`; the wrapper records **before** calling through (so a nested
`menu.toggle → menu.close` journals in call order, not completion order) and
appends the acquired pool slot afterwards.

### What the golden does NOT cover

* **`minimap.js`** — not ported anywhere in this repo. The golden records the
  facade's minimap *inputs* (`_mmState`) and the bake-gate decision, which are
  facade logic; it records nothing the minimap widget itself computes.
* **`demo.js` / `debugState('combat')`** — a screenshot/critic harness, not part
  of the HUD. Only `debugState('clean')` is exercised.
* **The widgets' own render numbers** (blade angles, ammo pips, marker pixels).
  Those belong to their own slices' goldens. This golden pins what the facade
  *feeds* them plus which pool slots came alive — enough to prove the wiring,
  without re-testing eleven other ports.
* **`menu.js`'s host effects** (`document.exitPointerLock`, `time.scale = 0`,
  `player.setControlEnabled`, `input.requestPointerLock`). Those sit behind
  `ui::menu::MenuHost`; the Rust test deliberately installs no host, and the
  golden's `requestPointerLock` journal entries are filtered out for that
  reason. Their *observable* effect — the frozen clock — is already baked into
  the golden's per-frame `dt`.

### The scenario

120 frames at 1/60 for the first 60 and then at the engine's 0.1 s clamp
ceiling (so `_regenTimer` can actually clear the 4.5 s regeneration delay
inside one capture). It walks: every event with and without its optional
fields; the player-target rejection; the kill path (hitmarker + damage number +
killfeed + banner + score); the 0.3 s `actor:death` credit window from both
sides; every defaulted argument (`hitmarker()`, `damageNumber(p, n)`,
`spawnGrenade(p)`, `hurt()`); `simulate` gating a `weapon:fire`; all three ways
the blip list is populated (`setBlips`, `ai.getHudActors()`, `ai.actors`) plus
the absent-`ai` case; objectives added/removed/nulled; `setMatch`;
`setHudVisible`; the pointer-lock-loss pause, the pause-key toggle, and
`pause()`/`resume()`; two `resize`s (one above and one below the `0.62..2.4`
clamp); `banner.show`; a direct `killfeed.push`; and `debugState('clean')`.

The **script itself is emitted into `golden.json`** and the Rust test replays it
from there, so there is no second hand-written copy of the scenario to drift.

---

## 2. Traps checked by name

* **An enum used as a table index is order-dependent.** The facade has three
  such mappings and none of them is an index: `HitKind` (`'hit'|'armour'|
  'head'|'kill'`) and `DamageKind` (`'hit'|'hs'|'armour'|'kill'`) are matched by
  *name* in `on_damage_dealt`'s two ternary chains, and the test maps the
  golden's strings back through the same names. `ReloadPhase` is reused from
  `crate::audio::foley` rather than re-declared, so there is no second variant
  order to get wrong. `DamageKind` is a **new** enum here: `markers.rs`'s
  `spawn_damage` collapsed the source's four-way kind to `is_kill: bool`
  (correct — only the dwell differs), so the full kind had to live somewhere,
  and the facade is where the source decides it.
* **Float arithmetic is not associative.** Three sites transcribed literally
  rather than tidied:
  * `(Math.atan2(…) * 180) / Math.PI` at `index.js:497`, `562`, `574` is
    **not** `f64::to_degrees`, which is `self * (180 / PI)` — a different
    grouping. `ui::system::atan2_degrees` / `radians_to_degrees` keep the
    source's multiply-then-divide. This is observable in the golden: the actor
    heading derived from `yaw = PI/3` is `59.99999999999999`, not `60`, and
    `degree_conversion_keeps_the_sources_grouping` asserts exactly that.
  * `Math.hypot(rx, rz) || 1` at `index.js:491-492` genuinely *is* `Math.hypot`,
    so the port uses `f64::hypot`, with a `|| 1` that falls through on **both**
    zero and NaN (JS falsiness).
  * `THREE.Vector3.length()` at `index.js:231` and `459-460` is
    `sqrt(x*x + y*y + z*z)`, **not** `hypot`. `ui::system::vec3_length` keeps
    that form.
* **Storage width is part of the algorithm — and it cuts both ways.** The
  usual form of this trap is porting a `Float32Array` as `f64`. The facade hit
  the *inverse*: `THREE.Vector3` stores plain JS numbers, so `_pos`,
  `_prevPos`, `_dir` and `_tmp` are `f64`, and the first draft narrowed them to
  `[f32; 3]` to match what `markers.rs` accepts. That is a ~1e-8 relative error
  in the movement bloom, the damage-arc direction and `_mmState` — four orders
  above the 1e-12 tolerance, and the golden caught it on **frame 1**. Positions
  are now `f64` throughout the facade, narrowing only in `narrow()`, at the
  `markers` boundary (which stores `[f32; 3]` in its pools and is pinned at
  that width by its own golden) and at `super::Blip`, whose `x`/`z` are `f32`
  and which the facade only ever copies into. See §8.
* **Preserve every `rng.fork()` and literal seed, in order.** `init` takes
  `ctx.rng.fork()`, then forks twice more: once for `WorldMarkers`
  (`index.js:82`) and once for the `Minimap` (`index.js:86`). The minimap is not
  ported, but the draw is still spent — `UiCore::new` binds it to
  `_minimap_rng` with a comment. `init_spends_both_rng_forks_in_order` pins
  `this.rng`'s four state words against the browser capture. (Note: JS stores
  the xoshiro words through `^=`, which yields a **signed** int32, so the test
  converts through `as i32 as u32`.)
* **Port source defects faithfully.** Two are pinned by
  `source_quirks_are_ported_not_fixed`:
  1. `_collectBlips` returns *before* touching `_blipCount` when the `ai`
     subsystem is absent or publishes a non-array (`index.js:552`). The blip
     list therefore **survives** the AI going away rather than clearing. The
     golden shows the count held at 2 across frames 64–101.
  2. `actor:death` within 0.3 s of a `damage:dealt` kill is dropped
     (`index.js:220`) — deliberate de-duplication, but it silently swallows a
     *second, unrelated* death in that window.
  A third is documented but structurally unreachable: see §4.

---

## 3. What is ported, and where the browser edge is

Everything in `index.js` except the DOM. The split is exactly the one the
widgets themselves already draw, and the one `audio/system.rs` draws:

| `index.js` | port |
|---|---|
| `init` state, forks, `onBeat` hook | `UiCore::new` + `UiCore::init` |
| `installStyles()`, root + 4 layers | `ui::system::view::HudRoot::install` (`wasm32`) |
| `lateUpdate`'s three `opacity` writes | `view::HudRoot::apply(&UiFrame)` (`wasm32`) |
| `root.style.setProperty('--k', …)` | `view::HudRoot::set_scale` (`wasm32`) |
| everything else in `lateUpdate` | `UiCore::late_update` → `UiFrame` |
| the 7 `ctx.events.on(...)` | `UiSystem::wire_events` |
| the public API | `UiCore`'s inherent methods |
| `dispose()` | `UiSystem::dispose` + `view::HudRoot::dispose` |

`UiCore::late_update` returns a `UiFrame` carrying every widget's computed
render state, the derived scalars, `_mmState`, and the effect journal. The
`wasm32` view is a transcriber with no decisions of its own.

`UiSystem` is a real `crate::registry::Subsystem` (`id = "ui"`,
`deps = ["render"]`, phases `LateUpdate` + `Resize`), wired through the real
`EventBus` — the test builds it through a real `Engine` so the subscriptions
under test are the ones `wire_events` makes.

---

## 4. Divergences, each deliberate

1. **`ctx.peek` becomes setters.** `weapons`/`player`/`ai` arrive through
   `UiCore::set_links`, `ctx.camera` through `set_camera`, `ctx.input` through
   `set_input`, `ctx.time` through `set_clock`. Same reason and same shape as
   `AudioCore::set_listener_basis`: `Ctx` carries no camera, no input, and none
   of those three subsystems exists in this port yet.
2. **The live reads inside event handlers are cached.** `_playerPos()` and
   `ctx.time.elapsed` are re-read at every event in the source; a handler here
   has no `ctx`. So `set_camera`/`set_links` recompute the player position the
   moment either input changes, and `set_clock` takes the clock. Call the four
   setters at the top of the frame — where `core/engine.js` advances the clock
   and where the camera reaches its final transform — and the handlers see
   exactly what the source sees. The golden proves this: the arc direction at
   frame 22 is `dx = 8.9`, which is `10 − camera.x(f22)`, not `10 − camera.x(f21)`.
   *Note* `state.time` is deliberately NOT that clock — the source writes it
   only inside `lateUpdate`, so during a frame's `update` phase it still holds
   the previous frame's `elapsed`, and `ammo.js` reads it in that state.
3. **`sfx(id, gain)` becomes data.** The source's fire-and-forget call into an
   optional `audio` subsystem is recorded as `UiEffect::Sfx { id, gain }` with
   the source's literal ids (`hit_kill`, `hit_head`, `hit_armour`, `hit_flesh`,
   `player_hurt`, `grenade_warn`, `regen`, `heartbeat`), which `audio/index.js`'s
   `UI_ALIAS` resolves. The caller forwards them. The source's `try {} catch {}`
   exists precisely because the HUD must not care whether anyone is listening.
4. **`ps.armour ?? ps.armor`** (`index.js:443-444`) — `super::PlayerPull` spells
   it `armour` only; the emitter picks.
5. **The `o._cmp` / `o._mm` scratch caches** (`index.js:539`, `575`) attach a
   scratch object to each objective and push *that object* into the output
   list. If the same objective appears twice in `_objectives`, both output
   entries are the same object and both read back the last write — a real
   aliasing defect. The port stores objectives **by value**, so the aliasing
   cannot arise; the observable behaviour for distinct objectives is identical.
   Documented rather than reproduced, because reproducing it would mean
   manufacturing shared mutable objects for no behavioural gain.
6. **`demo.js` is not ported**, so `debug_state(DebugState::Combat)` returns
   `DebugReport::CombatUnavailable` instead of starting a timeline. `Clean` and
   `Menu` are complete. `HudState::simulate` stays public so a future port of
   the timeline takes the numbers over exactly as the source does.
7. **`minimap.js` is not ported**, so `minimap.resize(k)` is absent and the bake
   gate's `bakeDone` is permanently `false` (which is what the source does until
   a bake succeeds). `UiFrame::minimap_bake_requested` and `UiFrame::minimap`
   carry the facade's own minimap work forward.

---

## 5. Two things the orchestrator must resolve — NOT done in this slice

### 5.1 `ui::mod.rs`'s `Hud` is a second, partial port of the same file

`apps/shmup/src/ui/mod.rs` already contains a `Hud` struct that ports a subset
of `index.js` ("`UiSystem` minus the `Subsystem` impl", per its own doc
comment). The fan-out plan lists `ui/index.js` as unported, and this slice was
briefed to write a new `system.rs`, which I did — I was told not to edit any
existing file under `src/ui/`.

`ui::system::UiCore` **supersedes** `ui::Hud`. It is the whole file, it has a
golden, and it fixes three things `Hud` gets wrong:

* `Hud::movement_bloom` uses `dx.hypot(dz)`, but the source goes through
  `THREE.Vector3.length()` — `sqrt(x*x+y*y+z*z)`. Different rounding.
* `Hud::camera_heading_deg` and `Hud::compass_bearing` use `to_degrees()`
  (`self * (180/PI)`), where the source is `(x * 180) / PI`. Different grouping.
* `Hud::late_update` does `s.time += dt` where the source assigns
  `s.time = t.elapsed`, never calls `markers.update_*`, drops the
  `player.health` fallback arm, drops `s.ads = input.ads && input.enabled`, and
  omits the pause/pointer-lock block, `_collectBlips`, the objective API,
  `setBlips`, `setMatch`, `debugState` and `_mmState`.

**Recommendation:** delete `Hud`, `HudFrame`, `movement_bloom`,
`camera_heading_deg`, `compass_bearing` and `CameraBasis` from `mod.rs`,
keeping `Blip`, `HudState`, `WeaponPull` and `PlayerPull` (which `system.rs`
imports and reuses). `mod.rs`'s `#[cfg(test)] mod tests` goes with `Hud`.
`system.rs` re-declares `CameraBasis` because `mod.rs`'s version documents
itself as pre-normalised while the source normalises inside `lateUpdate`.

### 5.2 The event-payload vocabulary is forked

`EventBus` payloads cross as `&dyn Any` and each handler downcasts to **one**
concrete type, so there must be exactly one payload type per event name across
the whole game. Today there are two:

| event | `crate::audio::system` | `crate::ui::system` | HUD needs that audio lacks |
|---|---|---|---|
| `weapon:fire` | `WeaponFire` | `WeaponFire` | `recoil` |
| `weapon:reload` | `WeaponReload` | `WeaponReload` | (compatible) |
| `damage:dealt` | `DamageDealt` | `DamageDealt` | `armour`, `amount`, `target_name`, `name` |
| `damage:taken` | `DamageTaken` | `DamageTaken` | `from` |
| `actor:death` | `ActorDeath` | `ActorDeath` | `by_name`, `actor_name` |
| `explosion` | `ExplosionEvent` | `ExplosionEvent` | (compatible) |
| `player:state` | `PlayerState` | `PlayerStateEvent` | `sprinting` |

Until they are unified into one superset per event name, **only one of the two
subsystems will see any given emit**. That is a whole-game decision (it also
touches every other facade slice landing in this fan-out), so it belongs in the
integration pass, not here. There is a loud comment above the payload block in
`system.rs` saying so.

---

## 6. Lines the orchestrator must add

```
apps/shmup/src/ui/mod.rs:  pub mod system;
```

Nothing else. No `Cargo.toml` change: the test's only dependency,
`serde_json` with `arbitrary_precision`, is already a dev-dependency, and
`axiom-math` (for `Mat4`) is already a dependency.

---

## 7. Tolerances

`tests/ui_system_port.rs` compares every float at `REL = 1e-12` relative with
an absolute floor of 1, and everything integer-, boolean- or string-valued
exactly. `damp` is `1 - exp(-rate * dt)` and the bearings go through `atan2`,
neither of which is bit-guaranteed across V8 and Rust's libm; 1e-12 is still an
extremely tight pin, because a single wrong `dt`, `raw_dt` or draw order moves
these values in their first significant digit, not their twelfth.

Pool slot indices, the effect journal's structure and ordering, the blip
friend/foe flags, the objective ids, the state's strings and every integer
field are compared exactly.

---

## 8. Integration-pass fixes (post-first-compile)

The slice was written under the fan-out's no-build rule and compiled for the
first time by the orchestrator. Two things came back. Both are recorded here
because both were instructive.

### 8.1 `Box<dyn MenuHost>` cannot be reborrowed short — 15 errors, one cause

`self.menu_host.as_deref_mut()` on an `Option<Box<dyn MenuHost>>` yields
`&mut (dyn MenuHost + 'static)`, because `Box<dyn Trait>` defaults its
trait-object lifetime to `'static`. `PauseMenu::show` wants
`Option<&mut dyn MenuHost>`, whose elided object lifetime is the reference's
own — and `&mut` is **invariant** in its pointee, so the two cannot be
unified except by making the reference `'static` too. The compiler duly
required the `&mut self` borrow to outlive `'static` (E0521), which poisoned
every other borrow in the same method: 15 errors, a mix of E0521, E0502 and
E0499, from one shape.

Confirmed with a standalone 40-line repro before touching the file, which also
ruled out the two obvious repairs:

| shape | result |
|---|---|
| `Option<Box<dyn Host>>` field, `as_deref_mut()` | **E0521** |
| host as an `Option<&mut dyn Host>` *parameter*, used once | compiles |
| …but fed from a boxed field by the caller | **E0521** (moves the problem) |
| …used **twice** in one method (the `lateUpdate` shape) | **E0597 + E0499** — the reborrow is pinned to the parameter's own lifetime, so it cannot be taken twice |
| generic `Option<H>` field, `h as &mut dyn Host` | compiles, twice included |
| **concrete slot type, `is_installed().then(\|\| &mut self.slot as &mut dyn Host)`** | **compiles, twice included** |

The last is what landed, because it needs no generic parameter rippling
through `UiCore`/`UiSystem`/every call site. `MenuHostSlot` is a concrete
newtype around the `Option<Box<dyn MenuHost>>` that forwards every `MenuHost`
method; unsizing a **concrete** `&mut T` to `&mut dyn Trait` picks the object
lifetime freely, which is exactly what the invariant reborrow could not do.

**No behaviour changed.** `Some` is still produced only when a host is
genuinely installed (`is_installed()` gates it), so `PauseMenu` sees the same
`Option` it always would; the forwarding impl's empty arms are unreachable
while nothing is installed. No effect was reordered — the `UiEffect` pushes sit
before and after the host block exactly where they did.

### 8.2 A real numeric bug: `f32` positions where THREE keeps `f64`

`late_update_matches_the_original_frame_for_frame` failed on **frame 1**:

```
move: got  9.44675592261636221e-2
      want 9.44675583038929179e-2      (9.8e-9 relative, tolerance 1e-12)
```

Four orders too large for libm noise, so it was a different quantity, not a
different rounding. Reproduced against the golden in Node before changing
anything — the decisive table:

| position width | length function | `move` |
|---|---|---|
| `f64` | `sqrt(x*x+y*y+z*z)` | `0.09446755830389292` **= golden** |
| `f64` | `Math.hypot` | `0.09446755830389292` = golden |
| `f32` | `sqrt(x*x+y*y+z*z)` | `0.09446755922616362` **= the port's wrong value** |
| `f32` | `Math.hypot` | `0.09446755922616362` |

So `hypot` vs `Vector3.length()` was **not** the culprit here (they agree on
this input); the culprit was `[f32; 3]` positions. The port had narrowed
`CameraState.position`, `prev_pos`, `player_pos`, the objective positions and
the three event payload points to `f32` to match what `markers.rs` accepts —
but the facade computes with them, and `THREE.Vector3` is `f64`.

Fixed by widening every position the facade does arithmetic on to `[f64; 3]`
and narrowing in exactly one function, `narrow()`, at the two carriers that
genuinely store `f32`: `markers` (its own golden is pinned at that width) and
`super::Blip` (a pure copy target, never computed from). All 120 frames then
matched at 1e-12.

**The port was wrong, not the golden.** The golden was never re-captured — it
is byte-identical to the original capture, re-verified after the fix.

### 8.3 And one over-claiming test

`degree_conversion_keeps_the_sources_grouping` asserted
`assert_ne!(rad.to_degrees(), yaw_derived)` on `rad = PI / 3`. Those two are
bit-identical on that input, so the assertion was testing a coincidence, not
the grouping. Worse, the first repair — recomputing the heading from each
frame's captured basis and demanding bit equality with the golden — compares
**V8's `Math.atan2` against Rust's**, which are not required to agree in the
last bit; it failed at frame 119 by one ULP for that reason, not for a
grouping reason.

The test now splits the two claims:

* the **positive** claim is asserted only where there is no transcendental —
  `_collectBlips`'s `(yaw * 180) / PI`, whose captured value is the inexact
  `59.99999999999999`;
* the **negative** claim searches for a bearing on which the two spellings
  genuinely disagree, feeds it through the port's own
  `UiCore::compass_objectives`, and asserts the result is
  `(a * 180) / PI` and not `a.to_degrees()` — both evaluated in Rust, so the
  grouping is the only variable.

Cross-libm agreement on `atan2` is where it belongs: in the frame-by-frame
test, at `REL = 1e-12`.

### Result

`cargo test -p axiom-shmup --test ui_system_port` — **5 passed, 0 failed.**
