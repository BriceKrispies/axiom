# Finishing `fx/index.js` → `apps/shmup/src/fx/system.rs`

`fx/system.rs` already existed as a partial port. This pass audited its
"deliberately not ported" claim function by function, found most of it to be
unfinished work wearing a justification, and completed the file.

| file | what changed |
|---|---|
| `apps/shmup/src/fx/system.rs` | completed (was ~683 lines, now ~1900) |
| `apps/shmup/tests/fx_system/capture.mjs` + `golden.json` | new |
| `apps/shmup/tests/fx_system_port.rs` | new, 15 tests |

## The claim that was wrong

The old module doc said `index.js` was "genuinely inseparable from THREE's
scene graph" and filed as unportable: `_syncLighting`, `muzzleFlash`/
`viewFlash`, `onWeaponFire`, the entire `debugBurst`/`_findTarget`/`_stage*`
capture harness, and the per-frame `update`/`lateUpdate`.

That is the same mistake `sky/mod.rs` records against itself. A camera
transform is a 4x4 matrix; a matrix is not a GPU. All of the above are now
ported, taking the matrices they read as an explicit `FxFrame` argument.

**Genuinely unportable, and all that remains unported:** `init`'s
`scene.add`/`registerPass`/`addLight` and `dispose`'s teardown;
`prewarmMaterials`/`_viewmodelPresent` (whose only observable is
`renderer.info.programs.length` — but the self-scheduling counter that decides
*when* it runs is ported and pinned); and `_pushLighting`, six copies of
already-computed values into `ShaderMaterial` uniforms (the values are all
published as fields).

## What was newly ported

- **`_syncLighting`** — the `sunFactor` clamp, the sun-derived `ambTop`/
  `ambBot`, the fog resolve, and the view-space sun/up directions for both
  cameras. Plus `setAmbient`, the `sky` override seam.
- **`update` / `lateUpdate`** — the whole frame drive: light-pool ticks, the
  script, the shell integration, five layer flushes, the decal and haze
  flushes, `stats.live`, and the second-frame pre-warm schedule.
- **`muzzleFlash` / `viewFlash` / `_toView` / `_fromView`** — the facade's
  space marshalling, split out as `resolve_muzzle_frame` so the conversions can
  be pinned away from `muzzle.js`'s several dozen RNG draws.
- **`onWeaponFire`** — including the `distanceToSquared < 2.25` first-person
  test.
- **The entire debug-burst harness** — `debugBurst`, `_stageWallHits`,
  `_impactAt`, `_stageMuzzle`/`_stageShell`/`_stageTracer`/`_stageCrossfire`,
  `_findTarget`, `_runScript`, `_fire`.
- **`MUZZLE_LIGHT` + `weaponKey`** (`index.js:1293-1310`), which are **dead
  code in the source** — nothing in `index.js` calls them. Ported per the
  recipe's "dead computation in the source is still part of the source".
- **Event payload types** — see below.

## Defects found in the existing port

1. **`stats.spawned` counted the wrong thing.** The old port incremented it in
   all four `emit_*` helpers; `index.js` has exactly one `stats.spawned++`, in
   `onImpact` (`index.js:385`). It counts *impacts*, not particles — two orders
   of magnitude out. The golden shows `statsSpawned: 0` after a full
   18-round `debugBurst('impacts')` loop (because `_impactAt` calls
   `spawnImpact` directly, never `onImpact`), which is what caught it.
2. **`stats.live` was missing entirely** (`index.js:809`). It is
   `add.spawned + lit.spawned` — cumulative counters, despite the name. Ported
   as the source has it, with a comment.
3. **Every physics mask was hardcoded `0xffff`.** The source passes
   `ph.MASK.WORLD` — `STATIC | PROP` = **3** — at `scorch`, `bloodSpatterBehind`,
   `_findTarget`, `_impactAt`, and `addDecal` (`o.mask = ph?.MASK?.WORLD ??
   0xffff`, where `0xffff` is only the *no-physics* fallback). Fixed to
   `crate::physics::surfaces::mask::WORLD`, with the fallback kept for
   `addDecal`.
4. **`this.now` was written at the wrong time** in four handlers. The source
   writes it *after* the guard: `onLand` after the 3.2 m/s speed gate,
   `onFootstep` after the `rng.float() > 0.55` coin flip, `onImpact` between
   the `!point` and `!normal` checks. `on_land`/`on_footstep`/`on_actor_death`/
   `tracer`/`explosion` now take `now` and write it where the source does. The
   footstep case is observable and pinned: a suppressed footstep spends a draw
   and leaves the clock alone.
5. **`this._probes` is a `Float32Array`.** `_findTarget` stores every probe
   hit point, normal, distance and cost through single precision and reads the
   rounded values back for the scoring, the planarity test and the span
   measurement. Written as `f64` this moves the chosen hit point in the eighth
   significant digit — the golden's `distance: 6.0169148445129395` is an f32
   value, and the `1e-9` pin catches the difference. This is the
   `Float32Array`-storage-width trap, and it applies in exactly this one place
   in the file.

## The event-payload vocabulary

The brief said `fx/system.rs` "already declares payload types". **It did
not** — it declared `DecalOpts`, `SmokeColumnOpts` and `FxStats`, none of which
is an `EventBus` payload. The crate's real state is that
`crate::audio::system` and `crate::ui::system` have each declared their own
set for the same six event names, and `ui/system.rs` carries a note saying so.

FX is now the third. Its payloads (`BulletImpact`, `WeaponFire`,
`WeaponShell`, `PlayerFootstep`) are declared in `fx/system.rs` with the same
integration note, listing exactly what neither of the other two carries:

| event | field FX needs that audio/ui lack |
|---|---|
| `bullet:impact` | `normal`, `incident` — without the normal there is no spray direction at all |
| `weapon:fire` | `origin`, `dir`, `intensity`, `light`, `flashScale`, the `fx === false` suppression flag |
| `weapon:shell` | `velocity` |
| `explosion` | `damage` (unused by `explode`, so not modelled) |
| `actor:death`, `player:land`, `player:footstep` | nothing — audio's types would serve |

Converging the three into one superset per event is a whole-game decision for
the integration pass, not this slice.

## The golden — the real `FxSystem`, really constructed

Same technique as `tests/sky_system/`: the capture imports the original
`FxSystem` and runs the real `async init(ctx)` against a stub WebGL surface.
The atlases really bake, the rings and the shell system are really built, the
real `Ambience` is really constructed, and `this.rng = ctx.rng.fork()` really
runs off `src/core/rng.js`.

Two stubs need justifying:

- **Physics is two analytic planes**, not a fabricated hit record: a 12 m wall
  at `z = -6` and a 12 cm "pillar" at `z = -3.5` — exactly the case
  `_findTarget`'s planarity scoring exists to beat. The same intersection is
  implemented in the Rust test, and **the plane definitions are read out of the
  golden**, so the two halves cannot drift on the geometry. (The recipe's
  warning is about a stub that *returns* the answer; this one *computes* it
  from a shape both sides declare.)
- **Cameras are real `THREE.PerspectiveCamera`s**, posed and
  `updateMatrixWorld`ed, and their matrices are written into the golden so the
  Rust side is fed byte-identical inputs.

### The sharpest instrument: `rngNext`

Every block that spends RNG ends by recording `fx.rng.float()`, compared with
**no tolerance at all** (the generator is integer arithmetic, so it is
bit-reproducible). One extra, missing or reordered draw anywhere in the ported
call graph moves it completely. It caught the first real bug in the harness
itself: the test was passing `12345` straight to `FxSystem::new`, skipping the
`ctx.rng.fork()` at `index.js:42` — `FxSystem::new` takes the *already-forked*
seed, so the test must do `Rng::new(12345).u32()`.

`debug_burst` then runs 12 configurations (6 kinds x with/without physics)
through two full loop periods at 60 Hz and compares the per-layer spawn
counters, `stats`, and `rngNext`. That single test exercises `find_target`
(f32 probes and all), `stage_wall_hits`'s stage-time draws, `impact_at`'s
fire-time draws, all four `_stage*` camera transforms, `muzzle_flash`'s view
conversion, and the `_runScript` wrap — end to end, against the original.

## Transcription details worth keeping

- **`_stageWallHits` draws at STAGE time; `debugBurst`'s `'combat'` arm draws
  at FIRE time.** `_stageWallHits` computes `u`/`v` outside the closure
  (`index.js:996-997`); `'combat'` calls `rng.signed()`/`rng.range()` *inside*
  it (`index.js:953-955`). `StageAction` has two variants (`Impact` and
  `ImpactRandom`) for exactly this reason — collapsing them would move three
  draws per loop and desync everything after.
- **`_syncLighting` calls `.transformDirection(m).normalize()`** — a second
  normalize of an already-unit vector. Not a no-op in floating point; both
  calls are kept.
- **`_findTarget`'s two coplanarity bands differ.** The scoring loop uses the
  exclusive `off > -0.12 && off < 0.12` (`index.js:1167`); the span loop twenty
  lines later uses the inclusive `off < -0.12 || off > 0.12` (`index.js:1226`).
  Both transcribed as written.
- **`_stageShell`'s velocity is `applyMatrix4(...).sub(cam.position)`**, not
  `transformDirection` — a point transform with the translation cancelled, so
  no renormalisation happens.
- **`weaponKey`'s table order is load-bearing.** `for (const name in
  MUZZLE_LIGHT)` returns the first key the lowercased input *contains*, so
  `"sniper rifle"` resolves to `rifle` (declared first), not `sniper`, and
  `"suppressed smg"` resolves to `smg`. Stored as an array, not a map.
- **`explode` never reads `o.damage`,** so `debugBurst('explosion')`'s
  `{ damage: 120 }` is dropped rather than carried as a field nothing consumes.

## Still not ported

- `ambience.js` (outside the port's file list). `add_smoke_column`/
  `add_smoke_source` remain no-ops and `update` computes `sun_factor` without
  a consumer. **This is the one place this port's RNG stream can diverge from
  the real game's**, if `Ambience`'s constructor draws. The capture constructs
  the real `Ambience`, so the golden reflects the truth; the Rust side does not
  yet. `FxSystem::new` names the insertion point.
- `audioPing` (`index.js:644-650`) — a two-branch dispatch into the audio
  subsystem (`a.playShell(pos, gain)` else `a.play('shell', pos, gain)`),
  called only from `shells.js:176`. Neither `fx/shells.rs` nor this file models
  the audio seam; it wants an `FxAudio` trait alongside `FxWorld`, which is a
  cross-slice decision.

## Cost

`tests/fx_system_port.rs` takes ~100 s in a debug build, almost entirely
`FxSystem::new`'s 1024 px atlas bake, which the `high` preset forces and which
runs ~50 times across the file. Nothing in these tests reads the baked pixels;
a future `FxSystem::with_atlases(...)` constructor would take it to seconds
without weakening a single assertion.

## Wiring

None. `fx/system.rs` is already in `fx/mod.rs`. `fx/mod.rs`'s module doc should
eventually drop its "What is not ported: the render seam" claim for `system` —
the remaining gap there is three items, listed at the top of `system.rs`.
