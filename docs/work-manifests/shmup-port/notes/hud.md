# Slice 1 — the HUD

*Wiring wave (`docs/work-manifests/shmup-port/13-wiring-wave.md`).*

## What this slice did

**`ui::system::UiCore` is now constructed, subscribed, stepped and — on
`wasm32` — painted.** Before this slice it had zero references outside
`ui/system.rs`, and so did all eleven `view` modules. 8,540 lines of ported HUD
reached no pixel.

| file | what changed |
|---|---|
| `apps/shmup/src/scene/wiring/hud.rs` | **new.** `HudRig` — the constructor, the seven subscriptions, the four `ctx` seams, the frame drive, the DOM mount, and the `wasm32` `HudViews` bundle that owns every widget's view. |
| `apps/shmup/src/scene/wiring/mod.rs` | `pub mod hud;` + a row in the table. |
| `apps/shmup/src/ui/mod.rs` | **`Hud` deleted** — the model-only second port of `index.js`, with `HudFrame`, `FramePull`, `CameraBasis`, `movement_bloom`, `camera_heading_deg`, `compass_bearing` and its tests. What is left is the shared vocabulary (`Blip`, `WeaponPull`, `PlayerPull`, `HudState`) plus the module docs, rewritten to say where the facade lives. 603 lines → 224. |
| `apps/shmup/src/ui/system.rs` | `UiSystem::wire_events_on(&EventBus)` (the same seven subscriptions without a `Ctx`, which `wire_events` now delegates to), and `UiCore::ammo_input()` — hoisted out of `late_update` so the facade and the `AmmoView` read one definition of the four text fields the numeric `AmmoFrame` does not carry. |
| `apps/shmup/src/ui/minimap.rs` | **new `view` module** — `MinimapView`, the `DrawOp` → `CanvasRenderingContext2D` realiser. The widget was ported as a display list with no interpreter; this is the interpreter. |

`ax refs UiCore` now returns `scene/wiring/hud.rs` alongside its own file.

## How the views are driven, and the finding underneath it

Six widget views are frame-driven (`apply(&SomethingFrame)`). Five —
`hitmarkers`, `damage`, `killfeed`, `markers`, `compass` — are **self-driving**:
each owns a second copy of the widget's pure core parameterised over its DOM
node type (`Hitmarkers<HitNode>` where `UiCore` holds `Hitmarkers<()>`), and
exposes `spawn`/`push`/`update(dt)` instead of an `apply`.

`UiEffect` is the channel the port authored for exactly this — its own doc says
the journal carries "the three things a numeric widget frame cannot: the
killfeed row's names, the banner's strings, and the damage number's value and
kind — all of which the `wasm32` view writes as text" — and every pooled effect
carries the `slot` the facade used. So `HudViews::apply` replays the journal's
spawns and then steps each self-driving view with the same `dt`.

That is correct today (the spawns all originate in event handlers and API calls,
which run *before* `late_update`, so replaying them before the view's `update`
reproduces the facade's own ordering; `Pool::acquire` is oldest-first and
deterministic, so slot `i` is slot `i` on both sides — pinned by
`journal_slots_walk_the_pool_in_acquire_order`). It is still **two state
machines where one would do**, and it costs a second `WorldMarkers` RNG fork
that `HudRig::new` has to reproduce by cloning the incoming stream.

**Recommended follow-up, not done here:** give the five self-driving views an
`apply(slot, &Frame)` + `show(slot, spec)` / `hide(slot)` shape and drive them
from `UiFrame`'s `Vec<(usize, …)>` lists, deleting their embedded cores. That
changes five `view` modules' public surface, which is reshaping rather than
connecting — out of scope for a wave that explicitly forbids re-porting, and
riskier than it looks in a wave that also forbids compiling.

## What is wired

* The overlay root and its four stacking layers, in `index.js:72-79`'s order,
  under `document.getElementById('ui') ?? document.body`.
* `style.css.tpl` + the SVG `<defs>`, through `style::install::install_styles`.
* All eleven widgets constructed in `index.js:81-93`'s order onto the layers the
  source parents them to (health's overlays on `hurt`, markers on `world`, arcs
  / crosshair / hitmarkers on `centre`, minimap / compass / match bar /
  killfeed / ammo / prompt / banner on `chrome`, the menu on the root).
* The seven event subscriptions (`weapon:fire`, `weapon:reload`,
  `damage:dealt`, `damage:taken`, `actor:death`, `explosion`, `player:state`).
* The four `ctx` seams, per frame: `ctx.time` (`set_clock`), `ctx.camera`
  (`set_camera`, built from the frame's `CameraPose` via the existing
  `scene::wiring::ai::camera_state` rather than a second composition),
  `ctx.input` (`set_input`), and `ctx.peek('weapons'/'player'/'ai')`
  (`set_links`).
* The AI actors as compass/minimap blips — `AiWiring::actor_poses()` mapped to
  `HudActor`; the dead are dropped by the facade.
* `resize` → the facade's `k`, the `--k` custom property, the compass's and
  crosshair's cached scale, and the minimap's backing store.
* The minimap bake gate (`UiEffect::MinimapTryBake`) and the draw
  (`UiEffect::MinimapDraw`), executed against a real 2D context.
* `sfx(id, gain)` collected into `HudRig::take_sfx()` for the audio slice.

## What I could not wire, and why

1. **The minimap's *map*.** `Minimap::try_bake` is called on the source's
   schedule with `layout: None`, so the widget draws its procedural plate (base,
   10 m grid, view cone, blips, objectives) and no building footprints.
   `Minimap::build_vector_map` needs a `LayoutSource` — `world.buildings`,
   `levelToWorld`, `isOpen` — and `crate::world::system::WorldSystem` satisfies
   all three *exactly*. The blocker is one tier up: `scene::level::build_level`
   consumes `WorldSystem::init(root)`, copies out its products and **drops the
   system**, so nothing reachable from a running `Game` can hand one over.
   `scene/level.rs` is not this slice's file.

   **Fix (one field):** keep the system on `Level` (`pub world_system:
   WorldSystem`, or just the three queries), `impl LayoutSource for WorldSystem`
   in `scene/wiring/hud.rs`, and pass `Some(&level.world_system)` into
   `run_bake_gate`. That is a whole-map upgrade for about fifteen lines, and it
   should be someone's next task.

   The depth-bake *fallback* additionally needs an orthographic depth readback
   the engine does not expose; it is the fallback, so it does not matter until
   the vector path is wired.

2. **`MenuHost`.** `UiCore::set_menu_host` is still uncalled, so the pause menu
   does not freeze `time.scale`, disable player control, push the FOV into the
   camera, or release/re-request pointer lock. Those four effects need `&mut
   Game`, and `MenuHost` is installed as a `Box<dyn MenuHost>` (`'static`).
   `Game` already owns `paused`/`control_enabled` itself, so installing a host
   that *also* decided them would be a second decision-maker — exactly what this
   wave is removing. The honest shape is a shared
   `Rc<RefCell<PauseEffects>>` cell the rig publishes and `Game` applies; it is
   a small, separate change and it is not made here. **Consequence today:
   closing the menu does not re-request pointer lock.**

3. **The menu is display-only.** `MenuView` builds the preset segments, the two
   sliders and the buttons but binds no listeners (that is how it was ported).
   `HudRig::sync_menu(quality_index, invert_y)` highlights the live settings;
   clicking does nothing. Not a regression — nothing was clickable before
   either, because nothing was mounted.

4. **`friendly` is hard `false` for every blip.** `ActorPose` publishes no team,
   and every actor this level spawns is hostile. One line in `hud_actor` when a
   friendly garrison lands.

5. **The page is not full-viewport.** `.ow-hud` is `position: fixed; inset: 0`
   — the source's page is a `100vw × 100vh` canvas plus an empty
   `<div id="ui">`. `apps/shmup/web/index.html` is a letterboxed
   `min(94vw, 1280px)` canvas inside a `<main>` with a heading and two
   paragraphs, so a mounted HUD would centre its crosshair on the *viewport*,
   not on the canvas. The page change is in the report; I did not make it
   (`web/index.html` is outside this slice's write set).

6. **Two `DrawOp` arms are inert** in `MinimapView`: `DrawBaked` (there is no
   bake — see 1) and `ImageSmoothingQuality` (no `web-sys` 0.3.99 binding, and
   it is only ever emitted immediately before `DrawBaked`). Both are commented
   at the site.

## Things found already broken

* **`ui::Hud` and `ui::system::UiCore` were two ports of one file** — the same
  `index.js`, sharing four value types but owning separate copies of all eleven
  widgets and separate frame drives. `scene/wiring/look.rs:91-110` had already
  diagnosed this and written the verdict ("`UiSystem`/`UiCore` survives, `Hud`
  is deleted") without carrying it out. Now carried out.
* **`ui/mod.rs`'s own doc comment described the deletion as future work**
  ("wiring `Hud` behind a real `Subsystem` impl … is a thin adapter, not a
  redesign") while `UiCore` — the thing that adapter would have produced — was
  already sitting in the next file over, finished.
* **`minimap.rs` had no realiser at all.** The module docs say the output "*is*
  a sequence of canvas2d calls" and leaves "rasterisation … to the view"; there
  was no view, and nothing in `apps/shmup` referenced
  `CanvasRenderingContext2d`. The display list had been complete and
  uninterpretable for as long as it had existed.
* **`AmmoInput` was built inline inside `late_update`** and the `AmmoView`'s
  `apply` needs the same value, so mounting the view would have meant a second
  hand-built copy. Hoisted to `UiCore::ammo_input()` instead.
* **`Game::camera_basis()` was dead the moment `UiCore` ran.** `UiCore`
  recomputes the XZ basis from `camera.matrix_world` itself, the way the source
  does; the hand-rolled yaw version in `game.rs` had one consumer (`Hud`) and
  goes with it.
